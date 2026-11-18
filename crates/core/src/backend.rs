//! The threads between the window and the agent. They make the agent's blocking calls so the
//! window thread never does, and queue what comes back for the window to take with
//! `try_recv`.
//!
//! Every open has a thread of its own, which makes that open's `Agent::open` call and every
//! `send` for it, in order; it ends with its session. Every look has a thread of its own too.
//! One more thread, the library thread, makes the other calls one at a time in the order they
//! were sent: the account, the listing, a history, an activity time, an image read again. It
//! also reads the git branch of a folder. The preparing thread reads and checks each
//! attachment, in the order they were attached, and then makes the picture of each image a
//! history holds. So a slow call holds up only what was sent after it to the same thread. A
//! history no longer wanted is not read, answered or drawn.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet, VecDeque};
use std::panic::{self, AssertUnwindSafe};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, TryRecvError};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use mr_agent::{
    Account, Agent, AgentError, HistoryItem, ImageData, ModelOption, SessionCommand, SessionEvent,
    SessionHandle, SessionId, StartSpec, StoredHistory, StoredSession, UsageLimits,
};

use crate::attach::{self, Prepared, Problem, Source, Thumb};
use crate::git::{self, Branch};
use crate::sessions::SessionKey;

/// What the window asks of the backend. Mirrors the agent-facing `Effect`s.
#[derive(Clone, Debug, PartialEq)]
pub enum ToBackend {
    CheckAccount,
    Open {
        open: u64,
        spec: StartSpec,
    },
    Send {
        open: u64,
        command: SessionCommand,
    },
    ListSessions {
        since_unix: Option<i64>,
    },
    ReadHistory {
        key: SessionKey,
        id: SessionId,
        max_items: usize,
    },
    /// `key`'s history is no longer wanted.
    DropHistory {
        key: SessionKey,
    },
    CheckActivity {
        key: SessionKey,
        id: SessionId,
    },
    /// Start a session in `folder`, learn the models and the usage limits, close it.
    Look {
        folder: PathBuf,
    },
    /// Read the git branch of `folder` from its repository's files.
    ReadBranch {
        folder: PathBuf,
    },
    /// Read and check what chip `chip` holds.
    Prepare {
        chip: u64,
        source: Source,
    },
    /// Read again the full image behind picture `picture`: the image at `index` of the stored
    /// message `message` of session `id`.
    ReadImage {
        picture: u64,
        id: SessionId,
        message: String,
        index: usize,
    },
}

/// What the backend queues for the window. Everything that belongs to an open carries its
/// number, so the window can drop what is stale.
#[derive(Clone, Debug, PartialEq)]
pub enum FromBackend {
    Account(Account),
    OpenFailed {
        open: u64,
        error: AgentError,
    },
    Event {
        open: u64,
        event: SessionEvent,
    },
    SendFailed {
        open: u64,
        command: SessionCommand,
        error: AgentError,
    },
    Sessions {
        since_unix: Option<i64>,
        result: Result<Vec<StoredSession>, AgentError>,
    },
    /// A history, without its images' bytes: each stored image has an id in `pictures`
    /// instead, which holds the ids of each of its user messages in turn, one per image, and
    /// whose picture comes by itself in a `Picture`.
    History {
        key: SessionKey,
        result: Result<StoredHistory, AgentError>,
        pictures: Vec<Vec<u64>>,
    },
    Activity {
        key: SessionKey,
        result: Result<Option<i64>, AgentError>,
    },
    /// What a look learned: empty values for what it did not.
    Look {
        models: Vec<ModelOption>,
        model: Option<String>,
        usage: Option<UsageLimits>,
    },
    /// The branch read for `folder`: none outside a repository or when it could not be read.
    Branch {
        folder: PathBuf,
        branch: Option<Branch>,
    },
    /// What reading chip `chip`'s attachment gave.
    Prepared {
        chip: u64,
        result: Result<Prepared, Problem>,
    },
    /// The picture of the stored image `id`: none when it cannot be read. The pictures of a
    /// history come after it, in no set order.
    Picture {
        id: u64,
        thumb: Option<Thumb>,
    },
    /// The full image behind picture `picture`, read again from the store.
    Image {
        picture: u64,
        result: Result<ImageData, AgentError>,
    },
}

/// Called after every message is queued for the window, on whichever thread
/// queued it. It must be cheap and must not block.
pub type Wake = Arc<dyn Fn() + Send + Sync>;

/// What reads and checks an attachment: `attach::prepare`, or a stand-in in a test.
type Preparer = Arc<dyn Fn(&Source) -> Result<Prepared, Problem> + Send + Sync>;

/// What makes the picture of a stored image: `attach::picture_of`, or a stand-in in a test.
type Picturer = Arc<dyn Fn(&ImageData) -> Option<Thumb> + Send + Sync>;

/// The top bit of every stored picture's id, which no chip's id has.
const STORED_PICTURE: u64 = 1 << 63;

/// The sessions whose history is wanted: a `ReadHistory` adds its key, a `DropHistory` takes it
/// out. The window's end, the library thread and the preparing thread share it.
#[derive(Clone, Default)]
struct Wanted(Arc<Mutex<HashSet<SessionKey>>>);

impl Wanted {
    fn set(&self) -> MutexGuard<'_, HashSet<SessionKey>> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn add(&self, key: SessionKey) {
        self.set().insert(key);
    }

    fn remove(&self, key: SessionKey) {
        self.set().remove(&key);
    }

    fn has(&self, key: SessionKey) -> bool {
        self.set().contains(&key)
    }
}

/// The window's end of the backend. Neither `send` nor `try_recv` blocks: the first only
/// pushes onto channels (or starts a thread) and the second polls one.
///
/// Who holds what: the window holds the sending end of every thread's job channel and the
/// one receiving end of `inbox`; every thread holds a clone of the `Outbox`. An open's
/// sending end sits in its `Lane`, which the open's thread empties when it ends, so a job is
/// either queued before that and answered by the thread, or refused here.
pub struct Backend {
    agent: Arc<dyn Agent>,
    outbox: Outbox,
    inbox: Receiver<FromBackend>,
    /// Set when the `Backend` is dropped: every thread skips what is still queued.
    stopping: Arc<AtomicBool>,
    wanted: Wanted,
    library: Sender<LibraryJob>,
    preparing: Sender<PrepareJob>,
    lanes: RefCell<HashMap<u64, Arc<Lane>>>,
    looks: RefCell<Vec<LookThread>>,
    /// How many opens' threads are running.
    open_threads: Arc<AtomicUsize>,
}

impl Backend {
    pub const LOOK_START_LIMIT: Duration = Duration::from_secs(20);
    pub const LOOK_USAGE_LIMIT: Duration = Duration::from_secs(10);

    /// Starts the library thread and the preparing thread. `wake` is called after every message
    /// is queued for the window.
    pub fn start(agent: Arc<dyn Agent>, wake: Wake) -> Backend {
        Backend::start_with(
            agent,
            wake,
            Arc::new(attach::prepare),
            Arc::new(attach::picture_of),
        )
    }

    /// `start`, with `prepare` reading each attachment and `picture` making each stored
    /// image's picture.
    fn start_with(
        agent: Arc<dyn Agent>,
        wake: Wake,
        prepare: Preparer,
        picture: Picturer,
    ) -> Backend {
        let (sender, inbox) = mpsc::channel();
        let outbox = Outbox { sender, wake };
        let stopping = Arc::new(AtomicBool::new(false));
        let wanted = Wanted::default();
        let (preparing, prepare_jobs) = mpsc::channel();
        let (library, jobs) = mpsc::channel();
        let thread = Library {
            agent: agent.clone(),
            outbox: outbox.clone(),
            stopping: stopping.clone(),
            wanted: wanted.clone(),
            preparing: preparing.clone(),
            next_picture: 0,
        };
        thread::Builder::new()
            .name("mindrune-library".into())
            .spawn(move || thread.run(jobs))
            .expect("the backend's library thread starts");
        let thread = Preparing {
            outbox: outbox.clone(),
            stopping: stopping.clone(),
            wanted: wanted.clone(),
            prepare,
            picture,
        };
        thread::Builder::new()
            .name("mindrune-prepare".into())
            .spawn(move || thread.run(prepare_jobs))
            .expect("the backend's preparing thread starts");
        Backend {
            agent,
            outbox,
            inbox,
            stopping,
            wanted,
            library,
            preparing,
            lanes: RefCell::new(HashMap::new()),
            looks: RefCell::new(Vec::new()),
            open_threads: Arc::new(AtomicUsize::new(0)),
        }
    }

    /// Never blocks. A `Send` for an open that has no thread is answered `SendFailed` with
    /// `AgentError::Closed`. A history is wanted from its `ReadHistory` until its `DropHistory`.
    pub fn send(&self, message: ToBackend) {
        match message {
            ToBackend::CheckAccount => self.ask_library(LibraryJob::Account),
            ToBackend::ListSessions { since_unix } => {
                self.ask_library(LibraryJob::Sessions { since_unix })
            }
            ToBackend::ReadHistory { key, id, max_items } => {
                self.wanted.add(key);
                self.ask_library(LibraryJob::History { key, id, max_items })
            }
            ToBackend::DropHistory { key } => self.wanted.remove(key),
            ToBackend::CheckActivity { key, id } => {
                self.ask_library(LibraryJob::Activity { key, id })
            }
            ToBackend::Open { open, spec } => self.open(open, spec),
            ToBackend::Send { open, command } => self.pass(open, command),
            ToBackend::Look { folder } => self.look(folder),
            ToBackend::ReadBranch { folder } => self.ask_library(LibraryJob::Branch { folder }),
            ToBackend::Prepare { chip, source } => {
                // A send fails only once the preparing thread has ended; there is nobody to ask.
                let _ = self.preparing.send(PrepareJob::Prepare { chip, source });
            }
            ToBackend::ReadImage {
                picture,
                id,
                message,
                index,
            } => self.ask_library(LibraryJob::Image {
                picture,
                id,
                message,
                index,
            }),
        }
    }

    /// Never blocks.
    pub fn try_recv(&self) -> Option<FromBackend> {
        self.inbox.try_recv().ok()
    }

    /// Waits up to `timeout` for the next message. This one blocks: it is only for the app
    /// quitting, which waits a bounded time for its sessions to end (see `glue::close_session`).
    pub fn recv_timeout(&self, timeout: Duration) -> Option<FromBackend> {
        self.inbox.recv_timeout(timeout).ok()
    }

    /// Waits until every thread has handled every message sent so far, looks included. For
    /// tests and the scripted demo only: with a real agent this can block for seconds.
    pub fn flush(&self) {
        // Every open's thread, the library thread and the preparing thread answer their copy of
        // `token` when they reach it, or drop it unanswered when they end first: either way
        // everything sent to them before it was handled. The library passes its copy on to the
        // preparing thread, behind the pictures it asked for. `recv` fails once the last copy
        // is gone.
        let (token, handled) = mpsc::channel();
        for lane in self.lanes.borrow().values() {
            let _ = lane.offer(OpenJob::Flush(token.clone()));
        }
        let _ = self.preparing.send(PrepareJob::Flush(token.clone()));
        let _ = self.library.send(LibraryJob::Flush(token));
        while handled.recv().is_ok() {}
        // A look's thread handles one message; it is handled when the thread has ended.
        let looks = std::mem::take(&mut *self.looks.borrow_mut());
        for look in looks {
            let _ = look.done.recv();
        }
    }

    /// How many opens still have a thread.
    pub fn open_count(&self) -> usize {
        self.open_threads.load(Ordering::SeqCst)
    }

    fn ask_library(&self, job: LibraryJob) {
        // A send fails only once the library thread has ended; there is nobody to ask.
        let _ = self.library.send(job);
    }

    fn open(&self, open: u64, spec: StartSpec) {
        let mut lanes = self.lanes.borrow_mut();
        lanes.retain(|_, lane| lane.has_thread());
        let (jobs, queue) = mpsc::channel();
        let lane = Arc::new(Lane {
            jobs: Mutex::new(Some(jobs.clone())),
        });
        let thread = OpenThread {
            open,
            agent: self.agent.clone(),
            outbox: self.outbox.clone(),
            stopping: self.stopping.clone(),
            lane: lane.clone(),
            _counted: Counted::new(&self.open_threads),
        };
        let started = thread::Builder::new()
            .name(format!("mindrune-open-{open}"))
            .spawn(move || thread.run(spec, jobs, queue));
        match started {
            Ok(_) => {
                // Numbers are never reused; were one, the window could not reach the old
                // session any more, so it is closed.
                if let Some(old) = lanes.insert(open, lane) {
                    let _ = old.offer(OpenJob::Stop);
                }
            }
            Err(error) => self.outbox.queue(FromBackend::OpenFailed {
                open,
                error: AgentError::Io(error.to_string()),
            }),
        }
    }

    fn pass(&self, open: u64, command: SessionCommand) {
        let mut lanes = self.lanes.borrow_mut();
        let refused = match lanes.get(&open) {
            Some(lane) => lane.offer(OpenJob::Send(command)).err(),
            None => Some(OpenJob::Send(command)),
        };
        if refused.is_some() {
            lanes.remove(&open);
        }
        drop(lanes);
        if let Some(OpenJob::Send(command)) = refused {
            self.refuse(open, command);
        }
    }

    fn refuse(&self, open: u64, command: SessionCommand) {
        self.outbox.queue(FromBackend::SendFailed {
            open,
            command,
            error: AgentError::Closed,
        });
    }

    fn look(&self, folder: PathBuf) {
        let mut looks = self.looks.borrow_mut();
        looks.retain(LookThread::is_running);
        let (signals, queue) = mpsc::channel();
        let (finished, done) = mpsc::channel();
        let thread = Look {
            agent: self.agent.clone(),
            outbox: self.outbox.clone(),
            folder,
            signals: signals.clone(),
            queue,
            _finished: finished,
        };
        let started = thread::Builder::new()
            .name("mindrune-look".into())
            .spawn(move || thread.run());
        match started {
            Ok(_) => looks.push(LookThread { signals, done }),
            Err(_) => self.outbox.queue(Learned::default().answer()),
        }
    }
}

impl Drop for Backend {
    /// Does not wait: an agent call may be blocked for seconds. Every thread skips what is
    /// still queued, passes `Close` to the session it holds and ends on its own.
    fn drop(&mut self) {
        self.stopping.store(true, Ordering::SeqCst);
        for lane in self.lanes.get_mut().values() {
            let _ = lane.offer(OpenJob::Stop);
        }
        for look in self.looks.get_mut().iter() {
            let _ = look.signals.send(LookSignal::Stop);
        }
        // The library thread ends when `library` is dropped with this, and the preparing thread
        // once `preparing` and the library thread's sender are gone.
    }
}

/// The sending end of the window's inbox, with its waker.
#[derive(Clone)]
struct Outbox {
    sender: Sender<FromBackend>,
    wake: Wake,
}

impl Outbox {
    /// Queues `message` for the window and wakes it, unless the `Backend` is gone.
    fn queue(&self, message: FromBackend) {
        if self.sender.send(message).is_ok() {
            (self.wake)();
        }
    }
}

/// A thread answers it when it reaches it; `flush` waits until every copy is gone.
type FlushToken = Sender<()>;

enum LibraryJob {
    Account,
    Sessions {
        since_unix: Option<i64>,
    },
    History {
        key: SessionKey,
        id: SessionId,
        max_items: usize,
    },
    Activity {
        key: SessionKey,
        id: SessionId,
    },
    Branch {
        folder: PathBuf,
    },
    Image {
        picture: u64,
        id: SessionId,
        message: String,
        index: usize,
    },
    Flush(FlushToken),
}

struct Library {
    agent: Arc<dyn Agent>,
    outbox: Outbox,
    stopping: Arc<AtomicBool>,
    wanted: Wanted,
    /// Where the pictures of a history's images are made.
    preparing: Sender<PrepareJob>,
    /// The number of the next stored picture's id.
    next_picture: u64,
}

impl Library {
    /// Ends when the `Backend`, which holds the only sending end of `jobs`, is dropped.
    fn run(mut self, jobs: Receiver<LibraryJob>) {
        while let Ok(job) = jobs.recv() {
            if self.stopping.load(Ordering::SeqCst) {
                break;
            }
            let answer = match job {
                LibraryJob::Account => FromBackend::Account(self.agent.account()),
                LibraryJob::Sessions { since_unix } => FromBackend::Sessions {
                    since_unix,
                    result: self.agent.list_sessions(since_unix),
                },
                // A history no longer wanted is not read, nor answered when it stopped being
                // wanted while it was read.
                LibraryJob::History { key, .. } if !self.wanted.has(key) => continue,
                LibraryJob::History { key, id, max_items } => {
                    let result = self.agent.read_session(&id, max_items);
                    if self.wanted.has(key) {
                        self.answer_history(key, result);
                    }
                    continue;
                }
                LibraryJob::Activity { key, id } => FromBackend::Activity {
                    key,
                    result: self.agent.last_active(&id),
                },
                LibraryJob::Branch { folder } => FromBackend::Branch {
                    branch: git::branch(&folder),
                    folder,
                },
                LibraryJob::Image {
                    picture,
                    id,
                    message,
                    index,
                } => FromBackend::Image {
                    picture,
                    result: self.agent.read_image(&id, &message, index),
                },
                LibraryJob::Flush(token) => {
                    // Answered behind the pictures asked for so far, or dropped once the
                    // preparing thread is gone.
                    let _ = self.preparing.send(PrepareJob::Flush(token));
                    continue;
                }
            };
            self.outbox.queue(answer);
        }
    }

    /// Answers a history at once, with an id for each stored image and none of their bytes,
    /// then asks the preparing thread for each picture. An image whose bytes the store could
    /// not give has an id and no picture, and nothing is answered for it.
    fn answer_history(&mut self, key: SessionKey, result: Result<StoredHistory, AgentError>) {
        let mut pictures = Vec::new();
        let mut images = Vec::new();
        let result = result.map(|mut history| {
            for item in &mut history.items {
                if let HistoryItem::User {
                    image_count,
                    images: stored,
                    ..
                } = item
                {
                    let ids: Vec<u64> = (0..*image_count).map(|_| self.picture_id()).collect();
                    // Each image keeps its place; the history keeps `None` in every entry, even
                    // one past the count, so no bytes reach the window.
                    for (at, image) in stored.iter_mut().enumerate() {
                        if let Some(image) = image.take()
                            && let Some(&id) = ids.get(at)
                        {
                            images.push((id, image));
                        }
                    }
                    pictures.push(ids);
                }
            }
            history
        });
        self.outbox.queue(FromBackend::History {
            key,
            result,
            pictures,
        });
        for (id, image) in images {
            let _ = self.preparing.send(PrepareJob::Picture { key, id, image });
        }
    }

    /// A new id for a stored picture. Its top bit keeps it apart from every chip's.
    fn picture_id(&mut self) -> u64 {
        let id = STORED_PICTURE | self.next_picture;
        self.next_picture += 1;
        id
    }
}

enum PrepareJob {
    Prepare {
        chip: u64,
        source: Source,
    },
    /// The picture of a stored image of `key`'s history.
    Picture {
        key: SessionKey,
        id: u64,
        image: ImageData,
    },
    Flush(FlushToken),
}

/// Reads attachments and makes the pictures of stored images, apart from the library: a file
/// on a slow disk, or a large image, holds up only what was asked after it, and a chip waits
/// for no stored picture but the one under way.
struct Preparing {
    outbox: Outbox,
    stopping: Arc<AtomicBool>,
    wanted: Wanted,
    prepare: Preparer,
    picture: Picturer,
}

impl Preparing {
    /// Ends once the `Backend` and the library thread, which hold the sending ends of `jobs`,
    /// are gone and nothing is left to do, or as soon as the `Backend` is dropped.
    ///
    /// What the user waits to send goes first: every job waiting is taken in before the next
    /// one starts, and a chip's goes before the pictures asked for earlier. A picture under way
    /// is finished first. A flush waits behind the pictures, so it is still answered after
    /// everything sent before it.
    fn run(self, jobs: Receiver<PrepareJob>) {
        let mut chips = VecDeque::new();
        let mut later = VecDeque::new();
        loop {
            while let Ok(job) = jobs.try_recv() {
                match job {
                    PrepareJob::Prepare { .. } => chips.push_back(job),
                    PrepareJob::Picture { .. } | PrepareJob::Flush(_) => later.push_back(job),
                }
            }
            let job = match chips.pop_front().or_else(|| later.pop_front()) {
                Some(job) => job,
                None => match jobs.recv() {
                    Ok(job) => job,
                    Err(_) => break,
                },
            };
            if self.stopping.load(Ordering::SeqCst) {
                break;
            }
            self.handle(job);
        }
    }

    /// Does one job: a chip's, a picture's or a flush.
    fn handle(&self, job: PrepareJob) {
        match job {
            PrepareJob::Prepare { chip, source } => {
                // The decoders read files from anywhere: one that panics costs its own chip
                // and not the thread.
                let result = panic::catch_unwind(AssertUnwindSafe(|| (self.prepare)(&source)))
                    .unwrap_or(Err(Problem::Unreadable));
                self.outbox.queue(FromBackend::Prepared { chip, result });
            }
            // The picture of a history no longer wanted is not made, nor answered when the
            // history stopped being wanted while it was made.
            PrepareJob::Picture { key, .. } if !self.wanted.has(key) => {}
            PrepareJob::Picture { key, id, image } => {
                // A stored image is read as a file is: a decoder that panics costs its own
                // picture.
                let thumb = panic::catch_unwind(AssertUnwindSafe(|| (self.picture)(&image)))
                    .unwrap_or(None);
                if self.wanted.has(key) {
                    self.outbox.queue(FromBackend::Picture { id, thumb });
                }
            }
            PrepareJob::Flush(token) => {
                let _ = token.send(());
            }
        }
    }
}

enum OpenJob {
    Send(SessionCommand),
    Flush(FlushToken),
    /// From the open's own event sink: the session reported `Exited`.
    Exited,
    /// The `Backend` is gone: close the session and end.
    Stop,
}

/// The window's way to one open's thread. Its sender is taken by the thread when it ends.
struct Lane {
    jobs: Mutex<Option<Sender<OpenJob>>>,
}

impl Lane {
    /// Queues `job` for the thread, or gives it back when the thread has ended.
    fn offer(&self, job: OpenJob) -> Result<(), OpenJob> {
        match &*self.jobs.lock().unwrap_or_else(PoisonError::into_inner) {
            Some(jobs) => jobs.send(job).map_err(|refused| refused.0),
            None => Err(job),
        }
    }

    fn has_thread(&self) -> bool {
        self.jobs
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .is_some()
    }

    /// Takes the sender: from here on every job is refused by `offer`.
    fn shut(&self) {
        self.jobs
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
    }
}

/// Counts a running thread, from its making until it is dropped.
struct Counted(Arc<AtomicUsize>);

impl Counted {
    fn new(count: &Arc<AtomicUsize>) -> Counted {
        count.fetch_add(1, Ordering::SeqCst);
        Counted(count.clone())
    }
}

impl Drop for Counted {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

struct OpenThread {
    open: u64,
    agent: Arc<dyn Agent>,
    outbox: Outbox,
    stopping: Arc<AtomicBool>,
    lane: Arc<Lane>,
    _counted: Counted,
}

impl OpenThread {
    /// Opens the session and passes on its commands, until its `open` failed, it passed on
    /// `Close`, a `send` returned `Closed`, the session reported `Exited`, or the `Backend`
    /// was dropped. Only in the last case does it pass `Close` itself.
    fn run(self, spec: StartSpec, jobs: Sender<OpenJob>, queue: Receiver<OpenJob>) {
        let open = self.open;
        let outbox = self.outbox.clone();
        // Runs on the agent's own thread.
        let sink = Box::new(move |event: SessionEvent| {
            let exited = matches!(event, SessionEvent::Exited { .. });
            outbox.queue(FromBackend::Event { open, event });
            if exited {
                let _ = jobs.send(OpenJob::Exited);
            }
        });
        match self.agent.open(spec, sink) {
            Ok(handle) => {
                if self.pass_on(&*handle, &queue) {
                    let _ = handle.send(SessionCommand::Close);
                }
            }
            Err(error) => self.outbox.queue(FromBackend::OpenFailed { open, error }),
        }
        self.end(&queue);
    }

    /// True when it stopped because the `Backend` was dropped.
    fn pass_on(&self, handle: &dyn SessionHandle, queue: &Receiver<OpenJob>) -> bool {
        loop {
            if self.stopping.load(Ordering::SeqCst) {
                return true;
            }
            // The lane holds a sender until `end`, so this waits for the next job.
            let Ok(job) = queue.recv() else {
                return true;
            };
            if self.stopping.load(Ordering::SeqCst) {
                return true;
            }
            match job {
                OpenJob::Send(command) => {
                    let close = command == SessionCommand::Close;
                    match handle.send(command.clone()) {
                        Ok(()) if close => return false,
                        Ok(()) => {}
                        Err(error) => {
                            let closed = error == AgentError::Closed;
                            self.outbox.queue(FromBackend::SendFailed {
                                open: self.open,
                                command,
                                error,
                            });
                            if closed {
                                return false;
                            }
                        }
                    }
                }
                OpenJob::Flush(token) => {
                    let _ = token.send(());
                }
                OpenJob::Exited => return false,
                OpenJob::Stop => return true,
            }
        }
    }

    /// Shuts the lane, then answers what was queued before that: a `Send` is refused with
    /// `Closed`, a flush token is dropped.
    fn end(&self, queue: &Receiver<OpenJob>) {
        self.lane.shut();
        while let Ok(job) = queue.try_recv() {
            if let OpenJob::Send(command) = job {
                self.outbox.queue(FromBackend::SendFailed {
                    open: self.open,
                    command,
                    error: AgentError::Closed,
                });
            }
        }
    }
}

enum LookSignal {
    Event(SessionEvent),
    /// The `Backend` is gone.
    Stop,
}

/// The window's end of a look's thread.
struct LookThread {
    signals: Sender<LookSignal>,
    /// Disconnects when the thread ends.
    done: Receiver<()>,
}

impl LookThread {
    fn is_running(&self) -> bool {
        matches!(self.done.try_recv(), Err(TryRecvError::Empty))
    }
}

/// What a look learned so far.
#[derive(Default)]
struct Learned {
    models: Vec<ModelOption>,
    model: Option<String>,
    usage: Option<UsageLimits>,
}

impl Learned {
    fn take(&mut self, event: SessionEvent) {
        match event {
            SessionEvent::Started(info) => {
                self.models = info.models;
                self.model = Some(info.model);
            }
            SessionEvent::UsageLimits(usage) => self.usage = Some(usage),
            _ => {}
        }
    }

    fn answer(self) -> FromBackend {
        FromBackend::Look {
            models: self.models,
            model: self.model,
            usage: self.usage,
        }
    }
}

struct Look {
    agent: Arc<dyn Agent>,
    outbox: Outbox,
    folder: PathBuf,
    /// A copy for the session's event sink.
    signals: Sender<LookSignal>,
    queue: Receiver<LookSignal>,
    /// Dropped when the thread ends, which tells `flush`.
    _finished: Sender<()>,
}

impl Look {
    /// Always answers, and always closes what it opened.
    fn run(self) {
        let signals = self.signals.clone();
        // Runs on the agent's own thread.
        let sink = Box::new(move |event| {
            let _ = signals.send(LookSignal::Event(event));
        });
        let mut learned = Learned::default();
        if let Ok(handle) = self.agent.open(StartSpec::new(&self.folder), sink) {
            self.learn(&*handle, &mut learned);
            let _ = handle.send(SessionCommand::Close);
        }
        self.outbox.queue(learned.answer());
    }

    fn learn(&self, handle: &dyn SessionHandle, learned: &mut Learned) {
        let started = self.wait_for(Backend::LOOK_START_LIMIT, learned, |event| {
            matches!(event, SessionEvent::Started(_))
        });
        if !started
            || !self.agent.capabilities().usage_limits
            || handle.send(SessionCommand::RefreshUsage).is_err()
        {
            return;
        }
        self.wait_for(Backend::LOOK_USAGE_LIMIT, learned, |event| {
            matches!(event, SessionEvent::UsageLimits(_))
        });
    }

    /// Takes in the session's events until one that `wanted` accepts, for at most `limit`.
    /// False when it gave up first: the time ran out, the session ended, or the `Backend` was
    /// dropped.
    fn wait_for(
        &self,
        limit: Duration,
        learned: &mut Learned,
        wanted: impl Fn(&SessionEvent) -> bool,
    ) -> bool {
        let deadline = Instant::now() + limit;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let event = match self.queue.recv_timeout(left) {
                Ok(LookSignal::Event(event)) => event,
                Ok(LookSignal::Stop)
                | Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => {
                    return false;
                }
            };
            if matches!(event, SessionEvent::Exited { .. }) {
                return false;
            }
            let found = wanted(&event);
            learned.take(event);
            if found {
                return true;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mr_agent::{
        Account, Agent, AgentError, Answer, Capabilities, EventSink, FakeAgent, HistoryItem,
        LimitWindow, PermissionMode, RequestId, SessionCommand, SessionEvent, SessionHandle,
        SessionId, StartSpec, StoredHistory, StoredSession, UsageLimits,
    };
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::Instant;

    /// How long any test waits for something before it fails.
    const LIMIT: Duration = Duration::from_secs(5);

    fn next(fake: &FakeAgent, backend: &Backend) -> FromBackend {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            fake.advance(0.0);
            if let Some(message) = backend.try_recv() {
                return message;
            }
            assert!(std::time::Instant::now() < deadline, "nothing arrived");
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
    }

    /// Like `next`, for an agent there is nothing to pump on.
    fn wait(backend: &Backend) -> FromBackend {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            if let Some(message) = backend.try_recv() {
                return message;
            }
            assert!(std::time::Instant::now() < deadline, "nothing arrived");
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
    }

    fn start_with(agent: Arc<dyn Agent>) -> (Backend, Arc<AtomicUsize>) {
        let wakes = Arc::new(AtomicUsize::new(0));
        let counter = wakes.clone();
        let backend = Backend::start(
            agent,
            Arc::new(move || {
                counter.fetch_add(1, Ordering::SeqCst);
            }),
        );
        (backend, wakes)
    }

    fn start(fake: &FakeAgent) -> (Backend, Arc<AtomicUsize>) {
        start_with(Arc::new(fake.clone()))
    }

    /// Fails the test unless `done` turns true before the deadline.
    fn eventually(what: &str, mut done: impl FnMut() -> bool) {
        let deadline = Instant::now() + LIMIT;
        while !done() {
            assert!(Instant::now() < deadline, "{what} did not happen");
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    /// The next message that `wanted` accepts; the others are skipped.
    fn next_such(
        fake: &FakeAgent,
        backend: &Backend,
        wanted: impl Fn(&FromBackend) -> bool,
    ) -> FromBackend {
        loop {
            let message = next(fake, backend);
            if wanted(&message) {
                return message;
            }
        }
    }

    fn started(fake: &FakeAgent, backend: &Backend, open: u64) -> SessionId {
        backend.send(ToBackend::Open {
            open,
            spec: StartSpec::new("/home/alex"),
        });
        let id = match next(fake, backend) {
            FromBackend::Event {
                open: got,
                event: SessionEvent::Started(info),
            } if got == open => info.id,
            other => panic!("{other:?}"),
        };
        after_started(fake, backend, open);
        id
    }

    /// Takes in what the fake says after `Started`: the effort level, then the breakdown.
    fn after_started(fake: &FakeAgent, backend: &Backend, open: u64) {
        let effort = FromBackend::Event {
            open,
            event: SessionEvent::EffortChanged(Some("medium".into())),
        };
        assert_eq!(next(fake, backend), effort);
        match next(fake, backend) {
            FromBackend::Event {
                open: got,
                event: SessionEvent::ContextBreakdown(_),
            } if got == open => {}
            other => panic!("expected the breakdown, got {other:?}"),
        }
    }

    /// An agent that is not installed.
    struct Absent;
    impl Agent for Absent {
        fn capabilities(&self) -> Capabilities {
            Capabilities {
                remote_access: false,
                plan_mode: false,
                usage_limits: false,
                images: false,
            }
        }
        fn account(&self) -> Account {
            Account::Missing
        }
        fn list_sessions(&self, _: Option<i64>) -> Result<Vec<StoredSession>, AgentError> {
            Ok(Vec::new())
        }
        fn read_session(&self, _: &SessionId, _: usize) -> Result<StoredHistory, AgentError> {
            Ok(StoredHistory::default())
        }
        fn last_active(&self, _: &SessionId) -> Result<Option<i64>, AgentError> {
            Ok(None)
        }
        fn open(&self, _: StartSpec, _: EventSink) -> Result<Box<dyn SessionHandle>, AgentError> {
            Err(AgentError::NotInstalled)
        }
    }

    /// Which call of a `Gated` agent waits for the test.
    enum Slow {
        OpenIn(PathBuf),
        Listing,
    }

    /// The fake, but one kind of call waits until the test lets it go, by sending on or
    /// dropping the `release` end. It says when it starts to wait.
    struct Gated {
        fake: FakeAgent,
        slow: Slow,
        entered: mpsc::Sender<()>,
        release: Mutex<mpsc::Receiver<()>>,
        holding: AtomicBool,
    }

    impl Gated {
        fn hold(&self) {
            self.holding.store(true, Ordering::SeqCst);
            let _ = self.entered.send(());
            let _ = self.release.lock().unwrap().recv();
            self.holding.store(false, Ordering::SeqCst);
        }

        fn holding(&self) -> bool {
            self.holding.load(Ordering::SeqCst)
        }
    }

    /// The agent, the end that says a slow call began, and the end that lets it go.
    fn gated(fake: &FakeAgent, slow: Slow) -> (Arc<Gated>, mpsc::Receiver<()>, mpsc::Sender<()>) {
        let (entered, began) = mpsc::channel();
        let (release, gate) = mpsc::channel();
        let agent = Arc::new(Gated {
            fake: fake.clone(),
            slow,
            entered,
            release: Mutex::new(gate),
            holding: AtomicBool::new(false),
        });
        (agent, began, release)
    }

    impl Agent for Gated {
        fn capabilities(&self) -> Capabilities {
            self.fake.capabilities()
        }
        fn account(&self) -> Account {
            self.fake.account()
        }
        fn list_sessions(&self, since: Option<i64>) -> Result<Vec<StoredSession>, AgentError> {
            if matches!(self.slow, Slow::Listing) {
                self.hold();
            }
            self.fake.list_sessions(since)
        }
        fn read_session(&self, id: &SessionId, max: usize) -> Result<StoredHistory, AgentError> {
            self.fake.read_session(id, max)
        }
        fn last_active(&self, id: &SessionId) -> Result<Option<i64>, AgentError> {
            self.fake.last_active(id)
        }
        fn open(
            &self,
            spec: StartSpec,
            sink: EventSink,
        ) -> Result<Box<dyn SessionHandle>, AgentError> {
            if let Slow::OpenIn(folder) = &self.slow
                && spec.folder == *folder
            {
                self.hold();
            }
            self.fake.open(spec, sink)
        }
    }

    fn hello() -> SessionCommand {
        SessionCommand::Send {
            text: "hello".into(),
            attachments: Vec::new(),
        }
    }

    fn stored_history() -> StoredHistory {
        StoredHistory {
            items: vec![
                HistoryItem::User {
                    text: "Tidy the notes".into(),
                    image_count: 0,
                    files: Vec::new(),
                    images: Vec::new(),
                    stored: None,
                },
                HistoryItem::AssistantText("Done, alex.".into()),
            ],
            earlier_left_out: false,
            model: Some("fake-model".into()),
            permission_mode: Some(PermissionMode::Ask),
            effort: None,
        }
    }

    #[test]
    fn recv_timeout_waits_for_a_message_and_no_longer_than_asked() {
        let fake = FakeAgent::new();
        let (backend, _) = start_with(Arc::new(fake.clone()));
        let started = std::time::Instant::now();
        assert_eq!(backend.recv_timeout(Duration::from_millis(50)), None);
        assert!(started.elapsed() >= Duration::from_millis(50));
        backend.send(ToBackend::CheckAccount);
        let account = backend.recv_timeout(Duration::from_secs(5));
        assert!(
            matches!(account, Some(FromBackend::Account(_))),
            "{account:?}"
        );
    }

    #[test]
    fn the_account_comes_back_and_wakes_the_window() {
        let fake = FakeAgent::new();
        let account = Account::SignedIn {
            name: "alex".into(),
            plan: Some("Max".into()),
            full_login: true,
        };
        fake.set_account(account.clone());
        let (backend, wakes) = start(&fake);
        backend.send(ToBackend::CheckAccount);
        assert_eq!(next(&fake, &backend), FromBackend::Account(account));
        // The worker queues the message first and wakes after, so the wake can
        // land a moment after `next` has taken the message.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while wakes.load(Ordering::SeqCst) < 1 {
            assert!(
                std::time::Instant::now() < deadline,
                "the window was never woken"
            );
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
    }

    #[test]
    fn events_carry_the_number_of_their_open() {
        let fake = FakeAgent::new();
        let (backend, _) = start(&fake);
        backend.send(ToBackend::Open {
            open: 7,
            spec: StartSpec::new("/p"),
        });
        let FromBackend::Event {
            open: 7,
            event: SessionEvent::Started(first),
        } = next(&fake, &backend)
        else {
            panic!()
        };
        after_started(&fake, &backend, 7);
        backend.send(ToBackend::Open {
            open: 8,
            spec: StartSpec::new("/q"),
        });
        let FromBackend::Event {
            open: 8,
            event: SessionEvent::Started(second),
        } = next(&fake, &backend)
        else {
            panic!()
        };
        after_started(&fake, &backend, 8);
        assert_ne!(first.id, second.id);
        fake.emit(&first.id, SessionEvent::TextDelta("late".into()));
        assert_eq!(
            next(&fake, &backend),
            FromBackend::Event {
                open: 7,
                event: SessionEvent::TextDelta("late".into())
            }
        );
    }

    #[test]
    fn commands_reach_the_right_session_and_failures_come_back() {
        let fake = FakeAgent::new();
        let (backend, _) = start(&fake);
        backend.send(ToBackend::Open {
            open: 1,
            spec: StartSpec::new("/p"),
        });
        let FromBackend::Event {
            event: SessionEvent::Started(info),
            ..
        } = next(&fake, &backend)
        else {
            panic!()
        };
        after_started(&fake, &backend, 1);
        backend.send(ToBackend::Send {
            open: 1,
            command: SessionCommand::Stop,
        });
        let answer = SessionCommand::Answer {
            id: RequestId("r9".into()),
            answer: Answer::Allow { always: false },
        };
        backend.send(ToBackend::Send {
            open: 1,
            command: answer.clone(),
        });
        let FromBackend::SendFailed {
            open: 1,
            command,
            error: AgentError::Protocol(_),
        } = next(&fake, &backend)
        else {
            panic!()
        };
        assert_eq!(command, answer);
        assert_eq!(fake.sent(), vec![(info.id, SessionCommand::Stop)]);
        backend.send(ToBackend::Send {
            open: 99,
            command: SessionCommand::Stop,
        });
        assert_eq!(
            next(&fake, &backend),
            FromBackend::SendFailed {
                open: 99,
                command: SessionCommand::Stop,
                error: AgentError::Closed
            }
        );
    }

    #[test]
    fn a_failed_open_is_reported_with_its_number() {
        let (backend, _) = start_with(Arc::new(Absent));
        backend.send(ToBackend::Open {
            open: 3,
            spec: StartSpec::new("/p"),
        });
        assert_eq!(
            wait(&backend),
            FromBackend::OpenFailed {
                open: 3,
                error: AgentError::NotInstalled
            }
        );
    }

    #[test]
    fn dropping_the_backend_closes_its_sessions() {
        let fake = FakeAgent::new();
        let (backend, _) = start(&fake);
        backend.send(ToBackend::Open {
            open: 1,
            spec: StartSpec::new("/p"),
        });
        let FromBackend::Event {
            event: SessionEvent::Started(info),
            ..
        } = next(&fake, &backend)
        else {
            panic!()
        };
        drop(backend);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while !fake
            .sent()
            .contains(&(info.id.clone(), SessionCommand::Close))
        {
            assert!(
                std::time::Instant::now() < deadline,
                "the session was not closed"
            );
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
    }

    #[test]
    fn a_slow_open_does_not_hold_up_another_session() {
        let fake = FakeAgent::new();
        let (agent, began, release) = gated(&fake, Slow::OpenIn("/slow".into()));
        let (backend, _) = start_with(agent.clone());
        backend.send(ToBackend::Open {
            open: 1,
            spec: StartSpec::new("/slow"),
        });
        began.recv_timeout(LIMIT).expect("open 1 began");
        backend.send(ToBackend::Open {
            open: 2,
            spec: StartSpec::new("/home/alex"),
        });
        backend.send(ToBackend::Send {
            open: 2,
            command: hello(),
        });
        eventually("the message reaching the fake", || {
            fake.sent().iter().any(|(_, command)| *command == hello())
        });
        assert!(agent.holding(), "open 1 is still blocked");
        drop(release);
    }

    #[test]
    fn a_slow_listing_does_not_hold_up_a_send() {
        let fake = FakeAgent::new();
        let (agent, began, release) = gated(&fake, Slow::Listing);
        let (backend, _) = start_with(agent.clone());
        backend.send(ToBackend::ListSessions { since_unix: None });
        began.recv_timeout(LIMIT).expect("the listing began");
        backend.send(ToBackend::Open {
            open: 1,
            spec: StartSpec::new("/home/alex"),
        });
        backend.send(ToBackend::Send {
            open: 1,
            command: hello(),
        });
        eventually("the message reaching the fake", || {
            fake.sent().iter().any(|(_, command)| *command == hello())
        });
        assert!(agent.holding(), "the listing is still blocked");
        release.send(()).unwrap();
        let sessions = next_such(&fake, &backend, |m| {
            matches!(m, FromBackend::Sessions { .. })
        });
        assert_eq!(
            sessions,
            FromBackend::Sessions {
                since_unix: None,
                result: Ok(Vec::new())
            }
        );
    }

    #[test]
    fn a_slow_look_does_not_hold_up_a_history() {
        let fake = FakeAgent::new();
        let id = SessionId("stored-1".into());
        fake.set_history(&id, stored_history());
        let (agent, began, release) = gated(&fake, Slow::OpenIn("/slow".into()));
        let (backend, _) = start_with(agent.clone());
        backend.send(ToBackend::Look {
            folder: "/slow".into(),
        });
        began.recv_timeout(LIMIT).expect("the look began");
        backend.send(ToBackend::ReadHistory {
            key: SessionKey(2),
            id,
            max_items: 10,
        });
        assert_eq!(
            wait(&backend),
            FromBackend::History {
                key: SessionKey(2),
                result: Ok(stored_history()),
                pictures: vec![Vec::new()],
            }
        );
        assert!(agent.holding(), "the look is still blocked");
        drop(release);
    }

    #[test]
    fn library_jobs_keep_their_order() {
        let fake = FakeAgent::new();
        let id = SessionId("stored-1".into());
        fake.set_history(&id, stored_history());
        let (backend, _) = start(&fake);
        backend.send(ToBackend::ListSessions { since_unix: None });
        backend.send(ToBackend::ReadHistory {
            key: SessionKey(1),
            id,
            max_items: 10,
        });
        let first = wait(&backend);
        assert!(matches!(first, FromBackend::Sessions { .. }), "{first:?}");
        let second = wait(&backend);
        assert!(matches!(second, FromBackend::History { .. }), "{second:?}");
    }

    #[test]
    fn the_listing_and_the_history_come_back() {
        let fake = FakeAgent::new();
        let id = SessionId("stored-1".into());
        let stored = StoredSession {
            id: id.clone(),
            folder: "/home/alex/notes".into(),
            title: "Tidy the notes".into(),
            last_active_unix: 1_794_000_000,
        };
        fake.set_sessions(vec![stored.clone()]);
        fake.set_history(&id, stored_history());
        fake.set_last_active(&id, Some(1_794_000_100));
        let (backend, _) = start(&fake);
        backend.send(ToBackend::ListSessions {
            since_unix: Some(1_793_000_000),
        });
        assert_eq!(
            wait(&backend),
            FromBackend::Sessions {
                since_unix: Some(1_793_000_000),
                result: Ok(vec![stored])
            }
        );
        backend.send(ToBackend::ReadHistory {
            key: SessionKey(4),
            id: id.clone(),
            max_items: 10,
        });
        assert_eq!(
            wait(&backend),
            FromBackend::History {
                key: SessionKey(4),
                result: Ok(stored_history()),
                pictures: vec![Vec::new()],
            }
        );
        backend.send(ToBackend::CheckActivity {
            key: SessionKey(5),
            id,
        });
        assert_eq!(
            wait(&backend),
            FromBackend::Activity {
                key: SessionKey(5),
                result: Ok(Some(1_794_000_100))
            }
        );
    }

    #[test]
    fn the_library_reads_a_branch() {
        let dir = tempfile::tempdir().unwrap();
        let repository = dir.path().join("notes");
        std::fs::create_dir_all(repository.join(".git")).unwrap();
        std::fs::write(repository.join(".git/HEAD"), "ref: refs/heads/main\n").unwrap();
        let plain = dir.path().join("plain");
        std::fs::create_dir_all(&plain).unwrap();
        let (backend, _) = start(&FakeAgent::new());
        backend.send(ToBackend::ReadBranch {
            folder: repository.clone(),
        });
        backend.send(ToBackend::ReadBranch {
            folder: plain.clone(),
        });
        assert_eq!(
            wait(&backend),
            FromBackend::Branch {
                folder: repository,
                branch: Some(Branch::Named("main".into())),
            }
        );
        assert_eq!(
            wait(&backend),
            FromBackend::Branch {
                folder: plain,
                branch: None,
            }
        );
    }

    #[test]
    fn an_attachment_is_prepared() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("dot.png");
        let pixels = image::RgbaImage::from_pixel(4, 3, image::Rgba([200, 40, 40, 255]));
        pixels.save(&path).unwrap();
        let (backend, _) = start(&FakeAgent::new());
        backend.send(ToBackend::Prepare {
            chip: 7,
            source: Source::Path(path),
        });
        let FromBackend::Prepared { chip, result } = wait(&backend) else {
            panic!("the answer is not a prepared chip");
        };
        let prepared = result.expect("the image is read");
        assert_eq!(chip, 7);
        assert_eq!(prepared.kind, crate::attach::Kind::Image);
        let thumb = prepared.thumb.expect("an image has a thumb");
        assert_eq!((thumb.width, thumb.height), (4, 3));
    }

    /// A folder holding a repository on the branch `main`.
    fn repository(dir: &tempfile::TempDir) -> PathBuf {
        let repository = dir.path().join("notes");
        std::fs::create_dir_all(repository.join(".git")).unwrap();
        std::fs::write(repository.join(".git/HEAD"), "ref: refs/heads/main\n").unwrap();
        repository
    }

    #[test]
    fn preparing_does_not_hold_up_the_library() {
        let dir = tempfile::tempdir().unwrap();
        let repository = repository(&dir);
        // Each attachment waits until the test lets it go, and says when it starts to wait.
        let (entered, began) = mpsc::channel();
        let (release, gate) = mpsc::channel::<()>();
        let gate = Mutex::new(gate);
        let prepare: Preparer = Arc::new(move |_: &Source| {
            let _ = entered.send(());
            let _ = gate.lock().unwrap().recv();
            Err(Problem::Unreadable)
        });
        let backend = Backend::start_with(
            Arc::new(FakeAgent::new()),
            Arc::new(|| {}),
            prepare,
            Arc::new(attach::picture_of),
        );
        for chip in 1..=3 {
            backend.send(ToBackend::Prepare {
                chip,
                source: Source::Path(format!("/home/alex/photo-{chip}.png").into()),
            });
        }
        began
            .recv_timeout(LIMIT)
            .expect("the first attachment is read");
        backend.send(ToBackend::ReadBranch {
            folder: repository.clone(),
        });
        assert_eq!(
            wait(&backend),
            FromBackend::Branch {
                folder: repository,
                branch: Some(Branch::Named("main".into())),
            }
        );
        drop(release);
        for chip in 1..=3 {
            assert_eq!(
                wait(&backend),
                FromBackend::Prepared {
                    chip,
                    result: Err(Problem::Unreadable)
                }
            );
        }
    }

    #[test]
    fn a_panic_while_preparing_costs_one_chip() {
        let dir = tempfile::tempdir().unwrap();
        let notes = dir.path().join("notes.txt");
        std::fs::write(&notes, "a line\n").unwrap();
        // Stands for a decoder that breaks on one file.
        let prepare: Preparer = Arc::new(|source: &Source| match source {
            Source::Path(path) if path.ends_with("broken.png") => panic!("a decoder broke"),
            other => attach::prepare(other),
        });
        let backend = Backend::start_with(
            Arc::new(FakeAgent::new()),
            Arc::new(|| {}),
            prepare,
            Arc::new(attach::picture_of),
        );
        backend.send(ToBackend::Prepare {
            chip: 1,
            source: Source::Path(dir.path().join("broken.png")),
        });
        backend.send(ToBackend::Prepare {
            chip: 2,
            source: Source::Path(notes.clone()),
        });
        let FromBackend::Prepared { chip, result } = wait(&backend) else {
            panic!("the answer is not a prepared chip");
        };
        assert_eq!(chip, 1);
        assert_eq!(result.map_err(Problem::words), Err("Cannot be read"));
        let FromBackend::Prepared { chip, result } = wait(&backend) else {
            panic!("the answer is not a prepared chip");
        };
        assert_eq!(chip, 2);
        assert_eq!(
            result.map(|prepared| prepared.attachment),
            Ok(mr_agent::Attachment::File { path: notes })
        );
    }

    /// A PNG of `width` by `height`, made here.
    fn png(width: u32, height: u32) -> ImageData {
        let pixels = image::RgbaImage::from_pixel(width, height, image::Rgba([40, 90, 200, 255]));
        let mut bytes = std::io::Cursor::new(Vec::new());
        pixels
            .write_to(&mut bytes, image::ImageFormat::Png)
            .unwrap();
        ImageData::new(bytes.into_inner())
    }

    /// A stored history of one message of the user's with `images`, `None` for one not read,
    /// and a reply.
    fn history_with_images(images: Vec<Option<ImageData>>) -> StoredHistory {
        StoredHistory {
            items: vec![
                HistoryItem::User {
                    text: "look at these".into(),
                    image_count: images.len(),
                    files: Vec::new(),
                    images,
                    stored: None,
                },
                HistoryItem::AssistantText("Seen, alex.".into()),
            ],
            ..StoredHistory::default()
        }
    }

    /// The history `backend` answers for `key`: its items, without any image's bytes, and the
    /// ids of its stored pictures.
    fn read_history(
        backend: &Backend,
        key: SessionKey,
        id: &SessionId,
    ) -> (StoredHistory, Vec<Vec<u64>>) {
        backend.send(ToBackend::ReadHistory {
            key,
            id: id.clone(),
            max_items: 10,
        });
        match wait(backend) {
            FromBackend::History {
                key: answered,
                result: Ok(history),
                pictures,
            } if answered == key => (history, pictures),
            other => panic!("not the history: {other:?}"),
        }
    }

    /// Picture answers until `count` came, by id.
    fn pictures(backend: &Backend, count: usize) -> HashMap<u64, Option<Thumb>> {
        let mut answers = HashMap::new();
        while answers.len() < count {
            match wait(backend) {
                FromBackend::Picture { id, thumb } => {
                    assert!(answers.insert(id, thumb).is_none(), "{id} answered twice");
                }
                other => panic!("not a picture: {other:?}"),
            }
        }
        answers
    }

    #[test]
    fn a_history_is_answered_before_its_pictures() {
        let fake = FakeAgent::new();
        let id = SessionId("stored-1".into());
        fake.set_history(
            &id,
            history_with_images(vec![Some(png(60, 30)), Some(png(5, 8))]),
        );
        // Each picture waits until the test lets it go, so the history cannot wait for them.
        let (release, gate) = mpsc::channel::<()>();
        let gate = Mutex::new(gate);
        let picture: Picturer = Arc::new(move |image: &ImageData| {
            let _ = gate.lock().unwrap().recv();
            attach::picture_of(image)
        });
        let backend = Backend::start_with(
            Arc::new(fake.clone()),
            Arc::new(|| {}),
            Arc::new(attach::prepare),
            picture,
        );
        let (history, ids) = read_history(&backend, SessionKey(3), &id);
        let [
            HistoryItem::User {
                image_count,
                images,
                ..
            },
            HistoryItem::AssistantText(_),
        ] = history.items.as_slice()
        else {
            panic!("not the stored items: {:?}", history.items);
        };
        assert_eq!(*image_count, 2);
        assert_eq!(images, &[None, None], "the bytes came with the history");
        let [ids] = ids.as_slice() else {
            panic!("not one list of ids: {ids:?}");
        };
        assert_eq!(ids.len(), 2);
        assert_ne!(ids[0], ids[1]);
        assert!(ids.iter().all(|id| id & 1 << 63 != 0), "{ids:?}");
        assert!(
            backend.try_recv().is_none(),
            "a picture came before it was let go"
        );
        drop(release);
        let answers = pictures(&backend, 2);
        let size = |id: u64| {
            let thumb = answers[&id].as_ref().expect("a picture");
            (thumb.width, thumb.height)
        };
        assert_eq!(size(ids[0]), (60, 30));
        assert_eq!(size(ids[1]), (5, 8));
        // Read again, the same session gives new ids.
        let (_, again) = read_history(&backend, SessionKey(3), &id);
        assert_eq!(again[0].len(), 2);
        assert!(again[0].iter().all(|id| !ids.contains(id)), "{again:?}");
        assert_eq!(pictures(&backend, 2).len(), 2);
    }

    #[test]
    fn an_unreadable_stored_image_answers_with_no_picture() {
        let fake = FakeAgent::new();
        let id = SessionId("stored-1".into());
        // Three counted: one read and no image, one an image, one not read at all.
        let not_an_image = ImageData::new(b"alex was here".to_vec());
        fake.set_history(
            &id,
            history_with_images(vec![Some(not_an_image), Some(png(4, 4)), None]),
        );
        let (backend, _) = start(&fake);
        let (_, ids) = read_history(&backend, SessionKey(1), &id);
        assert_eq!(ids[0].len(), 3, "an image not read still has an id");
        backend.flush();
        let answers = pictures(&backend, 2);
        assert_eq!(answers[&ids[0][0]], None);
        assert!(answers[&ids[0][1]].is_some());
        assert!(!answers.contains_key(&ids[0][2]));
        assert!(backend.try_recv().is_none());
    }

    #[test]
    fn an_unreadable_image_in_the_middle_keeps_the_others_in_place() {
        let fake = FakeAgent::new();
        let id = SessionId("stored-1".into());
        // The second was held another way than as base64: the reader could not read it.
        fake.set_history(
            &id,
            history_with_images(vec![Some(png(6, 3)), None, Some(png(2, 5))]),
        );
        let (backend, _) = start(&fake);
        let (_, ids) = read_history(&backend, SessionKey(1), &id);
        let [ids] = ids.as_slice() else {
            panic!("not one list of ids: {ids:?}");
        };
        assert_eq!(ids.len(), 3);
        backend.flush();
        let answers = pictures(&backend, 2);
        let size = |id: u64| {
            let thumb = answers[&id].as_ref().expect("a picture");
            (thumb.width, thumb.height)
        };
        assert_eq!(size(ids[0]), (6, 3));
        assert_eq!(size(ids[2]), (2, 5));
        assert!(!answers.contains_key(&ids[1]));
        assert_eq!(
            backend.try_recv(),
            None,
            "nothing comes for the one not read"
        );
    }

    #[test]
    fn a_chip_is_prepared_before_waiting_pictures() {
        let dir = tempfile::tempdir().unwrap();
        let fake = FakeAgent::new();
        let id = SessionId("stored-1".into());
        let images = (1..=4).map(|width| Some(png(width, 1))).collect();
        fake.set_history(&id, history_with_images(images));
        // Each picture says when it starts and waits until the test lets it go.
        let (entered, began) = mpsc::channel();
        let (release, gate) = mpsc::channel::<()>();
        let gate = Mutex::new(gate);
        let picture: Picturer = Arc::new(move |image: &ImageData| {
            let _ = entered.send(());
            let _ = gate.lock().unwrap().recv();
            attach::picture_of(image)
        });
        let backend = Backend::start_with(
            Arc::new(fake.clone()),
            Arc::new(|| {}),
            Arc::new(attach::prepare),
            picture,
        );
        let (_, ids) = read_history(&backend, SessionKey(1), &id);
        let ids = &ids[0];
        began
            .recv_timeout(LIMIT)
            .expect("the first picture is under way");
        // The library asked for every picture before it answers what was sent after the history.
        let plain = dir.path().to_path_buf();
        backend.send(ToBackend::ReadBranch {
            folder: plain.clone(),
        });
        assert_eq!(
            wait(&backend),
            FromBackend::Branch {
                folder: plain,
                branch: None,
            }
        );
        backend.send(ToBackend::Prepare {
            chip: 7,
            source: Source::Pasted(png(3, 2)),
        });
        drop(release);
        let answers: Vec<FromBackend> = (0..5).map(|_| wait(&backend)).collect();
        // The picture under way is finished first, then the chip, then the pictures that waited.
        assert!(
            matches!(&answers[0], FromBackend::Picture { id, .. } if *id == ids[0]),
            "{:?}",
            answers[0]
        );
        assert!(
            matches!(
                &answers[1],
                FromBackend::Prepared {
                    chip: 7,
                    result: Ok(_)
                }
            ),
            "{:?}",
            answers[1]
        );
        let mut rest: Vec<u64> = answers[2..]
            .iter()
            .map(|answer| match answer {
                FromBackend::Picture { id, .. } => *id,
                other => panic!("not a picture: {other:?}"),
            })
            .collect();
        rest.sort_unstable();
        assert_eq!(rest, ids[1..]);
    }

    #[test]
    fn a_history_answered_holds_no_bytes_whatever_the_agent_sends() {
        let fake = FakeAgent::new();
        let id = SessionId("stored-1".into());
        // An agent that counts one image and gives two.
        let mut history = history_with_images(vec![Some(png(3, 3)), Some(png(4, 4))]);
        if let HistoryItem::User { image_count, .. } = &mut history.items[0] {
            *image_count = 1;
        }
        fake.set_history(&id, history);
        let (backend, _) = start(&fake);
        let (answered, ids) = read_history(&backend, SessionKey(1), &id);
        let HistoryItem::User { images, .. } = &answered.items[0] else {
            panic!("not the user's message: {:?}", answered.items);
        };
        assert!(images.iter().all(Option::is_none), "{images:?}");
        assert_eq!(ids, vec![vec![ids[0][0]]]);
        backend.flush();
        let answers = pictures(&backend, 1);
        assert!(answers[&ids[0][0]].is_some());
        assert_eq!(
            backend.try_recv(),
            None,
            "nothing comes for the image not counted"
        );
    }

    #[test]
    fn a_panic_while_making_a_picture_costs_one_picture() {
        let fake = FakeAgent::new();
        let id = SessionId("stored-1".into());
        fake.set_history(
            &id,
            history_with_images(vec![Some(png(3, 3)), Some(png(4, 4))]),
        );
        // Stands for a decoder that breaks on one image.
        let picture: Picturer = Arc::new(|image: &ImageData| {
            let thumb = attach::picture_of(image).expect("a picture");
            assert!(thumb.width != 3, "a decoder broke");
            Some(thumb)
        });
        let backend = Backend::start_with(
            Arc::new(fake.clone()),
            Arc::new(|| {}),
            Arc::new(attach::prepare),
            picture,
        );
        let (_, ids) = read_history(&backend, SessionKey(1), &id);
        let answers = pictures(&backend, 2);
        assert_eq!(answers[&ids[0][0]], None);
        assert!(answers[&ids[0][1]].is_some());
    }

    #[test]
    fn a_history_without_images_asks_nothing_of_the_prepare_thread() {
        let fake = FakeAgent::new();
        let id = SessionId("stored-1".into());
        fake.set_history(&id, stored_history());
        let asked = Arc::new(AtomicUsize::new(0));
        let counter = asked.clone();
        let picture: Picturer = Arc::new(move |_: &ImageData| {
            counter.fetch_add(1, Ordering::SeqCst);
            None
        });
        let backend = Backend::start_with(
            Arc::new(fake.clone()),
            Arc::new(|| {}),
            Arc::new(attach::prepare),
            picture,
        );
        let (history, ids) = read_history(&backend, SessionKey(1), &id);
        assert_eq!(history, stored_history());
        assert_eq!(ids, vec![Vec::<u64>::new()]);
        backend.flush();
        assert_eq!(backend.try_recv(), None);
        assert_eq!(asked.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn a_session_that_ended_keeps_no_thread() {
        let fake = FakeAgent::new();
        let (backend, _) = start(&fake);
        let id = started(&fake, &backend, 1);
        assert_eq!(backend.open_count(), 1);
        let exited = SessionEvent::Exited {
            clean: true,
            detail: String::new(),
        };
        fake.emit(&id, exited.clone());
        assert_eq!(
            next(&fake, &backend),
            FromBackend::Event {
                open: 1,
                event: exited
            }
        );
        eventually("the open's thread ending", || backend.open_count() == 0);
        backend.send(ToBackend::Send {
            open: 1,
            command: SessionCommand::Stop,
        });
        assert_eq!(
            next(&fake, &backend),
            FromBackend::SendFailed {
                open: 1,
                command: SessionCommand::Stop,
                error: AgentError::Closed
            }
        );
    }

    #[test]
    fn a_look_reports_and_closes() {
        let fake = FakeAgent::new();
        let limits = UsageLimits {
            session: Some(LimitWindow {
                used: 0.25,
                resets_at_unix: 1_794_385_200,
            }),
            weekly: None,
        };
        let (backend, _) = start(&fake);
        backend.send(ToBackend::Look {
            folder: "/home/alex".into(),
        });
        // The fake answers `RefreshUsage` the way the real agent does.
        let deadline = Instant::now() + LIMIT;
        let mut asked = None;
        let look = loop {
            fake.advance(0.0);
            if asked.is_none()
                && let Some((id, _)) = fake
                    .sent()
                    .into_iter()
                    .find(|(_, command)| *command == SessionCommand::RefreshUsage)
            {
                fake.emit(&id, SessionEvent::UsageLimits(limits.clone()));
                asked = Some(id);
            }
            if let Some(message) = backend.try_recv() {
                break message;
            }
            assert!(Instant::now() < deadline, "the look did not answer");
            std::thread::sleep(Duration::from_millis(2));
        };
        let FromBackend::Look {
            models,
            model,
            usage,
        } = look
        else {
            panic!("{look:?}")
        };
        let ids: Vec<&str> = models.iter().map(|option| option.id.as_str()).collect();
        assert_eq!(ids, ["default", "fast"]);
        assert_eq!(model.as_deref(), Some("fake-model"));
        assert_eq!(usage, Some(limits));
        let id = asked.expect("the look asked for the limits");
        let sent = fake.sent();
        assert_eq!(
            sent[sent.len() - 2..],
            [
                (id.clone(), SessionCommand::RefreshUsage),
                (id, SessionCommand::Close)
            ]
        );
    }

    #[test]
    fn a_look_that_learns_nothing_still_answers() {
        let fake = FakeAgent::new();
        fake.fail_next_open("no such session");
        let (backend, _) = start(&fake);
        backend.send(ToBackend::Look {
            folder: "/home/alex".into(),
        });
        assert_eq!(
            next(&fake, &backend),
            FromBackend::Look {
                models: Vec::new(),
                model: None,
                usage: None
            }
        );
    }

    #[test]
    fn a_flush_waits_for_a_look() {
        let (backend, _) = start_with(Arc::new(Absent));
        backend.send(ToBackend::Look {
            folder: "/home/alex".into(),
        });
        backend.flush();
        assert_eq!(
            backend.try_recv(),
            Some(FromBackend::Look {
                models: Vec::new(),
                model: None,
                usage: None
            })
        );
    }

    #[test]
    fn dropping_the_backend_closes_every_session() {
        let fake = FakeAgent::new();
        let (backend, _) = start(&fake);
        let ids: Vec<SessionId> = (1..=3).map(|open| started(&fake, &backend, open)).collect();
        drop(backend);
        eventually("every session being closed", || {
            let sent = fake.sent();
            ids.iter()
                .all(|id| sent.contains(&(id.clone(), SessionCommand::Close)))
        });
    }

    #[test]
    fn after_a_flush_the_worker_has_done_what_was_asked() {
        let fake = FakeAgent::new();
        let (backend, _) = start(&fake);
        backend.send(ToBackend::Open {
            open: 1,
            spec: StartSpec::new("/p"),
        });
        backend.flush();
        fake.advance(0.0);
        assert!(matches!(
            backend.try_recv(),
            Some(FromBackend::Event {
                open: 1,
                event: SessionEvent::Started(_)
            })
        ));
    }

    /// Every message the backend holds now.
    fn drain(backend: &Backend) -> Vec<FromBackend> {
        std::iter::from_fn(|| backend.try_recv()).collect()
    }

    #[test]
    fn a_read_that_is_dropped_before_its_turn_is_never_done() {
        let fake = FakeAgent::new();
        let (a, b) = (SessionId("stored-1".into()), SessionId("stored-2".into()));
        fake.set_history(&a, stored_history());
        fake.set_history(&b, stored_history());
        fake.pause_reads();
        let (backend, _) = start(&fake);
        for (key, id) in [(SessionKey(1), a), (SessionKey(2), b)] {
            backend.send(ToBackend::ReadHistory {
                key,
                id,
                max_items: 10,
            });
        }
        eventually("the first read to start", || fake.history_reads() == 1);
        backend.send(ToBackend::DropHistory { key: SessionKey(2) });
        fake.resume_reads();
        backend.flush();
        assert_eq!(fake.history_reads(), 1);
        let answers = drain(&backend);
        assert!(
            matches!(
                answers.as_slice(),
                [FromBackend::History {
                    key: SessionKey(1),
                    result: Ok(_),
                    ..
                }]
            ),
            "{answers:?}"
        );
    }

    #[test]
    fn a_read_dropped_while_it_ran_answers_nothing() {
        let fake = FakeAgent::new();
        let id = SessionId("stored-1".into());
        fake.set_history(&id, history_with_images(vec![Some(png(4, 4))]));
        fake.pause_reads();
        let (backend, _) = start(&fake);
        backend.send(ToBackend::ReadHistory {
            key: SessionKey(1),
            id,
            max_items: 10,
        });
        eventually("the read to start", || fake.history_reads() == 1);
        backend.send(ToBackend::DropHistory { key: SessionKey(1) });
        fake.resume_reads();
        backend.flush();
        assert_eq!(fake.history_reads(), 1);
        let answers = drain(&backend);
        assert!(answers.is_empty(), "{answers:?}");
    }

    #[test]
    fn a_picture_whose_history_was_dropped_is_not_made() {
        let fake = FakeAgent::new();
        let id = SessionId("stored-1".into());
        fake.set_history(
            &id,
            history_with_images(vec![Some(png(4, 4)), Some(png(5, 5))]),
        );
        // The first picture waits until the test lets it go; it says when it starts.
        let (entered, began) = mpsc::channel::<()>();
        let (release, gate) = mpsc::channel::<()>();
        let gate = Mutex::new(gate);
        let entered = Mutex::new(entered);
        let picture: Picturer = Arc::new(move |image: &ImageData| {
            let _ = entered.lock().unwrap().send(());
            let _ = gate.lock().unwrap().recv();
            attach::picture_of(image)
        });
        let backend = Backend::start_with(
            Arc::new(fake.clone()),
            Arc::new(|| {}),
            Arc::new(attach::prepare),
            picture,
        );
        read_history(&backend, SessionKey(1), &id);
        began.recv_timeout(LIMIT).expect("the first picture began");
        backend.send(ToBackend::DropHistory { key: SessionKey(1) });
        drop(release);
        backend.flush();
        let answers = drain(&backend);
        assert!(answers.is_empty(), "{answers:?}");
        assert!(began.try_recv().is_err(), "the second picture was made");
    }

    /// A stored history of one message of the user's, `m1`, with `images`.
    fn history_of_message(images: Vec<Option<ImageData>>) -> StoredHistory {
        let mut history = history_with_images(images);
        if let HistoryItem::User { stored, .. } = &mut history.items[0] {
            *stored = Some("m1".into());
        }
        history
    }

    #[test]
    fn an_image_read_comes_back_under_its_picture_id() {
        let fake = FakeAgent::new();
        let id = SessionId("stored-1".into());
        let (first, second) = (png(3, 3), png(4, 4));
        fake.set_history(
            &id,
            history_of_message(vec![Some(first), Some(second.clone())]),
        );
        let (backend, _) = start(&fake);
        backend.send(ToBackend::ReadImage {
            picture: STORED_PICTURE | 41,
            id,
            message: "m1".into(),
            index: 1,
        });
        match wait(&backend) {
            FromBackend::Image {
                picture,
                result: Ok(image),
            } => {
                assert_eq!(picture, STORED_PICTURE | 41);
                assert!(Arc::ptr_eq(&image.shared(), &second.shared()));
            }
            other => panic!("not the image: {other:?}"),
        }
    }

    #[test]
    fn a_failed_read_comes_back_as_its_error() {
        let fake = FakeAgent::new();
        let id = SessionId("stored-1".into());
        fake.set_history(&id, history_of_message(vec![Some(png(3, 3))]));
        let (backend, _) = start(&fake);
        let read = |picture: u64, message: &str| ToBackend::ReadImage {
            picture,
            id: id.clone(),
            message: message.into(),
            index: 0,
        };
        backend.send(read(7, "m9"));
        assert_eq!(
            wait(&backend),
            FromBackend::Image {
                picture: 7,
                result: Err(AgentError::Io("the message is not in the session".into())),
            }
        );
        fake.fail_image("the disk is gone");
        backend.send(read(8, "m1"));
        assert_eq!(
            wait(&backend),
            FromBackend::Image {
                picture: 8,
                result: Err(AgentError::Io("the disk is gone".into())),
            }
        );
    }
}
