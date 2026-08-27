//! Platform-aware application directories, path safety helpers, and atomic file writes.

use std::fs;
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use directories::ProjectDirs;
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::error::{SecurityError, StorageError};

/// Environment variable that relocates every RustArcade directory under one root.
pub const HOME_ENV: &str = "RUSTARCADE_HOME";

/// All directories RustArcade uses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppPaths {
    config: PathBuf,
    data: PathBuf,
    cache: PathBuf,
    state: PathBuf,
    override_root: Option<PathBuf>,
}

impl AppPaths {
    /// Build paths under a single root (used by `RUSTARCADE_HOME` and by tests).
    pub fn from_root(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        Self {
            config: root.join("config"),
            data: root.join("data"),
            cache: root.join("cache"),
            state: root.join("state"),
            override_root: Some(root),
        }
    }

    /// Standard per-user directories (XDG on Linux, Library on macOS, AppData on Windows).
    pub fn system() -> Result<Self, StorageError> {
        let dirs = ProjectDirs::from("", "", "rustarcade").ok_or(StorageError::NoHomeDirectory)?;
        let data = dirs.data_dir().to_path_buf();
        let state = dirs
            .state_dir()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| data.join("state-logs"));
        Ok(Self {
            config: dirs.config_dir().to_path_buf(),
            data,
            cache: dirs.cache_dir().to_path_buf(),
            state,
            override_root: None,
        })
    }

    /// Resolve paths from an explicit override, else `RUSTARCADE_HOME`, else the system dirs.
    pub fn discover(override_root: Option<PathBuf>) -> Result<Self, StorageError> {
        if let Some(root) = override_root {
            return Ok(Self::from_root(root));
        }
        if let Some(root) = std::env::var_os(HOME_ENV).filter(|v| !v.is_empty()) {
            return Ok(Self::from_root(PathBuf::from(root)));
        }
        Self::system()
    }

    /// Whether the layout comes from an explicit root override.
    pub fn override_root(&self) -> Option<&Path> {
        self.override_root.as_deref()
    }

    /// Create every directory RustArcade needs (idempotent).
    pub fn ensure(&self) -> Result<(), StorageError> {
        for dir in self.all_dirs() {
            fs::create_dir_all(&dir).map_err(|e| StorageError::io("create", &dir, e))?;
        }
        Ok(())
    }

    fn all_dirs(&self) -> Vec<PathBuf> {
        vec![
            self.config.clone(),
            self.data.clone(),
            self.cache.clone(),
            self.state.clone(),
            self.games_dir(),
            self.bin_dir(),
            self.catalog_cache_dir(),
            self.state_dir(),
            self.downloads_dir(),
            self.build_dir(),
            self.logs_dir(),
        ]
    }

    pub fn config_dir(&self) -> &Path {
        &self.config
    }
    pub fn data_dir(&self) -> &Path {
        &self.data
    }
    pub fn cache_dir(&self) -> &Path {
        &self.cache
    }
    pub fn state_root(&self) -> &Path {
        &self.state
    }
    pub fn config_file(&self) -> PathBuf {
        self.config.join("config.toml")
    }
    /// Managed game installations: `data/games/<id>/current/...`.
    pub fn games_dir(&self) -> PathBuf {
        self.data.join("games")
    }
    pub fn game_dir(&self, id: &str) -> PathBuf {
        self.games_dir().join(id)
    }
    /// Convenience launchers (symlinks / copies) for installed games.
    pub fn bin_dir(&self) -> PathBuf {
        self.data.join("bin")
    }
    /// Cached copy of the remote catalog.
    pub fn catalog_cache_dir(&self) -> PathBuf {
        self.data.join("catalog")
    }
    /// Registry, library and history files.
    pub fn state_dir(&self) -> PathBuf {
        self.data.join("state")
    }
    pub fn registry_file(&self) -> PathBuf {
        self.state_dir().join("registry.json")
    }
    pub fn library_file(&self) -> PathBuf {
        self.state_dir().join("library.json")
    }
    pub fn history_file(&self) -> PathBuf {
        self.state_dir().join("history.json")
    }
    pub fn update_cache_file(&self) -> PathBuf {
        self.cache.join("update-check.json")
    }
    pub fn catalog_meta_file(&self) -> PathBuf {
        self.cache.join("catalog-meta.json")
    }
    pub fn downloads_dir(&self) -> PathBuf {
        self.cache.join("downloads")
    }
    pub fn build_dir(&self) -> PathBuf {
        self.cache.join("build")
    }
    pub fn logs_dir(&self) -> PathBuf {
        self.state.join("logs")
    }

    /// Human-readable listing used by `doctor` and the settings screen.
    pub fn describe(&self) -> Vec<(&'static str, PathBuf)> {
        vec![
            ("config", self.config.clone()),
            ("data", self.data.clone()),
            ("games", self.games_dir()),
            ("bin", self.bin_dir()),
            ("catalog cache", self.catalog_cache_dir()),
            ("state", self.state_dir()),
            ("downloads", self.downloads_dir()),
            ("logs", self.logs_dir()),
        ]
    }

    /// Ensure `candidate` lives inside the managed data directory.
    pub fn ensure_managed(&self, candidate: &Path) -> Result<(), SecurityError> {
        if is_within(&self.data, candidate) {
            Ok(())
        } else {
            Err(SecurityError::PathOutsideManagedRoot {
                path: candidate.to_path_buf(),
            })
        }
    }
}

/// Lexically normalize a path: drop `.` components and resolve `..` against previous
/// components (never above the root/prefix).
pub fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for comp in path.components() {
        match comp {
            Component::CurDir => {}
            Component::ParentDir => {
                let popped = out
                    .components()
                    .next_back()
                    .is_some_and(|c| matches!(c, Component::Normal(_)));
                if popped {
                    out.pop();
                } else if !matches!(
                    out.components().next_back(),
                    Some(Component::RootDir | Component::Prefix(_))
                ) {
                    out.push("..");
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// True when `candidate` is `root` or inside `root` (lexical comparison after
/// normalization; both must be absolute or both relative).
pub fn is_within(root: &Path, candidate: &Path) -> bool {
    let root = normalize(root);
    let candidate = normalize(candidate);
    candidate.starts_with(&root)
}

/// Validate a manifest-provided relative path and return it as a [`PathBuf`].
///
/// Rejects absolute paths, drive prefixes, `..`, empty or dot components, NUL bytes,
/// backslashes on Unix (they would be literal file names), and more than `max_depth`
/// components.
pub fn safe_relative(value: &str, max_depth: usize) -> Result<PathBuf, SecurityError> {
    let reject = |reason: &str| SecurityError::UnsafePath {
        value: value.to_string(),
        reason: reason.to_string(),
    };
    if value.is_empty() {
        return Err(reject("empty path"));
    }
    if value.contains('\0') {
        return Err(reject("contains a NUL byte"));
    }
    if value.contains('\\') {
        return Err(reject("backslashes are not allowed; use `/`"));
    }
    if value.starts_with('/') {
        return Err(reject("absolute paths are not allowed"));
    }
    let bytes = value.as_bytes();
    if bytes.len() >= 2 && bytes[1] == b':' && bytes[0].is_ascii_alphabetic() {
        return Err(reject("drive prefixes are not allowed"));
    }
    let mut out = PathBuf::new();
    let mut depth = 0;
    for part in value.split('/') {
        match part {
            "" => return Err(reject("empty path component")),
            "." | ".." => return Err(reject("`.` and `..` components are not allowed")),
            p if p.chars().any(char::is_control) => {
                return Err(reject("control characters are not allowed"));
            }
            p => {
                depth += 1;
                if depth > max_depth {
                    return Err(reject("path is nested too deeply"));
                }
                out.push(p);
            }
        }
    }
    Ok(out)
}

/// Join `rel` (already validated by [`safe_relative`]) onto `root` and re-check containment.
pub fn safe_join(root: &Path, rel: &Path) -> Result<PathBuf, SecurityError> {
    let joined = root.join(rel);
    if is_within(root, &joined) {
        Ok(joined)
    } else {
        Err(SecurityError::PathOutsideManagedRoot { path: joined })
    }
}

static NONCE_COUNTER: AtomicU64 = AtomicU64::new(0);

/// A short unique token for staging directories and temp files.
pub fn nonce() -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let counter = NONCE_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{millis:x}-{:x}-{counter:x}", std::process::id())
}

/// Write `bytes` to `path` atomically: write a sibling temp file, flush, then rename.
pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), StorageError> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).map_err(|e| StorageError::io("create", parent, e))?;
    let tmp = parent.join(format!(
        ".{}.tmp-{}",
        path.file_name().and_then(|n| n.to_str()).unwrap_or("file"),
        nonce()
    ));
    let result = (|| -> io::Result<()> {
        let mut file = fs::File::create(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        rename_with_retry(&tmp, path)
    })();
    if let Err(e) = result {
        let _ = fs::remove_file(&tmp);
        return Err(StorageError::io("write", path, e));
    }
    Ok(())
}

/// `fs::rename` with a few retries (Windows can transiently refuse renames of files that
/// an antivirus or indexer still holds open).
pub fn rename_with_retry(from: &Path, to: &Path) -> io::Result<()> {
    let mut last = None;
    for attempt in 0..6 {
        match fs::rename(from, to) {
            Ok(()) => return Ok(()),
            Err(e) => {
                last = Some(e);
                std::thread::sleep(Duration::from_millis(25 * (attempt + 1)));
            }
        }
    }
    Err(last.unwrap_or_else(|| io::Error::other("rename failed")))
}

/// `fs::remove_dir_all` with retries; succeeds when the path does not exist.
pub fn remove_dir_all_retry(path: &Path) -> io::Result<()> {
    let mut last = None;
    for attempt in 0..6 {
        match fs::remove_dir_all(path) {
            Ok(()) => return Ok(()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(e) => {
                last = Some(e);
                std::thread::sleep(Duration::from_millis(25 * (attempt + 1)));
            }
        }
    }
    Err(last.unwrap_or_else(|| io::Error::other("remove_dir_all failed")))
}

/// Serialize `value` as pretty JSON and write it atomically.
pub fn write_json_atomic<T: Serialize>(path: &Path, value: &T) -> Result<(), StorageError> {
    let bytes = serde_json::to_vec_pretty(value).map_err(|e| StorageError::Corrupt {
        path: path.to_path_buf(),
        reason: format!("serialization failed: {e}"),
    })?;
    atomic_write(path, &bytes)
}

/// Read a JSON file. Returns `Ok(None)` when it does not exist and
/// `Err(StorageError::Corrupt)` when it cannot be parsed.
pub fn read_json<T: DeserializeOwned>(path: &Path) -> Result<Option<T>, StorageError> {
    let bytes = match fs::read(path) {
        Ok(b) => b,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(StorageError::io("read", path, e)),
    };
    if bytes.iter().all(u8::is_ascii_whitespace) {
        return Ok(None);
    }
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|e| StorageError::Corrupt {
            path: path.to_path_buf(),
            reason: e.to_string(),
        })
}

/// Move a corrupt file aside (`<name>.corrupt-<nonce>`) so a fresh one can be written.
pub fn quarantine(path: &Path) -> Option<PathBuf> {
    let target = path.with_extension(format!("corrupt-{}", nonce()));
    fs::rename(path, &target).ok().map(|_| target)
}

/// Check that a directory exists (creating it if needed) and is writable.
pub fn check_writable(dir: &Path) -> Result<(), StorageError> {
    fs::create_dir_all(dir).map_err(|e| StorageError::io("create", dir, e))?;
    let probe = dir.join(format!(".write-test-{}", nonce()));
    match fs::write(&probe, b"ok") {
        Ok(()) => {
            let _ = fs::remove_file(&probe);
            Ok(())
        }
        Err(_) => Err(StorageError::NotWritable {
            path: dir.to_path_buf(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn override_root_places_all_dirs_under_root() {
        let root = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_root(root.path());
        for (_, dir) in paths.describe() {
            assert!(dir.starts_with(root.path()), "{dir:?}");
        }
        assert!(paths.logs_dir().starts_with(root.path()));
        paths.ensure().unwrap();
        assert!(paths.games_dir().is_dir());
        assert!(paths.logs_dir().is_dir());
        paths.ensure().unwrap(); // idempotent
    }

    #[test]
    fn discover_prefers_explicit_override() {
        let root = tempfile::tempdir().unwrap();
        let paths = AppPaths::discover(Some(root.path().to_path_buf())).unwrap();
        assert_eq!(paths.override_root(), Some(root.path()));
    }

    #[test]
    fn normalize_resolves_dots() {
        assert_eq!(
            normalize(Path::new("/a/b/../c/./d")),
            PathBuf::from("/a/c/d")
        );
        assert_eq!(normalize(Path::new("/../a")), PathBuf::from("/a"));
        assert_eq!(normalize(Path::new("a/../../b")), PathBuf::from("../b"));
    }

    #[test]
    fn is_within_handles_prefix_paths() {
        assert!(is_within(Path::new("/data"), Path::new("/data/games/x")));
        assert!(is_within(Path::new("/data"), Path::new("/data")));
        assert!(!is_within(Path::new("/data"), Path::new("/data2/games")));
        assert!(!is_within(Path::new("/data"), Path::new("/data/../etc")));
        assert!(!is_within(Path::new("/data"), Path::new("/etc/passwd")));
    }

    #[test]
    fn safe_relative_rejects_unsafe_values() {
        for bad in [
            "",
            "/abs",
            "../x",
            "a/../b",
            "./a",
            "a//b",
            "C:/x",
            "a\\b",
            "a\0b",
            "a/b/c/d/e/f",
        ] {
            assert!(safe_relative(bad, 4).is_err(), "{bad:?} should be rejected");
        }
        assert_eq!(
            safe_relative("bin/game", 4).unwrap(),
            PathBuf::from("bin/game")
        );
        assert_eq!(safe_relative("game", 4).unwrap(), PathBuf::from("game"));
    }

    #[test]
    fn safe_join_rejects_escape() {
        let root = Path::new("/root");
        assert_eq!(
            safe_join(root, Path::new("a/b")).unwrap(),
            PathBuf::from("/root/a/b")
        );
        assert!(safe_join(root, Path::new("../x")).is_err());
    }

    #[test]
    fn atomic_write_replaces_existing_and_leaves_no_temp() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("state.json");
        atomic_write(&file, b"one").unwrap();
        atomic_write(&file, b"two").unwrap();
        assert_eq!(fs::read_to_string(&file).unwrap(), "two");
        let leftovers: Vec<_> = fs::read_dir(dir.path()).unwrap().flatten().collect();
        assert_eq!(leftovers.len(), 1);
    }

    #[test]
    fn json_roundtrip_and_corrupt_detection() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("x.json");
        assert!(read_json::<Vec<u32>>(&file).unwrap().is_none());
        write_json_atomic(&file, &vec![1u32, 2, 3]).unwrap();
        assert_eq!(read_json::<Vec<u32>>(&file).unwrap(), Some(vec![1, 2, 3]));
        fs::write(&file, b"{not json").unwrap();
        assert!(matches!(
            read_json::<Vec<u32>>(&file),
            Err(StorageError::Corrupt { .. })
        ));
        let moved = quarantine(&file).unwrap();
        assert!(moved.exists());
        assert!(!file.exists());
    }

    #[test]
    fn nonce_is_unique() {
        let a = nonce();
        let b = nonce();
        assert_ne!(a, b);
    }
}
