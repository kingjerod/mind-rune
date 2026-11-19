//! Desktop notifications: a session that is ready or needs input while the user is not looking
//! is told to the desktop, and a press on the notification shows that session. On Linux they go
//! over the session bus, from a thread of their own that reaches the app only through its handle.
// Elsewhere only the tests use the pure parts.
#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

use std::collections::HashMap;
use std::sync::mpsc::Sender;

use mr_core::{Alert, SessionKey};
use rune::AppHandle;

use crate::glue::Ui;

/// What the notifier is told to do.
#[derive(Clone, Debug, PartialEq)]
pub enum Note {
    /// Show a notification about `key`'s session, in place of one about it that is out.
    Show {
        key: SessionKey,
        title: String,
        alert: Alert,
    },
    /// Take back the notification about `key`'s session.
    Close { key: SessionKey },
    /// Take back every notification.
    CloseAll,
}

/// The words of a notification: its title as typed, its text.
pub fn words(title: &str, alert: Alert) -> (String, &'static str) {
    let title = if title.is_empty() { "MindRune" } else { title };
    let text = match alert {
        Alert::Ready => "Finished.",
        Alert::NeedsInput => "Needs your input.",
    };
    (title.to_string(), text)
}

/// `&`, `<` and `>` made safe for a text that is read as markup.
pub fn escape(text: &str) -> String {
    let mut safe = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => safe.push_str("&amp;"),
            '<' => safe.push_str("&lt;"),
            '>' => safe.push_str("&gt;"),
            c => safe.push(c),
        }
    }
    safe
}

/// Which notification belongs to which session, and the token of a press.
#[derive(Default)]
pub struct Book {
    ids: HashMap<SessionKey, u32>,
    tokens: HashMap<u32, String>,
}

impl Book {
    /// The id a new notification about `key`'s session replaces: 0 when none is out.
    pub fn replaces(&self, key: SessionKey) -> u32 {
        self.id_of(key).unwrap_or(0)
    }

    /// A notification about `key`'s session is out as `id`. A token kept for the one it
    /// replaced, or for `id` before, is not for this one.
    pub fn shown(&mut self, key: SessionKey, id: u32) {
        if let Some(old) = self.ids.insert(key, id) {
            self.tokens.remove(&old);
        }
        self.tokens.remove(&id);
    }

    /// The id of the notification about `key`'s session, when one is out.
    pub fn id_of(&self, key: SessionKey) -> Option<u32> {
        self.ids.get(&key).copied()
    }

    /// Every notification out, lowest id first.
    pub fn all(&self) -> Vec<u32> {
        let mut ids: Vec<u32> = self.ids.values().copied().collect();
        ids.sort_unstable();
        ids
    }

    /// The desktop's token for a press on `id`, which comes just before the press. One for an
    /// id that is not out is not kept.
    pub fn token(&mut self, id: u32, token: String) {
        if self.ids.values().any(|&out| out == id) {
            self.tokens.insert(id, token);
        }
    }

    /// A press on `id`: its session, and its token when one came. The desktop takes a pressed
    /// notification away, so it is forgotten.
    pub fn pressed(&mut self, id: u32) -> Option<(SessionKey, Option<String>)> {
        let key = self
            .ids
            .iter()
            .find(|&(_, &out)| out == id)
            .map(|(&key, _)| key)?;
        self.ids.remove(&key);
        Some((key, self.tokens.remove(&id)))
    }

    /// `id` is gone from the desktop: it and its token are forgotten.
    pub fn closed(&mut self, id: u32) {
        self.ids.retain(|_, out| *out != id);
        self.tokens.remove(&id);
    }
}

/// Whether a signal from `sender` is the notification service's: the one that answered the last
/// `Notify`, `server`. Before any answer no signal counts.
pub fn from_server(sender: Option<&str>, server: Option<&str>) -> bool {
    matches!((sender, server), (Some(sender), Some(server)) if sender == server)
}

/// Whether a close of an id, received at `closed`, takes away the notification last shown as
/// that id, whose answer was received at `shown`. One received before that answer was about an
/// earlier showing.
pub fn closes_current<P: Ord>(closed: P, shown: Option<P>) -> bool {
    shown.is_none_or(|shown| closed > shown)
}

/// A press on a notification, on the notifier's thread: its session is shown and takes the
/// keyboard, and the window comes to the front with the press's token. A press the book did
/// not know only shows the window.
fn hand_over(handle: &AppHandle<Ui>, press: Option<(SessionKey, Option<String>)>) {
    let token = press.and_then(|(key, token)| {
        handle.update(move |ui, tx| crate::glue::alert_pressed(ui, tx, key));
        token
    });
    crate::show(handle, token);
}

/// Starts the notifier on its own thread. `None` where there is none (not Linux).
pub fn start(handle: AppHandle<Ui>) -> Option<Sender<Note>> {
    #[cfg(target_os = "linux")]
    return Some(bus::start(handle));
    #[cfg(not(target_os = "linux"))]
    {
        drop(handle);
        None
    }
}

/// The notifier on the session bus, as GNOME and the other Linux desktops serve
/// `org.freedesktop.Notifications`. Built as Rune's tray worker is: a thread, a current-thread
/// tokio runtime, one connection. GNOME sends the signals of a notification only to the
/// connection that sent it, and takes a replacement or a take-back only from that connection,
/// so there is one for the life of the app. Quitting takes every notification back first
/// (`glue::quitting` sends [`Note::CloseAll`]) and then closes it: a desktop takes a gone
/// sender's notifications away by itself only where it tied them to an app.
#[cfg(target_os = "linux")]
mod bus {
    use std::collections::HashMap;
    use std::fmt;
    use std::num::NonZeroUsize;
    use std::sync::mpsc::{Receiver, Sender};
    use std::time::Duration;

    use futures_util::StreamExt;
    use mr_core::Alert;
    use rune::AppHandle;
    use zbus::message::{Sequence, Type};
    use zbus::{Connection, MatchRule, Message, MessageStream, Variant};

    use super::{Book, Note, closes_current, escape, from_server, hand_over, words};
    use crate::glue::Ui;

    const NAME: &str = "org.freedesktop.Notifications";
    const PATH: &str = "/org/freedesktop/Notifications";
    /// How long the connection, and each call, may take.
    const TIMEOUT: Duration = Duration::from_secs(5);
    /// How many signals may wait while a call is out.
    const QUEUE: NonZeroUsize = NonZeroUsize::new(256).unwrap();

    pub fn start(handle: AppHandle<Ui>) -> Sender<Note> {
        let (tell, told) = std::sync::mpsc::channel();
        let spawned = std::thread::Builder::new()
            .name("mindrune-notify".into())
            .spawn(move || serve(handle, told));
        if let Err(error) = spawned {
            eprintln!("mindrune: no desktop notifications: {error}");
        }
        tell
    }

    /// The notifier's thread. Where no connection can be made it says so once and ends; the
    /// notes sent afterwards are dropped with the receiver.
    fn serve(handle: AppHandle<Ui>, told: Receiver<Note>) {
        let runtime = match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            Ok(runtime) => runtime,
            Err(error) => {
                eprintln!("mindrune: no desktop notifications: {error}");
                return;
            }
        };
        runtime.block_on(listen(&handle, told));
        // The pass below may still wait on the app's sender: it is not waited for.
        runtime.shutdown_background();
    }

    /// What the notifier knows of the desktop besides the book: who answers, and when each id
    /// was last shown.
    #[derive(Default)]
    struct Desk {
        book: Book,
        /// The unique name that answered the last `Notify`: signals are taken only from it.
        server: Option<String>,
        /// When the answer that last showed each id was received.
        shown_at: HashMap<u32, Sequence>,
    }

    impl Desk {
        /// `id` is forgotten, in the book and here.
        fn closed(&mut self, id: u32) {
            self.book.closed(id);
            self.shown_at.remove(&id);
        }
    }

    /// Makes the connection, then carries out the notes in the order they come and hears the
    /// desktop's signals in between. Ends when the app's sender is gone or the connection
    /// closes.
    async fn listen(handle: &AppHandle<Ui>, told: Receiver<Note>) {
        let setup = async {
            let conn = Connection::session().await?;
            // The bus sends only the service's broadcasts; its unicasts are checked in `heard`.
            let rule = MatchRule::builder()
                .msg_type(Type::Signal)
                .sender(NAME)
                .interface(NAME)
                .path(PATH)
                .build()?;
            let signals = MessageStream::for_match_rule(rule, &conn, Some(QUEUE)).await?;
            Ok::<_, zbus::Error>((conn, signals))
        };
        let (conn, mut signals) = match tokio::time::timeout(TIMEOUT, setup).await {
            Ok(Ok(made)) => made,
            Ok(Err(error)) => {
                eprintln!("mindrune: no desktop notifications: {error}");
                return;
            }
            Err(_) => {
                eprintln!("mindrune: no desktop notifications: the session bus did not answer");
                return;
            }
        };
        // Notes sent before the connection was up waited in `told`; they come through here.
        let (pass, mut notes) = tokio::sync::mpsc::unbounded_channel();
        tokio::task::spawn_blocking(move || {
            for note in told {
                if pass.send(note).is_err() {
                    break;
                }
            }
        });
        let mut desk = Desk::default();
        loop {
            tokio::select! {
                note = notes.recv() => {
                    let Some(note) = note else { break };
                    // The order of the notes is kept here: each is carried out to its answer,
                    // which a `Show` writes in the book, before the next is taken. So a
                    // take-back finds the id of the notification shown before it.
                    carry_out(&conn, &mut desk, note).await;
                }
                signal = signals.next() => {
                    let Some(signal) = signal else {
                        eprintln!("mindrune: no desktop notifications: the session bus closed");
                        break;
                    };
                    if let Ok(signal) = signal {
                        heard(handle, &mut desk, &signal);
                    }
                }
            }
        }
        let _ = conn.close().await;
    }

    async fn carry_out(conn: &Connection, desk: &mut Desk, note: Note) {
        match note {
            Note::Show { key, title, alert } => {
                let replaces = desk.book.replaces(key);
                let mut answer = notify(conn, replaces, &title, alert).await;
                if matches!(answer, Err(Failure::Refused(_))) && replaces != 0 {
                    // The service refused: the one it replaces may be gone. It is shown anew.
                    // Only after its own error reply: after a failure on the way, as after no
                    // answer, the first may be out, and a second could not be taken back.
                    desk.closed(replaces);
                    answer = notify(conn, 0, &title, alert).await;
                }
                match answer {
                    Ok(reply) => {
                        let id = reply.body().deserialize::<u32>();
                        let Ok(id) = id else {
                            eprintln!("mindrune: a desktop notification failed: a bad answer");
                            return;
                        };
                        desk.server = reply.header().sender().map(|name| name.to_string());
                        desk.shown_at.insert(id, reply.recv_position());
                        desk.book.shown(key, id);
                    }
                    // After no answer the notification may be out all the same: what the book
                    // knew stays.
                    Err(failure) => {
                        eprintln!("mindrune: a desktop notification failed: {failure}");
                    }
                }
            }
            Note::Close { key } => {
                if let Some(id) = desk.book.id_of(key) {
                    desk.closed(id);
                    let _ = call(conn, "CloseNotification", &(id,)).await;
                }
            }
            Note::CloseAll => {
                for id in desk.book.all() {
                    desk.closed(id);
                    let _ = call(conn, "CloseNotification", &(id,)).await;
                }
            }
        }
    }

    /// Why a call has no answer to read.
    enum Failure {
        /// The service answered with an error: it did not do what was asked.
        Refused(zbus::Error),
        /// The call failed on the way (the connection, the message): what the service did,
        /// if anything, is not known.
        Failed(zbus::Error),
        /// No answer came within [`TIMEOUT`].
        Unanswered(&'static str),
    }

    impl Failure {
        /// What a failed call means: only an error reply of the service says it refused.
        fn of(error: zbus::Error) -> Failure {
            match error {
                zbus::Error::MethodError(..) => Failure::Refused(error),
                error => Failure::Failed(error),
            }
        }
    }

    impl fmt::Display for Failure {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            match self {
                Failure::Refused(error) | Failure::Failed(error) => write!(f, "{error}"),
                Failure::Unanswered(method) => write!(f, "{method} had no answer"),
            }
        }
    }

    /// Shows a notification, in place of `replaces` when that is not 0. The answer holds its
    /// id.
    async fn notify(
        conn: &Connection,
        replaces: u32,
        title: &str,
        alert: Alert,
    ) -> Result<Message, Failure> {
        let (summary, text) = words(title, alert);
        // The summary is plain text; the body is read as markup.
        let body = escape(text);
        let hints = HashMap::from([
            ("desktop-entry", Variant::from("mindrune")),
            ("urgency", Variant::U8(1)),
        ]);
        let args = (
            "MindRune",
            replaces,
            "mindrune",
            summary.as_str(),
            body.as_str(),
            vec!["default", "Open"],
            hints,
            -1i32,
        );
        call(conn, "Notify", &args).await
    }

    /// Calls `method` of the desktop's notifications, giving up after [`TIMEOUT`].
    async fn call<B>(conn: &Connection, method: &'static str, args: &B) -> Result<Message, Failure>
    where
        B: serde::Serialize + zbus::DynamicType,
    {
        let call = conn.call_method(Some(NAME), PATH, Some(NAME), method, args);
        match tokio::time::timeout(TIMEOUT, call).await {
            Ok(Ok(reply)) => Ok(reply),
            Ok(Err(error)) => Err(Failure::of(error)),
            Err(_) => Err(Failure::Unanswered(method)),
        }
    }

    /// A signal of the desktop's notifications, taken only from the service that answered the
    /// last `Notify`. The token of a press comes just before it.
    fn heard(handle: &AppHandle<Ui>, desk: &mut Desk, signal: &Message) {
        let header = signal.header();
        let sender = header.sender().map(|name| name.as_str());
        if !from_server(sender, desk.server.as_deref()) {
            return;
        }
        let Some(member) = header.member() else {
            return;
        };
        let body = signal.body();
        match member.as_str() {
            "ActivationToken" => {
                if let Ok((id, token)) = body.deserialize::<(u32, String)>() {
                    desk.book.token(id, token);
                }
            }
            "ActionInvoked" => {
                if let Ok((id, action)) = body.deserialize::<(u32, String)>()
                    && action == "default"
                {
                    let press = desk.book.pressed(id);
                    desk.shown_at.remove(&id);
                    hand_over(handle, press);
                }
            }
            "NotificationClosed" => {
                if let Ok((id, _reason)) = body.deserialize::<(u32, u32)>()
                    && closes_current(signal.recv_position(), desk.shown_at.get(&id).copied())
                {
                    desk.closed(id);
                }
            }
            _ => {}
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn only_an_error_reply_counts_as_a_refusal() {
            // Built by hand: no bus is asked.
            let asked = Message::method_call(PATH, "Notify").build(&()).unwrap();
            let name = "org.freedesktop.DBus.Error.Failed";
            let reply = Message::error(&asked.header(), name)
                .build(&("no such notification",))
                .unwrap();
            let refused = zbus::Error::MethodError(
                name.try_into().unwrap(),
                Some("no such notification".into()),
                reply,
            );
            assert!(matches!(Failure::of(refused), Failure::Refused(_)));
            // Anything else went wrong on the way: the notification may be out.
            let broken = std::io::Error::from(std::io::ErrorKind::BrokenPipe);
            for local in [
                zbus::Error::InputOutput(std::sync::Arc::new(broken)),
                zbus::Error::Failure("the connection is closed".into()),
                zbus::Error::InvalidReply,
            ] {
                assert!(matches!(Failure::of(local), Failure::Failed(_)));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_words_of_each_alert() {
        assert_eq!(
            words("Tidy the notes", Alert::Ready),
            ("Tidy the notes".to_string(), "Finished.")
        );
        assert_eq!(
            words("Tidy the notes", Alert::NeedsInput),
            ("Tidy the notes".to_string(), "Needs your input.")
        );
    }

    #[test]
    fn a_title_is_shown_as_typed() {
        // The summary is plain text: the title goes in unchanged.
        assert_eq!(words("<b>a & b</b>", Alert::Ready).0, "<b>a & b</b>");
        // Where a text is read as markup, it is escaped.
        assert_eq!(escape("<b>a & b</b>"), "&lt;b&gt;a &amp; b&lt;/b&gt;");
        assert_eq!(escape("Finished."), "Finished.");
    }

    #[test]
    fn an_empty_title_reads_mindrune() {
        assert_eq!(words("", Alert::Ready).0, "MindRune");
    }

    #[test]
    fn a_second_notification_for_a_session_replaces_the_first() {
        let (a, b) = (SessionKey(1), SessionKey(2));
        let mut book = Book::default();
        assert_eq!(book.replaces(a), 0);
        book.shown(a, 7);
        assert_eq!(book.replaces(a), 7);
        assert_eq!(book.id_of(a), Some(7));
        assert_eq!(book.replaces(b), 0, "another session's is its own");
        assert_eq!(book.id_of(b), None);
    }

    #[test]
    fn a_press_gives_the_session_and_its_token() {
        let key = SessionKey(1);
        let mut book = Book::default();
        book.shown(key, 7);
        book.token(7, "token-1".into());
        assert_eq!(book.pressed(7), Some((key, Some("token-1".into()))));
        // The desktop takes a pressed notification away: there is nothing left to take back.
        assert_eq!(book.id_of(key), None);
        assert_eq!(book.pressed(7), None);
    }

    #[test]
    fn a_press_without_a_token_still_gives_the_session() {
        let key = SessionKey(1);
        let mut book = Book::default();
        book.shown(key, 7);
        assert_eq!(book.pressed(7), Some((key, None)));
    }

    #[test]
    fn a_press_on_an_unknown_id_gives_nothing() {
        let mut book = Book::default();
        book.shown(SessionKey(1), 7);
        book.token(8, "token-1".into());
        assert_eq!(book.pressed(8), None);
        assert_eq!(book.id_of(SessionKey(1)), Some(7), "the known one stays");
    }

    #[test]
    fn a_closed_notification_is_forgotten() {
        let key = SessionKey(1);
        let mut book = Book::default();
        book.shown(key, 7);
        book.token(7, "token-1".into());
        book.closed(7);
        assert_eq!(book.id_of(key), None);
        assert_eq!(book.replaces(key), 0);
        assert!(book.all().is_empty());
        // Its token went with it: a later notification of the same id has none.
        book.shown(key, 7);
        assert_eq!(book.pressed(7), Some((key, None)));
    }

    #[test]
    fn the_book_gives_the_id_of_what_was_shown() {
        let key = SessionKey(1);
        let mut book = Book::default();
        book.shown(key, 7);
        assert_eq!(book.id_of(key), Some(7));
        // One whose notification failed has none: the take-back behind it does nothing.
        assert_eq!(book.id_of(SessionKey(2)), None);
    }

    #[test]
    fn a_token_from_before_a_replace_is_not_used_after_it() {
        let key = SessionKey(1);
        let mut book = Book::default();
        book.shown(key, 7);
        book.token(7, "token-1".into());
        // Replaced in place: the desktop gives the same id back.
        book.shown(key, 7);
        assert_eq!(book.pressed(7), Some((key, None)));
    }

    #[test]
    fn a_signal_from_another_sender_is_dropped() {
        let server = Some(":1.42");
        assert!(from_server(Some(":1.42"), server));
        assert!(!from_server(Some(":1.99"), server));
        assert!(!from_server(None, server));
    }

    #[test]
    fn before_any_notification_no_signal_counts() {
        assert!(!from_server(Some(":1.42"), None));
        assert!(!from_server(None, None));
    }

    #[test]
    fn a_close_from_before_a_replace_is_ignored() {
        // The answer that last showed the id came at 5.
        assert!(!closes_current(3, Some(5)));
        assert!(closes_current(6, Some(5)));
        // Nothing known of its showing: the close counts.
        assert!(closes_current(3, None));
    }

    #[test]
    fn close_all_names_every_id() {
        let mut book = Book::default();
        book.shown(SessionKey(2), 9);
        book.shown(SessionKey(1), 7);
        book.shown(SessionKey(3), 8);
        // Shown again: the session's id is the new one, once.
        book.shown(SessionKey(3), 10);
        assert_eq!(book.all(), vec![7, 9, 10]);
    }
}
