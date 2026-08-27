//! Home: continue playing, featured games, and a summary pane.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{List, ListItem, Paragraph, Wrap};

use crate::library::{format_duration, format_relative};

use super::super::app::{HomeItem, TuiApp};
use super::super::widgets::{self, panel, render_empty, state_badge, support_style};

pub fn render(frame: &mut Frame, area: Rect, app: &mut TuiApp) {
    let theme = app.theme();
    let wide = area.width >= 96;
    let [list_area, side_area] = if wide {
        Layout::horizontal([Constraint::Percentage(62), Constraint::Percentage(38)]).areas(area)
    } else {
        Layout::vertical([Constraint::Min(3), Constraint::Length(0)]).areas(area)
    };

    let block = panel(&theme, "Home", true);
    let inner = block.inner(list_area);
    frame.render_widget(block, list_area);
    if app.home.items.is_empty() {
        render_empty(
            frame,
            inner,
            "The catalog is empty.",
            "Run `rustarcade catalog update` to fetch games.",
            &theme,
        );
    } else {
        let name_width = inner.width.saturating_sub(34).max(12) as usize;
        let items: Vec<ListItem> = app
            .home
            .items
            .iter()
            .map(|item| match item {
                HomeItem::Header(title) => ListItem::new(Line::from(Span::styled(
                    format!("  {title}"),
                    theme.title(),
                ))),
                HomeItem::Game(id) => {
                    let Some(view) = app.view(id) else {
                        return ListItem::new(Line::from(id.to_string()));
                    };
                    let name = widgets::truncate(&view.manifest.name, name_width);
                    let category = view
                        .manifest
                        .categories
                        .first()
                        .map(|c| c.label())
                        .unwrap_or("");
                    let star = if view.favorite { "★ " } else { "  " };
                    ListItem::new(Line::from(vec![
                        Span::styled(star, theme.warn()),
                        Span::raw(format!("{name:<name_width$} ")),
                        Span::styled(format!("{category:<11} "), theme.muted()),
                        Span::styled(
                            format!("{:<9} ", short_support(view.manifest.support_status)),
                            support_style(view.manifest.support_status, &theme),
                        ),
                        state_badge(&view.state, &theme),
                    ]))
                }
            })
            .collect();
        let list = List::new(items)
            .highlight_style(theme.highlight())
            .highlight_symbol("▶ ");
        frame.render_stateful_widget(list, inner, &mut app.home.state);
    }

    if wide {
        render_side(frame, side_area, app);
    }
}

fn short_support(status: crate::catalog::SupportStatus) -> &'static str {
    use crate::catalog::SupportStatus::*;
    match status {
        Verified => "verified",
        CommunityTested => "community",
        Experimental => "experim.",
        Broken => "broken",
        Archived => "archived",
    }
}

fn render_side(frame: &mut Frame, area: Rect, app: &TuiApp) {
    let theme = app.theme();
    let [summary_area, tips_area] =
        Layout::vertical([Constraint::Min(8), Constraint::Length(9)]).areas(area);
    let block = panel(&theme, "Selected", false);
    let inner = block.inner(summary_area);
    frame.render_widget(block, summary_area);
    let mut lines: Vec<Line> = Vec::new();
    if let Some(view) = app.home.selected_id().and_then(|id| app.view(id)) {
        let m = &view.manifest;
        lines.push(Line::from(Span::styled(m.name.clone(), theme.bold())));
        lines.push(Line::from(Span::styled(m.summary.clone(), theme.muted())));
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled("Status     ", theme.muted()),
            state_badge(&view.state, &theme),
        ]));
        lines.push(Line::from(vec![
            Span::styled("Support    ", theme.muted()),
            Span::styled(
                m.support_status.label(),
                support_style(m.support_status, &theme),
            ),
        ]));
        lines.push(Line::from(vec![
            Span::styled("Platforms  ", theme.muted()),
            Span::raw(
                m.compatibility
                    .os
                    .iter()
                    .map(|o| o.label())
                    .collect::<Vec<_>>()
                    .join(", "),
            ),
        ]));
        if let Some(v) = view.installed_version() {
            lines.push(Line::from(vec![
                Span::styled("Installed  ", theme.muted()),
                Span::raw(v.to_string()),
            ]));
        }
        if view.stats.sessions > 0 {
            lines.push(Line::from(vec![
                Span::styled("Play time  ", theme.muted()),
                Span::raw(format!(
                    "{} · {} session(s)",
                    format_duration(view.stats.total),
                    view.stats.sessions
                )),
            ]));
            if let Some(t) = view.stats.last_played {
                lines.push(Line::from(vec![
                    Span::styled("Last played ", theme.muted()),
                    Span::raw(format_relative(t, chrono::Utc::now())),
                ]));
            }
        }
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled("Enter for details", theme.muted())));
    } else {
        lines.push(Line::from(Span::styled(
            "Select a game to see its details.",
            theme.muted(),
        )));
    }
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);

    let block = panel(&theme, "Tips", false);
    let inner = block.inner(tips_area);
    frame.render_widget(block, tips_area);
    let tips = vec![
        Line::from(vec![
            Span::styled("Tab / 1-6 ", theme.key()),
            Span::styled("switch screens", theme.muted()),
        ]),
        Line::from(vec![
            Span::styled("/         ", theme.key()),
            Span::styled("search in Discover", theme.muted()),
        ]),
        Line::from(vec![
            Span::styled("Enter     ", theme.key()),
            Span::styled("details, then install & play", theme.muted()),
        ]),
        Line::from(vec![
            Span::styled("?         ", theme.key()),
            Span::styled("all keyboard shortcuts", theme.muted()),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            app.last_launch.clone().unwrap_or_else(|| {
                "Games run in this terminal and return here when they exit.".into()
            }),
            theme.muted(),
        )),
    ];
    frame.render_widget(Paragraph::new(tips).wrap(Wrap { trim: false }), inner);
}
