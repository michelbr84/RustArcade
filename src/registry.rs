//! Installation registry: which games are installed, where, and which files RustArcade owns.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::catalog::manifest::{GameId, InstallerKind};
use crate::error::StorageError;
use crate::paths::{AppPaths, quarantine, read_json, write_json_atomic};

pub const REGISTRY_SCHEMA_VERSION: u32 = 1;

/// Kind of filesystem object RustArcade created and therefore owns.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ManagedKind {
    Dir,
    File,
    Symlink,
}

/// A path (relative to the data directory) owned by an installation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManagedPath {
    pub path: PathBuf,
    pub kind: ManagedKind,
}

/// Where an installation came from, precisely enough to check for updates.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum InstallSource {
    GithubRelease {
        repository: String,
        tag: String,
        asset: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        sha256: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        checksum_source: Option<String>,
    },
    Cargo {
        #[serde(rename = "crate")]
        krate: String,
        version: String,
    },
    GitCargoBuild {
        repository: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reference: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        commit: Option<String>,
    },
}

impl InstallSource {
    pub fn kind(&self) -> InstallerKind {
        match self {
            InstallSource::GithubRelease { .. } => InstallerKind::GithubRelease,
            InstallSource::Cargo { .. } => InstallerKind::Cargo,
            InstallSource::GitCargoBuild { .. } => InstallerKind::GitCargoBuild,
        }
    }

    /// Short human description.
    pub fn describe(&self) -> String {
        match self {
            InstallSource::GithubRelease {
                repository,
                tag,
                asset,
                ..
            } => {
                format!("github.com/{repository} release {tag} ({asset})")
            }
            InstallSource::Cargo { krate, version } => format!("crates.io {krate} {version}"),
            InstallSource::GitCargoBuild {
                repository,
                reference,
                commit,
            } => {
                let mut s = repository.clone();
                if let Some(r) = reference {
                    s.push_str(&format!(" @ {r}"));
                }
                if let Some(c) = commit {
                    s.push_str(&format!(" ({})", &c[..c.len().min(12)]));
                }
                s
            }
        }
    }
}

/// One installed game.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstallRecord {
    pub id: GameId,
    pub name: String,
    pub version: String,
    pub installer: InstallerKind,
    /// Executable path relative to the data directory (`games/<id>/current/bin/<exe>`).
    pub executable: PathBuf,
    /// Convenience launcher relative to the data directory (`bin/<exe>`), if created.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bin_link: Option<PathBuf>,
    pub repository: String,
    pub source: InstallSource,
    pub installed_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    /// Everything RustArcade created for this game, relative to the data directory.
    pub managed: Vec<ManagedPath>,
    /// Log file of the most recent install/update.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub log: Option<PathBuf>,
    /// Whether the downloaded artifact was verified against a SHA-256 digest.
    #[serde(default)]
    pub checksum_verified: bool,
}

impl InstallRecord {
    /// Absolute executable path.
    pub fn executable_path(&self, paths: &AppPaths) -> PathBuf {
        paths.data_dir().join(&self.executable)
    }

    /// Absolute paths of every managed object.
    pub fn managed_paths(&self, paths: &AppPaths) -> Vec<(PathBuf, ManagedKind)> {
        self.managed
            .iter()
            .map(|m| (paths.data_dir().join(&m.path), m.kind))
            .collect()
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct RegistryFile {
    #[serde(default = "default_schema")]
    schema_version: u32,
    #[serde(default)]
    games: BTreeMap<String, InstallRecord>,
}

fn default_schema() -> u32 {
    REGISTRY_SCHEMA_VERSION
}

/// The persisted set of installations.
#[derive(Debug)]
pub struct Registry {
    path: PathBuf,
    records: BTreeMap<GameId, InstallRecord>,
}

impl Registry {
    /// Load `registry.json`. A corrupt file is quarantined and reported (second value)
    /// instead of aborting startup.
    pub fn load(path: &Path) -> Result<(Registry, Option<PathBuf>), StorageError> {
        let mut quarantined = None;
        let file = match read_json::<RegistryFile>(path) {
            Ok(Some(f)) => f,
            Ok(None) => RegistryFile::default(),
            Err(StorageError::Corrupt { .. }) => {
                quarantined = quarantine(path);
                RegistryFile::default()
            }
            Err(e) => return Err(e),
        };
        let records = file
            .games
            .into_values()
            .map(|r| (r.id.clone(), r))
            .collect();
        Ok((
            Registry {
                path: path.to_path_buf(),
                records,
            },
            quarantined,
        ))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn get(&self, id: &GameId) -> Option<&InstallRecord> {
        self.records.get(id)
    }

    pub fn contains(&self, id: &GameId) -> bool {
        self.records.contains_key(id)
    }

    pub fn iter(&self) -> impl Iterator<Item = &InstallRecord> {
        self.records.values()
    }

    pub fn len(&self) -> usize {
        self.records.len()
    }

    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// Insert or replace a record and persist immediately.
    pub fn upsert(&mut self, record: InstallRecord) -> Result<(), StorageError> {
        self.records.insert(record.id.clone(), record);
        self.save()
    }

    /// Remove a record and persist immediately.
    pub fn remove(&mut self, id: &GameId) -> Result<Option<InstallRecord>, StorageError> {
        let removed = self.records.remove(id);
        if removed.is_some() {
            self.save()?;
        }
        Ok(removed)
    }

    pub fn save(&self) -> Result<(), StorageError> {
        let file = RegistryFile {
            schema_version: REGISTRY_SCHEMA_VERSION,
            games: self
                .records
                .iter()
                .map(|(k, v)| (k.to_string(), v.clone()))
                .collect(),
        };
        write_json_atomic(&self.path, &file)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn sample(id: &str) -> InstallRecord {
        let now = Utc::now();
        InstallRecord {
            id: id.parse().unwrap(),
            name: "Sample".into(),
            version: "1.0.0".into(),
            installer: InstallerKind::Cargo,
            executable: PathBuf::from(format!("games/{id}/current/bin/{id}")),
            bin_link: Some(PathBuf::from(format!("bin/{id}"))),
            repository: "https://github.com/example/sample".into(),
            source: InstallSource::Cargo {
                krate: id.into(),
                version: "1.0.0".into(),
            },
            installed_at: now,
            updated_at: now,
            managed: vec![
                ManagedPath {
                    path: PathBuf::from(format!("games/{id}")),
                    kind: ManagedKind::Dir,
                },
                ManagedPath {
                    path: PathBuf::from(format!("bin/{id}")),
                    kind: ManagedKind::Symlink,
                },
            ],
            log: None,
            checksum_verified: false,
        }
    }

    #[test]
    fn roundtrip_and_remove() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state/registry.json");
        let (mut reg, q) = Registry::load(&path).unwrap();
        assert!(q.is_none());
        assert!(reg.is_empty());
        reg.upsert(sample("alpha")).unwrap();
        reg.upsert(sample("beta")).unwrap();
        let (reg2, _) = Registry::load(&path).unwrap();
        assert_eq!(reg2.len(), 2);
        assert_eq!(
            reg2.get(&"alpha".parse().unwrap()).unwrap().version,
            "1.0.0"
        );
        let mut reg3 = reg2;
        assert!(reg3.remove(&"alpha".parse().unwrap()).unwrap().is_some());
        assert!(reg3.remove(&"alpha".parse().unwrap()).unwrap().is_none());
        let (reg4, _) = Registry::load(&path).unwrap();
        assert_eq!(reg4.len(), 1);
    }

    #[test]
    fn corrupt_registry_is_quarantined_not_fatal() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("registry.json");
        std::fs::write(&path, "{ definitely not json").unwrap();
        let (reg, q) = Registry::load(&path).unwrap();
        assert!(reg.is_empty());
        let q = q.expect("corrupt file moved aside");
        assert!(q.exists());
        assert!(!path.exists());
    }

    #[test]
    fn paths_resolve_under_data_dir() {
        let root = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_root(root.path());
        let rec = sample("gamma");
        assert_eq!(
            rec.executable_path(&paths),
            paths.data_dir().join("games/gamma/current/bin/gamma")
        );
        let managed = rec.managed_paths(&paths);
        assert_eq!(managed.len(), 2);
        assert!(managed.iter().all(|(p, _)| p.starts_with(paths.data_dir())));
        assert!(rec.source.describe().contains("crates.io gamma 1.0.0"));
    }

    #[test]
    fn source_serializes_with_kind_tag() {
        let s = InstallSource::GithubRelease {
            repository: "o/r".into(),
            tag: "v1".into(),
            asset: "a.tar.gz".into(),
            sha256: None,
            checksum_source: Some("api-digest".into()),
        };
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains("\"kind\":\"github-release\""));
        assert_eq!(serde_json::from_str::<InstallSource>(&json).unwrap(), s);
        assert_eq!(s.kind(), InstallerKind::GithubRelease);
    }
}
