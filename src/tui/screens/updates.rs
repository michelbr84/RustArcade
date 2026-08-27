//! Updates screen: installed version vs latest.

use ratatui::Frame;
use ratatui::layout::{Constraint, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Cell, Row, Table};

use super::super::app::TuiApp;
use super::super::widgets::{self, panel, render_empty};

pub fn render(frame: &mut Frame, area: Rect, app: &mut TuiApp) {
    let theme = app.theme();
    let available = app.updates.checks.iter().filter(|c| c.available).count();
    let title = if app.updates.checking {
        format!("Updates · checking {}", widgets::spinner(app.tick / 2))
    } else if available > 0 {
        format!("Updates · {available} available")
    } else {
        "Updates".to_string()
    };
    let block = panel(&theme, &title, true);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if app.services.installed().is_empty() {
        render_empty(
            frame,
            inner,
            "Nothing installed.",
            "Install a game from Discover to see updates here.",
            &theme,
        );
        return;
    }
    if app.updates.checks.is_empty() {
        if app.updates.checking {
            render_empty(
                frame,
                inner,
                "Checking for updates…",
                "This uses the GitHub and crates.io APIs.",
                &theme,
            );
        } else if !app.updates.errors.is_empty() {
            render_empty(
                frame,
                inner,
                "Could not check for updates.",
                &app.updates.errors.join(" · "),
                &theme,
            );
        } else {
            render_empty(
                frame,
                inner,
                "No update information yet.",
                "Press r to check now.",
                &theme,
            );
        }
        return;
    }
    let rows: Vec<Row> = app
        .updates
        .checks
        .iter()
        .map(|c| {
            let name = app
                .view(&c.game)
                .map(|v| v.manifest.name.clone())
                .unwrap_or_else(|| c.game.to_string());
            let status = if c.available {
                Span::styled("update available", theme.warn())
            } else {
                Span::styled("up to date", theme.ok())
            };
            Row::new(vec![
                Cell::from(name),
                Cell::from(c.installed.clone()),
                Cell::from(c.latest.clone()),
                Cell::from(Span::styled(c.installer.label(), theme.muted())),
                Cell::from(status),
            ])
        })
        .collect();
    let widths = [
        Constraint::Min(16),
        Constraint::Length(16),
        Constraint::Length(20),
        Constraint::Length(18),
        Constraint::Length(17),
    ];
    let table = Table::new(rows, widths)
        .header(
            Row::new(vec!["Game", "Installed", "Latest", "Method", "Status"]).style(theme.muted()),
        )
        .row_highlight_style(theme.highlight())
        .highlight_symbol("▶ ")
        .column_spacing(1);
    let mut rows_area = inner;
    if !app.updates.errors.is_empty() {
        rows_area.height = rows_area.height.saturating_sub(1);
        let err_area = Rect {
            y: inner.y + inner.height.saturating_sub(1),
            height: 1,
            ..inner
        };
        frame.render_widget(
            ratatui::widgets::Paragraph::new(Line::from(Span::styled(
                format!("Could not check: {}", app.updates.errors.join(" · ")),
                theme.warn(),
            ))),
            err_area,
        );
    }
    frame.render_stateful_widget(table, rows_area, &mut app.updates.state);
}
