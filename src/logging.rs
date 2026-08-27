//! Logging setup (`tracing` to daily log files) and per-installation log writers.

use std::fs::{self, File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

use crate::error::StorageError;

/// Environment variable controlling the tracing filter (e.g. `debug`, `rustarcade=trace`).
pub const LOG_ENV: &str = "RUSTARCADE_LOG";

/// How RustArcade is being used, which decides where logs may go.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogMode {
    /// Full-screen TUI: never write to the terminal.
    Tui,
    /// Command line: warnings/debug output may go to stderr.
    Cli,
}

/// Keeps the non-blocking log writer alive; drop at the very end of `main`.
#[derive(Debug)]
pub struct LogGuard {
    _worker: Option<WorkerGuard>,
    pub file: Option<PathBuf>,
}

/// Initialise the global tracing subscriber. Safe to call once per process.
pub fn init(logs_dir: &Path, mode: LogMode, debug: bool) -> Result<LogGuard, StorageError> {
    fs::create_dir_all(logs_dir).map_err(|e| StorageError::io("create", logs_dir, e))?;
    let default_level = if debug { "debug" } else { "info" };
    let filter = EnvFilter::try_from_env(LOG_ENV)
        .unwrap_or_else(|_| EnvFilter::new(format!("rustarcade={default_level},warn")));

    let appender = tracing_appender::rolling::daily(logs_dir, "rustarcade.log");
    let (writer, worker) = tracing_appender::non_blocking(appender);
    let file_layer = tracing_subscriber::fmt::layer()
        .with_writer(writer)
        .with_ansi(false)
        .with_target(true);

    let stderr_layer = match mode {
        LogMode::Cli if debug => Some(
            tracing_subscriber::fmt::layer()
                .with_writer(std::io::stderr)
                .with_target(false)
                .compact(),
        ),
        _ => None,
    };

    let registry = tracing_subscriber::registry()
        .with(filter)
        .with(file_layer)
        .with(stderr_layer);
    // A second init (e.g. in tests) is not an error worth failing over.
    let _ = registry.try_init();
    Ok(LogGuard {
        _worker: Some(worker),
        file: Some(logs_dir.join("rustarcade.log")),
    })
}

/// Line-oriented log file for a single install/update job.
#[derive(Debug)]
pub struct InstallLog {
    path: PathBuf,
    writer: Mutex<BufWriter<File>>,
}

impl InstallLog {
    /// Create `logs_dir/<kind>-<game>-<timestamp>.log`.
    pub fn create(logs_dir: &Path, kind: &str, game: &str) -> Result<InstallLog, StorageError> {
        fs::create_dir_all(logs_dir).map_err(|e| StorageError::io("create", logs_dir, e))?;
        let stamp = chrono::Utc::now().format("%Y%m%dT%H%M%SZ");
        let path = logs_dir.join(format!("{kind}-{game}-{stamp}.log"));
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|e| StorageError::io("create", &path, e))?;
        let log = InstallLog {
            path,
            writer: Mutex::new(BufWriter::new(file)),
        };
        log.section(&format!("RustArcade {} — {kind} {game}", crate::VERSION));
        Ok(log)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Append one line (a newline is added).
    pub fn line(&self, text: &str) {
        if let Ok(mut w) = self.writer.lock() {
            let _ = writeln!(w, "{text}");
            let _ = w.flush();
        }
    }

    /// Append a titled section header with a timestamp.
    pub fn section(&self, title: &str) {
        let stamp = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC");
        self.line(&format!("==== {title} [{stamp}] ===="));
    }

    /// Last `n` non-empty lines of the log (for error summaries).
    pub fn tail(&self, n: usize) -> Vec<String> {
        if let Ok(mut w) = self.writer.lock() {
            let _ = w.flush();
        }
        tail_of_file(&self.path, n)
    }
}

/// Last `n` non-empty lines of any text file.
pub fn tail_of_file(path: &Path, n: usize) -> Vec<String> {
    let Ok(text) = fs::read_to_string(path) else {
        return Vec::new();
    };
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    lines
        .iter()
        .rev()
        .take(n)
        .rev()
        .map(|l| l.to_string())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_log_writes_sections_and_tail() {
        let dir = tempfile::tempdir().unwrap();
        let log = InstallLog::create(dir.path(), "install", "demo").unwrap();
        log.line("one");
        log.line("");
        log.line("two");
        log.section("done");
        let tail = log.tail(2);
        assert_eq!(tail.len(), 2);
        assert_eq!(tail[0], "two");
        assert!(tail[1].starts_with("==== done"));
        assert!(
            log.path()
                .file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .starts_with("install-demo-")
        );
    }

    #[test]
    fn init_creates_directory_and_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let logs = dir.path().join("logs");
        let g1 = init(&logs, LogMode::Tui, false).unwrap();
        let g2 = init(&logs, LogMode::Cli, true).unwrap();
        assert!(logs.is_dir());
        drop(g1);
        drop(g2);
    }
}
