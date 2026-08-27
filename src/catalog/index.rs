//! `catalog/index.json`: the list of manifests (with SHA-256 digests) that the remote
//! catalog updater downloads and verifies.

use std::fs;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::CatalogError;
use crate::paths::atomic_write;

pub const INDEX_FILE: &str = "index.json";
pub const GAMES_DIR: &str = "games";
pub const INDEX_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexEntry {
    /// File name inside `games/`.
    pub file: String,
    pub id: String,
    pub sha256: String,
    pub size: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogIndex {
    pub schema_version: u32,
    pub updated_at: DateTime<Utc>,
    pub games: Vec<IndexEntry>,
}

impl CatalogIndex {
    /// Two indexes are equivalent when they list the same files and digests.
    pub fn same_content(&self, other: &CatalogIndex) -> bool {
        self.schema_version == other.schema_version && self.games == other.games
    }
}

/// SHA-256 hex digest of `bytes`.
pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

/// Build an index from the manifests in `catalog_dir/games/`.
pub fn build_index(catalog_dir: &Path) -> Result<CatalogIndex, CatalogError> {
    let games = catalog_dir.join(GAMES_DIR);
    let mut entries = Vec::new();
    let read = fs::read_dir(&games).map_err(|e| CatalogError::Io {
        path: games.clone(),
        source: e,
    })?;
    let mut files: Vec<PathBuf> = read
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("toml"))
        .collect();
    files.sort();
    for path in files {
        let bytes = fs::read(&path).map_err(|e| CatalogError::Io {
            path: path.clone(),
            source: e,
        })?;
        let text = String::from_utf8_lossy(&bytes);
        let manifest = super::manifest::GameManifest::parse(&text)
            .map_err(|e| super::validate::parse_error(&path, &text, &e))?;
        entries.push(IndexEntry {
            file: path
                .file_name()
                .and_then(|f| f.to_str())
                .unwrap_or_default()
                .to_string(),
            id: manifest.id.to_string(),
            sha256: sha256_hex(&bytes),
            size: bytes.len() as u64,
        });
    }
    Ok(CatalogIndex {
        schema_version: INDEX_SCHEMA_VERSION,
        updated_at: Utc::now(),
        games: entries,
    })
}

/// Read an existing `index.json`.
pub fn read_index(catalog_dir: &Path) -> Result<Option<CatalogIndex>, CatalogError> {
    let path = catalog_dir.join(INDEX_FILE);
    match fs::read(&path) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|e| CatalogError::Index {
                url: path.display().to_string(),
                reason: e.to_string(),
            }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(CatalogError::Io { path, source: e }),
    }
}

/// Write (or, with `check_only`, verify) `index.json`. Returns `true` when the file on
/// disk was already up to date.
pub fn write_index(catalog_dir: &Path, check_only: bool) -> Result<bool, CatalogError> {
    let fresh = build_index(catalog_dir)?;
    let existing = read_index(catalog_dir)?;
    let up_to_date = existing.as_ref().is_some_and(|e| e.same_content(&fresh));
    if up_to_date || check_only {
        return Ok(up_to_date);
    }
    let path = catalog_dir.join(INDEX_FILE);
    let mut json = serde_json::to_string_pretty(&fresh).map_err(|e| CatalogError::Index {
        url: path.display().to_string(),
        reason: e.to_string(),
    })?;
    json.push('\n');
    atomic_write(&path, json.as_bytes()).map_err(|e| CatalogError::Io {
        path,
        source: std::io::Error::other(e.to_string()),
    })?;
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::manifest::tests::FULL;

    #[test]
    fn repository_index_is_current() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("catalog");
        assert!(
            write_index(&dir, true).unwrap(),
            "catalog/index.json is out of date — run `cargo run -- catalog index`"
        );
    }

    #[test]
    fn index_build_write_check() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("games")).unwrap();
        fs::write(dir.path().join("games/example-game.toml"), FULL).unwrap();
        assert!(!write_index(dir.path(), false).unwrap());
        let idx = read_index(dir.path()).unwrap().unwrap();
        assert_eq!(idx.games.len(), 1);
        assert_eq!(idx.games[0].id, "example-game");
        assert_eq!(idx.games[0].sha256, sha256_hex(FULL.as_bytes()));
        assert!(write_index(dir.path(), true).unwrap());
        fs::write(
            dir.path().join("games/example-game.toml"),
            FULL.replace("Example Game", "Renamed"),
        )
        .unwrap();
        assert!(!write_index(dir.path(), true).unwrap());
        assert!(!write_index(dir.path(), false).unwrap());
        assert!(write_index(dir.path(), true).unwrap());
    }
}
