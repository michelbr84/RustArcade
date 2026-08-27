//! GitHub release asset selection and checksum-file parsing.

use std::collections::BTreeMap;

use crate::catalog::manifest::GithubReleaseSpec;
use crate::error::InstallError;
use crate::net::ReleaseAsset;
use crate::platform::Platform;

/// Archive extensions tried for `{ext}`, most preferred first.
pub const ARCHIVE_EXTS: &[&str] = &["tar.gz", "tgz", "tar.xz", "txz", "zip"];

const NOISE_TOKENS: &[&str] = &[
    "sha256",
    "sha512",
    "sha1",
    ".sha",
    ".sig",
    ".asc",
    ".pem",
    ".sigstore",
    "checksum",
    "sums",
    ".txt",
    ".md",
    ".json",
    ".deb",
    ".rpm",
    ".msi",
    ".dmg",
    ".pkg",
    ".apk",
    ".appimage",
    "src",
    "source",
    "vendor",
    "installer",
    "dist-manifest",
    ".sbom",
    ".spdx",
    ".jar",
    ".whl",
    ".nupkg",
];

/// Expand a pattern into every concrete asset name it could denote for `platform`.
pub fn expand_pattern(pattern: &str, platform: &Platform, version: &str, tag: &str) -> Vec<String> {
    let mut exts: Vec<&str> = ARCHIVE_EXTS.to_vec();
    if platform.exe_suffix().is_empty() {
        exts.push("");
    } else {
        exts.push("exe");
    }
    let targets = platform.target_triples();
    let mut out = Vec::new();
    for target in &targets {
        for ext in &exts {
            let mut s = pattern.to_string();
            s = s.replace("{version}", version);
            s = s.replace("{tag}", tag);
            s = s.replace("{os}", platform.os.as_str());
            s = s.replace("{arch}", platform.arch.as_str());
            s = s.replace("{target}", target);
            s = s.replace("{ext}", ext);
            if ext.is_empty() {
                s = s.trim_end_matches('.').to_string();
            }
            if !out.contains(&s) {
                out.push(s);
            }
        }
    }
    out
}

/// Pick the release asset for `platform` following the manifest rules.
pub fn select_asset<'a>(
    assets: &'a [ReleaseAsset],
    spec: &GithubReleaseSpec,
    platform: &Platform,
    version: &str,
    tag: &str,
) -> Result<&'a ReleaseAsset, InstallError> {
    let pattern = spec
        .asset_patterns
        .get(&platform.key())
        .or(spec.asset.as_ref());
    if let Some(pattern) = pattern {
        let candidates = expand_pattern(pattern, platform, version, tag);
        for candidate in &candidates {
            if let Some(asset) = assets
                .iter()
                .find(|a| a.name.eq_ignore_ascii_case(candidate))
            {
                return Ok(asset);
            }
        }
        return Err(InstallError::NoMatchingAsset {
            tried: candidates,
            available: assets.iter().map(|a| a.name.clone()).collect(),
        });
    }
    heuristic_select(assets, platform)
}

fn heuristic_select<'a>(
    assets: &'a [ReleaseAsset],
    platform: &Platform,
) -> Result<&'a ReleaseAsset, InstallError> {
    let os_tokens = platform.os.asset_tokens();
    let arch_tokens = platform.arch.asset_tokens();
    let foreign = platform.arch.foreign_tokens();
    let other_os: Vec<&str> = crate::platform::Os::ALL
        .iter()
        .filter(|o| **o != platform.os)
        .flat_map(|o| o.asset_tokens().iter().copied())
        .collect();

    // Mask this platform's own OS tokens before scanning for other OSes, so that e.g.
    // "darwin" is not mistaken for the Windows token "win".
    let mask_own = |n: &str| {
        let mut masked = n.to_string();
        for t in os_tokens {
            masked = masked.replace(t, "#");
        }
        masked
    };
    let usable: Vec<(&ReleaseAsset, String)> = assets
        .iter()
        .map(|a| (a, a.name.to_ascii_lowercase()))
        .filter(|(_, n)| !NOISE_TOKENS.iter().any(|t| n.contains(t)))
        .filter(|(_, n)| os_tokens.iter().any(|t| n.contains(t)))
        .filter(|(_, n)| {
            let masked = mask_own(n);
            !other_os.iter().any(|t| masked.contains(t))
        })
        .collect();
    let any_arch_tagged = usable.iter().any(|(_, n)| {
        arch_tokens
            .iter()
            .chain(foreign.iter())
            .any(|t| n.contains(t))
    });

    let mut scored: Vec<(i32, &ReleaseAsset)> = Vec::new();
    for (asset, name) in &usable {
        if foreign.iter().any(|t| name.contains(t)) {
            continue;
        }
        let has_arch = arch_tokens.iter().any(|t| name.contains(t));
        if !has_arch && (any_arch_tagged || platform.arch != crate::platform::Arch::X86_64) {
            continue;
        }
        let mut score = 0;
        if name.ends_with(".tar.gz") || name.ends_with(".tgz") {
            score += 40;
        } else if name.ends_with(".tar.xz") || name.ends_with(".txz") {
            score += 35;
        } else if name.ends_with(".zip") {
            score += if platform.exe_suffix().is_empty() {
                30
            } else {
                40
            };
        } else if name.ends_with(".exe") {
            if platform.exe_suffix().is_empty() {
                continue;
            }
            score += 20;
        } else if name.contains('.') && !name.contains(".tar") {
            // Unknown extension: probably not a binary for us.
            continue;
        } else {
            if !platform.exe_suffix().is_empty() {
                continue;
            }
            score += 10;
        }
        if name.contains("musl") || name.contains("msvc") {
            score += 5;
        }
        if has_arch {
            score += 15;
        }
        scored.push((score, asset));
    }
    scored.sort_by(|a, b| {
        b.0.cmp(&a.0)
            .then_with(|| a.1.name.len().cmp(&b.1.name.len()))
    });
    match scored.as_slice() {
        [] => Err(InstallError::NoMatchingAsset {
            tried: vec![],
            available: assets.iter().map(|a| a.name.clone()).collect(),
        }),
        [(top, best), (second, other), ..]
            if top == second && best.name.len() == other.name.len() =>
        {
            Err(InstallError::AmbiguousAsset {
                candidates: scored
                    .iter()
                    .filter(|(s, _)| s == top)
                    .map(|(_, a)| a.name.clone())
                    .collect(),
            })
        }
        [(_, best), ..] => Ok(best),
    }
}

/// Expand a `checksum_asset` pattern for a chosen asset.
pub fn expand_checksum_pattern(
    pattern: &str,
    asset_name: &str,
    version: &str,
    tag: &str,
    platform: &Platform,
) -> String {
    pattern
        .replace("{asset}", asset_name)
        .replace("{version}", version)
        .replace("{tag}", tag)
        .replace("{os}", platform.os.as_str())
        .replace("{arch}", platform.arch.as_str())
}

/// Find the SHA-256 for `asset_name` inside a checksum file.
///
/// Understands `sha256sum` output (`hex  name` / `hex *name`), BSD `SHA256 (name) = hex`,
/// `name: hex`, certutil output, and files containing a single bare digest.
pub fn parse_checksum_file(text: &str, asset_name: &str) -> Option<String> {
    let mut only_hashes: Vec<String> = Vec::new();
    let lower_asset = asset_name.to_ascii_lowercase();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let tokens: Vec<&str> = line
            .split(|c: char| {
                c.is_whitespace() || c == '(' || c == ')' || c == '=' || c == ':' || c == '*'
            })
            .filter(|t| !t.is_empty())
            .collect();
        let hashes: Vec<&str> = tokens.iter().copied().filter(|t| is_hex64(t)).collect();
        let names: Vec<String> = tokens
            .iter()
            .filter(|t| !is_hex64(t))
            .map(|t| {
                t.rsplit(['/', '\\'])
                    .next()
                    .unwrap_or(t)
                    .to_ascii_lowercase()
            })
            .collect();
        if let Some(h) = hashes.first() {
            if names.iter().any(|n| n == &lower_asset) {
                return Some(h.to_ascii_lowercase());
            }
            only_hashes.push(h.to_ascii_lowercase());
        }
    }
    // A sidecar file for exactly this asset that only contains the digest.
    if only_hashes.len() == 1
        && !text.to_ascii_lowercase().contains(".tar")
        && !text.contains(".zip")
    {
        return only_hashes.pop();
    }
    if only_hashes.len() == 1 && text.to_ascii_lowercase().contains(&lower_asset) {
        return only_hashes.pop();
    }
    None
}

fn is_hex64(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Digests keyed by lowercase asset name, from a manifest `sha256` table.
pub fn manifest_digest(map: &BTreeMap<String, String>, asset: &str) -> Option<String> {
    map.iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(asset))
        .map(|(_, v)| v.to_ascii_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::{Arch, Os};

    fn assets(names: &[&str]) -> Vec<ReleaseAsset> {
        names
            .iter()
            .map(|n| ReleaseAsset {
                name: n.to_string(),
                size: 1,
                browser_download_url: format!("https://example.invalid/{n}"),
                digest: None,
            })
            .collect()
    }

    fn spec(asset: Option<&str>) -> GithubReleaseSpec {
        GithubReleaseSpec {
            repository: "o/r".into(),
            asset: asset.map(String::from),
            asset_patterns: BTreeMap::new(),
            checksum_asset: None,
            sha256: BTreeMap::new(),
            tag: None,
            allow_prerelease: false,
            os: vec![],
            binary: None,
            warnings: vec![],
        }
    }

    const LINUX: Platform = Platform::new(Os::Linux, Arch::X86_64);
    const LINUX_ARM: Platform = Platform::new(Os::Linux, Arch::Aarch64);
    const MAC_ARM: Platform = Platform::new(Os::Macos, Arch::Aarch64);
    const WIN: Platform = Platform::new(Os::Windows, Arch::X86_64);

    #[test]
    fn pattern_expansion_covers_targets_and_exts() {
        let names = expand_pattern("game-{version}-{target}.{ext}", &LINUX, "1.2.3", "v1.2.3");
        assert!(names.contains(&"game-1.2.3-x86_64-unknown-linux-gnu.tar.gz".to_string()));
        assert!(names.contains(&"game-1.2.3-x86_64-unknown-linux-musl.zip".to_string()));
        assert!(names.contains(&"game-1.2.3-x86_64-unknown-linux-gnu".to_string()));
        let win = expand_pattern("game-{tag}-{target}.{ext}", &WIN, "1.2.3", "v1.2.3");
        assert!(win.contains(&"game-v1.2.3-x86_64-pc-windows-msvc.zip".to_string()));
        assert!(win.contains(&"game-v1.2.3-x86_64-pc-windows-msvc.exe".to_string()));
    }

    #[test]
    fn real_world_patterns_select_correctly() {
        // chess-tui: tag without v, tar.gz everywhere incl. windows
        let chess = assets(&[
            "chess-tui-2.7.1-aarch64-apple-darwin.tar.gz",
            "chess-tui-2.7.1-aarch64-unknown-linux-gnu.tar.gz",
            "chess-tui-2.7.1-x86_64-apple-darwin.tar.gz",
            "chess-tui-2.7.1-x86_64-pc-windows-msvc.tar.gz",
            "chess-tui-2.7.1-x86_64-unknown-linux-gnu.tar.gz",
            "chess-tui_2.7.1-1_amd64.deb",
        ]);
        let s = spec(Some("chess-tui-{version}-{target}.tar.gz"));
        assert_eq!(
            select_asset(&chess, &s, &LINUX, "2.7.1", "2.7.1")
                .unwrap()
                .name,
            "chess-tui-2.7.1-x86_64-unknown-linux-gnu.tar.gz"
        );
        assert_eq!(
            select_asset(&chess, &s, &WIN, "2.7.1", "2.7.1")
                .unwrap()
                .name,
            "chess-tui-2.7.1-x86_64-pc-windows-msvc.tar.gz"
        );
        assert_eq!(
            select_asset(&chess, &s, &MAC_ARM, "2.7.1", "2.7.1")
                .unwrap()
                .name,
            "chess-tui-2.7.1-aarch64-apple-darwin.tar.gz"
        );
        assert!(matches!(
            select_asset(
                &chess,
                &s,
                &Platform::new(Os::Windows, Arch::Aarch64),
                "2.7.1",
                "2.7.1"
            ),
            Err(InstallError::NoMatchingAsset { .. })
        ));

        // tetro-tui: tag with v, zip on windows
        let tetro = assets(&[
            "tetro-tui-v3.6.2-aarch64-apple-darwin.tar.gz",
            "tetro-tui-v3.6.2-armv7-unknown-linux-gnueabihf.tar.gz",
            "tetro-tui-v3.6.2-x86_64-pc-windows-msvc.zip",
            "tetro-tui-v3.6.2-x86_64-unknown-linux-gnu.tar.gz",
            "tetro-tui-v3.6.2-x86_64-unknown-linux-musl.tar.gz",
        ]);
        let s = spec(Some("tetro-tui-{tag}-{target}.{ext}"));
        assert_eq!(
            select_asset(&tetro, &s, &LINUX, "3.6.2", "v3.6.2")
                .unwrap()
                .name,
            "tetro-tui-v3.6.2-x86_64-unknown-linux-gnu.tar.gz"
        );
        assert_eq!(
            select_asset(&tetro, &s, &WIN, "3.6.2", "v3.6.2")
                .unwrap()
                .name,
            "tetro-tui-v3.6.2-x86_64-pc-windows-msvc.zip"
        );

        // termfarm: per-platform raw binaries
        let farm = assets(&[
            "termfarm_2.2.0_checksums.txt",
            "termfarm_Linux_arm64",
            "termfarm_Linux_x86_64",
            "termfarm_Windows_x86_64.exe",
        ]);
        let mut s = spec(None);
        s.asset_patterns
            .insert("linux-x86_64".into(), "termfarm_Linux_x86_64".into());
        s.asset_patterns
            .insert("linux-aarch64".into(), "termfarm_Linux_arm64".into());
        s.asset_patterns.insert(
            "windows-x86_64".into(),
            "termfarm_Windows_x86_64.exe".into(),
        );
        assert_eq!(
            select_asset(&farm, &s, &LINUX, "2.2.0", "2.2.0")
                .unwrap()
                .name,
            "termfarm_Linux_x86_64"
        );
        assert_eq!(
            select_asset(&farm, &s, &LINUX_ARM, "2.2.0", "2.2.0")
                .unwrap()
                .name,
            "termfarm_Linux_arm64"
        );
        assert_eq!(
            select_asset(&farm, &s, &WIN, "2.2.0", "2.2.0")
                .unwrap()
                .name,
            "termfarm_Windows_x86_64.exe"
        );
        // no pattern for macOS → heuristic finds nothing
        assert!(matches!(
            select_asset(&farm, &s, &MAC_ARM, "2.2.0", "2.2.0"),
            Err(InstallError::NoMatchingAsset { .. })
        ));

        // tmaze: {version} in raw names
        let tmaze = assets(&[
            "tmaze_linux-aarch64_1.18.0",
            "tmaze_linux-x86_64_1.18.0",
            "tmaze_windows-x86_64_1.18.0.exe",
            "tmaze_x86_64_1.18.0.deb",
        ]);
        let mut s = spec(None);
        s.asset_patterns
            .insert("linux-x86_64".into(), "tmaze_linux-x86_64_{version}".into());
        assert_eq!(
            select_asset(&tmaze, &s, &LINUX, "1.18.0", "1.18.0")
                .unwrap()
                .name,
            "tmaze_linux-x86_64_1.18.0"
        );
    }

    #[test]
    fn heuristic_selection() {
        let hammurabi = assets(&[
            "hammurabi-linux-amd64.tar.gz",
            "hammurabi-macos-aarch64.tar.gz",
            "hammurabi-macos-amd64.tar.gz",
            "hammurabi-windows-amd64.zip",
        ]);
        let s = spec(None);
        assert_eq!(
            select_asset(&hammurabi, &s, &LINUX, "0.1.2", "v0.1.2")
                .unwrap()
                .name,
            "hammurabi-linux-amd64.tar.gz"
        );
        assert_eq!(
            select_asset(&hammurabi, &s, &MAC_ARM, "0.1.2", "v0.1.2")
                .unwrap()
                .name,
            "hammurabi-macos-aarch64.tar.gz"
        );
        assert_eq!(
            select_asset(&hammurabi, &s, &WIN, "0.1.2", "v0.1.2")
                .unwrap()
                .name,
            "hammurabi-windows-amd64.zip"
        );
        assert!(matches!(
            select_asset(&hammurabi, &s, &LINUX_ARM, "0.1.2", "v0.1.2"),
            Err(InstallError::NoMatchingAsset { .. })
        ));

        // cargo-dist layout with checksums, installers and source tarballs
        let poker = assets(&[
            "dist-manifest.json",
            "sha256.sum",
            "source.tar.gz",
            "source.tar.gz.sha256",
            "terminal-poker-aarch64-apple-darwin.tar.xz",
            "terminal-poker-aarch64-apple-darwin.tar.xz.sha256",
            "terminal-poker-installer.sh",
            "terminal-poker-installer.ps1",
            "terminal-poker-x86_64-pc-windows-msvc.zip",
            "terminal-poker-x86_64-pc-windows-msvc.zip.sha256",
            "terminal-poker-x86_64-unknown-linux-gnu.tar.xz",
            "terminal-poker-x86_64-unknown-linux-gnu.tar.xz.sha256",
        ]);
        assert_eq!(
            select_asset(&poker, &s, &LINUX, "1.0.1", "v1.0.1")
                .unwrap()
                .name,
            "terminal-poker-x86_64-unknown-linux-gnu.tar.xz"
        );
        assert_eq!(
            select_asset(&poker, &s, &WIN, "1.0.1", "v1.0.1")
                .unwrap()
                .name,
            "terminal-poker-x86_64-pc-windows-msvc.zip"
        );
        assert_eq!(
            select_asset(&poker, &s, &MAC_ARM, "1.0.1", "v1.0.1")
                .unwrap()
                .name,
            "terminal-poker-aarch64-apple-darwin.tar.xz"
        );

        // musl preferred over gnu when both exist
        let both = assets(&[
            "app-x86_64-unknown-linux-gnu.tar.gz",
            "app-x86_64-unknown-linux-musl.tar.gz",
        ]);
        assert_eq!(
            select_asset(&both, &s, &LINUX, "1", "1").unwrap().name,
            "app-x86_64-unknown-linux-musl.tar.gz"
        );

        // arch-less asset accepted only on x86_64 when nothing is arch-tagged
        let archless = assets(&["app-linux.tar.gz", "app-macos.tar.gz"]);
        assert_eq!(
            select_asset(&archless, &s, &LINUX, "1", "1").unwrap().name,
            "app-linux.tar.gz"
        );
        assert!(select_asset(&archless, &s, &LINUX_ARM, "1", "1").is_err());

        // ambiguity
        let amb = assets(&["app-linux-x86_64-a.tar.gz", "app-linux-x86_64-b.tar.gz"]);
        assert!(matches!(
            select_asset(&amb, &s, &LINUX, "1", "1"),
            Err(InstallError::AmbiguousAsset { .. })
        ));

        // windows raw exe accepted, unix raw binary accepted
        let raw = assets(&["game-windows-x64.exe", "game-linux-x86_64"]);
        assert_eq!(
            select_asset(&raw, &s, &WIN, "1", "1").unwrap().name,
            "game-windows-x64.exe"
        );
        assert_eq!(
            select_asset(&raw, &s, &LINUX, "1", "1").unwrap().name,
            "game-linux-x86_64"
        );
    }

    #[test]
    fn checksum_file_formats() {
        let h = "b4bff6541dea5fdd00fe3187b5c6995a367b921ae912b0eb6b85fa487f27cb99";
        let gnu = format!(
            "{h}  hammurabi-linux-amd64.tar.gz\n{}  other.tar.gz\n",
            "c".repeat(64)
        );
        assert_eq!(
            parse_checksum_file(&gnu, "hammurabi-linux-amd64.tar.gz").as_deref(),
            Some(h)
        );
        assert_eq!(parse_checksum_file(&gnu, "missing").as_deref(), None);
        let binary_mode = format!("{h} *terminal-poker-x86_64-unknown-linux-gnu.tar.xz\n");
        assert_eq!(
            parse_checksum_file(
                &binary_mode,
                "terminal-poker-x86_64-unknown-linux-gnu.tar.xz"
            )
            .as_deref(),
            Some(h)
        );
        let bsd = format!("SHA256 (game.zip) = {h}\n");
        assert_eq!(parse_checksum_file(&bsd, "game.zip").as_deref(), Some(h));
        let colon = format!("game.zip: {h}\n");
        assert_eq!(parse_checksum_file(&colon, "game.zip").as_deref(), Some(h));
        let bare = format!("{}\n", h.to_uppercase());
        assert_eq!(
            parse_checksum_file(&bare, "anything.tar.gz").as_deref(),
            Some(h)
        );
        let certutil = format!(
            "SHA256 hash of termitype-v0.0.11-x86_64-pc-windows-msvc.zip:\n{h}\nCertUtil: -hashfile command completed successfully.\n"
        );
        assert_eq!(
            parse_checksum_file(&certutil, "termitype-v0.0.11-x86_64-pc-windows-msvc.zip")
                .as_deref(),
            Some(h)
        );
        let with_path = format!("{h}  ./dist/game.zip\n");
        assert_eq!(
            parse_checksum_file(&with_path, "game.zip").as_deref(),
            Some(h)
        );
        assert_eq!(
            expand_checksum_pattern("{asset}.sha256", "a.tar.gz", "1", "v1", &LINUX),
            "a.tar.gz.sha256"
        );
        assert_eq!(
            expand_checksum_pattern("app_{version}_checksums.txt", "a", "2.2.0", "2.2.0", &LINUX),
            "app_2.2.0_checksums.txt"
        );
        let mut map = BTreeMap::new();
        map.insert("Game.zip".to_string(), h.to_uppercase());
        assert_eq!(manifest_digest(&map, "game.zip").as_deref(), Some(h));
    }
}
