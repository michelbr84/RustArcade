//! Launch a game as an interactive child process while the TUI is suspended.
//!
//! The sequence is: suspend the terminal session → run the child with inherited
//! stdio → wait → resume the session. A guard guarantees the resume step runs even
//! when spawning fails or a panic unwinds through the launcher.

use std::path::PathBuf;
use std::process::{Command, ExitStatus, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};

use crate::catalog::manifest::GameId;
use crate::error::LaunchError;
use crate::library::{ExitOutcome, PlaySession};

/// Hands terminal ownership back and forth around a child process.
pub trait TerminalSession {
    /// Leave the TUI (disable raw mode, leave the alternate screen).
    fn suspend(&mut self) -> Result<(), LaunchError>;
    /// Re-enter the TUI.
    fn resume(&mut self) -> Result<(), LaunchError>;
}

/// Session for the plain CLI: the terminal is already in normal mode.
#[derive(Debug, Default)]
pub struct NoTerminalSession;

impl TerminalSession for NoTerminalSession {
    fn suspend(&mut self) -> Result<(), LaunchError> {
        Ok(())
    }
    fn resume(&mut self) -> Result<(), LaunchError> {
        Ok(())
    }
}

/// Test double that records the order of calls and can fail on demand.
#[derive(Debug, Default)]
pub struct RecordingSession {
    pub events: Vec<&'static str>,
    pub fail_suspend: bool,
    pub fail_resume: bool,
}

impl TerminalSession for RecordingSession {
    fn suspend(&mut self) -> Result<(), LaunchError> {
        self.events.push("suspend");
        if self.fail_suspend {
            return Err(LaunchError::Terminal {
                stage: "suspend",
                source: std::io::Error::other("forced"),
            });
        }
        Ok(())
    }
    fn resume(&mut self) -> Result<(), LaunchError> {
        self.events.push("resume");
        if self.fail_resume {
            return Err(LaunchError::Terminal {
                stage: "resume",
                source: std::io::Error::other("forced"),
            });
        }
        Ok(())
    }
}

/// Ctrl+C / SIGTERM flag. While a child game runs the flag is ignored (and cleared
/// afterwards) so that RustArcade survives the interrupt the game receives.
#[derive(Debug, Clone, Default)]
pub struct InterruptFlag {
    interrupted: Arc<AtomicBool>,
    child_running: Arc<AtomicBool>,
}

impl InterruptFlag {
    pub fn new() -> InterruptFlag {
        InterruptFlag::default()
    }

    /// Install the process-wide signal handler (only the first call takes effect).
    pub fn install_handler(&self) {
        let flag = self.interrupted.clone();
        let child = self.child_running.clone();
        let _ = ctrlc::set_handler(move || {
            if !child.load(Ordering::SeqCst) {
                flag.store(true, Ordering::SeqCst);
            }
        });
    }

    /// Set the flag manually (tests, or the TUI treating Ctrl+C key events).
    pub fn raise(&self) {
        if !self.child_running.load(Ordering::SeqCst) {
            self.interrupted.store(true, Ordering::SeqCst);
        }
    }

    /// Return and clear the flag; always `false` while a child runs.
    pub fn take(&self) -> bool {
        if self.child_running.load(Ordering::SeqCst) {
            return false;
        }
        self.interrupted.swap(false, Ordering::SeqCst)
    }

    pub fn enter_child_mode(&self) {
        self.child_running.store(true, Ordering::SeqCst);
    }

    pub fn leave_child_mode(&self) {
        self.child_running.store(false, Ordering::SeqCst);
        self.interrupted.store(false, Ordering::SeqCst);
    }

    pub fn child_running(&self) -> bool {
        self.child_running.load(Ordering::SeqCst)
    }
}

/// Everything needed to start a game.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchSpec {
    pub game: GameId,
    pub executable: PathBuf,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    pub cwd: Option<PathBuf>,
}

/// Outcome of a play session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchResult {
    pub game: GameId,
    pub started_at: DateTime<Utc>,
    pub ended_at: DateTime<Utc>,
    pub duration: Duration,
    pub exit: ExitOutcome,
}

impl LaunchResult {
    pub fn session(&self) -> PlaySession {
        PlaySession {
            game: self.game.clone(),
            started_at: self.started_at,
            ended_at: self.ended_at,
            duration_secs: self.duration.as_secs(),
            exit: self.exit,
        }
    }
}

/// Map an [`ExitStatus`] to an [`ExitOutcome`].
pub fn exit_outcome(status: ExitStatus) -> ExitOutcome {
    if let Some(code) = status.code() {
        return ExitOutcome::Code { code };
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(signal) = status.signal() {
            return ExitOutcome::Signal { signal };
        }
    }
    ExitOutcome::Unknown
}

struct ResumeGuard<'a> {
    session: &'a mut dyn TerminalSession,
    interrupt: &'a InterruptFlag,
    done: bool,
}

impl ResumeGuard<'_> {
    fn finish(mut self) -> Result<(), LaunchError> {
        self.done = true;
        self.interrupt.leave_child_mode();
        self.session.resume()
    }
}

impl Drop for ResumeGuard<'_> {
    fn drop(&mut self) {
        if !self.done {
            self.interrupt.leave_child_mode();
            let _ = self.session.resume();
        }
    }
}

/// Run the game and wait for it to exit. The terminal session is always resumed.
pub fn launch(
    spec: &LaunchSpec,
    session: &mut dyn TerminalSession,
    interrupt: &InterruptFlag,
) -> Result<LaunchResult, LaunchError> {
    let meta = std::fs::metadata(&spec.executable).map_err(|_| LaunchError::MissingExecutable {
        path: spec.executable.clone(),
    })?;
    if !meta.is_file() {
        return Err(LaunchError::NotExecutable {
            path: spec.executable.clone(),
            reason: "not a regular file".into(),
        });
    }
    let mut cmd = Command::new(&spec.executable);
    cmd.args(&spec.args)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    for (k, v) in &spec.env {
        cmd.env(k, v);
    }
    if let Some(dir) = &spec.cwd
        && dir.is_dir()
    {
        cmd.current_dir(dir);
    }

    session.suspend()?;
    let guard = ResumeGuard {
        session,
        interrupt,
        done: false,
    };
    interrupt.enter_child_mode();
    let started_at = Utc::now();
    let clock = Instant::now();
    let mut child = cmd.spawn().map_err(|e| LaunchError::Spawn {
        path: spec.executable.clone(),
        source: e,
    })?;
    let status = child.wait().map_err(|e| LaunchError::Spawn {
        path: spec.executable.clone(),
        source: e,
    })?;
    let duration = clock.elapsed();
    let ended_at = Utc::now();
    guard.finish()?;
    Ok(LaunchResult {
        game: spec.game.clone(),
        started_at,
        ended_at,
        duration,
        exit: exit_outcome(status),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(exe: &str, args: &[&str]) -> LaunchSpec {
        LaunchSpec {
            game: "demo".parse().unwrap(),
            executable: PathBuf::from(exe),
            args: args.iter().map(|a| a.to_string()).collect(),
            env: vec![],
            cwd: None,
        }
    }

    #[test]
    fn interrupt_flag_ignored_during_child() {
        let flag = InterruptFlag::new();
        flag.raise();
        assert!(flag.take());
        assert!(!flag.take());
        flag.enter_child_mode();
        flag.raise();
        assert!(!flag.take());
        flag.leave_child_mode();
        assert!(!flag.take());
        assert!(!flag.child_running());
    }

    #[test]
    fn missing_executable_never_suspends() {
        let mut session = RecordingSession::default();
        let err = launch(
            &spec("/definitely/missing/game", &[]),
            &mut session,
            &InterruptFlag::new(),
        )
        .unwrap_err();
        assert!(matches!(err, LaunchError::MissingExecutable { .. }));
        assert!(session.events.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn exit_codes_and_session_order() {
        let mut session = RecordingSession::default();
        let flag = InterruptFlag::new();
        let ok = launch(&spec("/bin/sh", &["-c", "exit 0"]), &mut session, &flag).unwrap();
        assert!(ok.exit.success());
        let bad = launch(&spec("/bin/sh", &["-c", "exit 3"]), &mut session, &flag).unwrap();
        assert_eq!(bad.exit, ExitOutcome::Code { code: 3 });
        assert_eq!(
            session.events,
            vec!["suspend", "resume", "suspend", "resume"]
        );
        assert!(!flag.child_running());
        let sess = bad.session();
        assert_eq!(sess.game.as_str(), "demo");
    }

    #[cfg(unix)]
    #[test]
    fn signal_is_reported() {
        let mut session = RecordingSession::default();
        let r = launch(
            &spec("/bin/sh", &["-c", "kill -9 $$"]),
            &mut session,
            &InterruptFlag::new(),
        )
        .unwrap();
        assert_eq!(r.exit, ExitOutcome::Signal { signal: 9 });
    }

    #[cfg(unix)]
    #[test]
    fn spawn_failure_still_resumes() {
        let dir = tempfile::tempdir().unwrap();
        let not_exec = dir.path().join("plain.txt");
        std::fs::write(&not_exec, "not a program").unwrap();
        let mut session = RecordingSession::default();
        let flag = InterruptFlag::new();
        let err = launch(&spec(not_exec.to_str().unwrap(), &[]), &mut session, &flag).unwrap_err();
        assert!(matches!(err, LaunchError::Spawn { .. }), "{err:?}");
        assert_eq!(session.events, vec!["suspend", "resume"]);
        assert!(!flag.child_running());
    }

    #[cfg(unix)]
    #[test]
    fn resume_failure_is_reported() {
        let mut session = RecordingSession {
            fail_resume: true,
            ..Default::default()
        };
        let err = launch(
            &spec("/bin/sh", &["-c", "true"]),
            &mut session,
            &InterruptFlag::new(),
        )
        .unwrap_err();
        assert!(matches!(
            err,
            LaunchError::Terminal {
                stage: "resume",
                ..
            }
        ));
    }

    #[cfg(unix)]
    #[test]
    fn cwd_and_env_are_applied() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("out.txt");
        let mut s = spec(
            "/bin/sh",
            &["-c", "pwd > \"$OUT\"; echo \"$DEMO_VAR\" >> \"$OUT\""],
        );
        s.cwd = Some(dir.path().to_path_buf());
        s.env = vec![
            ("OUT".into(), out.display().to_string()),
            ("DEMO_VAR".into(), "hello".into()),
        ];
        launch(&s, &mut NoTerminalSession, &InterruptFlag::new()).unwrap();
        let text = std::fs::read_to_string(&out).unwrap();
        let canon = dir.path().canonicalize().unwrap();
        assert!(text.contains(&canon.display().to_string()), "{text}");
        assert!(text.contains("hello"));
    }
}
