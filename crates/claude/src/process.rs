//! A child process with line-oriented I/O, built on std threads and channels
//! only. Everything here is safe to call from a worker thread.

use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::time::Duration;

use mr_agent::AgentError;

/// How many trailing bytes of the child's stderr are kept for [`ExitInfo`].
const STDERR_TAIL_BYTES: usize = 4096;
/// How often the exit watcher looks at a child whose output has ended.
const EXIT_POLL: Duration = Duration::from_millis(5);

/// What to run. The child sees exactly `env` and nothing from this process.
pub struct SpawnConfig {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    pub env: Vec<(String, String)>,
}

/// How the child ended. `code` is `None` when it was stopped by a signal.
pub struct ExitInfo {
    pub code: Option<i32>,
    pub stderr_tail: String,
}

/// A running child. Dropping it kills the child.
pub struct ClaudeProcess {
    stdin: Mutex<Option<ChildStdin>>,
    child: Arc<Mutex<Child>>,
    closed: Arc<AtomicBool>,
}

impl ClaudeProcess {
    /// Starts the program. `on_line` gets each stdout line (no newline, decoded
    /// lossily, any length) on a reader thread. `on_exit` is called once, on that
    /// same thread, after the last line.
    pub fn spawn(
        config: SpawnConfig,
        on_line: impl Fn(String) + Send + 'static,
        on_exit: impl FnOnce(ExitInfo) + Send + 'static,
    ) -> Result<ClaudeProcess, AgentError> {
        let mut child = Command::new(&config.program)
            .args(&config.args)
            .current_dir(&config.cwd)
            .env_clear()
            .envs(config.env.iter().map(|(k, v)| (k, v)))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| spawn_error(e, &config))?;

        let stdin = child.stdin.take().expect("stdin was piped");
        let stdout = child.stdout.take().expect("stdout was piped");
        let stderr = child.stderr.take().expect("stderr was piped");
        let child = Arc::new(Mutex::new(child));
        let closed = Arc::new(AtomicBool::new(false));

        let stderr_thread = thread::spawn(move || read_tail(stderr));
        let watcher_child = Arc::clone(&child);
        let watcher_closed = Arc::clone(&closed);
        thread::spawn(move || {
            read_lines(stdout, on_line);
            let status = wait_for_exit(&watcher_child);
            let stderr_tail = stderr_thread.join().unwrap_or_default();
            watcher_closed.store(true, Ordering::SeqCst);
            on_exit(ExitInfo {
                code: status.and_then(|s| s.code()),
                stderr_tail,
            });
        });

        Ok(ClaudeProcess {
            stdin: Mutex::new(Some(stdin)),
            child,
            closed,
        })
    }

    /// Writes `line` and a newline to the child's stdin and flushes. Returns
    /// [`AgentError::Closed`] once the child has exited or input was closed.
    pub fn write_line(&self, line: &str) -> Result<(), AgentError> {
        let mut guard = lock(&self.stdin);
        if self.closed.load(Ordering::SeqCst) {
            return Err(AgentError::Closed);
        }
        let stdin = guard.as_mut().ok_or(AgentError::Closed)?;
        let mut bytes = Vec::with_capacity(line.len() + 1);
        bytes.extend_from_slice(line.as_bytes());
        bytes.push(b'\n');
        stdin
            .write_all(&bytes)
            .and_then(|()| stdin.flush())
            .map_err(|e| match e.kind() {
                std::io::ErrorKind::BrokenPipe => AgentError::Closed,
                _ => AgentError::Io(e.to_string()),
            })
    }

    /// Closes the child's stdin, which ends a child waiting for input.
    pub fn close_input(&self) {
        lock(&self.stdin).take();
    }

    /// Kills the child. The exit is still reported through `on_exit`.
    pub fn kill(&self) {
        // Failure means the child is already gone, which is what was asked for.
        let _ = lock(&self.child).kill();
    }
}

impl Drop for ClaudeProcess {
    fn drop(&mut self) {
        self.close_input();
        self.kill();
    }
}

fn spawn_error(error: std::io::Error, config: &SpawnConfig) -> AgentError {
    if error.kind() != std::io::ErrorKind::NotFound {
        return AgentError::Io(error.to_string());
    }
    // `spawn` reports a missing working directory the same way as a missing program.
    if config.cwd.is_dir() {
        AgentError::NotInstalled
    } else {
        AgentError::Io(format!(
            "working directory {} does not exist",
            config.cwd.display()
        ))
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Delivers each newline-terminated line, and a final unterminated one.
fn read_lines(stdout: impl Read, on_line: impl Fn(String)) {
    let mut reader = BufReader::new(stdout);
    let mut buf = Vec::new();
    loop {
        buf.clear();
        match reader.read_until(b'\n', &mut buf) {
            Ok(0) | Err(_) => return,
            Ok(_) => {
                if buf.last() == Some(&b'\n') {
                    buf.pop();
                }
                on_line(String::from_utf8_lossy(&buf).into_owned());
            }
        }
    }
}

/// Reads to the end and returns the last [`STDERR_TAIL_BYTES`] bytes, lossily decoded.
fn read_tail(mut stderr: impl Read) -> String {
    let mut tail: VecDeque<u8> = VecDeque::with_capacity(STDERR_TAIL_BYTES);
    let mut chunk = [0u8; 4096];
    loop {
        match stderr.read(&mut chunk) {
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Ok(0) | Err(_) => break,
            Ok(n) => {
                tail.extend(&chunk[..n]);
                let excess = tail.len().saturating_sub(STDERR_TAIL_BYTES);
                tail.drain(..excess);
            }
        }
    }
    let bytes: Vec<u8> = tail.into_iter().collect();
    String::from_utf8_lossy(&bytes).into_owned()
}

/// Polls rather than blocking in `wait`, so `kill` can take the lock meanwhile.
fn wait_for_exit(child: &Mutex<Child>) -> Option<ExitStatus> {
    loop {
        match lock(child).try_wait() {
            Ok(Some(status)) => return Some(status),
            Ok(None) => {}
            Err(_) => return None,
        }
        thread::sleep(EXIT_POLL);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Fails with `Interrupted` once, then yields its bytes.
    struct InterruptedOnce {
        interrupted: bool,
        bytes: &'static [u8],
    }

    impl Read for InterruptedOnce {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            if !self.interrupted {
                self.interrupted = true;
                return Err(std::io::ErrorKind::Interrupted.into());
            }
            let n = self.bytes.len().min(buf.len());
            buf[..n].copy_from_slice(&self.bytes[..n]);
            self.bytes = &self.bytes[n..];
            Ok(n)
        }
    }

    #[test]
    fn an_interrupted_read_of_stderr_is_retried() {
        let stderr = InterruptedOnce {
            interrupted: false,
            bytes: b"why it died",
        };
        assert_eq!(read_tail(stderr), "why it died");
    }
}
