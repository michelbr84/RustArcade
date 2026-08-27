//! GitHub release installer against a mock GitHub API.

mod common;

use common::{
    TestEnv, endpoints_for_release, fixture_bin, platform_target, release_manifest, sha256_hex,
    tar_gz, zip_bytes,
};
use httpmock::Method::GET;
use httpmock::MockServer;
use rustarcade::catalog::manifest::InstallerKind;
use rustarcade::install::{ChecksumPolicy, ProgressSink};
use rustarcade::launcher::NoTerminalSession;
use rustarcade::registry::InstallSource;
use rustarcade::services::GameState;
use tokio_util::sync::CancellationToken;

struct ReleaseFixture {
    asset_name: String,
    bytes: Vec<u8>,
}

fn archive_for(id: &str) -> ReleaseFixture {
    let (triple, ext) = platform_target();
    let exe = format!("{id}{}", std::env::consts::EXE_SUFFIX);
    let inner = format!("{id}-1.2.0/{exe}");
    let bin = std::fs::read(fixture_bin()).unwrap();
    let bytes = if ext == "zip" {
        zip_bytes(&[(&inner, &bin)])
    } else {
        tar_gz(&[(&inner, &bin)])
    };
    ReleaseFixture {
        asset_name: format!("{id}-{triple}.{ext}"),
        bytes,
    }
}

/// Serve a release with one asset (+ optional sidecar checksum / API digest).
fn mock_release(
    server: &MockServer,
    repo: &str,
    tag: &str,
    fixture: &ReleaseFixture,
    digest: Option<String>,
    sidecar: Option<String>,
    prerelease: bool,
) {
    let mut assets = vec![serde_json::json!({
        "name": fixture.asset_name,
        "size": fixture.bytes.len(),
        "browser_download_url": server.url(format!("/download/{}", fixture.asset_name)),
        "digest": digest,
    })];
    if sidecar.is_some() {
        assets.push(serde_json::json!({
            "name": format!("{}.sha256", fixture.asset_name),
            "size": 100,
            "browser_download_url": server.url(format!("/download/{}.sha256", fixture.asset_name)),
        }));
    }
    let release = serde_json::json!({"tag_name": tag, "prerelease": prerelease, "draft": false, "assets": assets});
    let repo_path = format!("/repos/{repo}/releases/latest");
    let list_path = format!("/repos/{repo}/releases");
    let r1 = release.clone();
    if prerelease {
        server.mock(move |when, then| {
            when.method(GET).path(repo_path.clone());
            then.status(404).body("{}");
        });
    } else {
        server.mock(move |when, then| {
            when.method(GET).path(repo_path.clone());
            then.status(200).json_body(r1.clone());
        });
    }
    let r2 = release.clone();
    server.mock(move |when, then| {
        when.method(GET).path(list_path.clone());
        then.status(200).json_body(serde_json::json!([r2.clone()]));
    });
    let bytes = fixture.bytes.clone();
    let name = fixture.asset_name.clone();
    server.mock(move |when, then| {
        when.method(GET).path(format!("/download/{name}"));
        then.status(200).body(bytes.clone());
    });
    if let Some(text) = sidecar {
        let name = fixture.asset_name.clone();
        server.mock(move |when, then| {
            when.method(GET).path(format!("/download/{name}.sha256"));
            then.status(200).body(text.clone());
        });
    }
}

#[test]
fn installs_from_release_with_sidecar_checksum_and_plays() {
    let server = MockServer::start();
    let fixture = archive_for("release-game");
    let sidecar = format!("{}  {}\n", sha256_hex(&fixture.bytes), fixture.asset_name);
    mock_release(
        &server,
        "fixtures/release-game",
        "v1.2.0",
        &fixture,
        None,
        Some(sidecar),
        false,
    );
    let env = TestEnv::with_endpoints(endpoints_for_release(&server));
    env.write_manifest(
        "release-game",
        &release_manifest(
            "release-game",
            "fixtures/release-game",
            "release-game-{target}.{ext}",
            Some("{asset}.sha256"),
        ),
    );
    let services = env.open();
    let rt = TestEnv::runtime();
    let id = services.resolve_id("release-game").unwrap();
    let plan = rt.block_on(services.plan_install(&id, None)).unwrap();
    assert_eq!(plan.installer, InstallerKind::GithubRelease);
    assert_eq!(plan.version.as_deref(), Some("1.2.0"));
    assert_eq!(plan.asset.as_deref(), Some(fixture.asset_name.as_str()));
    assert!(
        matches!(plan.checksum, ChecksumPolicy::ReleaseFile(_)),
        "{:?}",
        plan.checksum
    );
    assert!(!plan.compiles);
    let outcome = rt
        .block_on(services.install(plan, ProgressSink::Silent, CancellationToken::new()))
        .unwrap();
    assert_eq!(outcome.record.version, "1.2.0");
    assert!(outcome.record.checksum_verified);
    match &outcome.record.source {
        InstallSource::GithubRelease {
            tag, asset, sha256, ..
        } => {
            assert_eq!(tag, "v1.2.0");
            assert_eq!(asset, &fixture.asset_name);
            assert_eq!(sha256.as_deref(), Some(sha256_hex(&fixture.bytes).as_str()));
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(services.state_of(&id), GameState::Installed);
    assert!(
        services
            .play(&id, &mut NoTerminalSession)
            .unwrap()
            .exit
            .success()
    );
    // Downloads are cleaned up and the extraction directory is gone.
    assert!(
        !services
            .paths()
            .downloads_dir()
            .join("release-game")
            .exists()
    );
    assert!(
        !services
            .paths()
            .game_dir("release-game")
            .join("current/extract")
            .exists()
    );
}

#[test]
fn api_digest_is_used_when_no_checksum_file() {
    let server = MockServer::start();
    let fixture = archive_for("digest-game");
    let digest = format!("sha256:{}", sha256_hex(&fixture.bytes));
    mock_release(
        &server,
        "fixtures/digest-game",
        "2.0.0",
        &fixture,
        Some(digest),
        None,
        false,
    );
    let env = TestEnv::with_endpoints(endpoints_for_release(&server));
    env.write_manifest(
        "digest-game",
        &release_manifest(
            "digest-game",
            "fixtures/digest-game",
            "digest-game-{target}.{ext}",
            None,
        ),
    );
    let services = env.open();
    let rt = TestEnv::runtime();
    let id = services.resolve_id("digest-game").unwrap();
    let plan = rt.block_on(services.plan_install(&id, None)).unwrap();
    assert_eq!(plan.checksum, ChecksumPolicy::ApiDigest);
    let outcome = rt
        .block_on(services.install(plan, ProgressSink::Silent, CancellationToken::new()))
        .unwrap();
    assert!(outcome.record.checksum_verified);
    assert_eq!(outcome.record.version, "2.0.0");
}

#[test]
fn checksum_mismatch_aborts_and_deletes_artifact() {
    let server = MockServer::start();
    let fixture = archive_for("tampered-game");
    let wrong = format!("sha256:{}", "0".repeat(64));
    mock_release(
        &server,
        "fixtures/tampered-game",
        "v1.0.0",
        &fixture,
        Some(wrong),
        None,
        false,
    );
    let env = TestEnv::with_endpoints(endpoints_for_release(&server));
    env.write_manifest(
        "tampered-game",
        &release_manifest(
            "tampered-game",
            "fixtures/tampered-game",
            "tampered-game-{target}.{ext}",
            None,
        ),
    );
    let services = env.open();
    let rt = TestEnv::runtime();
    let id = services.resolve_id("tampered-game").unwrap();
    let plan = rt.block_on(services.plan_install(&id, None)).unwrap();
    let err = rt
        .block_on(services.install(plan, ProgressSink::Silent, CancellationToken::new()))
        .unwrap_err();
    assert!(err.is_security(), "{err}");
    assert_eq!(err.exit_code(), 3);
    let msg = err.user_message();
    assert!(msg.detail.contains("checksum mismatch"), "{msg}");
    assert_eq!(services.state_of(&id), GameState::Available);
    let downloads = services.paths().downloads_dir().join("tampered-game");
    let leftover: Vec<_> = std::fs::read_dir(&downloads)
        .map(|rd| rd.flatten().collect::<Vec<_>>())
        .unwrap_or_default();
    assert!(
        leftover.is_empty(),
        "artifact must be deleted: {leftover:?}"
    );
    let staging: Vec<_> = std::fs::read_dir(services.paths().game_dir("tampered-game"))
        .map(|rd| rd.flatten().collect::<Vec<_>>())
        .unwrap_or_default();
    assert!(staging.is_empty(), "{staging:?}");
}

#[test]
fn raw_binary_asset_is_installed() {
    let server = MockServer::start();
    let p = rustarcade::platform::Platform::current().unwrap();
    let fixture = ReleaseFixture {
        asset_name: format!(
            "raw-game-{}-{}{}",
            p.os,
            p.arch,
            std::env::consts::EXE_SUFFIX
        ),
        bytes: std::fs::read(fixture_bin()).unwrap(),
    };
    mock_release(
        &server,
        "fixtures/raw-game",
        "v3.1",
        &fixture,
        None,
        None,
        false,
    );
    let env = TestEnv::with_endpoints(endpoints_for_release(&server));
    let pattern = if cfg!(windows) {
        "raw-game-{os}-{arch}.{ext}"
    } else {
        "raw-game-{os}-{arch}"
    };
    env.write_manifest(
        "raw-game",
        &release_manifest("raw-game", "fixtures/raw-game", pattern, None),
    );
    let services = env.open();
    let rt = TestEnv::runtime();
    let id = services.resolve_id("raw-game").unwrap();
    let plan = rt.block_on(services.plan_install(&id, None)).unwrap();
    assert_eq!(plan.checksum, ChecksumPolicy::None);
    assert_eq!(plan.version.as_deref(), Some("3.1.0"));
    let outcome = rt
        .block_on(services.install(plan, ProgressSink::Silent, CancellationToken::new()))
        .unwrap();
    assert!(!outcome.record.checksum_verified);
    assert!(
        services
            .play(&id, &mut NoTerminalSession)
            .unwrap()
            .exit
            .success()
    );
}

#[test]
fn update_available_when_newer_release_is_published() {
    let server = MockServer::start();
    let fixture = archive_for("update-game");
    mock_release(
        &server,
        "fixtures/update-game",
        "v1.2.0",
        &fixture,
        None,
        None,
        false,
    );
    let env = TestEnv::with_endpoints(endpoints_for_release(&server));
    env.write_manifest(
        "update-game",
        &release_manifest(
            "update-game",
            "fixtures/update-game",
            "update-game-{target}.{ext}",
            None,
        ),
    );
    let services = env.open();
    let rt = TestEnv::runtime();
    let id = services.resolve_id("update-game").unwrap();
    let plan = rt.block_on(services.plan_install(&id, None)).unwrap();
    rt.block_on(services.install(plan, ProgressSink::Silent, CancellationToken::new()))
        .unwrap();
    let report = rt.block_on(services.check_updates(None, true));
    assert!(!report.checks[0].available);

    server.reset();
    mock_release(
        &server,
        "fixtures/update-game",
        "v1.3.0",
        &fixture,
        None,
        None,
        false,
    );
    let services = env.open(); // fresh ETag cache
    let report = rt.block_on(services.check_updates(None, true));
    assert!(report.checks[0].available, "{report:?}");
    assert_eq!(report.checks[0].latest, "1.3.0");
    let plan = rt
        .block_on(services.plan_update(&id, false))
        .unwrap()
        .expect("update plan");
    assert!(plan.is_update);
    let outcome = rt
        .block_on(services.install(plan, ProgressSink::Silent, CancellationToken::new()))
        .unwrap();
    assert_eq!(outcome.record.version, "1.3.0");
    assert_eq!(outcome.previous_version.as_deref(), Some("1.2.0"));
}

#[test]
fn prerelease_only_repository_is_reported_not_installed() {
    let server = MockServer::start();
    let fixture = archive_for("pre-game");
    mock_release(
        &server,
        "fixtures/pre-game",
        "v0.9.0-rc.1",
        &fixture,
        None,
        None,
        true,
    );
    let env = TestEnv::with_endpoints(endpoints_for_release(&server));
    env.write_manifest(
        "pre-game",
        &release_manifest(
            "pre-game",
            "fixtures/pre-game",
            "pre-game-{target}.{ext}",
            None,
        ),
    );
    let services = env.open();
    let rt = TestEnv::runtime();
    let id = services.resolve_id("pre-game").unwrap();
    let err = rt.block_on(services.plan_install(&id, None)).unwrap_err();
    let msg = err.user_message();
    assert!(
        msg.causes.iter().any(|c| c.contains("no stable releases")),
        "{msg}"
    );
    assert_eq!(services.state_of(&id), GameState::Available);
}

#[test]
fn asset_missing_for_platform_gives_actionable_error() {
    let server = MockServer::start();
    let fixture = ReleaseFixture {
        asset_name: "other-game-riscv64-unknown-plan9.tar.gz".into(),
        bytes: vec![0; 10],
    };
    mock_release(
        &server,
        "fixtures/other-game",
        "v1.0.0",
        &fixture,
        None,
        None,
        false,
    );
    let env = TestEnv::with_endpoints(endpoints_for_release(&server));
    env.write_manifest(
        "other-game",
        &release_manifest(
            "other-game",
            "fixtures/other-game",
            "other-game-{target}.{ext}",
            None,
        ),
    );
    let services = env.open();
    let rt = TestEnv::runtime();
    let id = services.resolve_id("other-game").unwrap();
    let err = rt.block_on(services.plan_install(&id, None)).unwrap_err();
    let msg = err.user_message();
    assert!(
        msg.causes
            .iter()
            .any(|c| c.contains("no release asset matches")),
        "{msg}"
    );
}
