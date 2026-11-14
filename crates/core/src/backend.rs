//! The threads between the window and the agent. They make the agent's blocking calls so the
//! window thread never does, and queue what comes back for the window to take with
//! `try_recv`.
//!
//! Every open has a thread of its own, which makes that open's `Agent::open` call and every
//! `send` for it, in order; it ends with its session. Every look has a thread of its own too.
//! One more thread, the library thread, makes the other calls one at a time in the order they
//! were sent: the account, the listing, a history, an activity time. So a slow call holds up
//! only what was sent after it to the same thread.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, TryRecvError};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use mr_agent::{
    Account, Agent, AgentError, ModelOption, SessionCommand, SessionEvent, SessionHandle,
    SessionId, StartSpec, StoredHistory, StoredSession, UsageLimits,
};

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
    CheckActivity {
        key: SessionKey,
        id: SessionId,
    },
    /// Start a session in `folder`, learn the models and the usage limits, close it.
    Look {
        folder: PathBuf,
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
    History {
        key: SessionKey,
        result: Result<StoredHistory, AgentError>,
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
}

/// Called after every message is queued for the window, on whichever thread
/// queued it. It must be cheap and must not block.
pub type Wake = Arc<dyn Fn() + Send + Sync>;

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
    library: Sender<LibraryJob>,
    lanes: RefCell<HashMap<u64, Arc<Lane>>>,
    looks: RefCell<Vec<LookThread>>,
    /// How many opens' threads are running.
    open_threads: Arc<AtomicUsize>,
}

impl Backend {
    pub const LOOK_START_LIMIT: Duration = Duration::from_secs(20);
    pub const LOOK_USAGE_LIMIT: Duration = Duration::from_secs(10);

    /// Starts the library thread. `wake` is called after every message is queued for the
    /// window.
    pub fn start(agent: Arc<dyn Agent>, wake: Wake) -> Backend {
        let (sender, inbox) = mpsc::channel();
        let outbox = Outbox { sender, wake };
        let stopping = Arc::new(AtomicBool::new(false));
        let (library, jobs) = mpsc::channel();
        let thread = Library {
            agent: agent.clone(),
            outbox: outbox.clone(),
            stopping: stopping.clone(),
        };
        thread::Builder::new()
            .name("mindrune-library".into())
            .spawn(move || thread.run(jobs))
            .expect("the backend's library thread starts");
        Backend {
            agent,
            outbox,
            inbox,
            stopping,
            library,
            lanes: RefCell::new(HashMap::new()),
            looks: RefCell::new(Vec::new()),
            open_threads: Arc::new(AtomicUsize::new(0)),
        }
    }

    /// Never blocks. A `Send` for an open that has no thread is answered `SendFailed` with
    /// `AgentError::Closed`.
    pub fn send(&self, message: ToBackend) {
        match message {
            ToBackend::CheckAccount => self.ask_library(LibraryJob::Account),
            ToBackend::ListSessions { since_unix } => {
                self.ask_library(LibraryJob::Sessions { since_unix })
            }
            ToBackend::ReadHistory { key, id, max_items } => {
                self.ask_library(LibraryJob::History { key, id, max_items })
            }
            ToBackend::CheckActivity { key, id } => {
                self.ask_library(LibraryJob::Activity { key, id })
            }
            ToBackend::Open { open, spec } => self.open(open, spec),
            ToBackend::Send { open, command } => self.pass(open, command),
            ToBackend::Look { folder } => self.look(folder),
        }
    }

    /// Never blocks.
    pub fn try_recv(&self) -> Option<FromBackend> {
        self.inbox.try_recv().ok()
    }

    /// Waits up to `timeout` for the next message. This one blocks: it is only for the window
    /// closing, which waits a bounded time for its session to end (see `glue::close_session`).
    pub fn recv_timeout(&self, timeout: Duration) -> Option<FromBackend> {
        self.inbox.recv_timeout(timeout).ok()
    }

    /// Waits until every thread has handled every message sent so far, looks included. For
    /// tests and the scripted demo only: with a real agent this can block for seconds.
    pub fn flush(&self) {
        // Every open's thread and the library thread answer their copy of `token` when they
        // reach it, or drop it unanswered when they end first: either way everything sent to
        // them before it was handled. `recv` fails once the last copy is gone.
        let (token, handled) = mpsc::channel();
        for lane in self.lanes.borrow().values() {
            let _ = lane.offer(OpenJob::Flush(token.clone()));
        }
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
        // The library thread ends when `library` is dropped with this.
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
    Flush(FlushToken),
}

struct Library {
    agent: Arc<dyn Agent>,
    outbox: Outbox,
    stopping: Arc<AtomicBool>,
}

impl Library {
    /// Ends when the `Backend`, which holds the only sending end of `jobs`, is dropped.
    fn run(self, jobs: Receiver<LibraryJob>) {
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
                LibraryJob::History { key, id, max_items } => FromBackend::History {
                    key,
                    result: self.agent.read_session(&id, max_items),
                },
                LibraryJob::Activity { key, id } => FromBackend::Activity {
                    key,
                    result: self.agent.last_active(&id),
                },
                LibraryJob::Flush(token) => {
                    let _ = token.send(());
                    continue;
                }
            };
            self.outbox.queue(answer);
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
        match next(fake, backend) {
            FromBackend::Event {
                open: got,
                event: SessionEvent::Started(info),
            } if got == open => info.id,
            other => panic!("{other:?}"),
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
                },
                HistoryItem::AssistantText("Done, alex.".into()),
            ],
            earlier_left_out: false,
            model: Some("fake-model".into()),
            permission_mode: Some(PermissionMode::Ask),
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
                result: Ok(stored_history())
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
                result: Ok(stored_history())
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
}
