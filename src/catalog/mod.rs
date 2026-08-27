//! Game catalog: manifest types, validation, loading, and search.

pub mod builtin;
pub mod index;
pub mod manifest;
pub mod remote;
pub mod search;
pub mod validate;

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::error::{CatalogError, Problem};

pub use manifest::{Category, GameId, GameManifest, InstallerKind, InstallerSpec, SupportStatus};
pub use validate::ValidationMode;

/// Where a catalog entry came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CatalogOrigin {
    Builtin,
    Remote,
    Local(PathBuf),
}

impl CatalogOrigin {
    pub fn label(&self) -> &'static str {
        match self {
            CatalogOrigin::Builtin => "built-in",
            CatalogOrigin::Remote => "remote",
            CatalogOrigin::Local(_) => "local",
        }
    }
}

/// A manifest plus provenance.
#[derive(Debug, Clone)]
pub struct CatalogEntry {
    pub manifest: Arc<GameManifest>,
    pub origin: CatalogOrigin,
    pub file: PathBuf,
}

/// Result of parsing and validating a set of manifest sources.
#[derive(Debug, Default)]
pub struct LoadReport {
    pub ok: Vec<(PathBuf, GameManifest)>,
    pub errors: Vec<CatalogError>,
}

impl LoadReport {
    pub fn is_clean(&self) -> bool {
        self.errors.is_empty()
    }
}

/// Parse + validate every `(path, toml text)` pair, then run cross-file checks.
pub fn load_sources(sources: Vec<(PathBuf, String)>, mode: ValidationMode) -> LoadReport {
    let mut report = LoadReport::default();
    for (path, text) in sources {
        match GameManifest::parse(&text) {
            Err(e) => report.errors.push(validate::parse_error(&path, &text, &e)),
            Ok(manifest) => {
                let problems: Vec<Problem> = validate::validate_manifest(&manifest, mode);
                if problems.is_empty() {
                    report.ok.push((path, manifest));
                } else {
                    report.errors.push(CatalogError::Invalid {
                        file: path,
                        problems,
                    });
                }
            }
        }
    }
    let cross = validate::validate_set(&report.ok);
    if !cross.is_empty() {
        // Drop manifests involved in cross-file problems so a broken pair never loads.
        let bad_ids: Vec<String> = cross
            .iter()
            .flat_map(|e| match e {
                CatalogError::DuplicateId { id, .. } => vec![id.clone()],
                CatalogError::DuplicateExecutable { ids, .. } => ids.clone(),
                CatalogError::Invalid { file, .. } => report
                    .ok
                    .iter()
                    .filter(|(p, _)| p == file)
                    .map(|(_, m)| m.id.to_string())
                    .collect(),
                _ => vec![],
            })
            .collect();
        report
            .ok
            .retain(|(_, m)| !bad_ids.contains(&m.id.to_string()));
        report.errors.extend(cross);
    }
    report.ok.sort_by(|a, b| a.1.id.cmp(&b.1.id));
    report
}

/// Read every `*.toml` in `dir` (sorted) as `(path, text)`.
pub fn read_dir_sources(dir: &Path) -> Result<Vec<(PathBuf, String)>, CatalogError> {
    let entries = fs::read_dir(dir).map_err(|e| CatalogError::Io {
        path: dir.to_path_buf(),
        source: e,
    })?;
    let mut files: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file() && p.extension().and_then(|e| e.to_str()) == Some("toml"))
        .collect();
    files.sort();
    let mut out = Vec::with_capacity(files.len());
    for path in files {
        let text = fs::read_to_string(&path).map_err(|e| CatalogError::Io {
            path: path.clone(),
            source: e,
        })?;
        out.push((path, text));
    }
    Ok(out)
}

/// Load a directory of manifests (or a single manifest file).
pub fn load_path(path: &Path, mode: ValidationMode) -> Result<LoadReport, CatalogError> {
    let sources = if path.is_dir() {
        read_dir_sources(path)?
    } else {
        let text = fs::read_to_string(path).map_err(|e| CatalogError::Io {
            path: path.to_path_buf(),
            source: e,
        })?;
        vec![(path.to_path_buf(), text)]
    };
    Ok(load_sources(sources, mode))
}

/// Load the embedded catalog.
pub fn load_builtin() -> LoadReport {
    load_sources(builtin::sources(), ValidationMode::Strict)
}

/// The merged, in-memory catalog.
#[derive(Debug, Clone, Default)]
pub struct Catalog {
    entries: BTreeMap<GameId, CatalogEntry>,
}

impl Catalog {
    pub fn empty() -> Catalog {
        Catalog::default()
    }

    /// Build from a clean load report.
    pub fn from_report(report: LoadReport, origin: CatalogOrigin) -> Result<Catalog, CatalogError> {
        if let Some(first) = report.errors.into_iter().next() {
            return Err(first);
        }
        let mut catalog = Catalog::empty();
        for (file, manifest) in report.ok {
            catalog.entries.insert(
                manifest.id.clone(),
                CatalogEntry {
                    manifest: Arc::new(manifest),
                    origin: origin.clone(),
                    file,
                },
            );
        }
        Ok(catalog)
    }

    /// The embedded catalog (guaranteed valid by a unit test).
    pub fn builtin() -> Result<Catalog, CatalogError> {
        Catalog::from_report(load_builtin(), CatalogOrigin::Builtin)
    }

    /// Load a directory of manifests, failing on the first problem.
    pub fn from_dir(
        dir: &Path,
        origin: CatalogOrigin,
        mode: ValidationMode,
    ) -> Result<Catalog, CatalogError> {
        Catalog::from_report(load_path(dir, mode)?, origin)
    }

    /// Overlay `other` on top of `self` (entries with the same id are replaced).
    pub fn merge(mut self, other: Catalog) -> Catalog {
        for (id, entry) in other.entries {
            self.entries.insert(id, entry);
        }
        self
    }

    pub fn get(&self, id: &GameId) -> Option<&CatalogEntry> {
        self.entries.get(id)
    }

    pub fn get_str(&self, id: &str) -> Option<&CatalogEntry> {
        GameId::new(id).ok().and_then(|id| self.entries.get(&id))
    }

    pub fn iter(&self) -> impl Iterator<Item = &CatalogEntry> {
        self.entries.values()
    }

    pub fn manifests(&self) -> impl Iterator<Item = &GameManifest> {
        self.entries.values().map(|e| e.manifest.as_ref())
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Categories in use with counts, sorted alphabetically.
    pub fn categories(&self) -> Vec<(Category, usize)> {
        let mut counts: BTreeMap<Category, usize> = BTreeMap::new();
        for m in self.manifests() {
            for c in &m.categories {
                *counts.entry(*c).or_default() += 1;
            }
        }
        let mut out: Vec<(Category, usize)> = counts.into_iter().collect();
        out.sort_by_key(|(c, _)| c.as_str());
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use manifest::tests::FULL;

    #[test]
    fn builtin_catalog_loads_and_validates() {
        let report = load_builtin();
        assert!(
            report.is_clean(),
            "built-in catalog problems: {:#?}",
            report.errors
        );
        assert!(
            report.ok.len() >= 10,
            "expected at least 10 built-in games, found {}",
            report.ok.len()
        );
        let catalog = Catalog::builtin().unwrap();
        assert_eq!(catalog.len(), builtin::count());
        for entry in catalog.iter() {
            assert_eq!(entry.origin, CatalogOrigin::Builtin);
            assert_eq!(
                entry.file.file_stem().unwrap().to_str().unwrap(),
                entry.manifest.id.as_str()
            );
        }
    }

    #[test]
    fn load_sources_reports_parse_validation_and_cross_errors() {
        let good = FULL.to_string();
        let bad_parse = "schema_version = 1\nid = \"x-y\"\ninstall = \"sh\"\n".to_string();
        let bad_valid = FULL.replace("schema_version = 1", "schema_version = 9");
        let dup = FULL.to_string();
        let report = load_sources(
            vec![
                (PathBuf::from("example-game.toml"), good),
                (PathBuf::from("broken.toml"), bad_parse),
                (PathBuf::from("old.toml"), bad_valid),
                (PathBuf::from("dup/example-game.toml"), dup),
            ],
            ValidationMode::Strict,
        );
        assert!(!report.is_clean());
        assert!(
            report
                .errors
                .iter()
                .any(|e| matches!(e, CatalogError::Parse { .. }))
        );
        assert!(
            report
                .errors
                .iter()
                .any(|e| matches!(e, CatalogError::Invalid { .. }))
        );
        assert!(
            report
                .errors
                .iter()
                .any(|e| matches!(e, CatalogError::DuplicateId { .. }))
        );
        // duplicates are dropped from the ok set
        assert!(report.ok.is_empty());
    }

    #[test]
    fn merge_overlays_by_id_and_counts_categories() {
        let base = Catalog::from_report(
            load_sources(
                vec![(PathBuf::from("example-game.toml"), FULL.to_string())],
                ValidationMode::Strict,
            ),
            CatalogOrigin::Builtin,
        )
        .unwrap();
        let overlay = Catalog::from_report(
            load_sources(
                vec![(
                    PathBuf::from("example-game.toml"),
                    FULL.replace("Example Game", "Overlay"),
                )],
                ValidationMode::Strict,
            ),
            CatalogOrigin::Remote,
        )
        .unwrap();
        let merged = base.merge(overlay);
        assert_eq!(merged.len(), 1);
        let entry = merged.get_str("example-game").unwrap();
        assert_eq!(entry.manifest.name, "Overlay");
        assert_eq!(entry.origin, CatalogOrigin::Remote);
        assert_eq!(
            merged.categories(),
            vec![(Category::Board, 1), (Category::Puzzle, 1)]
        );
        assert!(merged.get_str("Bad Id").is_none());
    }

    #[test]
    fn load_path_reads_dirs_and_files() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("example-game.toml"), FULL).unwrap();
        fs::write(dir.path().join("notes.txt"), "ignored").unwrap();
        let report = load_path(dir.path(), ValidationMode::Strict).unwrap();
        assert!(report.is_clean(), "{:?}", report.errors);
        assert_eq!(report.ok.len(), 1);
        let single = load_path(
            &dir.path().join("example-game.toml"),
            ValidationMode::Strict,
        )
        .unwrap();
        assert_eq!(single.ok.len(), 1);
        assert!(load_path(&dir.path().join("missing"), ValidationMode::Strict).is_err());
    }
}
