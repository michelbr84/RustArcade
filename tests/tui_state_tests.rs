//! TUI reducer and rendering tests (no real terminal: `TestBackend`).

mod common;

use common::TestEnv;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use rustarcade::error::{Error, InstallError};
use rustarcade::install::{Phase, ProgressEvent, ProgressSink};
use rustarcade::library::ExitOutcome;
use rustarcade::services::GameState;
use rustarcade::tui::app::{AppEvent, Effect, Message, Modal, Tab, TuiApp};
use rustarcade::tui::screens;
use tokio_util::sync::CancellationToken;

fn key(code: KeyCode) -> Message {
    Message::Key(KeyEvent::new(code, KeyModifiers::NONE))
}

fn press(app: &mut TuiApp, code: KeyCode) -> Vec<Effect> {
    app.update(key(code))
}

fn type_str(app: &mut TuiApp, text: &str) {
    for c in text.chars() {
        press(app, KeyCode::Char(c));
    }
}

fn screen_text(terminal: &Terminal<TestBackend>) -> String {
    let buffer = terminal.backend().buffer();
    let mut out = String::new();
    for y in 0..buffer.area.height {
        for x in 0..buffer.area.width {
            out.push_str(buffer[(x, y)].symbol());
        }
        out.push('\n');
    }
    out
}

fn draw(app: &mut TuiApp, width: u16, height: u16) -> String {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|f| screens::render(f, app)).unwrap();
    screen_text(&terminal)
}

fn app_with_env() -> (TestEnv, TuiApp, httpmock::MockServer) {
    let server = httpmock::MockServer::start();
    // The mock stays registered with the server for the server's lifetime.
    TestEnv::mock_crate(&server, "fixture-game", "0.1.0");
    let env = TestEnv::with_endpoints(TestEnv::endpoints_for(&server));
    let services = env.open();
    let app = TuiApp::new(services);
    (env, app, server)
}

#[test]
fn welcome_then_tabs_and_navigation() {
    let (_env, mut app, _server) = app_with_env();
    assert!(matches!(app.modal, Some(Modal::Welcome)));
    press(&mut app, KeyCode::Enter);
    assert!(app.modal.is_none());
    assert_eq!(app.tab, Tab::Home);
    assert!(app.home.selected_id().is_some());

    press(&mut app, KeyCode::Char('2'));
    assert_eq!(app.tab, Tab::Discover);
    assert_eq!(app.discover.ids.len(), 4);
    let first = app.discover.selected_id().cloned().unwrap();
    press(&mut app, KeyCode::Char('j'));
    assert_ne!(app.discover.selected_id().cloned().unwrap(), first);
    press(&mut app, KeyCode::Char('G'));
    assert_eq!(app.discover.state.selected(), Some(3));
    press(&mut app, KeyCode::Char('g'));
    assert_eq!(app.discover.state.selected(), Some(0));

    press(&mut app, KeyCode::Enter);
    assert!(app.details.is_some());
    press(&mut app, KeyCode::Esc);
    assert!(app.details.is_none());
    press(&mut app, KeyCode::Tab);
    assert_eq!(app.tab, Tab::Library);
    press(&mut app, KeyCode::BackTab);
    assert_eq!(app.tab, Tab::Discover);
    press(&mut app, KeyCode::Char('?'));
    assert!(matches!(app.modal, Some(Modal::Help)));
    press(&mut app, KeyCode::Esc);
    assert!(app.modal.is_none());
}

#[test]
fn search_and_category_filters() {
    let (_env, mut app, _server) = app_with_env();
    press(&mut app, KeyCode::Enter);
    press(&mut app, KeyCode::Char('2'));
    press(&mut app, KeyCode::Char('/'));
    assert!(app.is_editing_search());
    type_str(&mut app, "exit3");
    assert_eq!(
        app.discover
            .ids
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>(),
        vec!["fixture-exit3"]
    );
    press(&mut app, KeyCode::Enter);
    assert!(!app.is_editing_search());
    assert_eq!(app.discover.query, "exit3");
    press(&mut app, KeyCode::Esc);
    assert!(app.discover.query.is_empty());
    assert_eq!(app.discover.ids.len(), 4);
    press(&mut app, KeyCode::Char('c'));
    assert!(app.discover.category.is_some());
    press(&mut app, KeyCode::Esc);
    assert!(app.discover.category.is_none());
    // Ctrl+C asks to quit (no jobs → quits immediately).
    let effects = app.update(Message::Key(KeyEvent::new(
        KeyCode::Char('c'),
        KeyModifiers::CONTROL,
    )));
    assert!(matches!(effects.as_slice(), [Effect::Quit]));
    assert!(app.should_quit);
}

#[test]
fn install_flow_plan_confirm_progress_and_completion() {
    let (_env, mut app, _server) = app_with_env();
    let rt = TestEnv::runtime();
    press(&mut app, KeyCode::Enter);
    press(&mut app, KeyCode::Char('2'));
    press(&mut app, KeyCode::Char('/'));
    type_str(&mut app, "fixture-game");
    press(&mut app, KeyCode::Enter);
    let id = app.discover.selected_id().cloned().unwrap();
    assert_eq!(id.as_str(), "fixture-game");

    let effects = press(&mut app, KeyCode::Char('i'));
    assert!(matches!(effects.as_slice(), [Effect::PlanInstall { .. }]));
    assert!(matches!(app.modal, Some(Modal::Planning { .. })));

    let plan = rt.block_on(app.services.plan_install(&id, None)).unwrap();
    let effects = app.update(Message::Event(Box::new(AppEvent::PlanReady {
        id: id.clone(),
        then_play: false,
        result: Ok(plan.clone()),
    })));
    assert!(effects.is_empty());
    assert!(matches!(app.modal, Some(Modal::ConfirmInstall { .. })));
    let text = draw(&mut app, 100, 30);
    assert!(text.contains("Confirm installation"), "{text}");
    assert!(text.contains("Administrator"), "{text}");

    let effects = press(&mut app, KeyCode::Enter);
    assert!(matches!(effects.as_slice(), [Effect::RunInstall { .. }]));
    assert!(matches!(app.modal, Some(Modal::Progress { .. })));
    assert!(app.jobs.contains_key(&id));
    assert_eq!(app.services.state_of(&id), GameState::Available);

    app.update(Message::Event(Box::new(AppEvent::Progress(
        ProgressEvent::Started {
            job: 1,
            game: id.clone(),
            log: std::path::PathBuf::from("/tmp/log"),
        },
    ))));
    app.update(Message::Event(Box::new(AppEvent::Progress(
        ProgressEvent::Phase {
            job: 1,
            phase: Phase::Compiling,
            detail: "ratatui v0.30".into(),
        },
    ))));
    app.update(Message::Event(Box::new(AppEvent::Progress(
        ProgressEvent::Output {
            job: 1,
            line: "   Compiling ratatui".into(),
        },
    ))));
    let job = app.jobs.get(&id).unwrap();
    assert_eq!(job.phase, Phase::Compiling);
    assert_eq!(job.detail, "ratatui v0.30");
    assert_eq!(job.tail.len(), 1);
    let text = draw(&mut app, 100, 30);
    assert!(text.contains("Installing Fixture fixture-game"), "{text}");
    assert!(text.contains("Compiling"), "{text}");

    // Really run the install so the completion event has a real outcome.
    let outcome = rt.block_on(app.services.install(
        plan,
        ProgressSink::Silent,
        CancellationToken::new(),
    ));
    let outcome = outcome.expect("fixture install");
    let effects = app.update(Message::Event(Box::new(AppEvent::InstallDone {
        id: id.clone(),
        then_play: false,
        result: Ok(outcome),
    })));
    assert!(effects.is_empty());
    assert!(app.modal.is_none());
    assert!(app.jobs.is_empty());
    assert!(
        app.toast
            .as_ref()
            .is_some_and(|(t, _, err)| t.contains("installed") && !err)
    );
    assert_eq!(app.services.state_of(&id), GameState::Installed);
    assert_eq!(app.view(&id).unwrap().state, GameState::Installed);

    // Library now lists it; details show Play as the primary action.
    press(&mut app, KeyCode::Char('3'));
    assert_eq!(app.library.ids, vec![id.clone()]);
    press(&mut app, KeyCode::Enter);
    let text = draw(&mut app, 100, 30);
    assert!(text.contains("[Enter] Play"), "{text}");
    let effects = press(&mut app, KeyCode::Enter);
    assert!(matches!(effects.as_slice(), [Effect::Play { .. }]));

    // Uninstall asks for confirmation then removes.
    press(&mut app, KeyCode::Char('x'));
    assert!(matches!(app.modal, Some(Modal::ConfirmUninstall { .. })));
    press(&mut app, KeyCode::Enter);
    assert!(matches!(app.modal, Some(Modal::Info { .. })));
    assert_eq!(app.services.state_of(&id), GameState::Available);
}

#[test]
fn errors_offer_retry_and_quit_requires_confirmation_with_jobs() {
    let (_env, mut app, _server) = app_with_env();
    press(&mut app, KeyCode::Enter);
    press(&mut app, KeyCode::Char('2'));
    let id = app.discover.selected_id().cloned().unwrap();
    press(&mut app, KeyCode::Char('i'));
    let err = Error::Install(InstallError::ProcessFailed {
        program: "cargo".into(),
        code: Some(101),
        log: Some(std::path::PathBuf::from("/tmp/install.log")),
        last_lines: vec![],
    })
    .in_context("install", "Fixture", None);
    app.update(Message::Event(Box::new(AppEvent::PlanReady {
        id: id.clone(),
        then_play: false,
        result: Err(err),
    })));
    assert!(matches!(app.modal, Some(Modal::Error { .. })));
    let text = draw(&mut app, 100, 30);
    assert!(text.contains("Unable to install Fixture."), "{text}");
    assert!(text.contains("retry"), "{text}");
    let effects = press(&mut app, KeyCode::Char('r'));
    assert!(matches!(effects.as_slice(), [Effect::PlanInstall { .. }]));
    assert!(matches!(app.modal, Some(Modal::Planning { .. })));
    press(&mut app, KeyCode::Esc);
    assert!(app.modal.is_none());

    // A running job makes quitting ask first.
    app.jobs.insert(
        id.clone(),
        rustarcade::tui::app::JobView {
            id: id.clone(),
            name: "Fixture".into(),
            job: Some(9),
            phase: Phase::Downloading,
            detail: String::new(),
            done: 10,
            total: Some(100),
            tail: Default::default(),
            started: std::time::Instant::now(),
            log: None,
            then_play: false,
            is_update: false,
            cancel: CancellationToken::new(),
        },
    );
    let effects = press(&mut app, KeyCode::Char('q'));
    assert!(effects.is_empty());
    assert!(matches!(app.modal, Some(Modal::ConfirmQuit)));
    press(&mut app, KeyCode::Char('n'));
    assert!(!app.should_quit);
    press(&mut app, KeyCode::Char('q'));
    let effects = press(&mut app, KeyCode::Enter);
    assert!(matches!(effects.as_slice(), [Effect::Quit]));
    assert!(app.jobs.get(&id).unwrap().cancel.is_cancelled());
}

#[test]
fn favorites_settings_and_play_results() {
    let (_env, mut app, _server) = app_with_env();
    press(&mut app, KeyCode::Enter);
    press(&mut app, KeyCode::Char('2'));
    let id = app.discover.selected_id().cloned().unwrap();
    press(&mut app, KeyCode::Char('f'));
    assert!(app.view(&id).unwrap().favorite);
    press(&mut app, KeyCode::Char('4'));
    assert_eq!(app.favorites.ids, vec![id.clone()]);
    press(&mut app, KeyCode::Char('f'));
    assert!(app.favorites.ids.is_empty());

    press(&mut app, KeyCode::Char('6'));
    assert_eq!(app.tab, Tab::Settings);
    let effects = press(&mut app, KeyCode::Char(' '));
    match effects.as_slice() {
        [Effect::SaveConfig(cfg)] => assert_eq!(cfg.general.theme, rustarcade::config::Theme::Mono),
        other => panic!("{other:?}"),
    }
    press(&mut app, KeyCode::Down);
    let effects = press(&mut app, KeyCode::Enter);
    match effects.as_slice() {
        [Effect::SaveConfig(cfg)] => assert!(!cfg.general.confirm_before_install),
        other => panic!("{other:?}"),
    }

    let result = rustarcade::launcher::LaunchResult {
        game: id.clone(),
        started_at: chrono::Utc::now(),
        ended_at: chrono::Utc::now(),
        duration: std::time::Duration::from_secs(65),
        exit: ExitOutcome::Code { code: 0 },
    };
    app.update(Message::Played(Box::new(Ok(result))));
    assert!(
        app.toast
            .as_ref()
            .is_some_and(|(t, _, err)| t.contains("Played") && t.contains("1m 05s") && !err)
    );
    app.update(Message::Played(Box::new(Err(Error::Launch(
        rustarcade::error::LaunchError::MissingExecutable { path: "/x".into() },
    )))));
    assert!(matches!(app.modal, Some(Modal::Error { .. })));
}

#[test]
fn renders_every_screen_and_modal_at_common_sizes() {
    let (_env, mut app, _server) = app_with_env();
    press(&mut app, KeyCode::Enter);
    for (tab, needle) in [
        (Tab::Home, "Featured"),
        (Tab::Discover, "Discover"),
        (Tab::Library, "No games installed yet"),
        (Tab::Favorites, "No favorites yet"),
        (Tab::Updates, "Nothing installed"),
        (Tab::Settings, "Confirm before installing"),
    ] {
        app.update(Message::Key(KeyEvent::new(
            KeyCode::Char((b'1' + tab.index() as u8) as char),
            KeyModifiers::NONE,
        )));
        let text = draw(&mut app, 80, 24);
        assert!(text.contains("RUSTARCADE"), "{text}");
        assert!(text.contains(needle), "{tab:?}: {text}");
        let wide = draw(&mut app, 140, 40);
        assert!(wide.contains(needle), "{tab:?} wide: {wide}");
    }
    press(&mut app, KeyCode::Char('2'));
    press(&mut app, KeyCode::Enter);
    let text = draw(&mut app, 80, 24);
    assert!(text.contains("Third-party software"), "{text}");
    assert!(text.contains("Install & Play"), "{text}");

    app.modal = Some(Modal::Help);
    assert!(draw(&mut app, 80, 24).contains("Keyboard shortcuts"));
    app.modal = Some(Modal::ConfirmQuit);
    assert!(draw(&mut app, 80, 24).contains("Quit anyway"));
    app.modal = Some(Modal::LogView {
        path: "/tmp/x.log".into(),
        lines: (0..100).map(|i| format!("line {i}")).collect(),
        scroll: 99,
    });
    let text = draw(&mut app, 80, 24);
    assert!(text.contains("line 99"), "{text}");
    app.modal = None;

    // Too small: a notice instead of the interface.
    let text = draw(&mut app, 40, 12);
    assert!(text.contains("Terminal too small"), "{text}");
    assert!(!text.contains("RUSTARCADE"));
    app.update(Message::Resize(120, 40));
    assert_eq!(app.size, (120, 40));
    app.update(Message::Tick);
}
