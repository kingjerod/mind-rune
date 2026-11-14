//! The one worker thread between the window and the agent. It makes the
//! agent's blocking calls (`account`, `open`, `send`) so the window thread
//! never does, and queues what comes back for the window to take with
//! `try_recv`.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::Duration;

use mr_agent::{
    Account, Agent, AgentError, SessionCommand, SessionEvent, SessionHandle, StartSpec,
};

/// What the window asks of the worker. Mirrors the agent-facing `Effect`s.
#[derive(Clone, Debug, PartialEq)]
pub enum ToBackend {
    CheckAccount,
    Open { open: u64, spec: StartSpec },
    Send { open: u64, command: SessionCommand },
}

/// What the worker queues for the window. Everything that belongs to an open
/// carries its number, so the window can drop what is stale.
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
}

/// Called after every message is queued for the window, on whichever thread
/// queued it. It must be cheap and must not block.
pub type Wake = Arc<dyn Fn() + Send + Sync>;

enum Job {
    Message(ToBackend),
    /// Answered once every job before it has been handled.
    Flush(Sender<()>),
    /// Only wakes the worker so it notices that the `Backend` is gone.
    Stop,
}

/// The window's end of the worker. Neither `send` nor `try_recv` blocks: the
/// first pushes onto an unbounded channel and the second polls one.
pub struct Backend {
    jobs: Sender<Job>,
    inbox: Receiver<FromBackend>,
    stopping: Arc<AtomicBool>,
}

impl Backend {
    /// Starts the worker thread. `wake` is called after every message is queued for the window.
    pub fn start(agent: Arc<dyn Agent>, wake: Wake) -> Backend {
        let (jobs, job_queue) = mpsc::channel();
        let (outbox, inbox) = mpsc::channel();
        let stopping = Arc::new(AtomicBool::new(false));
        let worker = Worker {
            agent,
            outbox,
            wake,
            handles: HashMap::new(),
        };
        let flag = stopping.clone();
        thread::Builder::new()
            .name("mindrune-backend".into())
            .spawn(move || worker.run(job_queue, flag))
            .expect("the backend worker thread starts");
        Backend {
            jobs,
            inbox,
            stopping,
        }
    }

    /// Never blocks.
    pub fn send(&self, message: ToBackend) {
        // A send fails only once the worker has ended; there is nobody to tell.
        let _ = self.jobs.send(Job::Message(message));
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

    /// Waits until the worker has handled every message sent so far. For tests and
    /// the scripted demo only: with a real agent this can block for seconds.
    pub fn flush(&self) {
        let (done, handled) = mpsc::channel();
        if self.jobs.send(Job::Flush(done)).is_ok() {
            // An error means the worker ended first; either way it is finished.
            let _ = handled.recv();
        }
    }
}

impl Drop for Backend {
    /// Does not wait: an agent call may be blocked for seconds. The worker
    /// skips what is still queued, passes `Close` to every session it holds
    /// and ends on its own thread.
    fn drop(&mut self) {
        self.stopping.store(true, Ordering::SeqCst);
        let _ = self.jobs.send(Job::Stop);
    }
}

struct Worker {
    agent: Arc<dyn Agent>,
    outbox: Sender<FromBackend>,
    wake: Wake,
    handles: HashMap<u64, Box<dyn SessionHandle>>,
}

impl Worker {
    fn run(mut self, jobs: Receiver<Job>, stopping: Arc<AtomicBool>) {
        while let Ok(job) = jobs.recv() {
            if stopping.load(Ordering::SeqCst) {
                break;
            }
            match job {
                Job::Message(message) => self.handle(message),
                Job::Flush(done) => {
                    let _ = done.send(());
                }
                Job::Stop => break,
            }
        }
        for (_, handle) in self.handles.drain() {
            let _ = handle.send(SessionCommand::Close);
        }
    }

    fn handle(&mut self, message: ToBackend) {
        match message {
            ToBackend::CheckAccount => {
                let account = self.agent.account();
                self.queue(FromBackend::Account(account));
            }
            ToBackend::Open { open, spec } => {
                let outbox = self.outbox.clone();
                let wake = self.wake.clone();
                // Runs on the agent's own thread.
                let sink = Box::new(move |event| {
                    if outbox.send(FromBackend::Event { open, event }).is_ok() {
                        wake();
                    }
                });
                match self.agent.open(spec, sink) {
                    Ok(handle) => {
                        self.handles.insert(open, handle);
                    }
                    Err(error) => self.queue(FromBackend::OpenFailed { open, error }),
                }
            }
            ToBackend::Send { open, command } => {
                let result = match self.handles.get(&open) {
                    Some(handle) => handle.send(command.clone()),
                    None => Err(AgentError::Closed),
                };
                match result {
                    Ok(()) => {
                        if command == SessionCommand::Close {
                            self.handles.remove(&open);
                        }
                    }
                    Err(error) => {
                        if error == AgentError::Closed {
                            self.handles.remove(&open);
                        }
                        self.queue(FromBackend::SendFailed {
                            open,
                            command,
                            error,
                        });
                    }
                }
            }
        }
    }

    fn queue(&self, message: FromBackend) {
        if self.outbox.send(message).is_ok() {
            (self.wake)();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mr_agent::{
        Account, Agent, AgentError, Answer, Capabilities, EventSink, FakeAgent, HistoryItem,
        RequestId, SessionCommand, SessionEvent, SessionHandle, SessionId, StartSpec,
        StoredSession,
    };
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

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
            fn read_session(&self, _: &SessionId) -> Result<Vec<HistoryItem>, AgentError> {
                Ok(Vec::new())
            }
            fn open(
                &self,
                _: StartSpec,
                _: EventSink,
            ) -> Result<Box<dyn SessionHandle>, AgentError> {
                Err(AgentError::NotInstalled)
            }
        }
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
