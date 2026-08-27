//! The application core shared by the CLI and the TUI.
//!
//! Every user-facing action goes through [`Services`]: it owns the catalog, registry,
//! library, configuration, HTTP clients, and the guards that keep installs, updates
//! and launches from stepping on each other.

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

use crate::catalog::manifest::{
    Category, GameId, GameManifest, InstallerKind, RunCwd, SupportStatus,
};
use crate::catalog::remote::{self, CatalogMeta, CatalogRefreshReport};
use crate::catalog::{Catalog, CatalogOrigin, LoadReport, ValidationMode};
use crate::config::Config;
use crate::error::{CatalogError, Error, InstallError, LaunchError, StorageError};
use crate::install::{
    self, InstallEnv, InstallOutcome, InstallPlan, JobId, Progress, ProgressSink, SweepReport,
    UninstallReport, UpdateCheck,
};
use crate::launcher::{self, InterruptFlag, LaunchResult, LaunchSpec, TerminalSession};
use crate::library::{Library, PlaySession, PlayStats};
use crate::net::{CratesIoClient, Endpoints, GitHubClient, HttpClient};
use crate::paths::{AppPaths, read_json, write_json_atomic};
use crate::platform::{Platform, Tools};
use crate::registry::{InstallRecord, Registry};

/// Environment variable naming an extra directory of manifests layered over the catalog.
pub const CATALOG_DIR_ENV: &str = "RUSTARCADE_CATALOG_DIR";
/// Set to `0` to disable the embedded catalog (tests).
pub const BUILTIN_CATALOG_ENV: &str = "RUSTARCADE_BUILTIN_CATALOG";
/// Set to `1` to allow loopback HTTP and `file://` sources (tests only).
pub const INSECURE_LOCAL_ENV: &str = "RUSTARCADE_ALLOW_INSECURE_LOCAL";
/// Set to `1` to skip all network access.
pub const OFFLINE_ENV: &str = "RUSTARCADE_OFFLINE";

/// How to open the application core.
#[derive(Debug, Clone, Default)]
pub struct OpenOptions {
    pub root: Option<PathBuf>,
    pub catalog_dir: Option<PathBuf>,
    pub use_builtin_catalog: Option<bool>,
    pub tools: Option<Tools>,
    pub endpoints: Option<Endpoints>,
    pub allow_insecure_local: Option<bool>,
    pub offline: bool,
}

impl OpenOptions {
    /// Read overrides from the environment.
    pub fn from_env() -> OpenOptions {
        let env_flag = |name: &str| {
            std::env::var(name)
                .ok()
                .map(|v| matches!(v.trim(), "1" | "true" | "yes"))
        };
        OpenOptions {
            root: None,
            catalog_dir: std::env::var_os(CATALOG_DIR_ENV)
                .map(PathBuf::from)
                .filter(|p| !p.as_os_str().is_empty()),
            use_builtin_catalog: env_flag(BUILTIN_CATALOG_ENV),
            tools: None,
            endpoints: Some(Endpoints::from_env()),
            allow_insecure_local: env_flag(INSECURE_LOCAL_ENV),
            offline: env_flag(OFFLINE_ENV).unwrap_or(false),
        }
    }
}

/// Lifecycle state of a game as shown in the UI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case", tag = "state", content = "reason")]
pub enum GameState {
    Available,
    Installing,
    Installed,
    UpdateAvailable,
    Running,
    Broken(String),
    Unsupported(String),
}

impl GameState {
    pub fn label(&self) -> &'static str {
        match self {
            GameState::Available => "Available",
            GameState::Installing => "Installing",
            GameState::Installed => "Installed",
            GameState::UpdateAvailable => "Update available",
            GameState::Running => "Running",
            GameState::Broken(_) => "Broken",
            GameState::Unsupported(_) => "Unsupported",
        }
    }

    pub fn is_installed(&self) -> bool {
        matches!(
            self,
            GameState::Installed
                | GameState::UpdateAvailable
                | GameState::Running
                | GameState::Broken(_)
        )
    }
}

/// A manifest plus everything the UI needs to display it.
#[derive(Debug, Clone)]
pub struct GameView {
    pub manifest: Arc<GameManifest>,
    pub origin: CatalogOrigin,
    pub state: GameState,
    pub install: Option<InstallRecord>,
    pub favorite: bool,
    pub stats: PlayStats,
    pub latest_version: Option<String>,
}

impl GameView {
    pub fn id(&self) -> &GameId {
        &self.manifest.id
    }

    pub fn installed_version(&self) -> Option<&str> {
        self.install.as_ref().map(|r| r.version.as_str())
    }
}

/// List filter.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GameFilter {
    pub query: Option<String>,
    pub category: Option<Category>,
    pub installed_only: bool,
    pub favorites_only: bool,
    pub include_experimental: bool,
}

/// Cached result of update checks.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct UpdateCache {
    checked_at: Option<DateTime<Utc>>,
    #[serde(default)]
    entries: BTreeMap<String, UpdateCheckEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct UpdateCheckEntry {
    installed: String,
    latest: String,
    available: bool,
    installer: InstallerKind,
    checked_at: DateTime<Utc>,
}

/// Outcome of an update check across games.
#[derive(Debug, Clone, Default)]
pub struct UpdateReport {
    pub checks: Vec<UpdateCheck>,
    pub errors: Vec<(GameId, String)>,
}

impl UpdateReport {
    pub fn available(&self) -> Vec<&UpdateCheck> {
        self.checks.iter().filter(|c| c.available).collect()
    }
}

/// State of the remote catalog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CatalogStatus {
    BuiltinOnly,
    Cached {
        fetched_at: DateTime<Utc>,
        games: usize,
    },
    Refreshing,
    Failed {
        reason: String,
        cached: bool,
    },
    Updated {
        added: usize,
        updated: usize,
        removed: usize,
    },
}

impl CatalogStatus {
    pub fn label(&self) -> String {
        match self {
            CatalogStatus::BuiltinOnly => "catalog: built-in".into(),
            CatalogStatus::Cached { fetched_at, games } => {
                format!(
                    "catalog: {games} remote games (fetched {})",
                    crate::library::format_relative(*fetched_at, Utc::now())
                )
            }
            CatalogStatus::Refreshing => "catalog: refreshing…".into(),
            CatalogStatus::Failed { reason, cached } => {
                if *cached {
                    format!("catalog: offline, using cache ({reason})")
                } else {
                    format!("catalog: offline, using built-in ({reason})")
                }
            }
            CatalogStatus::Updated {
                added,
                updated,
                removed,
            } => {
                format!("catalog: updated (+{added} ~{updated} -{removed})")
            }
        }
    }
}

/// The application core.
pub struct Services {
    paths: AppPaths,
    platform: Platform,
    tools: Tools,
    config: RwLock<Config>,
    catalog: RwLock<Catalog>,
    registry: Mutex<Registry>,
    library: Mutex<Library>,
    update_cache: Mutex<UpdateCache>,
    http: HttpClient,
    github: GitHubClient,
    crates: CratesIoClient,
    endpoints: Endpoints,
    active_jobs: Mutex<HashSet<GameId>>,
    running: Mutex<Option<GameId>>,
    job_counter: AtomicU64,
    catalog_status: Mutex<CatalogStatus>,
    startup_notes: Vec<String>,
    sweep: SweepReport,
    interrupt: InterruptFlag,
    offline: bool,
    catalog_dir: Option<PathBuf>,
    use_builtin: bool,
}

impl std::fmt::Debug for Services {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Services")
            .field("paths", &self.paths)
            .field("platform", &self.platform)
            .field("offline", &self.offline)
            .finish_non_exhaustive()
    }
}

fn lock<'a, T>(m: &'a Mutex<T>, what: &str) -> Result<std::sync::MutexGuard<'a, T>, Error> {
    m.lock().map_err(|_| {
        Error::Storage(StorageError::Corrupt {
            path: PathBuf::from(what),
            reason: "lock poisoned".into(),
        })
    })
}

impl Services {
    /// Open (or initialise) the application state. Never fails because of the network.
    pub fn open(opts: OpenOptions) -> Result<Arc<Services>, Error> {
        let paths = AppPaths::discover(opts.root.clone())?;
        paths.ensure()?;
        let platform = Platform::current()?;
        let tools = opts.tools.clone().unwrap_or_else(Tools::detect);
        let config = Config::load(&paths.config_file())?;
        let endpoints = opts.endpoints.clone().unwrap_or_else(Endpoints::from_env);
        let allow_insecure_local = opts.allow_insecure_local.unwrap_or(false);
        let http = HttpClient::new(&config.network, allow_insecure_local)?;
        let github = GitHubClient::new(
            http.clone(),
            &endpoints.github_api,
            Some(paths.cache_dir().join("github")),
        );
        let crates = CratesIoClient::new(http.clone(), &endpoints.crates_io);

        let mut notes = Vec::new();
        let (registry, quarantined) = Registry::load(&paths.registry_file())?;
        if let Some(q) = quarantined {
            notes.push(format!(
                "registry.json was corrupt and moved to {}",
                q.display()
            ));
        }
        let (library, quarantined) = Library::load(&paths.library_file(), &paths.history_file())?;
        for q in quarantined {
            notes.push(format!("{} was corrupt and moved aside", q.display()));
        }
        let update_cache = read_json::<UpdateCache>(&paths.update_cache_file())
            .ok()
            .flatten()
            .unwrap_or_default();
        let sweep = install::transaction::sweep(&paths);
        for p in &sweep.restored {
            notes.push(format!("restored interrupted update at {}", p.display()));
        }

        let use_builtin = opts.use_builtin_catalog.unwrap_or(true);
        let (catalog, status) =
            Self::load_catalog(&paths, use_builtin, opts.catalog_dir.as_deref(), &mut notes)?;

        Ok(Arc::new(Services {
            paths,
            platform,
            tools,
            config: RwLock::new(config),
            catalog: RwLock::new(catalog),
            registry: Mutex::new(registry),
            library: Mutex::new(library),
            update_cache: Mutex::new(update_cache),
            http,
            github,
            crates,
            endpoints,
            active_jobs: Mutex::new(HashSet::new()),
            running: Mutex::new(None),
            job_counter: AtomicU64::new(1),
            catalog_status: Mutex::new(status),
            startup_notes: notes,
            sweep,
            interrupt: InterruptFlag::new(),
            offline: opts.offline,
            catalog_dir: opts.catalog_dir,
            use_builtin,
        }))
    }

    fn load_catalog(
        paths: &AppPaths,
        use_builtin: bool,
        catalog_dir: Option<&Path>,
        notes: &mut Vec<String>,
    ) -> Result<(Catalog, CatalogStatus), Error> {
        let mut catalog = if use_builtin {
            Catalog::builtin()?
        } else {
            Catalog::empty()
        };
        let mut status = CatalogStatus::BuiltinOnly;
        let cached_games = paths
            .catalog_cache_dir()
            .join(crate::catalog::index::GAMES_DIR);
        if cached_games.is_dir() {
            match Catalog::from_dir(&cached_games, CatalogOrigin::Remote, ValidationMode::Strict) {
                Ok(remote_catalog) => {
                    let games = remote_catalog.len();
                    catalog = catalog.merge(remote_catalog);
                    if let Some(meta) = remote::read_meta(&paths.catalog_meta_file()) {
                        status = CatalogStatus::Cached {
                            fetched_at: meta.fetched_at,
                            games,
                        };
                    }
                }
                Err(e) => notes.push(format!("cached remote catalog ignored: {e}")),
            }
        }
        if let Some(dir) = catalog_dir {
            let mode = ValidationMode::AllowLocalSources;
            let local = Catalog::from_dir(dir, CatalogOrigin::Local(dir.to_path_buf()), mode)?;
            catalog = catalog.merge(local);
        }
        Ok((catalog, status))
    }

    // ----- accessors -------------------------------------------------------------

    pub fn paths(&self) -> &AppPaths {
        &self.paths
    }
    pub fn platform(&self) -> Platform {
        self.platform
    }
    pub fn tools(&self) -> &Tools {
        &self.tools
    }
    pub fn endpoints(&self) -> &Endpoints {
        &self.endpoints
    }
    pub fn http(&self) -> &HttpClient {
        &self.http
    }
    pub fn interrupt(&self) -> &InterruptFlag {
        &self.interrupt
    }
    pub fn offline(&self) -> bool {
        self.offline
    }
    pub fn startup_notes(&self) -> &[String] {
        &self.startup_notes
    }
    pub fn sweep_report(&self) -> &SweepReport {
        &self.sweep
    }

    pub fn config(&self) -> Config {
        self.config.read().map(|c| c.clone()).unwrap_or_default()
    }

    pub fn save_config(&self, config: Config) -> Result<(), Error> {
        config.save(&self.paths.config_file())?;
        if let Ok(mut c) = self.config.write() {
            *c = config;
        }
        Ok(())
    }

    pub fn catalog_status(&self) -> CatalogStatus {
        self.catalog_status
            .lock()
            .map(|s| s.clone())
            .unwrap_or(CatalogStatus::BuiltinOnly)
    }

    fn next_job_id(&self) -> JobId {
        self.job_counter.fetch_add(1, Ordering::Relaxed)
    }

    // ----- catalog queries ---------------------------------------------------------

    /// Resolve user input (`chess-tui`) into a known catalog id.
    pub fn resolve_id(&self, raw: &str) -> Result<GameId, Error> {
        let id = GameId::new(raw.trim()).map_err(|_| CatalogError::UnknownGame(raw.to_string()))?;
        let catalog = self
            .catalog
            .read()
            .map_err(|_| CatalogError::UnknownGame(raw.to_string()))?;
        if catalog.get(&id).is_some() {
            return Ok(id);
        }
        // Allow uninstall/play of games that are installed but no longer in the catalog.
        if let Ok(reg) = self.registry.lock()
            && reg.contains(&id)
        {
            return Ok(id);
        }
        Err(CatalogError::UnknownGame(raw.to_string()).into())
    }

    pub fn manifest(&self, id: &GameId) -> Result<Arc<GameManifest>, Error> {
        let catalog = self
            .catalog
            .read()
            .map_err(|_| CatalogError::UnknownGame(id.to_string()))?;
        catalog
            .get(id)
            .map(|e| e.manifest.clone())
            .ok_or_else(|| CatalogError::UnknownGame(id.to_string()).into())
    }

    pub fn catalog_len(&self) -> usize {
        self.catalog.read().map(|c| c.len()).unwrap_or(0)
    }

    pub fn categories(&self) -> Vec<(Category, usize)> {
        self.catalog
            .read()
            .map(|c| c.categories())
            .unwrap_or_default()
    }

    /// Compute the state of a game from registry, jobs and update cache.
    pub fn state_of(&self, id: &GameId) -> GameState {
        let Ok(manifest) = self.manifest(id) else {
            return GameState::Unsupported("not in the catalog".into());
        };
        if !manifest.supports(&self.platform) {
            return GameState::Unsupported(format!(
                "not available on {}",
                self.platform.os.label()
            ));
        }
        if self
            .running
            .lock()
            .ok()
            .is_some_and(|r| r.as_ref() == Some(id))
        {
            return GameState::Running;
        }
        if self.active_jobs.lock().ok().is_some_and(|j| j.contains(id)) {
            return GameState::Installing;
        }
        let record = self.registry.lock().ok().and_then(|r| r.get(id).cloned());
        match record {
            None => GameState::Available,
            Some(record) => {
                let exe = record.executable_path(&self.paths);
                if self.paths.ensure_managed(&exe).is_err() {
                    return GameState::Broken(
                        "registered executable is outside the managed directory".into(),
                    );
                }
                if !exe.is_file() {
                    return GameState::Broken(format!("executable missing: {}", exe.display()));
                }
                let update = self
                    .update_cache
                    .lock()
                    .ok()
                    .and_then(|c| {
                        c.entries
                            .get(id.as_str())
                            .map(|e| e.available && e.installed == record.version)
                    })
                    .unwrap_or(false);
                if update {
                    GameState::UpdateAvailable
                } else {
                    GameState::Installed
                }
            }
        }
    }

    fn view(&self, entry: &crate::catalog::CatalogEntry) -> GameView {
        let id = &entry.manifest.id;
        let install = self.registry.lock().ok().and_then(|r| r.get(id).cloned());
        let (favorite, stats) = self
            .library
            .lock()
            .map(|l| (l.is_favorite(id), l.stats(id)))
            .unwrap_or((false, PlayStats::default()));
        let latest_version = self
            .update_cache
            .lock()
            .ok()
            .and_then(|c| c.entries.get(id.as_str()).map(|e| e.latest.clone()));
        GameView {
            manifest: entry.manifest.clone(),
            origin: entry.origin.clone(),
            state: self.state_of(id),
            install,
            favorite,
            stats,
            latest_version,
        }
    }

    /// Every catalog game, sorted by name.
    pub fn games(&self) -> Vec<GameView> {
        let Ok(catalog) = self.catalog.read() else {
            return Vec::new();
        };
        let mut views: Vec<GameView> = catalog.iter().map(|e| self.view(e)).collect();
        views.sort_by(|a, b| {
            a.manifest
                .name
                .to_lowercase()
                .cmp(&b.manifest.name.to_lowercase())
        });
        views
    }

    pub fn game(&self, id: &GameId) -> Result<GameView, Error> {
        let catalog = self
            .catalog
            .read()
            .map_err(|_| CatalogError::UnknownGame(id.to_string()))?;
        let entry = catalog
            .get(id)
            .ok_or_else(|| CatalogError::UnknownGame(id.to_string()))?;
        Ok(self.view(entry))
    }

    /// Filtered, ranked list.
    pub fn list(&self, filter: &GameFilter) -> Vec<GameView> {
        let mut views = self.games();
        views.retain(|v| {
            if !filter.include_experimental
                && matches!(
                    v.manifest.support_status,
                    SupportStatus::Experimental | SupportStatus::Broken | SupportStatus::Archived
                )
                && !v.state.is_installed()
            {
                return false;
            }
            if filter.installed_only && !v.state.is_installed() {
                return false;
            }
            if filter.favorites_only && !v.favorite {
                return false;
            }
            if let Some(c) = filter.category
                && !v.manifest.categories.contains(&c)
            {
                return false;
            }
            true
        });
        if let Some(q) = filter
            .query
            .as_deref()
            .map(str::trim)
            .filter(|q| !q.is_empty())
        {
            let mut ranked: Vec<(u8, GameView)> = views
                .into_iter()
                .filter_map(|v| crate::catalog::search::rank(&v.manifest, q).map(|r| (r, v)))
                .collect();
            ranked.sort_by(|a, b| {
                a.0.cmp(&b.0).then_with(|| {
                    a.1.manifest
                        .name
                        .to_lowercase()
                        .cmp(&b.1.manifest.name.to_lowercase())
                })
            });
            return ranked.into_iter().map(|(_, v)| v).collect();
        }
        views
    }

    pub fn search(&self, query: &str) -> Vec<GameView> {
        self.list(&GameFilter {
            query: Some(query.to_string()),
            include_experimental: true,
            ..GameFilter::default()
        })
    }

    pub fn installed(&self) -> Vec<GameView> {
        self.games()
            .into_iter()
            .filter(|v| v.state.is_installed())
            .collect()
    }

    // ----- library -----------------------------------------------------------------

    pub fn toggle_favorite(&self, id: &GameId) -> Result<bool, Error> {
        Ok(lock(&self.library, "library")?.toggle_favorite(id)?)
    }

    pub fn is_favorite(&self, id: &GameId) -> bool {
        self.library
            .lock()
            .map(|l| l.is_favorite(id))
            .unwrap_or(false)
    }

    pub fn favorites(&self) -> Vec<GameView> {
        self.games().into_iter().filter(|v| v.favorite).collect()
    }

    /// Recently played games (most recent first), skipping ones no longer in the catalog.
    pub fn recent(&self, limit: usize) -> Vec<GameView> {
        let ids = self
            .library
            .lock()
            .map(|l| l.recent(limit))
            .unwrap_or_default();
        ids.iter().filter_map(|id| self.game(id).ok()).collect()
    }

    pub fn stats(&self, id: &GameId) -> PlayStats {
        self.library.lock().map(|l| l.stats(id)).unwrap_or_default()
    }

    pub fn history(&self, id: Option<&GameId>) -> Vec<PlaySession> {
        self.library
            .lock()
            .map(|l| l.sessions(id).into_iter().cloned().collect())
            .unwrap_or_default()
    }

    pub fn total_play_time(&self) -> std::time::Duration {
        self.library
            .lock()
            .map(|l| l.total_play_time())
            .unwrap_or_default()
    }

    // ----- install / update / uninstall ---------------------------------------------

    fn installed_record(&self, id: &GameId) -> Option<InstallRecord> {
        self.registry.lock().ok().and_then(|r| r.get(id).cloned())
    }

    fn ensure_idle(&self, id: &GameId) -> Result<(), Error> {
        if self
            .running
            .lock()
            .ok()
            .is_some_and(|r| r.as_ref() == Some(id))
        {
            return Err(InstallError::GameRunning(id.to_string()).into());
        }
        if self.active_jobs.lock().ok().is_some_and(|j| j.contains(id)) {
            return Err(InstallError::JobInProgress(id.to_string()).into());
        }
        Ok(())
    }

    /// Build the plan shown before an installation (or reinstall/update).
    pub async fn plan_install(
        &self,
        id: &GameId,
        prefer: Option<InstallerKind>,
    ) -> Result<InstallPlan, Error> {
        let manifest = self.manifest(id)?;
        self.ensure_idle(id)?;
        if self.offline {
            return Err(
                Error::Network(crate::error::NetworkError::Offline).in_context(
                    "install",
                    &manifest.name,
                    None,
                ),
            );
        }
        let installed = self.installed_record(id);
        let config = self.config();
        let env = InstallEnv {
            paths: &self.paths,
            platform: self.platform,
            tools: &self.tools,
            http: &self.http,
            github: &self.github,
            crates: &self.crates,
            config: &config,
        };
        install::plan(&env, &manifest, prefer, installed.as_ref())
            .await
            .map_err(|e| e.in_context("plan the installation of", &manifest.name, None))
    }

    /// Execute a plan. Progress is streamed to `sink`; `cancel` aborts the job.
    pub async fn install(
        &self,
        plan: InstallPlan,
        sink: ProgressSink,
        cancel: CancellationToken,
    ) -> Result<InstallOutcome, Error> {
        let id = plan.game.clone();
        let manifest = self.manifest(&id)?;
        {
            self.ensure_idle(&id)?;
            lock(&self.active_jobs, "jobs")?.insert(id.clone());
        }
        let progress = Progress::new(self.next_job_id(), sink);
        let config = self.config();
        let env = InstallEnv {
            paths: &self.paths,
            platform: self.platform,
            tools: &self.tools,
            http: &self.http,
            github: &self.github,
            crates: &self.crates,
            config: &config,
        };
        let result =
            install::transaction::run(env, &manifest, &plan, progress, cancel, &self.registry)
                .await;
        if let Ok(mut jobs) = self.active_jobs.lock() {
            jobs.remove(&id);
        }
        if result.is_ok()
            && let Ok(mut cache) = self.update_cache.lock()
        {
            cache.entries.remove(id.as_str());
            let _ = write_json_atomic(&self.paths.update_cache_file(), &*cache);
        }
        result
    }

    /// Check for updates. Results are cached for `updates.cache_hours` unless `force`.
    pub async fn check_updates(&self, ids: Option<&[GameId]>, force: bool) -> UpdateReport {
        let mut report = UpdateReport::default();
        let records: Vec<InstallRecord> = self
            .registry
            .lock()
            .map(|r| {
                r.iter()
                    .filter(|rec| ids.is_none_or(|ids| ids.contains(&rec.id)))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        let cache_hours = self.config().updates.cache_hours;
        let now = Utc::now();
        for record in records {
            let cached = self
                .update_cache
                .lock()
                .ok()
                .and_then(|c| c.entries.get(record.id.as_str()).cloned());
            if !force
                && let Some(entry) = &cached
                && entry.installed == record.version
                && (now - entry.checked_at).num_seconds()
                    < (cache_hours.saturating_mul(3600)) as i64
            {
                report.checks.push(UpdateCheck {
                    game: record.id.clone(),
                    installed: entry.installed.clone(),
                    latest: entry.latest.clone(),
                    available: entry.available,
                    installer: entry.installer,
                });
                continue;
            }
            if self.offline {
                report
                    .errors
                    .push((record.id.clone(), "offline mode".into()));
                continue;
            }
            let Ok(manifest) = self.manifest(&record.id) else {
                report
                    .errors
                    .push((record.id.clone(), "no longer in the catalog".into()));
                continue;
            };
            let config = self.config();
            let env = InstallEnv {
                paths: &self.paths,
                platform: self.platform,
                tools: &self.tools,
                http: &self.http,
                github: &self.github,
                crates: &self.crates,
                config: &config,
            };
            match install::check_latest(&env, &manifest, &record).await {
                Ok(check) => {
                    if let Ok(mut cache) = self.update_cache.lock() {
                        cache.entries.insert(
                            record.id.to_string(),
                            UpdateCheckEntry {
                                installed: check.installed.clone(),
                                latest: check.latest.clone(),
                                available: check.available,
                                installer: check.installer,
                                checked_at: now,
                            },
                        );
                        cache.checked_at = Some(now);
                        let _ = write_json_atomic(&self.paths.update_cache_file(), &*cache);
                    }
                    report.checks.push(check);
                }
                Err(e) => report.errors.push((record.id.clone(), e.to_string())),
            }
        }
        report
    }

    /// Plan an update; `Ok(None)` when the game is already current.
    pub async fn plan_update(
        &self,
        id: &GameId,
        force: bool,
    ) -> Result<Option<InstallPlan>, Error> {
        let record = self
            .installed_record(id)
            .ok_or_else(|| InstallError::NotInstalled(id.to_string()))?;
        let report = self
            .check_updates(Some(std::slice::from_ref(id)), true)
            .await;
        if let Some((_, reason)) = report.errors.first() {
            return Err(InstallError::VersionUnknown {
                detail: reason.clone(),
            }
            .into());
        }
        let available = report.checks.first().is_some_and(|c| c.available);
        if !available && !force {
            return Ok(None);
        }
        let plan = self.plan_install(id, Some(record.installer)).await;
        let plan = match plan {
            Ok(p) => p,
            Err(_) => self.plan_install(id, None).await?,
        };
        Ok(Some(plan))
    }

    /// Describe what an uninstall would remove.
    pub fn uninstall_paths(&self, id: &GameId) -> Result<Vec<PathBuf>, Error> {
        let record = self
            .installed_record(id)
            .ok_or_else(|| InstallError::NotInstalled(id.to_string()))?;
        Ok(record
            .managed_paths(&self.paths)
            .into_iter()
            .map(|(p, _)| p)
            .collect())
    }

    pub fn uninstall(&self, id: &GameId) -> Result<UninstallReport, Error> {
        self.ensure_idle(id)?;
        let report = install::transaction::uninstall(&self.paths, &self.registry, id)?;
        if let Ok(mut cache) = self.update_cache.lock() {
            cache.entries.remove(id.as_str());
            let _ = write_json_atomic(&self.paths.update_cache_file(), &*cache);
        }
        Ok(report)
    }

    // ----- play ---------------------------------------------------------------------

    /// Build the launch specification for an installed game.
    pub fn launch_spec(&self, id: &GameId) -> Result<LaunchSpec, Error> {
        let record = self
            .installed_record(id)
            .ok_or_else(|| LaunchError::NotInstalled(id.to_string()))?;
        let exe = record.executable_path(&self.paths);
        self.paths.ensure_managed(&exe)?;
        let manifest = self.manifest(id).ok();
        let (args, env, cwd) = match &manifest {
            Some(m) => {
                let cwd = match m.run.cwd {
                    RunCwd::Current => None,
                    RunCwd::Install => Some(
                        self.paths
                            .game_dir(id.as_str())
                            .join(install::transaction::CURRENT_DIR),
                    ),
                    RunCwd::Home => std::env::home_dir(),
                };
                (
                    m.run.args.clone(),
                    m.run
                        .env
                        .iter()
                        .map(|(k, v)| (k.clone(), v.clone()))
                        .collect(),
                    cwd,
                )
            }
            None => (vec![], vec![], None),
        };
        Ok(LaunchSpec {
            game: id.clone(),
            executable: exe,
            args,
            env,
            cwd,
        })
    }

    /// Launch a game synchronously (must run on the thread that owns the terminal).
    pub fn play(
        &self,
        id: &GameId,
        session: &mut dyn TerminalSession,
    ) -> Result<LaunchResult, Error> {
        self.ensure_idle(id)?;
        let spec = self.launch_spec(id)?;
        if !spec.executable.is_file() {
            return Err(LaunchError::MissingExecutable {
                path: spec.executable,
            }
            .into());
        }
        if let Ok(mut r) = self.running.lock() {
            *r = Some(id.clone());
        }
        let result = launcher::launch(&spec, session, &self.interrupt);
        if let Ok(mut r) = self.running.lock() {
            *r = None;
        }
        let result = result?;
        lock(&self.library, "library")?.record_session(result.session())?;
        Ok(result)
    }

    // ----- catalog maintenance ------------------------------------------------------

    /// Fetch the remote catalog if the cache is stale (or `force`). `Ok(None)` = skipped.
    pub async fn refresh_catalog(
        &self,
        force: bool,
    ) -> Result<Option<CatalogRefreshReport>, Error> {
        let config = self.config();
        let meta: Option<CatalogMeta> = remote::read_meta(&self.paths.catalog_meta_file());
        if !force && !remote::is_stale(meta.as_ref(), config.catalog.refresh_hours) {
            return Ok(None);
        }
        if self.offline {
            return Ok(None);
        }
        if let Ok(mut s) = self.catalog_status.lock() {
            *s = CatalogStatus::Refreshing;
        }
        let result = remote::fetch_remote(
            &self.http,
            &config.catalog.remote_url,
            &self.paths.catalog_cache_dir(),
            &self.paths.catalog_meta_file(),
        )
        .await;
        match result {
            Ok(report) => {
                let mut notes = Vec::new();
                let (catalog, _) = Self::load_catalog(
                    &self.paths,
                    self.use_builtin,
                    self.catalog_dir.as_deref(),
                    &mut notes,
                )?;
                if let Ok(mut c) = self.catalog.write() {
                    *c = catalog;
                }
                if let Ok(mut s) = self.catalog_status.lock() {
                    *s = if report.unchanged() {
                        CatalogStatus::Cached {
                            fetched_at: Utc::now(),
                            games: report.fetched,
                        }
                    } else {
                        CatalogStatus::Updated {
                            added: report.added.len(),
                            updated: report.updated.len(),
                            removed: report.removed.len(),
                        }
                    };
                }
                Ok(Some(report))
            }
            Err(e) => {
                if let Ok(mut s) = self.catalog_status.lock() {
                    *s = CatalogStatus::Failed {
                        reason: e.to_string(),
                        cached: meta.is_some(),
                    };
                }
                Err(e.into())
            }
        }
    }

    /// Validate manifests at `path` (file or directory), or the embedded catalog.
    pub fn validate_catalog(path: Option<&Path>) -> Result<LoadReport, Error> {
        match path {
            Some(p) => Ok(crate::catalog::load_path(p, ValidationMode::Strict)?),
            None => Ok(crate::catalog::load_builtin()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn services(root: &Path) -> Arc<Services> {
        Services::open(OpenOptions {
            root: Some(root.to_path_buf()),
            tools: Some(Tools::none()),
            endpoints: Some(Endpoints::default()),
            allow_insecure_local: Some(true),
            offline: true,
            ..OpenOptions::default()
        })
        .unwrap()
    }

    #[test]
    fn opens_with_builtin_catalog_and_derives_states() {
        let root = tempfile::tempdir().unwrap();
        let s = services(root.path());
        assert!(s.catalog_len() >= 10);
        let id = s.resolve_id("chess-tui").unwrap();
        assert_eq!(s.state_of(&id), GameState::Available);
        assert!(s.resolve_id("no-such-game").is_err());
        let view = s.game(&id).unwrap();
        assert!(!view.favorite);
        assert!(s.toggle_favorite(&id).unwrap());
        assert_eq!(s.favorites().len(), 1);
        assert!(
            s.list(&GameFilter {
                query: Some("chess".into()),
                include_experimental: true,
                ..Default::default()
            })
            .iter()
            .any(|v| v.id() == &id)
        );
        assert!(
            s.list(&GameFilter {
                installed_only: true,
                ..Default::default()
            })
            .is_empty()
        );
        assert!(s.search("zzzz-nothing").is_empty());
        assert_eq!(s.catalog_status(), CatalogStatus::BuiltinOnly);
        // Unsupported example: a linux/macos-only game viewed from its manifest.
        let cf = s.resolve_id("new-connect-four").unwrap();
        if s.platform().os == crate::platform::Os::Windows {
            assert!(matches!(s.state_of(&cf), GameState::Unsupported(_)));
        } else {
            assert_eq!(s.state_of(&cf), GameState::Available);
        }
    }

    #[test]
    fn broken_state_when_executable_missing() {
        let root = tempfile::tempdir().unwrap();
        let s = services(root.path());
        let id = s.resolve_id("chess-tui").unwrap();
        let mut record = crate::registry::tests::sample("chess-tui");
        record.id = id.clone();
        s.registry.lock().unwrap().upsert(record).unwrap();
        assert!(matches!(s.state_of(&id), GameState::Broken(_)));
        assert!(matches!(
            s.play(&id, &mut launcher::NoTerminalSession),
            Err(Error::Launch(LaunchError::MissingExecutable { .. }))
        ));
        let paths = s.uninstall_paths(&id).unwrap();
        assert_eq!(paths.len(), 2);
        s.uninstall(&id).unwrap();
        assert_eq!(s.state_of(&id), GameState::Available);
    }

    #[tokio::test]
    async fn plan_reports_offline_and_missing_tools() {
        let root = tempfile::tempdir().unwrap();
        let s = services(root.path());
        let id = s.resolve_id("snakeshell").unwrap();
        let err = s.plan_install(&id, None).await.unwrap_err();
        assert!(err.to_string().contains("offline"), "{err}");
        assert!(s.plan_update(&id, false).await.is_err());
        let report = s.check_updates(None, false).await;
        assert!(report.checks.is_empty());

        // Online but with no tools and an unreachable crates.io: every installer is skipped.
        let root2 = tempfile::tempdir().unwrap();
        let online = Services::open(OpenOptions {
            root: Some(root2.path().to_path_buf()),
            tools: Some(Tools::none()),
            endpoints: Some(Endpoints {
                github_api: "https://127.0.0.1:9".into(),
                crates_io: "https://127.0.0.1:9".into(),
            }),
            allow_insecure_local: Some(false),
            offline: false,
            ..OpenOptions::default()
        })
        .unwrap();
        let err = online.plan_install(&id, None).await.unwrap_err();
        let msg = err.user_message();
        assert!(
            msg.detail.contains("None of the installation methods"),
            "{msg}"
        );
        assert!(
            msg.causes.iter().any(|c| c.contains("cargo not found")),
            "{msg}"
        );
    }
}
