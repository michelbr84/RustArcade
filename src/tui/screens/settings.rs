//! Settings: toggles persisted to config.toml, plus read-only paths.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{List, ListItem, ListState, Paragraph, Wrap};

use super::super::app::{Setting, TuiApp};
use super::super::widgets::panel;

pub fn render(frame: &mut Frame, area: Rect, app: &mut TuiApp) {
    let theme = app.theme();
    let [left, right] = if area.width >= 96 {
        Layout::horizontal([Constraint::Percentage(55), Constraint::Percentage(45)]).areas(area)
    } else {
        Layout::vertical([Constraint::Min(6), Constraint::Min(6)]).areas(area)
    };
    let block = panel(&theme, "Settings", true);
    let inner = block.inner(left);
    frame.render_widget(block, left);
    let label_width = inner.width.saturating_sub(16).max(20) as usize;
    let items: Vec<ListItem> = Setting::ALL
        .iter()
        .map(|s| {
            let value = s.value(&app.config);
            let style = match value.as_str() {
                "on" => theme.ok(),
                "off" => theme.muted(),
                _ => theme.accent(),
            };
            ListItem::new(Line::from(vec![
                Span::raw(format!("{:<label_width$}", s.label())),
                Span::styled(value, style),
            ]))
        })
        .collect();
    let list = List::new(items)
        .highlight_style(theme.highlight())
        .highlight_symbol("▶ ");
    let mut state = ListState::default().with_selected(Some(app.settings.selected));
    frame.render_stateful_widget(list, inner, &mut state);

    let block = panel(&theme, "About", false);
    let inner = block.inner(right);
    frame.render_widget(block, right);
    let paths = app.services.paths();
    let tools = app.services.tools();
    let tool =
        |t: &crate::platform::ToolInfo| t.version.clone().unwrap_or_else(|| "not found".into());
    let mut lines = vec![
        Line::from(vec![
            Span::styled("RustArcade ", theme.bold()),
            Span::raw(crate::VERSION),
        ]),
        Line::from(Span::styled(
            format!(
                "{} · {}",
                app.services.platform(),
                app.catalog_status.label()
            ),
            theme.muted(),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled("Cargo   ", theme.muted()),
            Span::raw(tool(&tools.cargo)),
        ]),
        Line::from(vec![
            Span::styled("Git     ", theme.muted()),
            Span::raw(tool(&tools.git)),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("Config  ", theme.muted()),
            Span::raw(paths.config_file().display().to_string()),
        ]),
        Line::from(vec![
            Span::styled("Games   ", theme.muted()),
            Span::raw(paths.games_dir().display().to_string()),
        ]),
        Line::from(vec![
            Span::styled("Logs    ", theme.muted()),
            Span::raw(paths.logs_dir().display().to_string()),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            "Changes are saved immediately to config.toml.",
            theme.muted(),
        )),
    ];
    if !app.services.startup_notes().is_empty() {
        lines.push(Line::from(""));
        for n in app.services.startup_notes() {
            lines.push(Line::from(Span::styled(format!("! {n}"), theme.warn())));
        }
    }
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}
