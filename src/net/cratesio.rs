//! crates.io API client (latest version lookups).

use serde::Deserialize;

use crate::error::NetworkError;

use super::http::HttpClient;

#[derive(Debug, Deserialize)]
struct CrateResponse {
    #[serde(rename = "crate")]
    krate: CrateInfo,
}

#[derive(Debug, Deserialize)]
struct CrateInfo {
    #[serde(default)]
    max_stable_version: Option<String>,
    #[serde(default)]
    max_version: Option<String>,
    #[serde(default)]
    newest_version: Option<String>,
}

/// Latest published version of a crate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CrateVersions {
    pub stable: Option<String>,
    pub newest: Option<String>,
}

#[derive(Debug, Clone)]
pub struct CratesIoClient {
    http: HttpClient,
    base: String,
}

impl CratesIoClient {
    pub fn new(http: HttpClient, base: &str) -> CratesIoClient {
        CratesIoClient {
            http,
            base: base.trim_end_matches('/').to_string(),
        }
    }

    pub async fn versions(&self, krate: &str) -> Result<CrateVersions, NetworkError> {
        let url = format!("{}/api/v1/crates/{krate}", self.base);
        let response: CrateResponse = self.http.get_json(&url).await?;
        Ok(CrateVersions {
            stable: response.krate.max_stable_version.clone(),
            newest: response
                .krate
                .newest_version
                .or(response.krate.max_version)
                .or(response.krate.max_stable_version),
        })
    }

    /// Latest stable version (falls back to the newest one when nothing stable exists).
    pub async fn latest_version(&self, krate: &str) -> Result<Option<String>, NetworkError> {
        let v = self.versions(krate).await?;
        Ok(v.stable.or(v.newest))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::NetworkConfig;

    #[tokio::test]
    async fn reads_max_stable_version() {
        let server = httpmock::MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method("GET").path("/api/v1/crates/demo");
                then.status(200).json_body(serde_json::json!({
                    "crate": {"max_stable_version": "1.2.3", "max_version": "2.0.0-beta", "newest_version": "2.0.0-beta"}
                }));
            })
            .await;
        server
            .mock_async(|when, then| {
                when.method("GET").path("/api/v1/crates/nope");
                then.status(404).body("{}");
            })
            .await;
        let http = HttpClient::new(&NetworkConfig::default(), true).unwrap();
        let c = CratesIoClient::new(http, &server.base_url());
        assert_eq!(
            c.latest_version("demo").await.unwrap().as_deref(),
            Some("1.2.3")
        );
        assert!(matches!(
            c.latest_version("nope").await,
            Err(NetworkError::NotFound { .. })
        ));
    }
}
