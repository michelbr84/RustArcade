//! Minimal GitHub REST client for release metadata, with an ETag cache.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::{Error, InstallError, NetworkError};
use crate::paths::{read_json, write_json_atomic};

use super::http::HttpClient;

/// One downloadable file attached to a release.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReleaseAsset {
    pub name: String,
    #[serde(default)]
    pub size: u64,
    pub browser_download_url: String,
    /// `sha256:<hex>` provided by GitHub for assets uploaded since 2025.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub digest: Option<String>,
}

impl ReleaseAsset {
    /// SHA-256 hex digest from GitHub's `digest` field, if present and well-formed.
    pub fn sha256_digest(&self) -> Option<String> {
        let d = self.digest.as_ref()?;
        let hex = d.strip_prefix("sha256:")?;
        (hex.len() == 64 && hex.bytes().all(|b| b.is_ascii_hexdigit()))
            .then(|| hex.to_ascii_lowercase())
    }
}

/// A GitHub release.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Release {
    pub tag_name: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub prerelease: bool,
    #[serde(default)]
    pub draft: bool,
    #[serde(default)]
    pub published_at: Option<String>,
    #[serde(default)]
    pub assets: Vec<ReleaseAsset>,
}

impl Release {
    /// Version derived from the tag (`v1.2.3` → `1.2.3`).
    pub fn version(&self) -> String {
        crate::version::display(&self.tag_name)
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct CachedResponse {
    etag: Option<String>,
    body: serde_json::Value,
}

/// GitHub API client.
#[derive(Debug, Clone)]
pub struct GitHubClient {
    http: HttpClient,
    api_base: String,
    cache_dir: Option<PathBuf>,
}

impl GitHubClient {
    pub fn new(http: HttpClient, api_base: &str, cache_dir: Option<PathBuf>) -> GitHubClient {
        GitHubClient {
            http,
            api_base: api_base.trim_end_matches('/').to_string(),
            cache_dir,
        }
    }

    fn cache_path(&self, key: &str) -> Option<PathBuf> {
        let dir = self.cache_dir.as_ref()?;
        let safe: String = key
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '-' || c == '.' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        Some(dir.join(format!("{safe}.json")))
    }

    async fn get_cached<T: serde::de::DeserializeOwned + Serialize>(
        &self,
        path: &str,
    ) -> Result<T, NetworkError> {
        let url = format!("{}{}", self.api_base, path);
        let cache_file = self.cache_path(path.trim_start_matches('/'));
        let cached: Option<CachedResponse> = cache_file
            .as_deref()
            .and_then(|p| read_json::<CachedResponse>(p).ok().flatten());
        let etag = cached.as_ref().and_then(|c| c.etag.clone());
        let response = self
            .http
            .get_github_json::<serde_json::Value>(&url, etag.as_deref())
            .await;
        match response {
            Ok(r) if r.status == 304 => {
                let body = cached
                    .map(|c| c.body)
                    .ok_or_else(|| NetworkError::InvalidBody {
                        url: url.clone(),
                        reason: "304 Not Modified without a cached copy".into(),
                    })?;
                serde_json::from_value(body).map_err(|e| NetworkError::InvalidBody {
                    url,
                    reason: e.to_string(),
                })
            }
            Ok(r) => {
                let body = r.body.ok_or_else(|| NetworkError::InvalidBody {
                    url: url.clone(),
                    reason: "empty body".into(),
                })?;
                if let Some(p) = &cache_file {
                    let _ = write_json_atomic(
                        p,
                        &CachedResponse {
                            etag: r.etag.clone(),
                            body: body.clone(),
                        },
                    );
                }
                serde_json::from_value(body).map_err(|e| NetworkError::InvalidBody {
                    url,
                    reason: e.to_string(),
                })
            }
            Err(NetworkError::RateLimited { reset_at }) => {
                // Serve stale data rather than failing when the limit is hit.
                if let Some(c) = cached
                    && let Ok(v) = serde_json::from_value::<T>(c.body)
                {
                    tracing::warn!("GitHub rate limit hit; using cached response for {path}");
                    return Ok(v);
                }
                Err(NetworkError::RateLimited { reset_at })
            }
            Err(e) => Err(e),
        }
    }

    /// The latest non-draft release. With `allow_prerelease`, pre-releases qualify too.
    pub async fn latest_release(
        &self,
        repo: &str,
        allow_prerelease: bool,
    ) -> Result<Release, Error> {
        if !allow_prerelease {
            match self
                .get_cached::<Release>(&format!("/repos/{repo}/releases/latest"))
                .await
            {
                Ok(r) => return Ok(r),
                Err(NetworkError::NotFound { .. }) => {}
                Err(e) => return Err(e.into()),
            }
        }
        let list = match self
            .get_cached::<Vec<Release>>(&format!("/repos/{repo}/releases?per_page=30"))
            .await
        {
            Ok(list) => list,
            Err(NetworkError::NotFound { .. }) => {
                return Err(InstallError::ReleaseNotFound {
                    repository: repo.to_string(),
                    detail: "the repository does not exist or is private".into(),
                }
                .into());
            }
            Err(e) => return Err(e.into()),
        };
        list.into_iter()
            .find(|r| !r.draft && (allow_prerelease || !r.prerelease))
            .ok_or_else(|| {
                InstallError::ReleaseNotFound {
                    repository: repo.to_string(),
                    detail: if allow_prerelease {
                        "the repository has no releases".into()
                    } else {
                        "the repository has no stable releases".into()
                    },
                }
                .into()
            })
    }

    /// A specific release by tag.
    pub async fn release_by_tag(&self, repo: &str, tag: &str) -> Result<Release, Error> {
        self.get_cached::<Release>(&format!("/repos/{repo}/releases/tags/{tag}"))
            .await
            .map_err(|e| match e {
                NetworkError::NotFound { .. } => InstallError::ReleaseNotFound {
                    repository: repo.to_string(),
                    detail: format!("release tag `{tag}` does not exist"),
                }
                .into(),
                other => Error::Network(other),
            })
    }

    /// Latest release, or a pinned tag when `tag` is given.
    pub async fn resolve(
        &self,
        repo: &str,
        tag: Option<&str>,
        allow_prerelease: bool,
    ) -> Result<Release, Error> {
        match tag {
            Some(t) => self.release_by_tag(repo, t).await,
            None => self.latest_release(repo, allow_prerelease).await,
        }
    }

    pub fn api_base(&self) -> &str {
        &self.api_base
    }

    /// Remove cached API responses (used by `catalog update --force` style refreshes).
    pub fn clear_cache(&self) {
        if let Some(dir) = &self.cache_dir
            && let Ok(entries) = std::fs::read_dir(dir)
        {
            for e in entries.flatten() {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::NetworkConfig;

    fn release_json(tag: &str, prerelease: bool) -> serde_json::Value {
        serde_json::json!({
            "tag_name": tag, "prerelease": prerelease, "draft": false,
            "assets": [{"name": format!("game-{tag}-x86_64-unknown-linux-gnu.tar.gz"), "size": 10,
                        "browser_download_url": "https://example.invalid/a.tar.gz",
                        "digest": "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}]
        })
    }

    #[tokio::test]
    async fn latest_release_falls_back_to_list_and_caches_etag() {
        let server = httpmock::MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method("GET").path("/repos/o/r/releases/latest");
                then.status(404).body("{}");
            })
            .await;
        let list_hits = server
            .mock_async(|when, then| {
                when.method("GET").path("/repos/o/r/releases");
                then.status(200)
                    .header("etag", "\"list\"")
                    .json_body(serde_json::json!([
                        release_json("v2.0.0-rc.1", true),
                        release_json("v1.5.0", false)
                    ]));
            })
            .await;
        let dir = tempfile::tempdir().unwrap();
        let http = HttpClient::new(&NetworkConfig::default(), true).unwrap();
        let gh = GitHubClient::new(http, &server.base_url(), Some(dir.path().to_path_buf()));
        let stable = gh.latest_release("o/r", false).await.unwrap();
        assert_eq!(stable.tag_name, "v1.5.0");
        assert_eq!(stable.version(), "1.5.0");
        assert_eq!(stable.assets[0].sha256_digest().unwrap(), "a".repeat(64));
        let pre = gh.latest_release("o/r", true).await.unwrap();
        assert_eq!(pre.tag_name, "v2.0.0-rc.1");
        assert!(list_hits.calls_async().await >= 2);
        // Cache file written
        assert!(std::fs::read_dir(dir.path()).unwrap().count() >= 1);
    }

    #[tokio::test]
    async fn not_modified_uses_cache_and_rate_limit_serves_stale() {
        let server = httpmock::MockServer::start_async().await;
        let dir = tempfile::tempdir().unwrap();
        let http = HttpClient::new(&NetworkConfig::default(), true).unwrap();
        let gh = GitHubClient::new(http, &server.base_url(), Some(dir.path().to_path_buf()));
        let first = server
            .mock_async(|when, then| {
                when.method("GET").path("/repos/o/r/releases/tags/v1");
                then.status(200)
                    .header("etag", "\"e1\"")
                    .json_body(release_json("v1", false));
            })
            .await;
        assert_eq!(gh.release_by_tag("o/r", "v1").await.unwrap().tag_name, "v1");
        first.delete_async().await;
        server
            .mock_async(|when, then| {
                when.method("GET")
                    .path("/repos/o/r/releases/tags/v1")
                    .header("if-none-match", "\"e1\"");
                then.status(304);
            })
            .await;
        assert_eq!(gh.release_by_tag("o/r", "v1").await.unwrap().tag_name, "v1");
        server.reset_async().await;
        server
            .mock_async(|when, then| {
                when.method("GET").path("/repos/o/r/releases/tags/v1");
                then.status(403)
                    .header("x-ratelimit-remaining", "0")
                    .body("{}");
            })
            .await;
        assert_eq!(gh.release_by_tag("o/r", "v1").await.unwrap().tag_name, "v1");
        gh.clear_cache();
        let err = gh.release_by_tag("o/r", "v1").await.unwrap_err();
        assert!(err.to_string().contains("rate limit"), "{err}");
    }

    #[tokio::test]
    async fn missing_tag_is_release_not_found() {
        let server = httpmock::MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method("GET").path("/repos/o/r/releases/tags/v9");
                then.status(404).body("{}");
            })
            .await;
        let http = HttpClient::new(&NetworkConfig::default(), true).unwrap();
        let gh = GitHubClient::new(http, &server.base_url(), None);
        assert!(matches!(
            gh.release_by_tag("o/r", "v9").await,
            Err(Error::Install(InstallError::ReleaseNotFound { .. }))
        ));
    }
}
