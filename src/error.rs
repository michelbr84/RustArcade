//! Typed error hierarchy for RustArcade.
//!
//! Every subsystem has its own error enum; [`Error`] wraps them all so the CLI and TUI
//! can render one actionable [`UserMessage`] (title, detail, possible causes, log path).

use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

use thiserror::Error;

/// Convenience alias used across the crate.
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Top-level error type.
#[derive(Debug, Error)]
pub enum Error {
    #[error(transparent)]
    Catalog(#[from] CatalogError),
    #[error(transparent)]
    Install(#[from] InstallError),
    #[error(transparent)]
    Launch(#[from] LaunchError),
    #[error(transparent)]
    Storage(#[from] StorageError),
    #[error(transparent)]
    Network(#[from] NetworkError),
    #[error(transparent)]
    Platform(#[from] PlatformError),
    #[error(transparent)]
    Security(#[from] SecurityError),
    #[error(transparent)]
    Config(#[from] ConfigError),
    /// An error that happened while performing `action` on `game`.
    #[error("{action} {game}: {source}")]
    InContext {
        action: &'static str,
        game: String,
        log: Option<PathBuf>,
        #[source]
        source: Box<Error>,
    },
}

/// A single validation problem inside a manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    /// Dotted field path such as `installers[1].binary`.
    pub path: String,
    pub message: String,
}

impl fmt::Display for Problem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.path, self.message)
    }
}

#[derive(Debug, Error)]
pub enum CatalogError {
    #[error("{file}: parse error{}: {message}", fmt_line_col(*line, *col))]
    Parse {
        file: PathBuf,
        line: Option<usize>,
        col: Option<usize>,
        message: String,
    },
    #[error("{file}: {} validation problem(s)", problems.len())]
    Invalid {
        file: PathBuf,
        problems: Vec<Problem>,
    },
    #[error("duplicate game id `{id}` in {files:?}")]
    DuplicateId { id: String, files: Vec<PathBuf> },
    #[error("executable `{executable}` is declared by several games: {ids:?}")]
    DuplicateExecutable {
        executable: String,
        ids: Vec<String>,
    },
    #[error("unknown game `{0}`")]
    UnknownGame(String),
    #[error("catalog index at {url} is unusable: {reason}")]
    Index { url: String, reason: String },
    #[error("remote catalog rejected: {reason}")]
    RemoteRejected { reason: String },
    #[error("{path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
}

#[derive(Debug, Error)]
pub enum InstallError {
    #[error("`{tool}` was not found on this system")]
    ToolMissing { tool: String, hint: String },
    #[error("{program} exited with {}", fmt_exit(*code))]
    ProcessFailed {
        program: String,
        code: Option<i32>,
        log: Option<PathBuf>,
        last_lines: Vec<String>,
    },
    #[error("no installer can be used for {game} on this system")]
    NoCompatibleInstaller { game: String, reasons: Vec<String> },
    #[error("no usable release found for {repository}: {detail}")]
    ReleaseNotFound { repository: String, detail: String },
    #[error("no release asset matches this platform")]
    NoMatchingAsset {
        tried: Vec<String>,
        available: Vec<String>,
    },
    #[error("several release assets could match this platform")]
    AmbiguousAsset { candidates: Vec<String> },
    #[error("expected executable `{expected}` was not produced by the installer")]
    BinaryNotFound {
        expected: String,
        found: Vec<String>,
    },
    #[error("{path} is not a usable executable: {reason}")]
    NotAnExecutable { path: PathBuf, reason: String },
    #[error("unsupported archive format: {name}")]
    UnsupportedArchive { name: String },
    #[error("no checksum is available for {asset} and the configuration requires one")]
    ChecksumUnavailable { asset: String },
    #[error("failed to extract `{entry}`: {source}")]
    Extract {
        entry: String,
        #[source]
        source: io::Error,
    },
    #[error("{0} is already installed")]
    AlreadyInstalled(String),
    #[error("{0} is not installed")]
    NotInstalled(String),
    #[error("an installation of {0} is already in progress")]
    JobInProgress(String),
    #[error("{0} is currently running")]
    GameRunning(String),
    #[error("installation cancelled")]
    Cancelled,
    #[error("could not determine the installed version: {detail}")]
    VersionUnknown { detail: String },
    #[error("repository checkout is at commit {actual}, expected {expected}")]
    CommitMismatch { expected: String, actual: String },
    #[error("{op} {path}: {source}")]
    Io {
        op: &'static str,
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("rollback during {stage}: {message}")]
    Rollback {
        stage: &'static str,
        message: String,
    },
}

impl InstallError {
    pub fn io(op: &'static str, path: impl Into<PathBuf>, source: io::Error) -> Self {
        Self::Io {
            op,
            path: path.into(),
            source,
        }
    }
}

#[derive(Debug, Error)]
pub enum SecurityError {
    #[error("checksum mismatch for {asset}: expected {expected}, got {actual}")]
    ChecksumMismatch {
        asset: String,
        expected: String,
        actual: String,
    },
    #[error("archive entry `{entry}` would be written outside the destination")]
    ArchiveEscape { entry: String },
    #[error("archive symlink `{entry}` points outside the destination ({target})")]
    UnsafeSymlink { entry: String, target: String },
    #[error("archive hard link `{entry}` points outside the destination")]
    HardLinkEscape { entry: String },
    #[error("refusing to touch {path}: outside the RustArcade managed directory")]
    PathOutsideManagedRoot { path: PathBuf },
    #[error("refusing insecure URL {url} (HTTPS is required)")]
    InsecureUrl { url: String },
    #[error("unsafe path `{value}`: {reason}")]
    UnsafePath { value: String, reason: String },
    #[error("archive exceeds the {limit_bytes} byte extraction limit")]
    ArchiveTooLarge { limit_bytes: u64 },
    #[error("archive has more than {limit} entries")]
    TooManyEntries { limit: usize },
    #[error("download exceeds the {limit_bytes} byte limit")]
    DownloadTooLarge { limit_bytes: u64 },
}

#[derive(Debug, Error)]
pub enum LaunchError {
    #[error("{0} is not installed")]
    NotInstalled(String),
    #[error("executable {path} is missing")]
    MissingExecutable { path: PathBuf },
    #[error("{path} is not executable: {reason}")]
    NotExecutable { path: PathBuf, reason: String },
    #[error("could not start {path}: {source}")]
    Spawn {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("terminal {stage} failed: {source}")]
    Terminal {
        stage: &'static str,
        #[source]
        source: io::Error,
    },
    #[error("{0} is busy (installing or running)")]
    Busy(String),
}

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("{op} {path}: {source}")]
    Io {
        op: &'static str,
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("{path} is corrupt: {reason}")]
    Corrupt { path: PathBuf, reason: String },
    #[error("{path} is not writable")]
    NotWritable { path: PathBuf },
    #[error("could not determine a home directory for this user")]
    NoHomeDirectory,
}

impl StorageError {
    pub fn io(op: &'static str, path: impl Into<PathBuf>, source: io::Error) -> Self {
        Self::Io {
            op,
            path: path.into(),
            source,
        }
    }
}

#[derive(Debug, Error)]
pub enum NetworkError {
    #[error("{url} returned HTTP {status}")]
    Status { url: String, status: u16 },
    #[error("{url}: {source}")]
    Transport {
        url: String,
        #[source]
        source: reqwest::Error,
    },
    #[error("{url} timed out")]
    Timeout { url: String },
    #[error("GitHub API rate limit exceeded{}", reset_hint(*reset_at))]
    RateLimited {
        reset_at: Option<chrono::DateTime<chrono::Utc>>,
    },
    #[error("{url}: unexpected response: {reason}")]
    InvalidBody { url: String, reason: String },
    #[error("{url} was not found")]
    NotFound { url: String },
    #[error("network access is disabled (offline mode)")]
    Offline,
}

#[derive(Debug, Error)]
pub enum PlatformError {
    #[error("unsupported operating system `{0}`")]
    UnsupportedOs(String),
    #[error("unsupported CPU architecture `{0}`")]
    UnsupportedArch(String),
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("{path}: {message}")]
    Parse { path: PathBuf, message: String },
    #[error("invalid configuration `{field}`: {reason}")]
    Invalid { field: String, reason: String },
    #[error("{path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
}

fn fmt_line_col(line: Option<usize>, col: Option<usize>) -> String {
    match (line, col) {
        (Some(l), Some(c)) => format!(" at line {l}, column {c}"),
        (Some(l), None) => format!(" at line {l}"),
        _ => String::new(),
    }
}

fn fmt_exit(code: Option<i32>) -> String {
    match code {
        Some(c) => format!("exit code {c}"),
        None => "a signal".to_string(),
    }
}

fn reset_hint(reset_at: Option<chrono::DateTime<chrono::Utc>>) -> String {
    reset_at
        .map(|t| format!(" (resets at {})", t.format("%H:%M UTC")))
        .unwrap_or_default()
}

/// Human-facing rendering of an error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserMessage {
    pub title: String,
    pub detail: String,
    pub causes: Vec<String>,
    pub hint: Option<String>,
    pub log: Option<PathBuf>,
}

impl fmt::Display for UserMessage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "{}", self.title)?;
        if !self.detail.is_empty() {
            writeln!(f)?;
            writeln!(f, "{}", self.detail)?;
        }
        if !self.causes.is_empty() {
            writeln!(f)?;
            writeln!(f, "Possible causes:")?;
            for cause in &self.causes {
                writeln!(f, "• {cause}")?;
            }
        }
        if let Some(hint) = &self.hint {
            writeln!(f)?;
            writeln!(f, "{hint}")?;
        }
        if let Some(log) = &self.log {
            writeln!(f)?;
            writeln!(f, "Log:")?;
            writeln!(f, "{}", log.display())?;
        }
        Ok(())
    }
}

impl Error {
    /// Wrap this error with the action/game it belongs to.
    pub fn in_context(self, action: &'static str, game: &str, log: Option<PathBuf>) -> Error {
        Error::InContext {
            action,
            game: game.to_string(),
            log,
            source: Box::new(self),
        }
    }

    /// Path of the log file associated with this error, if any.
    pub fn log_path(&self) -> Option<&Path> {
        match self {
            Error::InContext { log, source, .. } => log.as_deref().or_else(|| source.log_path()),
            Error::Install(InstallError::ProcessFailed { log, .. }) => log.as_deref(),
            _ => None,
        }
    }

    /// True when the failure is a security check (checksum, traversal, ...).
    pub fn is_security(&self) -> bool {
        match self {
            Error::Security(_) => true,
            Error::InContext { source, .. } => source.is_security(),
            _ => false,
        }
    }

    /// Exit code the CLI should use for this error.
    pub fn exit_code(&self) -> i32 {
        if self.is_security() { 3 } else { 1 }
    }

    /// Render this error as an actionable message.
    pub fn user_message(&self) -> UserMessage {
        match self {
            Error::InContext {
                action,
                game,
                log,
                source,
            } => {
                let mut msg = source.user_message();
                msg.title = format!("Unable to {action} {game}.");
                if msg.log.is_none() {
                    msg.log = log.clone();
                }
                msg
            }
            Error::Install(e) => install_message(e),
            Error::Security(e) => security_message(e),
            Error::Network(e) => network_message(e),
            Error::Launch(e) => launch_message(e),
            Error::Catalog(e) => catalog_message(e),
            Error::Storage(e) => UserMessage {
                title: "Storage error.".into(),
                detail: e.to_string(),
                causes: vec![
                    "The RustArcade data directory is not writable".into(),
                    "The disk is full or a file is locked by another program".into(),
                ],
                hint: Some("Run `rustarcade doctor` to check directory permissions.".into()),
                log: None,
            },
            Error::Platform(e) => UserMessage {
                title: "Unsupported platform.".into(),
                detail: e.to_string(),
                causes: vec![],
                hint: None,
                log: None,
            },
            Error::Config(e) => UserMessage {
                title: "Configuration error.".into(),
                detail: e.to_string(),
                causes: vec![],
                hint: Some("Fix or delete config.toml and try again.".into()),
                log: None,
            },
        }
    }
}

fn install_message(e: &InstallError) -> UserMessage {
    let (detail, causes, hint, log) = match e {
        InstallError::ToolMissing { tool, hint } => (
            format!("`{tool}` was not found on this system."),
            vec![
                format!("{tool} is not installed"),
                format!("{tool} is installed but not on PATH"),
            ],
            Some(hint.clone()),
            None,
        ),
        InstallError::ProcessFailed {
            program,
            code,
            log,
            last_lines,
        } => {
            let mut detail = format!("{program} returned {}.", fmt_exit(*code));
            if !last_lines.is_empty() {
                detail.push_str("\n\n");
                detail.push_str(&last_lines.join("\n"));
            }
            let causes = if program.contains("cargo") {
                vec![
                    "The Rust toolchain is outdated (run `rustup update`)".into(),
                    "A required native library or C compiler is missing".into(),
                    "The crate no longer builds on this platform".into(),
                ]
            } else if program.contains("git") {
                vec![
                    "The repository, branch, or tag no longer exists".into(),
                    "No network connection".into(),
                ]
            } else {
                vec!["The program failed unexpectedly".into()]
            };
            (
                detail,
                causes,
                Some("Open the log for the full output.".into()),
                log.clone(),
            )
        }
        InstallError::NoCompatibleInstaller { reasons, .. } => (
            "None of the installation methods declared for this game can run here.".into(),
            reasons.clone(),
            Some("Install the missing tools (Cargo from https://rustup.rs, Git) and retry.".into()),
            None,
        ),
        InstallError::ReleaseNotFound { repository, detail } => (
            format!("No usable GitHub release was found for {repository}: {detail}"),
            vec![
                "The project has not published a release yet".into(),
                "Only pre-releases exist and the manifest does not allow them".into(),
            ],
            Some("Try another installation method with `--method cargo`.".into()),
            None,
        ),
        InstallError::NoMatchingAsset { tried, available } => (
            format!(
                "No release asset matches this platform.\n\nTried: {}\nAvailable: {}",
                if tried.is_empty() {
                    "(heuristic match)".to_string()
                } else {
                    tried.join(", ")
                },
                if available.is_empty() {
                    "(none)".to_string()
                } else {
                    available.join(", ")
                }
            ),
            vec![
                "The project does not publish binaries for this OS/architecture".into(),
                "The release asset naming changed and the manifest needs an update".into(),
            ],
            Some("Try `--method cargo` to build from source.".into()),
            None,
        ),
        InstallError::AmbiguousAsset { candidates } => (
            format!(
                "Several release assets could match: {}",
                candidates.join(", ")
            ),
            vec!["The manifest needs an explicit `asset` pattern".into()],
            Some("Report this at the RustArcade catalog repository.".into()),
            None,
        ),
        InstallError::BinaryNotFound { expected, found } => (
            format!(
                "The installer finished but `{expected}` was not produced.\nFound: {}",
                if found.is_empty() {
                    "(nothing)".to_string()
                } else {
                    found.join(", ")
                }
            ),
            vec!["The project renamed its executable".into()],
            None,
            None,
        ),
        InstallError::Cancelled => ("The installation was cancelled.".into(), vec![], None, None),
        other => (other.to_string(), vec![], None, None),
    };
    UserMessage {
        title: "Installation failed.".into(),
        detail,
        causes,
        hint,
        log,
    }
}

fn security_message(e: &SecurityError) -> UserMessage {
    let causes = match e {
        SecurityError::ChecksumMismatch { .. } => vec![
            "The release asset was modified after the checksum was recorded".into(),
            "The download was corrupted or intercepted".into(),
        ],
        SecurityError::ArchiveEscape { .. }
        | SecurityError::UnsafeSymlink { .. }
        | SecurityError::HardLinkEscape { .. } => {
            vec![
                "The archive contains entries that try to write outside the install directory"
                    .into(),
            ]
        }
        SecurityError::InsecureUrl { .. } => {
            vec!["The manifest or redirect target uses plain HTTP".into()]
        }
        _ => vec![],
    };
    UserMessage {
        title: "Security check failed.".into(),
        detail: e.to_string(),
        causes,
        hint: Some("Nothing was installed and downloaded artifacts were deleted.".into()),
        log: None,
    }
}

fn network_message(e: &NetworkError) -> UserMessage {
    let (causes, hint) = match e {
        NetworkError::RateLimited { .. } => (
            vec!["Too many unauthenticated GitHub API requests from this address".into()],
            Some("Set the GITHUB_TOKEN environment variable to raise the GitHub API limit.".into()),
        ),
        NetworkError::Offline => (
            vec!["RustArcade was started with --offline or RUSTARCADE_OFFLINE=1".into()],
            Some("Run again without --offline to download games.".into()),
        ),
        _ => (
            vec![
                "No internet connection".into(),
                "The remote service is unavailable".into(),
            ],
            Some("Check your internet connection and retry.".into()),
        ),
    };
    UserMessage {
        title: "Network error.".into(),
        detail: e.to_string(),
        causes,
        hint,
        log: None,
    }
}

fn launch_message(e: &LaunchError) -> UserMessage {
    let (causes, hint) = match e {
        LaunchError::MissingExecutable { .. } => (
            vec!["The game files were removed outside RustArcade".into()],
            Some("Reinstall the game from its details screen.".into()),
        ),
        LaunchError::Spawn { .. } => (
            vec![
                "The executable is not compatible with this system".into(),
                "A shared library the game needs is missing (for example libasound2)".into(),
            ],
            None,
        ),
        _ => (vec![], None),
    };
    UserMessage {
        title: "Unable to launch the game.".into(),
        detail: e.to_string(),
        causes,
        hint,
        log: None,
    }
}

fn catalog_message(e: &CatalogError) -> UserMessage {
    if let CatalogError::UnknownGame(id) = e {
        return UserMessage {
            title: "Unknown game.".into(),
            detail: format!("`{id}` is not in the catalog."),
            causes: vec![],
            hint: Some(
                "Run `rustarcade list` or `rustarcade search <name>` to find the game id.".into(),
            ),
            log: None,
        };
    }
    let detail = match e {
        CatalogError::Invalid { file, problems } => {
            let mut s = format!("{}:", file.display());
            for p in problems {
                s.push_str(&format!("\n  • {p}"));
            }
            s
        }
        other => other.to_string(),
    };
    UserMessage {
        title: "Catalog error.".into(),
        detail,
        causes: vec![],
        hint: Some("Run `rustarcade catalog validate` for details.".into()),
        log: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_message_formats_causes_and_log() {
        let err = Error::Install(InstallError::ProcessFailed {
            program: "cargo".into(),
            code: Some(101),
            log: Some(PathBuf::from("/tmp/install.log")),
            last_lines: vec!["error: could not compile".into()],
        })
        .in_context("install", "Tetro TUI", None);
        let msg = err.user_message();
        let rendered = msg.to_string();
        assert_eq!(msg.title, "Unable to install Tetro TUI.");
        assert!(rendered.contains("cargo returned exit code 101."));
        assert!(rendered.contains("Possible causes:"));
        assert!(rendered.contains("• The Rust toolchain is outdated"));
        assert!(rendered.contains("/tmp/install.log"));
        assert_eq!(err.log_path(), Some(Path::new("/tmp/install.log")));
    }

    #[test]
    fn in_context_keeps_outer_log_when_inner_has_none() {
        let err = Error::Install(InstallError::Cancelled).in_context(
            "update",
            "x",
            Some(PathBuf::from("/tmp/x.log")),
        );
        assert_eq!(err.log_path(), Some(Path::new("/tmp/x.log")));
        assert_eq!(err.user_message().log, Some(PathBuf::from("/tmp/x.log")));
    }

    #[test]
    fn security_errors_use_exit_code_3() {
        let err = Error::Security(SecurityError::ArchiveEscape {
            entry: "../etc/passwd".into(),
        });
        assert!(err.is_security());
        assert_eq!(err.exit_code(), 3);
        assert!(err.in_context("install", "g", None).is_security());
        assert_eq!(Error::Install(InstallError::Cancelled).exit_code(), 1);
    }
}
