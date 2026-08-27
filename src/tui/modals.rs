//! Modal dialogs drawn over the current screen.

use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Gauge, LineGauge, Paragraph, Wrap};

use crate::install::Phase;

use super::app::{Modal, TuiApp};
use super::widgets::{self, bytes_label, centered, modal_block};

pub fn render(frame: &mut Frame, app: &TuiApp) {
    let Some(modal) = &app.modal else { return };
    let theme = app.theme();
    let area = frame.area();
    match modal {
        Modal::Welcome => {
            let lines = vec![
                Line::from(Span::styled("Welcome to RustArcade", theme.title()))
                    .alignment(Alignment::Center),
                Line::from(""),
                Line::from("RustArcade lets you discover, install, update and play open-source"),
                Line::from("terminal games from their original projects. It never bundles games."),
                Line::from(""),
                Line::from("Before anything is installed you will always see:"),
                Line::from("  • the source repository and installation method"),
                Line::from("  • the version, destination and required tools"),
                Line::from("  • whether the download can be checksum-verified"),
                Line::from(""),
                Line::from("Games are third-party software. RustArcade does not audit or"),
                Line::from("sandbox them, and never needs administrator privileges."),
                Line::from(""),
                Line::from(vec![
                    Span::styled("Enter", theme.key()),
                    Span::raw(" continue   "),
                    Span::styled("?", theme.key()),
                    Span::raw(" keyboard help"),
                ])
                .alignment(Alignment::Center),
            ];
            draw_text(frame, area, &theme, "Welcome", lines, 70, 17);
        }
        Modal::Help => {
            let k = |key: &'static str, what: &'static str| {
                Line::from(vec![
                    Span::styled(format!("{key:<14}"), theme.key()),
                    Span::raw(what),
                ])
            };
            let lines = vec![
                k(
                    "Tab / 1-6",
                    "switch between Home, Discover, Library, Favorites, Updates, Settings",
                ),
                k(
                    "↑ ↓ / j k",
                    "move selection · PgUp/PgDn page · g/G first/last",
                ),
                k("Enter", "open details · on details: Play or Install & Play"),
                k("Esc", "back / clear search"),
                k("/", "search (Discover, Library, Favorites)"),
                k("c", "cycle category filter"),
                k("i", "install selected game"),
                k("p", "play selected game"),
                k("u", "update selected game · a update all (Updates)"),
                k("x", "uninstall selected game"),
                k("f", "toggle favorite"),
                k("l", "view the last installation log"),
                k("r", "refresh catalog / re-check updates"),
                k("?", "this help · q quit"),
                Line::from(""),
                Line::from(Span::styled(
                    "Games run inside this terminal; RustArcade returns when they exit.",
                    theme.muted(),
                )),
            ];
            draw_text(frame, area, &theme, "Keyboard shortcuts", lines, 78, 20);
        }
        Modal::Planning { id, .. } => {
            let name = app
                .view(id)
                .map(|v| v.manifest.name.clone())
                .unwrap_or_else(|| id.to_string());
            let lines = vec![
                Line::from(""),
                Line::from(format!(
                    "{} Resolving how to install {name}…",
                    widgets::spinner(app.tick / 2)
                ))
                .alignment(Alignment::Center),
                Line::from(""),
                Line::from(Span::styled(
                    "Looking up releases and crate versions.",
                    theme.muted(),
                ))
                .alignment(Alignment::Center),
            ];
            draw_text(frame, area, &theme, "Please wait", lines, 60, 8);
        }
        Modal::ConfirmInstall { plan, then_play } => {
            let row = |label: &str, value: String| {
                Line::from(vec![
                    Span::styled(format!("{label:<16}"), theme.muted()),
                    Span::raw(value),
                ])
            };
            let mut lines = vec![
                Line::from(Span::styled(
                    format!(
                        "{} {}",
                        if plan.is_update { "Update" } else { "Install" },
                        plan.name
                    ),
                    theme.bold(),
                )),
                Line::from(""),
                row("Source", plan.source.clone()),
                row("Method", plan.installer.label().to_string()),
                row(
                    "Version",
                    match &plan.previous_version {
                        Some(prev) if plan.is_update => {
                            format!("{prev} → {}", plan.version_label())
                        }
                        _ => plan.version_label(),
                    },
                ),
            ];
            if let Some(a) = &plan.asset {
                lines.push(row("Asset", a.clone()));
                lines.push(row("Checksum", plan.checksum.label()));
            }
            lines.push(row("Destination", plan.destination.display().to_string()));
            for t in &plan.tools {
                let (mark, style) = if t.path.is_some() {
                    ("✓", theme.ok())
                } else {
                    ("✗", theme.err())
                };
                lines.push(Line::from(vec![
                    Span::styled(
                        format!("{:<16}", format!("Requires {}", t.name)),
                        theme.muted(),
                    ),
                    Span::styled(mark, style),
                    Span::raw(format!(
                        " {}",
                        t.version.clone().unwrap_or_else(|| "not found".into())
                    )),
                ]));
            }
            if !plan.missing_commands.is_empty() {
                lines.push(row("Missing tools", plan.missing_commands.join(", ")));
            }
            if !plan.missing_optional.is_empty() {
                lines.push(row(
                    "Optional tools",
                    format!("{} (not installed)", plan.missing_optional.join(", ")),
                ));
            }
            lines.push(row(
                "Compiles",
                if plan.compiles {
                    "yes — this can take several minutes".into()
                } else {
                    "no (prebuilt binary)".into()
                },
            ));
            lines.push(row("Administrator", "not required".into()));
            for w in &plan.warnings {
                lines.push(Line::from(vec![
                    Span::styled("Note            ", theme.muted()),
                    Span::styled(w.clone(), theme.warn()),
                ]));
            }
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "Third-party software: RustArcade does not audit or sandbox this game.",
                theme.muted(),
            )));
            lines.push(Line::from(""));
            lines.push(
                Line::from(vec![
                    Span::styled("Enter", theme.key()),
                    Span::raw(if *then_play {
                        " install & play   "
                    } else {
                        " install   "
                    }),
                    Span::styled("Esc", theme.key()),
                    Span::raw(" cancel"),
                ])
                .alignment(Alignment::Center),
            );
            let height = (lines.len() as u16 + 2).min(area.height);
            draw_text(
                frame,
                area,
                &theme,
                if plan.is_update {
                    "Confirm update"
                } else {
                    "Confirm installation"
                },
                lines,
                84,
                height,
            );
        }
        Modal::ConfirmUpdateAll { ids } => {
            let mut lines = vec![
                Line::from(format!("Update {} game(s)?", ids.len())),
                Line::from(""),
            ];
            for id in ids.iter().take(12) {
                let name = app
                    .view(id)
                    .map(|v| v.manifest.name.clone())
                    .unwrap_or_else(|| id.to_string());
                lines.push(Line::from(format!("  • {name}")));
            }
            if ids.len() > 12 {
                lines.push(Line::from(format!("  … and {} more", ids.len() - 12)));
            }
            lines.push(Line::from(""));
            lines.push(
                Line::from(vec![
                    Span::styled("Enter", theme.key()),
                    Span::raw(" update all   "),
                    Span::styled("Esc", theme.key()),
                    Span::raw(" cancel"),
                ])
                .alignment(Alignment::Center),
            );
            let height = (lines.len() as u16 + 2).min(area.height);
            draw_text(frame, area, &theme, "Update all", lines, 60, height);
        }
        Modal::ConfirmUninstall { name, paths, .. } => {
            let mut lines = vec![
                Line::from(format!("Uninstall {name}?")),
                Line::from(""),
                Line::from(Span::styled("This will remove:", theme.muted())),
            ];
            for p in paths {
                lines.push(Line::from(format!("  {}", p.display())));
            }
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "Save files and settings the game keeps outside RustArcade are not touched.",
                theme.muted(),
            )));
            lines.push(Line::from(""));
            lines.push(
                Line::from(vec![
                    Span::styled("Enter", theme.key()),
                    Span::raw(" uninstall   "),
                    Span::styled("Esc", theme.key()),
                    Span::raw(" cancel"),
                ])
                .alignment(Alignment::Center),
            );
            let height = (lines.len() as u16 + 2).min(area.height);
            draw_text(frame, area, &theme, "Confirm uninstall", lines, 84, height);
        }
        Modal::Progress { id } => render_progress(frame, app, id),
        Modal::Error { message, retry } => {
            let mut lines: Vec<Line> =
                vec![Line::from(Span::styled(message.title.clone(), theme.err()))];
            lines.push(Line::from(""));
            for l in message.detail.lines() {
                lines.push(Line::from(l.to_string()));
            }
            if !message.causes.is_empty() {
                lines.push(Line::from(""));
                lines.push(Line::from(Span::styled("Possible causes:", theme.muted())));
                for c in &message.causes {
                    lines.push(Line::from(format!("  • {c}")));
                }
            }
            if let Some(h) = &message.hint {
                lines.push(Line::from(""));
                lines.push(Line::from(h.clone()));
            }
            if let Some(l) = &message.log {
                lines.push(Line::from(""));
                lines.push(Line::from(vec![
                    Span::styled("Log: ", theme.muted()),
                    Span::raw(l.display().to_string()),
                ]));
            }
            lines.push(Line::from(""));
            let mut spans = Vec::new();
            if retry.is_some() {
                spans.push(Span::styled("r", theme.key()));
                spans.push(Span::raw(" retry   "));
            }
            if message.log.is_some() {
                spans.push(Span::styled("l", theme.key()));
                spans.push(Span::raw(" view log   "));
            }
            spans.push(Span::styled("Esc", theme.key()));
            spans.push(Span::raw(" back"));
            lines.push(Line::from(spans).alignment(Alignment::Center));
            let height = (lines.len() as u16 + 4).min(area.height);
            draw_text(frame, area, &theme, "Error", lines, 84, height);
        }
        Modal::LogView {
            path,
            lines,
            scroll,
        } => {
            let rect = centered(
                area,
                area.width.saturating_sub(4).max(40),
                area.height.saturating_sub(2).max(10),
            );
            widgets::clear(frame, rect);
            let block = modal_block(
                &theme,
                format!(
                    "Log · {}",
                    path.file_name().and_then(|n| n.to_str()).unwrap_or("")
                ),
            );
            let inner = block.inner(rect);
            frame.render_widget(block, rect);
            let visible = inner.height as usize;
            let start = scroll
                .saturating_sub(visible.saturating_sub(1))
                .min(lines.len().saturating_sub(visible));
            let text: Vec<Line> = lines
                .iter()
                .skip(start)
                .take(visible)
                .map(|l| Line::from(l.clone()))
                .collect();
            frame.render_widget(Paragraph::new(text), inner);
        }
        Modal::Info { title, lines } => {
            let mut text: Vec<Line> = lines.iter().map(|l| Line::from(l.clone())).collect();
            text.push(Line::from(""));
            text.push(
                Line::from(vec![
                    Span::styled("Enter", theme.key()),
                    Span::raw(" close"),
                ])
                .alignment(Alignment::Center),
            );
            let height = (text.len() as u16 + 2).min(area.height);
            draw_text(frame, area, &theme, title, text, 84, height);
        }
        Modal::ConfirmQuit => {
            let lines = vec![
                Line::from(""),
                Line::from(format!("{} installation(s) still running.", app.jobs.len()))
                    .alignment(Alignment::Center),
                Line::from("Quit anyway? Unfinished installs are rolled back.")
                    .alignment(Alignment::Center),
                Line::from(""),
                Line::from(vec![
                    Span::styled("Enter", theme.key()),
                    Span::raw(" quit   "),
                    Span::styled("Esc", theme.key()),
                    Span::raw(" stay"),
                ])
                .alignment(Alignment::Center),
            ];
            draw_text(frame, area, &theme, "Quit", lines, 60, 9);
        }
    }
}

fn draw_text(
    frame: &mut Frame,
    area: Rect,
    theme: &super::theme::Theme,
    title: &str,
    lines: Vec<Line>,
    width: u16,
    height: u16,
) {
    let rect = centered(area, width, height);
    widgets::clear(frame, rect);
    let block = modal_block(theme, title);
    let inner = block.inner(rect);
    frame.render_widget(block, rect);
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}

fn render_progress(frame: &mut Frame, app: &TuiApp, id: &crate::catalog::manifest::GameId) {
    let theme = app.theme();
    let area = frame.area();
    let rect = centered(area, 76, 19);
    widgets::clear(frame, rect);
    let Some(job) = app.jobs.get(id) else {
        let block = modal_block(&theme, "Progress");
        let inner = block.inner(rect);
        frame.render_widget(block, rect);
        frame.render_widget(Paragraph::new("Finished."), inner);
        return;
    };
    let block = modal_block(
        &theme,
        format!(
            "{} {}",
            if job.is_update {
                "Updating"
            } else {
                "Installing"
            },
            job.name
        ),
    );
    let inner = block.inner(rect);
    frame.render_widget(block, rect);
    let [phases_area, gauge_area, log_area, footer_area] = Layout::vertical([
        Constraint::Length(Phase::ALL.len() as u16),
        Constraint::Length(2),
        Constraint::Min(3),
        Constraint::Length(1),
    ])
    .areas(inner);

    let current = job.phase.index();
    let phase_lines: Vec<Line> = Phase::ALL
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let (mark, style) = if i < current {
                ("✓", theme.ok())
            } else if i == current {
                (
                    if job.phase == Phase::Ready {
                        "✓"
                    } else {
                        "▶"
                    },
                    theme.accent(),
                )
            } else {
                ("·", theme.muted())
            };
            let detail = if i == current && !job.detail.is_empty() {
                format!("  {}", widgets::truncate(&job.detail, 50))
            } else {
                String::new()
            };
            Line::from(vec![
                Span::styled(format!(" {mark} "), style),
                Span::styled(p.label(), if i == current { theme.bold() } else { style }),
                Span::styled(detail, theme.muted()),
            ])
        })
        .collect();
    frame.render_widget(Paragraph::new(phase_lines), phases_area);

    match (job.done, job.total) {
        (done, Some(total)) if total > 0 && job.phase == Phase::Downloading => {
            let ratio = (done as f64 / total as f64).clamp(0.0, 1.0);
            let gauge = Gauge::default()
                .gauge_style(theme.accent())
                .ratio(ratio)
                .label(format!("{} / {}", bytes_label(done), bytes_label(total)));
            frame.render_widget(gauge, gauge_area);
        }
        _ => {
            let ratio = if job.phase == Phase::Ready {
                1.0
            } else {
                ((job.phase.index() as f64) + 0.5) / Phase::ALL.len() as f64
            };
            let gauge = LineGauge::default()
                .filled_style(theme.accent())
                .unfilled_style(theme.muted())
                .ratio(ratio)
                .label(format!(
                    "{} elapsed",
                    crate::library::format_duration(job.started.elapsed())
                ));
            frame.render_widget(gauge, gauge_area);
        }
    }

    let tail: Vec<Line> = job
        .tail
        .iter()
        .map(|l| {
            Line::from(Span::styled(
                widgets::truncate(l, inner.width.saturating_sub(2) as usize),
                theme.muted(),
            ))
        })
        .collect();
    frame.render_widget(Paragraph::new(tail), log_area);
    let footer = Line::from(vec![
        Span::styled("Esc", theme.key()),
        Span::raw(" hide (continues in background)   "),
        Span::styled("c", theme.key()),
        Span::raw(" cancel"),
    ])
    .alignment(Alignment::Center);
    frame.render_widget(Paragraph::new(footer), footer_area);
}
