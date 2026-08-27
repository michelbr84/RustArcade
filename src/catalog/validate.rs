//! Strict manifest validation.
//!
//! Every rule is a small function so that each one can be unit-tested in isolation.
//! The rules exist to keep the catalog declarative and safe: no shell snippets, no
//! path traversal, no plain-HTTP sources, no surprises for the installers.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::error::{CatalogError, Problem};
use crate::paths::safe_relative;
use crate::platform::{Arch, Os};

use super::manifest::{
    CargoSpec, GameManifest, GitCargoBuildSpec, GithubReleaseSpec, InstallerSpec,
    SUPPORTED_SCHEMA_VERSION,
};

/// Whether local (loopback / `file://`) sources are acceptable. Only tests use `AllowLocalSources`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ValidationMode {
    #[default]
    Strict,
    AllowLocalSources,
}

/// Placeholders accepted in release asset patterns.
pub const ASSET_PLACEHOLDERS: &[&str] = &["version", "tag", "os", "arch", "target", "ext"];

const MAX_NAME: usize = 64;
const MAX_SUMMARY: usize = 160;
const MAX_DESCRIPTION: usize = 4000;
const MAX_CATEGORIES: usize = 5;
const MAX_TAGS: usize = 12;
const MAX_INSTALLERS: usize = 6;
const MAX_ARGS: usize = 32;
const MAX_ARG_LEN: usize = 256;
const MAX_PATH_DEPTH: usize = 4;

const ENV_DENYLIST: &[&str] = &[
    "PATH",
    "LD_PRELOAD",
    "LD_LIBRARY_PATH",
    "LD_AUDIT",
    "DYLD_INSERT_LIBRARIES",
    "DYLD_LIBRARY_PATH",
    "DYLD_FRAMEWORK_PATH",
    "CARGO_HOME",
    "RUSTUP_HOME",
    "HOME",
    "USERPROFILE",
];

struct Collector {
    problems: Vec<Problem>,
}

impl Collector {
    fn push(&mut self, path: impl Into<String>, message: impl Into<String>) {
        self.problems.push(Problem {
            path: path.into(),
            message: message.into(),
        });
    }
}

/// Validate a single manifest. Returns an empty list when it is acceptable.
pub fn validate_manifest(m: &GameManifest, mode: ValidationMode) -> Vec<Problem> {
    let mut c = Collector {
        problems: Vec::new(),
    };

    if m.schema_version != SUPPORTED_SCHEMA_VERSION {
        c.push(
            "schema_version",
            format!(
                "unsupported schema version {} (this RustArcade understands {SUPPORTED_SCHEMA_VERSION})",
                m.schema_version
            ),
        );
    }

    check_text(&mut c, "name", &m.name, 1, MAX_NAME);
    check_text(&mut c, "summary", &m.summary, 1, MAX_SUMMARY);
    if let Some(d) = &m.description {
        check_text(&mut c, "description", d, 1, MAX_DESCRIPTION);
    }
    if let Some(n) = &m.notes {
        check_text(&mut c, "notes", n, 1, MAX_DESCRIPTION);
    }
    if let Some(l) = &m.license {
        check_text(&mut c, "license", l, 1, 100);
    }
    for (i, a) in m.authors.iter().enumerate() {
        check_text(&mut c, format!("authors[{i}]"), a, 1, 100);
    }

    if let Err(reason) = check_https_url(&m.repository, ValidationMode::Strict) {
        c.push("repository", reason);
    }
    if let Some(h) = &m.homepage
        && let Err(reason) = check_https_url(h, ValidationMode::Strict)
    {
        c.push("homepage", reason);
    }

    if m.categories.is_empty() {
        c.push("categories", "at least one category is required");
    }
    if m.categories.len() > MAX_CATEGORIES {
        c.push(
            "categories",
            format!("at most {MAX_CATEGORIES} categories are allowed"),
        );
    }
    if has_duplicates(m.categories.iter()) {
        c.push("categories", "duplicate category");
    }
    if m.tags.len() > MAX_TAGS {
        c.push("tags", format!("at most {MAX_TAGS} tags are allowed"));
    }
    for (i, t) in m.tags.iter().enumerate() {
        if !is_slug(t, 32) {
            c.push(
                format!("tags[{i}]"),
                "tags must be lowercase letters, digits and dashes",
            );
        }
    }
    if has_duplicates(m.tags.iter()) {
        c.push("tags", "duplicate tag");
    }
    if let Some(d) = &m.verified_on
        && chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d").is_err()
    {
        c.push("verified_on", "must be a date formatted YYYY-MM-DD");
    }

    // compatibility
    if m.compatibility.os.is_empty() {
        c.push(
            "compatibility.os",
            "at least one operating system is required",
        );
    }
    if has_duplicates(m.compatibility.os.iter()) {
        c.push("compatibility.os", "duplicate operating system");
    }
    if has_duplicates(m.compatibility.arch.iter()) {
        c.push("compatibility.arch", "duplicate architecture");
    }
    if let Some(t) = &m.compatibility.min_terminal
        && (t.cols == 0 || t.rows == 0)
    {
        c.push(
            "compatibility.min_terminal",
            "cols and rows must be greater than zero",
        );
    }

    // requirements
    for (i, cmd) in m.requirements.commands.iter().enumerate() {
        if !is_command_name(cmd) {
            c.push(
                format!("requirements.commands[{i}]"),
                "must be a bare program name",
            );
        }
    }
    for (i, cmd) in m.requirements.optional_commands.iter().enumerate() {
        if !is_command_name(cmd) {
            c.push(
                format!("requirements.optional_commands[{i}]"),
                "must be a bare program name",
            );
        }
    }
    if let Some(n) = &m.requirements.notes {
        check_text(&mut c, "requirements.notes", n, 1, 500);
    }

    // run
    if let Err(reason) = check_executable_name(&m.run.executable) {
        c.push("run.executable", reason);
    }
    if m.run.args.len() > MAX_ARGS {
        c.push(
            "run.args",
            format!("at most {MAX_ARGS} arguments are allowed"),
        );
    }
    for (i, a) in m.run.args.iter().enumerate() {
        if a.is_empty() || a.len() > MAX_ARG_LEN || a.chars().any(char::is_control) {
            c.push(
                format!("run.args[{i}]"),
                "arguments must be non-empty, printable and at most 256 characters",
            );
        }
    }
    for (k, v) in &m.run.env {
        if let Err(reason) = check_env_key(k) {
            c.push(format!("run.env.{k}"), reason);
        }
        if v.len() > 1024 || v.chars().any(char::is_control) {
            c.push(
                format!("run.env.{k}"),
                "values must be printable and at most 1024 characters",
            );
        }
    }

    // installers
    if m.installers.is_empty() {
        c.push("installers", "at least one installer is required");
    }
    if m.installers.len() > MAX_INSTALLERS {
        c.push(
            "installers",
            format!("at most {MAX_INSTALLERS} installers are allowed"),
        );
    }
    let mut coverage: HashMap<(super::manifest::InstallerKind, Os), Vec<usize>> = HashMap::new();
    for (i, inst) in m.installers.iter().enumerate() {
        let prefix = format!("installers[{i}]");
        validate_installer_common(&mut c, &prefix, m, inst);
        match inst {
            InstallerSpec::Cargo(s) => validate_cargo(&mut c, &prefix, s),
            InstallerSpec::GithubRelease(s) => validate_github_release(&mut c, &prefix, s),
            InstallerSpec::GitCargoBuild(s) => validate_git_build(&mut c, &prefix, s, mode),
        }
        let effective: Vec<Os> = if inst.os().is_empty() {
            m.compatibility.os.clone()
        } else {
            inst.os().to_vec()
        };
        for os in effective {
            coverage.entry((inst.kind(), os)).or_default().push(i);
        }
    }
    for ((kind, os), indices) in coverage.iter().filter(|(_, v)| v.len() > 1) {
        c.push(
            "installers",
            format!("installers {indices:?} are both `{kind}` installers for {os}; restrict them with `os = [...]`"),
        );
    }
    for os in &m.compatibility.os {
        if !m.installers.iter().any(|i| i.applies_to(*os)) {
            c.push("installers", format!("no installer applies to {os}"));
        }
    }

    c.problems
}

fn validate_installer_common(
    c: &mut Collector,
    prefix: &str,
    m: &GameManifest,
    inst: &InstallerSpec,
) {
    if has_duplicates(inst.os().iter()) {
        c.push(format!("{prefix}.os"), "duplicate operating system");
    }
    for os in inst.os() {
        if !m.compatibility.os.contains(os) {
            c.push(
                format!("{prefix}.os"),
                format!("{os} is not listed in compatibility.os"),
            );
        }
    }
    if let Some(b) = inst.binary() {
        match safe_relative(b, MAX_PATH_DEPTH) {
            Err(e) => c.push(format!("{prefix}.binary"), e.to_string()),
            Ok(path) => {
                let file = path.file_name().and_then(|f| f.to_str()).unwrap_or("");
                if let Err(reason) = check_executable_name(file) {
                    c.push(format!("{prefix}.binary"), reason);
                }
            }
        }
    }
    if inst.warnings().len() > 5 {
        c.push(
            format!("{prefix}.warnings"),
            "at most 5 warnings are allowed",
        );
    }
    for (i, w) in inst.warnings().iter().enumerate() {
        check_text(c, format!("{prefix}.warnings[{i}]"), w, 1, 240);
    }
}

fn validate_cargo(c: &mut Collector, prefix: &str, s: &CargoSpec) {
    if !is_crate_name(&s.krate) {
        c.push(format!("{prefix}.crate"), "invalid crate name");
    }
    if let Some(v) = &s.version
        && semver::VersionReq::parse(v).is_err()
    {
        c.push(
            format!("{prefix}.version"),
            "must be a semver requirement such as `^1.2`",
        );
    }
    validate_features(c, prefix, &s.features);
    validate_bins(c, prefix, &s.bins);
}

fn validate_github_release(c: &mut Collector, prefix: &str, s: &GithubReleaseSpec) {
    if !is_github_slug(&s.repository) {
        c.push(format!("{prefix}.repository"), "must be `owner/repo`");
    }
    if let Some(a) = &s.asset
        && let Err(reason) = check_asset_pattern(a, false)
    {
        c.push(format!("{prefix}.asset"), reason);
    }
    for (key, pattern) in &s.asset_patterns {
        if !is_platform_key(key) {
            c.push(
                format!("{prefix}.asset_patterns.{key}"),
                "key must be `<os>-<arch>` such as `linux-x86_64`",
            );
        }
        if let Err(reason) = check_asset_pattern(pattern, false) {
            c.push(format!("{prefix}.asset_patterns.{key}"), reason);
        }
    }
    if let Some(ca) = &s.checksum_asset
        && let Err(reason) = check_asset_pattern(ca, true)
    {
        c.push(format!("{prefix}.checksum_asset"), reason);
    }
    for (name, digest) in &s.sha256 {
        if name.is_empty() || name.contains(['/', '\\']) || name.chars().any(char::is_control) {
            c.push(
                format!("{prefix}.sha256"),
                format!("`{name}` is not a valid asset name"),
            );
        }
        if !is_sha256_hex(digest) {
            c.push(
                format!("{prefix}.sha256.{name}"),
                "must be 64 hexadecimal characters",
            );
        }
    }
    if !s.sha256.is_empty() && s.tag.is_none() {
        c.push(
            format!("{prefix}.sha256"),
            "pinned digests require a pinned `tag`",
        );
    }
    if let Some(t) = &s.tag
        && !is_git_ref(t)
    {
        c.push(format!("{prefix}.tag"), "invalid tag name");
    }
}

fn validate_git_build(
    c: &mut Collector,
    prefix: &str,
    s: &GitCargoBuildSpec,
    mode: ValidationMode,
) {
    if let Err(reason) = check_https_url(&s.repository, mode) {
        c.push(format!("{prefix}.repository"), reason);
    }
    if let Some(r) = &s.reference
        && !is_git_ref(r)
    {
        c.push(format!("{prefix}.reference"), "invalid git reference");
    }
    if let Some(commit) = &s.commit
        && !(commit.len() == 40
            && commit
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()))
    {
        c.push(
            format!("{prefix}.commit"),
            "must be a full 40-character lowercase commit hash",
        );
    }
    if let Some(p) = &s.path
        && let Err(e) = safe_relative(p, MAX_PATH_DEPTH)
    {
        c.push(format!("{prefix}.path"), e.to_string());
    }
    if let Some(p) = &s.package
        && !is_crate_name(p)
    {
        c.push(format!("{prefix}.package"), "invalid package name");
    }
    validate_features(c, prefix, &s.features);
    validate_bins(c, prefix, &s.bins);
}

fn validate_features(c: &mut Collector, prefix: &str, features: &[String]) {
    for (i, f) in features.iter().enumerate() {
        let ok = !f.is_empty()
            && f.len() <= 64
            && f.split('/').count() <= 2
            && f.split('/').all(|part| {
                !part.is_empty()
                    && part
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
            });
        if !ok {
            c.push(format!("{prefix}.features[{i}]"), "invalid feature name");
        }
    }
}

fn validate_bins(c: &mut Collector, prefix: &str, bins: &[String]) {
    for (i, b) in bins.iter().enumerate() {
        if let Err(reason) = check_executable_name(b) {
            c.push(format!("{prefix}.bins[{i}]"), reason);
        }
    }
}

fn check_text(c: &mut Collector, path: impl Into<String>, value: &str, min: usize, max: usize) {
    let path = path.into();
    let len = value.chars().count();
    if len < min {
        c.push(path, "must not be empty");
    } else if len > max {
        c.push(path, format!("must be at most {max} characters"));
    } else if value
        .chars()
        .any(|ch| ch.is_control() && ch != '\n' && ch != '\t')
    {
        c.push(path, "must not contain control characters");
    }
}

/// HTTPS URL check shared by `repository`, `homepage` and git sources.
pub fn check_https_url(value: &str, mode: ValidationMode) -> Result<(), String> {
    let url = url::Url::parse(value).map_err(|e| format!("invalid URL: {e}"))?;
    let host = url.host_str().unwrap_or("");
    let local_ok = mode == ValidationMode::AllowLocalSources
        && (url.scheme() == "file"
            || (url.scheme() == "http" && matches!(host, "127.0.0.1" | "localhost" | "[::1]")));
    if url.scheme() != "https" && !local_ok {
        return Err(format!(
            "`{}://` is not allowed; use https://",
            url.scheme()
        ));
    }
    if url.scheme() != "file" && host.is_empty() {
        return Err("URL has no host".into());
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err("URL must not contain credentials".into());
    }
    if url.query().is_some() || url.fragment().is_some() {
        return Err("URL must not contain a query string or fragment".into());
    }
    if host == "github.com" {
        let segments: Vec<&str> = url
            .path_segments()
            .map(|s| s.filter(|p| !p.is_empty()).collect())
            .unwrap_or_default();
        if segments.len() < 2 {
            return Err(
                "GitHub URLs must point at a repository (https://github.com/owner/repo)".into(),
            );
        }
    }
    Ok(())
}

/// Executable / binary file-name rule.
pub fn check_executable_name(value: &str) -> Result<(), String> {
    if value.is_empty() {
        return Err("must not be empty".into());
    }
    if value.len() > 64 {
        return Err("must be at most 64 characters".into());
    }
    if value == "." || value == ".." {
        return Err("must be a file name".into());
    }
    if value.contains(['/', '\\']) {
        return Err("must be a bare file name without directories".into());
    }
    if value.to_ascii_lowercase().ends_with(".exe") {
        return Err("do not add `.exe`; RustArcade appends it on Windows".into());
    }
    let mut chars = value.chars();
    let first = chars.next().unwrap_or('_');
    if !first.is_ascii_alphanumeric() {
        return Err("must start with a letter or digit".into());
    }
    if !value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-'))
    {
        return Err("only letters, digits, `.`, `_` and `-` are allowed".into());
    }
    Ok(())
}

fn check_env_key(key: &str) -> Result<(), String> {
    if key.is_empty() || key.len() > 64 {
        return Err("environment variable names must be 1-64 characters".into());
    }
    if !key
        .bytes()
        .next()
        .is_some_and(|b| b.is_ascii_uppercase() || b == b'_')
        || !key
            .bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
    {
        return Err("environment variable names must be UPPER_SNAKE_CASE".into());
    }
    if ENV_DENYLIST.contains(&key) || key.starts_with("LD_") || key.starts_with("DYLD_") {
        return Err(format!("`{key}` may not be set by a manifest"));
    }
    Ok(())
}

/// Validate a release asset pattern (no directories, only known placeholders).
pub fn check_asset_pattern(pattern: &str, allow_asset_placeholder: bool) -> Result<(), String> {
    if pattern.is_empty() || pattern.len() > 200 {
        return Err("must be 1-200 characters".into());
    }
    if pattern.contains(['/', '\\']) || pattern.chars().any(char::is_control) {
        return Err("must be a bare file name".into());
    }
    let mut rest = pattern;
    while let Some(start) = rest.find('{') {
        let after = &rest[start + 1..];
        let Some(end) = after.find('}') else {
            return Err("unbalanced `{` in pattern".into());
        };
        let name = &after[..end];
        let known =
            ASSET_PLACEHOLDERS.contains(&name) || (allow_asset_placeholder && name == "asset");
        if !known {
            return Err(format!(
                "unknown placeholder `{{{name}}}` (allowed: {})",
                ASSET_PLACEHOLDERS.join(", ")
            ));
        }
        rest = &after[end + 1..];
    }
    if rest.contains('}') {
        return Err("unbalanced `}` in pattern".into());
    }
    Ok(())
}

pub fn is_platform_key(key: &str) -> bool {
    let Some((os, arch)) = key.split_once('-') else {
        return false;
    };
    Os::ALL.iter().any(|o| o.as_str() == os) && Arch::ALL.iter().any(|a| a.as_str() == arch)
}

pub fn is_github_slug(value: &str) -> bool {
    let Some((owner, repo)) = value.split_once('/') else {
        return false;
    };
    let ok_part = |p: &str| {
        !p.is_empty()
            && p != "."
            && p != ".."
            && p.len() <= 100
            && p.bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-'))
    };
    ok_part(owner) && ok_part(repo) && !repo.contains('/')
}

pub fn is_crate_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .next()
            .is_some_and(|b| b.is_ascii_alphanumeric())
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

pub fn is_git_ref(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && !value.starts_with('-')
        && !value.starts_with('/')
        && !value.ends_with('/')
        && !value.contains("..")
        && !value.contains("//")
        && !value.ends_with(".lock")
        && !value.ends_with('.')
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'/' | b'-'))
}

pub fn is_sha256_hex(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit())
}

pub fn is_slug(value: &str, max: usize) -> bool {
    !value.is_empty()
        && value.len() <= max
        && value
            .bytes()
            .next()
            .is_some_and(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        && value
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

fn is_command_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && !value.contains(['/', '\\'])
        && value
            .bytes()
            .next()
            .is_some_and(|b| b.is_ascii_alphanumeric())
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-' | b'+'))
}

fn has_duplicates<T: Ord, I: Iterator<Item = T>>(iter: I) -> bool {
    let mut seen = BTreeSet::new();
    for item in iter {
        if !seen.insert(item) {
            return true;
        }
    }
    false
}

/// Cross-file checks: ids must match file stems and be unique; executables must be unique.
pub fn validate_set(manifests: &[(PathBuf, GameManifest)]) -> Vec<CatalogError> {
    let mut errors = Vec::new();
    let mut by_id: HashMap<&str, Vec<PathBuf>> = HashMap::new();
    let mut by_exe: HashMap<&str, Vec<String>> = HashMap::new();
    for (file, m) in manifests {
        by_id.entry(m.id.as_str()).or_default().push(file.clone());
        by_exe
            .entry(m.run.executable.as_str())
            .or_default()
            .push(m.id.to_string());
        if let Some(stem) = file.file_stem().and_then(|s| s.to_str())
            && stem != m.id.as_str()
        {
            errors.push(CatalogError::Invalid {
                file: file.clone(),
                problems: vec![Problem {
                    path: "id".into(),
                    message: format!("id `{}` must match the file name `{stem}.toml`", m.id),
                }],
            });
        }
    }
    let mut seen = HashSet::new();
    for (id, files) in by_id {
        if files.len() > 1 && seen.insert(id.to_string()) {
            errors.push(CatalogError::DuplicateId {
                id: id.to_string(),
                files,
            });
        }
    }
    for (exe, ids) in by_exe {
        if ids.len() > 1 {
            errors.push(CatalogError::DuplicateExecutable {
                executable: exe.to_string(),
                ids,
            });
        }
    }
    errors.sort_by_key(|e| e.to_string());
    errors
}

/// Convert a TOML parse error into a [`CatalogError::Parse`] with line/column.
pub fn parse_error(file: &Path, text: &str, err: &toml::de::Error) -> CatalogError {
    let (line, col) = err
        .span()
        .map(|span| line_col(text, span.start))
        .map(|(l, c)| (Some(l), Some(c)))
        .unwrap_or((None, None));
    let mut message = err.message().to_string();
    if message.contains("unknown field") {
        message.push_str(" — RustArcade manifests do not support arbitrary commands; use a typed installer (`cargo`, `github-release`, `git-cargo-build`)");
    }
    CatalogError::Parse {
        file: file.to_path_buf(),
        line,
        col,
        message,
    }
}

fn line_col(text: &str, offset: usize) -> (usize, usize) {
    let offset = offset.min(text.len());
    let before = &text[..offset];
    let line = before.matches('\n').count() + 1;
    let col = before.rfind('\n').map(|i| offset - i).unwrap_or(offset + 1);
    (line, col)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::manifest::tests::FULL;

    fn parse(text: &str) -> GameManifest {
        GameManifest::parse(text).unwrap()
    }

    fn problems(text: &str) -> Vec<Problem> {
        validate_manifest(&parse(text), ValidationMode::Strict)
    }

    fn assert_problem(text: &str, path: &str) {
        let ps = problems(text);
        assert!(
            ps.iter().any(|p| p.path == path),
            "expected a problem at `{path}`, got {ps:?}"
        );
    }

    #[test]
    fn full_manifest_is_valid() {
        let ps = problems(FULL);
        assert!(ps.is_empty(), "{ps:?}");
    }

    #[test]
    fn unsupported_schema_rejected() {
        assert_problem(
            &FULL.replace("schema_version = 1", "schema_version = 2"),
            "schema_version",
        );
    }

    #[test]
    fn http_repository_rejected() {
        assert_problem(
            &FULL.replace(
                "repository = \"https://github.com/example/game\"\nhomepage",
                "repository = \"http://github.com/example/game\"\nhomepage",
            ),
            "repository",
        );
        assert!(check_https_url("git://github.com/a/b", ValidationMode::Strict).is_err());
        assert!(check_https_url("file:///tmp/repo", ValidationMode::Strict).is_err());
        assert!(check_https_url("https://user:pw@github.com/a/b", ValidationMode::Strict).is_err());
        assert!(check_https_url("https://github.com/only-owner", ValidationMode::Strict).is_err());
        assert!(check_https_url("https://github.com/a/b?x=1", ValidationMode::Strict).is_err());
        assert!(check_https_url("https://codeberg.org/a/b", ValidationMode::Strict).is_ok());
    }

    #[test]
    fn local_sources_only_in_local_mode() {
        assert!(check_https_url("file:///tmp/repo", ValidationMode::AllowLocalSources).is_ok());
        assert!(
            check_https_url(
                "http://127.0.0.1:8080/repo",
                ValidationMode::AllowLocalSources
            )
            .is_ok()
        );
        assert!(
            check_https_url("http://example.com/repo", ValidationMode::AllowLocalSources).is_err()
        );
    }

    #[test]
    fn executable_name_rules() {
        assert!(check_executable_name("chess-tui").is_ok());
        assert!(check_executable_name("balatro_tui").is_ok());
        for bad in [
            "", "bin/game", "game.exe", "..", ".hidden", "-x", "a b", "C:\\x",
        ] {
            assert!(check_executable_name(bad).is_err(), "{bad:?}");
        }
        assert_problem(
            &FULL.replace("executable = \"example-game\"", "executable = \"bin/game\""),
            "run.executable",
        );
    }

    #[test]
    fn env_denylist_and_arg_rules() {
        assert_problem(
            &FULL.replace("EXAMPLE_MODE = \"tui\"", "LD_PRELOAD = \"x.so\""),
            "run.env.LD_PRELOAD",
        );
        assert_problem(
            &FULL.replace("EXAMPLE_MODE = \"tui\"", "PATH = \"/tmp\""),
            "run.env.PATH",
        );
        assert_problem(
            &FULL.replace("EXAMPLE_MODE = \"tui\"", "lower = \"x\""),
            "run.env.lower",
        );
        assert_problem(
            &FULL.replace("args = [\"--no-sound\"]", "args = [\"a\\nb\"]"),
            "run.args[0]",
        );
    }

    #[test]
    fn binary_path_rules() {
        assert_problem(
            &FULL.replace("checksum_asset = \"SHA256SUMS\"", "binary = \"../evil\""),
            "installers[0].binary",
        );
        assert_problem(
            &FULL.replace(
                "checksum_asset = \"SHA256SUMS\"",
                "binary = \"/usr/bin/sh\"",
            ),
            "installers[0].binary",
        );
        assert_problem(
            &FULL.replace("checksum_asset = \"SHA256SUMS\"", "binary = \"a/b/c/d/e\""),
            "installers[0].binary",
        );
        let ok =
            problems(&FULL.replace("checksum_asset = \"SHA256SUMS\"", "binary = \"dir/game\""));
        assert!(ok.is_empty(), "{ok:?}");
    }

    #[test]
    fn github_release_rules() {
        assert_problem(
            &FULL.replace(
                "repository = \"example/game\"",
                "repository = \"https://github.com/example/game\"",
            ),
            "installers[0].repository",
        );
        assert_problem(
            &FULL.replace("{target}.{ext}", "{nope}.{ext}"),
            "installers[0].asset",
        );
        assert_problem(
            &FULL.replace("windows-x86_64 = ", "plan9-x86_64 = "),
            "installers[0].asset_patterns.plan9-x86_64",
        );
        assert_problem(
            &FULL.replace(
                "\"0000000000000000000000000000000000000000000000000000000000000000\"",
                "\"zz\"",
            ),
            "installers[0].sha256.game-1.0.0-x86_64-unknown-linux-gnu.tar.gz",
        );
        assert!(check_asset_pattern("dir/x.tar.gz", false).is_err());
        assert!(check_asset_pattern("{asset}.sha256", true).is_ok());
        assert!(check_asset_pattern("{asset}.sha256", false).is_err());
        assert!(check_asset_pattern("x{", false).is_err());
    }

    #[test]
    fn pinned_digests_require_tag() {
        assert_problem(
            &FULL.replace("tag = \"v1.0.0\"\n", ""),
            "installers[0].sha256",
        );
    }

    #[test]
    fn git_build_rules() {
        assert_problem(
            &FULL.replace(
                "reference = \"v1.0.0\"",
                "reference = \"--upload-pack=evil\"",
            ),
            "installers[2].reference",
        );
        assert_problem(
            &FULL.replace("reference = \"v1.0.0\"", "reference = \"a..b\""),
            "installers[2].reference",
        );
        assert_problem(
            &FULL.replace(
                "package = \"example-game\"",
                "package = \"example-game\"\ncommit = \"abc\"",
            ),
            "installers[2].commit",
        );
        assert_problem(
            &FULL.replace(
                "package = \"example-game\"",
                "package = \"example-game\"\npath = \"../x\"",
            ),
            "installers[2].path",
        );
        assert_problem(
            &FULL.replace(
                "repository = \"https://github.com/example/game\"\nreference",
                "repository = \"ssh://git@github.com/example/game\"\nreference",
            ),
            "installers[2].repository",
        );
    }

    #[test]
    fn cargo_rules() {
        assert_problem(
            &FULL.replace("crate = \"example-game\"", "crate = \"bad crate\""),
            "installers[1].crate",
        );
        assert_problem(
            &FULL.replace("version = \"^1\"", "version = \"not a req\""),
            "installers[1].version",
        );
        assert_problem(
            &FULL.replace("features = [\"extra\"]", "features = [\"a b\"]"),
            "installers[1].features[0]",
        );
        assert_problem(
            &FULL.replace("bins = [\"example-game\"]", "bins = [\"x/y\"]"),
            "installers[1].bins[0]",
        );
    }

    #[test]
    fn os_subset_and_coverage_rules() {
        assert_problem(
            &FULL.replace("os = [\"linux\"]", "os = [\"linux\", \"linux\"]"),
            "installers[1].os",
        );
        let m = FULL.replace(
            "os = [\"linux\", \"macos\", \"windows\"]",
            "os = [\"macos\"]",
        );
        // cargo installer restricted to linux, which is not in compatibility.os
        assert_problem(&m, "installers[1].os");
        // Two github-release installers without os restriction overlap.
        let dup = FULL.replace("[[installers]]\ntype = \"cargo\"", "[[installers]]\ntype = \"github-release\"\nrepository = \"example/game\"\n\n[[installers]]\ntype = \"cargo\"");
        assert_problem(&dup, "installers");
        // No installer for windows.
        let none = FULL
            .replace(
                "[[installers]]\ntype = \"github-release\"",
                "[[installers]]\nos = [\"linux\", \"macos\"]\ntype = \"github-release\"",
            )
            .replace(
                "[[installers]]\ntype = \"git-cargo-build\"",
                "[[installers]]\nos = [\"linux\"]\ntype = \"git-cargo-build\"",
            );
        let ps = problems(&none);
        assert!(
            ps.iter()
                .any(|p| p.message.contains("no installer applies to windows")),
            "{ps:?}"
        );
    }

    #[test]
    fn categories_tags_dates() {
        assert_problem(
            &FULL.replace("categories = [\"puzzle\", \"board\"]", "categories = []"),
            "categories",
        );
        assert_problem(
            &FULL.replace(
                "categories = [\"puzzle\", \"board\"]",
                "categories = [\"puzzle\", \"puzzle\"]",
            ),
            "categories",
        );
        assert_problem(
            &FULL.replace("tags = [\"demo\"]", "tags = [\"Demo Tag\"]"),
            "tags[0]",
        );
        assert_problem(
            &FULL.replace(
                "verified_on = \"2026-08-27\"",
                "verified_on = \"yesterday\"",
            ),
            "verified_on",
        );
        assert_problem(
            &FULL.replace("os = [\"linux\", \"macos\", \"windows\"]", "os = []"),
            "compatibility.os",
        );
    }

    #[test]
    fn set_rules_detect_duplicates_and_stem_mismatch() {
        let a = parse(FULL);
        let mut b = parse(FULL);
        b.run.executable = "other".into();
        let errors = validate_set(&[
            (PathBuf::from("x/example-game.toml"), a.clone()),
            (PathBuf::from("y/example-game.toml"), b),
        ]);
        assert!(
            errors
                .iter()
                .any(|e| matches!(e, CatalogError::DuplicateId { .. })),
            "{errors:?}"
        );
        let errors = validate_set(&[(PathBuf::from("x/wrong-name.toml"), a.clone())]);
        assert!(
            errors
                .iter()
                .any(|e| matches!(e, CatalogError::Invalid { .. }))
        );
        let mut c = parse(FULL);
        c.id = "other-game".parse().unwrap();
        let errors = validate_set(&[
            (PathBuf::from("example-game.toml"), a),
            (PathBuf::from("other-game.toml"), c),
        ]);
        assert!(
            errors
                .iter()
                .any(|e| matches!(e, CatalogError::DuplicateExecutable { .. })),
            "{errors:?}"
        );
    }

    #[test]
    fn parse_error_reports_line_col_and_hint() {
        let text = "schema_version = 1\ninstall = \"curl | sh\"\n";
        let err = GameManifest::parse(text).unwrap_err();
        let converted = parse_error(Path::new("x.toml"), text, &err);
        match converted {
            CatalogError::Parse { line, message, .. } => {
                assert_eq!(line, Some(2));
                assert!(message.contains("typed installer"), "{message}");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn helper_predicates() {
        assert!(is_github_slug("owner/repo.name"));
        assert!(!is_github_slug("owner"));
        assert!(!is_github_slug("../x"));
        assert!(is_git_ref("v1.0.0"));
        assert!(is_git_ref("release/1.x"));
        assert!(!is_git_ref("-x"));
        assert!(!is_git_ref("a..b"));
        assert!(is_platform_key("macos-aarch64"));
        assert!(!is_platform_key("macos"));
        assert!(is_sha256_hex(&"a".repeat(64)));
        assert!(!is_sha256_hex(&"a".repeat(63)));
    }
}
