//! Remote catalog synchronisation: download `index.json` + manifests over HTTPS,
//! verify every digest, validate everything, then atomically replace the cache.

use std::fs;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::error::CatalogError;
use crate::net::HttpClient;
use crate::paths::{nonce, read_json, remove_dir_all_retry, rename_with_retry, write_json_atomic};

use super::index::{CatalogIndex, GAMES_DIR, INDEX_FILE, INDEX_SCHEMA_VERSION, sha256_hex};
use super::manifest::id_problem;
use super::{ValidationMode, load_sources};

/// Metadata about the cached remote catalog.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogMeta {
    pub source_url: String,
    pub updated_at: DateTime<Utc>,
    pub fetched_at: DateTime<Utc>,
    pub games: usize,
}

/// What a refresh changed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CatalogRefreshReport {
    pub fetched: usize,
    pub added: Vec<String>,
    pub updated: Vec<String>,
    pub removed: Vec<String>,
}

impl CatalogRefreshReport {
    pub fn unchanged(&self) -> bool {
        self.added.is_empty() && self.updated.is_empty() && self.removed.is_empty()
    }
}

/// Read the cached catalog metadata, if any.
pub fn read_meta(meta_file: &Path) -> Option<CatalogMeta> {
    read_json::<CatalogMeta>(meta_file).ok().flatten()
}

/// Is the cache older than `max_age_hours`?
pub fn is_stale(meta: Option<&CatalogMeta>, max_age_hours: u64) -> bool {
    match meta {
        None => true,
        Some(m) => {
            (Utc::now() - m.fetched_at).num_seconds() > (max_age_hours.saturating_mul(3600)) as i64
        }
    }
}

fn index_error(url: &str, reason: impl Into<String>) -> CatalogError {
    CatalogError::Index {
        url: url.to_string(),
        reason: reason.into(),
    }
}

/// Download and install the remote catalog into `cache_dir`.
pub async fn fetch_remote(
    http: &HttpClient,
    base_url: &str,
    cache_dir: &Path,
    meta_file: &Path,
) -> Result<CatalogRefreshReport, CatalogError> {
    let base = base_url.trim_end_matches('/');
    let index_url = format!("{base}/{INDEX_FILE}");
    http.check_url(&index_url)
        .map_err(|e| index_error(&index_url, e.to_string()))?;
    let text = http
        .get_text(&index_url)
        .await
        .map_err(|e| index_error(&index_url, e.to_string()))?;
    let index: CatalogIndex =
        serde_json::from_str(&text).map_err(|e| index_error(&index_url, e.to_string()))?;
    if index.schema_version != INDEX_SCHEMA_VERSION {
        return Err(CatalogError::RemoteRejected {
            reason: format!(
                "index schema version {} is not supported",
                index.schema_version
            ),
        });
    }
    if index.games.len() > 500 {
        return Err(CatalogError::RemoteRejected {
            reason: "index lists an unreasonable number of games".into(),
        });
    }
    let previous_meta = read_meta(meta_file);
    if let Some(prev) = &previous_meta
        && index.updated_at < prev.updated_at
    {
        return Err(CatalogError::RemoteRejected {
            reason: format!(
                "remote index ({}) is older than the cached one ({})",
                index.updated_at.format("%Y-%m-%d %H:%M"),
                prev.updated_at.format("%Y-%m-%d %H:%M")
            ),
        });
    }

    // Download every manifest and verify its digest.
    let mut sources = Vec::with_capacity(index.games.len());
    for entry in &index.games {
        let stem = entry.file.strip_suffix(".toml").unwrap_or("");
        if entry.file.contains(['/', '\\']) || stem != entry.id || id_problem(stem).is_some() {
            return Err(CatalogError::RemoteRejected {
                reason: format!("index entry `{}` has an invalid file name", entry.file),
            });
        }
        let url = format!("{base}/{GAMES_DIR}/{}", entry.file);
        let bytes = http
            .get_bytes(&url, 256 * 1024)
            .await
            .map_err(|e| index_error(&url, e.to_string()))?;
        let digest = sha256_hex(&bytes);
        if digest != entry.sha256.to_ascii_lowercase() {
            return Err(CatalogError::RemoteRejected {
                reason: format!(
                    "digest mismatch for {} (expected {}, got {digest})",
                    entry.file, entry.sha256
                ),
            });
        }
        let text = String::from_utf8(bytes).map_err(|_| CatalogError::RemoteRejected {
            reason: format!("{} is not UTF-8", entry.file),
        })?;
        sources.push((PathBuf::from(&entry.file), text));
    }

    // Validate the complete set before accepting anything.
    let report = load_sources(sources.clone(), ValidationMode::Strict);
    if let Some(first) = report.errors.first() {
        return Err(CatalogError::RemoteRejected {
            reason: format!("remote manifests failed validation: {first}"),
        });
    }

    // Compute the diff against the current cache.
    let old_digests = current_digests(cache_dir);
    let mut result = CatalogRefreshReport {
        fetched: sources.len(),
        ..Default::default()
    };
    for entry in &index.games {
        match old_digests.get(&entry.file) {
            None => result.added.push(entry.id.clone()),
            Some(old) if old != &entry.sha256.to_ascii_lowercase() => {
                result.updated.push(entry.id.clone())
            }
            _ => {}
        }
    }
    for old in old_digests.keys() {
        if !index.games.iter().any(|e| &e.file == old) {
            result
                .removed
                .push(old.trim_end_matches(".toml").to_string());
        }
    }

    // Write to a temporary directory, then swap it in.
    let parent = cache_dir.parent().unwrap_or(cache_dir);
    let tmp = parent.join(format!(".catalog-tmp-{}", nonce()));
    let tmp_games = tmp.join(GAMES_DIR);
    fs::create_dir_all(&tmp_games).map_err(|e| CatalogError::Io {
        path: tmp_games.clone(),
        source: e,
    })?;
    for (file, text) in &sources {
        let path = tmp_games.join(file);
        fs::write(&path, text).map_err(|e| CatalogError::Io { path, source: e })?;
    }
    fs::write(tmp.join(INDEX_FILE), text.as_bytes()).map_err(|e| CatalogError::Io {
        path: tmp.join(INDEX_FILE),
        source: e,
    })?;
    let old = parent.join(format!(".catalog-old-{}", nonce()));
    if cache_dir.exists() {
        rename_with_retry(cache_dir, &old).map_err(|e| CatalogError::Io {
            path: cache_dir.to_path_buf(),
            source: e,
        })?;
    }
    if let Err(e) = rename_with_retry(&tmp, cache_dir) {
        let _ = rename_with_retry(&old, cache_dir);
        return Err(CatalogError::Io {
            path: cache_dir.to_path_buf(),
            source: e,
        });
    }
    let _ = remove_dir_all_retry(&old);
    write_json_atomic(
        meta_file,
        &CatalogMeta {
            source_url: base.to_string(),
            updated_at: index.updated_at,
            fetched_at: Utc::now(),
            games: index.games.len(),
        },
    )
    .map_err(|e| CatalogError::Io {
        path: meta_file.to_path_buf(),
        source: std::io::Error::other(e.to_string()),
    })?;
    Ok(result)
}

fn current_digests(cache_dir: &Path) -> std::collections::BTreeMap<String, String> {
    let mut out = std::collections::BTreeMap::new();
    if let Ok(entries) = fs::read_dir(cache_dir.join(GAMES_DIR)) {
        for e in entries.flatten() {
            let path = e.path();
            if path.extension().and_then(|x| x.to_str()) == Some("toml")
                && let Ok(bytes) = fs::read(&path)
            {
                out.insert(
                    e.file_name().to_string_lossy().to_string(),
                    sha256_hex(&bytes),
                );
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::manifest::tests::FULL;
    use crate::config::NetworkConfig;

    fn index_json(files: &[(&str, &str)], updated_at: &str) -> String {
        let games: Vec<serde_json::Value> = files
            .iter()
            .map(|(file, text)| {
                serde_json::json!({
                    "file": file, "id": file.trim_end_matches(".toml"),
                    "sha256": sha256_hex(text.as_bytes()), "size": text.len()
                })
            })
            .collect();
        serde_json::json!({"schema_version": 1, "updated_at": updated_at, "games": games})
            .to_string()
    }

    #[tokio::test]
    async fn fetches_verifies_and_swaps_then_rejects_rollback() {
        let server = httpmock::MockServer::start_async().await;
        let other = FULL
            .replace("id = \"example-game\"", "id = \"other-game\"")
            .replace(
                "executable = \"example-game\"",
                "executable = \"other-game\"",
            );
        server
            .mock_async(|when, then| {
                when.method("GET").path("/catalog/index.json");
                then.status(200).body(index_json(
                    &[("example-game.toml", FULL), ("other-game.toml", &other)],
                    "2026-08-27T10:00:00Z",
                ));
            })
            .await;
        server
            .mock_async(|when, then| {
                when.method("GET").path("/catalog/games/example-game.toml");
                then.status(200).body(FULL);
            })
            .await;
        server
            .mock_async(|when, then| {
                when.method("GET").path("/catalog/games/other-game.toml");
                then.status(200).body(other.clone());
            })
            .await;
        let dir = tempfile::tempdir().unwrap();
        let cache = dir.path().join("data/catalog");
        let meta = dir.path().join("cache/catalog-meta.json");
        let http = HttpClient::new(&NetworkConfig::default(), true).unwrap();
        let base = server.url("/catalog");
        let report = fetch_remote(&http, &base, &cache, &meta).await.unwrap();
        assert_eq!(report.fetched, 2);
        assert_eq!(report.added.len(), 2);
        assert!(cache.join("games/example-game.toml").is_file());
        assert!(cache.join("index.json").is_file());
        let m = read_meta(&meta).unwrap();
        assert_eq!(m.games, 2);
        assert!(!is_stale(Some(&m), 24));
        assert!(is_stale(None, 24));

        // Second fetch: unchanged.
        let report = fetch_remote(&http, &base, &cache, &meta).await.unwrap();
        assert!(report.unchanged());

        // Older index is rejected, cache untouched.
        server.reset_async().await;
        server
            .mock_async(|when, then| {
                when.method("GET").path("/catalog/index.json");
                then.status(200).body(index_json(
                    &[("example-game.toml", FULL)],
                    "2020-01-01T00:00:00Z",
                ));
            })
            .await;
        let err = fetch_remote(&http, &base, &cache, &meta).await.unwrap_err();
        assert!(matches!(err, CatalogError::RemoteRejected { .. }), "{err}");
        assert!(cache.join("games/other-game.toml").is_file());
    }

    #[tokio::test]
    async fn digest_mismatch_and_invalid_manifest_are_rejected() {
        let server = httpmock::MockServer::start_async().await;
        let dir = tempfile::tempdir().unwrap();
        let cache = dir.path().join("catalog");
        let meta = dir.path().join("meta.json");
        let http = HttpClient::new(&NetworkConfig::default(), true).unwrap();
        // digest mismatch
        server
            .mock_async(|when, then| {
                when.method("GET").path("/c/index.json");
                then.status(200).body(index_json(
                    &[("example-game.toml", "different")],
                    "2026-08-27T10:00:00Z",
                ));
            })
            .await;
        server
            .mock_async(|when, then| {
                when.method("GET").path("/c/games/example-game.toml");
                then.status(200).body(FULL);
            })
            .await;
        let err = fetch_remote(&http, &server.url("/c"), &cache, &meta)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("digest mismatch"), "{err}");
        assert!(!cache.exists());
        // invalid manifest content (schema 9) with a correct digest
        server.reset_async().await;
        let bad = FULL.replace("schema_version = 1", "schema_version = 9");
        let bad2 = bad.clone();
        server
            .mock_async(move |when, then| {
                when.method("GET").path("/c/index.json");
                then.status(200).body(index_json(
                    &[("example-game.toml", &bad2)],
                    "2026-08-27T10:00:00Z",
                ));
            })
            .await;
        server
            .mock_async(move |when, then| {
                when.method("GET").path("/c/games/example-game.toml");
                then.status(200).body(bad.clone());
            })
            .await;
        let err = fetch_remote(&http, &server.url("/c"), &cache, &meta)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("failed validation"), "{err}");
        assert!(!cache.exists());
        // bad file name in index
        server.reset_async().await;
        server
            .mock_async(|when, then| {
                when.method("GET").path("/c/index.json");
                then.status(200).body(index_json(
                    &[("../evil.toml", FULL)],
                    "2026-08-27T10:00:00Z",
                ));
            })
            .await;
        let err = fetch_remote(&http, &server.url("/c"), &cache, &meta)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("invalid file name"), "{err}");
    }
}
