//! Run external tools (cargo, git) with captured output, logging, and cancellation.
//!
//! Every invocation is a direct `Command` with an explicit argument vector — never a
//! shell — and the child's stdin is closed so nothing can prompt interactively.

use std::collections::VecDeque;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::error::InstallError;
use crate::logging::InstallLog;

const KEEP_LAST_LINES: usize = 20;

/// A fully specified command line.
#[derive(Debug, Clone)]
pub struct CommandSpec {
    pub program: PathBuf,
    pub args: Vec<OsString>,
    pub cwd: Option<PathBuf>,
    pub env: Vec<(String, String)>,
}

impl CommandSpec {
    pub fn new(program: impl Into<PathBuf>) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
            cwd: None,
            env: Vec::new(),
        }
    }

    pub fn arg(mut self, a: impl Into<OsString>) -> Self {
        self.args.push(a.into());
        self
    }

    pub fn args<I, S>(mut self, iter: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<OsString>,
    {
        self.args.extend(iter.into_iter().map(Into::into));
        self
    }

    pub fn cwd(mut self, dir: impl Into<PathBuf>) -> Self {
        self.cwd = Some(dir.into());
        self
    }

    pub fn env(mut self, key: &str, value: &str) -> Self {
        self.env.push((key.to_string(), value.to_string()));
        self
    }

    /// Shell-like rendering for logs (arguments are quoted when they contain spaces).
    pub fn display(&self) -> String {
        let mut s = self.program.display().to_string();
        for a in &self.args {
            let a = a.to_string_lossy();
            if a.contains(' ') {
                s.push_str(&format!(" \"{a}\""));
            } else {
                s.push(' ');
                s.push_str(&a);
            }
        }
        s
    }

    /// Program name without directories (for error messages).
    pub fn program_name(&self) -> String {
        self.program
            .file_name()
            .map(|f| f.to_string_lossy().to_string())
            .unwrap_or_else(|| self.program.display().to_string())
    }
}

/// What happened when the process finished.
#[derive(Debug, Clone)]
pub struct ProcessOutcome {
    pub code: Option<i32>,
    pub success: bool,
    pub last_lines: Vec<String>,
}

impl ProcessOutcome {
    /// Turn a failure into an [`InstallError::ProcessFailed`].
    pub fn check(self, program: &str, log: Option<&Path>) -> Result<ProcessOutcome, InstallError> {
        if self.success {
            Ok(self)
        } else {
            Err(InstallError::ProcessFailed {
                program: program.to_string(),
                code: self.code,
                log: log.map(Path::to_path_buf),
                last_lines: self
                    .last_lines
                    .iter()
                    .rev()
                    .take(6)
                    .rev()
                    .cloned()
                    .collect(),
            })
        }
    }
}

/// Spawn `spec`, stream its stdout/stderr lines to `log` and `on_line`, and wait.
/// Cancelling `cancel` kills the child and yields [`InstallError::Cancelled`].
pub async fn run_logged(
    spec: &CommandSpec,
    log: &InstallLog,
    mut on_line: impl FnMut(&str),
    cancel: &CancellationToken,
) -> Result<ProcessOutcome, InstallError> {
    log.section(&format!("$ {}", spec.display()));
    let mut cmd = Command::new(&spec.program);
    cmd.args(&spec.args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    if let Some(dir) = &spec.cwd {
        cmd.current_dir(dir);
    }
    for (k, v) in &spec.env {
        cmd.env(k, v);
    }
    let mut child = cmd
        .spawn()
        .map_err(|e| InstallError::io("spawn", &spec.program, e))?;

    let (tx, mut rx) = mpsc::unbounded_channel::<String>();
    let mut readers = Vec::new();
    if let Some(stdout) = child.stdout.take() {
        let tx = tx.clone();
        readers.push(tokio::spawn(async move {
            let mut lines = BufReader::new(stdout).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                if tx.send(line).is_err() {
                    break;
                }
            }
        }));
    }
    if let Some(stderr) = child.stderr.take() {
        let tx = tx.clone();
        readers.push(tokio::spawn(async move {
            let mut lines = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                if tx.send(line).is_err() {
                    break;
                }
            }
        }));
    }
    drop(tx);

    let mut last: VecDeque<String> = VecDeque::with_capacity(KEEP_LAST_LINES);
    let status;
    loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                let _ = child.kill().await;
                log.line("[cancelled by user]");
                return Err(InstallError::Cancelled);
            }
            line = rx.recv() => {
                match line {
                    Some(line) => {
                        let clean = strip_ansi(&line);
                        log.line(&clean);
                        on_line(&clean);
                        if last.len() == KEEP_LAST_LINES {
                            last.pop_front();
                        }
                        last.push_back(clean);
                    }
                    None => {
                        // Output streams closed; wait for exit.
                        status = child.wait().await.map_err(|e| InstallError::io("wait", &spec.program, e))?;
                        break;
                    }
                }
            }
        }
    }
    for r in readers {
        let _ = r.await;
    }
    log.line(&format!(
        "[exit: {}]",
        status
            .code()
            .map(|c| c.to_string())
            .unwrap_or_else(|| "signal".into())
    ));
    Ok(ProcessOutcome {
        code: status.code(),
        success: status.success(),
        last_lines: last.into_iter().collect(),
    })
}

/// Remove ANSI escape sequences so logs and progress lines stay readable.
pub fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            if chars.peek() == Some(&'[') {
                chars.next();
                for n in chars.by_ref() {
                    if n.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
            continue;
        }
        if c == '\r' {
            continue;
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn log() -> (tempfile::TempDir, InstallLog) {
        let dir = tempfile::tempdir().unwrap();
        let log = InstallLog::create(dir.path(), "test", "proc").unwrap();
        (dir, log)
    }

    #[test]
    fn strips_ansi_and_carriage_returns() {
        assert_eq!(
            strip_ansi("\x1b[32m   Compiling\x1b[0m foo\r"),
            "   Compiling foo"
        );
        assert_eq!(strip_ansi("plain"), "plain");
    }

    #[test]
    fn display_quotes_spaces() {
        let spec = CommandSpec::new("cargo").args(["install", "a b"]);
        assert_eq!(spec.display(), "cargo install \"a b\"");
        assert_eq!(spec.program_name(), "cargo");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn captures_output_and_exit_code() {
        let (_dir, log) = log();
        let mut lines = Vec::new();
        let spec = CommandSpec::new("sh").args(["-c", "echo one; echo two 1>&2; exit 3"]);
        let outcome = run_logged(
            &spec,
            &log,
            |l| lines.push(l.to_string()),
            &CancellationToken::new(),
        )
        .await
        .unwrap();
        assert_eq!(outcome.code, Some(3));
        assert!(!outcome.success);
        assert!(lines.contains(&"one".to_string()) && lines.contains(&"two".to_string()));
        let err = outcome.check("sh", Some(log.path())).unwrap_err();
        assert!(matches!(
            err,
            InstallError::ProcessFailed { code: Some(3), .. }
        ));
        let text = std::fs::read_to_string(log.path()).unwrap();
        assert!(text.contains("$ sh -c"));
        assert!(text.contains("[exit: 3]"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn cancellation_kills_child() {
        let (_dir, log) = log();
        let token = CancellationToken::new();
        let spec = CommandSpec::new("sh").args(["-c", "sleep 30"]);
        let t = token.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            t.cancel();
        });
        let start = std::time::Instant::now();
        let err = run_logged(&spec, &log, |_| {}, &token).await.unwrap_err();
        assert!(matches!(err, InstallError::Cancelled));
        assert!(start.elapsed() < std::time::Duration::from_secs(5));
    }

    #[tokio::test]
    async fn missing_program_is_reported() {
        let (_dir, log) = log();
        let spec = CommandSpec::new("/definitely/missing/program");
        let err = run_logged(&spec, &log, |_| {}, &CancellationToken::new())
            .await
            .unwrap_err();
        assert!(matches!(err, InstallError::Io { op: "spawn", .. }));
    }
}
