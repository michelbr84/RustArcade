//! Terminal user interface: event loop, terminal lifecycle, and effect execution.
//!
//! The main thread owns the terminal and is the only reader of stdin. Background
//! work (planning, installing, update checks, catalog refresh) runs on a tokio
//! runtime and reports back through a channel drained every tick.

pub mod app;
pub mod keymap;
pub mod modals;
pub mod screens;
pub mod theme;
pub mod widgets;

use std::io::{self, Write};
use std::sync::Arc;
use std::time::Duration;

use crossterm::event::{self, Event, KeyEventKind};
use crossterm::terminal::{
    Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use crossterm::{cursor, execute};
use ratatui::backend::CrosstermBackend;
use ratatui::{DefaultTerminal, Terminal};
use tokio::sync::mpsc;

use crate::error::LaunchError;
use crate::install::ProgressSink;
use crate::launcher::TerminalSession;
use crate::services::{OpenOptions, Services};

use app::{AppEvent, Effect, Message, TuiApp};

/// Run the interactive interface; returns the process exit code.
pub fn run(opts: OpenOptions) -> i32 {
    let services = match Services::open(opts) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("{}", e.user_message());
            return e.exit_code();
        }
    };
    if !io::stdout().is_terminal_like() {
        eprintln!(
            "RustArcade's interface needs an interactive terminal. Try `rustarcade list` or `rustarcade --help`."
        );
        return 2;
    }
    services.interrupt().install_handler();
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("could not start the async runtime: {e}");
            return 1;
        }
    };
    let mut terminal = match ratatui::try_init() {
        Ok(t) => t,
        Err(e) => {
            eprintln!("could not initialise the terminal: {e}");
            return 1;
        }
    };
    let result = run_loop(&runtime, services, &mut terminal);
    ratatui::restore();
    match result {
        Ok(code) => code,
        Err(e) => {
            eprintln!("RustArcade stopped: {e}");
            1
        }
    }
}

trait TerminalLike {
    fn is_terminal_like(&self) -> bool;
}

impl TerminalLike for io::Stdout {
    fn is_terminal_like(&self) -> bool {
        use std::io::IsTerminal;
        self.is_terminal()
    }
}

/// Suspends/resumes the ratatui terminal around a child game.
struct RatatuiSession<'a> {
    terminal: &'a mut DefaultTerminal,
}

impl TerminalSession for RatatuiSession<'_> {
    fn suspend(&mut self) -> Result<(), LaunchError> {
        let stage =
            |stage: &'static str| move |e: io::Error| LaunchError::Terminal { stage, source: e };
        disable_raw_mode().map_err(stage("disable raw mode"))?;
        let mut out = io::stdout();
        execute!(out, LeaveAlternateScreen, cursor::Show)
            .map_err(stage("leave alternate screen"))?;
        out.flush().map_err(stage("flush"))?;
        Ok(())
    }

    fn resume(&mut self) -> Result<(), LaunchError> {
        let stage =
            |stage: &'static str| move |e: io::Error| LaunchError::Terminal { stage, source: e };
        let mut out = io::stdout();
        execute!(
            out,
            EnterAlternateScreen,
            cursor::Hide,
            Clear(ClearType::All)
        )
        .map_err(stage("enter alternate screen"))?;
        enable_raw_mode().map_err(stage("enable raw mode"))?;
        // Drop any keystrokes typed into the game after it exited.
        while event::poll(Duration::from_millis(0)).unwrap_or(false) {
            let _ = event::read();
        }
        // A fresh Terminal has empty buffers, so the next draw repaints every cell without
        // querying the terminal (ratatui's `clear()` waits for a cursor-position reply).
        *self.terminal = Terminal::new(CrosstermBackend::new(io::stdout()))
            .map_err(stage("recreate terminal"))?;
        Ok(())
    }
}

fn run_loop(
    runtime: &tokio::runtime::Runtime,
    services: Arc<Services>,
    terminal: &mut DefaultTerminal,
) -> io::Result<i32> {
    let (tx, mut rx) = mpsc::unbounded_channel::<AppEvent>();
    let mut app = TuiApp::new(services.clone());
    if let Ok(size) = terminal.size() {
        app.size = (size.width, size.height);
    }
    let mut effects: Vec<Effect> = Vec::new();
    let config = app.config.clone();
    if config.catalog.auto_update && !services.offline() {
        effects.push(Effect::RefreshCatalog { force: false });
    }
    if config.updates.check_on_start && !services.offline() && !services.installed().is_empty() {
        app.updates.checking = true;
        effects.push(Effect::CheckUpdates {
            ids: None,
            force: false,
        });
    }

    loop {
        terminal.draw(|frame| screens::render(frame, &mut app))?;
        if app.should_quit {
            break;
        }
        while let Ok(ev) = rx.try_recv() {
            effects.extend(app.update(Message::Event(Box::new(ev))));
        }
        if services.interrupt().take() {
            effects.extend(app.update(Message::Interrupt));
        }
        if event::poll(Duration::from_millis(50))? {
            match event::read()? {
                Event::Key(key) if key.kind == KeyEventKind::Press => {
                    effects.extend(app.update(Message::Key(key)))
                }
                Event::Resize(w, h) => effects.extend(app.update(Message::Resize(w, h))),
                _ => {}
            }
        } else {
            effects.extend(app.update(Message::Tick));
        }
        let pending: Vec<Effect> = std::mem::take(&mut effects);
        for effect in pending {
            execute_effect(
                effect,
                runtime,
                &services,
                &tx,
                &mut app,
                terminal,
                &mut effects,
            );
        }
    }
    Ok(0)
}

fn execute_effect(
    effect: Effect,
    runtime: &tokio::runtime::Runtime,
    services: &Arc<Services>,
    tx: &mpsc::UnboundedSender<AppEvent>,
    app: &mut TuiApp,
    terminal: &mut DefaultTerminal,
    follow_ups: &mut Vec<Effect>,
) {
    match effect {
        Effect::PlanInstall {
            id,
            then_play,
            prefer,
        } => {
            let services = services.clone();
            let tx = tx.clone();
            runtime.spawn(async move {
                let result = services.plan_install(&id, prefer).await;
                let _ = tx.send(AppEvent::PlanReady {
                    id,
                    then_play,
                    result,
                });
            });
        }
        Effect::RunInstall { plan, then_play } => {
            let id = plan.game.clone();
            let cancel = app
                .jobs
                .get(&id)
                .map(|j| j.cancel.clone())
                .unwrap_or_default();
            let services = services.clone();
            let tx = tx.clone();
            let progress_tx = tx.clone();
            let sink =
                ProgressSink::Callback(Arc::new(move |ev: &crate::install::ProgressEvent| {
                    let _ = progress_tx.send(AppEvent::Progress(ev.clone()));
                }));
            runtime.spawn(async move {
                let result = services.install(*plan, sink, cancel).await;
                let _ = tx.send(AppEvent::InstallDone {
                    id,
                    then_play,
                    result,
                });
            });
        }
        Effect::PlanUpdate { id } => {
            let services = services.clone();
            let tx = tx.clone();
            runtime.spawn(async move {
                let result = services.plan_update(&id, false).await;
                let _ = tx.send(AppEvent::UpdatePlanReady { id, result });
            });
        }
        Effect::CheckUpdates { ids, force } => {
            let services = services.clone();
            let tx = tx.clone();
            runtime.spawn(async move {
                let report = services.check_updates(ids.as_deref(), force).await;
                let _ = tx.send(AppEvent::UpdatesChecked(report));
            });
        }
        Effect::RefreshCatalog { force } => {
            let services = services.clone();
            let tx = tx.clone();
            runtime.spawn(async move {
                let result = services.refresh_catalog(force).await;
                let _ = tx.send(AppEvent::CatalogRefreshed(result));
            });
        }
        Effect::Play { id } => {
            let mut session = RatatuiSession { terminal };
            let result = services.play(&id, &mut session);
            follow_ups.extend(app.update(Message::Played(Box::new(result))));
        }
        Effect::SaveConfig(config) => {
            if let Err(e) = services.save_config(*config) {
                app.show_error(e, None);
            }
        }
        Effect::Quit => app.should_quit = true,
    }
}
