//! git-cargo-build installer against a local repository, using the real toolchain.

mod common;

use std::path::Path;
use std::process::Command;

use common::{TestEnv, git_manifest};
use rustarcade::install::ProgressSink;
use rustarcade::launcher::NoTerminalSession;
use rustarcade::registry::InstallSource;
use rustarcade::services::GameState;
use tokio_util::sync::CancellationToken;

fn toolchain_available() -> bool {
    let ok = which::which("git").is_ok() && which::which("cargo").is_ok();
    if !ok {
        eprintln!("skipping: git and cargo are required for this test");
    }
    ok
}

fn git(dir: &Path, args: &[&str]) {
    let status = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args([
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .status()
        .expect("run git");
    assert!(status.success(), "git {args:?} failed");
}

fn write_crate(dir: &Path, body: &str) {
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("Cargo.toml"),
        "[package]\nname = \"hello-game\"\nversion = \"0.3.0\"\nedition = \"2021\"\n\n[workspace]\n",
    )
    .unwrap();
    std::fs::write(dir.join("src/main.rs"), body).unwrap();
}

fn make_repo(root: &Path) -> String {
    let repo = root.join("hello-game");
    write_crate(&repo, "fn main() { println!(\"hello from git\"); }\n");
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "init"]);
    git(&repo, &["tag", "v0.3.0"]);
    url::Url::from_file_path(&repo).unwrap().to_string()
}

fn head(repo: &Path) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["rev-parse", "HEAD"])
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

#[test]
fn builds_from_local_git_repository_and_detects_updates() {
    if !toolchain_available() {
        return;
    }
    let env = TestEnv::new();
    let url = make_repo(env.root.path());
    env.write_manifest("git-game", &git_manifest("git-game", &url, None));
    let services = env.open_with_real_tools();
    let rt = TestEnv::runtime();
    let id = services.resolve_id("git-game").unwrap();
    let plan = rt.block_on(services.plan_install(&id, None)).unwrap();
    assert!(plan.compiles);
    assert!(
        plan.version
            .as_deref()
            .is_some_and(|v| v.starts_with("git ")),
        "{:?}",
        plan.version
    );
    let outcome = rt
        .block_on(services.install(plan, ProgressSink::Silent, CancellationToken::new()))
        .unwrap();
    assert_eq!(outcome.record.version, "0.3.0");
    let repo = env.root.path().join("hello-game");
    match &outcome.record.source {
        InstallSource::GitCargoBuild { commit, .. } => {
            assert_eq!(commit.as_deref(), Some(head(&repo).as_str()))
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(services.state_of(&id), GameState::Installed);
    let result = services.play(&id, &mut NoTerminalSession).unwrap();
    assert!(result.exit.success());
    assert!(
        std::fs::read_dir(services.paths().build_dir())
            .unwrap()
            .next()
            .is_none(),
        "build dir must be cleaned"
    );

    let report = rt.block_on(services.check_updates(None, true));
    assert!(!report.checks[0].available, "{report:?}");

    std::fs::write(
        repo.join("src/main.rs"),
        "fn main() { println!(\"hello v2\"); }\n",
    )
    .unwrap();
    git(&repo, &["commit", "-q", "-am", "v2"]);
    let report = rt.block_on(services.check_updates(None, true));
    assert!(report.checks[0].available, "{report:?}");
    let plan = rt
        .block_on(services.plan_update(&id, false))
        .unwrap()
        .expect("update plan");
    let outcome = rt
        .block_on(services.install(plan, ProgressSink::Silent, CancellationToken::new()))
        .unwrap();
    match &outcome.record.source {
        InstallSource::GitCargoBuild { commit, .. } => {
            assert_eq!(commit.as_deref(), Some(head(&repo).as_str()))
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn tagged_reference_is_checked_out_and_broken_sources_roll_back() {
    if !toolchain_available() {
        return;
    }
    let env = TestEnv::new();
    let url = make_repo(env.root.path());
    let repo = env.root.path().join("hello-game");
    // Break main after tagging so the tag still builds but HEAD does not.
    std::fs::write(repo.join("src/main.rs"), "fn main() { this is not rust }\n").unwrap();
    git(&repo, &["commit", "-q", "-am", "break"]);

    env.write_manifest("git-tag", &git_manifest("git-tag", &url, Some("v0.3.0")));
    let head_manifest = format!(
        "{}binary = \"hello-game\"\n",
        git_manifest("git-head", &url, None).replace(
            "executable = \"hello-game\"",
            "executable = \"hello-game-head\""
        )
    );
    env.write_manifest("git-head", &head_manifest);
    let services = env.open_with_real_tools();
    let rt = TestEnv::runtime();

    let tag_id = services.resolve_id("git-tag").unwrap();
    let plan = rt.block_on(services.plan_install(&tag_id, None)).unwrap();
    let outcome = rt
        .block_on(services.install(plan, ProgressSink::Silent, CancellationToken::new()))
        .unwrap();
    assert_eq!(outcome.record.version, "0.3.0");

    let head_id = services.resolve_id("git-head").unwrap();
    let plan = rt.block_on(services.plan_install(&head_id, None)).unwrap();
    let err = rt
        .block_on(services.install(plan, ProgressSink::Silent, CancellationToken::new()))
        .unwrap_err();
    let msg = err.user_message();
    assert!(msg.detail.contains("cargo build"), "{msg}");
    assert!(msg.log.as_ref().is_some_and(|l| l.is_file()));
    assert_eq!(services.state_of(&head_id), GameState::Available);
    assert!(
        std::fs::read_dir(services.paths().build_dir())
            .unwrap()
            .next()
            .is_none()
    );
}
