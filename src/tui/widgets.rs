//! Reusable rendering helpers: header, tabs, footer, status bar, badges, empty states.

use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Flex, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph, Tabs, Wrap};

use crate::services::GameState;

use super::app::{Tab, TuiApp};
use super::theme::Theme;

/// A rectangle of `width`×`height` centred inside `area` (clamped to it).
pub fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let [h] = Layout::horizontal([Constraint::Length(width.min(area.width))])
        .flex(Flex::Center)
        .areas(area);
    let [v] = Layout::vertical([Constraint::Length(height.min(area.height))])
        .flex(Flex::Center)
        .areas(h);
    v
}

/// A bordered, cleared modal box with a title.
pub fn modal_block(theme: &Theme, title: impl Into<String>) -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(theme.border_focused())
        .title(Line::from(format!(" {} ", title.into())).style(theme.title()))
}

pub fn panel(theme: &Theme, title: impl Into<String>, focused: bool) -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(if focused {
            theme.border_focused()
        } else {
            theme.border()
        })
        .title(Line::from(format!(" {} ", title.into())).style(theme.title()))
}

pub fn render_header(frame: &mut Frame, area: Rect, app: &TuiApp) {
    let theme = app.theme();
    let views = &app.views;
    let installed = views.iter().filter(|v| v.state.is_installed()).count();
    let updates = views
        .iter()
        .filter(|v| v.state == GameState::UpdateAvailable)
        .count();
    let play = crate::library::format_duration(app.services.total_play_time());
    let left = Line::from(vec![
        Span::styled(" ▲ RUSTARCADE ", theme.title()),
        Span::styled("Your terminal. Your arcade.", theme.muted()),
    ]);
    let mut right = vec![
        Span::styled(format!("{} games", views.len()), theme.muted()),
        Span::styled(" · ", theme.muted()),
        Span::styled(format!("{installed} installed"), theme.muted()),
    ];
    if updates > 0 {
        right.push(Span::styled(" · ", theme.muted()));
        right.push(Span::styled(
            format!("{updates} update{}", if updates == 1 { "" } else { "s" }),
            theme.warn(),
        ));
    }
    right.push(Span::styled(" · ", theme.muted()));
    right.push(Span::styled(format!("{play} played "), theme.muted()));
    let [l, r] = Layout::horizontal([
        Constraint::Min(20),
        Constraint::Length(Line::from(right.clone()).width() as u16),
    ])
    .areas(area);
    frame.render_widget(Paragraph::new(left), l);
    frame.render_widget(
        Paragraph::new(Line::from(right)).alignment(Alignment::Right),
        r,
    );
}

pub fn render_tabs(frame: &mut Frame, area: Rect, app: &TuiApp) {
    let theme = app.theme();
    let titles: Vec<Line> = Tab::ALL
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let mut label = format!(" {} {} ", i + 1, t.label());
            if *t == Tab::Updates {
                let n = app
                    .views
                    .iter()
                    .filter(|v| v.state == GameState::UpdateAvailable)
                    .count();
                if n > 0 {
                    label = format!(" {} {} ({n}) ", i + 1, t.label());
                }
            }
            Line::from(label)
        })
        .collect();
    let tabs = Tabs::new(titles)
        .select(app.tab.index())
        .style(theme.muted())
        .highlight_style(theme.highlight())
        .divider(Span::styled("│", theme.muted()))
        .block(
            Block::default()
                .borders(Borders::BOTTOM)
                .border_style(theme.border()),
        );
    frame.render_widget(tabs, area);
}

pub fn render_status(frame: &mut Frame, area: Rect, app: &TuiApp) {
    let theme = app.theme();
    let mut spans: Vec<Span> = Vec::new();
    if let Some((text, _, is_error)) = &app.toast {
        spans.push(Span::styled(
            format!(" {text} "),
            if *is_error { theme.err() } else { theme.ok() },
        ));
    } else if let Some(job) = app.jobs.values().next() {
        let pct = match (job.done, job.total) {
            (d, Some(t)) if t > 0 => format!(" {}%", d * 100 / t),
            _ => String::new(),
        };
        spans.push(Span::styled(
            format!(
                " {} {}: {}{}{} ",
                spinner(app.tick),
                if job.is_update {
                    "Updating"
                } else {
                    "Installing"
                },
                job.name,
                if job.detail.is_empty() {
                    String::new()
                } else {
                    format!(" — {}", job.detail)
                },
                pct
            ),
            theme.accent(),
        ));
    } else {
        spans.push(Span::styled(
            format!(" {} ", app.catalog_status.label()),
            theme.muted(),
        ));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

pub fn render_footer(frame: &mut Frame, area: Rect, hints: &[(&str, &str)], theme: &Theme) {
    let mut spans = Vec::new();
    for (i, (key, label)) in hints.iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled("  ", theme.muted()));
        }
        spans.push(Span::styled(*key, theme.key()));
        spans.push(Span::styled(format!(" {label}"), theme.muted()));
    }
    let para = Paragraph::new(Line::from(spans)).block(
        Block::default()
            .borders(Borders::TOP)
            .border_style(theme.border()),
    );
    frame.render_widget(para, area);
}

/// Centered message for lists with nothing to show.
pub fn render_empty(frame: &mut Frame, area: Rect, title: &str, hint: &str, theme: &Theme) {
    let lines = vec![
        Line::from(""),
        Line::from(Span::styled(title, theme.bold())).alignment(Alignment::Center),
        Line::from(""),
        Line::from(Span::styled(hint, theme.muted())).alignment(Alignment::Center),
    ];
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), area);
}

pub fn state_badge(state: &GameState, theme: &Theme) -> Span<'static> {
    match state {
        GameState::Available => Span::styled("available", theme.muted()),
        GameState::Installing => Span::styled("installing…", theme.accent()),
        GameState::Installed => Span::styled("installed", theme.ok()),
        GameState::UpdateAvailable => Span::styled("update", theme.warn()),
        GameState::Running => Span::styled("running", theme.accent()),
        GameState::Broken(_) => Span::styled("broken", theme.err()),
        GameState::Unsupported(_) => Span::styled("unsupported", theme.muted()),
    }
}

pub fn support_style(status: crate::catalog::SupportStatus, theme: &Theme) -> Style {
    use crate::catalog::SupportStatus::*;
    match status {
        Verified => theme.ok(),
        CommunityTested => theme.accent(),
        Experimental => theme.warn(),
        Broken | Archived => theme.err(),
    }
}

pub fn spinner(tick: u64) -> char {
    const FRAMES: [char; 8] = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧'];
    FRAMES[(tick as usize) % FRAMES.len()]
}

/// Clear an area before drawing a modal on top of it.
pub fn clear(frame: &mut Frame, area: Rect) {
    frame.render_widget(Clear, area);
}

pub fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let cut: String = s.chars().take(max.saturating_sub(1)).collect();
        format!("{cut}…")
    }
}

pub fn bytes_label(n: u64) -> String {
    if n >= 1024 * 1024 {
        format!("{:.1} MB", n as f64 / (1024.0 * 1024.0))
    } else if n >= 1024 {
        format!("{:.0} KB", n as f64 / 1024.0)
    } else {
        format!("{n} B")
    }
}
