//! Operating system / architecture detection and external tool discovery.

use std::fmt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::error::PlatformError;

/// Supported operating systems.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Os {
    Linux,
    Macos,
    Windows,
}

impl Os {
    pub const ALL: [Os; 3] = [Os::Linux, Os::Macos, Os::Windows];

    pub fn current() -> Result<Os, PlatformError> {
        match std::env::consts::OS {
            "linux" => Ok(Os::Linux),
            "macos" => Ok(Os::Macos),
            "windows" => Ok(Os::Windows),
            other => Err(PlatformError::UnsupportedOs(other.to_string())),
        }
    }

    /// Manifest spelling.
    pub fn as_str(self) -> &'static str {
        match self {
            Os::Linux => "linux",
            Os::Macos => "macos",
            Os::Windows => "windows",
        }
    }

    /// Human label.
    pub fn label(self) -> &'static str {
        match self {
            Os::Linux => "Linux",
            Os::Macos => "macOS",
            Os::Windows => "Windows",
        }
    }

    /// Tokens commonly found in release asset names for this OS.
    pub fn asset_tokens(self) -> &'static [&'static str] {
        match self {
            Os::Linux => &["linux"],
            Os::Macos => &["macos", "darwin", "apple", "osx", "mac"],
            Os::Windows => &["windows", "win64", "win32", "win"],
        }
    }
}

impl fmt::Display for Os {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Os {
    type Err = PlatformError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "linux" => Ok(Os::Linux),
            "macos" | "darwin" | "osx" => Ok(Os::Macos),
            "windows" | "win" => Ok(Os::Windows),
            other => Err(PlatformError::UnsupportedOs(other.to_string())),
        }
    }
}

/// Supported CPU architectures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Arch {
    #[serde(rename = "x86_64")]
    X86_64,
    #[serde(rename = "aarch64")]
    Aarch64,
}

impl Arch {
    pub const ALL: [Arch; 2] = [Arch::X86_64, Arch::Aarch64];

    pub fn current() -> Result<Arch, PlatformError> {
        match std::env::consts::ARCH {
            "x86_64" => Ok(Arch::X86_64),
            "aarch64" => Ok(Arch::Aarch64),
            other => Err(PlatformError::UnsupportedArch(other.to_string())),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Arch::X86_64 => "x86_64",
            Arch::Aarch64 => "aarch64",
        }
    }

    /// Tokens commonly found in release asset names for this architecture.
    pub fn asset_tokens(self) -> &'static [&'static str] {
        match self {
            Arch::X86_64 => &["x86_64", "amd64", "x64", "x86-64"],
            Arch::Aarch64 => &["aarch64", "arm64"],
        }
    }

    /// Tokens that identify *other* architectures (used to reject assets).
    pub fn foreign_tokens(self) -> &'static [&'static str] {
        match self {
            Arch::X86_64 => &[
                "aarch64", "arm64", "armv7", "armv6", "i686", "i386", "riscv", "powerpc", "s390x",
                "x86-32",
            ],
            Arch::Aarch64 => &[
                "x86_64", "amd64", "x64", "armv7", "armv6", "i686", "i386", "riscv", "powerpc",
                "s390x",
            ],
        }
    }
}

impl fmt::Display for Arch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Arch {
    type Err = PlatformError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "x86_64" | "amd64" | "x64" => Ok(Arch::X86_64),
            "aarch64" | "arm64" => Ok(Arch::Aarch64),
            other => Err(PlatformError::UnsupportedArch(other.to_string())),
        }
    }
}

/// The platform RustArcade is running on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Platform {
    pub os: Os,
    pub arch: Arch,
}

impl Platform {
    pub fn current() -> Result<Platform, PlatformError> {
        Ok(Platform {
            os: Os::current()?,
            arch: Arch::current()?,
        })
    }

    pub const fn new(os: Os, arch: Arch) -> Platform {
        Platform { os, arch }
    }

    /// `linux-x86_64` style key used by manifests (`asset_patterns`).
    pub fn key(&self) -> String {
        format!("{}-{}", self.os, self.arch)
    }

    /// Rust target triples this platform can run, most preferred first.
    pub fn target_triples(&self) -> Vec<String> {
        let arch = self.arch.as_str();
        match self.os {
            Os::Linux => vec![
                format!("{arch}-unknown-linux-gnu"),
                format!("{arch}-unknown-linux-musl"),
            ],
            Os::Macos => vec![format!("{arch}-apple-darwin")],
            Os::Windows => vec![
                format!("{arch}-pc-windows-msvc"),
                format!("{arch}-pc-windows-gnu"),
            ],
        }
    }

    /// Executable suffix (`.exe` on Windows).
    pub fn exe_suffix(&self) -> &'static str {
        match self.os {
            Os::Windows => ".exe",
            _ => "",
        }
    }

    /// `name` with the platform executable suffix.
    pub fn exe_name(&self, base: &str) -> String {
        format!("{base}{}", self.exe_suffix())
    }
}

impl fmt::Display for Platform {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.os.label(), self.arch)
    }
}

/// Location and version of an external tool.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolInfo {
    pub name: String,
    pub path: Option<PathBuf>,
    pub version: Option<String>,
}

impl ToolInfo {
    pub fn missing(name: &str) -> Self {
        Self {
            name: name.to_string(),
            path: None,
            version: None,
        }
    }

    pub fn at(name: &str, path: PathBuf) -> Self {
        let version = probe_version(&path);
        Self {
            name: name.to_string(),
            path: Some(path),
            version,
        }
    }

    pub fn available(&self) -> bool {
        self.path.is_some()
    }
}

/// External tools RustArcade may invoke.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tools {
    pub cargo: ToolInfo,
    pub git: ToolInfo,
    pub rustc: ToolInfo,
}

impl Tools {
    /// Detect tools using `RUSTARCADE_CARGO` / `RUSTARCADE_GIT` overrides, then `PATH`,
    /// then `~/.cargo/bin`.
    pub fn detect() -> Tools {
        let cargo_override = std::env::var_os("RUSTARCADE_CARGO").map(PathBuf::from);
        let git_override = std::env::var_os("RUSTARCADE_GIT").map(PathBuf::from);
        Self::detect_with(cargo_override, git_override)
    }

    /// Detect tools with explicit overrides (tests pass fixture binaries here).
    pub fn detect_with(cargo: Option<PathBuf>, git: Option<PathBuf>) -> Tools {
        let cargo = cargo
            .filter(|p| p.is_file())
            .or_else(|| find_tool("cargo"))
            .map(|p| ToolInfo::at("cargo", p))
            .unwrap_or_else(|| ToolInfo::missing("cargo"));
        let git = git
            .filter(|p| p.is_file())
            .or_else(|| find_tool("git"))
            .map(|p| ToolInfo::at("git", p))
            .unwrap_or_else(|| ToolInfo::missing("git"));
        let rustc = find_tool("rustc")
            .map(|p| ToolInfo::at("rustc", p))
            .unwrap_or_else(|| ToolInfo::missing("rustc"));
        Tools { cargo, git, rustc }
    }

    /// Tools with nothing detected (used by tests that must not touch the system).
    pub fn none() -> Tools {
        Tools {
            cargo: ToolInfo::missing("cargo"),
            git: ToolInfo::missing("git"),
            rustc: ToolInfo::missing("rustc"),
        }
    }
}

fn find_tool(name: &str) -> Option<PathBuf> {
    if let Ok(path) = which::which(name) {
        return Some(path);
    }
    let cargo_home = std::env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::home_dir().map(|h| h.join(".cargo")))?;
    let exe = if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_string()
    };
    let candidate = cargo_home.join("bin").join(exe);
    candidate.is_file().then_some(candidate)
}

fn probe_version(path: &Path) -> Option<String> {
    let output = Command::new(path)
        .arg("--version")
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    text.lines()
        .next()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
}

/// Facts about the current terminal, for `doctor` and the settings screen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalInfo {
    pub term: Option<String>,
    pub color_term: Option<String>,
    pub term_program: Option<String>,
    pub is_tty: bool,
    pub size: Option<(u16, u16)>,
}

impl TerminalInfo {
    pub fn detect() -> TerminalInfo {
        use std::io::IsTerminal;
        TerminalInfo {
            term: std::env::var("TERM").ok().filter(|s| !s.is_empty()),
            color_term: std::env::var("COLORTERM").ok().filter(|s| !s.is_empty()),
            term_program: std::env::var("TERM_PROGRAM").ok().filter(|s| !s.is_empty()),
            is_tty: std::io::stdout().is_terminal(),
            size: crossterm::terminal::size().ok(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn os_and_arch_parse_aliases() {
        assert_eq!("darwin".parse::<Os>().unwrap(), Os::Macos);
        assert_eq!("Windows".parse::<Os>().unwrap(), Os::Windows);
        assert!("plan9".parse::<Os>().is_err());
        assert_eq!("amd64".parse::<Arch>().unwrap(), Arch::X86_64);
        assert_eq!("arm64".parse::<Arch>().unwrap(), Arch::Aarch64);
        assert!("mips".parse::<Arch>().is_err());
    }

    #[test]
    fn serde_spelling_matches_manifests() {
        assert_eq!(serde_json::to_string(&Os::Macos).unwrap(), "\"macos\"");
        assert_eq!(serde_json::to_string(&Arch::X86_64).unwrap(), "\"x86_64\"");
        assert_eq!(
            serde_json::from_str::<Arch>("\"aarch64\"").unwrap(),
            Arch::Aarch64
        );
    }

    #[test]
    fn target_triples_and_keys() {
        let p = Platform::new(Os::Linux, Arch::X86_64);
        assert_eq!(p.key(), "linux-x86_64");
        assert_eq!(p.target_triples()[0], "x86_64-unknown-linux-gnu");
        assert_eq!(p.exe_name("game"), "game");
        let w = Platform::new(Os::Windows, Arch::Aarch64);
        assert_eq!(w.exe_name("game"), "game.exe");
        assert_eq!(w.target_triples()[0], "aarch64-pc-windows-msvc");
        assert_eq!(
            Platform::new(Os::Macos, Arch::Aarch64).target_triples(),
            vec!["aarch64-apple-darwin"]
        );
    }

    #[test]
    fn current_platform_is_supported_here() {
        let p = Platform::current().unwrap();
        assert!(!p.target_triples().is_empty());
    }

    #[test]
    fn tools_none_reports_nothing() {
        let t = Tools::none();
        assert!(!t.cargo.available());
        assert!(!t.git.available());
    }

    #[test]
    fn tool_override_must_exist() {
        let t = Tools::detect_with(Some(PathBuf::from("/definitely/not/here/cargo")), None);
        // Falls back to detection rather than trusting a bogus override.
        assert!(t.cargo.path.as_deref() != Some(Path::new("/definitely/not/here/cargo")));
    }
}
