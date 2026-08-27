//! End-to-end install / play / update / rollback / uninstall through the fake cargo.

mod common;

use std::sync::Arc;
use std::time::Duration;

use common::TestEnv;
use rustarcade::catalog::manifest::InstallerKind;
use rustarcade::error::{Error, InstallError};
use rustarcade::install::{Phase, ProgressEvent, ProgressSink};
use rustarcade::launcher::NoTerminalSession;
use rustarcade::library::ExitOutcome;
use rustarcade::services::{GameState, Services};
use tokio_util::sync::CancellationToken;

fn install(
    rt: &tokio::runtime::Runtime,
    services: &Arc<Services>,
    id: &str,
) -> rustarcade::install::InstallOutcome {
    let id = services.resolve_id(id).unwrap();
    let plan = rt.block_on(services.plan_install(&id, None)).unwrap();
    rt.block_on(services.install(plan, ProgressSink::Silent, CancellationToken::new()))
        .unwrap()
}

#[test]
fn install_registers_game_and_creates_launcher() {
    let server = httpmock::MockServer::start();
    let _m = TestEnv::mock_crate(&server, "fixture-game", "0.1.0");
    let env = TestEnv::with_endpoints(TestEnv::endpoints_for(&server));
    let services = env.open();
    let rt = TestEnv::runtime();
    let id = services.resolve_id("fixture-game").unwrap();
    assert_eq!(services.state_of(&id), GameState::Available);

    let plan = rt.block_on(services.plan_install(&id, None)).unwrap();
    assert_eq!(plan.installer, InstallerKind::Cargo);
    assert_eq!(plan.version.as_deref(), Some("0.1.0"));
    assert!(!plan.is_update);
    assert!(!plan.requires_admin);
    assert!(plan.tools.iter().all(|t| t.path.is_some()));

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let outcome = rt
        .block_on(services.install(plan, ProgressSink::Channel(tx), CancellationToken::new()))
        .unwrap();
    assert_eq!(outcome.record.version, "0.1.0");
    assert!(outcome.warnings.is_empty(), "{:?}", outcome.warnings);
    let exe = outcome.record.executable_path(services.paths());
    assert!(exe.is_file(), "{exe:?}");
    assert!(exe.starts_with(services.paths().games_dir()));
    assert!(outcome.log.is_file());
    assert_eq!(services.state_of(&id), GameState::Installed);

    let mut phases = Vec::new();
    let mut started = false;
    let mut finished_ok = false;
    while let Ok(ev) = rx.try_recv() {
        match ev {
            ProgressEvent::Started { .. } => started = true,
            ProgressEvent::Phase { phase, .. } => phases.push(phase),
            ProgressEvent::Finished { success, .. } => finished_ok = success,
            _ => {}
        }
    }
    assert!(started && finished_ok);
    assert!(phases.contains(&Phase::Compiling), "{phases:?}");
    assert_eq!(phases.last(), Some(&Phase::Ready));

    let launcher = services
        .paths()
        .bin_dir()
        .join(services.platform().exe_name("fixture-game"));
    assert!(
        std::fs::symlink_metadata(&launcher).is_ok(),
        "launcher missing at {launcher:?}"
    );
    let leftovers: Vec<String> = std::fs::read_dir(services.paths().game_dir("fixture-game"))
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();
    assert_eq!(leftovers, vec!["current"]);
    let view = services.game(&id).unwrap();
    assert_eq!(view.installed_version(), Some("0.1.0"));
    assert!(services.installed().iter().any(|v| v.id() == &id));
}

#[test]
fn installed_games_play_and_record_history() {
    let server = httpmock::MockServer::start();
    let _a = TestEnv::mock_crate(&server, "fixture-game", "0.1.0");
    let _b = TestEnv::mock_crate(&server, "fixture-exit3", "0.1.0");
    let env = TestEnv::with_endpoints(TestEnv::endpoints_for(&server));
    let services = env.open();
    let rt = TestEnv::runtime();
    install(&rt, &services, "fixture-game");
    install(&rt, &services, "fixture-exit3");
    let ok = services.resolve_id("fixture-game").unwrap();
    let bad = services.resolve_id("fixture-exit3").unwrap();

    let result = services.play(&ok, &mut NoTerminalSession).unwrap();
    assert!(result.exit.success());
    let result = services.play(&bad, &mut NoTerminalSession).unwrap();
    assert_eq!(result.exit, ExitOutcome::Code { code: 3 });

    assert_eq!(services.stats(&ok).sessions, 1);
    assert_eq!(
        services.stats(&bad).last_exit,
        Some(ExitOutcome::Code { code: 3 })
    );
    let recent: Vec<String> = services
        .recent(5)
        .iter()
        .map(|v| v.id().to_string())
        .collect();
    assert_eq!(recent, vec!["fixture-exit3", "fixture-game"]);
    assert_eq!(services.history(None).len(), 2);
    assert!(services.state_of(&ok) == GameState::Installed);
    // History survives reopening.
    let reopened = env.open();
    assert_eq!(reopened.history(Some(&ok)).len(), 1);
}

#[test]
fn failed_install_rolls_back_and_keeps_log() {
    let server = httpmock::MockServer::start();
    let _m = TestEnv::mock_crate(&server, "fixture-fail", "0.1.0");
    let env = TestEnv::with_endpoints(TestEnv::endpoints_for(&server));
    let services = env.open();
    let rt = TestEnv::runtime();
    let id = services.resolve_id("fixture-fail").unwrap();
    let plan = rt.block_on(services.plan_install(&id, None)).unwrap();
    let err = rt
        .block_on(services.install(plan, ProgressSink::Silent, CancellationToken::new()))
        .unwrap_err();
    let msg = err.user_message();
    assert_eq!(msg.title, "Unable to install Fixture fixture-fail.");
    assert!(msg.detail.contains("exit code 101"), "{msg}");
    assert!(msg.causes.iter().any(|c| c.contains("toolchain")), "{msg}");
    let log = msg.log.expect("log path");
    assert!(log.is_file());
    let text = std::fs::read_to_string(&log).unwrap();
    assert!(text.contains("could not compile"), "{text}");
    assert_eq!(services.state_of(&id), GameState::Available);
    assert!(services.installed().is_empty());
    let game_dir = services.paths().game_dir("fixture-fail");
    let leftovers: Vec<_> = std::fs::read_dir(&game_dir)
        .map(|rd| rd.flatten().collect::<Vec<_>>())
        .unwrap_or_default();
    assert!(leftovers.is_empty(), "staging left behind: {leftovers:?}");
}

#[test]
fn update_swaps_version_and_failed_update_keeps_old_version() {
    let server = httpmock::MockServer::start();
    let mut mock = TestEnv::mock_crate(&server, "fixture-game", "0.1.0");
    let env = TestEnv::with_endpoints(TestEnv::endpoints_for(&server));
    let services = env.open();
    let rt = TestEnv::runtime();
    let first = install(&rt, &services, "fixture-game");
    let id = first.record.id.clone();
    let report = rt.block_on(services.check_updates(None, true));
    assert_eq!(report.checks.len(), 1);
    assert!(!report.checks[0].available);
    assert!(
        rt.block_on(services.plan_update(&id, false))
            .unwrap()
            .is_none()
    );

    // A newer crate appears.
    mock.delete();
    let _newer = TestEnv::mock_crate(&server, "fixture-game", "0.2.0");
    env.set_fake_version("0.2.0");
    let report = rt.block_on(services.check_updates(None, true));
    assert!(report.checks[0].available);
    assert_eq!(report.checks[0].latest, "0.2.0");
    assert_eq!(services.state_of(&id), GameState::UpdateAvailable);
    let view = services.game(&id).unwrap();
    assert_eq!(view.latest_version.as_deref(), Some("0.2.0"));

    let plan = rt
        .block_on(services.plan_update(&id, false))
        .unwrap()
        .expect("update plan");
    assert!(plan.is_update);
    assert_eq!(plan.previous_version.as_deref(), Some("0.1.0"));
    let outcome = rt
        .block_on(services.install(plan, ProgressSink::Silent, CancellationToken::new()))
        .unwrap();
    assert_eq!(outcome.record.version, "0.2.0");
    assert_eq!(outcome.previous_version.as_deref(), Some("0.1.0"));
    assert_eq!(outcome.record.installed_at, first.record.installed_at);
    assert!(outcome.record.updated_at >= first.record.updated_at);
    assert_eq!(services.state_of(&id), GameState::Installed);
    let names: Vec<String> = std::fs::read_dir(services.paths().game_dir("fixture-game"))
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();
    assert_eq!(
        names,
        vec!["current"],
        "previous version must be removed after a successful swap"
    );

    // Now a broken update must leave 0.2.0 intact.
    env.set_fake_version("fail");
    let plan = rt
        .block_on(services.plan_update(&id, true))
        .unwrap()
        .expect("forced plan");
    let err = rt
        .block_on(services.install(plan, ProgressSink::Silent, CancellationToken::new()))
        .unwrap_err();
    assert!(err.to_string().contains("101"), "{err}");
    let view = services.game(&id).unwrap();
    assert_eq!(view.installed_version(), Some("0.2.0"));
    assert!(
        view.install
            .unwrap()
            .executable_path(services.paths())
            .is_file()
    );
    assert!(
        services
            .play(&id, &mut NoTerminalSession)
            .unwrap()
            .exit
            .success()
    );
}

#[test]
fn cancelled_install_rolls_back() {
    let server = httpmock::MockServer::start();
    let _m = TestEnv::mock_crate(&server, "fixture-slow", "0.1.0");
    let env = TestEnv::with_endpoints(TestEnv::endpoints_for(&server));
    let services = env.open();
    let rt = TestEnv::runtime();
    let id = services.resolve_id("fixture-slow").unwrap();
    let plan = rt.block_on(services.plan_install(&id, None)).unwrap();
    let token = CancellationToken::new();
    let t = token.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(600));
        t.cancel();
    });
    let start = std::time::Instant::now();
    let err = rt
        .block_on(services.install(plan, ProgressSink::Silent, token))
        .unwrap_err();
    assert!(
        start.elapsed() < Duration::from_secs(6),
        "cancellation must kill the child promptly"
    );
    assert!(matches!(err, Error::InContext { .. }), "{err:?}");
    assert!(err.to_string().contains("cancelled"), "{err}");
    assert_eq!(services.state_of(&id), GameState::Available);
    let leftovers: Vec<_> = std::fs::read_dir(services.paths().game_dir("fixture-slow"))
        .map(|rd| rd.flatten().collect::<Vec<_>>())
        .unwrap_or_default();
    assert!(leftovers.is_empty(), "{leftovers:?}");
}

#[test]
fn concurrent_install_of_same_game_is_rejected() {
    let server = httpmock::MockServer::start();
    let _m = TestEnv::mock_crate(&server, "fixture-slow", "0.1.0");
    let env = TestEnv::with_endpoints(TestEnv::endpoints_for(&server));
    let services = env.open();
    let rt = TestEnv::runtime();
    let id = services.resolve_id("fixture-slow").unwrap();
    let plan = rt.block_on(services.plan_install(&id, None)).unwrap();
    let plan2 = plan.clone();
    let token = CancellationToken::new();
    let first = {
        let services = services.clone();
        let token = token.clone();
        rt.spawn(async move { services.install(plan, ProgressSink::Silent, token).await })
    };
    std::thread::sleep(Duration::from_millis(400));
    assert_eq!(services.state_of(&id), GameState::Installing);
    let err = rt
        .block_on(services.install(plan2, ProgressSink::Silent, CancellationToken::new()))
        .unwrap_err();
    assert!(
        matches!(err, Error::Install(InstallError::JobInProgress(_))),
        "{err:?}"
    );
    assert!(matches!(
        services.uninstall(&id),
        Err(Error::Install(InstallError::JobInProgress(_)))
    ));
    assert!(rt.block_on(services.plan_install(&id, None)).is_err());
    token.cancel();
    let _ = rt.block_on(first);
    assert_eq!(services.state_of(&id), GameState::Available);
}

#[test]
fn uninstall_removes_only_managed_paths_and_keeps_favorites() {
    let server = httpmock::MockServer::start();
    let _m = TestEnv::mock_crate(&server, "fixture-game", "0.1.0");
    let env = TestEnv::with_endpoints(TestEnv::endpoints_for(&server));
    let services = env.open();
    let rt = TestEnv::runtime();
    let outcome = install(&rt, &services, "fixture-game");
    let id = outcome.record.id.clone();
    assert!(services.toggle_favorite(&id).unwrap());
    let foreign = services.paths().data_dir().join("keep-me.txt");
    std::fs::write(&foreign, "user data").unwrap();
    let paths = services.uninstall_paths(&id).unwrap();
    assert!(
        paths
            .iter()
            .all(|p| p.starts_with(services.paths().data_dir()))
    );
    let report = services.uninstall(&id).unwrap();
    assert_eq!(report.removed.len(), 2, "{report:?}");
    assert!(!services.paths().game_dir("fixture-game").exists());
    assert!(
        std::fs::symlink_metadata(
            services
                .paths()
                .bin_dir()
                .join(services.platform().exe_name("fixture-game"))
        )
        .is_err()
    );
    assert!(foreign.exists());
    assert_eq!(services.state_of(&id), GameState::Available);
    assert!(services.is_favorite(&id));
    assert!(matches!(
        services.uninstall(&id),
        Err(Error::Install(InstallError::NotInstalled(_)))
    ));
    assert!(services.play(&id, &mut NoTerminalSession).is_err());
}

#[test]
fn reopen_recovers_interrupted_swap_and_flags_broken_installs() {
    let server = httpmock::MockServer::start();
    let _m = TestEnv::mock_crate(&server, "fixture-game", "0.1.0");
    let env = TestEnv::with_endpoints(TestEnv::endpoints_for(&server));
    let services = env.open();
    let rt = TestEnv::runtime();
    let outcome = install(&rt, &services, "fixture-game");
    let id = outcome.record.id.clone();
    drop(services);
    // Simulate a crash between "rename current → previous" and "rename staging → current".
    let game_dir = env.home().join("data/games/fixture-game");
    std::fs::rename(game_dir.join("current"), game_dir.join("previous-crash")).unwrap();
    std::fs::create_dir_all(game_dir.join("staging-crash/bin")).unwrap();
    let services = env.open();
    assert_eq!(services.sweep_report().restored.len(), 1);
    assert_eq!(services.state_of(&id), GameState::Installed);
    assert!(!game_dir.join("staging-crash").exists());
    // Deleting the executable outside RustArcade marks the game broken.
    std::fs::remove_dir_all(game_dir.join("current")).unwrap();
    assert!(matches!(services.state_of(&id), GameState::Broken(_)));
    let report = rt.block_on(rustarcade::doctor::run(&services));
    assert!(
        report
            .checks
            .iter()
            .any(|c| c.name == "Installation registry" && c.detail.contains("broken"))
    );
}
