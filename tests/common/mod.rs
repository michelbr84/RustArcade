//! Shared helpers for integration tests: compiled fixtures, isolated homes, mock servers.
#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, OnceLock};

use rustarcade::net::Endpoints;
use rustarcade::platform::Tools;
use rustarcade::services::{OpenOptions, Services};

/// Compile `tests/fixtures/fixture_bin.rs` once per test binary and return its path.
pub fn fixture_bin() -> PathBuf {
    static BIN: OnceLock<PathBuf> = OnceLock::new();
    BIN.get_or_init(|| {
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fixture_bin.rs");
        let out_dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("fixtures");
        fs::create_dir_all(&out_dir).expect("create fixture dir");
        let exe = out_dir.join(format!("fixture_bin{}", std::env::consts::EXE_SUFFIX));
        if exe.is_file()
            && fs::metadata(&exe).and_then(|m| m.modified()).ok()
                >= fs::metadata(&src).and_then(|m| m.modified()).ok()
        {
            return exe;
        }
        let tmp = out_dir.join(format!(
            "fixture_bin-{}{}",
            std::process::id(),
            std::env::consts::EXE_SUFFIX
        ));
        let status = Command::new("rustc")
            .args(["-O", "--edition", "2021", "--crate-name", "fixture_bin"])
            .arg(&src)
            .arg("-o")
            .arg(&tmp)
            .status()
            .expect("run rustc");
        assert!(status.success(), "rustc failed to build the fixture");
        if fs::rename(&tmp, &exe).is_err() && !exe.is_file() {
            fs::copy(&tmp, &exe).expect("copy fixture");
        }
        let _ = fs::remove_file(&tmp);
        exe
    })
    .clone()
}

/// An isolated RustArcade home with a fixture catalog and a fake `cargo`.
pub struct TestEnv {
    pub root: tempfile::TempDir,
    pub catalog_dir: PathBuf,
    pub fake_cargo: PathBuf,
    pub endpoints: Endpoints,
}

impl TestEnv {
    pub fn new() -> TestEnv {
        TestEnv::with_endpoints(Endpoints::default())
    }

    /// Point GitHub/crates.io at a mock server.
    pub fn with_endpoints(endpoints: Endpoints) -> TestEnv {
        let root = tempfile::tempdir().expect("tempdir");
        let tools_dir = root.path().join("tools");
        fs::create_dir_all(&tools_dir).unwrap();
        let fake_cargo = tools_dir.join(format!("cargo{}", std::env::consts::EXE_SUFFIX));
        fs::copy(fixture_bin(), &fake_cargo).expect("copy fake cargo");
        let catalog_dir = root.path().join("catalog");
        fs::create_dir_all(&catalog_dir).unwrap();
        let env = TestEnv {
            root,
            catalog_dir,
            fake_cargo,
            endpoints,
        };
        env.write_manifest(
            "fixture-game",
            &cargo_manifest("fixture-game", "fixture-game", &[]),
        );
        env.write_manifest(
            "fixture-exit3",
            &cargo_manifest("fixture-exit3", "fixture-exit3", &["--exit", "3"]),
        );
        env.write_manifest(
            "fixture-fail",
            &cargo_manifest("fixture-fail", "fixture-fail", &[]),
        );
        env.write_manifest(
            "fixture-slow",
            &cargo_manifest("fixture-slow", "fixture-slow", &[]),
        );
        env
    }

    pub fn home(&self) -> PathBuf {
        self.root.path().join("home")
    }

    pub fn write_manifest(&self, id: &str, text: &str) {
        fs::write(self.catalog_dir.join(format!("{id}.toml")), text).expect("write manifest");
    }

    /// Set the version the fake cargo reports (`fail` makes it fail).
    pub fn set_fake_version(&self, version: &str) {
        fs::write(self.fake_cargo.with_file_name("fixture-version"), version).unwrap();
    }

    pub fn open(&self) -> Arc<Services> {
        self.open_with(Tools::detect_with(Some(self.fake_cargo.clone()), None))
    }

    /// Open with the real toolchain (git-build tests).
    pub fn open_with_real_tools(&self) -> Arc<Services> {
        self.open_with(Tools::detect())
    }

    pub fn open_with(&self, tools: Tools) -> Arc<Services> {
        Services::open(OpenOptions {
            root: Some(self.home()),
            catalog_dir: Some(self.catalog_dir.clone()),
            use_builtin_catalog: Some(false),
            tools: Some(tools),
            endpoints: Some(self.endpoints.clone()),
            allow_insecure_local: Some(true),
            offline: false,
        })
        .expect("open services")
    }

    /// Register a crates.io mock for `krate` returning `version`.
    pub fn mock_crate<'a>(
        server: &'a httpmock::MockServer,
        krate: &str,
        version: &str,
    ) -> httpmock::Mock<'a> {
        let path = format!("/api/v1/crates/{krate}");
        let body = serde_json::json!({"crate": {"max_stable_version": version, "newest_version": version}});
        server.mock(move |when, then| {
            when.method(httpmock::Method::GET).path(path.clone());
            then.status(200).json_body(body.clone());
        })
    }

    /// Endpoints pointing both APIs at `server`.
    pub fn endpoints_for(server: &httpmock::MockServer) -> Endpoints {
        Endpoints {
            github_api: server.base_url(),
            crates_io: server.base_url(),
        }
    }

    /// The `rustarcade` binary configured for this environment.
    pub fn cmd(&self) -> assert_cmd::Command {
        let mut cmd = assert_cmd::cargo::cargo_bin_cmd!("rustarcade");
        cmd.env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .env("HOME", self.root.path())
            .env("RUSTARCADE_HOME", self.home())
            .env("RUSTARCADE_CATALOG_DIR", &self.catalog_dir)
            .env("RUSTARCADE_BUILTIN_CATALOG", "0")
            .env("RUSTARCADE_CARGO", &self.fake_cargo)
            .env("RUSTARCADE_ALLOW_INSECURE_LOCAL", "1")
            .env("RUSTARCADE_GITHUB_API", &self.endpoints.github_api)
            .env("RUSTARCADE_CRATES_API", &self.endpoints.crates_io)
            .env("TERM", "dumb")
            .env("NO_COLOR", "1");
        if let Some(cargo_home) = std::env::var_os("CARGO_HOME") {
            cmd.env("CARGO_HOME", cargo_home);
        }
        if let Some(rustup) = std::env::var_os("RUSTUP_HOME") {
            cmd.env("RUSTUP_HOME", rustup);
        }
        cmd
    }

    pub fn runtime() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap()
    }
}

/// A manifest installed through the fake cargo.
pub fn cargo_manifest(id: &str, krate: &str, args: &[&str]) -> String {
    let args = args
        .iter()
        .map(|a| format!("\"{a}\""))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        r#"schema_version = 1
id = "{id}"
name = "Fixture {id}"
summary = "A fixture game."
repository = "https://github.com/rustarcade/fixtures"
license = "MIT"
categories = ["arcade"]
support_status = "verified"

[compatibility]
os = ["linux", "macos", "windows"]

[run]
executable = "{krate}"
args = [{args}]

[[installers]]
type = "cargo"
crate = "{krate}"
"#
    )
}

/// A manifest installed from a (mock) GitHub release.
pub fn release_manifest(
    id: &str,
    repo: &str,
    asset_pattern: &str,
    checksum_asset: Option<&str>,
) -> String {
    let checksum = checksum_asset
        .map(|c| format!("checksum_asset = \"{c}\"\n"))
        .unwrap_or_default();
    format!(
        r#"schema_version = 1
id = "{id}"
name = "Release {id}"
summary = "A release fixture."
repository = "https://github.com/{repo}"
categories = ["arcade"]

[compatibility]
os = ["linux", "macos", "windows"]

[run]
executable = "{id}"

[[installers]]
type = "github-release"
repository = "{repo}"
asset = "{asset_pattern}"
{checksum}"#
    )
}

/// A manifest built from a local git repository.
pub fn git_manifest(id: &str, url: &str, reference: Option<&str>) -> String {
    let reference = reference
        .map(|r| format!("reference = \"{r}\"\n"))
        .unwrap_or_default();
    format!(
        r#"schema_version = 1
id = "{id}"
name = "Git {id}"
summary = "A git fixture."
repository = "https://github.com/rustarcade/fixtures"
categories = ["arcade"]

[compatibility]
os = ["linux", "macos", "windows"]

[run]
executable = "hello-game"

[[installers]]
type = "git-cargo-build"
repository = "{url}"
{reference}"#
    )
}

/// Build a `.tar.gz` containing `entries` (path → bytes, executable bit set).
pub fn tar_gz(entries: &[(&str, &[u8])]) -> Vec<u8> {
    use std::io::Write;
    let mut builder = tar::Builder::new(Vec::new());
    for (name, data) in entries {
        let mut header = tar::Header::new_gnu();
        header.set_size(data.len() as u64);
        header.set_mode(0o755);
        header.set_entry_type(tar::EntryType::Regular);
        header.set_path(name).unwrap();
        header.set_cksum();
        builder.append(&header, *data).unwrap();
    }
    let tar = builder.into_inner().unwrap();
    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    gz.write_all(&tar).unwrap();
    gz.finish().unwrap()
}

/// Build a `.zip` containing `entries`.
pub fn zip_bytes(entries: &[(&str, &[u8])]) -> Vec<u8> {
    use std::io::Write;
    let mut cursor = std::io::Cursor::new(Vec::new());
    {
        let mut zw = zip::ZipWriter::new(&mut cursor);
        let opts = zip::write::SimpleFileOptions::default().unix_permissions(0o755);
        for (name, data) in entries {
            zw.start_file(*name, opts).unwrap();
            zw.write_all(data).unwrap();
        }
        zw.finish().unwrap();
    }
    cursor.into_inner()
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    rustarcade::catalog::index::sha256_hex(bytes)
}

/// Endpoints for release tests (GitHub API and downloads on the same mock server).
pub fn endpoints_for_release(server: &httpmock::MockServer) -> Endpoints {
    TestEnv::endpoints_for(server)
}

/// Current platform's first target triple and archive extension.
pub fn platform_target() -> (String, &'static str) {
    let p = rustarcade::platform::Platform::current().unwrap();
    let triple = p.target_triples().remove(0);
    let ext = if cfg!(windows) { "zip" } else { "tar.gz" };
    (triple, ext)
}
