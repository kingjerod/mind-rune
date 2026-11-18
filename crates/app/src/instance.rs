//! One running copy per user. The first copy holds a lock file in the user's runtime folder
//! and listens on a Unix socket beside it; a copy launched after it tells it what was asked
//! (show the window, perhaps with a folder, or quit) and ends.

use std::ffi::OsString;
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// What a copy launched later asks of the running one. Its `Debug` never shows the token.
#[derive(Clone, PartialEq, Eq)]
pub enum Ask {
    /// Show the window; with a folder, open a new session there.
    Show {
        token: Option<String>,
        folder: Option<PathBuf>,
    },
    Quit,
}

impl fmt::Debug for Ask {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Ask::Show { token, folder } => f
                .debug_struct("Show")
                .field("token", &token.as_ref().map(|_| "hidden"))
                .field("folder", folder)
                .finish(),
            Ask::Quit => f.write_str("Quit"),
        }
    }
}

/// What the running copy says back to an ask: on the wire, `ok` or `asked`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Told {
    /// The ask was carried out.
    Ok,
    /// A quit was asked for and something is running: the app shows its question instead.
    Asked,
}

/// What claiming the folder found.
pub enum Claim {
    /// This is the first copy: it serves the ones launched after it.
    First(Listener),
    /// Another copy runs: it is told.
    Second,
    /// There is no way to be one copy here: this one runs alone.
    Alone,
}

/// How long a copy launched later waits for the running one, and how long the running one
/// waits for a line.
pub const ANSWER_WAIT: Duration = Duration::from_secs(2);

/// How long the running copy waits for the app's word on an ask before it answers `ok`
/// without it. Shorter than [`ANSWER_WAIT`], so that this answer still reaches the copy that
/// asked, whose own wait began before the line was read.
pub const DECIDE_WAIT: Duration = Duration::from_secs(1);

/// The longest line the running copy reads, its line break included.
pub const LINE_LIMIT: usize = 4096;

/// The longest activation token passed on, in bytes. The window system's own messages are
/// short (4,096 bytes on Wayland, the token's request included); no desktop gives a token
/// near this long.
pub const TOKEN_LIMIT: usize = 512;

/// The file the first copy holds locked, with its process id in it.
const LOCK: &str = "lock";

/// `$XDG_RUNTIME_DIR/mindrune`, else `run` beside the app's own saved file. A run moved
/// under `RUNE_DIRS_ROOT` uses `run` in its own folder there, so it never finds the real
/// running copy. `None` when there is no folder that is only the user's.
pub fn folder() -> Option<PathBuf> {
    folder_from(|key| std::env::var_os(key))
}

/// [`folder`] with the environment looked up through `var`. The saved file is where
/// `Prefs::open` keeps it.
fn folder_from(var: impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    let set = |key: &str| {
        var(key)
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
    };
    let own = || rune::dirs::AppDirs::from_env("mindrune", &var).map(|d| d.config().join("run"));
    // Rune moves the app's folders under any root that is set, relative or not.
    if var(rune::dirs::ROOT_ENV).is_some_and(|root| !root.is_empty()) {
        return own();
    }
    if let Some(runtime) = set("XDG_RUNTIME_DIR") {
        return Some(runtime.join("mindrune"));
    }
    match set("RUNE_PREFS_DIR") {
        Some(dir) => Some(dir.join("mindrune").join("run")),
        None => own(),
    }
}

/// Whether `value` can be an activation token: printable ASCII, nothing else, and no longer
/// than [`TOKEN_LIMIT`]. It is handed to the window system, which takes no other.
pub fn is_token(value: &str) -> bool {
    !value.is_empty() && value.len() <= TOKEN_LIMIT && value.bytes().all(|b| b.is_ascii_graphic())
}

/// The path of the lock file in `dir`, to name in a message.
pub fn lock_path(dir: &Path) -> PathBuf {
    dir.join(LOCK)
}

/// One line of JSON, no line break inside. A folder that is not valid UTF-8 is left out.
pub fn encode(ask: &Ask) -> String {
    let mut line = serde_json::Map::new();
    match ask {
        Ask::Show { token, folder } => {
            line.insert("ask".into(), "show".into());
            if let Some(token) = token {
                line.insert("token".into(), token.as_str().into());
            }
            if let Some(folder) = folder.as_deref().and_then(Path::to_str) {
                line.insert("folder".into(), folder.into());
            }
        }
        Ask::Quit => {
            line.insert("ask".into(), "quit".into());
        }
    }
    serde_json::Value::Object(line).to_string()
}

/// The ask in `line`, which is one JSON object with nothing in it but what [`encode`] writes:
/// an `ask` of `show`, with a [token](is_token) and an absolute `folder` when there are any,
/// or of `quit`.
pub fn decode(line: &str) -> Option<Ask> {
    let serde_json::Value::Object(mut fields) = serde_json::from_str(line).ok()? else {
        return None;
    };
    let ask = fields.remove("ask")?;
    let mut text = |key: &str| match fields.remove(key) {
        None => Some(None),
        Some(serde_json::Value::String(value)) => Some(Some(value)),
        Some(_) => None,
    };
    let ask = match ask.as_str()? {
        "show" => {
            let token = text("token")?;
            if token.as_deref().is_some_and(|token| !is_token(token)) {
                return None;
            }
            let folder = text("folder")?;
            if folder
                .as_deref()
                .is_some_and(|folder| folder.contains('\0'))
            {
                return None;
            }
            let folder = folder.map(PathBuf::from);
            if folder.as_ref().is_some_and(|folder| !folder.is_absolute()) {
                return None;
            }
            Ask::Show { token, folder }
        }
        "quit" => Ask::Quit,
        _ => return None,
    };
    fields.is_empty().then_some(ask)
}

#[cfg(unix)]
pub use unix::{Listener, claim, quit, tell};

#[cfg(unix)]
mod unix {
    use super::*;
    use std::fs::{self, DirBuilder, File, OpenOptions, TryLockError};
    use std::io::{Read, Write};
    use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::sync::mpsc::Receiver;
    use std::thread;
    use std::time::Instant;

    /// The socket the first copy listens on.
    pub(super) const SOCK: &str = "sock";

    /// The first copy's socket, and its lock, held for the life of the process.
    pub struct Listener {
        socket: UnixListener,
        lock: File,
    }

    /// Makes `dir` (only the user's) and takes its lock: the first copy binds the socket in
    /// it, a later one finds the lock taken. The first copy's two files get the sticky bit
    /// ([`keep`]).
    pub fn claim(dir: &Path) -> io::Result<Claim> {
        DirBuilder::new().recursive(true).mode(0o700).create(dir)?;
        fs::set_permissions(dir, fs::Permissions::from_mode(0o700))?;
        let mut lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .open(dir.join(LOCK))?;
        match lock.try_lock() {
            Ok(()) => {}
            Err(TryLockError::WouldBlock) => return Ok(Claim::Second),
            Err(TryLockError::Error(error)) => return Err(error),
        }
        let sock = dir.join(SOCK);
        match fs::remove_file(&sock) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error),
            _ => {}
        }
        let socket = UnixListener::bind(&sock)?;
        lock.set_len(0)?;
        write!(lock, "{}", std::process::id())?;
        keep(&dir.join(LOCK));
        keep(&sock);
        Ok(Claim::First(Listener { socket, lock }))
    }

    /// Sets the sticky bit on `path`, leaving its other bits. A file in the user's runtime
    /// folder may be cleaned up while the app runs unless it has that bit or is touched every
    /// six hours (the XDG rule), and a launch after that would not find this copy. Where the
    /// bit cannot be set, the copy runs as it did without it.
    fn keep(path: &Path) {
        if let Ok(found) = fs::metadata(path) {
            let mode = found.permissions().mode() | 0o1000;
            let _ = fs::set_permissions(path, fs::Permissions::from_mode(mode));
        }
    }

    /// Tells the running copy, and waits at most `ANSWER_WAIT` for its answer: that the ask
    /// was carried out, or that the app asks the user first. Any other answer is an error.
    pub fn tell(dir: &Path, ask: &Ask) -> io::Result<Told> {
        let deadline = Instant::now() + ANSWER_WAIT;
        // A first copy holds the lock a moment before its socket is there.
        let mut stream = loop {
            match UnixStream::connect(dir.join(SOCK)) {
                Ok(stream) => break stream,
                Err(error) if Instant::now() >= deadline => return Err(error),
                Err(_) => thread::sleep(Duration::from_millis(20)),
            }
        };
        stream.set_write_timeout(Some(left(deadline)?))?;
        stream.write_all(format!("{}\n", encode(ask)).as_bytes())?;
        match read_line(&mut stream, deadline)?.as_deref() {
            Some(b"ok") => Ok(Told::Ok),
            Some(b"asked") => Ok(Told::Asked),
            _ => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "the running copy did not say ok",
            )),
        }
    }

    /// Asks the running copy to quit, as [`tell`] does. A quit that is carried out can end
    /// the running copy before its answer is written: so when no answer comes, the copy that
    /// has [gone](gone) within what is left of `ANSWER_WAIT` counts as one that quit.
    pub fn quit(dir: &Path) -> io::Result<Told> {
        let deadline = Instant::now() + ANSWER_WAIT;
        tell(dir, &Ask::Quit).or_else(|error| {
            if gone(dir, deadline) {
                Ok(Told::Ok)
            } else {
                Err(error)
            }
        })
    }

    /// Whether no copy holds the lock in `dir` any more, looked at until `deadline` and once
    /// at least. The lock is tried and given back at once; nothing is made: no file, no
    /// socket. A lock file that is not there is held by nobody. When it cannot be told, the
    /// answer is no.
    fn gone(dir: &Path, deadline: Instant) -> bool {
        loop {
            let lock = match File::open(dir.join(LOCK)) {
                Ok(lock) => lock,
                Err(error) => return error.kind() == io::ErrorKind::NotFound,
            };
            match lock.try_lock() {
                Ok(()) => return true,
                Err(TryLockError::WouldBlock) if Instant::now() < deadline => {
                    thread::sleep(Duration::from_millis(20));
                }
                Err(_) => return false,
            }
        }
    }

    impl Listener {
        /// Serves on a thread named `mindrune-instance` until the process ends. `on_ask` hands
        /// each ask on. For one the app has a word to say about, it gives back where that word
        /// will come: the listener waits for it, at most [`DECIDE_WAIT`], before it answers.
        pub fn serve(self, on_ask: impl Fn(Ask) -> Option<Receiver<Told>> + Send + 'static) {
            thread::Builder::new()
                .name("mindrune-instance".into())
                .spawn(move || {
                    let _lock = self.lock;
                    for stream in self.socket.incoming() {
                        match stream {
                            Ok(stream) => answer(stream, &on_ask),
                            // Kept from spinning while no connection can be taken.
                            Err(_) => thread::sleep(Duration::from_millis(100)),
                        }
                    }
                })
                .expect("the listener's thread starts");
        }
    }

    /// Takes one ask from `stream` and answers it: "asked" when the app says in time that it
    /// asks the user first, else "ok". Anything that is not an ask closes it unanswered.
    fn answer(mut stream: UnixStream, on_ask: &impl Fn(Ask) -> Option<Receiver<Told>>) {
        let deadline = Instant::now() + ANSWER_WAIT;
        let Ok(Some(line)) = read_line(&mut stream, deadline) else {
            return;
        };
        let Some(ask) = std::str::from_utf8(&line).ok().and_then(decode) else {
            return;
        };
        let told = on_ask(ask)
            .and_then(|word| {
                let wait = left(deadline).ok()?.min(DECIDE_WAIT);
                word.recv_timeout(wait).ok()
            })
            .unwrap_or(Told::Ok);
        if let Ok(left) = left(deadline) {
            let _ = stream.set_write_timeout(Some(left));
            let _ = stream.write_all(match told {
                Told::Ok => b"ok\n",
                Told::Asked => b"asked\n",
            });
        }
    }

    /// The bytes before the first line break, read until `deadline`; `None` when the stream
    /// ends first or the line is longer than [`LINE_LIMIT`] with its break.
    fn read_line(stream: &mut UnixStream, deadline: Instant) -> io::Result<Option<Vec<u8>>> {
        let mut line = Vec::new();
        let mut chunk = [0; 512];
        loop {
            stream.set_read_timeout(Some(left(deadline)?))?;
            let read = stream.read(&mut chunk)?;
            if read == 0 {
                return Ok(None);
            }
            if let Some(end) = chunk[..read].iter().position(|&b| b == b'\n') {
                line.extend_from_slice(&chunk[..end]);
                return Ok((line.len() < LINE_LIMIT).then_some(line));
            }
            line.extend_from_slice(&chunk[..read]);
            if line.len() >= LINE_LIMIT {
                return Ok(None);
            }
        }
    }

    /// The time until `deadline`; an error once it has passed.
    fn left(deadline: Instant) -> io::Result<Duration> {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(io::ErrorKind::TimedOut.into());
        }
        Ok(left)
    }
}

/// Without Unix sockets every copy runs alone.
#[cfg(not(unix))]
pub use other::{Listener, claim, quit, tell};

#[cfg(not(unix))]
mod other {
    use super::*;

    /// Never made: no copy is first here.
    pub enum Listener {}

    pub fn claim(_dir: &Path) -> io::Result<Claim> {
        Ok(Claim::Alone)
    }

    pub fn tell(_dir: &Path, _ask: &Ask) -> io::Result<Told> {
        Err(io::ErrorKind::Unsupported.into())
    }

    pub fn quit(_dir: &Path) -> io::Result<Told> {
        Err(io::ErrorKind::Unsupported.into())
    }

    impl Listener {
        pub fn serve(
            self,
            _on_ask: impl Fn(Ask) -> Option<std::sync::mpsc::Receiver<Told>> + Send + 'static,
        ) {
            match self {}
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::unix::SOCK;
    use super::*;
    use std::fs::{self, File};
    use std::io::{Read, Write};
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::net::UnixStream;
    use std::sync::mpsc;
    use std::thread;
    use std::time::Instant;

    /// Serves `listener`, passing on every ask it takes; each is carried out at once.
    fn serving(listener: Listener) -> mpsc::Receiver<Ask> {
        let (sent, received) = mpsc::channel();
        listener.serve(move |ask| {
            sent.send(ask).unwrap();
            None
        });
        received
    }

    fn first(dir: &Path) -> Listener {
        match claim(dir).unwrap() {
            Claim::First(listener) => listener,
            Claim::Second => panic!("second"),
            Claim::Alone => panic!("alone"),
        }
    }

    fn show(token: &str, folder: &str) -> Ask {
        Ask::Show {
            token: Some(token.into()),
            folder: Some(folder.into()),
        }
    }

    #[test]
    fn the_first_claims_and_the_second_is_told_so() {
        let dir = tempfile::tempdir().unwrap();
        let dir = dir.path().join("mindrune");
        let received = serving(first(&dir));
        assert!(matches!(claim(&dir).unwrap(), Claim::Second));
        let pid = fs::read_to_string(dir.join(LOCK)).unwrap();
        assert_eq!(pid, std::process::id().to_string());

        let ask = Ask::Show {
            token: None,
            folder: None,
        };
        assert_eq!(tell(&dir, &ask).unwrap(), Told::Ok);
        assert_eq!(received.recv_timeout(ANSWER_WAIT).unwrap(), ask);
    }

    #[test]
    fn a_quit_the_app_asks_about_first_is_answered_asked() {
        let dir = tempfile::tempdir().unwrap();
        // The app's word comes a moment after the ask was handed on, as from its own thread.
        first(dir.path()).serve(|ask| {
            let (say, word) = mpsc::channel();
            thread::spawn(move || {
                thread::sleep(Duration::from_millis(100));
                let told = if ask == Ask::Quit {
                    Told::Asked
                } else {
                    Told::Ok
                };
                let _ = say.send(told);
            });
            Some(word)
        });
        assert_eq!(tell(dir.path(), &Ask::Quit).unwrap(), Told::Asked);
        let show = Ask::Show {
            token: None,
            folder: None,
        };
        assert_eq!(tell(dir.path(), &show).unwrap(), Told::Ok);
    }

    #[test]
    fn an_app_that_does_not_say_in_time_is_answered_ok() {
        let dir = tempfile::tempdir().unwrap();
        // Each ask's sender is kept and never used: no word comes, and none is given up on.
        let kept = std::sync::Mutex::new(Vec::new());
        first(dir.path()).serve(move |ask| {
            let (say, word) = mpsc::channel();
            match ask {
                Ask::Quit => kept.lock().unwrap().push(say),
                // Dropped at once: there will be no word, and nothing to wait for.
                Ask::Show { .. } => drop(say),
            }
            Some(word)
        });
        let started = Instant::now();
        assert_eq!(tell(dir.path(), &Ask::Quit).unwrap(), Told::Ok);
        let waited = started.elapsed();
        assert!(waited >= DECIDE_WAIT, "it waited for the word: {waited:?}");
        assert!(waited < ANSWER_WAIT, "and answered in time: {waited:?}");
        let show = Ask::Show {
            token: None,
            folder: None,
        };
        let started = Instant::now();
        assert_eq!(tell(dir.path(), &show).unwrap(), Told::Ok);
        assert!(started.elapsed() < DECIDE_WAIT, "no wait without a sender");
    }

    #[test]
    fn any_other_answer_than_ok_or_asked_is_an_error() {
        use std::os::unix::net::UnixListener;
        for (answer, told) in [
            ("ok\n", Some(Told::Ok)),
            ("asked\n", Some(Told::Asked)),
            ("yes\n", None),
            ("asked \n", None),
            ("OK\n", None),
            ("\n", None),
            ("ok", None),
        ] {
            let dir = tempfile::tempdir().unwrap();
            let socket = UnixListener::bind(dir.path().join(SOCK)).unwrap();
            let peer = thread::spawn(move || {
                let (mut stream, _) = socket.accept().unwrap();
                let mut line = [0; 64];
                let _ = stream.read(&mut line);
                stream.write_all(answer.as_bytes()).unwrap();
            });
            assert_eq!(tell(dir.path(), &Ask::Quit).ok(), told, "{answer:?}");
            peer.join().unwrap();
        }
    }

    /// A running copy, made by hand in `dir`: it holds the lock and takes one ask, which it
    /// never answers. It closes the connection at once and lets go of the lock when `gone`
    /// says so, as a copy that has quit does, or holds it until the test ends.
    fn unanswering(dir: &Path, gone: bool) -> (thread::JoinHandle<()>, mpsc::Sender<()>) {
        use std::os::unix::net::UnixListener;
        let held = File::create(dir.join(LOCK)).unwrap();
        held.try_lock().unwrap();
        let socket = UnixListener::bind(dir.join(SOCK)).unwrap();
        let (end, ended) = mpsc::channel::<()>();
        let copy = thread::spawn(move || {
            let (mut stream, _) = socket.accept().unwrap();
            let mut line = [0; 64];
            let _ = stream.read(&mut line);
            drop(stream);
            if gone {
                // The process ends a moment after its connection closed.
                thread::sleep(Duration::from_millis(150));
                drop(held);
            } else {
                let _ = ended.recv();
                drop(held);
            }
        });
        (copy, end)
    }

    /// The names of the files in `dir`, sorted.
    fn files(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn a_quit_whose_answer_never_came_but_whose_copy_is_gone_is_a_success() {
        let dir = tempfile::tempdir().unwrap();
        let (copy, _end) = unanswering(dir.path(), true);
        let before = files(dir.path());
        let started = Instant::now();
        assert_eq!(quit(dir.path()).ok(), Some(Told::Ok));
        assert!(started.elapsed() < ANSWER_WAIT + Duration::from_millis(500));
        copy.join().unwrap();
        assert_eq!(files(dir.path()), before, "nothing was left behind");
        // The lock is free again: looking took it and gave it back.
        let lock = File::open(dir.path().join(LOCK)).unwrap();
        lock.try_lock().expect("nothing holds the lock");
    }

    #[test]
    fn a_quit_whose_answer_never_came_and_whose_copy_still_runs_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let (copy, end) = unanswering(dir.path(), false);
        let started = Instant::now();
        assert!(quit(dir.path()).is_err());
        let waited = started.elapsed();
        assert!(
            waited < ANSWER_WAIT + Duration::from_millis(500),
            "no longer than a launch may wait: {waited:?}"
        );
        end.send(()).unwrap();
        copy.join().unwrap();
        // A show that is not answered is an error whether or not the copy has gone.
        let dir = tempfile::tempdir().unwrap();
        let (copy, _end) = unanswering(dir.path(), true);
        let show = Ask::Show {
            token: None,
            folder: None,
        };
        assert!(tell(dir.path(), &show).is_err());
        copy.join().unwrap();
    }

    #[test]
    fn a_copy_whose_lock_file_is_gone_has_gone_and_looking_makes_no_file() {
        let dir = tempfile::tempdir().unwrap();
        // No copy, no socket: the tell fails, and there is no lock for anything to hold.
        let started = Instant::now();
        assert_eq!(quit(dir.path()).ok(), Some(Told::Ok));
        assert!(started.elapsed() < ANSWER_WAIT + Duration::from_millis(500));
        assert_eq!(files(dir.path()), Vec::<String>::new());
    }

    #[test]
    fn the_lock_and_the_socket_are_kept_from_being_cleaned_up() {
        let dir = tempfile::tempdir().unwrap();
        let _first = first(dir.path());
        for file in [LOCK, SOCK] {
            let mode = fs::metadata(dir.path().join(file))
                .unwrap()
                .permissions()
                .mode();
            assert_ne!(mode & 0o1000, 0, "{file}: the sticky bit, of {mode:o}");
        }
        let lock = fs::metadata(dir.path().join(LOCK)).unwrap();
        assert_eq!(lock.permissions().mode() & 0o777, 0o600, "still the user's");
    }

    #[test]
    fn a_socket_left_by_a_copy_that_died_is_taken_over() {
        let dir = tempfile::tempdir().unwrap();
        drop(first(dir.path()));
        assert!(dir.path().join(SOCK).exists(), "left behind");

        let received = serving(first(dir.path()));
        tell(dir.path(), &Ask::Quit).unwrap();
        assert_eq!(received.recv_timeout(ANSWER_WAIT).unwrap(), Ask::Quit);
    }

    #[test]
    fn an_ask_goes_through() {
        let dir = tempfile::tempdir().unwrap();
        let received = serving(first(dir.path()));
        let ask = show("token-1", "/home/alex/my \"notes\"");
        tell(dir.path(), &ask).unwrap();
        assert_eq!(received.recv_timeout(ANSWER_WAIT).unwrap(), ask);
        tell(dir.path(), &Ask::Quit).unwrap();
        assert_eq!(received.recv_timeout(ANSWER_WAIT).unwrap(), Ask::Quit);
    }

    #[test]
    fn what_is_written_is_read_back() {
        let asks = [
            Ask::Show {
                token: None,
                folder: None,
            },
            Ask::Show {
                token: Some("token-1".into()),
                folder: None,
            },
            Ask::Show {
                token: None,
                folder: Some("/home/alex/a\nb".into()),
            },
            show("token-1", "/home/alex/my \"notes\""),
            Ask::Quit,
        ];
        for ask in asks {
            let line = encode(&ask);
            assert!(!line.contains('\n'), "{line}");
            assert_eq!(decode(&line), Some(ask));
        }
        assert_eq!(encode(&Ask::Quit), r#"{"ask":"quit"}"#);
        assert_eq!(
            encode(&Ask::Show {
                token: None,
                folder: None
            }),
            r#"{"ask":"show"}"#
        );
        for rubbish in [
            r#"{"ask":"run","cmd":"x"}"#,
            r#"{"ask":"show","cmd":"x"}"#,
            r#"{"ask":"quit","folder":"/home/alex"}"#,
            r#"{"ask":"show","folder":"notes"}"#,
            r#"{"ask":"show","token":1}"#,
            r#"{"ask":"show","token":""}"#,
            r#"{"ask":"show","token":"a\u0000b"}"#,
            r#"{"ask":"show","token":"a\u0007b"}"#,
            r#"{"ask":"show","token":"a b"}"#,
            r#"{"ask":"show","token":"é"}"#,
            r#"{"ask":"show","folder":"/home/alex/a\u0000b"}"#,
            "[]",
            "show",
            "",
        ] {
            assert_eq!(decode(rubbish), None, "{rubbish}");
        }
    }

    #[test]
    fn rubbish_and_a_line_without_end_are_dropped_and_the_listener_lives() {
        let dir = tempfile::tempdir().unwrap();
        let received = serving(first(dir.path()));
        let sock = dir.path().join(SOCK);

        let mut long = UnixStream::connect(&sock).unwrap();
        // The listener stops reading at its limit and closes: the rest may not be taken.
        let _ = long.write_all(&[b'x'; 10_000]);
        let mut answer = String::new();
        let _ = long.read_to_string(&mut answer);
        assert_eq!(answer, "");

        let mut hello = UnixStream::connect(&sock).unwrap();
        hello.write_all(b"hello\n").unwrap();
        let mut answer = String::new();
        hello.read_to_string(&mut answer).unwrap();
        assert_eq!(answer, "");

        tell(dir.path(), &Ask::Quit).unwrap();
        assert_eq!(received.recv_timeout(ANSWER_WAIT).unwrap(), Ask::Quit);
        assert!(received.try_recv().is_err(), "nothing else was taken");
    }

    #[test]
    fn a_line_that_is_not_text_is_dropped() {
        let dir = tempfile::tempdir().unwrap();
        let received = serving(first(dir.path()));
        let mut odd = UnixStream::connect(dir.path().join(SOCK)).unwrap();
        odd.write_all(b"{\"ask\":\"quit\"\xff}\n").unwrap();
        let mut answer = String::new();
        odd.read_to_string(&mut answer).unwrap();
        assert_eq!(answer, "");
        assert!(received.try_recv().is_err());
    }

    #[test]
    fn a_token_is_never_shown_in_debug() {
        let shown = format!("{:?}", show("token-secret", "/home/alex"));
        assert!(!shown.contains("token-secret"), "{shown}");
        assert!(shown.contains("/home/alex"), "{shown}");
        assert_eq!(format!("{:?}", Ask::Quit), "Quit");
    }

    #[test]
    fn only_a_printable_token_is_one() {
        assert!(is_token("1234_TIME5678"));
        for bad in ["", "a\0b", "a\nb", "a b", "é"] {
            assert!(!is_token(bad), "{bad:?}");
        }
    }

    #[test]
    fn a_token_longer_than_its_limit_is_refused() {
        assert_eq!(TOKEN_LIMIT, 512);
        assert!(is_token(&"t".repeat(TOKEN_LIMIT)));
        let long = "t".repeat(TOKEN_LIMIT + 1);
        assert!(!is_token(&long));
        let line = format!(r#"{{"ask":"show","token":"{long}"}}"#);
        assert!(line.len() < LINE_LIMIT, "short enough to be read");
        assert_eq!(decode(&line), None);
    }

    #[test]
    fn a_first_copy_that_does_not_answer_is_an_error_not_a_wait() {
        let dir = tempfile::tempdir().unwrap();
        let _silent = first(dir.path());
        let started = Instant::now();
        assert!(tell(dir.path(), &Ask::Quit).is_err());
        assert!(started.elapsed() < Duration::from_secs(3));
    }

    #[test]
    fn a_second_that_comes_before_the_socket_waits_for_it() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().to_path_buf();
        let held = File::create(path.join(LOCK)).unwrap();
        held.try_lock().unwrap();
        assert!(matches!(claim(&path).unwrap(), Claim::Second));
        let (sent, received) = mpsc::channel();
        let late = thread::spawn(move || {
            thread::sleep(Duration::from_millis(500));
            drop(held);
            first(&path).serve(move |ask| {
                sent.send(ask).unwrap();
                None
            });
        });
        tell(dir.path(), &Ask::Quit).unwrap();
        assert_eq!(received.recv_timeout(ANSWER_WAIT).unwrap(), Ask::Quit);
        late.join().unwrap();
    }

    #[test]
    fn the_folder_is_only_the_users() {
        let dir = tempfile::tempdir().unwrap();
        let dir = dir.path().join("mindrune");
        let _first = first(&dir);
        let mode = fs::metadata(&dir).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o700);
    }

    #[test]
    fn the_folder_is_the_moved_root_else_the_runtime_one_else_beside_the_saved_file() {
        fn env<'a>(pairs: &'a [(&str, &str)]) -> impl Fn(&str) -> Option<OsString> + 'a {
            move |key| {
                pairs
                    .iter()
                    .find(|(k, _)| *k == key)
                    .map(|(_, v)| OsString::from(v))
            }
        }
        let runtime = [
            ("XDG_RUNTIME_DIR", "/run/user/1000"),
            ("HOME", "/home/alex"),
        ];
        assert_eq!(
            folder_from(env(&runtime)),
            Some(PathBuf::from("/run/user/1000/mindrune"))
        );
        let moved = [
            ("RUNE_DIRS_ROOT", "/scratch/root"),
            ("XDG_RUNTIME_DIR", "/run/user/1000"),
            ("HOME", "/home/alex"),
        ];
        assert_eq!(
            folder_from(env(&moved)),
            Some(PathBuf::from("/scratch/root/config/mindrune/run"))
        );
        let home = [("XDG_RUNTIME_DIR", ""), ("HOME", "/home/alex")];
        let saved = rune::dirs::AppDirs::from_env("mindrune", env(&home)).unwrap();
        assert_eq!(folder_from(env(&home)), Some(saved.config().join("run")));
        let prefs = [("RUNE_PREFS_DIR", "/scratch"), ("HOME", "/home/alex")];
        assert_eq!(
            folder_from(env(&prefs)),
            Some(PathBuf::from("/scratch/mindrune/run"))
        );
        assert_eq!(folder_from(env(&[])), None, "no shared folder");
        assert_eq!(folder_from(env(&[("HOME", "")])), None);
    }
}
