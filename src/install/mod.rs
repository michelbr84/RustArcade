//! Installation engine: typed installers, planning, progress, and transactions.
//!
//! Only three installer kinds exist ([`InstallerKind`]); each turns a declarative
//! manifest entry into a managed installation under `data/games/<id>/current`.

pub mod archive;
pub mod assets;
pub mod cargo;
pub mod git_build;
pub mod github_release;
pub mod process;
pub mod transaction;

use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::Serialize;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::catalog::manifest::{GameId, GameManifest, InstallerKind, InstallerSpec};
use crate::config::Config;
use crate::error::{Error, InstallError};
use crate::logging::InstallLog;
use crate::net::{CratesIoClient, GitHubClient, HttpClient, ReleaseAsset};
use crate::paths::AppPaths;
use crate::platform::{Platform, Tools};
use crate::registry::{InstallRecord, InstallSource};

pub use transaction::{InstallOutcome, SweepReport, UninstallReport};

/// Identifier of a background job (install/update).
pub type JobId = u64;

/// Installation phases shown in progress UIs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Phase {
    Resolving,
    Downloading,
    Verifying,
    Extracting,
    Compiling,
    Registering,
    Ready,
}

impl Phase {
    pub const ALL: [Phase; 7] = [
        Phase::Resolving,
        Phase::Downloading,
        Phase::Verifying,
        Phase::Extracting,
        Phase::Compiling,
        Phase::Registering,
        Phase::Ready,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Phase::Resolving => "Resolving",
            Phase::Downloading => "Downloading",
            Phase::Verifying => "Verifying",
            Phase::Extracting => "Extracting",
            Phase::Compiling => "Compiling",
            Phase::Registering => "Registering",
            Phase::Ready => "Ready",
        }
    }

    pub fn index(self) -> usize {
        Phase::ALL.iter().position(|p| *p == self).unwrap_or(0)
    }
}

/// Progress notifications emitted by installers.
#[derive(Debug, Clone)]
pub enum ProgressEvent {
    Started {
        job: JobId,
        game: GameId,
        log: PathBuf,
    },
    Phase {
        job: JobId,
        phase: Phase,
        detail: String,
    },
    Bytes {
        job: JobId,
        done: u64,
        total: Option<u64>,
    },
    Output {
        job: JobId,
        line: String,
    },
    Finished {
        job: JobId,
        game: GameId,
        success: bool,
        message: String,
    },
}

impl ProgressEvent {
    pub fn job(&self) -> JobId {
        match self {
            ProgressEvent::Started { job, .. }
            | ProgressEvent::Phase { job, .. }
            | ProgressEvent::Bytes { job, .. }
            | ProgressEvent::Output { job, .. }
            | ProgressEvent::Finished { job, .. } => *job,
        }
    }
}

/// Shared callback receiving progress events.
pub type ProgressCallback = Arc<dyn Fn(&ProgressEvent) + Send + Sync>;

/// Where progress events go.
#[derive(Clone)]
pub enum ProgressSink {
    Channel(mpsc::UnboundedSender<ProgressEvent>),
    Callback(ProgressCallback),
    Silent,
}

impl fmt::Debug for ProgressSink {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProgressSink::Channel(_) => f.write_str("ProgressSink::Channel"),
            ProgressSink::Callback(_) => f.write_str("ProgressSink::Callback"),
            ProgressSink::Silent => f.write_str("ProgressSink::Silent"),
        }
    }
}

/// A job-bound progress reporter.
#[derive(Debug, Clone)]
pub struct Progress {
    pub job: JobId,
    sink: ProgressSink,
}

impl Progress {
    pub fn new(job: JobId, sink: ProgressSink) -> Progress {
        Progress { job, sink }
    }

    pub fn silent() -> Progress {
        Progress {
            job: 0,
            sink: ProgressSink::Silent,
        }
    }

    pub fn emit(&self, event: ProgressEvent) {
        match &self.sink {
            ProgressSink::Channel(tx) => {
                let _ = tx.send(event);
            }
            ProgressSink::Callback(cb) => cb(&event),
            ProgressSink::Silent => {}
        }
    }

    pub fn phase(&self, phase: Phase, detail: impl Into<String>) {
        self.emit(ProgressEvent::Phase {
            job: self.job,
            phase,
            detail: detail.into(),
        });
    }

    pub fn bytes(&self, done: u64, total: Option<u64>) {
        self.emit(ProgressEvent::Bytes {
            job: self.job,
            done,
            total,
        });
    }

    pub fn output(&self, line: impl Into<String>) {
        self.emit(ProgressEvent::Output {
            job: self.job,
            line: line.into(),
        });
    }

    pub fn started(&self, game: &GameId, log: &Path) {
        self.emit(ProgressEvent::Started {
            job: self.job,
            game: game.clone(),
            log: log.to_path_buf(),
        });
    }

    pub fn finished(&self, game: &GameId, success: bool, message: impl Into<String>) {
        self.emit(ProgressEvent::Finished {
            job: self.job,
            game: game.clone(),
            success,
            message: message.into(),
        });
    }
}

/// Read-only environment shared by every installer.
#[derive(Debug, Clone, Copy)]
pub struct InstallEnv<'a> {
    pub paths: &'a AppPaths,
    pub platform: Platform,
    pub tools: &'a Tools,
    pub http: &'a HttpClient,
    pub github: &'a GitHubClient,
    pub crates: &'a CratesIoClient,
    pub config: &'a Config,
}

/// Per-job context: environment plus progress, log and cancellation.
#[derive(Debug)]
pub struct JobContext<'a> {
    pub env: InstallEnv<'a>,
    pub progress: Progress,
    pub log: Arc<InstallLog>,
    pub cancel: CancellationToken,
}

impl JobContext<'_> {
    /// Fail fast if the job was cancelled.
    pub fn check_cancel(&self) -> Result<(), InstallError> {
        if self.cancel.is_cancelled() {
            Err(InstallError::Cancelled)
        } else {
            Ok(())
        }
    }
}

/// How a downloaded asset will be verified.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case", tag = "kind", content = "detail")]
pub enum ChecksumPolicy {
    /// Digest pinned in the manifest.
    Manifest,
    /// `digest` reported by the GitHub API for the asset.
    ApiDigest,
    /// A checksum file published with the release.
    ReleaseFile(String),
    /// Nothing available: HTTPS transport integrity only.
    None,
}

impl ChecksumPolicy {
    pub fn label(&self) -> String {
        match self {
            ChecksumPolicy::Manifest => "SHA-256 pinned in the catalog manifest".into(),
            ChecksumPolicy::ApiDigest => "SHA-256 digest published by GitHub for this asset".into(),
            ChecksumPolicy::ReleaseFile(name) => format!("SHA-256 from release file {name}"),
            ChecksumPolicy::None => "not provided upstream (HTTPS transport only)".into(),
        }
    }

    pub fn available(&self) -> bool {
        !matches!(self, ChecksumPolicy::None)
    }
}

/// Where the expected digest comes from (resolved form of [`ChecksumPolicy`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChecksumSource {
    Manifest(String),
    ApiDigest(String),
    ReleaseFile { name: String, url: String },
    None,
}

impl ChecksumSource {
    pub fn policy(&self) -> ChecksumPolicy {
        match self {
            ChecksumSource::Manifest(_) => ChecksumPolicy::Manifest,
            ChecksumSource::ApiDigest(_) => ChecksumPolicy::ApiDigest,
            ChecksumSource::ReleaseFile { name, .. } => ChecksumPolicy::ReleaseFile(name.clone()),
            ChecksumSource::None => ChecksumPolicy::None,
        }
    }
}

/// Availability of a tool needed by a plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ToolStatus {
    pub name: String,
    pub path: Option<PathBuf>,
    pub version: Option<String>,
}

/// Installer-specific data resolved while planning.
#[derive(Debug, Clone)]
pub enum Resolved {
    Release {
        tag: String,
        version: String,
        asset: ReleaseAsset,
        checksum: ChecksumSource,
    },
    Cargo {
        latest: Option<String>,
    },
    Git {
        remote_commit: Option<String>,
    },
}

/// Everything the user sees before confirming an installation.
#[derive(Debug, Clone)]
pub struct InstallPlan {
    pub game: GameId,
    pub name: String,
    pub installer: InstallerKind,
    pub installer_index: usize,
    pub source: String,
    pub version: Option<String>,
    pub asset: Option<String>,
    pub checksum: ChecksumPolicy,
    pub destination: PathBuf,
    pub executable: String,
    pub tools: Vec<ToolStatus>,
    pub missing_commands: Vec<String>,
    pub missing_optional: Vec<String>,
    pub warnings: Vec<String>,
    pub requires_network: bool,
    pub compiles: bool,
    pub requires_admin: bool,
    pub is_update: bool,
    pub previous_version: Option<String>,
    /// Why other installers were not chosen.
    pub skipped: Vec<String>,
    pub resolved: Resolved,
}

impl InstallPlan {
    /// The installer spec this plan uses.
    pub fn spec<'m>(&self, manifest: &'m GameManifest) -> Option<&'m InstallerSpec> {
        manifest.installers.get(self.installer_index)
    }

    /// Version string for display (`latest` when unknown until install time).
    pub fn version_label(&self) -> String {
        self.version.clone().unwrap_or_else(|| "latest".into())
    }
}

/// Result of a successful fetch into the staging directory.
#[derive(Debug, Clone)]
pub struct Fetched {
    /// Absolute path of the executable inside staging.
    pub executable: PathBuf,
    pub version: String,
    pub source: InstallSource,
    pub checksum_verified: bool,
}

/// Installer preference order for a manifest on this platform.
fn candidates<'m>(
    env: &InstallEnv<'_>,
    manifest: &'m GameManifest,
    prefer: Option<InstallerKind>,
) -> Vec<(usize, &'m InstallerSpec)> {
    let mut list: Vec<(usize, &InstallerSpec)> = manifest
        .installers
        .iter()
        .enumerate()
        .filter(|(_, s)| s.applies_to(env.platform.os))
        .collect();
    if let Some(kind) = prefer {
        list.retain(|(_, s)| s.kind() == kind);
    } else if let Some(kind) = env.config.install.preferred_method {
        list.sort_by_key(|(_, s)| if s.kind() == kind { 0 } else { 1 });
    }
    list
}

/// Tools an installer kind needs.
pub fn required_tools(kind: InstallerKind) -> &'static [&'static str] {
    match kind {
        InstallerKind::Cargo => &["cargo"],
        InstallerKind::GitCargoBuild => &["git", "cargo"],
        InstallerKind::GithubRelease => &[],
    }
}

fn tool_hint(name: &str) -> String {
    match name {
        "cargo" | "rustc" => {
            "Install the Rust toolchain from https://rustup.rs and restart your terminal.".into()
        }
        "git" => "Install Git from https://git-scm.com/downloads.".into(),
        other => format!("Install `{other}` and make sure it is on PATH."),
    }
}

fn tool_status(env: &InstallEnv<'_>, name: &str) -> ToolStatus {
    let info = match name {
        "cargo" => &env.tools.cargo,
        "git" => &env.tools.git,
        _ => &env.tools.rustc,
    };
    ToolStatus {
        name: name.to_string(),
        path: info.path.clone(),
        version: info.version.clone(),
    }
}

/// Build an installation plan for `manifest`, trying installers in order until one can
/// run on this system. `prefer` restricts the choice to a single kind.
pub async fn plan(
    env: &InstallEnv<'_>,
    manifest: &GameManifest,
    prefer: Option<InstallerKind>,
    installed: Option<&InstallRecord>,
) -> Result<InstallPlan, Error> {
    if !manifest.supports(&env.platform) {
        return Err(InstallError::NoCompatibleInstaller {
            game: manifest.name.clone(),
            reasons: vec![format!(
                "{} is not available on {}",
                manifest.name, env.platform
            )],
        }
        .into());
    }
    if !manifest.support_status.installable() {
        return Err(InstallError::NoCompatibleInstaller {
            game: manifest.name.clone(),
            reasons: vec![format!(
                "{} is marked as {} in the catalog",
                manifest.name, manifest.support_status
            )],
        }
        .into());
    }
    let mut skipped = Vec::new();
    let mut network_error: Option<Error> = None;
    for (index, spec) in candidates(env, manifest, prefer) {
        let kind = spec.kind();
        let missing: Vec<&str> = required_tools(kind)
            .iter()
            .copied()
            .filter(|t| tool_status(env, t).path.is_none())
            .collect();
        if let Some(tool) = missing.first() {
            skipped.push(format!("{kind}: {tool} not found — {}", tool_hint(tool)));
            continue;
        }
        let resolved = match spec {
            InstallerSpec::GithubRelease(s) => github_release::plan(env, manifest, s).await,
            InstallerSpec::Cargo(s) => cargo::plan(env, s).await,
            InstallerSpec::GitCargoBuild(s) => git_build::plan(env, s).await,
        };
        match resolved {
            Ok(resolved) => {
                return Ok(finish_plan(
                    env, manifest, index, spec, resolved, installed, skipped,
                ));
            }
            Err(Error::Install(
                e @ (InstallError::NoMatchingAsset { .. }
                | InstallError::ReleaseNotFound { .. }
                | InstallError::AmbiguousAsset { .. }
                | InstallError::ChecksumUnavailable { .. }),
            )) => {
                skipped.push(format!("{kind}: {e}"));
            }
            Err(e @ Error::Network(_)) => {
                skipped.push(format!("{kind}: {e}"));
                network_error = Some(e);
            }
            Err(e) => return Err(e),
        }
    }
    if let Some(e) = network_error {
        return Err(e);
    }
    if skipped.is_empty() {
        skipped.push("the manifest declares no installer for this operating system".into());
    }
    Err(InstallError::NoCompatibleInstaller {
        game: manifest.name.clone(),
        reasons: skipped,
    }
    .into())
}

fn finish_plan(
    env: &InstallEnv<'_>,
    manifest: &GameManifest,
    index: usize,
    spec: &InstallerSpec,
    resolved: Resolved,
    installed: Option<&InstallRecord>,
    skipped: Vec<String>,
) -> InstallPlan {
    let kind = spec.kind();
    let (version, asset, checksum) = match &resolved {
        Resolved::Release {
            version,
            asset,
            checksum,
            ..
        } => (
            Some(version.clone()),
            Some(asset.name.clone()),
            checksum.policy(),
        ),
        Resolved::Cargo { latest } => (latest.clone(), None, ChecksumPolicy::None),
        Resolved::Git { remote_commit } => (
            remote_commit
                .as_ref()
                .map(|c| format!("git {}", &c[..c.len().min(12)])),
            None,
            ChecksumPolicy::None,
        ),
    };
    let mut warnings: Vec<String> = spec.warnings().to_vec();
    if let Some(n) = &manifest.requirements.notes {
        warnings.push(n.clone());
    }
    if matches!(
        manifest.support_status,
        crate::catalog::SupportStatus::Experimental
    ) {
        warnings.push(
            "This game is marked experimental: the install path has not been verified recently."
                .into(),
        );
    }
    let executable_rel = manifest.binary_for(spec);
    let executable = Path::new(executable_rel)
        .file_name()
        .and_then(|f| f.to_str())
        .unwrap_or(executable_rel)
        .to_string();
    InstallPlan {
        game: manifest.id.clone(),
        name: manifest.name.clone(),
        installer: kind,
        installer_index: index,
        source: spec.source_label(),
        version,
        asset,
        checksum,
        destination: env.paths.game_dir(manifest.id.as_str()).join("current"),
        executable: env.platform.exe_name(&executable),
        tools: required_tools(kind)
            .iter()
            .map(|t| tool_status(env, t))
            .collect(),
        missing_commands: manifest
            .requirements
            .commands
            .iter()
            .filter(|c| which::which(c).is_err())
            .cloned()
            .collect(),
        missing_optional: manifest
            .requirements
            .optional_commands
            .iter()
            .filter(|c| which::which(c).is_err())
            .cloned()
            .collect(),
        warnings,
        requires_network: true,
        compiles: kind.compiles(),
        requires_admin: false,
        is_update: installed.is_some(),
        previous_version: installed.map(|r| r.version.clone()),
        skipped,
        resolved,
    }
}

/// Run the installer chosen by `plan`, producing files inside `staging`.
pub async fn fetch(
    ctx: &JobContext<'_>,
    manifest: &GameManifest,
    plan: &InstallPlan,
    staging: &Path,
) -> Result<Fetched, Error> {
    let spec = plan.spec(manifest).ok_or_else(|| InstallError::Rollback {
        stage: "plan",
        message: "installer index out of range".into(),
    })?;
    match (spec, &plan.resolved) {
        (
            InstallerSpec::GithubRelease(s),
            Resolved::Release {
                tag,
                version,
                asset,
                checksum,
            },
        ) => github_release::fetch(ctx, manifest, s, tag, version, asset, checksum, staging).await,
        (InstallerSpec::Cargo(s), _) => cargo::fetch(ctx, manifest, s, staging).await,
        (InstallerSpec::GitCargoBuild(s), _) => git_build::fetch(ctx, manifest, s, staging).await,
        _ => Err(InstallError::Rollback {
            stage: "plan",
            message: "plan does not match installer".into(),
        }
        .into()),
    }
}

/// Outcome of an update check for one installed game.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UpdateCheck {
    pub game: GameId,
    pub installed: String,
    pub latest: String,
    pub available: bool,
    pub installer: InstallerKind,
}

/// Look up the newest available version for an installed game.
pub async fn check_latest(
    env: &InstallEnv<'_>,
    manifest: &GameManifest,
    record: &InstallRecord,
) -> Result<UpdateCheck, Error> {
    let spec = manifest
        .installers_for(&env.platform)
        .into_iter()
        .find(|s| s.kind() == record.installer)
        .or_else(|| manifest.installers_for(&env.platform).into_iter().next())
        .ok_or_else(|| InstallError::NoCompatibleInstaller {
            game: manifest.name.clone(),
            reasons: vec!["no installer applies to this platform".into()],
        })?;
    let (latest, available) = match spec {
        InstallerSpec::Cargo(s) => {
            let latest = env
                .crates
                .latest_version(&s.krate)
                .await?
                .unwrap_or_else(|| record.version.clone());
            let available = crate::version::is_newer(&record.version, &latest);
            (latest, available)
        }
        InstallerSpec::GithubRelease(s) => {
            let release = env
                .github
                .resolve(&s.repository, s.tag.as_deref(), s.allow_prerelease)
                .await?;
            let latest = release.version();
            let available = crate::version::is_newer(&record.version, &latest);
            (latest, available)
        }
        InstallerSpec::GitCargoBuild(s) => {
            let remote = git_build::remote_commit(env, s).await?;
            match (&record.source, remote) {
                (
                    InstallSource::GitCargoBuild {
                        commit: Some(local),
                        ..
                    },
                    Some(remote),
                ) => {
                    let short = remote[..remote.len().min(12)].to_string();
                    (format!("{} ({short})", record.version), remote != *local)
                }
                (_, Some(remote)) => (
                    format!("{} ({})", record.version, &remote[..remote.len().min(12)]),
                    false,
                ),
                (_, None) => (record.version.clone(), false),
            }
        }
    };
    Ok(UpdateCheck {
        game: manifest.id.clone(),
        installed: record.version.clone(),
        latest,
        available,
        installer: spec.kind(),
    })
}

/// Interpret a cargo/git output line as a phase change.
pub fn classify_line(line: &str) -> Option<(Phase, String)> {
    let t = line.trim();
    let take = |prefix: &str| t.strip_prefix(prefix).map(|r| r.trim().to_string());
    if let Some(rest) = take("Compiling ").or_else(|| take("Building ")) {
        return Some((Phase::Compiling, rest));
    }
    if let Some(rest) = take("Downloading ")
        .or_else(|| take("Downloaded "))
        .or_else(|| take("Cloning into"))
    {
        return Some((Phase::Downloading, rest));
    }
    if let Some(rest) = take("Updating ")
        .or_else(|| take("Resolving "))
        .or_else(|| take("Locking "))
    {
        return Some((Phase::Resolving, rest));
    }
    if let Some(rest) = take("Installing ")
        .or_else(|| take("Installed "))
        .or_else(|| take("Finished "))
    {
        return Some((Phase::Registering, rest));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phases_have_stable_order() {
        assert_eq!(Phase::Resolving.index(), 0);
        assert_eq!(Phase::Ready.index(), 6);
        assert_eq!(Phase::ALL.len(), 7);
    }

    #[test]
    fn classifies_cargo_lines() {
        assert_eq!(
            classify_line("   Compiling ratatui v0.30.2"),
            Some((Phase::Compiling, "ratatui v0.30.2".into()))
        );
        assert_eq!(
            classify_line(" Downloading crates ..."),
            Some((Phase::Downloading, "crates ...".into()))
        );
        assert_eq!(
            classify_line("    Updating crates.io index"),
            Some((Phase::Resolving, "crates.io index".into()))
        );
        assert_eq!(
            classify_line("  Installing /tmp/x/bin/game"),
            Some((Phase::Registering, "/tmp/x/bin/game".into()))
        );
        assert_eq!(classify_line("warning: unused"), None);
    }

    #[test]
    fn progress_sinks_deliver_events() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let p = Progress::new(7, ProgressSink::Channel(tx));
        p.phase(Phase::Downloading, "x");
        match rx.try_recv().unwrap() {
            ProgressEvent::Phase { job, phase, detail } => {
                assert_eq!((job, phase, detail.as_str()), (7, Phase::Downloading, "x"));
            }
            other => panic!("{other:?}"),
        }
        let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
        let s2 = seen.clone();
        let cb = Progress::new(
            1,
            ProgressSink::Callback(Arc::new(move |e: &ProgressEvent| {
                s2.lock().unwrap().push(e.job())
            })),
        );
        cb.bytes(1, None);
        cb.output("line");
        assert_eq!(*seen.lock().unwrap(), vec![1, 1]);
        Progress::silent().phase(Phase::Ready, "");
        assert_eq!(
            ChecksumPolicy::ReleaseFile("SHA256SUMS".into()).label(),
            "SHA-256 from release file SHA256SUMS"
        );
        assert!(!ChecksumPolicy::None.available());
    }
}
