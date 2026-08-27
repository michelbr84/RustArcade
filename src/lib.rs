//! RustArcade — a terminal game launcher and catalog.
//!
//! The library exposes the shared core used by both the CLI and the TUI. See
//! `docs/ARCHITECTURE.md` for the module map.

#![forbid(unsafe_code)]

pub mod catalog;
pub mod cli;
pub mod config;
pub mod doctor;
pub mod error;
pub mod install;
pub mod launcher;
pub mod library;
pub mod logging;
pub mod net;
pub mod paths;
pub mod platform;
pub mod registry;
pub mod services;
pub mod tui;
pub mod version;

pub use error::{Error, Result};

/// Application name.
pub const APP_NAME: &str = "RustArcade";
/// Crate version.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
/// HTTP `User-Agent` used for every request (crates.io requires one).
pub const USER_AGENT: &str = concat!(
    "rustarcade/",
    env!("CARGO_PKG_VERSION"),
    " (+https://github.com/michelbr84/RustArcade)"
);
