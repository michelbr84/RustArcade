//! Lenient version parsing and comparison for upstream tags, crates and installs.

use semver::Version;

/// Parse a version string leniently: strips a leading `v`/`V`, tolerates `1.2` / `1`,
/// keeps pre-release and build metadata.
pub fn lenient_parse(raw: &str) -> Option<Version> {
    let mut s = raw.trim();
    if let Some(rest) = s.strip_prefix(['v', 'V']) {
        s = rest;
    }
    if s.is_empty() {
        return None;
    }
    if let Ok(v) = Version::parse(s) {
        return Some(v);
    }
    // Split off pre-release / build metadata to pad the numeric core.
    let (core, suffix) = match s.find(['-', '+']) {
        Some(idx) => (&s[..idx], &s[idx..]),
        None => (s, ""),
    };
    let parts: Vec<&str> = core.split('.').collect();
    if parts.is_empty()
        || parts.len() > 3
        || parts
            .iter()
            .any(|p| p.is_empty() || !p.chars().all(|c| c.is_ascii_digit()))
    {
        return None;
    }
    // Drop leading zeros (`2026.01.14` → `2026.1.14`); semver forbids them.
    let mut padded = parts
        .iter()
        .map(|p| p.parse::<u64>().map(|n| n.to_string()))
        .collect::<Result<Vec<_>, _>>()
        .ok()?;
    while padded.len() < 3 {
        padded.push("0".to_string());
    }
    Version::parse(&format!("{}{suffix}", padded.join("."))).ok()
}

/// Outcome of comparing an installed version with the latest available one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Comparison {
    Newer,
    Same,
    Older,
    Unknown,
}

/// Compare `installed` with `latest`. Non-semver strings are compared for equality only.
pub fn compare(installed: &str, latest: &str) -> Comparison {
    match (lenient_parse(installed), lenient_parse(latest)) {
        (Some(mut a), Some(mut b)) => {
            // Build metadata never decides whether an update exists.
            a.build = semver::BuildMetadata::EMPTY;
            b.build = semver::BuildMetadata::EMPTY;
            if b > a {
                Comparison::Newer
            } else if b == a {
                Comparison::Same
            } else {
                Comparison::Older
            }
        }
        _ => {
            if installed.trim().trim_start_matches(['v', 'V'])
                == latest.trim().trim_start_matches(['v', 'V'])
            {
                Comparison::Same
            } else {
                Comparison::Unknown
            }
        }
    }
}

/// True when `latest` is strictly newer than `installed`.
pub fn is_newer(installed: &str, latest: &str) -> bool {
    compare(installed, latest) == Comparison::Newer
}

/// Normalize a tag/version for display (`v1.2.3` → `1.2.3`).
pub fn display(raw: &str) -> String {
    lenient_parse(raw)
        .map(|v| v.to_string())
        .unwrap_or_else(|| raw.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lenient_strips_prefix_and_pads() {
        assert_eq!(lenient_parse("v1.2.3").unwrap(), Version::new(1, 2, 3));
        assert_eq!(lenient_parse("1.2").unwrap(), Version::new(1, 2, 0));
        assert_eq!(lenient_parse("3").unwrap(), Version::new(3, 0, 0));
        assert_eq!(lenient_parse(" V0.0.11 ").unwrap(), Version::new(0, 0, 11));
        assert_eq!(lenient_parse("1.2-rc.1").unwrap().to_string(), "1.2.0-rc.1");
        assert_eq!(
            lenient_parse("2026.01.14").unwrap(),
            Version::new(2026, 1, 14)
        );
    }

    #[test]
    fn lenient_rejects_garbage() {
        for bad in ["", "v", "abc", "1.2.3.4", "1..2", "release-1", "-1"] {
            assert!(lenient_parse(bad).is_none(), "{bad:?}");
        }
    }

    #[test]
    fn comparison_semantics() {
        assert_eq!(compare("1.0.0", "v1.1.0"), Comparison::Newer);
        assert_eq!(compare("1.1.0", "1.1.0"), Comparison::Same);
        assert_eq!(compare("1.2.0", "1.1.0"), Comparison::Older);
        assert_eq!(compare("1.0.0-rc.1", "1.0.0"), Comparison::Newer);
        assert_eq!(compare("abc", "abc"), Comparison::Same);
        assert_eq!(compare("abc", "def"), Comparison::Unknown);
        assert!(is_newer("0.1.0", "0.2.0"));
        assert!(!is_newer("0.2.0", "0.1.0"));
        assert_eq!(compare("1.0.0+build1", "1.0.0+build2"), Comparison::Same);
    }

    #[test]
    fn display_normalizes() {
        assert_eq!(display("v1.2"), "1.2.0");
        assert_eq!(display("2026.06.19-b"), "2026.6.19-b");
        assert_eq!(display("weird"), "weird");
    }
}
