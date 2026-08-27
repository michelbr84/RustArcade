//! Discover / Library / Favorites: a filterable table with a preview pane.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Cell, Paragraph, Row, Table, Wrap};

use crate::library::format_duration;

use super::super::app::TuiApp;
use super::super::widgets::{self, panel, render_empty, state_badge, support_style};

pub fn render(
    frame: &mut Frame,
    area: Rect,
    app: &mut TuiApp,
    title: &str,
    empty_title: &str,
    empty_hint: &str,
) {
    let theme = app.theme();
    let wide = area.width >= 100;
    let [table_area, preview_area] = if wide {
        Layout::horizontal([Constraint::Percentage(60), Constraint::Percentage(40)]).areas(area)
    } else {
        Layout::vertical([Constraint::Min(3), Constraint::Length(0)]).areas(area)
    };

    let Some(list) = app.active_list_ref() else {
        return;
    };
    let mut heading = title.to_string();
    if let Some(c) = list.category {
        heading.push_str(&format!(" · {}", c.label()));
    }
    heading.push_str(&format!(" ({})", list.ids.len()));
    let block = panel(&theme, &heading, true);
    let inner = block.inner(table_area);
    frame.render_widget(block, table_area);

    let show_search = list.editing || !list.query.is_empty();
    let [search_area, rows_area] = if show_search {
        Layout::vertical([Constraint::Length(1), Constraint::Min(1)]).areas(inner)
    } else {
        Layout::vertical([Constraint::Length(0), Constraint::Min(1)]).areas(inner)
    };
    if show_search {
        let line = Line::from(vec![
            Span::styled("/ ", theme.key()),
            Span::raw(list.query.clone()),
            Span::styled(if list.editing { "▏" } else { "" }, theme.accent()),
            Span::styled(
                if list.editing {
                    "  (Enter keep · Esc clear)"
                } else {
                    "  (Esc clear)"
                },
                theme.muted(),
            ),
        ]);
        frame.render_widget(Paragraph::new(line), search_area);
    }

    if list.ids.is_empty() {
        let (t, h) = if !list.query.is_empty() || list.category.is_some() {
            (
                "No games match.",
                "Press Esc to clear the search and category filter.",
            )
        } else {
            (empty_title, empty_hint)
        };
        render_empty(frame, rows_area, t, h, &theme);
    } else {
        let name_width = rows_area.width.saturating_sub(38).clamp(10, 40) as usize;
        let rows: Vec<Row> = list
            .ids
            .iter()
            .map(|id| {
                let Some(view) = app.view(id) else {
                    return Row::new(vec![Cell::from(id.to_string())]);
                };
                let m = &view.manifest;
                let star = if view.favorite { "★" } else { " " };
                let version = view
                    .installed_version()
                    .map(|v| v.to_string())
                    .unwrap_or_default();
                Row::new(vec![
                    Cell::from(Span::styled(star, theme.warn())),
                    Cell::from(widgets::truncate(&m.name, name_width)),
                    Cell::from(Span::styled(
                        m.categories.first().map(|c| c.label()).unwrap_or(""),
                        theme.muted(),
                    )),
                    Cell::from(Span::styled(
                        short_support(m.support_status),
                        support_style(m.support_status, &theme),
                    )),
                    Cell::from(Line::from(vec![
                        state_badge(&view.state, &theme),
                        Span::styled(
                            if version.is_empty() {
                                String::new()
                            } else {
                                format!(" {version}")
                            },
                            theme.muted(),
                        ),
                    ])),
                ])
            })
            .collect();
        let widths = [
            Constraint::Length(1),
            Constraint::Length(name_width as u16),
            Constraint::Length(11),
            Constraint::Length(9),
            Constraint::Min(12),
        ];
        let table = Table::new(rows, widths)
            .header(Row::new(vec!["", "Name", "Category", "Support", "State"]).style(theme.muted()))
            .row_highlight_style(theme.highlight())
            .highlight_symbol("▶ ")
            .column_spacing(1);
        let Some(list_mut) = app.active_list() else {
            return;
        };
        frame.render_stateful_widget(table, rows_area, &mut list_mut.state);
    }

    if wide {
        render_preview(frame, preview_area, app);
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

fn render_preview(frame: &mut Frame, area: Rect, app: &TuiApp) {
    let theme = app.theme();
    let block = panel(&theme, "Preview", false);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let Some(view) = app
        .active_list_ref()
        .and_then(|l| l.selected_id())
        .and_then(|id| app.view(id))
    else {
        frame.render_widget(
            Paragraph::new(Span::styled("Nothing selected.", theme.muted())),
            inner,
        );
        return;
    };
    let m = &view.manifest;
    let mut lines: Vec<Line> = vec![
        Line::from(Span::styled(m.name.clone(), theme.bold())),
        Line::from(Span::styled(m.summary.clone(), theme.muted())),
        Line::from(""),
    ];
    if let Some(d) = &m.description {
        lines.push(Line::from(d.trim().replace('\n', " ")));
        lines.push(Line::from(""));
    }
    lines.push(Line::from(vec![
        Span::styled("Repository ", theme.muted()),
        Span::raw(m.repository.trim_start_matches("https://").to_string()),
    ]));
    lines.push(Line::from(vec![
        Span::styled("License    ", theme.muted()),
        Span::raw(m.license.clone().unwrap_or_else(|| "unknown".into())),
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
    lines.push(Line::from(vec![
        Span::styled("Install    ", theme.muted()),
        Span::raw(
            m.installers
                .iter()
                .filter(|i| i.applies_to(app.services.platform().os))
                .map(|i| i.kind().label())
                .collect::<Vec<_>>()
                .join(" → "),
        ),
    ]));
    lines.push(Line::from(vec![
        Span::styled("Status     ", theme.muted()),
        state_badge(&view.state, &theme),
    ]));
    if view.stats.sessions > 0 {
        lines.push(Line::from(vec![
            Span::styled("Play time  ", theme.muted()),
            Span::raw(format_duration(view.stats.total)),
        ]));
    }
    if let Some(n) = &m.notes {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(n.clone(), theme.warn())));
    }
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}
