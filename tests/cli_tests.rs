//! Command-line round trips through the real binary.

mod common;

use common::TestEnv;
use predicates::prelude::*;

#[test]
fn version_and_help() {
    let env = TestEnv::new();
    env.cmd()
        .arg("version")
        .assert()
        .success()
        .stdout(predicate::str::contains("rustarcade 0.1.0"));
    env.cmd()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains("rustarcade"));
    env.cmd()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("Your terminal. Your arcade."));
}

#[test]
fn list_search_and_info_use_the_fixture_catalog() {
    let env = TestEnv::new();
    env.cmd()
        .args(["list", "--all"])
        .assert()
        .success()
        .stdout(predicate::str::contains("fixture-game"))
        .stdout(predicate::str::contains("4 game(s)"));
    env.cmd()
        .args(["search", "exit3"])
        .assert()
        .success()
        .stdout(predicate::str::contains("fixture-exit3"));
    env.cmd()
        .args(["search", "nothing-matches-this"])
        .assert()
        .success()
        .stdout(predicate::str::contains("No games match"));
    let out = env
        .cmd()
        .args(["search", "--json", "fixture"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let json: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert!(json.as_array().unwrap().len() >= 4);
    env.cmd()
        .args(["info", "fixture-game"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Cargo (crates.io)"))
        .stdout(predicate::str::contains("State             Available"));
    env.cmd()
        .args(["info", "nope"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("Unknown game"));
    env.cmd()
        .args(["list", "--installed"])
        .assert()
        .success()
        .stdout(predicate::str::contains("No games match"));
}

#[test]
fn catalog_validate_and_index() {
    let env = TestEnv::new();
    let fixtures =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/manifests");
    env.cmd()
        .args(["catalog", "validate"])
        .arg(fixtures.join("invalid"))
        .assert()
        .code(1)
        .stdout(predicate::str::contains("ERROR"))
        .stdout(predicate::str::contains("typed installer"))
        .stdout(predicate::str::contains("unknown variant `flatpak`"))
        .stdout(predicate::str::contains("0 valid, 3 problem(s)"));
    env.cmd()
        .args(["catalog", "validate"])
        .arg(fixtures.join("valid"))
        .assert()
        .success()
        .stdout(predicate::str::contains("1 valid, 0 problem(s)"));
    env.cmd()
        .args(["catalog", "validate"])
        .assert()
        .success()
        .stdout(predicate::str::contains("0 problem(s)"));
    let catalog = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("catalog");
    env.cmd()
        .args(["catalog", "index", "--check", "--dir"])
        .arg(&catalog)
        .assert()
        .success();
}

#[test]
fn doctor_reports_json_and_text() {
    let env = TestEnv::new();
    let out = env
        .cmd()
        .args(["--offline", "doctor", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let json: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert!(json["checks"].as_array().unwrap().len() > 8);
    env.cmd()
        .args(["--offline", "doctor"])
        .assert()
        .success()
        .stdout(predicate::str::contains("RustArcade Doctor"))
        .stdout(predicate::str::contains("offline mode"));
}

#[test]
fn install_play_update_uninstall_roundtrip() {
    let server = httpmock::MockServer::start();
    let mut mock = TestEnv::mock_crate(&server, "fixture-game", "0.1.0");
    let _m2 = TestEnv::mock_crate(&server, "fixture-exit3", "0.1.0");
    let env = TestEnv::with_endpoints(TestEnv::endpoints_for(&server));
    env.cmd()
        .args(["install", "fixture-game"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("--yes"));
    env.cmd()
        .args(["install", "fixture-game", "--yes"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Install Fixture fixture-game"))
        .stdout(predicate::str::contains("Administrator     not required"))
        .stdout(predicate::str::contains("installed to"));
    env.cmd()
        .args(["list", "--installed"])
        .assert()
        .success()
        .stdout(predicate::str::contains("installed 0.1.0"));
    env.cmd()
        .args(["info", "fixture-game"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Installed version 0.1.0"));
    env.cmd()
        .args(["play", "fixture-game"])
        .assert()
        .success()
        .stdout(predicate::str::contains("fixture game running"))
        .stdout(predicate::str::contains("exit code 0"));
    env.cmd()
        .args(["install", "fixture-exit3", "--yes", "--play"])
        .assert()
        .success()
        .stdout(predicate::str::contains("exit code 3"));
    env.cmd().args(["play", "fixture-exit3"]).assert().code(1);
    env.cmd()
        .args(["update", "--check"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "fixture-game: 0.1.0 is up to date",
        ));

    mock.delete();
    let _newer = TestEnv::mock_crate(&server, "fixture-game", "0.2.0");
    env.set_fake_version("0.2.0");
    env.cmd()
        .args(["update", "--check"])
        .assert()
        .success()
        .stdout(predicate::str::contains("fixture-game: 0.1.0 → 0.2.0"));
    env.cmd()
        .args(["update", "fixture-game", "--yes"])
        .assert()
        .success()
        .stdout(predicate::str::contains("is now 0.2.0"));
    env.cmd()
        .args(["update", "--all", "--yes"])
        .assert()
        .success()
        .stdout(predicate::str::contains("up to date"));

    env.cmd()
        .args(["uninstall", "fixture-game", "--yes"])
        .assert()
        .success()
        .stdout(predicate::str::contains("uninstalled"));
    env.cmd()
        .args(["uninstall", "fixture-game", "--yes"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("not installed"));
    env.cmd()
        .args(["play", "fixture-game"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("not installed"));
    env.cmd()
        .args(["--offline", "install", "fixture-game", "--yes"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("offline"));
}

#[test]
fn install_failure_reports_log_path() {
    let server = httpmock::MockServer::start();
    let _m = TestEnv::mock_crate(&server, "fixture-fail", "0.1.0");
    let env = TestEnv::with_endpoints(TestEnv::endpoints_for(&server));
    env.cmd()
        .args(["install", "fixture-fail", "--yes"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains(
            "Unable to install Fixture fixture-fail.",
        ))
        .stderr(predicate::str::contains("exit code 101"))
        .stderr(predicate::str::contains("Log:"));
    env.cmd()
        .args(["list", "--installed"])
        .assert()
        .success()
        .stdout(predicate::str::contains("No games match"));
}
