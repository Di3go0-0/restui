use crate::core::state::{InputMode, Panel, ResponseTab};
use vimltui::{VimMode, VisualKind};

use super::App;

impl App {
    /// Save current active type text/buf to the storage for the current lang.
    pub(super) fn swap_type_lang_out(&mut self) {
        match self.state.response_view.type_lang {
            crate::core::state::TypeLang::Inferred => {} // response_type_text IS the active text
            crate::core::state::TypeLang::TypeScript => {
                std::mem::swap(&mut self.state.response_view.type_text, &mut self.state.response_view.type_ts_text);
                std::mem::swap(&mut self.state.response_view.type_vim, &mut self.state.response_view.type_ts_vim);
            }
            crate::core::state::TypeLang::CSharp => {
                std::mem::swap(&mut self.state.response_view.type_text, &mut self.state.response_view.type_csharp_text);
                std::mem::swap(&mut self.state.response_view.type_vim, &mut self.state.response_view.type_csharp_vim);
            }
        }
    }

    /// Load the type text/buf for the new lang into the active slots.
    pub(super) fn swap_type_lang_in(&mut self) {
        match self.state.response_view.type_lang {
            crate::core::state::TypeLang::Inferred => {} // response_type_text IS the active text
            crate::core::state::TypeLang::TypeScript => {
                std::mem::swap(&mut self.state.response_view.type_text, &mut self.state.response_view.type_ts_text);
                std::mem::swap(&mut self.state.response_view.type_vim, &mut self.state.response_view.type_ts_vim);
            }
            crate::core::state::TypeLang::CSharp => {
                std::mem::swap(&mut self.state.response_view.type_text, &mut self.state.response_view.type_csharp_text);
                std::mem::swap(&mut self.state.response_view.type_vim, &mut self.state.response_view.type_csharp_vim);
            }
        }
    }

    /// Sync response_type_text from type_vim's internal lines.
    pub(super) fn sync_type_vim_text(&mut self) {
        self.state.response_view.type_text = self.state.response_view.type_vim.content();
    }

    /// Sync app input mode from body_vim's mode.
    pub(super) fn sync_mode_from_vim(&mut self) {
        self.state.mode = match &self.state.body_vim.mode {
            VimMode::Normal => InputMode::Normal,
            VimMode::Insert => InputMode::Insert,
            VimMode::Visual(VisualKind::Block) => InputMode::VisualBlock,
            VimMode::Visual(_) => InputMode::Visual,
        };
    }

    pub(super) fn sync_mode_from_vim_resp(&mut self) {
        self.state.mode = match &self.state.response_view.resp_vim.mode {
            VimMode::Normal => InputMode::Normal,
            VimMode::Insert => InputMode::Insert,
            VimMode::Visual(VisualKind::Block) => InputMode::VisualBlock,
            VimMode::Visual(_) => InputMode::Visual,
        };
    }

    pub(super) fn sync_mode_from_vim_type(&mut self) {
        self.state.mode = match &self.state.response_view.type_vim.mode {
            VimMode::Normal => InputMode::Normal,
            VimMode::Insert => InputMode::Insert,
            VimMode::Visual(VisualKind::Block) => InputMode::VisualBlock,
            VimMode::Visual(_) => InputMode::Visual,
        };
    }

    pub(super) fn validate_response_type(&mut self) {
        self.state.response_view.type_validation_errors.clear();

        let resp = match &self.state.current_response {
            Some(r) => r,
            None => return,
        };

        let json_val = match serde_json::from_str::<serde_json::Value>(&resp.body) {
            Ok(v) => v,
            Err(_) => {
                self.state.response_view.type_validation_errors.push("Response body is not valid JSON".to_string());
                return;
            }
        };

        let user_type = match crate::model::response_type::parse_type_text(&self.state.response_view.type_text) {
            Ok(t) => t,
            Err(e) => {
                self.state.response_view.type_validation_errors.push(format!("Type parse error: {}", e));
                return;
            }
        };

        let mismatches = user_type.validate(&json_val);
        for m in mismatches {
            self.state.response_view.type_validation_errors.push(
                format!("{}: expected {}, got {}", m.path, m.expected, m.actual)
            );
        }
    }

    /// Find char forward on the current line (f/t motion).
    /// If `before` is true, stop one position before the found char (t motion).
    pub(super) fn find_char_forward(&mut self, target: char, before: bool) {
        match self.state.active_panel {
            Panel::Body => {
                let body = self.state.current_request.get_body(self.state.body_type);
                let lines: Vec<&str> = body.lines().collect();
                if let Some(line) = lines.get(self.state.body_vim.cursor_row) {
                    let bytes = line.as_bytes();
                    let start = self.state.body_vim.cursor_col + 1;
                    for i in start..bytes.len() {
                        if bytes[i] == target as u8 {
                            self.state.body_vim.cursor_col = if before { i.saturating_sub(1).max(start.saturating_sub(1)) } else { i };
                            break;
                        }
                    }
                }
            }
            Panel::Request if self.state.request_edit.field_editing => {
                let text = self.get_request_field_text();
                let bytes = text.as_bytes();
                let cursor = self.get_request_cursor();
                let start = cursor + 1;
                for i in start..bytes.len() {
                    if bytes[i] == target as u8 {
                        self.set_request_cursor(if before { i.saturating_sub(1).max(start.saturating_sub(1)) } else { i });
                        break;
                    }
                }
            }
            Panel::Response if self.state.response_view.tab == ResponseTab::Type && self.state.response_view.type_sub_focus == crate::core::state::TypeSubFocus::Editor => {
                self.state.response_view.type_vim.find_char_forward(target, before);
            }
            Panel::Response => {
                let text = self.get_response_body_text();
                let lines: Vec<&str> = text.lines().collect();
                if let Some(line) = lines.get(self.state.response_view.resp_vim.cursor_row) {
                    let bytes = line.as_bytes();
                    let start = self.state.response_view.resp_vim.cursor_col + 1;
                    for i in start..bytes.len() {
                        if bytes[i] == target as u8 {
                            self.state.response_view.resp_vim.cursor_col = if before { i.saturating_sub(1).max(start.saturating_sub(1)) } else { i };
                            break;
                        }
                    }
                }
            }
            _ => {}
        }
    }

    /// Find char backward on the current line (F/T motion).
    /// If `after` is true, stop one position after the found char (T motion).
    pub(super) fn find_char_backward(&mut self, target: char, after: bool) {
        match self.state.active_panel {
            Panel::Body => {
                let body = self.state.current_request.get_body(self.state.body_type);
                let lines: Vec<&str> = body.lines().collect();
                if let Some(line) = lines.get(self.state.body_vim.cursor_row) {
                    let bytes = line.as_bytes();
                    let col = self.state.body_vim.cursor_col;
                    if col > 0 {
                        for i in (0..col).rev() {
                            if bytes[i] == target as u8 {
                                self.state.body_vim.cursor_col = if after { (i + 1).min(col) } else { i };
                                break;
                            }
                        }
                    }
                }
            }
            Panel::Request if self.state.request_edit.field_editing => {
                let text = self.get_request_field_text();
                let bytes = text.as_bytes();
                let cursor = self.get_request_cursor();
                if cursor > 0 {
                    for i in (0..cursor).rev() {
                        if bytes[i] == target as u8 {
                            self.set_request_cursor(if after { (i + 1).min(cursor) } else { i });
                            break;
                        }
                    }
                }
            }
            Panel::Response if self.state.response_view.tab == ResponseTab::Type && self.state.response_view.type_sub_focus == crate::core::state::TypeSubFocus::Editor => {
                self.state.response_view.type_vim.find_char_backward(target, after);
            }
            Panel::Response => {
                let text = self.get_response_body_text();
                let lines: Vec<&str> = text.lines().collect();
                if let Some(line) = lines.get(self.state.response_view.resp_vim.cursor_row) {
                    let bytes = line.as_bytes();
                    let col = self.state.response_view.resp_vim.cursor_col;
                    if col > 0 {
                        for i in (0..col).rev() {
                            if bytes[i] == target as u8 {
                                self.state.response_view.resp_vim.cursor_col = if after { (i + 1).min(col) } else { i };
                                break;
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }
}
