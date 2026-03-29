use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::action::Action;
use crate::state::{AppState, Direction, InputMode, Overlay, Panel, RequestFocus, RequestTab, ResponseTab, PENDING_KEY_TIMEOUT};

pub fn map_key(key: KeyEvent, state: &AppState) -> Option<Action> {
    // 0. Command Palette consumes all input when open
    if state.command_palette.open {
        return map_command_palette_key(key);
    }

    // 0.5. Search mode consumes input when active
    if state.search.active {
        return map_search_key(key);
    }

    // 0.6. Collections filter mode consumes input when active
    if state.collections_filter_active {
        return map_collections_filter_key(key);
    }

    // 1. Overlays consume input first
    if state.overlay.is_some() {
        return map_overlay_key(key, state);
    }

    // 2. Ctrl shortcuts (work in all modes)
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        match key.code {
            KeyCode::Char('r') => {
                // Ctrl+R: Redo when in any vim editing context, execute request otherwise
                let in_vim_edit = match state.active_panel {
                    Panel::Body => state.mode == InputMode::Normal,
                    Panel::Request => state.request_field_editing,
                    _ => false,
                };
                if in_vim_edit {
                    return Some(Action::Redo);
                }
                return Some(Action::ExecuteRequest);
            }
            KeyCode::Char('v') => {
                // Ctrl+V: Visual Block in normal mode on Body/Response, paste in insert mode
                if state.mode == InputMode::Insert {
                    return Some(Action::PasteFromClipboard);
                }
                return Some(Action::EnterVisualBlockMode);
            }
            KeyCode::Char('d') => return Some(Action::ScrollHalfDown),
            KeyCode::Char('u') => return Some(Action::ScrollHalfUp),
            KeyCode::Char('s') => return Some(Action::ToggleInsecureMode),
            _ => {}
        }
    }

    // 3. Visual mode (both Visual and VisualBlock)
    if state.mode == InputMode::Visual || state.mode == InputMode::VisualBlock {
        return map_visual_mode_key(key);
    }

    // 4. Insert mode
    if state.mode == InputMode::Insert {
        return map_insert_mode_key(key, state);
    }

    // 5. Check pending key (for dd, yy)
    if let Some((pending, instant)) = state.pending_key {
        if instant.elapsed() < PENDING_KEY_TIMEOUT {
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
        if pending == 'q' && instant.elapsed() < PENDING_KEY_TIMEOUT {
            if key.code == KeyCode::Char('q') {
                return Some(Action::Quit);
            }
        }
    }

    // Cancel in-flight request with Esc
    if state.request_in_flight && key.code == KeyCode::Esc {
        return Some(Action::CancelRequest);
    }

    // Global normal mode keys
    match key.code {
        KeyCode::Char('q') => return Some(Action::PendingKey('q')),
        KeyCode::Char('?') => return Some(Action::OpenOverlay(Overlay::Help)),
        KeyCode::Char('T') => return Some(Action::OpenOverlay(Overlay::ThemeSelector { selected: 0 })),
        KeyCode::Char('E') => return Some(Action::OpenOverlay(Overlay::EnvironmentEditor {
            selected: 0,
            editing_key: false,
            new_key: String::new(),
            new_value: String::new(),
            cursor: 0,
        })),
        KeyCode::Char(':') => return Some(Action::OpenCommandPalette),
        // Panel aliases: 1=Collections, 2=Request, 3=Body, 4=Response (only without count prefix)
        KeyCode::Char('1') if state.count_prefix.is_none() => return Some(Action::FocusPanel(Panel::Collections)),
        KeyCode::Char('2') if state.count_prefix.is_none() => return Some(Action::FocusPanel(Panel::Request)),
        KeyCode::Char('3') if state.count_prefix.is_none() => return Some(Action::FocusPanel(Panel::Body)),
        KeyCode::Char('4') if state.count_prefix.is_none() => return Some(Action::FocusPanel(Panel::Response)),
        // Count prefix accumulation (5-9 always, 1-4 when count already started, 0 when count started)
        KeyCode::Char(c @ '1'..='9') if state.count_prefix.is_some() => return Some(Action::AccumulateCount(c.to_digit(10).unwrap())),
        KeyCode::Char(c @ '5'..='9') => return Some(Action::AccumulateCount(c.to_digit(10).unwrap())),
        KeyCode::Char('0') if state.count_prefix.is_some() => return Some(Action::AccumulateCount(0)),
        _ => {}
    }

    // Panel-specific normal mode
    match state.active_panel {
        Panel::Collections => map_collections_key(key),
        Panel::Request => map_request_normal_key(key, state),
        Panel::Body => map_body_normal_key(key),
        Panel::Response => map_response_key(key, state),
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
        Some(Overlay::NewCollection { .. }) | Some(Overlay::RenameRequest { .. }) | Some(Overlay::SetCacheTTL { .. }) => match key.code {
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
        Some(Overlay::MoveRequest { .. }) | Some(Overlay::ThemeSelector { .. }) => match key.code {
            KeyCode::Esc => Some(Action::CloseOverlay),
            KeyCode::Char('j') | KeyCode::Down => Some(Action::OverlayDown),
            KeyCode::Char('k') | KeyCode::Up => Some(Action::OverlayUp),
            KeyCode::Enter => Some(Action::OverlayConfirm),
            _ => None,
        },
        Some(Overlay::EnvironmentEditor { cursor, editing_key, .. }) => {
            let editing = *cursor > 0 || *editing_key;
            if editing {
                // In editing mode: typing into key or value
                match key.code {
                    KeyCode::Esc => Some(Action::CloseOverlay),  // cancel edit
                    KeyCode::Enter => Some(Action::OverlayConfirm), // confirm edit
                    KeyCode::Backspace => Some(Action::OverlayBackspace),
                    KeyCode::Char(c) => Some(Action::OverlayInput(c)),
                    _ => None,
                }
            } else {
                // Navigation mode
                match key.code {
                    KeyCode::Esc => Some(Action::CloseOverlay),
                    KeyCode::Char('j') | KeyCode::Down => Some(Action::OverlayDown),
                    KeyCode::Char('k') | KeyCode::Up => Some(Action::OverlayUp),
                    KeyCode::Char('e') | KeyCode::Enter => Some(Action::OverlayConfirm), // start editing value
                    KeyCode::Char('a') => Some(Action::OverlayInput('a')), // add new
                    KeyCode::Char('d') => Some(Action::OverlayDelete),     // delete
                    _ => None,
                }
            }
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
            Panel::Response if state.response_tab == ResponseTab::Type => Some(Action::InlineCursorUp),
            _ => None,
        },
        KeyCode::Down => match state.active_panel {
            Panel::Body => Some(Action::InlineCursorDown),
            Panel::Response if state.response_tab == ResponseTab::Type => Some(Action::InlineCursorDown),
            _ => None,
        },
        KeyCode::Home => Some(Action::InlineCursorHome),
        KeyCode::End => Some(Action::InlineCursorEnd),
        KeyCode::Tab => Some(Action::InlineTab),
        KeyCode::Enter => match state.active_panel {
            Panel::Body => Some(Action::InlineNewline),
            Panel::Response if state.response_tab == ResponseTab::Type => Some(Action::InlineNewline),
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
                RequestFocus::PathParam(_) if state.path_param_edit_field == 0 => {
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
        KeyCode::Char('e') => Some(Action::BodyWordEnd),
        KeyCode::Char('g') => Some(Action::ScrollTop),
        KeyCode::Char('G') => Some(Action::ScrollBottom),
        KeyCode::Char('0') | KeyCode::Home => Some(Action::BodyLineHome),
        KeyCode::Char('$') | KeyCode::End => Some(Action::BodyLineEnd),
        KeyCode::Char('f') => Some(Action::PendingKey('f')),
        KeyCode::Char('F') => Some(Action::PendingKey('F')),
        KeyCode::Char('t') => Some(Action::PendingKey('t')),
        KeyCode::Char('T') => Some(Action::PendingKey('T')),
        _ => None,
    }
}

fn map_pending_key(pending: char, key: KeyEvent, state: &AppState) -> Option<Action> {
    match (pending, key.code) {
        ('d', KeyCode::Char('d')) => match state.active_panel {
            Panel::Request if state.request_field_editing => Some(Action::DeleteLine),
            Panel::Request => match state.request_focus {
                RequestFocus::Header(_) => Some(Action::DeleteHeader),
                RequestFocus::Param(_) => Some(Action::DeleteParam),
                RequestFocus::Cookie(_) => Some(Action::DeleteCookie),
                RequestFocus::PathParam(_) => Some(Action::DeletePathParam),
                _ => None,
            },
            Panel::Body => Some(Action::DeleteLine),
            Panel::Collections => Some(Action::DeleteSelected),
            _ => None,
        },
        ('d', KeyCode::Char('w')) => Some(Action::DeleteWord),
        ('d', KeyCode::Char('e')) => Some(Action::DeleteWordEnd),
        ('d', KeyCode::Char('b')) => Some(Action::DeleteWordBack),
        ('d', KeyCode::Char('$')) => Some(Action::DeleteToEnd),
        ('d', KeyCode::Char('0')) => Some(Action::DeleteToStart),
        ('d', KeyCode::Char('G')) => Some(Action::DeleteToBottom),
        ('c', KeyCode::Char('c')) => Some(Action::ChangeLine),
        ('c', KeyCode::Char('w')) | ('c', KeyCode::Char('e')) => Some(Action::ChangeWord),
        ('c', KeyCode::Char('b')) => Some(Action::ChangeWordBack),
        ('c', KeyCode::Char('$')) => Some(Action::ChangeToEnd),
        ('c', KeyCode::Char('0')) => Some(Action::ChangeToStart),
        ('y', KeyCode::Char('y')) => match state.active_panel {
            Panel::Collections => Some(Action::YankRequest),
            _ => Some(Action::YankLine),
        },
        ('y', KeyCode::Char('w')) => Some(Action::YankWord),
        ('y', KeyCode::Char('$')) => Some(Action::YankToEnd),
        ('y', KeyCode::Char('0')) => Some(Action::YankToStart),
        ('y', KeyCode::Char('G')) => Some(Action::YankToBottom),
        ('r', KeyCode::Char(c)) => {
            // r + char: replace character under cursor
            match state.active_panel {
                Panel::Body => Some(Action::ReplaceChar(c)),
                Panel::Request if state.request_field_editing => Some(Action::ReplaceChar(c)),
                _ => None,
            }
        },
        // z-fold keys (collections panel)
        ('z', KeyCode::Char('o')) if state.active_panel == Panel::Collections => Some(Action::ExpandCollection),
        ('z', KeyCode::Char('c')) if state.active_panel == Panel::Collections => Some(Action::CollapseCollection),
        ('z', KeyCode::Char('a')) if state.active_panel == Panel::Collections => Some(Action::ToggleCollapse),
        ('z', KeyCode::Char('M')) if state.active_panel == Panel::Collections => Some(Action::CollapseAll),
        ('z', KeyCode::Char('R')) if state.active_panel == Panel::Collections => Some(Action::ExpandAll),
        // f/F/t/T find char motions
        ('f', KeyCode::Char(c)) => Some(Action::FindCharForward(c)),
        ('F', KeyCode::Char(c)) => Some(Action::FindCharBackward(c)),
        ('t', KeyCode::Char(c)) => Some(Action::FindCharForwardBefore(c)),
        ('T', KeyCode::Char(c)) => Some(Action::FindCharBackwardAfter(c)),
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
        KeyCode::Char('/') => Some(Action::StartCollectionsFilter),
        KeyCode::Char('z') => Some(Action::PendingKey('z')),
        _ => None,
    }
}

fn map_request_normal_key(key: KeyEvent, state: &AppState) -> Option<Action> {
    // Field-edit normal mode: vim motions inside a field
    if state.request_field_editing {
        return map_request_field_edit_key(key);
    }

    // Panel navigation mode: move between fields
    match key.code {
        KeyCode::Char('j') | KeyCode::Down => Some(Action::RequestFocusDown),
        KeyCode::Char('k') | KeyCode::Up => Some(Action::RequestFocusUp),
        KeyCode::Char(']') => Some(Action::NextMethod),
        KeyCode::Char('[') => Some(Action::PrevMethod),
        KeyCode::Char('}') => Some(Action::RequestNextTab),
        KeyCode::Char('{') => Some(Action::RequestPrevTab),
        KeyCode::Char(' ') => Some(Action::ToggleItemEnabled),
        KeyCode::Char('e') => Some(Action::EnterRequestFieldEdit),
        KeyCode::Char('a') => match state.request_tab {
            RequestTab::Headers => Some(Action::AddHeader),
            RequestTab::Queries => Some(Action::AddParam),
            RequestTab::Cookies => Some(Action::AddCookie),
            RequestTab::Params => Some(Action::AddPathParam),
        },
        KeyCode::Char('A') => Some(Action::ShowHeaderAutocomplete),
        KeyCode::Char('d') => Some(Action::PendingKey('d')),
        KeyCode::Char('x') => match state.request_focus {
            RequestFocus::Header(_) => Some(Action::DeleteHeader),
            RequestFocus::Param(_) => Some(Action::DeleteParam),
            RequestFocus::Cookie(_) => Some(Action::DeleteCookie),
            RequestFocus::PathParam(_) => Some(Action::DeletePathParam),
            _ => None,
        },
        KeyCode::Char('p') => Some(Action::OpenOverlay(Overlay::EnvironmentSelector)),
        KeyCode::Char('y') => Some(Action::CopyResponseBody),
        KeyCode::Char('Y') => Some(Action::CopyAsCurl),
        _ => None,
    }
}

fn map_request_field_edit_key(key: KeyEvent) -> Option<Action> {
    match key.code {
        // Vim motions
        KeyCode::Char('h') | KeyCode::Left => Some(Action::InlineCursorLeft),
        KeyCode::Char('l') | KeyCode::Right => Some(Action::InlineCursorRight),
        KeyCode::Char('w') => Some(Action::BodyWordForward),
        KeyCode::Char('b') => Some(Action::BodyWordBackward),
        KeyCode::Char('e') => Some(Action::BodyWordEnd),
        KeyCode::Char('0') | KeyCode::Home => Some(Action::InlineCursorHome),
        KeyCode::Char('$') | KeyCode::End => Some(Action::InlineCursorEnd),
        // Enter insert mode
        KeyCode::Char('i') => Some(Action::EnterInsertMode),
        KeyCode::Char('I') => Some(Action::EnterInsertModeStart),
        KeyCode::Char('a') => Some(Action::EnterAppendMode),
        KeyCode::Char('A') => Some(Action::EnterAppendModeEnd),
        // Visual mode
        KeyCode::Char('v') => Some(Action::EnterVisualMode),
        // Edit
        KeyCode::Char('x') => Some(Action::DeleteCharUnderCursor),
        KeyCode::Char('s') => Some(Action::Substitute),
        KeyCode::Char('C') => Some(Action::ChangeToEnd),
        KeyCode::Char('D') => Some(Action::DeleteToEnd),
        KeyCode::Char('c') => Some(Action::PendingKey('c')),
        KeyCode::Char('d') => Some(Action::PendingKey('d')),
        KeyCode::Char('r') => Some(Action::PendingKey('r')),
        KeyCode::Char('y') => Some(Action::PendingKey('y')),
        KeyCode::Char('u') => Some(Action::Undo),
        KeyCode::Char('p') | KeyCode::Char('P') => Some(Action::Paste),
        // Find char motions
        KeyCode::Char('f') => Some(Action::PendingKey('f')),
        KeyCode::Char('F') => Some(Action::PendingKey('F')),
        KeyCode::Char('t') => Some(Action::PendingKey('t')),
        KeyCode::Char('T') => Some(Action::PendingKey('T')),
        // Tab to switch between name/value sub-fields
        KeyCode::Tab => Some(Action::InlineTab),
        // Exit field editing
        KeyCode::Esc => Some(Action::ExitRequestFieldEdit),
        _ => None,
    }
}

fn map_body_normal_key(key: KeyEvent) -> Option<Action> {
    match key.code {
        // Body tab switching
        KeyCode::Char('}') => return Some(Action::BodyNextTab),
        KeyCode::Char('{') => return Some(Action::BodyPrevTab),
        // Vim motions
        KeyCode::Char('j') | KeyCode::Down => Some(Action::ScrollDown),
        KeyCode::Char('k') | KeyCode::Up => Some(Action::ScrollUp),
        KeyCode::Char('g') => Some(Action::ScrollTop),
        KeyCode::Char('G') => Some(Action::ScrollBottom),
        KeyCode::Char('w') => Some(Action::BodyWordForward),
        KeyCode::Char('b') => Some(Action::BodyWordBackward),
        KeyCode::Char('e') => Some(Action::BodyWordEnd),
        KeyCode::Char('0') | KeyCode::Home => Some(Action::BodyLineHome),
        KeyCode::Char('$') | KeyCode::End => Some(Action::BodyLineEnd),
        // Enter modes
        KeyCode::Char('i') => Some(Action::EnterInsertMode),
        KeyCode::Char('I') => Some(Action::EnterInsertModeStart),
        KeyCode::Char('a') => Some(Action::EnterAppendMode),
        KeyCode::Char('A') => Some(Action::EnterAppendModeEnd),
        KeyCode::Char('o') => Some(Action::OpenLineBelow),
        KeyCode::Char('O') => Some(Action::OpenLineAbove),
        KeyCode::Char('v') => Some(Action::EnterVisualMode),
        // Edit
        KeyCode::Char('x') => Some(Action::DeleteCharUnderCursor),
        KeyCode::Char('s') => Some(Action::Substitute),
        KeyCode::Char('S') => Some(Action::ChangeLine),
        KeyCode::Char('C') => Some(Action::ChangeToEnd),
        KeyCode::Char('D') => Some(Action::DeleteToEnd),
        KeyCode::Char('c') => Some(Action::PendingKey('c')),
        KeyCode::Char('r') => Some(Action::PendingKey('r')),
        KeyCode::Char('u') => Some(Action::Undo),
        KeyCode::Char('p') | KeyCode::Char('P') => Some(Action::Paste),
        KeyCode::Char('d') => Some(Action::PendingKey('d')),
        KeyCode::Char('y') => Some(Action::PendingKey('y')),
        // Body type (use T for cycle, t for find-char-before)
        KeyCode::Char('f') => Some(Action::PendingKey('f')),
        KeyCode::Char('F') => Some(Action::PendingKey('F')),
        KeyCode::Char('t') => Some(Action::PendingKey('t')),
        KeyCode::Char('T') => Some(Action::PendingKey('T')),
        // Search
        KeyCode::Char('/') => Some(Action::StartSearch),
        KeyCode::Char('n') => Some(Action::SearchNext),
        KeyCode::Char('N') => Some(Action::SearchPrev),
        _ => None,
    }
}

fn map_response_key(key: KeyEvent, state: &AppState) -> Option<Action> {
    if state.response_tab == ResponseTab::Type {
        return match key.code {
            KeyCode::Char('j') | KeyCode::Down => Some(Action::ScrollDown),
            KeyCode::Char('k') | KeyCode::Up => Some(Action::ScrollUp),
            KeyCode::Char('g') => Some(Action::ScrollTop),
            KeyCode::Char('G') => Some(Action::ScrollBottom),
            KeyCode::Char('i') => Some(Action::EnterInsertMode),
            KeyCode::Char('R') => Some(Action::RegenerateType),
            KeyCode::Char('}') => Some(Action::ResponseNextTab),
            KeyCode::Char('{') => Some(Action::ResponsePrevTab),
            _ => None,
        };
    }

    match key.code {
        KeyCode::Char('}') => Some(Action::ResponseNextTab),
        KeyCode::Char('{') => Some(Action::ResponsePrevTab),
        KeyCode::Char('j') | KeyCode::Down => Some(Action::ScrollDown),
        KeyCode::Char('k') | KeyCode::Up => Some(Action::ScrollUp),
        KeyCode::Char('h') | KeyCode::Left => Some(Action::InlineCursorLeft),
        KeyCode::Char('l') | KeyCode::Right => Some(Action::InlineCursorRight),
        KeyCode::Char('g') => Some(Action::ScrollTop),
        KeyCode::Char('G') => Some(Action::ScrollBottom),
        KeyCode::Char('w') => Some(Action::BodyWordForward),
        KeyCode::Char('b') => Some(Action::BodyWordBackward),
        KeyCode::Char('e') => Some(Action::BodyWordEnd),
        KeyCode::Char('0') | KeyCode::Home => Some(Action::BodyLineHome),
        KeyCode::Char('$') | KeyCode::End => Some(Action::BodyLineEnd),
        KeyCode::Char('v') => Some(Action::EnterVisualMode),
        KeyCode::Char('y') => Some(Action::CopyResponseBody),
        KeyCode::Char('Y') => Some(Action::CopyAsCurl),
        KeyCode::Char('p') => Some(Action::OpenOverlay(Overlay::EnvironmentSelector)),
        KeyCode::Char('H') => Some(Action::ToggleResponseHeaders),
        KeyCode::Char('/') => Some(Action::StartSearch),
        KeyCode::Char('n') => Some(Action::SearchNext),
        KeyCode::Char('N') => Some(Action::SearchPrev),
        _ => None,
    }
}

fn map_search_key(key: KeyEvent) -> Option<Action> {
    match key.code {
        KeyCode::Esc => Some(Action::SearchCancel),
        KeyCode::Enter => Some(Action::SearchConfirm),
        KeyCode::Backspace => Some(Action::SearchBackspace),
        KeyCode::Char(c) => Some(Action::SearchInput(c)),
        _ => None,
    }
}

fn map_collections_filter_key(key: KeyEvent) -> Option<Action> {
    match key.code {
        KeyCode::Esc => Some(Action::CollectionsFilterCancel),
        KeyCode::Enter => Some(Action::CollectionsFilterConfirm),
        KeyCode::Backspace => Some(Action::CollectionsFilterBackspace),
        KeyCode::Char(c) => Some(Action::CollectionsFilterInput(c)),
        _ => None,
    }
}
