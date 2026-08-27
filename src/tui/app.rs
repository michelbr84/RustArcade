//! TUI state machine: a pure `update(Message) -> Vec<Effect>` reducer over [`TuiApp`].

use std::collections::{BTreeMap, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use crossterm::event::KeyEvent;
use ratatui::widgets::{ListState, TableState};
use tokio_util::sync::CancellationToken;

use crate::catalog::manifest::{Category, GameId, InstallerKind};
use crate::catalog::remote::CatalogRefreshReport;
use crate::config::Config;
use crate::error::{Error, UserMessage};
use crate::install::{
    InstallOutcome, InstallPlan, JobId, Phase, ProgressEvent, UninstallReport, UpdateCheck,
};
use crate::launcher::LaunchResult;
use crate::library::format_duration;
use crate::services::{CatalogStatus, GameFilter, GameState, GameView, Services, UpdateReport};

use super::keymap::{self, Action};
use super::theme::Theme;

/// Top-level tabs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Home,
    Discover,
    Library,
    Favorites,
    Updates,
    Settings,
}

impl Tab {
    pub const ALL: [Tab; 6] = [
        Tab::Home,
        Tab::Discover,
        Tab::Library,
        Tab::Favorites,
        Tab::Updates,
        Tab::Settings,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Tab::Home => "Home",
            Tab::Discover => "Discover",
            Tab::Library => "Library",
            Tab::Favorites => "Favorites",
            Tab::Updates => "Updates",
            Tab::Settings => "Settings",
        }
    }

    pub fn index(self) -> usize {
        Tab::ALL.iter().position(|t| *t == self).unwrap_or(0)
    }

    pub fn from_index(i: usize) -> Option<Tab> {
        Tab::ALL.get(i).copied()
    }

    pub fn next(self) -> Tab {
        Tab::ALL[(self.index() + 1) % Tab::ALL.len()]
    }

    pub fn prev(self) -> Tab {
        Tab::ALL[(self.index() + Tab::ALL.len() - 1) % Tab::ALL.len()]
    }
}

/// A filterable, searchable list of games.
#[derive(Debug, Default)]
pub struct ListView {
    pub query: String,
    pub editing: bool,
    pub category: Option<Category>,
    pub ids: Vec<GameId>,
    pub state: TableState,
}

impl ListView {
    pub fn selected_id(&self) -> Option<&GameId> {
        self.state.selected().and_then(|i| self.ids.get(i))
    }

    fn select_index(&mut self, i: usize) {
        if self.ids.is_empty() {
            self.state.select(None);
        } else {
            self.state.select(Some(i.min(self.ids.len() - 1)));
        }
    }

    pub fn move_by(&mut self, delta: isize) {
        if self.ids.is_empty() {
            self.state.select(None);
            return;
        }
        let current = self.state.selected().unwrap_or(0) as isize;
        let next = (current + delta).clamp(0, self.ids.len() as isize - 1);
        self.state.select(Some(next as usize));
    }

    pub fn first(&mut self) {
        self.select_index(0);
    }

    pub fn last(&mut self) {
        self.select_index(self.ids.len().saturating_sub(1));
    }

    /// Replace the ids, keeping the same game selected when possible.
    pub fn set_ids(&mut self, ids: Vec<GameId>) {
        let previous = self.selected_id().cloned();
        self.ids = ids;
        match previous.and_then(|p| self.ids.iter().position(|i| *i == p)) {
            Some(i) => self.state.select(Some(i)),
            None => self.select_index(self.state.selected().unwrap_or(0)),
        }
    }
}

/// Item in the Home screen list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HomeItem {
    Header(String),
    Game(GameId),
}

#[derive(Debug, Default)]
pub struct HomeState {
    pub items: Vec<HomeItem>,
    pub state: ListState,
}

impl HomeState {
    pub fn selected_id(&self) -> Option<&GameId> {
        match self.state.selected().and_then(|i| self.items.get(i)) {
            Some(HomeItem::Game(id)) => Some(id),
            _ => None,
        }
    }

    fn move_by(&mut self, delta: isize) {
        if self.items.is_empty() {
            self.state.select(None);
            return;
        }
        let len = self.items.len() as isize;
        let mut i = self.state.selected().unwrap_or(0) as isize;
        let step = if delta < 0 { -1 } else { 1 };
        let mut remaining = delta.abs();
        while remaining > 0 {
            let mut j = i + step;
            while j >= 0 && j < len && matches!(self.items[j as usize], HomeItem::Header(_)) {
                j += step;
            }
            if j < 0 || j >= len {
                break;
            }
            i = j;
            remaining -= 1;
        }
        self.state.select(Some(i as usize));
    }

    fn ensure_selection(&mut self) {
        if let Some(i) = self.state.selected()
            && matches!(self.items.get(i), Some(HomeItem::Game(_)))
        {
            return;
        }
        let first = self
            .items
            .iter()
            .position(|i| matches!(i, HomeItem::Game(_)));
        self.state.select(first);
    }
}

#[derive(Debug, Default)]
pub struct UpdatesState {
    pub checks: Vec<UpdateCheck>,
    pub errors: Vec<String>,
    pub checking: bool,
    pub checked: bool,
    pub state: TableState,
}

impl UpdatesState {
    pub fn selected(&self) -> Option<&UpdateCheck> {
        self.state.selected().and_then(|i| self.checks.get(i))
    }

    fn move_by(&mut self, delta: isize) {
        if self.checks.is_empty() {
            self.state.select(None);
            return;
        }
        let current = self.state.selected().unwrap_or(0) as isize;
        let next = (current + delta).clamp(0, self.checks.len() as isize - 1);
        self.state.select(Some(next as usize));
    }
}

/// Settings rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Setting {
    Theme,
    ConfirmInstall,
    ConfirmUpdate,
    ShowExperimental,
    CheckUpdatesOnStart,
    AutoUpdateCatalog,
    RequireChecksum,
    KeepDownloads,
    ShowWelcome,
}

impl Setting {
    pub const ALL: [Setting; 9] = [
        Setting::Theme,
        Setting::ConfirmInstall,
        Setting::ConfirmUpdate,
        Setting::ShowExperimental,
        Setting::CheckUpdatesOnStart,
        Setting::AutoUpdateCatalog,
        Setting::RequireChecksum,
        Setting::KeepDownloads,
        Setting::ShowWelcome,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Setting::Theme => "Theme",
            Setting::ConfirmInstall => "Confirm before installing",
            Setting::ConfirmUpdate => "Confirm before updating",
            Setting::ShowExperimental => "Show experimental games in Discover",
            Setting::CheckUpdatesOnStart => "Check for game updates on start",
            Setting::AutoUpdateCatalog => "Refresh the remote catalog automatically",
            Setting::RequireChecksum => "Require a checksum for release downloads",
            Setting::KeepDownloads => "Keep downloaded archives in the cache",
            Setting::ShowWelcome => "Show the welcome screen on start",
        }
    }

    pub fn value(self, config: &Config) -> String {
        match self {
            Setting::Theme => config.general.theme.label().to_string(),
            Setting::ConfirmInstall => on_off(config.general.confirm_before_install),
            Setting::ConfirmUpdate => on_off(config.general.confirm_before_update),
            Setting::ShowExperimental => on_off(config.general.show_experimental),
            Setting::CheckUpdatesOnStart => on_off(config.updates.check_on_start),
            Setting::AutoUpdateCatalog => on_off(config.catalog.auto_update),
            Setting::RequireChecksum => on_off(config.install.require_checksum),
            Setting::KeepDownloads => on_off(config.install.keep_downloads),
            Setting::ShowWelcome => on_off(config.general.show_welcome),
        }
    }

    pub fn toggle(self, config: &mut Config) {
        match self {
            Setting::Theme => config.general.theme = config.general.theme.next(),
            Setting::ConfirmInstall => {
                config.general.confirm_before_install = !config.general.confirm_before_install
            }
            Setting::ConfirmUpdate => {
                config.general.confirm_before_update = !config.general.confirm_before_update
            }
            Setting::ShowExperimental => {
                config.general.show_experimental = !config.general.show_experimental
            }
            Setting::CheckUpdatesOnStart => {
                config.updates.check_on_start = !config.updates.check_on_start
            }
            Setting::AutoUpdateCatalog => config.catalog.auto_update = !config.catalog.auto_update,
            Setting::RequireChecksum => {
                config.install.require_checksum = !config.install.require_checksum
            }
            Setting::KeepDownloads => {
                config.install.keep_downloads = !config.install.keep_downloads
            }
            Setting::ShowWelcome => config.general.show_welcome = !config.general.show_welcome,
        }
    }
}

fn on_off(b: bool) -> String {
    if b { "on".into() } else { "off".into() }
}

#[derive(Debug, Default)]
pub struct SettingsState {
    pub selected: usize,
}

/// What a retry from an error dialog should do.
#[derive(Debug, Clone)]
pub enum RetryAction {
    Install {
        id: GameId,
        then_play: bool,
        prefer: Option<InstallerKind>,
    },
    Update {
        id: GameId,
    },
}

/// Overlay dialogs.
#[derive(Debug)]
pub enum Modal {
    Welcome,
    Help,
    Planning {
        id: GameId,
        then_play: bool,
    },
    ConfirmInstall {
        plan: Box<InstallPlan>,
        then_play: bool,
    },
    ConfirmUpdateAll {
        ids: Vec<GameId>,
    },
    ConfirmUninstall {
        id: GameId,
        name: String,
        paths: Vec<PathBuf>,
    },
    Progress {
        id: GameId,
    },
    Error {
        message: UserMessage,
        retry: Option<RetryAction>,
    },
    LogView {
        path: PathBuf,
        lines: Vec<String>,
        scroll: usize,
    },
    Info {
        title: String,
        lines: Vec<String>,
    },
    ConfirmQuit,
}

/// A running install/update job as seen by the UI.
#[derive(Debug)]
pub struct JobView {
    pub id: GameId,
    pub name: String,
    pub job: Option<JobId>,
    pub phase: Phase,
    pub detail: String,
    pub done: u64,
    pub total: Option<u64>,
    pub tail: VecDeque<String>,
    pub started: Instant,
    pub log: Option<PathBuf>,
    pub then_play: bool,
    pub is_update: bool,
    pub cancel: CancellationToken,
}

/// Results of background work.
#[derive(Debug)]
pub enum AppEvent {
    Progress(ProgressEvent),
    PlanReady {
        id: GameId,
        then_play: bool,
        result: Result<InstallPlan, Error>,
    },
    UpdatePlanReady {
        id: GameId,
        result: Result<Option<InstallPlan>, Error>,
    },
    InstallDone {
        id: GameId,
        then_play: bool,
        result: Result<InstallOutcome, Error>,
    },
    UpdatesChecked(UpdateReport),
    CatalogRefreshed(Result<Option<CatalogRefreshReport>, Error>),
}

/// Inputs to the reducer.
#[derive(Debug)]
pub enum Message {
    Key(KeyEvent),
    Resize(u16, u16),
    Tick,
    Interrupt,
    Event(Box<AppEvent>),
    Played(Box<Result<LaunchResult, Error>>),
}

/// Side effects the event loop must perform.
#[derive(Debug)]
pub enum Effect {
    PlanInstall {
        id: GameId,
        then_play: bool,
        prefer: Option<InstallerKind>,
    },
    RunInstall {
        plan: Box<InstallPlan>,
        then_play: bool,
    },
    PlanUpdate {
        id: GameId,
    },
    CheckUpdates {
        ids: Option<Vec<GameId>>,
        force: bool,
    },
    RefreshCatalog {
        force: bool,
    },
    Play {
        id: GameId,
    },
    SaveConfig(Box<Config>),
    Quit,
}

/// Detail overlay.
#[derive(Debug, Clone)]
pub struct DetailsState {
    pub id: GameId,
    pub scroll: u16,
}

/// Complete UI state.
pub struct TuiApp {
    pub services: Arc<Services>,
    pub config: Config,
    pub tab: Tab,
    pub details: Option<DetailsState>,
    pub modal: Option<Modal>,
    pub views: Vec<GameView>,
    pub home: HomeState,
    pub discover: ListView,
    pub library: ListView,
    pub favorites: ListView,
    pub updates: UpdatesState,
    pub settings: SettingsState,
    pub jobs: BTreeMap<GameId, JobView>,
    pub update_queue: VecDeque<GameId>,
    pub toast: Option<(String, Instant, bool)>,
    pub size: (u16, u16),
    pub tick: u64,
    pub should_quit: bool,
    pub catalog_status: CatalogStatus,
    pub last_launch: Option<String>,
}

impl std::fmt::Debug for TuiApp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TuiApp")
            .field("tab", &self.tab)
            .field("modal", &self.modal.is_some())
            .finish_non_exhaustive()
    }
}

pub const MIN_COLS: u16 = 60;
pub const MIN_ROWS: u16 = 16;
const TOAST_SECS: u64 = 6;

impl TuiApp {
    pub fn new(services: Arc<Services>) -> TuiApp {
        let config = services.config();
        let catalog_status = services.catalog_status();
        let mut app = TuiApp {
            services,
            modal: if config.general.show_welcome {
                Some(Modal::Welcome)
            } else {
                None
            },
            config,
            tab: Tab::Home,
            details: None,
            views: Vec::new(),
            home: HomeState::default(),
            discover: ListView::default(),
            library: ListView::default(),
            favorites: ListView::default(),
            updates: UpdatesState::default(),
            settings: SettingsState::default(),
            jobs: BTreeMap::new(),
            update_queue: VecDeque::new(),
            toast: None,
            size: (80, 24),
            tick: 0,
            should_quit: false,
            catalog_status,
            last_launch: None,
        };
        app.refresh();
        app
    }

    pub fn theme(&self) -> Theme {
        Theme::from_choice(self.config.general.theme)
    }

    pub fn too_small(&self) -> bool {
        self.size.0 < MIN_COLS || self.size.1 < MIN_ROWS
    }

    pub fn view(&self, id: &GameId) -> Option<&GameView> {
        self.views.iter().find(|v| v.id() == id)
    }

    /// Re-read every game from the services and rebuild the lists.
    pub fn refresh(&mut self) {
        self.views = self.services.games();
        self.catalog_status = self.services.catalog_status();
        self.rebuild_lists();
    }

    fn rebuild_lists(&mut self) {
        let include_experimental = self.config.general.show_experimental;
        let discover_ids = self
            .services
            .list(&GameFilter {
                query: Some(self.discover.query.clone()),
                category: self.discover.category,
                include_experimental,
                ..GameFilter::default()
            })
            .into_iter()
            .map(|v| v.id().clone())
            .collect();
        self.discover.set_ids(discover_ids);
        let library_ids = self
            .services
            .list(&GameFilter {
                query: Some(self.library.query.clone()),
                category: self.library.category,
                installed_only: true,
                include_experimental: true,
                ..GameFilter::default()
            })
            .into_iter()
            .map(|v| v.id().clone())
            .collect();
        self.library.set_ids(library_ids);
        let favorite_ids = self
            .services
            .list(&GameFilter {
                query: Some(self.favorites.query.clone()),
                category: self.favorites.category,
                favorites_only: true,
                include_experimental: true,
                ..GameFilter::default()
            })
            .into_iter()
            .map(|v| v.id().clone())
            .collect();
        self.favorites.set_ids(favorite_ids);

        let recent: Vec<GameId> = self
            .services
            .recent(3)
            .into_iter()
            .map(|v| v.id().clone())
            .collect();
        let mut items = Vec::new();
        if !recent.is_empty() {
            items.push(HomeItem::Header("Continue playing".into()));
            items.extend(recent.iter().cloned().map(HomeItem::Game));
        }
        let mut featured: Vec<&GameView> = self
            .views
            .iter()
            .filter(|v| !recent.contains(v.id()))
            .filter(|v| {
                include_experimental
                    || v.state.is_installed()
                    || !matches!(
                        v.manifest.support_status,
                        crate::catalog::SupportStatus::Experimental
                    )
            })
            .collect();
        featured.sort_by_key(|v| {
            (
                v.manifest.support_status,
                !v.state.is_installed(),
                v.manifest.name.to_lowercase(),
            )
        });
        if !featured.is_empty() {
            items.push(HomeItem::Header("Featured".into()));
            items.extend(featured.iter().map(|v| HomeItem::Game(v.id().clone())));
        }
        self.home.items = items;
        self.home.ensure_selection();
        if self.updates.state.selected().is_none() && !self.updates.checks.is_empty() {
            self.updates.state.select(Some(0));
        }
    }

    pub fn active_list(&mut self) -> Option<&mut ListView> {
        match self.tab {
            Tab::Discover => Some(&mut self.discover),
            Tab::Library => Some(&mut self.library),
            Tab::Favorites => Some(&mut self.favorites),
            _ => None,
        }
    }

    pub fn active_list_ref(&self) -> Option<&ListView> {
        match self.tab {
            Tab::Discover => Some(&self.discover),
            Tab::Library => Some(&self.library),
            Tab::Favorites => Some(&self.favorites),
            _ => None,
        }
    }

    /// Game the current screen is focused on.
    pub fn focused_id(&self) -> Option<GameId> {
        if let Some(d) = &self.details {
            return Some(d.id.clone());
        }
        match self.tab {
            Tab::Home => self.home.selected_id().cloned(),
            Tab::Updates => self.updates.selected().map(|c| c.game.clone()),
            _ => self
                .active_list_ref()
                .and_then(|l| l.selected_id().cloned()),
        }
    }

    pub fn is_editing_search(&self) -> bool {
        self.details.is_none() && self.active_list_ref().is_some_and(|l| l.editing)
    }

    pub fn toast(&mut self, text: impl Into<String>, is_error: bool) {
        self.toast = Some((text.into(), Instant::now(), is_error));
    }

    // ----- reducer -------------------------------------------------------------------

    pub fn update(&mut self, msg: Message) -> Vec<Effect> {
        match msg {
            Message::Key(key) => match keymap::map(self, key) {
                Some(action) => self.handle_action(action),
                None => Vec::new(),
            },
            Message::Resize(w, h) => {
                self.size = (w, h);
                Vec::new()
            }
            Message::Tick => {
                self.tick = self.tick.wrapping_add(1);
                if let Some((_, at, _)) = &self.toast
                    && at.elapsed().as_secs() >= TOAST_SECS
                {
                    self.toast = None;
                }
                Vec::new()
            }
            Message::Interrupt => self.handle_action(Action::Quit),
            Message::Event(ev) => self.handle_event(*ev),
            Message::Played(result) => self.after_play(*result),
        }
    }

    fn handle_action(&mut self, action: Action) -> Vec<Effect> {
        // Modal-scoped actions first.
        if self.modal.is_some() {
            return self.handle_modal_action(action);
        }
        if self.is_editing_search() {
            return self.handle_search_action(action);
        }
        match action {
            Action::Quit => {
                if self.jobs.is_empty() {
                    self.should_quit = true;
                    vec![Effect::Quit]
                } else {
                    self.modal = Some(Modal::ConfirmQuit);
                    Vec::new()
                }
            }
            Action::ForceQuit => {
                self.should_quit = true;
                vec![Effect::Quit]
            }
            Action::Help => {
                self.modal = Some(Modal::Help);
                Vec::new()
            }
            Action::GoTab(tab) => self.go_tab(tab),
            Action::NextTab => {
                let t = self.tab.next();
                self.go_tab(t)
            }
            Action::PrevTab => {
                let t = self.tab.prev();
                self.go_tab(t)
            }
            Action::Back => {
                if self.details.take().is_some() {
                    return Vec::new();
                }
                if let Some(list) = self.active_list()
                    && (!list.query.is_empty() || list.category.is_some())
                {
                    list.query.clear();
                    list.category = None;
                    self.rebuild_lists();
                }
                Vec::new()
            }
            Action::Up => self.move_selection(-1),
            Action::Down => self.move_selection(1),
            Action::PageUp => self.move_selection(-10),
            Action::PageDown => self.move_selection(10),
            Action::First => {
                match self.tab {
                    Tab::Settings => self.settings.selected = 0,
                    Tab::Home => {
                        self.home.state.select(None);
                        self.home.ensure_selection();
                    }
                    _ => {
                        if let Some(l) = self.active_list() {
                            l.first();
                        }
                    }
                }
                Vec::new()
            }
            Action::Last => {
                match self.tab {
                    Tab::Settings => self.settings.selected = Setting::ALL.len() - 1,
                    Tab::Home => self.home.move_by(self.home.items.len() as isize),
                    _ => {
                        if let Some(l) = self.active_list() {
                            l.last();
                        }
                    }
                }
                Vec::new()
            }
            Action::Select => self.select(),
            Action::SearchStart => {
                if let Some(list) = self.active_list() {
                    list.editing = true;
                }
                Vec::new()
            }
            Action::CycleCategory => {
                if let Some(list) = self.active_list() {
                    list.category = next_category(list.category);
                    self.rebuild_lists();
                }
                Vec::new()
            }
            Action::ToggleFavorite => {
                if let Some(id) = self.focused_id() {
                    match self.services.toggle_favorite(&id) {
                        Ok(now) => {
                            let name = self
                                .view(&id)
                                .map(|v| v.manifest.name.clone())
                                .unwrap_or_default();
                            self.toast(
                                if now {
                                    format!("★ {name} added to favorites")
                                } else {
                                    format!("{name} removed from favorites")
                                },
                                false,
                            );
                            self.refresh();
                        }
                        Err(e) => self.show_error(e, None),
                    }
                }
                Vec::new()
            }
            Action::Install { then_play } => self.start_install(then_play, None),
            Action::Play => self.start_play(),
            Action::Update => self.start_update(),
            Action::UpdateAll => {
                let ids: Vec<GameId> = self
                    .updates
                    .checks
                    .iter()
                    .filter(|c| c.available)
                    .map(|c| c.game.clone())
                    .collect();
                if ids.is_empty() {
                    self.toast("No updates available", false);
                } else {
                    self.modal = Some(Modal::ConfirmUpdateAll { ids });
                }
                Vec::new()
            }
            Action::Uninstall => {
                if let Some(id) = self.focused_id() {
                    let state = self.services.state_of(&id);
                    if !state.is_installed() {
                        self.toast("This game is not installed", true);
                        return Vec::new();
                    }
                    match self.services.uninstall_paths(&id) {
                        Ok(paths) => {
                            let name = self
                                .view(&id)
                                .map(|v| v.manifest.name.clone())
                                .unwrap_or_else(|| id.to_string());
                            self.modal = Some(Modal::ConfirmUninstall { id, name, paths });
                        }
                        Err(e) => self.show_error(e, None),
                    }
                }
                Vec::new()
            }
            Action::Refresh => match self.tab {
                Tab::Updates => self.check_updates(true),
                _ => {
                    self.refresh();
                    vec![Effect::RefreshCatalog { force: true }]
                }
            },
            Action::ViewLog => {
                if let Some(id) = self.focused_id() {
                    let log = self
                        .view(&id)
                        .and_then(|v| v.install.as_ref())
                        .and_then(|r| r.log.clone());
                    match log {
                        Some(path) => self.open_log(path),
                        None => self.toast("No installation log for this game", true),
                    }
                }
                Vec::new()
            }
            Action::ToggleSetting => {
                if self.tab == Tab::Settings {
                    let setting = Setting::ALL[self.settings.selected.min(Setting::ALL.len() - 1)];
                    setting.toggle(&mut self.config);
                    self.rebuild_lists();
                    return vec![Effect::SaveConfig(Box::new(self.config.clone()))];
                }
                Vec::new()
            }
            Action::CancelJob => {
                if let Some(id) = self.focused_id()
                    && let Some(job) = self.jobs.get(&id)
                {
                    job.cancel.cancel();
                    self.toast("Cancelling…", false);
                }
                Vec::new()
            }
            Action::ScrollUp | Action::ScrollDown => {
                if let Some(d) = &mut self.details {
                    d.scroll = if matches!(action, Action::ScrollUp) {
                        d.scroll.saturating_sub(1)
                    } else {
                        d.scroll.saturating_add(1)
                    };
                }
                Vec::new()
            }
            Action::SearchChar(_)
            | Action::SearchBackspace
            | Action::SearchCommit
            | Action::SearchCancel
            | Action::Confirm
            | Action::Cancel
            | Action::Retry
            | Action::HideModal => Vec::new(),
        }
    }

    fn go_tab(&mut self, tab: Tab) -> Vec<Effect> {
        self.details = None;
        self.tab = tab;
        self.refresh();
        if tab == Tab::Updates && !self.updates.checked && !self.updates.checking {
            return self.check_updates(false);
        }
        Vec::new()
    }

    fn move_selection(&mut self, delta: isize) -> Vec<Effect> {
        if self.details.is_some() {
            if let Some(d) = &mut self.details {
                d.scroll = if delta < 0 {
                    d.scroll.saturating_sub(delta.unsigned_abs() as u16)
                } else {
                    d.scroll.saturating_add(delta as u16)
                };
            }
            return Vec::new();
        }
        match self.tab {
            Tab::Home => self.home.move_by(delta),
            Tab::Updates => self.updates.move_by(delta),
            Tab::Settings => {
                let max = Setting::ALL.len() as isize - 1;
                self.settings.selected =
                    (self.settings.selected as isize + delta).clamp(0, max) as usize;
            }
            _ => {
                if let Some(l) = self.active_list() {
                    l.move_by(delta);
                }
            }
        }
        Vec::new()
    }

    fn select(&mut self) -> Vec<Effect> {
        if self.details.is_some() {
            return self.primary_action();
        }
        match self.tab {
            Tab::Settings => self.handle_action(Action::ToggleSetting),
            Tab::Updates => {
                if let Some(id) = self.focused_id() {
                    self.details = Some(DetailsState { id, scroll: 0 });
                }
                Vec::new()
            }
            _ => {
                if let Some(id) = self.focused_id() {
                    self.details = Some(DetailsState { id, scroll: 0 });
                }
                Vec::new()
            }
        }
    }

    /// Enter on the details screen: play if installed, otherwise install & play.
    fn primary_action(&mut self) -> Vec<Effect> {
        let Some(id) = self.focused_id() else {
            return Vec::new();
        };
        match self.services.state_of(&id) {
            GameState::Installed | GameState::UpdateAvailable => self.start_play(),
            GameState::Available | GameState::Broken(_) => self.start_install(true, None),
            GameState::Installing => {
                self.modal = Some(Modal::Progress { id });
                Vec::new()
            }
            GameState::Running => Vec::new(),
            GameState::Unsupported(reason) => {
                self.toast(format!("Unsupported: {reason}"), true);
                Vec::new()
            }
        }
    }

    fn start_install(&mut self, then_play: bool, prefer: Option<InstallerKind>) -> Vec<Effect> {
        let Some(id) = self.focused_id() else {
            return Vec::new();
        };
        match self.services.state_of(&id) {
            GameState::Installing => {
                self.modal = Some(Modal::Progress { id });
                Vec::new()
            }
            GameState::Running => {
                self.toast("The game is running", true);
                Vec::new()
            }
            GameState::Unsupported(reason) => {
                self.toast(format!("Unsupported: {reason}"), true);
                Vec::new()
            }
            _ => {
                self.modal = Some(Modal::Planning {
                    id: id.clone(),
                    then_play,
                });
                vec![Effect::PlanInstall {
                    id,
                    then_play,
                    prefer,
                }]
            }
        }
    }

    fn start_play(&mut self) -> Vec<Effect> {
        let Some(id) = self.focused_id() else {
            return Vec::new();
        };
        let state = self.services.state_of(&id);
        if !state.is_installed() {
            self.toast("Install the game first (press i)", true);
            return Vec::new();
        }
        if matches!(state, GameState::Broken(_)) {
            self.toast("This installation is broken — reinstall it with i", true);
            return Vec::new();
        }
        vec![Effect::Play { id }]
    }

    fn start_update(&mut self) -> Vec<Effect> {
        let Some(id) = self.focused_id() else {
            return Vec::new();
        };
        if !self.services.state_of(&id).is_installed() {
            self.toast("Install the game first (press i)", true);
            return Vec::new();
        }
        self.modal = Some(Modal::Planning {
            id: id.clone(),
            then_play: false,
        });
        vec![Effect::PlanUpdate { id }]
    }

    fn check_updates(&mut self, force: bool) -> Vec<Effect> {
        if self.services.installed().is_empty() {
            self.updates.checked = true;
            return Vec::new();
        }
        self.updates.checking = true;
        vec![Effect::CheckUpdates { ids: None, force }]
    }

    fn open_log(&mut self, path: PathBuf) {
        let lines: Vec<String> = std::fs::read_to_string(&path)
            .map(|t| t.lines().map(str::to_string).collect())
            .unwrap_or_else(|e| vec![format!("could not read log: {e}")]);
        let scroll = lines.len().saturating_sub(1);
        self.modal = Some(Modal::LogView {
            path,
            lines,
            scroll,
        });
    }

    pub fn show_error(&mut self, error: Error, retry: Option<RetryAction>) {
        tracing::warn!("ui error: {error}");
        self.modal = Some(Modal::Error {
            message: error.user_message(),
            retry,
        });
    }

    fn handle_search_action(&mut self, action: Action) -> Vec<Effect> {
        let Some(list) = self.active_list() else {
            return Vec::new();
        };
        match action {
            Action::SearchChar(c) => {
                list.query.push(c);
                self.rebuild_lists();
            }
            Action::SearchBackspace => {
                list.query.pop();
                self.rebuild_lists();
            }
            Action::SearchCommit => list.editing = false,
            Action::SearchCancel => {
                list.editing = false;
                list.query.clear();
                self.rebuild_lists();
            }
            Action::Down => list.move_by(1),
            Action::Up => list.move_by(-1),
            _ => {}
        }
        Vec::new()
    }

    fn handle_modal_action(&mut self, action: Action) -> Vec<Effect> {
        let modal = self.modal.take();
        match modal {
            Some(Modal::Welcome) => {
                if matches!(action, Action::Confirm | Action::Cancel | Action::HideModal) {
                    return Vec::new();
                }
                self.modal = Some(Modal::Welcome);
                Vec::new()
            }
            Some(Modal::Help) | Some(Modal::Info { .. }) => Vec::new(),
            Some(Modal::Planning { id, then_play }) => {
                if matches!(action, Action::Cancel | Action::HideModal) {
                    // The plan result will arrive and be discarded.
                    return Vec::new();
                }
                self.modal = Some(Modal::Planning { id, then_play });
                Vec::new()
            }
            Some(Modal::ConfirmInstall { plan, then_play }) => match action {
                Action::Confirm => self.run_plan(plan, then_play),
                Action::Cancel | Action::HideModal => Vec::new(),
                _ => {
                    self.modal = Some(Modal::ConfirmInstall { plan, then_play });
                    Vec::new()
                }
            },
            Some(Modal::ConfirmUpdateAll { ids }) => match action {
                Action::Confirm => {
                    self.update_queue = ids.into_iter().collect();
                    self.next_queued_update()
                }
                Action::Cancel | Action::HideModal => Vec::new(),
                _ => {
                    self.modal = Some(Modal::ConfirmUpdateAll { ids });
                    Vec::new()
                }
            },
            Some(Modal::ConfirmUninstall { id, name, paths }) => match action {
                Action::Confirm => {
                    match self.services.uninstall(&id) {
                        Ok(report) => self.after_uninstall(report),
                        Err(e) => self.show_error(e, None),
                    }
                    self.refresh();
                    Vec::new()
                }
                Action::Cancel | Action::HideModal => Vec::new(),
                _ => {
                    self.modal = Some(Modal::ConfirmUninstall { id, name, paths });
                    Vec::new()
                }
            },
            Some(Modal::Progress { id }) => match action {
                Action::Cancel | Action::HideModal | Action::Confirm => Vec::new(),
                Action::CancelJob => {
                    if let Some(job) = self.jobs.get(&id) {
                        job.cancel.cancel();
                        self.toast("Cancelling…", false);
                    }
                    self.modal = Some(Modal::Progress { id });
                    Vec::new()
                }
                _ => {
                    self.modal = Some(Modal::Progress { id });
                    Vec::new()
                }
            },
            Some(Modal::Error { message, retry }) => match action {
                Action::Retry => match retry {
                    Some(RetryAction::Install {
                        id,
                        then_play,
                        prefer,
                    }) => {
                        self.modal = Some(Modal::Planning {
                            id: id.clone(),
                            then_play,
                        });
                        vec![Effect::PlanInstall {
                            id,
                            then_play,
                            prefer,
                        }]
                    }
                    Some(RetryAction::Update { id }) => {
                        self.modal = Some(Modal::Planning {
                            id: id.clone(),
                            then_play: false,
                        });
                        vec![Effect::PlanUpdate { id }]
                    }
                    None => Vec::new(),
                },
                Action::ViewLog => {
                    match message.log.clone() {
                        Some(path) => self.open_log(path),
                        None => self.modal = Some(Modal::Error { message, retry }),
                    }
                    Vec::new()
                }
                Action::Cancel | Action::HideModal | Action::Confirm => Vec::new(),
                _ => {
                    self.modal = Some(Modal::Error { message, retry });
                    Vec::new()
                }
            },
            Some(Modal::LogView {
                path,
                lines,
                mut scroll,
            }) => match action {
                Action::Cancel | Action::HideModal | Action::Confirm => Vec::new(),
                other => {
                    let max = lines.len().saturating_sub(1);
                    scroll = match other {
                        Action::Up | Action::ScrollUp => scroll.saturating_sub(1),
                        Action::Down | Action::ScrollDown => (scroll + 1).min(max),
                        Action::PageUp => scroll.saturating_sub(20),
                        Action::PageDown => (scroll + 20).min(max),
                        Action::First => 0,
                        Action::Last => max,
                        _ => scroll,
                    };
                    self.modal = Some(Modal::LogView {
                        path,
                        lines,
                        scroll,
                    });
                    Vec::new()
                }
            },
            Some(Modal::ConfirmQuit) => match action {
                Action::Confirm => {
                    for job in self.jobs.values() {
                        job.cancel.cancel();
                    }
                    self.should_quit = true;
                    vec![Effect::Quit]
                }
                Action::Cancel | Action::HideModal => Vec::new(),
                _ => {
                    self.modal = Some(Modal::ConfirmQuit);
                    Vec::new()
                }
            },
            None => Vec::new(),
        }
    }

    fn run_plan(&mut self, plan: Box<InstallPlan>, then_play: bool) -> Vec<Effect> {
        let id = plan.game.clone();
        self.jobs.insert(
            id.clone(),
            JobView {
                id: id.clone(),
                name: plan.name.clone(),
                job: None,
                phase: Phase::Resolving,
                detail: String::new(),
                done: 0,
                total: None,
                tail: VecDeque::new(),
                started: Instant::now(),
                log: None,
                then_play,
                is_update: plan.is_update,
                cancel: CancellationToken::new(),
            },
        );
        self.modal = Some(Modal::Progress { id });
        self.refresh();
        vec![Effect::RunInstall { plan, then_play }]
    }

    fn next_queued_update(&mut self) -> Vec<Effect> {
        while let Some(id) = self.update_queue.pop_front() {
            if self.services.state_of(&id).is_installed() && !self.jobs.contains_key(&id) {
                self.modal = Some(Modal::Planning {
                    id: id.clone(),
                    then_play: false,
                });
                return vec![Effect::PlanUpdate { id }];
            }
        }
        Vec::new()
    }

    fn after_uninstall(&mut self, report: UninstallReport) {
        let mut lines = vec![
            format!("{} was uninstalled.", report.game),
            String::new(),
            "Removed:".into(),
        ];
        lines.extend(report.removed.iter().map(|p| format!("  {}", p.display())));
        if !report.warnings.is_empty() {
            lines.push(String::new());
            lines.extend(report.warnings.iter().map(|w| format!("Note: {w}")));
        }
        lines.push(String::new());
        lines.push("Save files and settings outside RustArcade were kept.".into());
        self.modal = Some(Modal::Info {
            title: "Uninstalled".into(),
            lines,
        });
    }

    fn handle_event(&mut self, event: AppEvent) -> Vec<Effect> {
        match event {
            AppEvent::Progress(p) => {
                self.apply_progress(p);
                Vec::new()
            }
            AppEvent::PlanReady {
                id,
                then_play,
                result,
            } => {
                let waiting =
                    matches!(&self.modal, Some(Modal::Planning { id: pid, .. }) if *pid == id);
                if !waiting {
                    return Vec::new();
                }
                self.modal = None;
                match result {
                    Ok(plan) => {
                        let needs_confirm = if plan.is_update {
                            self.config.general.confirm_before_update
                        } else {
                            self.config.general.confirm_before_install
                        };
                        if needs_confirm {
                            self.modal = Some(Modal::ConfirmInstall {
                                plan: Box::new(plan),
                                then_play,
                            });
                            Vec::new()
                        } else {
                            self.run_plan(Box::new(plan), then_play)
                        }
                    }
                    Err(e) => {
                        self.show_error(
                            e,
                            Some(RetryAction::Install {
                                id,
                                then_play,
                                prefer: None,
                            }),
                        );
                        Vec::new()
                    }
                }
            }
            AppEvent::UpdatePlanReady { id, result } => {
                let waiting =
                    matches!(&self.modal, Some(Modal::Planning { id: pid, .. }) if *pid == id);
                if !waiting {
                    return Vec::new();
                }
                self.modal = None;
                match result {
                    Ok(Some(plan)) => {
                        if self.config.general.confirm_before_update && self.update_queue.is_empty()
                        {
                            self.modal = Some(Modal::ConfirmInstall {
                                plan: Box::new(plan),
                                then_play: false,
                            });
                            Vec::new()
                        } else {
                            self.run_plan(Box::new(plan), false)
                        }
                    }
                    Ok(None) => {
                        let name = self
                            .view(&id)
                            .map(|v| v.manifest.name.clone())
                            .unwrap_or_else(|| id.to_string());
                        self.toast(format!("{name} is already up to date"), false);
                        self.next_queued_update()
                    }
                    Err(e) => {
                        self.update_queue.clear();
                        self.show_error(e, Some(RetryAction::Update { id }));
                        Vec::new()
                    }
                }
            }
            AppEvent::InstallDone {
                id,
                then_play,
                result,
            } => {
                let job = self.jobs.remove(&id);
                let is_update = job.as_ref().is_some_and(|j| j.is_update);
                if matches!(&self.modal, Some(Modal::Progress { id: pid }) if *pid == id) {
                    self.modal = None;
                }
                self.refresh();
                match result {
                    Ok(outcome) => {
                        let verb = if is_update { "updated to" } else { "installed" };
                        self.toast(
                            format!(
                                "✓ {} {verb} {}",
                                outcome.record.name, outcome.record.version
                            ),
                            false,
                        );
                        let mut effects = Vec::new();
                        if then_play && self.details.as_ref().is_some_and(|d| d.id == id) {
                            effects.push(Effect::Play { id: id.clone() });
                        }
                        if !self.update_queue.is_empty() {
                            effects.extend(self.next_queued_update());
                        } else if self.tab == Tab::Updates {
                            effects.extend(self.check_updates(true));
                        }
                        effects
                    }
                    Err(e) => {
                        self.update_queue.clear();
                        let retry = if is_update {
                            RetryAction::Update { id }
                        } else {
                            RetryAction::Install {
                                id,
                                then_play,
                                prefer: None,
                            }
                        };
                        self.show_error(e, Some(retry));
                        Vec::new()
                    }
                }
            }
            AppEvent::UpdatesChecked(report) => {
                self.updates.checking = false;
                self.updates.checked = true;
                self.updates.checks = report.checks;
                self.updates
                    .checks
                    .sort_by_key(|c| (!c.available, c.game.to_string()));
                self.updates.errors = report
                    .errors
                    .iter()
                    .map(|(g, e)| format!("{g}: {e}"))
                    .collect();
                if self.updates.state.selected().is_none() && !self.updates.checks.is_empty() {
                    self.updates.state.select(Some(0));
                }
                self.refresh();
                let n = self.updates.checks.iter().filter(|c| c.available).count();
                if n > 0 {
                    self.toast(
                        format!("{n} update{} available", if n == 1 { "" } else { "s" }),
                        false,
                    );
                }
                Vec::new()
            }
            AppEvent::CatalogRefreshed(result) => {
                match result {
                    Ok(Some(r)) if !r.unchanged() => {
                        self.toast(
                            format!(
                                "Catalog updated: +{} ~{} -{}",
                                r.added.len(),
                                r.updated.len(),
                                r.removed.len()
                            ),
                            false,
                        );
                    }
                    Ok(_) => {}
                    Err(e) => tracing::warn!("catalog refresh failed: {e}"),
                }
                self.refresh();
                Vec::new()
            }
        }
    }

    fn apply_progress(&mut self, p: ProgressEvent) {
        let job_id = p.job();
        let Some(job) = self.jobs.values_mut().find(|j| {
            j.job == Some(job_id)
                || (j.job.is_none()
                    && matches!(&p, ProgressEvent::Started { game, .. } if *game == j.id))
        }) else {
            return;
        };
        match p {
            ProgressEvent::Started { job: id, log, .. } => {
                job.job = Some(id);
                job.log = Some(log);
            }
            ProgressEvent::Phase { phase, detail, .. } => {
                job.phase = phase;
                job.detail = detail;
                if phase != Phase::Downloading {
                    job.done = 0;
                    job.total = None;
                }
            }
            ProgressEvent::Bytes { done, total, .. } => {
                job.done = done;
                job.total = total;
            }
            ProgressEvent::Output { line, .. } => {
                if !line.trim().is_empty() {
                    if job.tail.len() >= 6 {
                        job.tail.pop_front();
                    }
                    job.tail.push_back(line);
                }
            }
            ProgressEvent::Finished { .. } => {}
        }
    }

    fn after_play(&mut self, result: Result<LaunchResult, Error>) -> Vec<Effect> {
        self.refresh();
        match result {
            Ok(r) => {
                let name = self
                    .view(&r.game)
                    .map(|v| v.manifest.name.clone())
                    .unwrap_or_else(|| r.game.to_string());
                let text = format!(
                    "Played {name} for {} ({})",
                    format_duration(r.duration),
                    r.exit.label()
                );
                self.last_launch = Some(text.clone());
                self.toast(text, !r.exit.success());
            }
            Err(e) => self.show_error(e, None),
        }
        Vec::new()
    }

    /// Footer hints for the current context.
    pub fn hints(&self) -> Vec<(&'static str, &'static str)> {
        if let Some(modal) = &self.modal {
            return match modal {
                Modal::Welcome | Modal::Help | Modal::Info { .. } => vec![("Enter/Esc", "close")],
                Modal::Planning { .. } => vec![("Esc", "cancel")],
                Modal::ConfirmInstall { .. }
                | Modal::ConfirmUpdateAll { .. }
                | Modal::ConfirmUninstall { .. }
                | Modal::ConfirmQuit => vec![("Enter/y", "confirm"), ("Esc/n", "cancel")],
                Modal::Progress { .. } => vec![("Esc", "hide"), ("c", "cancel job")],
                Modal::Error { retry, message } => {
                    let mut h = Vec::new();
                    if retry.is_some() {
                        h.push(("r", "retry"));
                    }
                    if message.log.is_some() {
                        h.push(("l", "view log"));
                    }
                    h.push(("Esc", "back"));
                    h
                }
                Modal::LogView { .. } => {
                    vec![("↑↓", "scroll"), ("g/G", "top/bottom"), ("Esc", "close")]
                }
            };
        }
        if self.is_editing_search() {
            return vec![("type", "to filter"), ("Enter", "keep"), ("Esc", "clear")];
        }
        if let Some(d) = &self.details {
            let state = self.services.state_of(&d.id);
            let mut h: Vec<(&str, &str)> = match state {
                GameState::Available => vec![("Enter", "install & play"), ("i", "install")],
                GameState::Installed => {
                    vec![("Enter", "play"), ("u", "check update"), ("x", "uninstall")]
                }
                GameState::UpdateAvailable => {
                    vec![("Enter", "play"), ("u", "update"), ("x", "uninstall")]
                }
                GameState::Installing => vec![("Enter", "progress"), ("c", "cancel")],
                GameState::Broken(_) => vec![("Enter", "reinstall"), ("x", "remove"), ("l", "log")],
                GameState::Running | GameState::Unsupported(_) => vec![],
            };
            h.push(("f", "favorite"));
            h.push(("↑↓", "scroll"));
            h.push(("Esc", "back"));
            return h;
        }
        match self.tab {
            Tab::Home => vec![
                ("↑↓", "navigate"),
                ("Enter", "details"),
                ("Tab", "next tab"),
                ("f", "favorite"),
                ("?", "help"),
                ("q", "quit"),
            ],
            Tab::Discover | Tab::Library | Tab::Favorites => vec![
                ("↑↓", "navigate"),
                ("Enter", "details"),
                ("/", "search"),
                ("c", "category"),
                ("i", "install"),
                ("p", "play"),
                ("f", "favorite"),
                ("x", "uninstall"),
                ("q", "quit"),
            ],
            Tab::Updates => vec![
                ("↑↓", "navigate"),
                ("u", "update"),
                ("a", "update all"),
                ("r", "re-check"),
                ("Enter", "details"),
                ("q", "quit"),
            ],
            Tab::Settings => vec![
                ("↑↓", "navigate"),
                ("Enter/Space", "toggle"),
                ("Tab", "next tab"),
                ("q", "quit"),
            ],
        }
    }
}

fn next_category(current: Option<Category>) -> Option<Category> {
    match current {
        None => Some(Category::ALL[0]),
        Some(c) => {
            let i = Category::ALL.iter().position(|x| *x == c).unwrap_or(0);
            Category::ALL.get(i + 1).copied()
        }
    }
}
