//! Game details with state-dependent actions.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Wrap};

use crate::library::{format_duration, format_relative};
use crate::services::GameState;

use super::super::app::TuiApp;
use super::super::widgets::{panel, state_badge, support_style};

pub fn render(frame: &mut Frame, area: Rect, app: &mut TuiApp) {
    let theme = app.theme();
    let Some(details) = app.details.clone() else {
        return;
    };
    let Some(view) = app.view(&details.id).cloned() else {
        frame.render_widget(
            Paragraph::new("This game is no longer in the catalog."),
            area,
        );
        return;
    };
    let m = &view.manifest;
    let [body_area, actions_area] =
        Layout::vertical([Constraint::Min(3), Constraint::Length(3)]).areas(area);
    let block = panel(&theme, format!("{}  ·  {}", m.name, m.id), true);
    let inner = block.inner(body_area);
    frame.render_widget(block, body_area);

    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::from(Span::styled(m.summary.clone(), theme.bold())));
    if let Some(d) = &m.description {
        lines.push(Line::from(""));
        for l in d.trim().lines() {
            lines.push(Line::from(l.to_string()));
        }
    }
    lines.push(Line::from(""));
    let row = |label: &str, value: Span<'static>| {
        Line::from(vec![
            Span::styled(format!("{label:<18}"), theme.muted()),
            value,
        ])
    };
    lines.push(row("Status", state_badge(&view.state, &theme)));
    if let GameState::Broken(reason) | GameState::Unsupported(reason) = &view.state {
        lines.push(row("Reason", Span::styled(reason.clone(), theme.err())));
    }
    lines.push(row(
        "Support",
        Span::styled(
            m.support_status.label(),
            support_style(m.support_status, &theme),
        ),
    ));
    if let Some(v) = &m.verified_on {
        lines.push(row("Verified on", Span::raw(v.clone())));
    }
    lines.push(row("Repository", Span::raw(m.repository.clone())));
    if let Some(h) = &m.homepage {
        lines.push(row("Homepage", Span::raw(h.clone())));
    }
    lines.push(row(
        "License",
        Span::raw(m.license.clone().unwrap_or_else(|| "unknown".into())),
    ));
    lines.push(row(
        "Categories",
        Span::raw(
            m.categories
                .iter()
                .map(|c| c.label())
                .collect::<Vec<_>>()
                .join(", "),
        ),
    ));
    if !m.tags.is_empty() {
        lines.push(row("Tags", Span::raw(m.tags.join(", "))));
    }
    let platform = app.services.platform();
    let platforms: Vec<String> = crate::platform::Os::ALL
        .iter()
        .map(|o| {
            format!(
                "{} {}",
                o.label(),
                if m.compatibility.os.contains(o) {
                    "✓"
                } else {
                    "✗"
                }
            )
        })
        .collect();
    lines.push(row("Platforms", Span::raw(platforms.join("   "))));
    if let Some(t) = &m.compatibility.min_terminal {
        lines.push(row(
            "Min. terminal",
            Span::raw(format!(
                "{}x{} (current {}x{})",
                t.cols, t.rows, app.size.0, app.size.1
            )),
        ));
    }
    let installers: Vec<String> = m
        .installers
        .iter()
        .filter(|i| i.applies_to(platform.os))
        .map(|i| i.kind().label().to_string())
        .collect();
    lines.push(row("Install via", Span::raw(installers.join(" → "))));
    if let Some(r) = &view.install {
        lines.push(Line::from(""));
        lines.push(row(
            "Installed",
            Span::styled(r.version.clone(), theme.ok()),
        ));
        lines.push(row(
            "Method",
            Span::raw(format!("{} — {}", r.installer.label(), r.source.describe())),
        ));
        lines.push(row(
            "Executable",
            Span::raw(
                r.executable_path(app.services.paths())
                    .display()
                    .to_string(),
            ),
        ));
        lines.push(row(
            "Installed on",
            Span::raw(r.installed_at.format("%Y-%m-%d").to_string()),
        ));
        lines.push(row(
            "Checksum",
            if r.checksum_verified {
                Span::styled("verified (SHA-256)", theme.ok())
            } else {
                Span::styled("not verified upstream", theme.muted())
            },
        ));
    }
    if let Some(l) = &view.latest_version {
        lines.push(row("Latest", Span::raw(l.clone())));
    }
    if view.stats.sessions > 0 {
        lines.push(row(
            "Play time",
            Span::raw(format!(
                "{} across {} session(s)",
                format_duration(view.stats.total),
                view.stats.sessions
            )),
        ));
        if let Some(t) = view.stats.last_played {
            lines.push(row(
                "Last played",
                Span::raw(format_relative(t, chrono::Utc::now())),
            ));
        }
    }
    if !m.requirements.optional_commands.is_empty() {
        lines.push(row(
            "Optional tools",
            Span::raw(m.requirements.optional_commands.join(", ")),
        ));
    }
    if let Some(n) = &m.requirements.notes {
        lines.push(row("Requirements", Span::raw(n.clone())));
    }
    if let Some(n) = &m.notes {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(format!("Note: {n}"), theme.warn())));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Third-party software: RustArcade downloads this game from its original project and does not audit or sandbox it.",
        theme.muted(),
    )));
    let max_scroll = (lines.len() as u16).saturating_sub(inner.height);
    if let Some(d) = &mut app.details {
        d.scroll = d.scroll.min(max_scroll);
    }
    let scroll = app.details.as_ref().map(|d| d.scroll).unwrap_or(0);
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .scroll((scroll, 0)),
        inner,
    );

    let block = panel(&theme, "Actions", false);
    let inner = block.inner(actions_area);
    frame.render_widget(block, actions_area);
    let actions: Vec<(&str, &str)> = match &view.state {
        GameState::Available => vec![
            ("Enter", "Install & Play"),
            ("i", "Install"),
            (
                "f",
                if view.favorite {
                    "Unfavorite"
                } else {
                    "Favorite"
                },
            ),
        ],
        GameState::Installed => vec![
            ("Enter", "Play"),
            ("u", "Check for update"),
            ("x", "Uninstall"),
            (
                "f",
                if view.favorite {
                    "Unfavorite"
                } else {
                    "Favorite"
                },
            ),
        ],
        GameState::UpdateAvailable => vec![
            ("Enter", "Play"),
            ("u", "Update"),
            ("x", "Uninstall"),
            ("f", "Favorite"),
        ],
        GameState::Installing => vec![("Enter", "Show progress"), ("c", "Cancel")],
        GameState::Running => vec![],
        GameState::Broken(_) => vec![
            ("Enter", "Reinstall"),
            ("x", "Remove leftovers"),
            ("l", "View log"),
        ],
        GameState::Unsupported(_) => vec![("f", "Favorite")],
    };
    let mut spans = Vec::new();
    for (i, (k, label)) in actions.iter().enumerate() {
        if i > 0 {
            spans.push(Span::raw("   "));
        }
        spans.push(Span::styled(format!("[{k}]"), theme.key()));
        spans.push(Span::raw(format!(" {label}")));
    }
    if spans.is_empty() {
        spans.push(Span::styled("No actions available.", theme.muted()));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), inner);
}
