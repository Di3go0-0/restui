use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::action::Action;
use crate::keybinding_config::{KeyBind, KeybindingsConfig};
use crate::state::{AppState, Direction, InputMode, Overlay, Panel, RequestFocus, RequestTab, ResponseTab, TypeSubFocus, PENDING_KEY_TIMEOUT};

pub fn map_key(key: KeyEvent, state: &AppState) -> Option<Action> {
    let kb = &state.keybindings;
    let k = KeyBind::from_event(key);

    // 0. Command Palette consumes all input when open
    if state.command_palette.open {
        return map_command_palette_key(&k, key, kb);
    }

    // 0.5. Search mode consumes input when active
    if state.search.active {
        return map_search_key(&k, key, kb);
    }

    // 0.6. Collections filter mode consumes input when active
    if state.collections_filter_active {
        return map_collections_filter_key(&k, key, kb);
    }

    // 1. Overlays consume input first
    if state.overlay.is_some() {
        return map_overlay_key(&k, key, state, kb);
    }

    // 2. Global ctrl shortcuts (work in all modes)
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        if let Some(action) = map_global_ctrl(&k, state, kb) {
            return Some(action);
        }
    }

    // 2.5. Body panel: delegate to VimEditor for all vim operations
    if state.active_panel == Panel::Body {
        // Visual mode → all to VimEditor
        if state.mode == InputMode::Visual || state.mode == InputMode::VisualBlock {
            return Some(Action::BodyVimInput(key));
        }

        // Insert mode → check app keys first, then VimEditor
        if state.mode == InputMode::Insert {
            if key.modifiers.contains(KeyModifiers::CONTROL) {
                match key.code {
                    KeyCode::Char('n') => return Some(Action::AutocompleteNext),
                    KeyCode::Char('p') => return Some(Action::AutocompletePrev),
                    KeyCode::Char('y') => return Some(Action::AutocompleteAccept),
                    // Panel navigation works even in insert mode
                    KeyCode::Char('h') => return Some(Action::NavigatePanel(Direction::Left)),
                    KeyCode::Char('j') => return Some(Action::NavigatePanel(Direction::Down)),
                    KeyCode::Char('k') => return Some(Action::NavigatePanel(Direction::Up)),
                    KeyCode::Char('l') => return Some(Action::NavigatePanel(Direction::Right)),
                    _ => {}
                }
            }
            return Some(Action::BodyVimInput(key));
        }

        // Normal mode → check app keys, then VimEditor
        // (skip pending key handling — VimEditor handles operator+motion internally)
        if let Some(action) = map_body_app_key(&k, kb) {
            return Some(action);
        }
        return Some(Action::BodyVimInput(key));
    }

    // 2.6. Response panel: delegate to VimEditor
    if state.active_panel == Panel::Response {
        let is_type_editor = state.response_tab == ResponseTab::Type
            && state.type_sub_focus == TypeSubFocus::Editor;

        if is_type_editor {
            // Type editor: full vim editing via VimEditor
            if state.mode == InputMode::Visual || state.mode == InputMode::VisualBlock {
                return Some(Action::TypeVimInput(key));
            }
            if state.mode == InputMode::Insert {
                // Panel navigation works even in insert mode
                if key.modifiers.contains(KeyModifiers::CONTROL) {
                    match key.code {
                        KeyCode::Char('h') => return Some(Action::NavigatePanel(Direction::Left)),
                        KeyCode::Char('j') => return Some(Action::NavigatePanel(Direction::Down)),
                        KeyCode::Char('k') => return Some(Action::NavigatePanel(Direction::Up)),
                        KeyCode::Char('l') => return Some(Action::NavigatePanel(Direction::Right)),
                        _ => {}
                    }
                }
                return Some(Action::TypeVimInput(key));
            }
            // Normal mode: check app keys first
            if let Some(action) = map_response_app_key(&k, state, kb) {
                return Some(action);
            }
            return Some(Action::TypeVimInput(key));
        }

        // Response body / type preview: read-only vim (scroll, visual, search)
        if state.mode == InputMode::Visual || state.mode == InputMode::VisualBlock {
            return Some(Action::RespVimInput(key));
        }
        // Normal mode: check app keys first
        if let Some(action) = map_response_app_key(&k, state, kb) {
            return Some(action);
        }
        return Some(Action::RespVimInput(key));
    }

    // 3. Visual mode (both Visual and VisualBlock)
    if state.mode == InputMode::Visual || state.mode == InputMode::VisualBlock {
        return map_visual_mode_key(&k, kb);
    }

    // 4. Insert mode
    if state.mode == InputMode::Insert {
        return map_insert_mode_key(&k, key, state, kb);
    }

    // 5. Check pending key (for dd, yy, etc.)
    if let Some((pending, instant)) = state.pending_key {
        if instant.elapsed() < PENDING_KEY_TIMEOUT {
            return map_pending_key(pending, key, state);
        }
    }

    // 6. Normal mode
    map_normal_mode_key(&k, key, state, kb)
}

// ── Helpers ────────────────────────────────────────────────────────────────

fn lookup<'a>(ctx: &'a std::collections::HashMap<KeyBind, String>, key: &KeyBind) -> Option<&'a str> {
    KeybindingsConfig::lookup(ctx, key)
}

// ── Global Ctrl shortcuts ──────────────────────────────────────────────────

fn map_global_ctrl(k: &KeyBind, state: &AppState, kb: &KeybindingsConfig) -> Option<Action> {
    match lookup(&kb.global, k)? {
        "ctrl_r" | "execute_request" | "redo" => {
            // Ctrl+R: Redo when in any vim editing context, execute request otherwise
            let in_vim_edit = match state.active_panel {
                Panel::Body => state.mode == InputMode::Normal,
                Panel::Request => state.request_field_editing,
                Panel::Response if state.response_tab == ResponseTab::Type && state.type_sub_focus == TypeSubFocus::Editor => state.mode == InputMode::Normal,
                _ => false,
            };
            if in_vim_edit { Some(Action::Redo) } else { Some(Action::ExecuteRequest) }
        }
        "ctrl_v" | "paste_clipboard" | "visual_block" => {
            if state.mode == InputMode::Insert {
                Some(Action::PasteFromClipboard)
            } else {
                Some(Action::EnterVisualBlockMode)
            }
        }
        "scroll_half_down" => Some(Action::ScrollHalfDown),
        "scroll_half_up" => Some(Action::ScrollHalfUp),
        "toggle_insecure" => Some(Action::ToggleInsecureMode),
        "save_request" => Some(Action::SaveRequest),
        _ => None,
    }
}

// ── Normal mode ────────────────────────────────────────────────────────────

fn map_normal_mode_key(k: &KeyBind, key: KeyEvent, state: &AppState, kb: &KeybindingsConfig) -> Option<Action> {
    // Ctrl+J/K: sub-focus navigation within Type tab (before panel navigation)
    if key.modifiers.contains(KeyModifiers::CONTROL)
        && state.active_panel == Panel::Response
        && state.response_tab == ResponseTab::Type
    {
        if let KeyCode::Char('j') = key.code {
            if state.type_sub_focus == TypeSubFocus::Editor {
                return Some(Action::TypeSubFocusDown);
            }
        }
        if let KeyCode::Char('k') = key.code {
            if state.type_sub_focus == TypeSubFocus::Preview {
                return Some(Action::TypeSubFocusUp);
            }
        }
    }

    // Ctrl+h/j/k/l for panel navigation
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        return match lookup(&kb.global, k)? {
            "navigate_left" => Some(Action::NavigatePanel(Direction::Left)),
            "navigate_down" => Some(Action::NavigatePanel(Direction::Down)),
            "navigate_up" => Some(Action::NavigatePanel(Direction::Up)),
            "navigate_right" => Some(Action::NavigatePanel(Direction::Right)),
            _ => None,
        };
    }

    // Exit diff view with Esc or q
    if state.viewing_diff.is_some() && state.active_panel == Panel::Response {
        if key.code == KeyCode::Esc || key.code == KeyCode::Char('q') {
            return Some(Action::ExitDiffView);
        }
    }

    // Clear search highlights with Esc in normal mode
    if key.code == KeyCode::Esc && !state.search.query.is_empty() && state.mode == InputMode::Normal {
        return Some(Action::SearchCancel);
    }

    // Cancel in-flight request with Esc
    if state.request_in_flight && key.code == KeyCode::Esc {
        return Some(Action::CancelRequest);
    }

    // Global normal mode keys
    if let Some(action) = lookup(&kb.global, k) {
        match action {
            "quit" => return Some(Action::Quit),
            "help" => return Some(Action::OpenOverlay(Overlay::Help)),
            "open_theme_selector" if state.active_panel == Panel::Collections || (state.active_panel == Panel::Request && !state.request_field_editing) => {
                return Some(Action::OpenOverlay(Overlay::ThemeSelector { selected: 0 }));
            }
            "open_history" if state.active_panel == Panel::Collections || (state.active_panel == Panel::Request && !state.request_field_editing) => {
                return Some(Action::OpenOverlay(Overlay::History { selected: 0 }));
            }
            "open_env_editor" => return Some(Action::OpenOverlay(Overlay::EnvironmentEditor {
                selected: 0, editing_key: false, new_key: String::new(), new_value: String::new(), cursor: 0,
            })),
            "open_command_palette" => return Some(Action::OpenCommandPalette),
            "focus_panel_1" if state.count_prefix.is_none() => return Some(Action::FocusPanel(Panel::Collections)),
            "focus_panel_2" if state.count_prefix.is_none() => return Some(Action::FocusPanel(Panel::Request)),
            "focus_panel_3" if state.count_prefix.is_none() => return Some(Action::FocusPanel(Panel::Body)),
            "focus_panel_4" if state.count_prefix.is_none() => return Some(Action::FocusPanel(Panel::Response)),
            _ => {}
        }
    }

    // Count prefix accumulation (hardcoded — numeric input concern, not keybinding)
    if let KeyCode::Char(c @ '1'..='9') = key.code {
        if state.count_prefix.is_some() {
            return Some(Action::AccumulateCount(c.to_digit(10).unwrap()));
        }
    }
    if let KeyCode::Char(c @ '5'..='9') = key.code {
        return Some(Action::AccumulateCount(c.to_digit(10).unwrap()));
    }
    if let KeyCode::Char('0') = key.code {
        if state.count_prefix.is_some() {
            return Some(Action::AccumulateCount(0));
        }
    }

    // Panel-specific normal mode
    match state.active_panel {
        Panel::Collections => map_collections_key(k, kb),
        Panel::Request => map_request_normal_key(k, state, kb),
        Panel::Body => map_body_app_key(k, kb),
        Panel::Response => map_response_key(k, state, kb),
    }
}

// ── Collections ────────────────────────────────────────────────────────────

fn map_collections_key(k: &KeyBind, kb: &KeybindingsConfig) -> Option<Action> {
    match lookup(&kb.collections, k)? {
        "scroll_down" => Some(Action::ScrollDown),
        "scroll_up" => Some(Action::ScrollUp),
        "scroll_top" => Some(Action::ScrollTop),
        "scroll_bottom" => Some(Action::ScrollBottom),
        "toggle_collapse" => Some(Action::ToggleCollapse),
        "select_request" => Some(Action::SelectRequest),
        "create_collection" => Some(Action::CreateCollection),
        "save_request" => Some(Action::SaveRequest),
        "save_request_as" => Some(Action::SaveRequestAs),
        "add_request" => Some(Action::AddRequestToCollection),
        "new_empty_request" => Some(Action::NewEmptyRequest),
        "rename_request" => Some(Action::RenameRequest),
        "delete_pending" => Some(Action::PendingKey('d')),
        "move_request" => Some(Action::MoveRequest),
        "yank_pending" => Some(Action::PendingKey('y')),
        "paste_request" => Some(Action::PasteRequest),
        "copy_as_curl" => Some(Action::CopyAsCurl),
        "next_collection" => Some(Action::NextCollection),
        "prev_collection" => Some(Action::PrevCollection),
        "start_filter" => Some(Action::StartCollectionsFilter),
        "fold_pending" => Some(Action::PendingKey('z')),
        _ => None,
    }
}

// ── Request (panel navigation) ─────────────────────────────────────────────

fn map_request_normal_key(k: &KeyBind, state: &AppState, kb: &KeybindingsConfig) -> Option<Action> {
    if state.request_field_editing {
        return map_request_field_edit_key(k, kb);
    }

    match lookup(&kb.request, k)? {
        "focus_down" => Some(Action::RequestFocusDown),
        "focus_up" => Some(Action::RequestFocusUp),
        "next_method" => Some(Action::NextMethod),
        "prev_method" => Some(Action::PrevMethod),
        "next_tab" => Some(Action::RequestNextTab),
        "prev_tab" => Some(Action::RequestPrevTab),
        "toggle_enabled" => Some(Action::ToggleItemEnabled),
        "enter_field_edit" => Some(Action::EnterRequestFieldEdit),
        "add_item" => match state.request_tab {
            RequestTab::Headers => Some(Action::AddHeader),
            RequestTab::Queries => Some(Action::AddParam),
            RequestTab::Cookies => Some(Action::AddCookie),
            RequestTab::Params => Some(Action::AddPathParam),
        },
        "show_autocomplete" => Some(Action::ShowHeaderAutocomplete),
        "delete_pending" => Some(Action::PendingKey('d')),
        "delete_item" => match state.request_focus {
            RequestFocus::Header(_) => Some(Action::DeleteHeader),
            RequestFocus::Param(_) => Some(Action::DeleteParam),
            RequestFocus::Cookie(_) => Some(Action::DeleteCookie),
            RequestFocus::PathParam(_) => Some(Action::DeletePathParam),
            _ => None,
        },
        "open_env_selector" => Some(Action::OpenOverlay(Overlay::EnvironmentSelector)),
        "copy_response" => Some(Action::CopyResponseBody),
        "copy_as_curl" => Some(Action::CopyAsCurl),
        _ => None,
    }
}

// ── Request field edit (vim normal inside a field) ──────────────────────────

fn map_request_field_edit_key(k: &KeyBind, kb: &KeybindingsConfig) -> Option<Action> {
    match lookup(&kb.request_field, k)? {
        "cursor_left" => Some(Action::InlineCursorLeft),
        "cursor_right" => Some(Action::InlineCursorRight),
        "word_forward" => Some(Action::BodyWordForward),
        "word_backward" => Some(Action::BodyWordBackward),
        "word_end" => Some(Action::BodyWordEnd),
        "line_home" => Some(Action::InlineCursorHome),
        "line_end" => Some(Action::InlineCursorEnd),
        "enter_insert" => Some(Action::EnterInsertMode),
        "enter_insert_start" => Some(Action::EnterInsertModeStart),
        "enter_append" => Some(Action::EnterAppendMode),
        "enter_append_end" => Some(Action::EnterAppendModeEnd),
        "enter_visual" => Some(Action::EnterVisualMode),
        "delete_char" => Some(Action::DeleteCharUnderCursor),
        "substitute" => Some(Action::Substitute),
        "change_to_end" => Some(Action::ChangeToEnd),
        "delete_to_end" => Some(Action::DeleteToEnd),
        "change_pending" => Some(Action::PendingKey('c')),
        "delete_pending" => Some(Action::PendingKey('d')),
        "replace_pending" => Some(Action::PendingKey('r')),
        "yank_pending" => Some(Action::PendingKey('y')),
        "undo" => Some(Action::Undo),
        "paste" => Some(Action::Paste),
        "find_forward" => Some(Action::PendingKey('f')),
        "find_backward" => Some(Action::PendingKey('F')),
        "find_before" => Some(Action::PendingKey('t')),
        "find_after" => Some(Action::PendingKey('T')),
        "tab" => Some(Action::InlineTab),
        "exit_field_edit" => Some(Action::ExitRequestFieldEdit),
        _ => None,
    }
}

// ── Body (app-specific keys only; all vim ops go through BodyVimInput) ───

fn map_body_app_key(k: &KeyBind, kb: &KeybindingsConfig) -> Option<Action> {
    match lookup(&kb.body, k)? {
        "next_tab" => Some(Action::BodyNextTab),
        "prev_tab" => Some(Action::BodyPrevTab),
        "start_search" => Some(Action::StartSearch),
        "search_next" => Some(Action::SearchNext),
        "search_prev" => Some(Action::SearchPrev),
        _ => None,
    }
}

// ── Response (app-specific keys only; vim ops go through RespVimInput/TypeVimInput) ──

fn map_response_app_key(k: &KeyBind, state: &AppState, kb: &KeybindingsConfig) -> Option<Action> {
    let ctx = response_context(state, kb);
    match lookup(ctx, k)? {
        "next_tab" => Some(Action::ResponseNextTab),
        "prev_tab" => Some(Action::ResponsePrevTab),
        "copy_response" => Some(Action::CopyResponseBody),
        "copy_as_curl" => Some(Action::CopyAsCurl),
        "start_search" => Some(Action::StartSearch),
        "search_next" => Some(Action::SearchNext),
        "search_prev" => Some(Action::SearchPrev),
        "open_env_selector" => Some(Action::OpenOverlay(Overlay::EnvironmentSelector)),
        "toggle_headers" => Some(Action::ToggleResponseHeaders),
        "response_history" => Some(Action::OpenOverlay(Overlay::ResponseHistory { selected: 0 })),
        "response_diff" => Some(Action::OpenOverlay(Overlay::ResponseDiffSelect { selected: 0 })),
        "type_lang_next" if state.response_tab == ResponseTab::Type => Some(Action::TypeLangNext),
        "type_lang_prev" if state.response_tab == ResponseTab::Type => Some(Action::TypeLangPrev),
        "regenerate_type" if state.response_tab == ResponseTab::Type && state.type_sub_focus == TypeSubFocus::Editor => Some(Action::RegenerateType),
        "export_response" => Some(Action::ExportResponse),
        "toggle_wrap" => Some(Action::ToggleWrap),
        _ => None,
    }
}

fn map_response_key(k: &KeyBind, state: &AppState, kb: &KeybindingsConfig) -> Option<Action> {
    // Tab switching is shared across all response sub-tabs
    // Check the specific sub-context map first for tab actions
    let ctx = response_context(state, kb);

    // Type language sub-tab switching (only in Type tab)
    if state.response_tab == ResponseTab::Type {
        if let Some(action) = lookup(ctx, k) {
            match action {
                "type_lang_next" => return Some(Action::TypeLangNext),
                "type_lang_prev" => return Some(Action::TypeLangPrev),
                _ => {}
            }
        }
    }

    match lookup(ctx, k)? {
        "scroll_down" => Some(Action::ScrollDown),
        "scroll_up" => Some(Action::ScrollUp),
        "cursor_left" => Some(Action::InlineCursorLeft),
        "cursor_right" => Some(Action::InlineCursorRight),
        "scroll_top" => Some(Action::ScrollTop),
        "scroll_bottom" => Some(Action::ScrollBottom),
        "word_forward" => Some(Action::BodyWordForward),
        "word_backward" => Some(Action::BodyWordBackward),
        "word_end" => Some(Action::BodyWordEnd),
        "line_home" => Some(Action::BodyLineHome),
        "line_end" => Some(Action::BodyLineEnd),
        "enter_visual" => Some(Action::EnterVisualMode),
        "copy_response" => Some(Action::CopyResponseBody),
        "copy_as_curl" => Some(Action::CopyAsCurl),
        "find_forward" => Some(Action::PendingKey('f')),
        "find_backward" => Some(Action::PendingKey('F')),
        "find_before" => Some(Action::PendingKey('t')),
        "find_after" => Some(Action::PendingKey('T')),
        "start_search" => Some(Action::StartSearch),
        "search_next" => Some(Action::SearchNext),
        "search_prev" => Some(Action::SearchPrev),
        "next_tab" => Some(Action::ResponseNextTab),
        "prev_tab" => Some(Action::ResponsePrevTab),
        // Response body specific
        "open_env_selector" => Some(Action::OpenOverlay(Overlay::EnvironmentSelector)),
        "toggle_headers" => Some(Action::ToggleResponseHeaders),
        "response_history" => Some(Action::OpenOverlay(Overlay::ResponseHistory { selected: 0 })),
        "response_diff" => Some(Action::OpenOverlay(Overlay::ResponseDiffSelect { selected: 0 })),
        // Type editor specific (only works when in editor sub-focus)
        "enter_insert" if state.response_tab == ResponseTab::Type && state.type_sub_focus == TypeSubFocus::Editor => Some(Action::EnterInsertMode),
        "enter_insert_start" if state.response_tab == ResponseTab::Type && state.type_sub_focus == TypeSubFocus::Editor => Some(Action::EnterInsertModeStart),
        "enter_append" if state.response_tab == ResponseTab::Type && state.type_sub_focus == TypeSubFocus::Editor => Some(Action::EnterAppendMode),
        "enter_append_end" if state.response_tab == ResponseTab::Type && state.type_sub_focus == TypeSubFocus::Editor => Some(Action::EnterAppendModeEnd),
        "open_line_below" if state.response_tab == ResponseTab::Type && state.type_sub_focus == TypeSubFocus::Editor => Some(Action::OpenLineBelow),
        "open_line_above" if state.response_tab == ResponseTab::Type && state.type_sub_focus == TypeSubFocus::Editor => Some(Action::OpenLineAbove),
        "delete_char" if state.response_tab == ResponseTab::Type && state.type_sub_focus == TypeSubFocus::Editor => Some(Action::DeleteCharUnderCursor),
        "substitute" if state.response_tab == ResponseTab::Type && state.type_sub_focus == TypeSubFocus::Editor => Some(Action::Substitute),
        "change_line" if state.response_tab == ResponseTab::Type && state.type_sub_focus == TypeSubFocus::Editor => Some(Action::ChangeLine),
        "change_to_end" if state.response_tab == ResponseTab::Type && state.type_sub_focus == TypeSubFocus::Editor => Some(Action::ChangeToEnd),
        "delete_to_end_line" if state.response_tab == ResponseTab::Type && state.type_sub_focus == TypeSubFocus::Editor => Some(Action::DeleteToEnd),
        "change_pending" if state.response_tab == ResponseTab::Type && state.type_sub_focus == TypeSubFocus::Editor => Some(Action::PendingKey('c')),
        "replace_pending" if state.response_tab == ResponseTab::Type && state.type_sub_focus == TypeSubFocus::Editor => Some(Action::PendingKey('r')),
        "delete_pending" if state.response_tab == ResponseTab::Type && state.type_sub_focus == TypeSubFocus::Editor => Some(Action::PendingKey('d')),
        "yank_pending" if state.response_tab == ResponseTab::Type && state.type_sub_focus == TypeSubFocus::Editor => Some(Action::PendingKey('y')),
        "undo" if state.response_tab == ResponseTab::Type && state.type_sub_focus == TypeSubFocus::Editor => Some(Action::Undo),
        "paste" if state.response_tab == ResponseTab::Type && state.type_sub_focus == TypeSubFocus::Editor => Some(Action::Paste),
        "regenerate_type" if state.response_tab == ResponseTab::Type && state.type_sub_focus == TypeSubFocus::Editor => Some(Action::RegenerateType),
        _ => None,
    }
}

fn response_context<'a>(state: &AppState, kb: &'a KeybindingsConfig) -> &'a std::collections::HashMap<KeyBind, String> {
    if state.response_tab == ResponseTab::Type {
        if state.type_sub_focus == TypeSubFocus::Editor {
            &kb.response_type_editor
        } else {
            &kb.response_type_preview
        }
    } else {
        &kb.response_body
    }
}

// ── Visual mode ────────────────────────────────────────────────────────────

fn map_visual_mode_key(k: &KeyBind, kb: &KeybindingsConfig) -> Option<Action> {
    match lookup(&kb.visual, k)? {
        "exit_visual" => Some(Action::ExitVisualMode),
        "yank" => Some(Action::VisualYank),
        "delete" => Some(Action::VisualDelete),
        "paste" => Some(Action::VisualPaste),
        "cursor_down" => Some(Action::InlineCursorDown),
        "cursor_up" => Some(Action::InlineCursorUp),
        "cursor_left" => Some(Action::InlineCursorLeft),
        "cursor_right" => Some(Action::InlineCursorRight),
        "word_forward" => Some(Action::BodyWordForward),
        "word_backward" => Some(Action::BodyWordBackward),
        "word_end" => Some(Action::BodyWordEnd),
        "scroll_top" => Some(Action::ScrollTop),
        "scroll_bottom" => Some(Action::ScrollBottom),
        "line_home" => Some(Action::BodyLineHome),
        "line_end" => Some(Action::BodyLineEnd),
        "find_forward" => Some(Action::PendingKey('f')),
        "find_backward" => Some(Action::PendingKey('F')),
        "find_before" => Some(Action::PendingKey('t')),
        "find_after" => Some(Action::PendingKey('T')),
        "navigate_left" => Some(Action::NavigatePanel(Direction::Left)),
        "navigate_down" => Some(Action::NavigatePanel(Direction::Down)),
        "navigate_up" => Some(Action::NavigatePanel(Direction::Up)),
        "navigate_right" => Some(Action::NavigatePanel(Direction::Right)),
        _ => None,
    }
}

// ── Insert mode ────────────────────────────────────────────────────────────

fn map_insert_mode_key(k: &KeyBind, key: KeyEvent, state: &AppState, kb: &KeybindingsConfig) -> Option<Action> {
    // Check configurable bindings first
    if let Some(action) = lookup(&kb.insert, k) {
        match action {
            "exit_insert" => return Some(Action::ExitInsertMode),
            "navigate_left" => return Some(Action::NavigatePanel(Direction::Left)),
            "navigate_down" => return Some(Action::NavigatePanel(Direction::Down)),
            "navigate_up" => return Some(Action::NavigatePanel(Direction::Up)),
            "navigate_right" => return Some(Action::NavigatePanel(Direction::Right)),
            "autocomplete_next" => return Some(Action::AutocompleteNext),
            "autocomplete_prev" => return Some(Action::AutocompletePrev),
            "autocomplete_accept" => return Some(Action::AutocompleteAccept),
            _ => {}
        }
    }

    // Non-configurable: raw character/key input
    match key.code {
        KeyCode::Char(c) => Some(Action::InlineInput(c)),
        KeyCode::Backspace => Some(Action::InlineBackspace),
        KeyCode::Delete => Some(Action::InlineDelete),
        KeyCode::Left => Some(Action::InlineCursorLeft),
        KeyCode::Right => Some(Action::InlineCursorRight),
        KeyCode::Up => match state.active_panel {
            Panel::Response if state.response_tab == ResponseTab::Type && state.type_sub_focus == TypeSubFocus::Editor => Some(Action::InlineCursorUp),
            _ => None,
        },
        KeyCode::Down => match state.active_panel {
            Panel::Response if state.response_tab == ResponseTab::Type && state.type_sub_focus == TypeSubFocus::Editor => Some(Action::InlineCursorDown),
            _ => None,
        },
        KeyCode::Home => Some(Action::InlineCursorHome),
        KeyCode::End => Some(Action::InlineCursorEnd),
        KeyCode::Tab => Some(Action::InlineTab),
        KeyCode::Enter => match state.active_panel {
            Panel::Response if state.response_tab == ResponseTab::Type && state.type_sub_focus == TypeSubFocus::Editor => Some(Action::InlineNewline),
            Panel::Request => match state.request_focus {
                RequestFocus::Header(_) if state.header_edit_field == 0 => Some(Action::InlineTab),
                RequestFocus::Param(_) if state.param_edit_field == 0 => Some(Action::InlineTab),
                RequestFocus::Cookie(_) if state.cookie_edit_field == 0 => Some(Action::InlineTab),
                RequestFocus::PathParam(_) if state.path_param_edit_field == 0 => Some(Action::InlineTab),
                _ => Some(Action::ExitInsertMode),
            },
            _ => Some(Action::ExitInsertMode),
        },
        _ => None,
    }
}

// ── Pending keys (dd, yy, cw, etc.) ────────────────────────────────────────
// The pending system stays hardcoded because it follows vim grammar:
// operator (d/c/y/r/z/f/F/t/T) + motion (d/w/b/$/'0/G/etc.)
// The TRIGGER keys for operators are configurable in each context.

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
            Panel::Response if state.response_tab == ResponseTab::Type && state.type_sub_focus == TypeSubFocus::Editor => Some(Action::DeleteLine),
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
        ('r', KeyCode::Char(c)) => match state.active_panel {
            Panel::Body => Some(Action::ReplaceChar(c)),
            Panel::Request if state.request_field_editing => Some(Action::ReplaceChar(c)),
            Panel::Response if state.response_tab == ResponseTab::Type && state.type_sub_focus == TypeSubFocus::Editor => Some(Action::ReplaceChar(c)),
            _ => None,
        },
        // z-fold keys (collections panel)
        ('z', KeyCode::Char('o')) if state.active_panel == Panel::Collections => Some(Action::ExpandCollection),
        ('z', KeyCode::Char('c')) if state.active_panel == Panel::Collections => Some(Action::CollapseCollection),
        ('z', KeyCode::Char('a')) if state.active_panel == Panel::Collections => Some(Action::ToggleCollapse),
        ('z', KeyCode::Char('M')) if state.active_panel == Panel::Collections => Some(Action::CollapseAll),
        ('z', KeyCode::Char('R')) if state.active_panel == Panel::Collections => Some(Action::ExpandAll),
        // f/F/t/T find char motions (hardcoded — the char IS the data)
        ('f', KeyCode::Char(c)) => Some(Action::FindCharForward(c)),
        ('F', KeyCode::Char(c)) => Some(Action::FindCharBackward(c)),
        ('t', KeyCode::Char(c)) => Some(Action::FindCharForwardBefore(c)),
        ('T', KeyCode::Char(c)) => Some(Action::FindCharBackwardAfter(c)),
        _ => map_normal_mode_key(&KeyBind::from_event(key), key, state, &state.keybindings),
    }
}

// ── Command palette ────────────────────────────────────────────────────────

fn map_command_palette_key(k: &KeyBind, key: KeyEvent, kb: &KeybindingsConfig) -> Option<Action> {
    // Check configurable bindings first
    if let Some(action) = lookup(&kb.command_palette, k) {
        match action {
            "close" => return Some(Action::CommandPaletteClose),
            "confirm" => return Some(Action::CommandPaletteConfirm),
            "nav_up" => return Some(Action::CommandPaletteUp),
            "nav_down" => return Some(Action::CommandPaletteDown),
            _ => {}
        }
    }

    // Non-configurable: text input
    match key.code {
        KeyCode::Char(c) => Some(Action::CommandPaletteInput(c)),
        KeyCode::Backspace => Some(Action::CommandPaletteBackspace),
        _ => None,
    }
}

// ── Search ─────────────────────────────────────────────────────────────────

fn map_search_key(k: &KeyBind, key: KeyEvent, kb: &KeybindingsConfig) -> Option<Action> {
    if let Some(action) = lookup(&kb.search, k) {
        match action {
            "cancel" => return Some(Action::SearchCancel),
            "confirm" => return Some(Action::SearchConfirm),
            _ => {}
        }
    }

    // Non-configurable: text input
    match key.code {
        KeyCode::Backspace => Some(Action::SearchBackspace),
        KeyCode::Char(c) => Some(Action::SearchInput(c)),
        _ => None,
    }
}

// ── Collections filter ─────────────────────────────────────────────────────

fn map_collections_filter_key(k: &KeyBind, key: KeyEvent, kb: &KeybindingsConfig) -> Option<Action> {
    if let Some(action) = lookup(&kb.collections_filter, k) {
        match action {
            "cancel" => return Some(Action::CollectionsFilterCancel),
            "confirm" => return Some(Action::CollectionsFilterConfirm),
            _ => {}
        }
    }

    match key.code {
        KeyCode::Backspace => Some(Action::CollectionsFilterBackspace),
        KeyCode::Char(c) => Some(Action::CollectionsFilterInput(c)),
        _ => None,
    }
}

// ── Overlay ────────────────────────────────────────────────────────────────

fn map_overlay_key(k: &KeyBind, key: KeyEvent, state: &AppState, kb: &KeybindingsConfig) -> Option<Action> {
    // Overlay-specific behavior stays hardcoded since each overlay type has unique input needs
    match &state.overlay {
        Some(Overlay::HeaderAutocomplete { .. }) => {
            if let Some(action) = lookup(&kb.overlay, k) {
                match action {
                    "close" => return Some(Action::CloseOverlay),
                    "nav_down" => return Some(Action::OverlayDown),
                    "nav_up" => return Some(Action::OverlayUp),
                    "confirm" => return Some(Action::OverlayConfirm),
                    _ => {}
                }
            }
            None
        }
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
        Some(Overlay::MoveRequest { .. }) | Some(Overlay::ThemeSelector { .. }) | Some(Overlay::ResponseHistory { .. }) | Some(Overlay::ResponseDiffSelect { .. }) => {
            if let Some(action) = lookup(&kb.overlay, k) {
                match action {
                    "close" => return Some(Action::CloseOverlay),
                    "nav_down" => return Some(Action::OverlayDown),
                    "nav_up" => return Some(Action::OverlayUp),
                    "confirm" => return Some(Action::OverlayConfirm),
                    _ => {}
                }
            }
            None
        }
        Some(Overlay::EnvironmentEditor { cursor, editing_key, .. }) => {
            let editing = *cursor > 0 || *editing_key;
            if editing {
                match key.code {
                    KeyCode::Esc => Some(Action::CloseOverlay),
                    KeyCode::Enter => Some(Action::OverlayConfirm),
                    KeyCode::Backspace => Some(Action::OverlayBackspace),
                    KeyCode::Char(c) => Some(Action::OverlayInput(c)),
                    _ => None,
                }
            } else {
                match key.code {
                    KeyCode::Esc => Some(Action::CloseOverlay),
                    KeyCode::Char('j') | KeyCode::Down => Some(Action::OverlayDown),
                    KeyCode::Char('k') | KeyCode::Up => Some(Action::OverlayUp),
                    KeyCode::Char('e') | KeyCode::Enter => Some(Action::OverlayConfirm),
                    KeyCode::Char('a') => Some(Action::OverlayInput('a')),
                    KeyCode::Char('d') => Some(Action::OverlayDelete),
                    _ => None,
                }
            }
        }
        _ => {
            if let Some(action) = lookup(&kb.overlay, k) {
                match action {
                    "close" => return Some(Action::CloseOverlay),
                    "nav_down" => return Some(Action::OverlayDown),
                    "nav_up" => return Some(Action::OverlayUp),
                    "confirm" => return Some(Action::OverlayConfirm),
                    _ => {}
                }
            }
            // Help overlay can also be closed with '?'
            if let KeyCode::Char('?') = key.code {
                if matches!(state.overlay, Some(Overlay::Help)) {
                    return Some(Action::CloseOverlay);
                }
            }
            None
        }
    }
}
