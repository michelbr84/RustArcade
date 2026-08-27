//! User configuration (`config.toml`).

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::catalog::manifest::InstallerKind;
use crate::error::ConfigError;
use crate::paths::atomic_write;

/// Default remote catalog location (the `catalog/` directory of the RustArcade repository).
pub const DEFAULT_REMOTE_CATALOG: &str =
    "https://raw.githubusercontent.com/michelbr84/RustArcade/main/catalog";

/// Colour theme for the TUI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    #[default]
    Default,
    Mono,
}

impl Theme {
    pub fn label(self) -> &'static str {
        match self {
            Theme::Default => "Default",
            Theme::Mono => "Monochrome",
        }
    }

    pub fn next(self) -> Theme {
        match self {
            Theme::Default => Theme::Mono,
            Theme::Mono => Theme::Default,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct GeneralConfig {
    pub theme: Theme,
    pub confirm_before_install: bool,
    pub confirm_before_update: bool,
    pub show_experimental: bool,
    pub show_welcome: bool,
}

impl Default for GeneralConfig {
    fn default() -> Self {
        Self {
            theme: Theme::Default,
            confirm_before_install: true,
            confirm_before_update: true,
            show_experimental: true,
            show_welcome: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct CatalogConfig {
    pub remote_url: String,
    pub auto_update: bool,
    pub refresh_hours: u64,
}

impl Default for CatalogConfig {
    fn default() -> Self {
        Self {
            remote_url: DEFAULT_REMOTE_CATALOG.to_string(),
            auto_update: true,
            refresh_hours: 24,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct UpdatesConfig {
    pub check_on_start: bool,
    pub cache_hours: u64,
}

impl Default for UpdatesConfig {
    fn default() -> Self {
        Self {
            check_on_start: true,
            cache_hours: 6,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct InstallConfig {
    /// When set, this installer kind is tried before the manifest's own order.
    pub preferred_method: Option<InstallerKind>,
    /// Refuse GitHub release installs that cannot be checksum-verified.
    pub require_checksum: bool,
    /// Keep downloaded archives in the cache after installation.
    pub keep_downloads: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct NetworkConfig {
    pub connect_timeout_secs: u64,
    pub read_timeout_secs: u64,
    pub max_download_mb: u64,
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            connect_timeout_secs: 15,
            read_timeout_secs: 60,
            max_download_mb: 200,
        }
    }
}

/// Complete configuration. Unknown keys are ignored so older binaries keep working with
/// newer files; missing keys take their defaults.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Config {
    pub general: GeneralConfig,
    pub catalog: CatalogConfig,
    pub updates: UpdatesConfig,
    pub install: InstallConfig,
    pub network: NetworkConfig,
}

impl Config {
    /// Load from `path`; a missing file yields the defaults.
    pub fn load(path: &Path) -> Result<Config, ConfigError> {
        let text = match std::fs::read_to_string(path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Config::default()),
            Err(e) => {
                return Err(ConfigError::Io {
                    path: path.to_path_buf(),
                    source: e,
                });
            }
        };
        let config: Config = toml::from_str(&text).map_err(|e| ConfigError::Parse {
            path: path.to_path_buf(),
            message: e.to_string(),
        })?;
        config.validate()?;
        Ok(config)
    }

    /// Persist atomically as pretty TOML.
    pub fn save(&self, path: &Path) -> Result<(), ConfigError> {
        self.validate()?;
        let text = toml::to_string_pretty(self).map_err(|e| ConfigError::Invalid {
            field: "config".into(),
            reason: e.to_string(),
        })?;
        atomic_write(path, text.as_bytes()).map_err(|e| ConfigError::Io {
            path: path.to_path_buf(),
            source: std::io::Error::other(e.to_string()),
        })
    }

    pub fn validate(&self) -> Result<(), ConfigError> {
        let url = url::Url::parse(&self.catalog.remote_url).map_err(|e| ConfigError::Invalid {
            field: "catalog.remote_url".into(),
            reason: e.to_string(),
        })?;
        if url.scheme() != "https" {
            return Err(ConfigError::Invalid {
                field: "catalog.remote_url".into(),
                reason: "must use https://".into(),
            });
        }
        if self.network.max_download_mb == 0 || self.network.connect_timeout_secs == 0 {
            return Err(ConfigError::Invalid {
                field: "network".into(),
                reason: "timeouts and limits must be greater than zero".into(),
            });
        }
        Ok(())
    }

    /// Bytes allowed for a single download.
    pub fn max_download_bytes(&self) -> u64 {
        self.network.max_download_mb.saturating_mul(1024 * 1024)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_valid_and_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let cfg = Config::default();
        cfg.validate().unwrap();
        cfg.save(&path).unwrap();
        assert_eq!(Config::load(&path).unwrap(), cfg);
    }

    #[test]
    fn missing_file_yields_defaults() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            Config::load(&dir.path().join("nope.toml")).unwrap(),
            Config::default()
        );
    }

    #[test]
    fn partial_file_fills_defaults_and_ignores_unknown_keys() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "[general]\ntheme = \"mono\"\nfuture_key = 1\n").unwrap();
        let cfg = Config::load(&path).unwrap();
        assert_eq!(cfg.general.theme, Theme::Mono);
        assert!(cfg.general.confirm_before_install);
        assert_eq!(cfg.catalog.refresh_hours, 24);
    }

    #[test]
    fn rejects_insecure_catalog_url_and_bad_toml() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "[catalog]\nremote_url = \"http://example.com\"\n").unwrap();
        assert!(matches!(
            Config::load(&path),
            Err(ConfigError::Invalid { .. })
        ));
        std::fs::write(&path, "not = [valid").unwrap();
        assert!(matches!(
            Config::load(&path),
            Err(ConfigError::Parse { .. })
        ));
    }
}
