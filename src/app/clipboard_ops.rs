use anyhow::Result;

use crate::action::Action;
use crate::http_client;
use crate::state::{InputMode, Panel, ResponseTab};
use crate::vim_buffer::row_col_to_offset as vim_row_col_to_offset;
use vimltui::Register;

use super::inline_edit::{is_word_char, is_punct_char, row_col_to_offset};
use super::App;

impl App {
    pub(super) fn handle_clipboard_ops(&mut self, action: Action, count: usize) -> Result<()> {
        match action {
            Action::VisualYank => {
                let is_block = self.state.mode == InputMode::VisualBlock;
                let text = match self.state.active_panel {
                    Panel::Body if is_block => Some(self.get_block_selection()),
                    Panel::Body => Some(self.get_visual_selection()),
                    Panel::Response if self.state.response_view.tab == ResponseTab::Type && self.state.response_view.type_sub_focus == crate::state::TypeSubFocus::Editor => {
                        Some(self.state.response_view.type_vim.selected_text().unwrap_or_default())
                    }
                    Panel::Response if is_block => Some(self.get_response_block_selection()),
                    Panel::Response => Some(self.get_response_visual_selection()),
                    Panel::Request if self.state.request_edit.field_editing => Some(self.get_request_visual_selection()),
                    _ => None,
                };
                if let Some(text) = text {
                    self.state.yank_buffer = text.clone();
                    match crate::clipboard::copy_to_clipboard(&text) {
                        Ok(()) => self.state.set_status("Yanked"),
                        Err(e) => self.state.set_status(e),
                    }
                    self.state.mode = InputMode::Normal;
                }
            }
            Action::VisualDelete => {
                match self.state.active_panel {
                    Panel::Body => {
                        let is_block = self.state.mode == InputMode::VisualBlock;
                        let text = if is_block { self.get_block_selection() } else { self.get_visual_selection() };
                        self.state.yank_buffer = text;
                        self.push_body_undo();
                        if is_block {
                            self.delete_block_selection();
                        } else {
                            self.delete_visual_selection();
                        }
                        self.state.mode = InputMode::Normal;
                    }
                    Panel::Response if self.state.response_view.tab == ResponseTab::Type && self.state.response_view.type_sub_focus == crate::state::TypeSubFocus::Editor => {
                        let text = self.state.response_view.type_vim.selected_text().unwrap_or_default();
                        self.state.yank_buffer = text;
                        let _ = crate::clipboard::copy_to_clipboard(&self.state.yank_buffer);
                        self.state.response_view.type_vim.visual_delete();
                        self.sync_type_vim_text();
                        self.state.response_view.type_locked = true;
                        self.state.mode = InputMode::Normal;
                    }
                    Panel::Request if self.state.request_edit.field_editing => {
                        self.push_request_undo();
                        let text = self.get_request_visual_selection();
                        self.state.yank_buffer = text;
                        self.delete_request_visual_selection();
                        self.state.mode = InputMode::Normal;
                    }
                    _ => {}
                }
            }
            Action::VisualPaste => {
                let paste = crate::clipboard::read_clipboard().unwrap_or_else(|_| self.state.yank_buffer.clone());
                if paste.is_empty() {
                    self.state.mode = InputMode::Normal;
                } else {
                    match self.state.active_panel {
                        Panel::Body => {
                            let is_block = self.state.mode == InputMode::VisualBlock;
                            self.push_body_undo();
                            if is_block {
                                self.delete_block_selection();
                            } else {
                                self.delete_visual_selection();
                            }
                            self.state.mode = InputMode::Normal;
                            self.paste_text_at_cursor(&paste);
                        }
                        Panel::Response if self.state.response_view.tab == ResponseTab::Type && self.state.response_view.type_sub_focus == crate::state::TypeSubFocus::Editor => {
                            self.state.response_view.type_vim.visual_delete();
                            self.state.response_view.type_vim.unnamed_register = Register { content: paste.clone(), linewise: paste.ends_with('\n') };
                            self.state.response_view.type_vim.paste_after();
                            self.sync_type_vim_text();
                            self.state.response_view.type_locked = true;
                            self.state.mode = InputMode::Normal;
                        }
                        Panel::Request if self.state.request_edit.field_editing => {
                            self.push_request_undo();
                            self.delete_request_visual_selection();
                            self.paste_request_text(&paste);
                            self.state.mode = InputMode::Normal;
                        }
                        _ => {}
                    }
                }
            }
            Action::Paste => {
                let paste = crate::clipboard::read_clipboard().unwrap_or_else(|_| self.state.yank_buffer.clone());
                if self.state.active_panel == Panel::Response && self.state.response_view.tab == ResponseTab::Type && self.state.response_view.type_sub_focus == crate::state::TypeSubFocus::Editor {
                    self.state.response_view.type_vim.save_undo();
                    self.state.response_view.type_vim.unnamed_register = Register { content: paste.clone(), linewise: paste.ends_with('\n') };
                    self.state.response_view.type_vim.paste_after();
                    self.sync_type_vim_text();
                    self.state.response_view.type_locked = true;
                } else if self.state.active_panel == Panel::Body {
                    self.push_body_undo();
                    self.paste_text_at_cursor(&paste);
                } else if self.state.active_panel == Panel::Request && self.state.request_edit.field_editing {
                    self.push_request_undo();
                    self.paste_request_text(&paste);
                }
            }
            Action::PasteFromClipboard => {
                if let Ok(text) = crate::clipboard::read_clipboard() {
                    if self.state.active_panel == Panel::Body {
                        // Auto-format JSON if body type is JSON
                        let text = if self.state.body_type == crate::state::BodyType::Json {
                            if let Ok(val) = serde_json::from_str::<serde_json::Value>(&text) {
                                serde_json::to_string_pretty(&val).unwrap_or(text)
                            } else {
                                text
                            }
                        } else {
                            text
                        };
                        // If body is empty, replace entirely
                        if self.active_body().is_empty() {
                            self.set_active_body(Some(text.clone()));
                            self.state.body_vim.cursor_row = 0;
                            self.state.body_vim.cursor_col = 0;
                        } else {
                            self.paste_text_at_cursor(&text);
                        }
                        self.state.validate_body();
                        self.state.set_status("Pasted from clipboard");
                    } else if self.state.active_panel == Panel::Response && self.state.response_view.tab == ResponseTab::Type && self.state.response_view.type_sub_focus == crate::state::TypeSubFocus::Editor {
                        self.state.response_view.type_vim.save_undo();
                        self.state.response_view.type_vim.unnamed_register = Register { content: text.clone(), linewise: text.ends_with('\n') };
                        self.state.response_view.type_vim.paste_after();
                        self.sync_type_vim_text();
                        self.state.response_view.type_locked = true;
                        self.state.set_status("Pasted from clipboard");
                    }
                }
            }
            Action::YankLine => {
                self.state.pending_key = None;
                match self.state.active_panel {
                    Panel::Request if self.state.request_edit.field_editing => {
                        let text = self.get_request_field_text();
                        self.state.yank_buffer = text.clone();
                        let _ = crate::clipboard::copy_to_clipboard(&text);
                        self.state.set_status("Yanked field");
                    }
                    Panel::Body => {
                        let body = self.state.current_request.get_body(self.state.body_type);
                        let lines: Vec<&str> = body.lines().collect();
                        let row = self.state.body_vim.cursor_row;
                        let end_row = (row + count).min(lines.len());
                        if row < lines.len() {
                            let yanked: String = lines[row..end_row].join("\n");
                            self.state.yank_buffer = format!("{}\n", yanked);
                            let _ = crate::clipboard::copy_to_clipboard(&yanked);
                            if count > 1 {
                                self.state.set_status(format!("Yanked {} lines", end_row - row));
                            } else {
                                self.state.set_status("Yanked line");
                            }
                        }
                    }
                    Panel::Response if self.state.response_view.tab == ResponseTab::Type && self.state.response_view.type_sub_focus == crate::state::TypeSubFocus::Editor => {
                        let lines: Vec<&str> = self.state.response_view.type_text.lines().collect();
                        let row = self.state.response_view.type_vim.cursor_row;
                        let end_row = (row + count).min(lines.len());
                        if row < lines.len() {
                            let yanked: String = lines[row..end_row].join("\n");
                            self.state.yank_buffer = format!("{}\n", yanked);
                            let _ = crate::clipboard::copy_to_clipboard(&yanked);
                            self.state.set_status("Yanked line");
                        }
                    }
                    Panel::Response => {
                        let text = self.get_response_body_text();
                        let lines: Vec<&str> = text.lines().collect();
                        let row = self.state.response_view.resp_vim.cursor_row;
                        let end_row = (row + count).min(lines.len());
                        if row < lines.len() {
                            let yanked: String = lines[row..end_row].join("\n");
                            self.state.yank_buffer = format!("{}\n", yanked);
                            let _ = crate::clipboard::copy_to_clipboard(&yanked);
                            self.state.set_status("Yanked line");
                        }
                    }
                    _ => {}
                }
            }
            Action::YankWord => {
                self.state.pending_key = None;
                if self.state.active_panel == Panel::Response && self.state.response_view.tab == ResponseTab::Type && self.state.response_view.type_sub_focus == crate::state::TypeSubFocus::Editor {
                    let lines: Vec<&str> = self.state.response_view.type_text.lines().collect();
                    let row = self.state.response_view.type_vim.cursor_row;
                    let col = self.state.response_view.type_vim.cursor_col;
                    if let Some(line) = lines.get(row) {
                        let end_col = crate::vim_buffer::word_end_forward(line.as_bytes(), col);
                        self.state.yank_buffer = line[col..end_col].to_string();
                        let _ = crate::clipboard::copy_to_clipboard(&self.state.yank_buffer);
                        self.state.set_status("Yanked word");
                    }
                } else if self.state.active_panel == Panel::Body {
                    let body = self.state.current_request.get_body(self.state.body_type);
                    let lines: Vec<&str> = body.lines().collect();
                    let row = self.state.body_vim.cursor_row;
                    if let Some(line) = lines.get(row) {
                        let bytes = line.as_bytes();
                        let col = self.state.body_vim.cursor_col;
                        let mut end_col = col;
                        if end_col < bytes.len() {
                            if is_word_char(bytes[end_col]) {
                                while end_col < bytes.len() && is_word_char(bytes[end_col]) { end_col += 1; }
                            } else if is_punct_char(bytes[end_col]) {
                                while end_col < bytes.len() && is_punct_char(bytes[end_col]) { end_col += 1; }
                            }
                            while end_col < bytes.len() && bytes[end_col].is_ascii_whitespace() { end_col += 1; }
                        }
                        self.state.yank_buffer = line[col..end_col].to_string();
                        let _ = crate::clipboard::copy_to_clipboard(&self.state.yank_buffer);
                        self.state.set_status("Yanked word");
                    }
                } else if self.state.active_panel == Panel::Request && self.state.request_edit.field_editing {
                    let text = self.get_request_field_text();
                    let bytes = text.as_bytes();
                    let col = self.get_request_cursor();
                    let mut end_col = col;
                    if end_col < bytes.len() {
                        if is_word_char(bytes[end_col]) {
                            while end_col < bytes.len() && is_word_char(bytes[end_col]) { end_col += 1; }
                        } else if is_punct_char(bytes[end_col]) {
                            while end_col < bytes.len() && is_punct_char(bytes[end_col]) { end_col += 1; }
                        }
                        while end_col < bytes.len() && bytes[end_col].is_ascii_whitespace() { end_col += 1; }
                    }
                    self.state.yank_buffer = text[col..end_col].to_string();
                    let _ = crate::clipboard::copy_to_clipboard(&self.state.yank_buffer);
                    self.state.set_status("Yanked word");
                }
            }
            Action::YankToEnd => {
                self.state.pending_key = None;
                match self.state.active_panel {
                    Panel::Body => {
                        let body = self.state.current_request.get_body(self.state.body_type);
                        let lines: Vec<&str> = body.lines().collect();
                        let row = self.state.body_vim.cursor_row;
                        let col = self.state.body_vim.cursor_col;
                        if let Some(line) = lines.get(row) {
                            if col < line.len() {
                                let yanked = &line[col..];
                                self.state.yank_buffer = yanked.to_string();
                                let _ = crate::clipboard::copy_to_clipboard(&self.state.yank_buffer);
                                self.state.set_status("Yanked to end of line");
                            }
                        }
                    }
                    Panel::Request if self.state.request_edit.field_editing => {
                        let text = self.get_request_field_text();
                        let col = self.get_request_cursor();
                        if col < text.len() {
                            self.state.yank_buffer = text[col..].to_string();
                            let _ = crate::clipboard::copy_to_clipboard(&self.state.yank_buffer);
                            self.state.set_status("Yanked to end");
                        }
                    }
                    _ => {}
                }
            }
            Action::YankToStart => {
                self.state.pending_key = None;
                match self.state.active_panel {
                    Panel::Body => {
                        let body = self.state.current_request.get_body(self.state.body_type);
                        let lines: Vec<&str> = body.lines().collect();
                        let row = self.state.body_vim.cursor_row;
                        let col = self.state.body_vim.cursor_col;
                        if let Some(line) = lines.get(row) {
                            if col > 0 {
                                let yanked = &line[..col];
                                self.state.yank_buffer = yanked.to_string();
                                let _ = crate::clipboard::copy_to_clipboard(&self.state.yank_buffer);
                                self.state.set_status("Yanked to start of line");
                            }
                        }
                    }
                    Panel::Request if self.state.request_edit.field_editing => {
                        let text = self.get_request_field_text();
                        let col = self.get_request_cursor();
                        if col > 0 {
                            self.state.yank_buffer = text[..col].to_string();
                            let _ = crate::clipboard::copy_to_clipboard(&self.state.yank_buffer);
                            self.state.set_status("Yanked to start");
                        }
                    }
                    _ => {}
                }
            }
            Action::YankToBottom => {
                self.state.pending_key = None;
                match self.state.active_panel {
                    Panel::Body => {
                        let body = self.state.current_request.get_body(self.state.body_type);
                        let lines: Vec<&str> = body.lines().collect();
                        let row = self.state.body_vim.cursor_row;
                        if row < lines.len() {
                            let yanked: String = lines[row..].join("\n");
                            self.state.yank_buffer = format!("{}\n", yanked);
                            let _ = crate::clipboard::copy_to_clipboard(&yanked);
                            self.state.set_status("Yanked to end of file");
                        }
                    }
                    Panel::Request if self.state.request_edit.field_editing => {
                        // Single-line field: same as yank to end
                        let text = self.get_request_field_text();
                        let col = self.get_request_cursor();
                        if col < text.len() {
                            self.state.yank_buffer = text[col..].to_string();
                            let _ = crate::clipboard::copy_to_clipboard(&self.state.yank_buffer);
                            self.state.set_status("Yanked to end");
                        }
                    }
                    _ => {}
                }
            }
            Action::DeleteLine => {
                self.state.pending_key = None;
                if self.state.active_panel == Panel::Body {
                    let body = self.state.current_request.get_body(self.state.body_type);
                    let lines: Vec<&str> = body.lines().collect();
                    let row = self.state.body_vim.cursor_row;
                    let end_row = (row + count).min(lines.len());
                    if row < lines.len() {
                        let yanked: String = lines[row..end_row].join("\n");
                        self.state.yank_buffer = format!("{}\n", yanked);
                        let _ = crate::clipboard::copy_to_clipboard(&self.state.yank_buffer);
                        self.push_body_undo();
                        for _ in 0..(end_row - row) {
                            self.delete_body_line(self.state.body_vim.cursor_row);
                        }
                        if count > 1 {
                            self.state.set_status(format!("Deleted {} lines", end_row - row));
                        } else {
                            self.state.set_status("Line deleted");
                        }
                    }
                } else if self.state.active_panel == Panel::Response && self.state.response_view.tab == ResponseTab::Type && self.state.response_view.type_sub_focus == crate::state::TypeSubFocus::Editor {
                    self.state.response_view.type_vim.save_undo();
                    let row = self.state.response_view.type_vim.cursor_row;
                    let yanked = self.state.response_view.type_vim.delete_line(row).unwrap_or_default();
                    self.sync_type_vim_text();
                    self.state.yank_buffer = format!("{}\n", yanked);
                    let _ = crate::clipboard::copy_to_clipboard(&self.state.yank_buffer);
                    self.state.response_view.type_locked = true;
                    self.state.response_view.type_vim.ensure_cursor_visible();
                    self.state.set_status("Line deleted");
                } else if self.state.active_panel == Panel::Request && self.state.request_edit.field_editing {
                    // dd in request field edit: clear the field
                    self.push_request_undo();
                    let text = self.get_request_field_text();
                    self.state.yank_buffer = text;
                    let _ = crate::clipboard::copy_to_clipboard(&self.state.yank_buffer);
                    self.clear_request_field();
                    self.set_request_cursor(0);
                    self.state.set_status("Field cleared");
                }
            }
            Action::DeleteCharUnderCursor => {
                if self.state.active_panel == Panel::Response && self.state.response_view.tab == ResponseTab::Type && self.state.response_view.type_sub_focus == crate::state::TypeSubFocus::Editor {
                    self.state.response_view.type_vim.save_undo();
                    self.state.response_view.type_vim.delete_char_at_cursor();
                    self.sync_type_vim_text();
                    self.state.response_view.type_locked = true;
                } else if self.state.active_panel == Panel::Body {
                    self.push_body_undo();
                    let body = self.state.current_request.get_body_mut(self.state.body_type);
                    let pos = row_col_to_offset(body, self.state.body_vim.cursor_row, self.state.body_vim.cursor_col);
                    if pos < body.len() {
                        let ch = body.as_bytes()[pos];
                        if ch != b'\n' {
                            body.remove(pos);
                            // Clamp cursor if at end of line now
                            let lines: Vec<&str> = body.lines().collect();
                            let line_len = lines.get(self.state.body_vim.cursor_row).map(|l| l.len()).unwrap_or(0);
                            self.state.body_vim.cursor_col = self.state.body_vim.cursor_col.min(line_len.saturating_sub(1).max(0));
                        }
                    }
                } else if self.state.active_panel == Panel::Request && self.state.request_edit.field_editing {
                    self.push_request_undo();
                    self.delete_request_char_under_cursor();
                }
            }
            Action::DeleteWord => {
                self.state.pending_key = None;
                if self.state.active_panel == Panel::Response && self.state.response_view.tab == ResponseTab::Type && self.state.response_view.type_sub_focus == crate::state::TypeSubFocus::Editor {
                    self.state.response_view.type_vim.save_undo();
                    let lines: Vec<&str> = self.state.response_view.type_text.lines().collect();
                    let row = self.state.response_view.type_vim.cursor_row;
                    let col = self.state.response_view.type_vim.cursor_col;
                    if let Some(line) = lines.get(row) {
                        let end_col = crate::vim_buffer::word_end_forward(line.as_bytes(), col);
                        self.state.yank_buffer = line[col..end_col].to_string();
                        let _ = crate::clipboard::copy_to_clipboard(&self.state.yank_buffer);
                        let start = vim_row_col_to_offset(&self.state.response_view.type_text, row, col);
                        let end = vim_row_col_to_offset(&self.state.response_view.type_text, row, end_col);
                        self.state.response_view.type_text.drain(start..end);
                    }
                    self.state.response_view.type_locked = true;
                } else if self.state.active_panel == Panel::Body {
                    self.push_body_undo();
                    let body_text = self.active_body().to_string();
                    let lines: Vec<&str> = body_text.lines().collect();
                    let row = self.state.body_vim.cursor_row;
                    if let Some(line) = lines.get(row) {
                        let bytes = line.as_bytes();
                        let col = self.state.body_vim.cursor_col;
                        let mut end_col = col;
                        if end_col < bytes.len() {
                            if is_word_char(bytes[end_col]) {
                                while end_col < bytes.len() && is_word_char(bytes[end_col]) { end_col += 1; }
                            } else if is_punct_char(bytes[end_col]) {
                                while end_col < bytes.len() && is_punct_char(bytes[end_col]) { end_col += 1; }
                            }
                            while end_col < bytes.len() && bytes[end_col].is_ascii_whitespace() { end_col += 1; }
                        }
                        self.state.yank_buffer = line[col..end_col].to_string();
                        let _ = crate::clipboard::copy_to_clipboard(&self.state.yank_buffer);
                        let body = self.state.current_request.get_body_mut(self.state.body_type);
                        let start = row_col_to_offset(body, row, col);
                        let end = row_col_to_offset(body, row, end_col);
                        body.drain(start..end);
                        // Clamp cursor
                        let lines2: Vec<&str> = body.lines().collect();
                        let line_len = lines2.get(row).map(|l| l.len()).unwrap_or(0);
                        self.state.body_vim.cursor_col = col.min(line_len.saturating_sub(1).max(0));
                    }
                } else if self.state.active_panel == Panel::Request && self.state.request_edit.field_editing {
                    self.push_request_undo();
                    let text = self.get_request_field_text();
                    let bytes = text.as_bytes();
                    let col = self.get_request_cursor();
                    let mut end_col = col;
                    if end_col < bytes.len() {
                        if is_word_char(bytes[end_col]) {
                            while end_col < bytes.len() && is_word_char(bytes[end_col]) { end_col += 1; }
                        } else if is_punct_char(bytes[end_col]) {
                            while end_col < bytes.len() && is_punct_char(bytes[end_col]) { end_col += 1; }
                        }
                        while end_col < bytes.len() && bytes[end_col].is_ascii_whitespace() { end_col += 1; }
                    }
                    self.state.yank_buffer = text[col..end_col].to_string();
                    let _ = crate::clipboard::copy_to_clipboard(&self.state.yank_buffer);
                    self.drain_request_field(col, end_col);
                    let new_len = self.get_request_field_len();
                    self.set_request_cursor(col.min(new_len.saturating_sub(1).max(0)));
                }
            }
            Action::DeleteWordEnd => {
                self.state.pending_key = None;
                if self.state.active_panel == Panel::Response && self.state.response_view.tab == ResponseTab::Type && self.state.response_view.type_sub_focus == crate::state::TypeSubFocus::Editor {
                    self.state.response_view.type_vim.save_undo();
                    let lines: Vec<&str> = self.state.response_view.type_text.lines().collect();
                    let row = self.state.response_view.type_vim.cursor_row;
                    let col = self.state.response_view.type_vim.cursor_col;
                    if let Some(line) = lines.get(row) {
                        let end_col = crate::vim_buffer::word_end_forward(line.as_bytes(), col);
                        self.state.yank_buffer = line[col..end_col].to_string();
                        let _ = crate::clipboard::copy_to_clipboard(&self.state.yank_buffer);
                        let start = vim_row_col_to_offset(&self.state.response_view.type_text, row, col);
                        let end = vim_row_col_to_offset(&self.state.response_view.type_text, row, end_col);
                        self.state.response_view.type_text.drain(start..end);
                    }
                    self.state.response_view.type_locked = true;
                } else if self.state.active_panel == Panel::Body {
                    self.push_body_undo();
                    let body_text = self.active_body().to_string();
                    let lines: Vec<&str> = body_text.lines().collect();
                    let row = self.state.body_vim.cursor_row;
                    if let Some(line) = lines.get(row) {
                        let bytes = line.as_bytes();
                        let col = self.state.body_vim.cursor_col;
                        let mut end_col = col;
                        if end_col < bytes.len() {
                            // de: delete to end of word (inclusive of last word char)
                            if is_word_char(bytes[end_col]) {
                                while end_col < bytes.len() && is_word_char(bytes[end_col]) { end_col += 1; }
                            } else if is_punct_char(bytes[end_col]) {
                                while end_col < bytes.len() && is_punct_char(bytes[end_col]) { end_col += 1; }
                            } else {
                                // on whitespace, skip whitespace then word
                                while end_col < bytes.len() && bytes[end_col].is_ascii_whitespace() { end_col += 1; }
                                if end_col < bytes.len() && is_word_char(bytes[end_col]) {
                                    while end_col < bytes.len() && is_word_char(bytes[end_col]) { end_col += 1; }
                                } else if end_col < bytes.len() && is_punct_char(bytes[end_col]) {
                                    while end_col < bytes.len() && is_punct_char(bytes[end_col]) { end_col += 1; }
                                }
                            }
                        }
                        self.state.yank_buffer = line[col..end_col].to_string();
                        let _ = crate::clipboard::copy_to_clipboard(&self.state.yank_buffer);
                        let body = self.state.current_request.get_body_mut(self.state.body_type);
                        let start = row_col_to_offset(body, row, col);
                        let end = row_col_to_offset(body, row, end_col);
                        body.drain(start..end);
                        let lines2: Vec<&str> = body.lines().collect();
                        let line_len = lines2.get(row).map(|l| l.len()).unwrap_or(0);
                        self.state.body_vim.cursor_col = col.min(line_len.saturating_sub(1).max(0));
                    }
                } else if self.state.active_panel == Panel::Request && self.state.request_edit.field_editing {
                    self.push_request_undo();
                    let text = self.get_request_field_text();
                    let bytes = text.as_bytes();
                    let col = self.get_request_cursor();
                    let mut end_col = col;
                    if end_col < bytes.len() {
                        if is_word_char(bytes[end_col]) {
                            while end_col < bytes.len() && is_word_char(bytes[end_col]) { end_col += 1; }
                        } else if is_punct_char(bytes[end_col]) {
                            while end_col < bytes.len() && is_punct_char(bytes[end_col]) { end_col += 1; }
                        } else {
                            while end_col < bytes.len() && bytes[end_col].is_ascii_whitespace() { end_col += 1; }
                            if end_col < bytes.len() && is_word_char(bytes[end_col]) {
                                while end_col < bytes.len() && is_word_char(bytes[end_col]) { end_col += 1; }
                            } else if end_col < bytes.len() && is_punct_char(bytes[end_col]) {
                                while end_col < bytes.len() && is_punct_char(bytes[end_col]) { end_col += 1; }
                            }
                        }
                    }
                    self.state.yank_buffer = text[col..end_col].to_string();
                    let _ = crate::clipboard::copy_to_clipboard(&self.state.yank_buffer);
                    self.drain_request_field(col, end_col);
                    let new_len = self.get_request_field_len();
                    self.set_request_cursor(col.min(new_len.saturating_sub(1).max(0)));
                }
            }
            Action::DeleteWordBack => {
                self.state.pending_key = None;
                if self.state.active_panel == Panel::Response && self.state.response_view.tab == ResponseTab::Type && self.state.response_view.type_sub_focus == crate::state::TypeSubFocus::Editor {
                    self.state.response_view.type_vim.save_undo();
                    let lines: Vec<&str> = self.state.response_view.type_text.lines().collect();
                    let row = self.state.response_view.type_vim.cursor_row;
                    let col = self.state.response_view.type_vim.cursor_col;
                    if let Some(line) = lines.get(row) {
                        let start_col = crate::vim_buffer::word_start_backward(line.as_bytes(), col);
                        self.state.yank_buffer = line[start_col..col].to_string();
                        let _ = crate::clipboard::copy_to_clipboard(&self.state.yank_buffer);
                        let start = vim_row_col_to_offset(&self.state.response_view.type_text, row, start_col);
                        let end = vim_row_col_to_offset(&self.state.response_view.type_text, row, col);
                        self.state.response_view.type_text.drain(start..end);
                        self.state.response_view.type_vim.cursor_col = start_col;
                    }
                    self.state.response_view.type_locked = true;
                } else if self.state.active_panel == Panel::Body {
                    self.push_body_undo();
                    let body_text = self.active_body().to_string();
                    let lines: Vec<&str> = body_text.lines().collect();
                    let row = self.state.body_vim.cursor_row;
                    if let Some(line) = lines.get(row) {
                        let bytes = line.as_bytes();
                        let col = self.state.body_vim.cursor_col;
                        let mut start_col = col;
                        if start_col > 0 {
                            start_col -= 1;
                            while start_col > 0 && bytes[start_col].is_ascii_whitespace() { start_col -= 1; }
                            if is_word_char(bytes[start_col]) {
                                while start_col > 0 && is_word_char(bytes[start_col - 1]) { start_col -= 1; }
                            } else if is_punct_char(bytes[start_col]) {
                                while start_col > 0 && is_punct_char(bytes[start_col - 1]) { start_col -= 1; }
                            }
                        }
                        self.state.yank_buffer = line[start_col..col].to_string();
                        let _ = crate::clipboard::copy_to_clipboard(&self.state.yank_buffer);
                        let body = self.state.current_request.get_body_mut(self.state.body_type);
                        let start = row_col_to_offset(body, row, start_col);
                        let end = row_col_to_offset(body, row, col);
                        body.drain(start..end);
                        self.state.body_vim.cursor_col = start_col;
                    }
                } else if self.state.active_panel == Panel::Request && self.state.request_edit.field_editing {
                    self.push_request_undo();
                    let text = self.get_request_field_text();
                    let bytes = text.as_bytes();
                    let col = self.get_request_cursor();
                    let mut start_col = col;
                    if start_col > 0 {
                        start_col -= 1;
                        while start_col > 0 && bytes[start_col].is_ascii_whitespace() { start_col -= 1; }
                        if is_word_char(bytes[start_col]) {
                            while start_col > 0 && is_word_char(bytes[start_col - 1]) { start_col -= 1; }
                        } else if is_punct_char(bytes[start_col]) {
                            while start_col > 0 && is_punct_char(bytes[start_col - 1]) { start_col -= 1; }
                        }
                    }
                    self.state.yank_buffer = text[start_col..col].to_string();
                    let _ = crate::clipboard::copy_to_clipboard(&self.state.yank_buffer);
                    self.drain_request_field(start_col, col);
                    self.set_request_cursor(start_col);
                }
            }
            Action::DeleteToEnd => {
                self.state.pending_key = None;
                match self.state.active_panel {
                    Panel::Response if self.state.response_view.tab == ResponseTab::Type && self.state.response_view.type_sub_focus == crate::state::TypeSubFocus::Editor => {
                        self.state.response_view.type_vim.save_undo();
                        let yanked = {
                        let row = self.state.response_view.type_vim.cursor_row;
                        let col = self.state.response_view.type_vim.cursor_col;
                        let line_len = self.state.response_view.type_vim.current_line_len();
                        let text = self.state.response_view.type_vim.delete_range(col, line_len, row);
                        self.sync_type_vim_text();
                        text
                    };
                        self.state.yank_buffer = yanked;
                        let _ = crate::clipboard::copy_to_clipboard(&self.state.yank_buffer);
                        self.state.response_view.type_locked = true;
                    }
                    Panel::Body => {
                        self.push_body_undo();
                        let body = self.state.current_request.get_body_mut(self.state.body_type);
                        let lines: Vec<&str> = body.lines().collect();
                        let row = self.state.body_vim.cursor_row;
                        let col = self.state.body_vim.cursor_col;
                        if let Some(line) = lines.get(row) {
                            if col < line.len() {
                                let deleted = &line[col..];
                                self.state.yank_buffer = deleted.to_string();
                                let _ = crate::clipboard::copy_to_clipboard(&self.state.yank_buffer);
                                let start = row_col_to_offset(body, row, col);
                                let end = row_col_to_offset(body, row, line.len());
                                body.drain(start..end);
                                // Clamp cursor
                                let lines2: Vec<&str> = body.lines().collect();
                                let line_len = lines2.get(row).map(|l| l.len()).unwrap_or(0);
                                self.state.body_vim.cursor_col = if line_len > 0 { col.min(line_len - 1) } else { 0 };
                            }
                        }
                    }
                    Panel::Request if self.state.request_edit.field_editing => {
                        self.push_request_undo();
                        let text = self.get_request_field_text();
                        let col = self.get_request_cursor();
                        if col < text.len() {
                            self.state.yank_buffer = text[col..].to_string();
                            let _ = crate::clipboard::copy_to_clipboard(&self.state.yank_buffer);
                            self.drain_request_field(col, text.len());
                            let len = self.get_request_field_len();
                            if len > 0 {
                                self.set_request_cursor(col.min(len - 1));
                            } else {
                                self.set_request_cursor(0);
                            }
                        }
                    }
                    _ => {}
                }
            }
            Action::DeleteToStart => {
                self.state.pending_key = None;
                match self.state.active_panel {
                    Panel::Body => {
                        self.push_body_undo();
                        let body_text = self.active_body().to_string();
                        let lines: Vec<&str> = body_text.lines().collect();
                        let row = self.state.body_vim.cursor_row;
                        let col = self.state.body_vim.cursor_col;
                        if col > 0 {
                            if let Some(line) = lines.get(row) {
                                let deleted = &line[..col];
                                self.state.yank_buffer = deleted.to_string();
                                let _ = crate::clipboard::copy_to_clipboard(&self.state.yank_buffer);
                                let body = self.state.current_request.get_body_mut(self.state.body_type);
                                let start = row_col_to_offset(body, row, 0);
                                let end = row_col_to_offset(body, row, col);
                                body.drain(start..end);
                                self.state.body_vim.cursor_col = 0;
                            }
                        }
                    }
                    Panel::Request if self.state.request_edit.field_editing => {
                        self.push_request_undo();
                        let text = self.get_request_field_text();
                        let col = self.get_request_cursor();
                        if col > 0 {
                            self.state.yank_buffer = text[..col].to_string();
                            let _ = crate::clipboard::copy_to_clipboard(&self.state.yank_buffer);
                            self.drain_request_field(0, col);
                            self.set_request_cursor(0);
                        }
                    }
                    _ => {}
                }
            }
            Action::DeleteToBottom => {
                self.state.pending_key = None;
                match self.state.active_panel {
                    Panel::Body => {
                        self.push_body_undo();
                        let body_text = self.active_body().to_string();
                        let lines: Vec<&str> = body_text.lines().collect();
                        let row = self.state.body_vim.cursor_row;
                        if row < lines.len() {
                            let yanked: String = lines[row..].join("\n");
                            self.state.yank_buffer = format!("{}\n", yanked);
                            let _ = crate::clipboard::copy_to_clipboard(&yanked);
                            let body = self.state.current_request.get_body_mut(self.state.body_type);
                            // Delete from start of current row to end of body
                            let start = row_col_to_offset(body, row, 0);
                            // Also remove the preceding newline if not at row 0
                            let drain_start = if row > 0 && start > 0 { start - 1 } else { start };
                            body.drain(drain_start..body.len());
                            // Clamp cursor
                            let max_row = body.lines().count().saturating_sub(1);
                            self.state.body_vim.cursor_row = self.state.body_vim.cursor_row.min(max_row);
                            let cur_line_len = body.lines().nth(self.state.body_vim.cursor_row).map(|l| l.len()).unwrap_or(0);
                            self.state.body_vim.cursor_col = self.state.body_vim.cursor_col.min(cur_line_len.saturating_sub(1));
                            self.state.set_status("Deleted to end of file");
                        }
                    }
                    Panel::Request if self.state.request_edit.field_editing => {
                        // Single-line: same as delete to end
                        self.push_request_undo();
                        let text = self.get_request_field_text();
                        let col = self.get_request_cursor();
                        if col < text.len() {
                            self.state.yank_buffer = text[col..].to_string();
                            let _ = crate::clipboard::copy_to_clipboard(&self.state.yank_buffer);
                            self.drain_request_field(col, text.len());
                            let len = self.get_request_field_len();
                            if len > 0 {
                                self.set_request_cursor(col.min(len - 1));
                            } else {
                                self.set_request_cursor(0);
                            }
                        }
                    }
                    _ => {}
                }
            }
            Action::ChangeLine => {
                self.state.pending_key = None;
                if self.state.active_panel == Panel::Response && self.state.response_view.tab == ResponseTab::Type && self.state.response_view.type_sub_focus == crate::state::TypeSubFocus::Editor {
                    self.state.response_view.type_vim.save_undo();
                    let row = self.state.response_view.type_vim.cursor_row;
                    let yanked = if row < self.state.response_view.type_vim.lines.len() {
                        let line = std::mem::take(&mut self.state.response_view.type_vim.lines[row]);
                        self.state.response_view.type_vim.cursor_col = 0;
                        self.state.response_view.type_vim.modified = true;
                        line
                    } else { String::new() };
                    self.sync_type_vim_text();
                    self.state.yank_buffer = yanked;
                    let _ = crate::clipboard::copy_to_clipboard(&self.state.yank_buffer);
                    self.state.response_view.type_locked = true;
                    self.state.mode = InputMode::Insert;
                } else if self.state.active_panel == Panel::Body {
                    self.push_body_undo();
                    let body = self.state.current_request.get_body_mut(self.state.body_type);
                    let lines: Vec<&str> = body.lines().collect();
                    let row = self.state.body_vim.cursor_row;
                    if row < lines.len() {
                        let line_text = lines[row].to_string();
                        self.state.yank_buffer = line_text.clone();
                        let _ = crate::clipboard::copy_to_clipboard(&line_text);
                        // Replace line content with empty
                        let offset = row_col_to_offset(body, row, 0);
                        let end = offset + lines[row].len();
                        body.drain(offset..end);
                    }
                    self.state.body_vim.cursor_col = 0;
                    self.state.mode = InputMode::Insert;
                } else if self.state.active_panel == Panel::Request && self.state.request_edit.field_editing {
                    self.push_request_undo();
                    let text = self.get_request_field_text();
                    self.state.yank_buffer = text.clone();
                    let _ = crate::clipboard::copy_to_clipboard(&text);
                    self.clear_request_field();
                    self.set_request_cursor(0);
                    self.state.mode = InputMode::Insert;
                }
            }
            Action::ChangeWord => {
                self.state.pending_key = None;
                if self.state.active_panel == Panel::Response && self.state.response_view.tab == ResponseTab::Type && self.state.response_view.type_sub_focus == crate::state::TypeSubFocus::Editor {
                    self.state.response_view.type_vim.save_undo();
                    let lines: Vec<&str> = self.state.response_view.type_text.lines().collect();
                    let row = self.state.response_view.type_vim.cursor_row;
                    let col = self.state.response_view.type_vim.cursor_col;
                    if let Some(line) = lines.get(row) {
                        let end_col = crate::vim_buffer::word_end_forward(line.as_bytes(), col);
                        let deleted = &line[col..end_col];
                        self.state.yank_buffer = deleted.to_string();
                        let _ = crate::clipboard::copy_to_clipboard(&self.state.yank_buffer);
                        let start = vim_row_col_to_offset(&self.state.response_view.type_text, row, col);
                        let end = vim_row_col_to_offset(&self.state.response_view.type_text, row, end_col);
                        self.state.response_view.type_text.drain(start..end);
                    }
                    self.state.response_view.type_locked = true;
                    self.state.mode = InputMode::Insert;
                } else if self.state.active_panel == Panel::Body {
                    self.push_body_undo();
                    let body_text = self.active_body().to_string();
                    let lines: Vec<&str> = body_text.lines().collect();
                    let row = self.state.body_vim.cursor_row;
                    if let Some(line) = lines.get(row) {
                        let bytes = line.as_bytes();
                        let col = self.state.body_vim.cursor_col;
                        let mut end_col = col;
                        if end_col < bytes.len() {
                            if is_word_char(bytes[end_col]) {
                                while end_col < bytes.len() && is_word_char(bytes[end_col]) { end_col += 1; }
                            } else if is_punct_char(bytes[end_col]) {
                                while end_col < bytes.len() && is_punct_char(bytes[end_col]) { end_col += 1; }
                            }
                            // Skip trailing whitespace
                            while end_col < bytes.len() && bytes[end_col].is_ascii_whitespace() { end_col += 1; }
                        }
                        let deleted = &line[col..end_col];
                        self.state.yank_buffer = deleted.to_string();
                        let _ = crate::clipboard::copy_to_clipboard(&self.state.yank_buffer);
                        let body = self.state.current_request.get_body_mut(self.state.body_type);
                        let start = row_col_to_offset(body, row, col);
                        let end = row_col_to_offset(body, row, end_col);
                        body.drain(start..end);
                    }
                    self.state.mode = InputMode::Insert;
                } else if self.state.active_panel == Panel::Request && self.state.request_edit.field_editing {
                    self.push_request_undo();
                    let text = self.get_request_field_text();
                    let bytes = text.as_bytes();
                    let col = self.get_request_cursor();
                    let mut end_col = col;
                    if end_col < bytes.len() {
                        if is_word_char(bytes[end_col]) {
                            while end_col < bytes.len() && is_word_char(bytes[end_col]) { end_col += 1; }
                        } else if is_punct_char(bytes[end_col]) {
                            while end_col < bytes.len() && is_punct_char(bytes[end_col]) { end_col += 1; }
                        }
                        while end_col < bytes.len() && bytes[end_col].is_ascii_whitespace() { end_col += 1; }
                    }
                    self.state.yank_buffer = text[col..end_col].to_string();
                    let _ = crate::clipboard::copy_to_clipboard(&self.state.yank_buffer);
                    self.drain_request_field(col, end_col);
                    self.set_request_cursor(col);
                    self.state.mode = InputMode::Insert;
                }
            }
            Action::ChangeWordBack => {
                self.state.pending_key = None;
                if self.state.active_panel == Panel::Response && self.state.response_view.tab == ResponseTab::Type && self.state.response_view.type_sub_focus == crate::state::TypeSubFocus::Editor {
                    self.state.response_view.type_vim.save_undo();
                    let lines: Vec<&str> = self.state.response_view.type_text.lines().collect();
                    let row = self.state.response_view.type_vim.cursor_row;
                    let col = self.state.response_view.type_vim.cursor_col;
                    if let Some(line) = lines.get(row) {
                        let start_col = crate::vim_buffer::word_start_backward(line.as_bytes(), col);
                        let deleted = &line[start_col..col];
                        self.state.yank_buffer = deleted.to_string();
                        let _ = crate::clipboard::copy_to_clipboard(&self.state.yank_buffer);
                        let start = vim_row_col_to_offset(&self.state.response_view.type_text, row, start_col);
                        let end = vim_row_col_to_offset(&self.state.response_view.type_text, row, col);
                        self.state.response_view.type_text.drain(start..end);
                        self.state.response_view.type_vim.cursor_col = start_col;
                    }
                    self.state.response_view.type_locked = true;
                    self.state.mode = InputMode::Insert;
                } else if self.state.active_panel == Panel::Body {
                    self.push_body_undo();
                    let body_text = self.active_body().to_string();
                    let lines: Vec<&str> = body_text.lines().collect();
                    let row = self.state.body_vim.cursor_row;
                    if let Some(line) = lines.get(row) {
                        let bytes = line.as_bytes();
                        let col = self.state.body_vim.cursor_col;
                        let mut start_col = col;
                        if start_col > 0 {
                            start_col -= 1;
                            while start_col > 0 && bytes[start_col].is_ascii_whitespace() { start_col -= 1; }
                            if is_word_char(bytes[start_col]) {
                                while start_col > 0 && is_word_char(bytes[start_col - 1]) { start_col -= 1; }
                            } else if is_punct_char(bytes[start_col]) {
                                while start_col > 0 && is_punct_char(bytes[start_col - 1]) { start_col -= 1; }
                            }
                        }
                        let deleted = &line[start_col..col];
                        self.state.yank_buffer = deleted.to_string();
                        let _ = crate::clipboard::copy_to_clipboard(&self.state.yank_buffer);
                        let body = self.state.current_request.get_body_mut(self.state.body_type);
                        let start = row_col_to_offset(body, row, start_col);
                        let end = row_col_to_offset(body, row, col);
                        body.drain(start..end);
                        self.state.body_vim.cursor_col = start_col;
                    }
                    self.state.mode = InputMode::Insert;
                } else if self.state.active_panel == Panel::Request && self.state.request_edit.field_editing {
                    self.push_request_undo();
                    let text = self.get_request_field_text();
                    let bytes = text.as_bytes();
                    let col = self.get_request_cursor();
                    let mut start_col = col;
                    if start_col > 0 {
                        start_col -= 1;
                        while start_col > 0 && bytes[start_col].is_ascii_whitespace() { start_col -= 1; }
                        if is_word_char(bytes[start_col]) {
                            while start_col > 0 && is_word_char(bytes[start_col - 1]) { start_col -= 1; }
                        } else if is_punct_char(bytes[start_col]) {
                            while start_col > 0 && is_punct_char(bytes[start_col - 1]) { start_col -= 1; }
                        }
                    }
                    self.state.yank_buffer = text[start_col..col].to_string();
                    let _ = crate::clipboard::copy_to_clipboard(&self.state.yank_buffer);
                    self.drain_request_field(start_col, col);
                    self.set_request_cursor(start_col);
                    self.state.mode = InputMode::Insert;
                }
            }
            Action::ChangeToEnd => {
                if self.state.active_panel == Panel::Response && self.state.response_view.tab == ResponseTab::Type && self.state.response_view.type_sub_focus == crate::state::TypeSubFocus::Editor {
                    self.state.response_view.type_vim.save_undo();
                    let yanked = {
                        let row = self.state.response_view.type_vim.cursor_row;
                        let col = self.state.response_view.type_vim.cursor_col;
                        let line_len = self.state.response_view.type_vim.current_line_len();
                        let text = self.state.response_view.type_vim.delete_range(col, line_len, row);
                        self.sync_type_vim_text();
                        text
                    };
                    self.state.yank_buffer = yanked;
                    let _ = crate::clipboard::copy_to_clipboard(&self.state.yank_buffer);
                    self.state.response_view.type_locked = true;
                    self.state.mode = InputMode::Insert;
                } else if self.state.active_panel == Panel::Body {
                    self.push_body_undo();
                    let body = self.state.current_request.get_body_mut(self.state.body_type);
                    let lines: Vec<&str> = body.lines().collect();
                    let row = self.state.body_vim.cursor_row;
                    let col = self.state.body_vim.cursor_col;
                    if let Some(line) = lines.get(row) {
                        if col < line.len() {
                            let deleted = &line[col..];
                            self.state.yank_buffer = deleted.to_string();
                            let _ = crate::clipboard::copy_to_clipboard(&self.state.yank_buffer);
                            let start = row_col_to_offset(body, row, col);
                            let end = row_col_to_offset(body, row, line.len());
                            body.drain(start..end);
                        }
                    }
                    self.state.mode = InputMode::Insert;
                } else if self.state.active_panel == Panel::Request && self.state.request_edit.field_editing {
                    self.push_request_undo();
                    let text = self.get_request_field_text();
                    let col = self.get_request_cursor();
                    if col < text.len() {
                        self.state.yank_buffer = text[col..].to_string();
                        let _ = crate::clipboard::copy_to_clipboard(&self.state.yank_buffer);
                        self.drain_request_field(col, text.len());
                        self.set_request_cursor(col);
                    }
                    self.state.mode = InputMode::Insert;
                }
            }
            Action::ChangeToStart => {
                self.state.pending_key = None;
                match self.state.active_panel {
                    Panel::Body => {
                        self.push_body_undo();
                        let body_text = self.active_body().to_string();
                        let lines: Vec<&str> = body_text.lines().collect();
                        let row = self.state.body_vim.cursor_row;
                        let col = self.state.body_vim.cursor_col;
                        if col > 0 {
                            if let Some(line) = lines.get(row) {
                                let deleted = &line[..col];
                                self.state.yank_buffer = deleted.to_string();
                                let _ = crate::clipboard::copy_to_clipboard(&self.state.yank_buffer);
                                let body = self.state.current_request.get_body_mut(self.state.body_type);
                                let start = row_col_to_offset(body, row, 0);
                                let end = row_col_to_offset(body, row, col);
                                body.drain(start..end);
                                self.state.body_vim.cursor_col = 0;
                            }
                        }
                        self.state.mode = InputMode::Insert;
                    }
                    Panel::Request if self.state.request_edit.field_editing => {
                        self.push_request_undo();
                        let text = self.get_request_field_text();
                        let col = self.get_request_cursor();
                        if col > 0 {
                            self.state.yank_buffer = text[..col].to_string();
                            let _ = crate::clipboard::copy_to_clipboard(&self.state.yank_buffer);
                            self.drain_request_field(0, col);
                        }
                        self.set_request_cursor(0);
                        self.state.mode = InputMode::Insert;
                    }
                    _ => {}
                }
            }
            Action::ReplaceChar(c) => {
                self.state.pending_key = None;
                if self.state.active_panel == Panel::Response && self.state.response_view.tab == ResponseTab::Type && self.state.response_view.type_sub_focus == crate::state::TypeSubFocus::Editor {
                    self.state.response_view.type_vim.save_undo();
                    {
                        let row = self.state.response_view.type_vim.cursor_row;
                        let col = self.state.response_view.type_vim.cursor_col;
                        if row < self.state.response_view.type_vim.lines.len() && col < self.state.response_view.type_vim.lines[row].len() {
                            self.state.response_view.type_vim.lines[row].remove(col);
                            self.state.response_view.type_vim.lines[row].insert(col, c);
                            self.state.response_view.type_vim.modified = true;
                            self.sync_type_vim_text();
                        }
                    }
                    self.state.response_view.type_locked = true;
                } else if self.state.active_panel == Panel::Body {
                    self.push_body_undo();
                    let body = self.state.current_request.get_body_mut(self.state.body_type);
                    let pos = row_col_to_offset(body, self.state.body_vim.cursor_row, self.state.body_vim.cursor_col);
                    if pos < body.len() && body.as_bytes()[pos] != b'\n' {
                        body.remove(pos);
                        body.insert(pos, c);
                    }
                } else if self.state.active_panel == Panel::Request && self.state.request_edit.field_editing {
                    self.push_request_undo();
                    let cursor = self.get_request_cursor();
                    let len = self.get_request_field_len();
                    if cursor < len {
                        self.replace_request_char_at(cursor, c);
                    }
                }
            }
            Action::Substitute => {
                if self.state.active_panel == Panel::Response && self.state.response_view.tab == ResponseTab::Type && self.state.response_view.type_sub_focus == crate::state::TypeSubFocus::Editor {
                    self.state.response_view.type_vim.save_undo();
                    self.state.response_view.type_vim.delete_char_at_cursor();
                    self.sync_type_vim_text();
                    self.state.response_view.type_locked = true;
                    self.state.mode = InputMode::Insert;
                } else if self.state.active_panel == Panel::Body {
                    self.push_body_undo();
                    let body = self.state.current_request.get_body_mut(self.state.body_type);
                    let pos = row_col_to_offset(body, self.state.body_vim.cursor_row, self.state.body_vim.cursor_col);
                    if pos < body.len() && body.as_bytes()[pos] != b'\n' {
                        let ch = body.remove(pos);
                        self.state.yank_buffer = ch.to_string();
                    }
                    self.state.mode = InputMode::Insert;
                } else if self.state.active_panel == Panel::Request && self.state.request_edit.field_editing {
                    self.push_request_undo();
                    let cursor = self.get_request_cursor();
                    let len = self.get_request_field_len();
                    if cursor < len {
                        let text = self.get_request_field_text();
                        self.state.yank_buffer = text[cursor..cursor+1].to_string();
                        self.delete_request_char_under_cursor();
                    }
                    self.state.mode = InputMode::Insert;
                }
            }
            Action::CopyResponseBody => {
                if let Some(ref resp) = self.state.current_response {
                    match crate::clipboard::copy_to_clipboard(&resp.formatted_body()) {
                        Ok(()) => self.state.set_status("Response body copied"),
                        Err(e) => self.state.set_status(e),
                    }
                }
            }
            Action::CopyAsCurl => {
                let resolved = self.resolve_env_vars(&self.state.current_request);
                let curl = http_client::to_curl(&resolved);
                match crate::clipboard::copy_to_clipboard(&curl) {
                    Ok(()) => self.state.set_status("Curl command copied"),
                    Err(e) => self.state.set_status(e),
                }
            }
            _ => {}
        }
        Ok(())
    }
}
