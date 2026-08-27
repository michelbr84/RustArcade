//! `cargo install --root <staging> <crate>` installer.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::catalog::manifest::{CargoSpec, GameManifest};
use crate::error::{Error, InstallError};
use crate::registry::InstallSource;

use super::process::{CommandSpec, run_logged};
use super::{Fetched, InstallEnv, JobContext, Phase, Resolved, classify_line};

/// Resolve the latest crate version for the plan (best effort; offline yields `None`).
pub async fn plan(env: &InstallEnv<'_>, spec: &CargoSpec) -> Result<Resolved, Error> {
    let latest = match env.crates.latest_version(&spec.krate).await {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!("could not query crates.io for {}: {e}", spec.krate);
            None
        }
    };
    Ok(Resolved::Cargo { latest })
}

/// Build the `cargo install` command line for `spec` targeting `root`.
pub fn install_command(cargo: &Path, spec: &CargoSpec, root: &Path) -> CommandSpec {
    let mut cmd = CommandSpec::new(cargo)
        .args(["install", "--root"])
        .arg(root)
        .args(["--color", "never"]);
    if spec.locked {
        cmd = cmd.arg("--locked");
    }
    if let Some(v) = &spec.version {
        cmd = cmd.args(["--version", v]);
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
    cmd = cmd.arg(&spec.krate);
    cmd.env("CARGO_TERM_COLOR", "never")
        .env("CARGO_TERM_PROGRESS_WHEN", "never")
        .env("GIT_TERMINAL_PROMPT", "0")
}

#[derive(Debug, Deserialize)]
struct Crates2 {
    #[serde(default)]
    installs: HashMap<String, serde_json::Value>,
}

/// Read the installed version of `krate` from `<root>/.crates2.json`.
pub fn installed_version(root: &Path, krate: &str) -> Option<String> {
    let text = std::fs::read_to_string(root.join(".crates2.json")).ok()?;
    let parsed: Crates2 = serde_json::from_str(&text).ok()?;
    parsed.installs.keys().find_map(|key| {
        let mut parts = key.split(' ');
        let name = parts.next()?;
        let version = parts.next()?;
        (name == krate).then(|| version.to_string())
    })
}

/// Run `cargo install` into `staging` and locate the produced executable.
pub async fn fetch(
    ctx: &JobContext<'_>,
    manifest: &GameManifest,
    spec: &CargoSpec,
    staging: &Path,
) -> Result<Fetched, Error> {
    let cargo = ctx
        .env
        .tools
        .cargo
        .path
        .clone()
        .ok_or_else(|| InstallError::ToolMissing {
            tool: "cargo".into(),
            hint: super::tool_hint("cargo"),
        })?;
    ctx.check_cancel()?;
    ctx.progress
        .phase(Phase::Resolving, format!("crates.io {}", spec.krate));
    let cmd = install_command(&cargo, spec, staging);
    let progress = ctx.progress.clone();
    let outcome = run_logged(
        &cmd,
        &ctx.log,
        |line| {
            if let Some((phase, detail)) = classify_line(line) {
                progress.phase(phase, detail);
            }
            progress.output(line);
        },
        &ctx.cancel,
    )
    .await?;
    outcome.check("cargo install", Some(ctx.log.path()))?;

    let version =
        installed_version(staging, &spec.krate).ok_or_else(|| InstallError::VersionUnknown {
            detail: format!("{} has no entry in .crates2.json", spec.krate),
        })?;
    let binary = spec.binary.as_deref().unwrap_or(&manifest.run.executable);
    let exe = staging.join("bin").join(ctx.env.platform.exe_name(binary));
    if !exe.is_file() {
        let found = list_bin_dir(&staging.join("bin"));
        return Err(InstallError::BinaryNotFound {
            expected: ctx.env.platform.exe_name(binary),
            found,
        }
        .into());
    }
    Ok(Fetched {
        executable: exe,
        version: version.clone(),
        source: InstallSource::Cargo {
            krate: spec.krate.clone(),
            version,
        },
        checksum_verified: false,
    })
}

fn list_bin_dir(dir: &Path) -> Vec<String> {
    std::fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .map(|e| e.file_name().to_string_lossy().to_string())
                .collect()
        })
        .unwrap_or_default()
}

/// Path of the executable inside a cargo-style root.
pub fn bin_path(root: &Path, exe_name: &str) -> PathBuf {
    root.join("bin").join(exe_name)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec() -> CargoSpec {
        CargoSpec {
            krate: "demo-game".into(),
            version: Some("^1.2".into()),
            features: vec!["a".into(), "b".into()],
            default_features: false,
            locked: true,
            bins: vec!["demo-game".into()],
            os: vec![],
            binary: None,
            warnings: vec![],
        }
    }

    #[test]
    fn builds_structured_arguments() {
        let cmd = install_command(
            Path::new("/usr/bin/cargo"),
            &spec(),
            Path::new("/tmp/staging"),
        );
        let args: Vec<String> = cmd
            .args
            .iter()
            .map(|a| a.to_string_lossy().to_string())
            .collect();
        assert_eq!(
            args,
            vec![
                "install",
                "--root",
                "/tmp/staging",
                "--color",
                "never",
                "--locked",
                "--version",
                "^1.2",
                "--no-default-features",
                "--features",
                "a,b",
                "--bin",
                "demo-game",
                "demo-game"
            ]
        );
        assert!(
            cmd.env
                .iter()
                .any(|(k, v)| k == "GIT_TERMINAL_PROMPT" && v == "0")
        );
        assert!(cmd.display().starts_with("/usr/bin/cargo install"));
    }

    #[test]
    fn parses_crates2_json() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join(".crates2.json"),
            r#"{"installs":{"demo-game 1.4.2 (registry+https://github.com/rust-lang/crates.io-index)":{"bins":["demo-game"]},"other 0.1.0 (registry+x)":{}}}"#,
        )
        .unwrap();
        assert_eq!(
            installed_version(dir.path(), "demo-game").as_deref(),
            Some("1.4.2")
        );
        assert_eq!(installed_version(dir.path(), "missing"), None);
        assert_eq!(installed_version(Path::new("/nope"), "x"), None);
    }
}
