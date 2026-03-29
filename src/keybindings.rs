use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::action::Action;
use crate::state::{AppState, Direction, InputMode, Overlay, Panel, RequestFocus, RequestTab};

pub fn map_key(key: KeyEvent, state: &AppState) -> Option<Action> {
    // 0. Command Palette consumes all input when open
    if state.command_palette_open {
        return map_command_palette_key(key);
    }

    // 1. Overlays consume input first
    if state.overlay.is_some() {
        return map_overlay_key(key, state);
    }

    // 2. Ctrl shortcuts (work in all modes)
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        match key.code {
            KeyCode::Char('r') => return Some(Action::ExecuteRequest),
            KeyCode::Char('v') => return Some(Action::PasteFromClipboard),
            KeyCode::Char('d') => return Some(Action::ScrollHalfDown),
            KeyCode::Char('u') => return Some(Action::ScrollHalfUp),
            KeyCode::Char('s') => return Some(Action::ToggleInsecureMode),
            _ => {}
        }
    }

    // 3. Visual mode
    if state.mode == InputMode::Visual {
        return map_visual_mode_key(key);
    }

    // 4. Insert mode
    if state.mode == InputMode::Insert {
        return map_insert_mode_key(key, state);
    }

    // 5. Check pending key (for dd, yy)
    if let Some((pending, instant)) = state.pending_key {
        if instant.elapsed() < std::time::Duration::from_millis(500) {
            return map_pending_key(pending, key, state);
        }
    }

    // 6. Normal mode
    map_normal_mode_key(key, state)
}

fn map_normal_mode_key(key: KeyEvent, state: &AppState) -> Option<Action> {
    // Ctrl+h/j/k/l for panel navigation
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        return match key.code {
            KeyCode::Char('h') => Some(Action::NavigatePanel(Direction::Left)),
            KeyCode::Char('j') => Some(Action::NavigatePanel(Direction::Down)),
            KeyCode::Char('k') => Some(Action::NavigatePanel(Direction::Up)),
            KeyCode::Char('l') => Some(Action::NavigatePanel(Direction::Right)),
            _ => None,
        };
    }

    // qq to quit (pending key)
    if let Some((pending, instant)) = state.pending_key {
        if pending == 'q' && instant.elapsed() < std::time::Duration::from_millis(500) {
            if key.code == KeyCode::Char('q') {
                return Some(Action::Quit);
            }
        }
    }

    // Global normal mode keys
    match key.code {
        KeyCode::Char('q') => return Some(Action::PendingKey('q')),
        KeyCode::Char('?') => return Some(Action::OpenOverlay(Overlay::Help)),
        KeyCode::Char('T') => return Some(Action::CycleTheme),
        KeyCode::Char(':') => return Some(Action::OpenCommandPalette),
        // Panel aliases: 1=Collections, 2=Request, 3=Body, 4=Response
        KeyCode::Char('1') => return Some(Action::FocusPanel(Panel::Collections)),
        KeyCode::Char('2') => return Some(Action::FocusPanel(Panel::Request)),
        KeyCode::Char('3') => return Some(Action::FocusPanel(Panel::Body)),
        KeyCode::Char('4') => return Some(Action::FocusPanel(Panel::Response)),
        _ => {}
    }

    // Panel-specific normal mode
    match state.active_panel {
        Panel::Collections => map_collections_key(key),
        Panel::Request => map_request_normal_key(key, state),
        Panel::Body => map_body_normal_key(key),
        Panel::Response => map_response_key(key),
    }
}

fn map_command_palette_key(key: KeyEvent) -> Option<Action> {
    // Ctrl shortcuts inside palette
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        return match key.code {
            KeyCode::Char('n') | KeyCode::Char('j') => Some(Action::CommandPaletteDown),
            KeyCode::Char('p') | KeyCode::Char('k') => Some(Action::CommandPaletteUp),
            _ => None,
        };
    }

    match key.code {
        KeyCode::Esc => Some(Action::CommandPaletteClose),
        KeyCode::Enter => Some(Action::CommandPaletteConfirm),
        KeyCode::Char(c) => Some(Action::CommandPaletteInput(c)),
        KeyCode::Backspace => Some(Action::CommandPaletteBackspace),
        KeyCode::Up | KeyCode::BackTab => Some(Action::CommandPaletteUp),
        KeyCode::Down | KeyCode::Tab => Some(Action::CommandPaletteDown),
        _ => None,
    }
}

fn map_overlay_key(key: KeyEvent, state: &AppState) -> Option<Action> {
    // Ctrl+N / Ctrl+P for cmp-style navigation in all overlays
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        return match key.code {
            KeyCode::Char('n') | KeyCode::Char('j') => Some(Action::OverlayDown),
            KeyCode::Char('p') | KeyCode::Char('k') => Some(Action::OverlayUp),
            _ => None,
        };
    }

    match &state.overlay {
        Some(Overlay::HeaderAutocomplete { .. }) => match key.code {
            KeyCode::Esc => Some(Action::CloseOverlay),
            KeyCode::Char('j') | KeyCode::Down => Some(Action::OverlayDown),
            KeyCode::Char('k') | KeyCode::Up => Some(Action::OverlayUp),
            KeyCode::Enter => Some(Action::OverlayConfirm),
            _ => None,
        },
        Some(Overlay::NewCollection { .. }) | Some(Overlay::RenameRequest { .. }) => match key.code {
            KeyCode::Esc => Some(Action::CloseOverlay),
            KeyCode::Enter => Some(Action::OverlayConfirm),
            KeyCode::Backspace => Some(Action::OverlayBackspace),
            KeyCode::Char(c) => Some(Action::OverlayInput(c)),
            _ => None,
        },
        Some(Overlay::ConfirmDelete { .. }) => match key.code {
            KeyCode::Esc | KeyCode::Char('n') => Some(Action::CloseOverlay),
            KeyCode::Enter | KeyCode::Char('y') => Some(Action::OverlayConfirm),
            _ => None,
        },
        Some(Overlay::MoveRequest { .. }) => match key.code {
            KeyCode::Esc => Some(Action::CloseOverlay),
            KeyCode::Char('j') | KeyCode::Down => Some(Action::OverlayDown),
            KeyCode::Char('k') | KeyCode::Up => Some(Action::OverlayUp),
            KeyCode::Enter => Some(Action::OverlayConfirm),
            _ => None,
        },
        _ => match key.code {
            KeyCode::Esc | KeyCode::Char('?') => Some(Action::CloseOverlay),
            KeyCode::Char('j') | KeyCode::Down => Some(Action::OverlayDown),
            KeyCode::Char('k') | KeyCode::Up => Some(Action::OverlayUp),
            KeyCode::Enter => Some(Action::OverlayConfirm),
            _ => None,
        },
    }
}

fn map_insert_mode_key(key: KeyEvent, state: &AppState) -> Option<Action> {
    // Ctrl combos in insert mode
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        return match key.code {
            KeyCode::Char('h') => Some(Action::NavigatePanel(Direction::Left)),
            KeyCode::Char('j') => Some(Action::NavigatePanel(Direction::Down)),
            KeyCode::Char('k') => Some(Action::NavigatePanel(Direction::Up)),
            KeyCode::Char('l') => Some(Action::NavigatePanel(Direction::Right)),
            // nvim-cmp style autocomplete
            KeyCode::Char('n') => Some(Action::AutocompleteNext),
            KeyCode::Char('p') => Some(Action::AutocompletePrev),
            KeyCode::Char('y') => Some(Action::AutocompleteAccept),
            _ => None,
        };
    }

    match key.code {
        KeyCode::Esc => Some(Action::ExitInsertMode),
        KeyCode::Char(c) => Some(Action::InlineInput(c)),
        KeyCode::Backspace => Some(Action::InlineBackspace),
        KeyCode::Delete => Some(Action::InlineDelete),
        KeyCode::Left => Some(Action::InlineCursorLeft),
        KeyCode::Right => Some(Action::InlineCursorRight),
        KeyCode::Up => match state.active_panel {
            Panel::Body => Some(Action::InlineCursorUp),
            _ => None,
        },
        KeyCode::Down => match state.active_panel {
            Panel::Body => Some(Action::InlineCursorDown),
            _ => None,
        },
        KeyCode::Home => Some(Action::InlineCursorHome),
        KeyCode::End => Some(Action::InlineCursorEnd),
        KeyCode::Tab => Some(Action::InlineTab),
        KeyCode::Enter => match state.active_panel {
            Panel::Body => Some(Action::InlineNewline),
            Panel::Request => match state.request_focus {
                RequestFocus::Header(_) if state.header_edit_field == 0 => {
                    Some(Action::InlineTab)
                }
                RequestFocus::Param(_) if state.param_edit_field == 0 => {
                    Some(Action::InlineTab)
                }
                RequestFocus::Cookie(_) if state.cookie_edit_field == 0 => {
                    Some(Action::InlineTab)
                }
                _ => Some(Action::ExitInsertMode),
            },
            _ => Some(Action::ExitInsertMode),
        },
        _ => None,
    }
}

fn map_visual_mode_key(key: KeyEvent) -> Option<Action> {
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        return match key.code {
            KeyCode::Char('h') => Some(Action::NavigatePanel(Direction::Left)),
            KeyCode::Char('j') => Some(Action::NavigatePanel(Direction::Down)),
            KeyCode::Char('k') => Some(Action::NavigatePanel(Direction::Up)),
            KeyCode::Char('l') => Some(Action::NavigatePanel(Direction::Right)),
            _ => None,
        };
    }

    match key.code {
        KeyCode::Esc => Some(Action::ExitVisualMode),
        KeyCode::Char('y') => Some(Action::VisualYank),
        KeyCode::Char('d') | KeyCode::Char('x') => Some(Action::VisualDelete),
        KeyCode::Char('j') | KeyCode::Down => Some(Action::InlineCursorDown),
        KeyCode::Char('k') | KeyCode::Up => Some(Action::InlineCursorUp),
        KeyCode::Char('h') | KeyCode::Left => Some(Action::InlineCursorLeft),
        KeyCode::Char('l') | KeyCode::Right => Some(Action::InlineCursorRight),
        KeyCode::Char('w') => Some(Action::BodyWordForward),
        KeyCode::Char('b') => Some(Action::BodyWordBackward),
        KeyCode::Char('g') => Some(Action::ScrollTop),
        KeyCode::Char('G') => Some(Action::ScrollBottom),
        KeyCode::Char('0') | KeyCode::Home => Some(Action::BodyLineHome),
        KeyCode::Char('$') | KeyCode::End => Some(Action::BodyLineEnd),
        _ => None,
    }
}

fn map_pending_key(pending: char, key: KeyEvent, state: &AppState) -> Option<Action> {
    match (pending, key.code) {
        ('d', KeyCode::Char('d')) => match state.active_panel {
            Panel::Request => match state.request_focus {
                RequestFocus::Header(_) => Some(Action::DeleteHeader),
                RequestFocus::Param(_) => Some(Action::DeleteParam),
                RequestFocus::Cookie(_) => Some(Action::DeleteCookie),
                _ => None,
            },
            Panel::Body => Some(Action::YankLine),
            Panel::Collections => Some(Action::DeleteSelected),
            _ => None,
        },
        ('y', KeyCode::Char('y')) => match state.active_panel {
            Panel::Collections => Some(Action::YankRequest),
            _ => Some(Action::YankLine),
        },
        _ => map_normal_mode_key(key, state),
    }
}

fn map_collections_key(key: KeyEvent) -> Option<Action> {
    match key.code {
        KeyCode::Char('j') | KeyCode::Down => Some(Action::ScrollDown),
        KeyCode::Char('k') | KeyCode::Up => Some(Action::ScrollUp),
        KeyCode::Char('g') => Some(Action::ScrollTop),
        KeyCode::Char('G') => Some(Action::ScrollBottom),
        KeyCode::Char(' ') => Some(Action::ToggleCollapse),
        KeyCode::Enter => Some(Action::SelectRequest),
        KeyCode::Char('n') => Some(Action::CreateCollection),
        KeyCode::Char('s') => Some(Action::SaveRequest),
        KeyCode::Char('S') => Some(Action::SaveRequestAs),
        KeyCode::Char('C') => Some(Action::NewEmptyRequest),
        KeyCode::Char('r') => Some(Action::RenameRequest),
        KeyCode::Char('d') => Some(Action::PendingKey('d')),
        KeyCode::Char('m') => Some(Action::MoveRequest),
        KeyCode::Char('y') => Some(Action::PendingKey('y')),
        KeyCode::Char('p') => Some(Action::PasteRequest),
        KeyCode::Char('Y') => Some(Action::CopyAsCurl),
        KeyCode::Char('L') | KeyCode::Char('}') => Some(Action::NextCollection),
        KeyCode::Char('H') | KeyCode::Char('{') => Some(Action::PrevCollection),
        _ => None,
    }
}

fn map_request_normal_key(key: KeyEvent, state: &AppState) -> Option<Action> {
    match key.code {
        KeyCode::Char('j') | KeyCode::Down => Some(Action::RequestFocusDown),
        KeyCode::Char('k') | KeyCode::Up => Some(Action::RequestFocusUp),
        KeyCode::Char(']') => Some(Action::NextMethod),
        KeyCode::Char('[') => Some(Action::PrevMethod),
        KeyCode::Char('}') => Some(Action::RequestNextTab),
        KeyCode::Char('{') => Some(Action::RequestPrevTab),
        KeyCode::Char(' ') => Some(Action::ToggleItemEnabled),
        KeyCode::Char('i') | KeyCode::Char('e') => Some(Action::EnterInsertMode),
        KeyCode::Char('a') => match state.request_tab {
            RequestTab::Headers => Some(Action::AddHeader),
            RequestTab::Queries => Some(Action::AddParam),
            RequestTab::Cookies => Some(Action::AddCookie),
        },
        KeyCode::Char('A') => Some(Action::ShowHeaderAutocomplete),
        KeyCode::Char('d') => Some(Action::PendingKey('d')),
        KeyCode::Char('x') => match state.request_focus {
            RequestFocus::Header(_) => Some(Action::DeleteHeader),
            RequestFocus::Param(_) => Some(Action::DeleteParam),
            RequestFocus::Cookie(_) => Some(Action::DeleteCookie),
            _ => None,
        },
        KeyCode::Char('p') => Some(Action::OpenOverlay(Overlay::EnvironmentSelector)),
        KeyCode::Char('y') => Some(Action::CopyResponseBody),
        KeyCode::Char('Y') => Some(Action::CopyAsCurl),
        _ => None,
    }
}

fn map_body_normal_key(key: KeyEvent) -> Option<Action> {
    match key.code {
        // Vim motions
        KeyCode::Char('j') | KeyCode::Down => Some(Action::ScrollDown),
        KeyCode::Char('k') | KeyCode::Up => Some(Action::ScrollUp),
        KeyCode::Char('g') => Some(Action::ScrollTop),
        KeyCode::Char('G') => Some(Action::ScrollBottom),
        KeyCode::Char('w') => Some(Action::BodyWordForward),
        KeyCode::Char('b') => Some(Action::BodyWordBackward),
        KeyCode::Char('0') | KeyCode::Home => Some(Action::BodyLineHome),
        KeyCode::Char('$') | KeyCode::End => Some(Action::BodyLineEnd),
        // Enter modes
        KeyCode::Char('i') => Some(Action::EnterInsertMode),
        KeyCode::Char('I') => Some(Action::EnterInsertModeStart),
        KeyCode::Char('a') => Some(Action::EnterAppendMode),
        KeyCode::Char('A') => Some(Action::EnterInsertModeStart), // A = end of line + insert
        KeyCode::Char('o') => Some(Action::OpenLineBelow),
        KeyCode::Char('O') => Some(Action::OpenLineAbove),
        KeyCode::Char('v') => Some(Action::EnterVisualMode),
        // Edit
        KeyCode::Char('p') | KeyCode::Char('P') => Some(Action::Paste),
        KeyCode::Char('d') => Some(Action::PendingKey('d')),
        KeyCode::Char('y') => Some(Action::PendingKey('y')),
        // Body type
        KeyCode::Char('t') => Some(Action::CycleBodyType),
        _ => None,
    }
}

fn map_response_key(key: KeyEvent) -> Option<Action> {
    match key.code {
        KeyCode::Char('j') | KeyCode::Down => Some(Action::ScrollDown),
        KeyCode::Char('k') | KeyCode::Up => Some(Action::ScrollUp),
        KeyCode::Char('h') | KeyCode::Left => Some(Action::InlineCursorLeft),
        KeyCode::Char('l') | KeyCode::Right => Some(Action::InlineCursorRight),
        KeyCode::Char('g') => Some(Action::ScrollTop),
        KeyCode::Char('G') => Some(Action::ScrollBottom),
        KeyCode::Char('w') => Some(Action::BodyWordForward),
        KeyCode::Char('b') => Some(Action::BodyWordBackward),
        KeyCode::Char('0') | KeyCode::Home => Some(Action::BodyLineHome),
        KeyCode::Char('$') | KeyCode::End => Some(Action::BodyLineEnd),
        KeyCode::Char('v') => Some(Action::EnterVisualMode),
        KeyCode::Char('y') => Some(Action::CopyResponseBody),
        KeyCode::Char('Y') => Some(Action::CopyAsCurl),
        KeyCode::Char('p') => Some(Action::OpenOverlay(Overlay::EnvironmentSelector)),
        _ => None,
    }
}
