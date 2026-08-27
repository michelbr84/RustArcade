//! Transactional install / update / uninstall of managed game directories.
//!
//! Layout: `data/games/<id>/current` holds the live installation. Installs build in
//! `staging-<nonce>`, are verified, then swapped in; the previous version survives as
//! `previous-<nonce>` until the registry has been updated.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use chrono::Utc;
use tokio_util::sync::CancellationToken;

use crate::catalog::manifest::{GameId, GameManifest};
use crate::error::{Error, InstallError, SecurityError, StorageError};
use crate::logging::InstallLog;
use crate::paths::{is_within, nonce, remove_dir_all_retry, rename_with_retry};
use crate::registry::{InstallRecord, ManagedKind, ManagedPath, Registry};

use super::archive;
use super::{Fetched, InstallEnv, InstallPlan, JobContext, Phase, Progress};

/// What an install/update produced.
#[derive(Debug, Clone)]
pub struct InstallOutcome {
    pub record: InstallRecord,
    pub previous_version: Option<String>,
    pub log: PathBuf,
    pub warnings: Vec<String>,
}

/// What an uninstall removed.
#[derive(Debug, Clone, Default)]
pub struct UninstallReport {
    pub game: String,
    pub removed: Vec<PathBuf>,
    pub warnings: Vec<String>,
}

/// Leftovers cleaned at startup.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SweepReport {
    pub removed: Vec<PathBuf>,
    pub restored: Vec<PathBuf>,
}

/// Directory name prefixes used by the transaction.
pub const STAGING_PREFIX: &str = "staging-";
pub const PREVIOUS_PREFIX: &str = "previous-";
pub const CURRENT_DIR: &str = "current";

/// Perform a full install (or update) transaction.
pub async fn run(
    env: InstallEnv<'_>,
    manifest: &GameManifest,
    plan: &InstallPlan,
    progress: Progress,
    cancel: CancellationToken,
    registry: &Mutex<Registry>,
) -> Result<InstallOutcome, Error> {
    let kind = if plan.is_update { "update" } else { "install" };
    let log = Arc::new(InstallLog::create(
        &env.paths.logs_dir(),
        kind,
        manifest.id.as_str(),
    )?);
    let log_path = log.path().to_path_buf();
    progress.started(&manifest.id, &log_path);
    log.line(&format!(
        "method: {} source: {} version: {}",
        plan.installer,
        plan.source,
        plan.version_label()
    ));

    let ctx = JobContext {
        env,
        progress: progress.clone(),
        log: log.clone(),
        cancel,
    };
    let result = run_inner(&ctx, manifest, plan, registry).await;
    match &result {
        Ok(outcome) => {
            log.section(&format!(
                "SUCCESS {} {}",
                manifest.id, outcome.record.version
            ));
            progress.phase(
                Phase::Ready,
                format!("{} {}", manifest.name, outcome.record.version),
            );
            progress.finished(
                &manifest.id,
                true,
                format!("{} {} is ready", manifest.name, outcome.record.version),
            );
        }
        Err(e) => {
            log.section(&format!("FAILED: {e}"));
            progress.finished(&manifest.id, false, e.to_string());
        }
    }
    result.map_err(|e| {
        let action: &'static str = if plan.is_update { "update" } else { "install" };
        e.in_context(action, &manifest.name, Some(log_path.clone()))
    })
}

async fn run_inner(
    ctx: &JobContext<'_>,
    manifest: &GameManifest,
    plan: &InstallPlan,
    registry: &Mutex<Registry>,
) -> Result<InstallOutcome, Error> {
    let env = &ctx.env;
    let paths = env.paths;
    let game_dir = paths.game_dir(manifest.id.as_str());
    fs::create_dir_all(&game_dir).map_err(|e| InstallError::io("create", &game_dir, e))?;
    let staging = game_dir.join(format!("{STAGING_PREFIX}{}", nonce()));
    fs::create_dir_all(&staging).map_err(|e| InstallError::io("create", &staging, e))?;
    paths.ensure_managed(&staging)?;

    let fetched = match super::fetch(ctx, manifest, plan, &staging).await {
        Ok(f) => f,
        Err(e) => {
            cleanup_staging(&staging, &ctx.log);
            return Err(e);
        }
    };
    if let Err(e) = verify(ctx, &staging, &fetched) {
        cleanup_staging(&staging, &ctx.log);
        return Err(e);
    }

    ctx.progress.phase(Phase::Registering, "updating registry");
    match commit(ctx, manifest, plan, &staging, fetched, registry) {
        Ok(outcome) => Ok(outcome),
        Err(e) => {
            cleanup_staging(&staging, &ctx.log);
            Err(e)
        }
    }
}

fn verify(ctx: &JobContext<'_>, staging: &Path, fetched: &Fetched) -> Result<(), Error> {
    ctx.progress.phase(Phase::Verifying, "checking executable");
    if !is_within(staging, &fetched.executable) {
        return Err(SecurityError::PathOutsideManagedRoot {
            path: fetched.executable.clone(),
        }
        .into());
    }
    archive::verify_executable(&fetched.executable)?;
    archive::make_executable(&fetched.executable)?;
    ctx.log.line(&format!(
        "verified executable {}",
        fetched.executable.display()
    ));
    Ok(())
}

fn commit(
    ctx: &JobContext<'_>,
    manifest: &GameManifest,
    plan: &InstallPlan,
    staging: &Path,
    fetched: Fetched,
    registry: &Mutex<Registry>,
) -> Result<InstallOutcome, Error> {
    let paths = ctx.env.paths;
    let game_dir = paths.game_dir(manifest.id.as_str());
    let current = game_dir.join(CURRENT_DIR);
    let previous = game_dir.join(format!("{PREVIOUS_PREFIX}{}", nonce()));
    let mut warnings = Vec::new();

    // Step a: move the live installation aside.
    let had_current = current.exists();
    if had_current {
        rename_with_retry(&current, &previous)
            .map_err(|e| InstallError::io("rename", &current, e))?;
    }
    // Step b: promote staging.
    if let Err(e) = rename_with_retry(staging, &current) {
        if had_current {
            let _ = rename_with_retry(&previous, &current);
        }
        return Err(InstallError::io("rename", staging, e).into());
    }

    // Step c: convenience launcher in bin/.
    let exe_rel = fetched
        .executable
        .strip_prefix(staging)
        .map(Path::to_path_buf)
        .unwrap_or_else(|_| PathBuf::from("bin").join(&plan.executable));
    let executable_abs = current.join(&exe_rel);
    let executable_rel = executable_abs
        .strip_prefix(paths.data_dir())
        .map(Path::to_path_buf)
        .map_err(|_| SecurityError::PathOutsideManagedRoot {
            path: executable_abs.clone(),
        })?;
    // The convenience launcher is named after the manifest's unique `run.executable`.
    let bin_name = ctx.env.platform.exe_name(&manifest.run.executable);
    let bin_path = paths.bin_dir().join(&bin_name);
    let bin_link = match create_launcher(&executable_abs, &bin_path) {
        Ok(kind) => Some((
            bin_path
                .strip_prefix(paths.data_dir())
                .map(Path::to_path_buf)
                .unwrap_or(bin_path.clone()),
            kind,
        )),
        Err(e) => {
            warnings.push(format!(
                "could not create launcher {}: {e}",
                bin_path.display()
            ));
            None
        }
    };

    // Step d: registry.
    let now = Utc::now();
    let mut managed = vec![ManagedPath {
        path: game_dir
            .strip_prefix(paths.data_dir())
            .map(Path::to_path_buf)
            .unwrap_or(game_dir.clone()),
        kind: ManagedKind::Dir,
    }];
    if let Some((rel, kind)) = &bin_link {
        managed.push(ManagedPath {
            path: rel.clone(),
            kind: *kind,
        });
    }
    let (installed_at, previous_version) = {
        let reg = registry.lock().map_err(|_| StorageError::Corrupt {
            path: paths.registry_file(),
            reason: "registry lock poisoned".into(),
        })?;
        match reg.get(&manifest.id) {
            Some(old) => (old.installed_at, Some(old.version.clone())),
            None => (now, None),
        }
    };
    let record = InstallRecord {
        id: manifest.id.clone(),
        name: manifest.name.clone(),
        version: fetched.version.clone(),
        installer: plan.installer,
        executable: executable_rel,
        bin_link: bin_link.as_ref().map(|(rel, _)| rel.clone()),
        repository: manifest.repository.clone(),
        source: fetched.source.clone(),
        installed_at,
        updated_at: now,
        managed,
        log: Some(ctx.log.path().to_path_buf()),
        checksum_verified: fetched.checksum_verified,
    };
    let saved = {
        let mut reg = registry.lock().map_err(|_| StorageError::Corrupt {
            path: paths.registry_file(),
            reason: "registry lock poisoned".into(),
        })?;
        reg.upsert(record.clone())
    };
    if let Err(e) = saved {
        // Roll the swap back so the previous version stays live.
        let _ = rename_with_retry(&current, staging);
        if had_current {
            let _ = rename_with_retry(&previous, &current);
        }
        return Err(Error::Storage(e));
    }

    // Step e: drop the old version.
    if had_current && let Err(e) = remove_dir_all_retry(&previous) {
        warnings.push(format!(
            "previous version left at {}: {e}",
            previous.display()
        ));
    }
    let download_dir = paths.downloads_dir().join(manifest.id.as_str());
    if !ctx.env.config.install.keep_downloads {
        let _ = remove_dir_all_retry(&download_dir);
    }
    ctx.log.line(&format!(
        "registered {} {} at {}",
        manifest.id,
        record.version,
        executable_abs.display()
    ));
    Ok(InstallOutcome {
        record,
        previous_version,
        log: ctx.log.path().to_path_buf(),
        warnings,
    })
}

fn cleanup_staging(staging: &Path, log: &InstallLog) {
    match remove_dir_all_retry(staging) {
        Ok(()) => log.line(&format!("removed staging directory {}", staging.display())),
        Err(e) => log.line(&format!(
            "could not remove staging directory {}: {e}",
            staging.display()
        )),
    }
}

/// Create `bin/<name>` pointing at the installed executable.
fn create_launcher(target: &Path, link: &Path) -> std::io::Result<ManagedKind> {
    if let Some(parent) = link.parent() {
        fs::create_dir_all(parent)?;
    }
    if fs::symlink_metadata(link).is_ok() {
        fs::remove_file(link)?;
    }
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target, link)?;
        Ok(ManagedKind::Symlink)
    }
    #[cfg(not(unix))]
    {
        fs::copy(target, link)?;
        Ok(ManagedKind::File)
    }
}

/// Remove everything RustArcade created for `id` (never anything outside the data dir).
pub fn uninstall(
    paths: &crate::paths::AppPaths,
    registry: &Mutex<Registry>,
    id: &GameId,
) -> Result<UninstallReport, Error> {
    let record = {
        let reg = registry.lock().map_err(|_| StorageError::Corrupt {
            path: paths.registry_file(),
            reason: "registry lock poisoned".into(),
        })?;
        reg.get(id)
            .cloned()
            .ok_or_else(|| InstallError::NotInstalled(id.to_string()))?
    };
    let mut report = UninstallReport {
        game: record.name.clone(),
        ..Default::default()
    };
    // Validate every path before touching anything.
    let targets = record.managed_paths(paths);
    for (path, _) in &targets {
        paths.ensure_managed(path)?;
        if path == paths.data_dir() || path == &paths.games_dir() || path == &paths.bin_dir() {
            return Err(SecurityError::PathOutsideManagedRoot { path: path.clone() }.into());
        }
    }
    // Links/files first, then directories.
    let mut ordered = targets.clone();
    ordered.sort_by_key(|(_, kind)| {
        if matches!(kind, ManagedKind::Dir) {
            1
        } else {
            0
        }
    });
    for (path, kind) in ordered {
        let result = match kind {
            ManagedKind::Dir => remove_dir_all_retry(&path),
            ManagedKind::File | ManagedKind::Symlink => match fs::remove_file(&path) {
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                other => other,
            },
        };
        match result {
            Ok(()) => report.removed.push(path),
            Err(e) => report
                .warnings
                .push(format!("could not remove {}: {e}", path.display())),
        }
    }
    let _ = remove_dir_all_retry(&paths.downloads_dir().join(id.as_str()));
    let mut reg = registry.lock().map_err(|_| StorageError::Corrupt {
        path: paths.registry_file(),
        reason: "registry lock poisoned".into(),
    })?;
    reg.remove(id)?;
    Ok(report)
}

/// Clean interrupted transactions: remove `staging-*`, restore or delete `previous-*`.
pub fn sweep(paths: &crate::paths::AppPaths) -> SweepReport {
    let mut report = SweepReport::default();
    let Ok(games) = fs::read_dir(paths.games_dir()) else {
        return report;
    };
    for game in games.flatten().map(|e| e.path()).filter(|p| p.is_dir()) {
        let Ok(entries) = fs::read_dir(&game) else {
            continue;
        };
        let current = game.join(CURRENT_DIR);
        for entry in entries.flatten().map(|e| e.path()) {
            let name = entry
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_string();
            if name.starts_with(STAGING_PREFIX) {
                if remove_dir_all_retry(&entry).is_ok() {
                    report.removed.push(entry);
                }
            } else if name.starts_with(PREVIOUS_PREFIX) {
                if current.exists() {
                    if remove_dir_all_retry(&entry).is_ok() {
                        report.removed.push(entry);
                    }
                } else if rename_with_retry(&entry, &current).is_ok() {
                    report.restored.push(current.clone());
                }
            }
        }
    }
    // Stale build directories.
    if let Ok(builds) = fs::read_dir(paths.build_dir()) {
        for b in builds.flatten().map(|e| e.path()) {
            if remove_dir_all_retry(&b).is_ok() {
                report.removed.push(b);
            }
        }
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::AppPaths;

    #[test]
    fn sweep_removes_staging_and_restores_previous() {
        let root = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_root(root.path());
        paths.ensure().unwrap();
        let g = paths.game_dir("demo");
        fs::create_dir_all(g.join("staging-1/bin")).unwrap();
        fs::create_dir_all(g.join("previous-1/bin")).unwrap();
        fs::write(g.join("previous-1/bin/demo"), "x").unwrap();
        fs::create_dir_all(paths.build_dir().join("demo-123")).unwrap();
        let report = sweep(&paths);
        assert!(!g.join("staging-1").exists());
        assert!(g.join("current/bin/demo").is_file());
        assert_eq!(report.restored, vec![g.join("current")]);
        assert!(report.removed.iter().any(|p| p.ends_with("staging-1")));
        assert!(report.removed.iter().any(|p| p.ends_with("demo-123")));
        // With current present, previous is deleted.
        fs::create_dir_all(g.join("previous-2")).unwrap();
        let report = sweep(&paths);
        assert!(!g.join("previous-2").exists());
        assert!(report.restored.is_empty());
    }

    #[test]
    fn uninstall_only_touches_managed_paths() {
        let root = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_root(root.path());
        paths.ensure().unwrap();
        let reg_path = paths.registry_file();
        let (mut reg, _) = Registry::load(&reg_path).unwrap();
        let mut record = crate::registry::tests::sample("demo");
        record.managed = vec![
            ManagedPath {
                path: PathBuf::from("games/demo"),
                kind: ManagedKind::Dir,
            },
            ManagedPath {
                path: PathBuf::from("bin/demo"),
                kind: ManagedKind::File,
            },
        ];
        reg.upsert(record).unwrap();
        fs::create_dir_all(paths.game_dir("demo").join("current/bin")).unwrap();
        fs::write(paths.game_dir("demo").join("current/bin/demo"), "x").unwrap();
        fs::write(paths.bin_dir().join("demo"), "x").unwrap();
        let foreign = root.path().join("keep.txt");
        fs::write(&foreign, "keep").unwrap();
        let registry = Mutex::new(reg);
        let report = uninstall(&paths, &registry, &"demo".parse().unwrap()).unwrap();
        assert_eq!(report.removed.len(), 2);
        assert!(!paths.game_dir("demo").exists());
        assert!(!paths.bin_dir().join("demo").exists());
        assert!(foreign.exists());
        assert!(
            registry
                .lock()
                .unwrap()
                .get(&"demo".parse().unwrap())
                .is_none()
        );
        assert!(matches!(
            uninstall(&paths, &registry, &"demo".parse().unwrap()),
            Err(Error::Install(InstallError::NotInstalled(_)))
        ));
    }

    #[test]
    fn uninstall_refuses_paths_outside_data_dir() {
        let root = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_root(root.path());
        paths.ensure().unwrap();
        let (mut reg, _) = Registry::load(&paths.registry_file()).unwrap();
        let mut record = crate::registry::tests::sample("evil");
        record.managed = vec![ManagedPath {
            path: PathBuf::from("../../etc"),
            kind: ManagedKind::Dir,
        }];
        reg.upsert(record).unwrap();
        let registry = Mutex::new(reg);
        let err = uninstall(&paths, &registry, &"evil".parse().unwrap()).unwrap_err();
        assert!(err.is_security());
        // Registry untouched.
        assert!(
            registry
                .lock()
                .unwrap()
                .get(&"evil".parse().unwrap())
                .is_some()
        );
    }
}
