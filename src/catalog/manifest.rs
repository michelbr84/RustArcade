//! Declarative game manifest types (`catalog/games/<id>.toml`).
//!
//! Manifests describe *what* a game is and *which typed installer* can fetch it. They
//! never contain shell commands; `#[serde(deny_unknown_fields)]` rejects any attempt to
//! smuggle one in.

use std::collections::BTreeMap;
use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::platform::{Arch, Os, Platform};

/// The only manifest schema version understood by this build.
pub const SUPPORTED_SCHEMA_VERSION: u32 = 1;

/// Maximum length of a game id.
pub const MAX_ID_LEN: usize = 48;

/// Validated game identifier: lowercase ASCII letters, digits and single dashes.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct GameId(String);

impl GameId {
    pub fn new(value: impl Into<String>) -> Result<Self, String> {
        let value = value.into();
        match id_problem(&value) {
            None => Ok(GameId(value)),
            Some(reason) => Err(format!("invalid game id `{value}`: {reason}")),
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Why an id is invalid, if it is.
pub fn id_problem(value: &str) -> Option<&'static str> {
    if value.len() < 2 {
        return Some("must be at least 2 characters");
    }
    if value.len() > MAX_ID_LEN {
        return Some("must be at most 48 characters");
    }
    if !value
        .bytes()
        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    {
        return Some("only lowercase letters, digits and dashes are allowed");
    }
    if value.starts_with('-') || value.ends_with('-') {
        return Some("must not start or end with a dash");
    }
    if value.contains("--") {
        return Some("must not contain consecutive dashes");
    }
    None
}

impl TryFrom<String> for GameId {
    type Error = String;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        GameId::new(value)
    }
}

impl From<GameId> for String {
    fn from(id: GameId) -> String {
        id.0
    }
}

impl FromStr for GameId {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        GameId::new(s)
    }
}

impl fmt::Display for GameId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl AsRef<str> for GameId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

/// Game genre used for filtering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Category {
    Arcade,
    Puzzle,
    Board,
    Card,
    Strategy,
    Roguelike,
    Simulation,
    Sports,
    Typing,
    Idle,
    Classic,
    Multiplayer,
    Other,
}

impl Category {
    pub const ALL: [Category; 13] = [
        Category::Arcade,
        Category::Puzzle,
        Category::Board,
        Category::Card,
        Category::Strategy,
        Category::Roguelike,
        Category::Simulation,
        Category::Sports,
        Category::Typing,
        Category::Idle,
        Category::Classic,
        Category::Multiplayer,
        Category::Other,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Category::Arcade => "arcade",
            Category::Puzzle => "puzzle",
            Category::Board => "board",
            Category::Card => "card",
            Category::Strategy => "strategy",
            Category::Roguelike => "roguelike",
            Category::Simulation => "simulation",
            Category::Sports => "sports",
            Category::Typing => "typing",
            Category::Idle => "idle",
            Category::Classic => "classic",
            Category::Multiplayer => "multiplayer",
            Category::Other => "other",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Category::Arcade => "Arcade",
            Category::Puzzle => "Puzzle",
            Category::Board => "Board",
            Category::Card => "Card",
            Category::Strategy => "Strategy",
            Category::Roguelike => "Roguelike",
            Category::Simulation => "Simulation",
            Category::Sports => "Sports",
            Category::Typing => "Typing",
            Category::Idle => "Idle",
            Category::Classic => "Classic",
            Category::Multiplayer => "Multiplayer",
            Category::Other => "Other",
        }
    }
}

impl fmt::Display for Category {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Category {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Category::ALL
            .iter()
            .copied()
            .find(|c| c.as_str() == s.to_ascii_lowercase())
            .ok_or_else(|| format!("unknown category `{s}`"))
    }
}

/// How much RustArcade trusts a manifest to work.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, Default,
)]
#[serde(rename_all = "kebab-case")]
pub enum SupportStatus {
    /// Installed and launched successfully by RustArcade maintainers.
    Verified,
    /// Install path is unambiguous and reported working by the community.
    CommunityTested,
    /// Builds in principle; inactive or unproven upstream.
    #[default]
    Experimental,
    /// Known not to work right now.
    Broken,
    /// Upstream repository archived.
    Archived,
}

impl SupportStatus {
    pub fn label(self) -> &'static str {
        match self {
            SupportStatus::Verified => "Verified",
            SupportStatus::CommunityTested => "Community Tested",
            SupportStatus::Experimental => "Experimental",
            SupportStatus::Broken => "Broken",
            SupportStatus::Archived => "Archived",
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            SupportStatus::Verified => "verified",
            SupportStatus::CommunityTested => "community-tested",
            SupportStatus::Experimental => "experimental",
            SupportStatus::Broken => "broken",
            SupportStatus::Archived => "archived",
        }
    }

    /// Whether installs should be allowed at all.
    pub fn installable(self) -> bool {
        !matches!(self, SupportStatus::Broken)
    }
}

impl fmt::Display for SupportStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// Minimum terminal size hint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TerminalSize {
    pub cols: u16,
    pub rows: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Compatibility {
    /// Operating systems the game runs on (non-empty).
    pub os: Vec<Os>,
    /// CPU architectures; empty means all supported architectures.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub arch: Vec<Arch>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_terminal: Option<TerminalSize>,
}

impl Compatibility {
    pub fn supports(&self, platform: &Platform) -> bool {
        self.os.contains(&platform.os)
            && (self.arch.is_empty() || self.arch.contains(&platform.arch))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields, default)]
pub struct Requirements {
    /// External programs the game needs at runtime.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub commands: Vec<String>,
    /// External programs that unlock optional features.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub optional_commands: Vec<String>,
    /// Free-text note shown in the install plan (e.g. runtime libraries).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

/// Working directory used when launching the game.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum RunCwd {
    /// The directory RustArcade was started from.
    #[default]
    Current,
    /// The game's managed installation directory.
    Install,
    /// The user's home directory.
    Home,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunSpec {
    /// Executable file name (no directories, no `.exe`).
    pub executable: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
    #[serde(default)]
    pub cwd: RunCwd,
}

/// Installer kind identifier (used in registry, config and CLI flags).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum InstallerKind {
    Cargo,
    GithubRelease,
    GitCargoBuild,
}

impl InstallerKind {
    pub const ALL: [InstallerKind; 3] = [
        InstallerKind::GithubRelease,
        InstallerKind::Cargo,
        InstallerKind::GitCargoBuild,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            InstallerKind::Cargo => "cargo",
            InstallerKind::GithubRelease => "github-release",
            InstallerKind::GitCargoBuild => "git-cargo-build",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            InstallerKind::Cargo => "Cargo (crates.io)",
            InstallerKind::GithubRelease => "GitHub Release",
            InstallerKind::GitCargoBuild => "Git + Cargo build",
        }
    }

    /// Does this method compile the game locally?
    pub fn compiles(self) -> bool {
        !matches!(self, InstallerKind::GithubRelease)
    }
}

impl fmt::Display for InstallerKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for InstallerKind {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        InstallerKind::ALL
            .iter()
            .copied()
            .find(|k| k.as_str() == s)
            .ok_or_else(|| format!("unknown installer type `{s}`"))
    }
}

fn default_true() -> bool {
    true
}

/// `cargo install --root <managed> <crate>`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CargoSpec {
    #[serde(rename = "crate")]
    pub krate: String,
    /// Semver requirement passed as `--version`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub features: Vec<String>,
    #[serde(default = "default_true")]
    pub default_features: bool,
    #[serde(default)]
    pub locked: bool,
    /// Restrict which binaries are built (`--bin`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bins: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub os: Vec<Os>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub binary: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

/// Download a prebuilt asset from a GitHub release.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GithubReleaseSpec {
    /// `owner/repo`.
    pub repository: String,
    /// Asset name pattern with `{version} {tag} {os} {arch} {target} {ext}` placeholders.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub asset: Option<String>,
    /// Per-platform patterns keyed by `os-arch` (take precedence over `asset`).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub asset_patterns: BTreeMap<String, String>,
    /// Name (or pattern with `{asset}`) of a checksum file asset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checksum_asset: Option<String>,
    /// Pinned SHA-256 digests keyed by asset name.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub sha256: BTreeMap<String, String>,
    /// Pin a release tag instead of using the latest.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
    #[serde(default)]
    pub allow_prerelease: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub os: Vec<Os>,
    /// Path of the executable inside the archive (defaults to `run.executable`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub binary: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

/// Clone a git repository and build it with Cargo.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GitCargoBuildSpec {
    /// HTTPS clone URL.
    pub repository: String,
    /// Branch or tag to check out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reference: Option<String>,
    /// Full commit hash the checkout must resolve to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commit: Option<String>,
    /// Relative path of the package inside the repository.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// Workspace package to build.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub features: Vec<String>,
    #[serde(default = "default_true")]
    pub default_features: bool,
    #[serde(default)]
    pub locked: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bins: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub os: Vec<Os>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub binary: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

/// A typed installer definition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum InstallerSpec {
    Cargo(CargoSpec),
    GithubRelease(GithubReleaseSpec),
    GitCargoBuild(GitCargoBuildSpec),
}

impl InstallerSpec {
    pub fn kind(&self) -> InstallerKind {
        match self {
            InstallerSpec::Cargo(_) => InstallerKind::Cargo,
            InstallerSpec::GithubRelease(_) => InstallerKind::GithubRelease,
            InstallerSpec::GitCargoBuild(_) => InstallerKind::GitCargoBuild,
        }
    }

    /// OS restriction for this installer (empty = all OSes the game supports).
    pub fn os(&self) -> &[Os] {
        match self {
            InstallerSpec::Cargo(s) => &s.os,
            InstallerSpec::GithubRelease(s) => &s.os,
            InstallerSpec::GitCargoBuild(s) => &s.os,
        }
    }

    pub fn binary(&self) -> Option<&str> {
        match self {
            InstallerSpec::Cargo(s) => s.binary.as_deref(),
            InstallerSpec::GithubRelease(s) => s.binary.as_deref(),
            InstallerSpec::GitCargoBuild(s) => s.binary.as_deref(),
        }
    }

    pub fn warnings(&self) -> &[String] {
        match self {
            InstallerSpec::Cargo(s) => &s.warnings,
            InstallerSpec::GithubRelease(s) => &s.warnings,
            InstallerSpec::GitCargoBuild(s) => &s.warnings,
        }
    }

    /// Does this installer apply to `os`?
    pub fn applies_to(&self, os: Os) -> bool {
        self.os().is_empty() || self.os().contains(&os)
    }

    /// Human description of where the game comes from.
    pub fn source_label(&self) -> String {
        match self {
            InstallerSpec::Cargo(s) => format!("crates.io/crates/{}", s.krate),
            InstallerSpec::GithubRelease(s) => format!("github.com/{}/releases", s.repository),
            InstallerSpec::GitCargoBuild(s) => s.repository.clone(),
        }
    }
}

/// A complete game manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GameManifest {
    pub schema_version: u32,
    pub id: GameId,
    pub name: String,
    /// One-line description shown in lists.
    pub summary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// HTTPS URL of the source repository.
    pub repository: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub homepage: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub authors: Vec<String>,
    pub categories: Vec<Category>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    #[serde(default)]
    pub support_status: SupportStatus,
    /// `YYYY-MM-DD` date of the last successful verification.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verified_on: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    pub compatibility: Compatibility,
    #[serde(default)]
    pub requirements: Requirements,
    pub run: RunSpec,
    pub installers: Vec<InstallerSpec>,
}

impl GameManifest {
    /// Parse TOML text (structural checks only; see [`crate::catalog::validate`]).
    pub fn parse(text: &str) -> Result<GameManifest, toml::de::Error> {
        toml::from_str(text)
    }

    pub fn supports(&self, platform: &Platform) -> bool {
        self.compatibility.supports(platform)
    }

    /// Installers applicable to `platform`, in manifest order.
    pub fn installers_for(&self, platform: &Platform) -> Vec<&InstallerSpec> {
        self.installers
            .iter()
            .filter(|i| i.applies_to(platform.os))
            .collect()
    }

    /// Executable name produced by `installer` (falls back to `run.executable`).
    pub fn binary_for<'a>(&'a self, installer: &'a InstallerSpec) -> &'a str {
        installer.binary().unwrap_or(&self.run.executable)
    }

    /// `owner/repo` when the repository is hosted on GitHub.
    pub fn github_slug(&self) -> Option<String> {
        let url = url::Url::parse(&self.repository).ok()?;
        if url.host_str()? != "github.com" {
            return None;
        }
        let mut segments = url.path_segments()?;
        let owner = segments.next()?;
        let repo = segments.next()?.trim_end_matches(".git");
        (!owner.is_empty() && !repo.is_empty()).then(|| format!("{owner}/{repo}"))
    }

    /// Case-insensitive haystack for search.
    pub fn search_text(&self) -> String {
        let mut s = String::new();
        s.push_str(self.id.as_str());
        s.push(' ');
        s.push_str(&self.name);
        s.push(' ');
        s.push_str(&self.summary);
        s.push(' ');
        if let Some(d) = &self.description {
            s.push_str(d);
            s.push(' ');
        }
        for c in &self.categories {
            s.push_str(c.as_str());
            s.push(' ');
        }
        for t in &self.tags {
            s.push_str(t);
            s.push(' ');
        }
        s.to_lowercase()
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) const FULL: &str = r#"
schema_version = 1
id = "example-game"
name = "Example Game"
summary = "An example."
description = "Longer text."
repository = "https://github.com/example/game"
homepage = "https://example.org"
license = "MIT"
authors = ["Someone"]
categories = ["puzzle", "board"]
tags = ["demo"]
support_status = "verified"
verified_on = "2026-08-27"

[compatibility]
os = ["linux", "macos", "windows"]
arch = ["x86_64", "aarch64"]
min_terminal = { cols = 80, rows = 24 }

[requirements]
optional_commands = ["stockfish"]

[run]
executable = "example-game"
args = ["--no-sound"]
env = { EXAMPLE_MODE = "tui" }
cwd = "install"

[[installers]]
type = "github-release"
repository = "example/game"
asset = "game-{version}-{target}.{ext}"
checksum_asset = "SHA256SUMS"
tag = "v1.0.0"

[installers.asset_patterns]
windows-x86_64 = "game-win64.zip"

[installers.sha256]
"game-1.0.0-x86_64-unknown-linux-gnu.tar.gz" = "0000000000000000000000000000000000000000000000000000000000000000"

[[installers]]
type = "cargo"
crate = "example-game"
version = "^1"
features = ["extra"]
default_features = false
locked = true
bins = ["example-game"]
os = ["linux"]

[[installers]]
type = "git-cargo-build"
repository = "https://github.com/example/game"
reference = "v1.0.0"
package = "example-game"
"#;

    #[test]
    fn parses_full_manifest() {
        let m = GameManifest::parse(FULL).unwrap();
        assert_eq!(m.id.as_str(), "example-game");
        assert_eq!(m.categories, vec![Category::Puzzle, Category::Board]);
        assert_eq!(m.support_status, SupportStatus::Verified);
        assert_eq!(m.run.cwd, RunCwd::Install);
        assert_eq!(m.installers.len(), 3);
        assert_eq!(m.installers[0].kind(), InstallerKind::GithubRelease);
        match &m.installers[1] {
            InstallerSpec::Cargo(c) => {
                assert_eq!(c.krate, "example-game");
                assert!(!c.default_features);
                assert!(c.locked);
                assert_eq!(c.os, vec![Os::Linux]);
            }
            other => panic!("unexpected {other:?}"),
        }
        assert_eq!(m.github_slug().as_deref(), Some("example/game"));
        assert!(m.supports(&Platform::new(Os::Linux, Arch::X86_64)));
        assert_eq!(
            m.installers_for(&Platform::new(Os::Macos, Arch::Aarch64))
                .len(),
            2
        );
        assert!(m.search_text().contains("demo"));
    }

    #[test]
    fn rejects_unknown_fields_and_shell_snippets() {
        let bad = FULL.replace("[run]\n", "[run]\ncommand = \"curl x | sh\"\n");
        let err = GameManifest::parse(&bad).unwrap_err().to_string();
        assert!(err.contains("unknown field `command`"), "{err}");
        let bad = format!("{FULL}\ninstall = \"make install\"\n");
        assert!(GameManifest::parse(&bad).is_err());
    }

    #[test]
    fn rejects_unknown_installer_type() {
        let bad = FULL.replace("type = \"cargo\"", "type = \"shell\"");
        let err = GameManifest::parse(&bad).unwrap_err().to_string();
        assert!(err.contains("unknown variant `shell`"), "{err}");
    }

    #[test]
    fn rejects_invalid_ids_at_parse_time() {
        let bad = FULL.replace("id = \"example-game\"", "id = \"Bad_ID\"");
        let err = GameManifest::parse(&bad).unwrap_err().to_string();
        assert!(err.contains("invalid game id"), "{err}");
        assert!(id_problem("ok-id").is_none());
        assert!(id_problem("-x").is_some());
        assert!(id_problem("a--b").is_some());
        assert!(id_problem("a").is_some());
        assert!(id_problem(&"a".repeat(49)).is_some());
    }

    #[test]
    fn installer_kind_roundtrips() {
        for k in InstallerKind::ALL {
            assert_eq!(k.as_str().parse::<InstallerKind>().unwrap(), k);
            let json = serde_json::to_string(&k).unwrap();
            assert_eq!(serde_json::from_str::<InstallerKind>(&json).unwrap(), k);
        }
        assert_eq!(
            serde_json::to_string(&InstallerKind::GitCargoBuild).unwrap(),
            "\"git-cargo-build\""
        );
    }

    #[test]
    fn categories_parse() {
        assert_eq!("Puzzle".parse::<Category>().unwrap(), Category::Puzzle);
        assert!("dancing".parse::<Category>().is_err());
    }
}
