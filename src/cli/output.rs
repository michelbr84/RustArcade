//! Human and JSON output helpers for the CLI.

use std::io::{self, IsTerminal, Write};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use indicatif::{ProgressBar, ProgressStyle};
use serde::Serialize;
use unicode_width::UnicodeWidthStr;

use crate::doctor::{CheckStatus, DoctorReport};
use crate::error::Error;
use crate::install::{InstallPlan, Phase, ProgressCallback, ProgressEvent};
use crate::library::{format_duration, format_relative};
use crate::platform::Platform;
use crate::services::GameView;

/// JSON projection of a game.
#[derive(Debug, Serialize)]
pub struct GameJson {
    pub id: String,
    pub name: String,
    pub summary: String,
    pub repository: String,
    pub license: Option<String>,
    pub categories: Vec<String>,
    pub tags: Vec<String>,
    pub support_status: String,
    pub os: Vec<String>,
    pub installers: Vec<String>,
    pub state: crate::services::GameState,
    pub installed_version: Option<String>,
    pub latest_version: Option<String>,
    pub favorite: bool,
    pub sessions: usize,
    pub total_play_secs: u64,
    pub last_played: Option<String>,
}

impl GameJson {
    pub fn from_view(v: &GameView) -> GameJson {
        let m = &v.manifest;
        GameJson {
            id: m.id.to_string(),
            name: m.name.clone(),
            summary: m.summary.clone(),
            repository: m.repository.clone(),
            license: m.license.clone(),
            categories: m
                .categories
                .iter()
                .map(|c| c.as_str().to_string())
                .collect(),
            tags: m.tags.clone(),
            support_status: m.support_status.as_str().to_string(),
            os: m
                .compatibility
                .os
                .iter()
                .map(|o| o.as_str().to_string())
                .collect(),
            installers: m
                .installers
                .iter()
                .map(|i| i.kind().as_str().to_string())
                .collect(),
            state: v.state.clone(),
            installed_version: v.installed_version().map(String::from),
            latest_version: v.latest_version.clone(),
            favorite: v.favorite,
            sessions: v.stats.sessions,
            total_play_secs: v.stats.total.as_secs(),
            last_played: v.stats.last_played.map(|t| t.to_rfc3339()),
        }
    }
}

pub fn print_json<T: Serialize>(value: &T) {
    match serde_json::to_string_pretty(value) {
        Ok(s) => println!("{s}"),
        Err(e) => eprintln!("could not serialise output: {e}"),
    }
}

fn pad(s: &str, width: usize) -> String {
    let w = UnicodeWidthStr::width(s);
    if w >= width {
        s.to_string()
    } else {
        format!("{s}{}", " ".repeat(width - w))
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let cut: String = s.chars().take(max.saturating_sub(1)).collect();
        format!("{cut}…")
    }
}

/// Render a list of games as an aligned table.
pub fn print_table(views: &[GameView]) {
    let rows: Vec<[String; 5]> = views
        .iter()
        .map(|v| {
            let state = match &v.state {
                crate::services::GameState::Installed => {
                    format!("installed {}", v.installed_version().unwrap_or(""))
                }
                crate::services::GameState::UpdateAvailable => format!(
                    "update {} → {}",
                    v.installed_version().unwrap_or(""),
                    v.latest_version.clone().unwrap_or_default()
                ),
                other => other.label().to_lowercase(),
            };
            [
                v.manifest.id.to_string(),
                truncate(&v.manifest.name, 28),
                v.manifest
                    .categories
                    .iter()
                    .map(|c| c.as_str())
                    .collect::<Vec<_>>()
                    .join(", "),
                v.manifest.support_status.label().to_string(),
                state,
            ]
        })
        .collect();
    let headers = ["ID", "NAME", "CATEGORY", "SUPPORT", "STATE"];
    let mut widths = headers.map(|h| h.len());
    for row in &rows {
        for (i, cell) in row.iter().enumerate() {
            widths[i] = widths[i].max(UnicodeWidthStr::width(cell.as_str()));
        }
    }
    let line = |cells: &[String]| {
        cells
            .iter()
            .enumerate()
            .map(|(i, c)| pad(c, widths[i]))
            .collect::<Vec<_>>()
            .join("  ")
            .trim_end()
            .to_string()
    };
    println!("{}", line(&headers.map(String::from)));
    for row in &rows {
        println!("{}", line(row));
    }
    println!("\n{} game(s)", rows.len());
}

/// Render the details of one game.
pub fn print_info(v: &GameView, platform: Platform) {
    let m = &v.manifest;
    println!("{}  [{}]", m.name, m.id);
    println!("{}", m.summary);
    if let Some(d) = &m.description {
        println!();
        println!("{}", d.trim());
    }
    println!();
    let row = |k: &str, val: String| println!("{:<18}{}", k, val);
    row("Repository", m.repository.clone());
    if let Some(h) = &m.homepage {
        row("Homepage", h.clone());
    }
    row(
        "License",
        m.license.clone().unwrap_or_else(|| "unknown".into()),
    );
    row(
        "Categories",
        m.categories
            .iter()
            .map(|c| c.label())
            .collect::<Vec<_>>()
            .join(", "),
    );
    if !m.tags.is_empty() {
        row("Tags", m.tags.join(", "));
    }
    row(
        "Platforms",
        m.compatibility
            .os
            .iter()
            .map(|o| o.label())
            .collect::<Vec<_>>()
            .join(", "),
    );
    if let Some(t) = &m.compatibility.min_terminal {
        row("Min. terminal", format!("{}x{}", t.cols, t.rows));
    }
    row("Support status", m.support_status.label().to_string());
    row(
        "Installers",
        m.installers
            .iter()
            .map(|i| {
                let mut s = i.kind().label().to_string();
                if !i.applies_to(platform.os) {
                    s.push_str(" (other OS)");
                }
                s
            })
            .collect::<Vec<_>>()
            .join(", "),
    );
    row("State", v.state.label().to_string());
    if let Some(r) = &v.install {
        row("Installed version", r.version.clone());
        row(
            "Installed via",
            format!("{} — {}", r.installer.label(), r.source.describe()),
        );
        row("Executable", r.executable.display().to_string());
        row(
            "Installed on",
            r.installed_at.format("%Y-%m-%d").to_string(),
        );
        row(
            "Checksum",
            if r.checksum_verified {
                "verified".into()
            } else {
                "not verified".into()
            },
        );
    }
    if let Some(l) = &v.latest_version {
        row("Latest version", l.clone());
    }
    row(
        "Favorite",
        if v.favorite {
            "yes".into()
        } else {
            "no".into()
        },
    );
    if v.stats.sessions > 0 {
        row(
            "Play time",
            format!(
                "{} across {} session(s)",
                format_duration(v.stats.total),
                v.stats.sessions
            ),
        );
        if let Some(t) = v.stats.last_played {
            row("Last played", format_relative(t, chrono::Utc::now()));
        }
    }
    if !m.requirements.optional_commands.is_empty() {
        row(
            "Optional tools",
            m.requirements.optional_commands.join(", "),
        );
    }
    if let Some(n) = &m.requirements.notes {
        row("Requirements", n.clone());
    }
    if let Some(n) = &m.notes {
        println!();
        println!("Note: {n}");
    }
}

/// Render an installation plan.
pub fn print_plan(plan: &InstallPlan) {
    println!();
    println!(
        "{} {}",
        if plan.is_update { "Update" } else { "Install" },
        plan.name
    );
    println!("{:<18}{}", "Source", plan.source);
    println!("{:<18}{}", "Method", plan.installer.label());
    println!("{:<18}{}", "Version", plan.version_label());
    if let Some(a) = &plan.asset {
        println!("{:<18}{}", "Asset", a);
        println!("{:<18}{}", "Checksum", plan.checksum.label());
    }
    println!("{:<18}{}", "Destination", plan.destination.display());
    println!("{:<18}{}", "Executable", plan.executable);
    for t in &plan.tools {
        println!(
            "{:<18}{} {}",
            format!("Requires {}", t.name),
            if t.path.is_some() { "✓" } else { "✗" },
            t.version.clone().unwrap_or_else(|| "not found".into())
        );
    }
    if !plan.missing_commands.is_empty() {
        println!(
            "{:<18}{}",
            "Missing tools",
            plan.missing_commands.join(", ")
        );
    }
    if !plan.missing_optional.is_empty() {
        println!(
            "{:<18}{} (optional)",
            "Not installed",
            plan.missing_optional.join(", ")
        );
    }
    println!(
        "{:<18}{}",
        "Network",
        if plan.requires_network { "yes" } else { "no" }
    );
    println!(
        "{:<18}{}",
        "Compiles locally",
        if plan.compiles { "yes" } else { "no" }
    );
    println!(
        "{:<18}{}",
        "Administrator",
        if plan.requires_admin {
            "required"
        } else {
            "not required"
        }
    );
    for w in &plan.warnings {
        println!("{:<18}{}", "Note", w);
    }
    for s in &plan.skipped {
        println!("{:<18}{}", "Skipped", s);
    }
    println!(
        "{:<18}this game is third-party software; RustArcade does not audit or sandbox it.",
        "Third-party"
    );
    println!();
}

/// Ask a yes/no question on the terminal. Non-interactive sessions answer "no".
pub fn confirm(prompt: &str) -> Result<bool, Error> {
    if !io::stdin().is_terminal() {
        eprintln!("{prompt} [y/N] — not an interactive terminal; pass --yes to confirm.");
        return Ok(false);
    }
    print!("{prompt} [y/N] ");
    io::stdout().flush().ok();
    let mut line = String::new();
    io::stdin().read_line(&mut line).ok();
    Ok(matches!(
        line.trim().to_ascii_lowercase().as_str(),
        "y" | "yes"
    ))
}

/// Progress reporting for the CLI: a spinner that turns into a byte bar while downloading.
pub fn progress_sink() -> (ProgressCallback, ProgressBar) {
    let bar = ProgressBar::new_spinner();
    bar.set_style(
        ProgressStyle::with_template("{spinner} {msg}")
            .unwrap_or_else(|_| ProgressStyle::default_spinner()),
    );
    bar.enable_steady_tick(Duration::from_millis(100));
    let state = Arc::new(Mutex::new((bar.clone(), false)));
    let sink: ProgressCallback = Arc::new(move |event: &ProgressEvent| {
        let Ok(mut guard) = state.lock() else { return };
        let (bar, in_bytes) = &mut *guard;
        match event {
            ProgressEvent::Started { log, .. } => {
                bar.set_message(format!("Starting (log: {})", log.display()))
            }
            ProgressEvent::Phase { phase, detail, .. } => {
                if *in_bytes {
                    bar.set_style(
                        ProgressStyle::with_template("{spinner} {msg}")
                            .unwrap_or_else(|_| ProgressStyle::default_spinner()),
                    );
                    *in_bytes = false;
                }
                let msg = if detail.is_empty() {
                    phase.label().to_string()
                } else {
                    format!("{}: {detail}", phase.label())
                };
                bar.set_message(msg);
                if *phase == Phase::Ready {
                    bar.finish_with_message(format!("Ready: {detail}"));
                }
            }
            ProgressEvent::Bytes { done, total, .. } => {
                if let Some(total) = total {
                    if !*in_bytes {
                        bar.set_style(
                            ProgressStyle::with_template("{spinner} {msg} [{bar:30}] {bytes}/{total_bytes} ({bytes_per_sec})")
                                .unwrap_or_else(|_| ProgressStyle::default_bar()),
                        );
                        bar.set_length(*total);
                        *in_bytes = true;
                    }
                    bar.set_position(*done);
                } else {
                    bar.set_message(format!("Downloading {} bytes", done));
                }
            }
            ProgressEvent::Output { line, .. } => {
                if line.trim().starts_with("Compiling") || line.trim().starts_with("error") {
                    bar.set_message(line.trim().to_string());
                }
            }
            ProgressEvent::Finished {
                success, message, ..
            } => {
                if *success {
                    bar.finish_with_message(message.clone());
                } else {
                    bar.abandon_with_message(format!("Failed: {message}"));
                }
            }
        }
    });
    (sink, bar)
}

/// Render the doctor report.
pub fn print_doctor(report: &DoctorReport) {
    println!("RustArcade Doctor ({})", crate::VERSION);
    println!();
    let width = report
        .checks
        .iter()
        .map(|c| c.name.len())
        .max()
        .unwrap_or(10)
        + 2;
    for c in &report.checks {
        let mark = match c.status {
            CheckStatus::Ok => "✓",
            CheckStatus::Warning => "!",
            CheckStatus::Error => "✗",
        };
        println!(
            "{} {:<width$}{}  [{}]",
            mark,
            c.name,
            c.detail,
            c.status.label(),
            width = width
        );
        if let Some(fix) = &c.fix {
            println!("  {:<width$}→ {fix}", "", width = width);
        }
    }
    println!();
    let (w, e) = (
        report.count(CheckStatus::Warning),
        report.count(CheckStatus::Error),
    );
    match (w, e) {
        (0, 0) => println!("No problems found."),
        (w, 0) => println!("{w} warning(s), no errors."),
        (w, e) => println!("{e} error(s), {w} warning(s)."),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pad_and_truncate_are_width_aware() {
        assert_eq!(pad("ab", 4), "ab  ");
        assert_eq!(pad("abcd", 2), "abcd");
        assert_eq!(truncate("hello world", 5), "hell…");
        assert_eq!(truncate("hi", 5), "hi");
        assert_eq!(pad("日本", 6), "日本  ");
    }
}
