//! Command-line interface. Every command is a thin wrapper over [`Services`].

pub mod output;

use std::path::PathBuf;
use std::sync::Arc;

use clap::{Parser, Subcommand};
use tokio_util::sync::CancellationToken;

use crate::catalog::manifest::{Category, InstallerKind};
use crate::error::{Error, InstallError};
use crate::install::ProgressSink;
use crate::launcher::NoTerminalSession;
use crate::services::{GameFilter, OpenOptions, Services};

const LONG_ABOUT: &str = "RustArcade — Your terminal. Your arcade.

Discover, install, update, and play open-source terminal games from one place.
Run without a subcommand to open the interactive interface.";

#[derive(Parser, Debug)]
#[command(name = "rustarcade", version, about = "Your terminal. Your arcade.", long_about = LONG_ABOUT)]
pub struct Cli {
    /// Verbose logging (log lines are also mirrored to stderr).
    #[arg(long, global = true)]
    pub debug: bool,
    /// Keep all RustArcade data under DIR (same as RUSTARCADE_HOME).
    #[arg(long, global = true, value_name = "DIR")]
    pub home: Option<PathBuf>,
    /// Never use the network.
    #[arg(long, global = true)]
    pub offline: bool,
    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// List games in the catalog.
    List {
        /// Only installed games.
        #[arg(long)]
        installed: bool,
        /// Filter by category.
        #[arg(long)]
        category: Option<Category>,
        /// Include experimental games.
        #[arg(long)]
        all: bool,
        /// Output JSON.
        #[arg(long)]
        json: bool,
    },
    /// Search games by name, description, or tag.
    Search {
        query: String,
        #[arg(long)]
        json: bool,
    },
    /// Show details about a game.
    Info {
        id: String,
        #[arg(long)]
        json: bool,
    },
    /// Install a game.
    Install {
        id: String,
        /// Force a specific installation method.
        #[arg(long, value_enum)]
        method: Option<InstallerKind>,
        /// Skip the confirmation prompt.
        #[arg(short = 'y', long)]
        yes: bool,
        /// Launch the game after installing.
        #[arg(long)]
        play: bool,
    },
    /// Launch an installed game.
    Play { id: String },
    /// Update installed games.
    Update {
        id: Option<String>,
        /// Update every installed game.
        #[arg(long)]
        all: bool,
        /// Only report available updates.
        #[arg(long)]
        check: bool,
        /// Skip confirmation prompts.
        #[arg(short = 'y', long)]
        yes: bool,
    },
    /// Remove a game installed by RustArcade.
    Uninstall {
        id: String,
        #[arg(short = 'y', long)]
        yes: bool,
    },
    /// Diagnose the environment.
    Doctor {
        #[arg(long)]
        json: bool,
    },
    /// Manage the game catalog.
    Catalog {
        #[command(subcommand)]
        command: CatalogCommand,
    },
    /// Print the version.
    Version,
}

#[derive(Subcommand, Debug)]
pub enum CatalogCommand {
    /// Download the latest official catalog.
    Update {
        /// Fetch even if the cached copy is fresh.
        #[arg(long)]
        force: bool,
    },
    /// Validate manifests (a file, a directory, or the built-in catalog).
    Validate { path: Option<PathBuf> },
    /// Generate or check `catalog/index.json` (maintainers).
    Index {
        /// Fail if the index is out of date instead of rewriting it.
        #[arg(long)]
        check: bool,
        /// Catalog directory containing `games/`.
        #[arg(long, default_value = "catalog")]
        dir: PathBuf,
    },
}

/// Run a CLI command; returns the process exit code.
pub fn run(command: Command, opts: OpenOptions) -> i32 {
    if let Command::Version = command {
        println!("rustarcade {}", crate::VERSION);
        return 0;
    }
    if let Command::Catalog {
        command: CatalogCommand::Validate { path },
    } = &command
    {
        return validate(path.as_deref());
    }
    if let Command::Catalog {
        command: CatalogCommand::Index { check, dir },
    } = &command
    {
        return index(dir, *check);
    }
    let services = match Services::open(opts) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("{}", e.user_message());
            return e.exit_code();
        }
    };
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
    let result = runtime.block_on(dispatch(command, services));
    match result {
        Ok(code) => code,
        Err(e) => {
            eprintln!("{}", e.user_message());
            e.exit_code()
        }
    }
}

async fn dispatch(command: Command, services: Arc<Services>) -> Result<i32, Error> {
    match command {
        Command::Version
        | Command::Catalog {
            command: CatalogCommand::Validate { .. },
        }
        | Command::Catalog {
            command: CatalogCommand::Index { .. },
        } => Ok(0),
        Command::List {
            installed,
            category,
            all,
            json,
        } => {
            let views = services.list(&GameFilter {
                installed_only: installed,
                category,
                include_experimental: all
                    || installed
                    || services.config().general.show_experimental,
                ..GameFilter::default()
            });
            if json {
                output::print_json(
                    &views
                        .iter()
                        .map(output::GameJson::from_view)
                        .collect::<Vec<_>>(),
                );
            } else if views.is_empty() {
                println!("No games match.");
            } else {
                output::print_table(&views);
            }
            Ok(0)
        }
        Command::Search { query, json } => {
            let views = services.search(&query);
            if json {
                output::print_json(
                    &views
                        .iter()
                        .map(output::GameJson::from_view)
                        .collect::<Vec<_>>(),
                );
            } else if views.is_empty() {
                println!("No games match \"{query}\".");
            } else {
                output::print_table(&views);
            }
            Ok(0)
        }
        Command::Info { id, json } => {
            let id = services.resolve_id(&id)?;
            let view = services.game(&id)?;
            if json {
                output::print_json(&output::GameJson::from_view(&view));
            } else {
                output::print_info(&view, services.platform());
            }
            Ok(0)
        }
        Command::Install {
            id,
            method,
            yes,
            play,
        } => {
            let id = services.resolve_id(&id)?;
            let plan = services.plan_install(&id, method).await?;
            if plan.is_update && !plan.previous_version.is_none() {
                println!(
                    "{} is already installed (version {}); reinstalling.",
                    plan.name,
                    plan.previous_version.clone().unwrap_or_default()
                );
            }
            output::print_plan(&plan);
            if !yes && !output::confirm("Proceed with the installation?")? {
                println!("Cancelled.");
                return Ok(1);
            }
            let (sink, bar) = output::progress_sink();
            let outcome = services
                .install(plan, ProgressSink::Callback(sink), CancellationToken::new())
                .await;
            bar.finish_and_clear();
            let outcome = outcome?;
            println!(
                "✓ {} {} installed to {}",
                outcome.record.name,
                outcome.record.version,
                outcome.record.executable_path(services.paths()).display()
            );
            for w in &outcome.warnings {
                println!("  note: {w}");
            }
            if play {
                let result = services.play(&id, &mut NoTerminalSession)?;
                println!(
                    "Played {} for {} ({}).",
                    outcome.record.name,
                    crate::library::format_duration(result.duration),
                    result.exit.label()
                );
            }
            Ok(0)
        }
        Command::Play { id } => {
            let id = services.resolve_id(&id)?;
            let name = services
                .manifest(&id)
                .map(|m| m.name.clone())
                .unwrap_or_else(|_| id.to_string());
            let result = services.play(&id, &mut NoTerminalSession)?;
            println!(
                "Played {name} for {} ({}).",
                crate::library::format_duration(result.duration),
                result.exit.label()
            );
            Ok(if result.exit.success() { 0 } else { 1 })
        }
        Command::Update {
            id,
            all,
            check,
            yes,
        } => {
            let targets: Vec<crate::catalog::manifest::GameId> = match (&id, all) {
                (Some(id), _) => vec![services.resolve_id(id)?],
                (None, true) | (None, false) if check || all => services
                    .installed()
                    .iter()
                    .map(|v| v.id().clone())
                    .collect(),
                _ => {
                    eprintln!("Specify a game id, --all, or --check.");
                    return Ok(2);
                }
            };
            if targets.is_empty() {
                println!("No games are installed.");
                return Ok(0);
            }
            let report = services.check_updates(Some(&targets), true).await;
            for (game, reason) in &report.errors {
                eprintln!("{game}: could not check for updates: {reason}");
            }
            let available: Vec<_> = report.available().into_iter().cloned().collect();
            for c in &report.checks {
                if c.available {
                    println!("{}: {} → {}", c.game, c.installed, c.latest);
                } else {
                    println!("{}: {} is up to date", c.game, c.installed);
                }
            }
            if check || available.is_empty() {
                return Ok(if report.errors.is_empty() { 0 } else { 1 });
            }
            if !yes && !output::confirm(&format!("Update {} game(s)?", available.len()))? {
                println!("Cancelled.");
                return Ok(1);
            }
            let mut failures = 0;
            for c in available {
                match services.plan_update(&c.game, true).await {
                    Ok(Some(plan)) => {
                        println!("Updating {} ({} → {})…", plan.name, c.installed, c.latest);
                        let (sink, bar) = output::progress_sink();
                        let outcome = services
                            .install(plan, ProgressSink::Callback(sink), CancellationToken::new())
                            .await;
                        bar.finish_and_clear();
                        match outcome {
                            Ok(o) => println!("✓ {} is now {}", o.record.name, o.record.version),
                            Err(e) => {
                                failures += 1;
                                eprintln!("{}", e.user_message());
                            }
                        }
                    }
                    Ok(None) => println!("{}: already up to date", c.game),
                    Err(e) => {
                        failures += 1;
                        eprintln!("{}", e.user_message());
                    }
                }
            }
            Ok(if failures == 0 { 0 } else { 1 })
        }
        Command::Uninstall { id, yes } => {
            let id = services.resolve_id(&id)?;
            let paths = services.uninstall_paths(&id)?;
            println!("Uninstalling {id} will remove:");
            for p in &paths {
                println!("  {}", p.display());
            }
            println!("Game save files or configuration outside RustArcade are kept.");
            if !yes && !output::confirm("Proceed?")? {
                println!("Cancelled.");
                return Ok(1);
            }
            let report = services.uninstall(&id)?;
            println!(
                "✓ {} uninstalled ({} item(s) removed)",
                report.game,
                report.removed.len()
            );
            for w in &report.warnings {
                println!("  note: {w}");
            }
            Ok(0)
        }
        Command::Doctor { json } => {
            let report = crate::doctor::run(&services).await;
            if json {
                output::print_json(&report);
            } else {
                output::print_doctor(&report);
            }
            Ok(report.exit_code())
        }
        Command::Catalog {
            command: CatalogCommand::Update { force },
        } => {
            match services.refresh_catalog(force).await? {
                None => println!("Catalog is up to date (use --force to refresh anyway)."),
                Some(r) if r.unchanged() => {
                    println!("Catalog refreshed: {} games, no changes.", r.fetched)
                }
                Some(r) => {
                    println!(
                        "Catalog refreshed: {} games (+{} added, {} updated, {} removed).",
                        r.fetched,
                        r.added.len(),
                        r.updated.len(),
                        r.removed.len()
                    );
                    for a in &r.added {
                        println!("  + {a}");
                    }
                    for u in &r.updated {
                        println!("  ~ {u}");
                    }
                    for d in &r.removed {
                        println!("  - {d}");
                    }
                }
            }
            Ok(0)
        }
    }
}

fn validate(path: Option<&std::path::Path>) -> i32 {
    let report = match Services::validate_catalog(path) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("{}", e.user_message());
            return e.exit_code();
        }
    };
    for (file, manifest) in &report.ok {
        println!(
            "OK    {}  ({}, {})",
            file.display(),
            manifest.id,
            manifest.support_status
        );
    }
    for error in &report.errors {
        match error {
            crate::error::CatalogError::Invalid { file, problems } => {
                println!("ERROR {}", file.display());
                for p in problems {
                    println!("        {p}");
                }
            }
            other => println!("ERROR {other}"),
        }
    }
    println!(
        "{} valid, {} problem(s)",
        report.ok.len(),
        report.errors.len()
    );
    if report.is_clean() { 0 } else { 1 }
}

fn index(dir: &std::path::Path, check: bool) -> i32 {
    match crate::catalog::index::write_index(dir, check) {
        Ok(true) => {
            println!("{} is up to date.", dir.join("index.json").display());
            0
        }
        Ok(false) if check => {
            eprintln!(
                "{} is out of date; run `rustarcade catalog index` to regenerate it.",
                dir.join("index.json").display()
            );
            1
        }
        Ok(false) => {
            println!("Wrote {}.", dir.join("index.json").display());
            0
        }
        Err(e) => {
            eprintln!("{}", Error::from(e).user_message());
            1
        }
    }
}

/// Map an install error for a "nothing to do" case into a friendly exit code.
pub fn exit_code_for(err: &Error) -> i32 {
    match err {
        Error::Install(InstallError::Cancelled) => 1,
        other => other.exit_code(),
    }
}
