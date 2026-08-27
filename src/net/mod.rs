//! Network layer: HTTPS-only HTTP client, GitHub and crates.io API clients.

pub mod cratesio;
pub mod github;
pub mod http;

pub use cratesio::CratesIoClient;
pub use github::{GitHubClient, Release, ReleaseAsset};
pub use http::{DownloadResult, HttpClient};

/// API base URLs (overridable for tests via environment variables).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoints {
    pub github_api: String,
    pub crates_io: String,
}

pub const GITHUB_API_ENV: &str = "RUSTARCADE_GITHUB_API";
pub const CRATES_API_ENV: &str = "RUSTARCADE_CRATES_API";

impl Default for Endpoints {
    fn default() -> Self {
        Self {
            github_api: "https://api.github.com".into(),
            crates_io: "https://crates.io".into(),
        }
    }
}

impl Endpoints {
    /// Defaults, replaced by `RUSTARCADE_GITHUB_API` / `RUSTARCADE_CRATES_API` when set.
    pub fn from_env() -> Self {
        let mut e = Self::default();
        if let Ok(v) = std::env::var(GITHUB_API_ENV)
            && !v.is_empty()
        {
            e.github_api = v.trim_end_matches('/').to_string();
        }
        if let Ok(v) = std::env::var(CRATES_API_ENV)
            && !v.is_empty()
        {
            e.crates_io = v.trim_end_matches('/').to_string();
        }
        e
    }
}
