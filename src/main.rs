//! RustArcade binary entry point: parses arguments, sets up logging, and starts either
//! the TUI (no subcommand) or a CLI command.

use clap::Parser;
use rustarcade::cli::{Cli, Command};
use rustarcade::logging::{self, LogMode};
use rustarcade::paths::AppPaths;
use rustarcade::services::OpenOptions;

fn main() {
    let cli = Cli::parse();
    let mut opts = OpenOptions::from_env();
    opts.root = cli.home.clone();
    opts.offline = opts.offline || cli.offline;

    let paths = match AppPaths::discover(opts.root.clone()) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    };
    let mode = if cli.command.is_none() {
        LogMode::Tui
    } else {
        LogMode::Cli
    };
    let _log_guard = match logging::init(&paths.logs_dir(), mode, cli.debug) {
        Ok(g) => Some(g),
        Err(e) => {
            eprintln!("warning: logging disabled: {e}");
            None
        }
    };
    tracing::info!("rustarcade {} starting", rustarcade::VERSION);

    let code = match cli.command {
        None => rustarcade::tui::run(opts),
        Some(Command::Version) => {
            println!("rustarcade {}", rustarcade::VERSION);
            0
        }
        Some(command) => rustarcade::cli::run(command, opts),
    };
    std::process::exit(code);
}
