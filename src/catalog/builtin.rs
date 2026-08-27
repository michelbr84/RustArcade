//! Manifests embedded at compile time by `build.rs`.

use std::path::PathBuf;

include!(concat!(env!("OUT_DIR"), "/builtin_catalog.rs"));

/// Pseudo-path prefix used for embedded manifests in error messages.
pub const BUILTIN_PREFIX: &str = "<built-in>";

/// `(pseudo path, TOML source)` for every embedded manifest.
pub fn sources() -> Vec<(PathBuf, String)> {
    BUILTIN_MANIFESTS
        .iter()
        .map(|(name, text)| {
            (
                PathBuf::from(BUILTIN_PREFIX).join(name),
                (*text).to_string(),
            )
        })
        .collect()
}

/// Number of embedded manifests.
pub fn count() -> usize {
    BUILTIN_MANIFESTS.len()
}
