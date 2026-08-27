//! Colours and styles for the TUI.

use ratatui::style::{Color, Modifier, Style};

use crate::config::Theme as ThemeChoice;

/// Resolved palette.
#[derive(Debug, Clone, Copy)]
pub struct Theme {
    pub accent: Color,
    pub accent_soft: Color,
    pub text: Color,
    pub muted: Color,
    pub ok: Color,
    pub warn: Color,
    pub err: Color,
    pub border: Color,
    pub highlight_bg: Color,
    pub highlight_fg: Color,
}

impl Theme {
    pub fn from_choice(choice: ThemeChoice) -> Theme {
        match choice {
            ThemeChoice::Default => Theme {
                accent: Color::Cyan,
                accent_soft: Color::LightCyan,
                text: Color::Reset,
                muted: Color::DarkGray,
                ok: Color::Green,
                warn: Color::Yellow,
                err: Color::Red,
                border: Color::DarkGray,
                highlight_bg: Color::Cyan,
                highlight_fg: Color::Black,
            },
            ThemeChoice::Mono => Theme {
                accent: Color::White,
                accent_soft: Color::Gray,
                text: Color::Reset,
                muted: Color::DarkGray,
                ok: Color::White,
                warn: Color::Gray,
                err: Color::White,
                border: Color::Gray,
                highlight_bg: Color::White,
                highlight_fg: Color::Black,
            },
        }
    }

    pub fn title(&self) -> Style {
        Style::default()
            .fg(self.accent)
            .add_modifier(Modifier::BOLD)
    }
    pub fn muted(&self) -> Style {
        Style::default().fg(self.muted)
    }
    pub fn accent(&self) -> Style {
        Style::default().fg(self.accent)
    }
    pub fn bold(&self) -> Style {
        Style::default().add_modifier(Modifier::BOLD)
    }
    pub fn ok(&self) -> Style {
        Style::default().fg(self.ok)
    }
    pub fn warn(&self) -> Style {
        Style::default().fg(self.warn)
    }
    pub fn err(&self) -> Style {
        Style::default().fg(self.err).add_modifier(Modifier::BOLD)
    }
    pub fn border(&self) -> Style {
        Style::default().fg(self.border)
    }
    pub fn border_focused(&self) -> Style {
        Style::default().fg(self.accent)
    }
    pub fn highlight(&self) -> Style {
        Style::default()
            .bg(self.highlight_bg)
            .fg(self.highlight_fg)
            .add_modifier(Modifier::BOLD)
    }
    pub fn key(&self) -> Style {
        Style::default()
            .fg(self.accent_soft)
            .add_modifier(Modifier::BOLD)
    }
}
