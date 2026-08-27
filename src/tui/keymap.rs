//! Keyboard mapping (context-sensitive, keyboard-only).

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::app::{Modal, Tab, TuiApp};

/// User intents produced from key presses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Quit,
    ForceQuit,
    Help,
    GoTab(Tab),
    NextTab,
    PrevTab,
    Up,
    Down,
    PageUp,
    PageDown,
    First,
    Last,
    Select,
    Back,
    SearchStart,
    SearchChar(char),
    SearchBackspace,
    SearchCommit,
    SearchCancel,
    CycleCategory,
    ToggleFavorite,
    Install { then_play: bool },
    Play,
    Update,
    UpdateAll,
    Uninstall,
    Confirm,
    Cancel,
    Retry,
    ViewLog,
    ScrollUp,
    ScrollDown,
    Refresh,
    CancelJob,
    ToggleSetting,
    HideModal,
}

/// Map a key press to an action for the current UI context.
pub fn map(app: &TuiApp, key: KeyEvent) -> Option<Action> {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    if ctrl && matches!(key.code, KeyCode::Char('c') | KeyCode::Char('C')) {
        return Some(Action::Quit);
    }
    if app.modal.is_some() {
        return map_modal(app, key);
    }
    if app.is_editing_search() {
        return Some(match key.code {
            KeyCode::Esc => Action::SearchCancel,
            KeyCode::Enter => Action::SearchCommit,
            KeyCode::Backspace => Action::SearchBackspace,
            KeyCode::Down => Action::Down,
            KeyCode::Up => Action::Up,
            KeyCode::Char(c) if !ctrl => Action::SearchChar(c),
            _ => return None,
        });
    }
    if app.details.is_some() {
        return Some(match key.code {
            KeyCode::Esc | KeyCode::Backspace | KeyCode::Char('h') | KeyCode::Left => Action::Back,
            KeyCode::Enter => Action::Select,
            KeyCode::Up | KeyCode::Char('k') => Action::Up,
            KeyCode::Down | KeyCode::Char('j') => Action::Down,
            KeyCode::PageUp => Action::PageUp,
            KeyCode::PageDown => Action::PageDown,
            KeyCode::Char('i') => Action::Install { then_play: false },
            KeyCode::Char('p') => Action::Play,
            KeyCode::Char('u') => Action::Update,
            KeyCode::Char('x') | KeyCode::Delete => Action::Uninstall,
            KeyCode::Char('f') => Action::ToggleFavorite,
            KeyCode::Char('l') => Action::ViewLog,
            KeyCode::Char('c') => Action::CancelJob,
            KeyCode::Char('?') => Action::Help,
            KeyCode::Char('q') => Action::Quit,
            _ => return None,
        });
    }
    let common = match key.code {
        KeyCode::Char('q') => Some(Action::Quit),
        KeyCode::Char('?') | KeyCode::F(1) => Some(Action::Help),
        KeyCode::Tab => Some(Action::NextTab),
        KeyCode::BackTab => Some(Action::PrevTab),
        KeyCode::Char(c @ '1'..='6') => {
            Tab::from_index(c as usize - '1' as usize).map(Action::GoTab)
        }
        KeyCode::Up | KeyCode::Char('k') => Some(Action::Up),
        KeyCode::Down | KeyCode::Char('j') => Some(Action::Down),
        KeyCode::PageUp => Some(Action::PageUp),
        KeyCode::PageDown => Some(Action::PageDown),
        KeyCode::Home | KeyCode::Char('g') => Some(Action::First),
        KeyCode::End | KeyCode::Char('G') => Some(Action::Last),
        KeyCode::Enter => Some(Action::Select),
        KeyCode::Esc => Some(Action::Back),
        KeyCode::Char('r') => Some(Action::Refresh),
        _ => None,
    };
    if common.is_some() {
        return common;
    }
    match app.tab {
        Tab::Home => match key.code {
            KeyCode::Char('f') => Some(Action::ToggleFavorite),
            KeyCode::Char('i') => Some(Action::Install { then_play: false }),
            KeyCode::Char('p') => Some(Action::Play),
            KeyCode::Right | KeyCode::Char('l') => Some(Action::NextTab),
            KeyCode::Left | KeyCode::Char('h') => Some(Action::PrevTab),
            _ => None,
        },
        Tab::Discover | Tab::Library | Tab::Favorites => match key.code {
            KeyCode::Char('/') => Some(Action::SearchStart),
            KeyCode::Char('c') => Some(Action::CycleCategory),
            KeyCode::Char('f') => Some(Action::ToggleFavorite),
            KeyCode::Char('i') => Some(Action::Install { then_play: false }),
            KeyCode::Char('p') => Some(Action::Play),
            KeyCode::Char('u') => Some(Action::Update),
            KeyCode::Char('x') | KeyCode::Delete => Some(Action::Uninstall),
            KeyCode::Char('l') => Some(Action::ViewLog),
            KeyCode::Right => Some(Action::NextTab),
            KeyCode::Left => Some(Action::PrevTab),
            _ => None,
        },
        Tab::Updates => match key.code {
            KeyCode::Char('u') => Some(Action::Update),
            KeyCode::Char('a') => Some(Action::UpdateAll),
            KeyCode::Char('f') => Some(Action::ToggleFavorite),
            KeyCode::Right | KeyCode::Char('l') => Some(Action::NextTab),
            KeyCode::Left | KeyCode::Char('h') => Some(Action::PrevTab),
            _ => None,
        },
        Tab::Settings => match key.code {
            KeyCode::Char(' ') => Some(Action::ToggleSetting),
            KeyCode::Right | KeyCode::Char('l') => Some(Action::NextTab),
            KeyCode::Left | KeyCode::Char('h') => Some(Action::PrevTab),
            _ => None,
        },
    }
}

fn map_modal(app: &TuiApp, key: KeyEvent) -> Option<Action> {
    let modal = app.modal.as_ref()?;
    Some(match modal {
        Modal::Welcome | Modal::Help | Modal::Info { .. } => match key.code {
            KeyCode::Enter
            | KeyCode::Esc
            | KeyCode::Char('q')
            | KeyCode::Char(' ')
            | KeyCode::Char('?') => Action::HideModal,
            _ => return None,
        },
        Modal::Planning { .. } => match key.code {
            KeyCode::Esc => Action::Cancel,
            _ => return None,
        },
        Modal::ConfirmInstall { .. }
        | Modal::ConfirmUpdateAll { .. }
        | Modal::ConfirmUninstall { .. }
        | Modal::ConfirmQuit => match key.code {
            KeyCode::Enter | KeyCode::Char('y') | KeyCode::Char('Y') => Action::Confirm,
            KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Char('q') => {
                Action::Cancel
            }
            _ => return None,
        },
        Modal::Progress { .. } => match key.code {
            KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q') => Action::HideModal,
            KeyCode::Char('c') => Action::CancelJob,
            _ => return None,
        },
        Modal::Error { .. } => match key.code {
            KeyCode::Char('r') | KeyCode::Char('R') => Action::Retry,
            KeyCode::Char('l') | KeyCode::Char('L') => Action::ViewLog,
            KeyCode::Esc | KeyCode::Enter | KeyCode::Char('b') | KeyCode::Char('q') => {
                Action::Cancel
            }
            _ => return None,
        },
        Modal::LogView { .. } => match key.code {
            KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q') => Action::Cancel,
            KeyCode::Up | KeyCode::Char('k') => Action::Up,
            KeyCode::Down | KeyCode::Char('j') => Action::Down,
            KeyCode::PageUp => Action::PageUp,
            KeyCode::PageDown => Action::PageDown,
            KeyCode::Home | KeyCode::Char('g') => Action::First,
            KeyCode::End | KeyCode::Char('G') => Action::Last,
            _ => return None,
        },
    })
}
