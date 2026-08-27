//! HTTPS-only HTTP client with streaming, size-capped, checksummed downloads.

use std::path::Path;
use std::sync::Once;
use std::time::Duration;

use futures_util::StreamExt;
use reqwest::header::{ACCEPT, AUTHORIZATION, HeaderMap, HeaderValue, USER_AGENT};
use reqwest::redirect::{Attempt, Policy};
use serde::de::DeserializeOwned;
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;
use tokio_util::sync::CancellationToken;
use url::Url;

use crate::config::NetworkConfig;
use crate::error::{InstallError, NetworkError, SecurityError};

static PROVIDER: Once = Once::new();

/// Install the `ring` TLS provider exactly once.
fn install_crypto_provider() {
    PROVIDER.call_once(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

/// Is `url` a loopback HTTP URL that tests are allowed to use?
pub fn is_local_loopback(url: &Url) -> bool {
    url.scheme() == "http"
        && matches!(
            url.host_str(),
            Some("127.0.0.1") | Some("localhost") | Some("[::1]")
        )
}

/// Result of a successful download.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadResult {
    pub bytes: u64,
    pub sha256: String,
}

/// A JSON response with the headers RustArcade cares about.
#[derive(Debug)]
pub struct JsonResponse<T> {
    pub status: u16,
    pub body: Option<T>,
    pub etag: Option<String>,
    pub rate_limit_remaining: Option<u64>,
    pub rate_limit_reset: Option<chrono::DateTime<chrono::Utc>>,
}

/// Shared HTTP client.
#[derive(Debug, Clone)]
pub struct HttpClient {
    client: reqwest::Client,
    allow_insecure_local: bool,
    max_download: u64,
    github_token: Option<String>,
}

impl HttpClient {
    pub fn new(
        config: &NetworkConfig,
        allow_insecure_local: bool,
    ) -> Result<HttpClient, NetworkError> {
        install_crypto_provider();
        let policy = Policy::custom(move |attempt: Attempt| {
            if attempt.previous().len() >= 10 {
                return attempt.error("too many redirects");
            }
            let next = attempt.url();
            if next.scheme() == "https" || (allow_insecure_local && is_local_loopback(next)) {
                attempt.follow()
            } else {
                attempt.error("refusing to follow a redirect to a non-HTTPS URL")
            }
        });
        let mut headers = HeaderMap::new();
        headers.insert(USER_AGENT, HeaderValue::from_static(crate::USER_AGENT));
        let client = reqwest::Client::builder()
            .default_headers(headers)
            .redirect(policy)
            .connect_timeout(Duration::from_secs(config.connect_timeout_secs))
            .read_timeout(Duration::from_secs(config.read_timeout_secs))
            .https_only(!allow_insecure_local)
            .build()
            .map_err(|e| NetworkError::Transport {
                url: "<client>".into(),
                source: e,
            })?;
        let github_token = std::env::var("GITHUB_TOKEN")
            .ok()
            .or_else(|| std::env::var("GH_TOKEN").ok())
            .filter(|t| !t.trim().is_empty());
        Ok(HttpClient {
            client,
            allow_insecure_local,
            max_download: config.max_download_mb.saturating_mul(1024 * 1024),
            github_token,
        })
    }

    pub fn allows_insecure_local(&self) -> bool {
        self.allow_insecure_local
    }

    /// Validate that `url` is HTTPS (or loopback HTTP when explicitly allowed).
    pub fn check_url(&self, raw: &str) -> Result<Url, SecurityError> {
        let url = Url::parse(raw).map_err(|_| SecurityError::InsecureUrl {
            url: raw.to_string(),
        })?;
        if url.scheme() == "https" || (self.allow_insecure_local && is_local_loopback(&url)) {
            Ok(url)
        } else {
            Err(SecurityError::InsecureUrl {
                url: raw.to_string(),
            })
        }
    }

    fn github_headers(&self, etag: Option<&str>) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(
            ACCEPT,
            HeaderValue::from_static("application/vnd.github+json"),
        );
        headers.insert(
            "X-GitHub-Api-Version",
            HeaderValue::from_static("2022-11-28"),
        );
        if let Some(token) = &self.github_token
            && let Ok(v) = HeaderValue::from_str(&format!("Bearer {token}"))
        {
            headers.insert(AUTHORIZATION, v);
        }
        if let Some(etag) = etag
            && let Ok(v) = HeaderValue::from_str(etag)
        {
            headers.insert("If-None-Match", v);
        }
        headers
    }

    /// GET a JSON document from the GitHub API (adds auth/accept headers; maps rate limits).
    pub async fn get_github_json<T: DeserializeOwned>(
        &self,
        url: &str,
        etag: Option<&str>,
    ) -> Result<JsonResponse<T>, NetworkError> {
        let parsed = self.check_url(url).map_err(|_| NetworkError::InvalidBody {
            url: url.to_string(),
            reason: "insecure URL".into(),
        })?;
        let response = self
            .client
            .get(parsed)
            .headers(self.github_headers(etag))
            .send()
            .await
            .map_err(|e| map_transport(url, e))?;
        let status = response.status().as_u16();
        let etag = header_string(response.headers(), "etag");
        let remaining =
            header_string(response.headers(), "x-ratelimit-remaining").and_then(|v| v.parse().ok());
        let reset = header_string(response.headers(), "x-ratelimit-reset")
            .and_then(|v| v.parse::<i64>().ok())
            .and_then(|secs| chrono::DateTime::from_timestamp(secs, 0));
        if status == 304 {
            return Ok(JsonResponse {
                status,
                body: None,
                etag,
                rate_limit_remaining: remaining,
                rate_limit_reset: reset,
            });
        }
        if (status == 403 || status == 429) && remaining == Some(0) {
            return Err(NetworkError::RateLimited { reset_at: reset });
        }
        if status == 404 {
            return Err(NetworkError::NotFound {
                url: url.to_string(),
            });
        }
        if !(200..300).contains(&status) {
            return Err(NetworkError::Status {
                url: url.to_string(),
                status,
            });
        }
        let body = response
            .json::<T>()
            .await
            .map_err(|e| NetworkError::InvalidBody {
                url: url.to_string(),
                reason: e.to_string(),
            })?;
        Ok(JsonResponse {
            status,
            body: Some(body),
            etag,
            rate_limit_remaining: remaining,
            rate_limit_reset: reset,
        })
    }

    /// GET a JSON document from any HTTPS endpoint.
    pub async fn get_json<T: DeserializeOwned>(&self, url: &str) -> Result<T, NetworkError> {
        let parsed = self.check_url(url).map_err(|_| NetworkError::InvalidBody {
            url: url.to_string(),
            reason: "insecure URL".into(),
        })?;
        let response = self
            .client
            .get(parsed)
            .header(ACCEPT, "application/json")
            .send()
            .await
            .map_err(|e| map_transport(url, e))?;
        let status = response.status().as_u16();
        if status == 404 {
            return Err(NetworkError::NotFound {
                url: url.to_string(),
            });
        }
        if !(200..300).contains(&status) {
            return Err(NetworkError::Status {
                url: url.to_string(),
                status,
            });
        }
        response
            .json::<T>()
            .await
            .map_err(|e| NetworkError::InvalidBody {
                url: url.to_string(),
                reason: e.to_string(),
            })
    }

    /// GET a small text document (checksum files, catalog manifests). Capped at 1 MiB.
    pub async fn get_text(&self, url: &str) -> Result<String, NetworkError> {
        let bytes = self.get_bytes(url, 1024 * 1024).await?;
        String::from_utf8(bytes).map_err(|_| NetworkError::InvalidBody {
            url: url.to_string(),
            reason: "not valid UTF-8".into(),
        })
    }

    /// GET a document into memory, capped at `limit` bytes.
    pub async fn get_bytes(&self, url: &str, limit: usize) -> Result<Vec<u8>, NetworkError> {
        let parsed = self.check_url(url).map_err(|_| NetworkError::InvalidBody {
            url: url.to_string(),
            reason: "insecure URL".into(),
        })?;
        let response = self
            .client
            .get(parsed)
            .send()
            .await
            .map_err(|e| map_transport(url, e))?;
        let status = response.status().as_u16();
        if status == 404 {
            return Err(NetworkError::NotFound {
                url: url.to_string(),
            });
        }
        if !(200..300).contains(&status) {
            return Err(NetworkError::Status {
                url: url.to_string(),
                status,
            });
        }
        let mut out = Vec::new();
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| map_transport(url, e))?;
            if out.len() + chunk.len() > limit {
                return Err(NetworkError::InvalidBody {
                    url: url.to_string(),
                    reason: format!("response larger than {limit} bytes"),
                });
            }
            out.extend_from_slice(&chunk);
        }
        Ok(out)
    }

    /// Stream `url` to `dest`, hashing as it goes. Aborts (and deletes the partial file)
    /// on cancellation, size overflow, or transport errors.
    pub async fn download(
        &self,
        url: &str,
        dest: &Path,
        mut on_progress: impl FnMut(u64, Option<u64>),
        cancel: &CancellationToken,
    ) -> Result<DownloadResult, crate::Error> {
        let parsed = self.check_url(url)?;
        let response = self
            .client
            .get(parsed)
            .send()
            .await
            .map_err(|e| map_transport(url, e))?;
        let status = response.status().as_u16();
        if status == 404 {
            return Err(NetworkError::NotFound {
                url: url.to_string(),
            }
            .into());
        }
        if !(200..300).contains(&status) {
            return Err(NetworkError::Status {
                url: url.to_string(),
                status,
            }
            .into());
        }
        let total = response.content_length();
        if let Some(t) = total
            && t > self.max_download
        {
            return Err(SecurityError::DownloadTooLarge {
                limit_bytes: self.max_download,
            }
            .into());
        }
        if let Some(parent) = dest.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| InstallError::io("create", parent, e))?;
        }
        let mut file = tokio::fs::File::create(dest)
            .await
            .map_err(|e| InstallError::io("create", dest, e))?;
        let mut hasher = Sha256::new();
        let mut done: u64 = 0;
        let mut stream = response.bytes_stream();
        let result: Result<(), crate::Error> = async {
            loop {
                if cancel.is_cancelled() {
                    return Err(InstallError::Cancelled.into());
                }
                let next = tokio::select! {
                    biased;
                    _ = cancel.cancelled() => return Err(InstallError::Cancelled.into()),
                    chunk = stream.next() => chunk,
                };
                let Some(chunk) = next else { break };
                let chunk = chunk.map_err(|e| map_transport(url, e))?;
                done += chunk.len() as u64;
                if done > self.max_download {
                    return Err(SecurityError::DownloadTooLarge {
                        limit_bytes: self.max_download,
                    }
                    .into());
                }
                hasher.update(&chunk);
                file.write_all(&chunk)
                    .await
                    .map_err(|e| InstallError::io("write", dest, e))?;
                on_progress(done, total);
            }
            file.flush()
                .await
                .map_err(|e| InstallError::io("write", dest, e))?;
            Ok(())
        }
        .await;
        drop(file);
        if let Err(e) = result {
            let _ = tokio::fs::remove_file(dest).await;
            return Err(e);
        }
        Ok(DownloadResult {
            bytes: done,
            sha256: hex::encode(hasher.finalize()),
        })
    }
}

fn header_string(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
}

fn map_transport(url: &str, e: reqwest::Error) -> NetworkError {
    if e.is_timeout() {
        NetworkError::Timeout {
            url: url.to_string(),
        }
    } else {
        NetworkError::Transport {
            url: url.to_string(),
            source: e,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn client(allow_local: bool) -> HttpClient {
        HttpClient::new(&NetworkConfig::default(), allow_local).unwrap()
    }

    #[test]
    fn https_only_unless_local_allowed() {
        let strict = client(false);
        assert!(strict.check_url("https://github.com/x").is_ok());
        assert!(strict.check_url("http://github.com/x").is_err());
        assert!(strict.check_url("http://127.0.0.1:1/x").is_err());
        assert!(strict.check_url("ftp://x").is_err());
        assert!(strict.check_url("not a url").is_err());
        let local = client(true);
        assert!(local.check_url("http://127.0.0.1:1/x").is_ok());
        assert!(local.check_url("http://localhost:1/x").is_ok());
        assert!(local.check_url("http://example.com/x").is_err());
    }

    #[tokio::test]
    async fn download_hashes_and_caps_size() {
        let server = httpmock::MockServer::start_async().await;
        let body = b"hello rustarcade".to_vec();
        server
            .mock_async(|when, then| {
                when.method("GET").path("/file.bin");
                then.status(200).body(body.clone());
            })
            .await;
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("file.bin");
        let c = client(true);
        let mut seen = Vec::new();
        let res = c
            .download(
                &server.url("/file.bin"),
                &dest,
                |d, t| seen.push((d, t)),
                &CancellationToken::new(),
            )
            .await
            .unwrap();
        assert_eq!(res.bytes, body.len() as u64);
        assert_eq!(res.sha256, crate::catalog::index::sha256_hex(&body));
        assert_eq!(std::fs::read(&dest).unwrap(), body);
        assert!(!seen.is_empty());

        let small = HttpClient::new(
            &NetworkConfig {
                max_download_mb: 1,
                ..NetworkConfig::default()
            },
            true,
        )
        .unwrap();
        // 1 MiB limit: serve 2 MiB.
        let big = vec![0u8; 2 * 1024 * 1024];
        server
            .mock_async(|when, then| {
                when.method("GET").path("/big.bin");
                then.status(200).body(big.clone());
            })
            .await;
        let dest2 = dir.path().join("big.bin");
        let err = small
            .download(
                &server.url("/big.bin"),
                &dest2,
                |_, _| {},
                &CancellationToken::new(),
            )
            .await
            .unwrap_err();
        assert!(err.is_security(), "{err}");
        assert!(!dest2.exists(), "partial download must be removed");
    }

    #[tokio::test]
    async fn download_reports_404_and_cancellation() {
        let server = httpmock::MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method("GET").path("/missing");
                then.status(404);
            })
            .await;
        let dir = tempfile::tempdir().unwrap();
        let c = client(true);
        let err = c
            .download(
                &server.url("/missing"),
                &dir.path().join("m"),
                |_, _| {},
                &CancellationToken::new(),
            )
            .await
            .unwrap_err();
        assert!(matches!(
            err,
            crate::Error::Network(NetworkError::NotFound { .. })
        ));
        let token = CancellationToken::new();
        token.cancel();
        server
            .mock_async(|when, then| {
                when.method("GET").path("/slow");
                then.status(200).body(vec![1u8; 4096]);
            })
            .await;
        let err = c
            .download(
                &server.url("/slow"),
                &dir.path().join("s"),
                |_, _| {},
                &token,
            )
            .await
            .unwrap_err();
        assert!(matches!(
            err,
            crate::Error::Install(InstallError::Cancelled)
        ));
    }

    #[tokio::test]
    async fn github_json_maps_rate_limit_and_not_found() {
        let server = httpmock::MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method("GET").path("/limited");
                then.status(403)
                    .header("x-ratelimit-remaining", "0")
                    .header("x-ratelimit-reset", "1700000000")
                    .body("{}");
            })
            .await;
        server
            .mock_async(|when, then| {
                when.method("GET").path("/gone");
                then.status(404).body("{}");
            })
            .await;
        server
            .mock_async(|when, then| {
                when.method("GET")
                    .path("/ok")
                    .header("accept", "application/vnd.github+json");
                then.status(200)
                    .header("etag", "\"abc\"")
                    .body("{\"tag_name\":\"v1\"}");
            })
            .await;
        let c = client(true);
        let err = c
            .get_github_json::<serde_json::Value>(&server.url("/limited"), None)
            .await
            .unwrap_err();
        assert!(matches!(
            err,
            NetworkError::RateLimited { reset_at: Some(_) }
        ));
        let err = c
            .get_github_json::<serde_json::Value>(&server.url("/gone"), None)
            .await
            .unwrap_err();
        assert!(matches!(err, NetworkError::NotFound { .. }));
        let ok = c
            .get_github_json::<serde_json::Value>(&server.url("/ok"), None)
            .await
            .unwrap();
        assert_eq!(ok.etag.as_deref(), Some("\"abc\""));
        assert_eq!(ok.body.unwrap()["tag_name"], "v1");
    }
}
