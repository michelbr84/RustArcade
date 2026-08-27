//! Screen rendering dispatcher.

pub mod details;
pub mod home;
pub mod list;
pub mod settings;
pub mod updates;

use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout};
use ratatui::text::Line;
use ratatui::widgets::Paragraph;

use super::app::{MIN_COLS, MIN_ROWS, Tab, TuiApp};
use super::widgets;

/// Draw the whole interface.
pub fn render(frame: &mut Frame, app: &mut TuiApp) {
    let area = frame.area();
    app.size = (area.width, area.height);
    let theme = app.theme();
    if app.too_small() {
        let msg = Paragraph::new(vec![
            Line::from(""),
            Line::from(format!(
                "Terminal too small ({}x{}).",
                area.width, area.height
            ))
            .alignment(Alignment::Center),
            Line::from(format!("RustArcade needs at least {MIN_COLS}x{MIN_ROWS}."))
                .alignment(Alignment::Center),
            Line::from(""),
            Line::from("Resize the window or press q to quit.")
                .style(theme.muted())
                .alignment(Alignment::Center),
        ]);
        frame.render_widget(msg, area);
        return;
    }
    let [header, tabs, body, status, footer] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(2),
        Constraint::Min(3),
        Constraint::Length(1),
        Constraint::Length(2),
    ])
    .areas(area);
    widgets::render_header(frame, header, app);
    widgets::render_tabs(frame, tabs, app);
    if app.details.is_some() {
        details::render(frame, body, app);
    } else {
        match app.tab {
            Tab::Home => home::render(frame, body, app),
            Tab::Discover => list::render(
                frame,
                body,
                app,
                "Discover",
                "No games match your search.",
                "Press Esc to clear the filter.",
            ),
            Tab::Library => list::render(
                frame,
                body,
                app,
                "Library",
                "No games installed yet.",
                "Press 2 to open Discover and i to install a game.",
            ),
            Tab::Favorites => list::render(
                frame,
                body,
                app,
                "Favorites",
                "No favorites yet.",
                "Press f on any game to add it here.",
            ),
            Tab::Updates => updates::render(frame, body, app),
            Tab::Settings => settings::render(frame, body, app),
        }
    }
    widgets::render_status(frame, status, app);
    let hints = app.hints();
    widgets::render_footer(frame, footer, &hints, &theme);
    super::modals::render(frame, app);
}
