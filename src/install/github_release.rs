//! GitHub release installer: resolve → select asset → download → verify → extract → install.

use std::path::Path;

use crate::catalog::manifest::{GameManifest, GithubReleaseSpec};
use crate::error::{Error, InstallError, SecurityError};
use crate::net::ReleaseAsset;
use crate::paths::{remove_dir_all_retry, safe_relative};
use crate::registry::InstallSource;

use super::archive;
use super::assets::{expand_checksum_pattern, manifest_digest, parse_checksum_file, select_asset};
use super::{ChecksumSource, InstallEnv, JobContext, Phase, Resolved};

/// Resolve the release and pick the asset + checksum source for this platform.
pub async fn plan(
    env: &InstallEnv<'_>,
    manifest: &GameManifest,
    spec: &GithubReleaseSpec,
) -> Result<Resolved, Error> {
    let release = env
        .github
        .resolve(&spec.repository, spec.tag.as_deref(), spec.allow_prerelease)
        .await?;
    let version = release.version();
    let asset = select_asset(
        &release.assets,
        spec,
        &env.platform,
        &version,
        &release.tag_name,
    )?
    .clone();
    env.http.check_url(&asset.browser_download_url)?;

    let checksum = if let Some(hex) = manifest_digest(&spec.sha256, &asset.name) {
        ChecksumSource::Manifest(hex)
    } else if let Some(hex) = asset.sha256_digest() {
        ChecksumSource::ApiDigest(hex)
    } else if let Some(pattern) = &spec.checksum_asset {
        let name = expand_checksum_pattern(
            pattern,
            &asset.name,
            &version,
            &release.tag_name,
            &env.platform,
        );
        match release
            .assets
            .iter()
            .find(|a| a.name.eq_ignore_ascii_case(&name))
        {
            Some(file) => ChecksumSource::ReleaseFile {
                name: file.name.clone(),
                url: file.browser_download_url.clone(),
            },
            None => ChecksumSource::None,
        }
    } else {
        ChecksumSource::None
    };
    if env.config.install.require_checksum && matches!(checksum, ChecksumSource::None) {
        return Err(InstallError::ChecksumUnavailable {
            asset: asset.name.clone(),
        }
        .into());
    }
    let _ = manifest;
    Ok(Resolved::Release {
        tag: release.tag_name.clone(),
        version,
        asset,
        checksum,
    })
}

/// Download, verify, extract and place the executable into `staging/bin`.
#[allow(clippy::too_many_arguments)]
pub async fn fetch(
    ctx: &JobContext<'_>,
    manifest: &GameManifest,
    spec: &GithubReleaseSpec,
    tag: &str,
    version: &str,
    asset: &ReleaseAsset,
    checksum: &ChecksumSource,
    staging: &Path,
) -> Result<super::Fetched, Error> {
    let env = &ctx.env;
    ctx.check_cancel()?;
    ctx.progress
        .phase(Phase::Resolving, format!("{} {tag}", spec.repository));
    ctx.log.line(&format!(
        "release {tag} asset {} ({} bytes)",
        asset.name, asset.size
    ));

    // Download
    let download_dir = env.paths.downloads_dir().join(manifest.id.as_str());
    std::fs::create_dir_all(&download_dir)
        .map_err(|e| InstallError::io("create", &download_dir, e))?;
    let artifact = download_dir.join(&asset.name);
    ctx.progress.phase(Phase::Downloading, asset.name.clone());
    ctx.log.line(&format!("GET {}", asset.browser_download_url));
    let progress = ctx.progress.clone();
    let downloaded = env
        .http
        .download(
            &asset.browser_download_url,
            &artifact,
            |done, total| progress.bytes(done, total),
            &ctx.cancel,
        )
        .await?;
    ctx.log.line(&format!(
        "downloaded {} bytes sha256={}",
        downloaded.bytes, downloaded.sha256
    ));

    // Verify
    ctx.progress
        .phase(Phase::Verifying, checksum.policy().label());
    let expected = match checksum {
        ChecksumSource::Manifest(hex) | ChecksumSource::ApiDigest(hex) => Some(hex.clone()),
        ChecksumSource::ReleaseFile { name, url } => {
            let text = env.http.get_text(url).await?;
            match parse_checksum_file(&text, &asset.name) {
                Some(hex) => Some(hex),
                None => {
                    ctx.log.line(&format!(
                        "checksum file {name} has no entry for {}",
                        asset.name
                    ));
                    if env.config.install.require_checksum {
                        let _ = std::fs::remove_file(&artifact);
                        return Err(InstallError::ChecksumUnavailable {
                            asset: asset.name.clone(),
                        }
                        .into());
                    }
                    None
                }
            }
        }
        ChecksumSource::None => None,
    };
    let checksum_verified = match expected {
        Some(expected) => {
            if expected.to_ascii_lowercase() != downloaded.sha256 {
                let _ = std::fs::remove_file(&artifact);
                ctx.log.line("CHECKSUM MISMATCH — artifact deleted");
                return Err(SecurityError::ChecksumMismatch {
                    asset: asset.name.clone(),
                    expected,
                    actual: downloaded.sha256,
                }
                .into());
            }
            ctx.log.line("checksum OK");
            true
        }
        None => {
            ctx.log
                .line("no checksum available; relying on HTTPS transport integrity");
            false
        }
    };

    // Extract
    ctx.check_cancel()?;
    ctx.progress.phase(Phase::Extracting, asset.name.clone());
    let extract_dir = staging.join("extract");
    let report = archive::extract(&artifact, &extract_dir)?;
    ctx.log.line(&format!(
        "extracted {} file(s), {} bytes, {} link(s) skipped",
        report.files, report.bytes, report.skipped_links
    ));

    // Locate and install the executable
    let binary_rel = match &spec.binary {
        Some(b) => safe_relative(b, 4)?,
        None => Path::new(&manifest.run.executable).to_path_buf(),
    };
    let found = archive::discover_binary(&extract_dir, &binary_rel, &env.platform)?;
    let bin_dir = staging.join("bin");
    std::fs::create_dir_all(&bin_dir).map_err(|e| InstallError::io("create", &bin_dir, e))?;
    let exe_name = env.platform.exe_name(&manifest.run.executable);
    let dest = bin_dir.join(&exe_name);
    std::fs::copy(&found, &dest).map_err(|e| InstallError::io("copy", &dest, e))?;
    archive::make_executable(&dest)?;
    ctx.log.line(&format!(
        "installed {} -> {}",
        found.display(),
        dest.display()
    ));

    if let Err(e) = remove_dir_all_retry(&extract_dir) {
        tracing::warn!("could not clean extraction directory: {e}");
    }
    if !env.config.install.keep_downloads {
        let _ = std::fs::remove_file(&artifact);
    }

    Ok(super::Fetched {
        executable: dest,
        version: version.to_string(),
        source: InstallSource::GithubRelease {
            repository: spec.repository.clone(),
            tag: tag.to_string(),
            asset: asset.name.clone(),
            sha256: Some(downloaded.sha256),
            checksum_source: checksum_verified.then(|| checksum.policy().label()),
        },
        checksum_verified,
    })
}
