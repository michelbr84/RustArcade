//! `git clone` + `cargo build --release` installer.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Deserialize;

use crate::catalog::manifest::{GameManifest, GitCargoBuildSpec};
use crate::error::{Error, InstallError, SecurityError};
use crate::paths::{nonce, remove_dir_all_retry, safe_relative};
use crate::registry::InstallSource;

use super::archive;
use super::process::{CommandSpec, run_logged};
use super::{Fetched, InstallEnv, JobContext, Phase, Resolved, classify_line};

fn check_repo_url(env: &InstallEnv<'_>, url: &str) -> Result<(), SecurityError> {
    let parsed = url::Url::parse(url).map_err(|_| SecurityError::InsecureUrl {
        url: url.to_string(),
    })?;
    let local_ok = env.http.allows_insecure_local()
        && (parsed.scheme() == "file" || crate::net::http::is_local_loopback(&parsed));
    if parsed.scheme() == "https" || local_ok {
        Ok(())
    } else {
        Err(SecurityError::InsecureUrl {
            url: url.to_string(),
        })
    }
}

/// `git ls-remote` the reference (or HEAD) to learn the remote commit. Best effort.
pub async fn remote_commit(
    env: &InstallEnv<'_>,
    spec: &GitCargoBuildSpec,
) -> Result<Option<String>, Error> {
    check_repo_url(env, &spec.repository)?;
    let Some(git) = env.tools.git.path.clone() else {
        return Ok(None);
    };
    let reference = spec.reference.clone().unwrap_or_else(|| "HEAD".into());
    let output = tokio::time::timeout(
        Duration::from_secs(30),
        tokio::process::Command::new(git)
            .args(["ls-remote", "--", &spec.repository, &reference])
            .env("GIT_TERMINAL_PROMPT", "0")
            .stdin(std::process::Stdio::null())
            .output(),
    )
    .await;
    let Ok(Ok(output)) = output else {
        return Ok(None);
    };
    if !output.status.success() {
        return Ok(None);
    }
    let text = String::from_utf8_lossy(&output.stdout);
    // Prefer the peeled tag (`^{}`) line when present, since it names the commit.
    let mut best: Option<String> = None;
    for line in text.lines() {
        let mut parts = line.split_whitespace();
        let (Some(sha), Some(name)) = (parts.next(), parts.next()) else {
            continue;
        };
        if sha.len() != 40 {
            continue;
        }
        if name.ends_with("^{}") {
            return Ok(Some(sha.to_string()));
        }
        if best.is_none() {
            best = Some(sha.to_string());
        }
    }
    Ok(best)
}

pub async fn plan(env: &InstallEnv<'_>, spec: &GitCargoBuildSpec) -> Result<Resolved, Error> {
    let remote_commit = match remote_commit(env, spec).await {
        Ok(c) => c,
        Err(Error::Security(e)) => return Err(e.into()),
        Err(e) => {
            tracing::warn!("ls-remote failed for {}: {e}", spec.repository);
            None
        }
    };
    if let (Some(pinned), Some(remote)) = (&spec.commit, &remote_commit)
        && pinned != remote
    {
        return Err(InstallError::CommitMismatch {
            expected: pinned.clone(),
            actual: remote.clone(),
        }
        .into());
    }
    Ok(Resolved::Git { remote_commit })
}

/// Build the `git clone` command.
pub fn clone_command(git: &Path, spec: &GitCargoBuildSpec, dest: &Path) -> CommandSpec {
    let mut cmd =
        CommandSpec::new(git).args(["clone", "--depth", "1", "--single-branch", "--no-tags"]);
    if let Some(r) = &spec.reference {
        cmd = cmd.args(["--branch", r]);
    }
    cmd.arg("--")
        .arg(&spec.repository)
        .arg(dest)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_CONFIG_NOSYSTEM", "1")
}

/// Build the `cargo build --release` command.
pub fn build_command(
    cargo: &Path,
    spec: &GitCargoBuildSpec,
    src_dir: &Path,
    target_dir: &Path,
    locked: bool,
) -> CommandSpec {
    let mut cmd = CommandSpec::new(cargo).args(["build", "--release", "--color", "never"]);
    if locked {
        cmd = cmd.arg("--locked");
    }
    if let Some(p) = &spec.package {
        cmd = cmd.args(["--package", p]);
    }
    if !spec.default_features {
        cmd = cmd.arg("--no-default-features");
    }
    if !spec.features.is_empty() {
        cmd = cmd.args(["--features", &spec.features.join(",")]);
    }
    for bin in &spec.bins {
        cmd = cmd.args(["--bin", bin]);
    }
    cmd.cwd(src_dir)
        .env("CARGO_TARGET_DIR", &target_dir.display().to_string())
        .env("CARGO_TERM_COLOR", "never")
        .env("CARGO_TERM_PROGRESS_WHEN", "never")
        .env("GIT_TERMINAL_PROMPT", "0")
}

#[derive(Debug, Deserialize)]
struct Metadata {
    packages: Vec<Package>,
}

#[derive(Debug, Deserialize)]
struct Package {
    name: String,
    version: String,
    #[serde(default)]
    targets: Vec<Target>,
}

#[derive(Debug, Deserialize)]
struct Target {
    name: String,
    kind: Vec<String>,
}

/// Version of the package that produces `binary` (from `cargo metadata`).
pub fn package_version(metadata_json: &str, package: Option<&str>, binary: &str) -> Option<String> {
    let meta: Metadata = serde_json::from_str(metadata_json).ok()?;
    if let Some(name) = package
        && let Some(p) = meta.packages.iter().find(|p| p.name == name)
    {
        return Some(p.version.clone());
    }
    if let Some(p) = meta.packages.iter().find(|p| {
        p.targets
            .iter()
            .any(|t| t.kind.iter().any(|k| k == "bin") && t.name == binary)
    }) {
        return Some(p.version.clone());
    }
    if meta.packages.len() == 1 {
        return Some(meta.packages[0].version.clone());
    }
    None
}

async fn git_head(git: &Path, dir: &Path) -> Option<String> {
    let output = tokio::process::Command::new(git)
        .args(["-C"])
        .arg(dir)
        .args(["rev-parse", "HEAD"])
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(std::process::Stdio::null())
        .output()
        .await
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .filter(|s| s.len() == 40)
}

async fn cargo_metadata(cargo: &Path, dir: &Path) -> Option<String> {
    let output = tokio::process::Command::new(cargo)
        .args([
            "metadata",
            "--no-deps",
            "--format-version",
            "1",
            "--offline",
        ])
        .current_dir(dir)
        .env("CARGO_TERM_COLOR", "never")
        .stdin(std::process::Stdio::null())
        .output()
        .await
        .ok()?;
    if output.status.success() {
        return Some(String::from_utf8_lossy(&output.stdout).to_string());
    }
    // Retry without --offline (metadata may need the index for path deps).
    let output = tokio::process::Command::new(cargo)
        .args(["metadata", "--no-deps", "--format-version", "1"])
        .current_dir(dir)
        .env("CARGO_TERM_COLOR", "never")
        .stdin(std::process::Stdio::null())
        .output()
        .await
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).to_string())
}

/// Clone, build, and copy the executable into `staging/bin`.
pub async fn fetch(
    ctx: &JobContext<'_>,
    manifest: &GameManifest,
    spec: &GitCargoBuildSpec,
    staging: &Path,
) -> Result<Fetched, Error> {
    let env = &ctx.env;
    check_repo_url(env, &spec.repository)?;
    let git = env
        .tools
        .git
        .path
        .clone()
        .ok_or_else(|| InstallError::ToolMissing {
            tool: "git".into(),
            hint: super::tool_hint("git"),
        })?;
    let cargo = env
        .tools
        .cargo
        .path
        .clone()
        .ok_or_else(|| InstallError::ToolMissing {
            tool: "cargo".into(),
            hint: super::tool_hint("cargo"),
        })?;
    let work = env
        .paths
        .build_dir()
        .join(format!("{}-{}", manifest.id, nonce()));
    let src = work.join("src");
    let target = work.join("target");
    std::fs::create_dir_all(&work).map_err(|e| InstallError::io("create", &work, e))?;

    let result = build_in(ctx, manifest, spec, &git, &cargo, &src, &target, staging).await;
    if let Err(e) = remove_dir_all_retry(&work) {
        tracing::warn!("could not remove build directory {}: {e}", work.display());
    }
    result
}

#[allow(clippy::too_many_arguments)]
async fn build_in(
    ctx: &JobContext<'_>,
    manifest: &GameManifest,
    spec: &GitCargoBuildSpec,
    git: &Path,
    cargo: &Path,
    src: &Path,
    target: &Path,
    staging: &Path,
) -> Result<Fetched, Error> {
    let env = &ctx.env;
    ctx.check_cancel()?;
    ctx.progress
        .phase(Phase::Downloading, format!("Cloning {}", spec.repository));
    let progress = ctx.progress.clone();
    run_logged(
        &clone_command(git, spec, src),
        &ctx.log,
        |line| progress.output(line),
        &ctx.cancel,
    )
    .await?
    .check("git clone", Some(ctx.log.path()))?;

    let head = git_head(git, src).await;
    if let Some(pinned) = &spec.commit {
        match &head {
            Some(actual) if actual == pinned => {}
            Some(actual) => {
                return Err(InstallError::CommitMismatch {
                    expected: pinned.clone(),
                    actual: actual.clone(),
                }
                .into());
            }
            None => {
                return Err(InstallError::CommitMismatch {
                    expected: pinned.clone(),
                    actual: "unknown".into(),
                }
                .into());
            }
        }
    }

    let package_dir = match &spec.path {
        Some(p) => src.join(safe_relative(p, 4)?),
        None => src.to_path_buf(),
    };
    if !package_dir.join("Cargo.toml").is_file() {
        return Err(InstallError::BinaryNotFound {
            expected: "Cargo.toml".into(),
            found: vec![format!("{} has no Cargo.toml", package_dir.display())],
        }
        .into());
    }
    let locked = spec.locked
        && (package_dir.join("Cargo.lock").is_file() || src.join("Cargo.lock").is_file());

    ctx.check_cancel()?;
    ctx.progress.phase(
        Phase::Compiling,
        format!("cargo build --release ({})", manifest.id),
    );
    let progress = ctx.progress.clone();
    run_logged(
        &build_command(cargo, spec, &package_dir, target, locked),
        &ctx.log,
        |line| {
            if let Some((phase, detail)) = classify_line(line) {
                progress.phase(phase, detail);
            }
            progress.output(line);
        },
        &ctx.cancel,
    )
    .await?
    .check("cargo build", Some(ctx.log.path()))?;

    let binary = spec.binary.as_deref().unwrap_or(&manifest.run.executable);
    let exe_name = env.platform.exe_name(binary);
    let built = target.join("release").join(&exe_name);
    if !built.is_file() {
        return Err(InstallError::BinaryNotFound {
            expected: exe_name,
            found: std::fs::read_dir(target.join("release"))
                .map(|rd| {
                    rd.flatten()
                        .filter(|e| e.path().is_file())
                        .map(|e| e.file_name().to_string_lossy().to_string())
                        .take(20)
                        .collect()
                })
                .unwrap_or_default(),
        }
        .into());
    }
    let bin_dir = staging.join("bin");
    std::fs::create_dir_all(&bin_dir).map_err(|e| InstallError::io("create", &bin_dir, e))?;
    let dest = bin_dir.join(&exe_name);
    std::fs::copy(&built, &dest).map_err(|e| InstallError::io("copy", &dest, e))?;
    archive::make_executable(&dest)?;

    let metadata = cargo_metadata(cargo, &package_dir).await;
    let version = metadata
        .as_deref()
        .and_then(|m| package_version(m, spec.package.as_deref(), binary))
        .or_else(|| head.as_ref().map(|h| format!("git-{}", &h[..7])))
        .unwrap_or_else(|| "unknown".into());
    Ok(Fetched {
        executable: dest,
        version,
        source: InstallSource::GitCargoBuild {
            repository: spec.repository.clone(),
            reference: spec.reference.clone(),
            commit: head,
        },
        checksum_verified: false,
    })
}

/// Location of the cache/build work directories for cleanup.
pub fn build_work_dirs(build_dir: &Path, game: &str) -> Vec<PathBuf> {
    std::fs::read_dir(build_dir)
        .map(|rd| {
            rd.flatten()
                .map(|e| e.path())
                .filter(|p| {
                    p.file_name()
                        .and_then(|n| n.to_str())
                        .is_some_and(|n| n.starts_with(&format!("{game}-")))
                })
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec() -> GitCargoBuildSpec {
        GitCargoBuildSpec {
            repository: "https://github.com/o/r".into(),
            reference: Some("v1.0.0".into()),
            commit: None,
            path: None,
            package: Some("game".into()),
            features: vec!["x".into()],
            default_features: false,
            locked: true,
            bins: vec!["game".into()],
            os: vec![],
            binary: None,
            warnings: vec![],
        }
    }

    #[test]
    fn clone_and_build_commands_are_structured() {
        let c = clone_command(Path::new("git"), &spec(), Path::new("/w/src"));
        let args: Vec<String> = c
            .args
            .iter()
            .map(|a| a.to_string_lossy().to_string())
            .collect();
        assert_eq!(
            args,
            vec![
                "clone",
                "--depth",
                "1",
                "--single-branch",
                "--no-tags",
                "--branch",
                "v1.0.0",
                "--",
                "https://github.com/o/r",
                "/w/src"
            ]
        );
        let b = build_command(
            Path::new("cargo"),
            &spec(),
            Path::new("/w/src"),
            Path::new("/w/target"),
            true,
        );
        let args: Vec<String> = b
            .args
            .iter()
            .map(|a| a.to_string_lossy().to_string())
            .collect();
        assert_eq!(
            args,
            vec![
                "build",
                "--release",
                "--color",
                "never",
                "--locked",
                "--package",
                "game",
                "--no-default-features",
                "--features",
                "x",
                "--bin",
                "game"
            ]
        );
        assert!(
            b.env
                .iter()
                .any(|(k, v)| k == "CARGO_TARGET_DIR" && v == "/w/target")
        );
    }

    #[test]
    fn package_version_lookup() {
        let json = r#"{"packages":[
            {"name":"core","version":"0.1.0","targets":[{"name":"core","kind":["lib"]}]},
            {"name":"game","version":"2.3.4","targets":[{"name":"game","kind":["bin"]}]}]}"#;
        assert_eq!(
            package_version(json, Some("game"), "game").as_deref(),
            Some("2.3.4")
        );
        assert_eq!(
            package_version(json, None, "game").as_deref(),
            Some("2.3.4")
        );
        assert_eq!(package_version(json, None, "nope"), None);
        let single = r#"{"packages":[{"name":"solo","version":"9.9.9","targets":[]}]}"#;
        assert_eq!(
            package_version(single, None, "anything").as_deref(),
            Some("9.9.9")
        );
    }
}
