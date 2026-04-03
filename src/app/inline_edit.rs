use crate::state::{InputMode, Panel, RequestFocus, ResponseTab};

use super::App;

// Vim word-class helpers
pub(super) fn is_word_char(b: u8) -> bool { b.is_ascii_alphanumeric() || b == b'_' }
pub(super) fn is_punct_char(b: u8) -> bool { !b.is_ascii_whitespace() && !is_word_char(b) }

pub(super) fn row_col_to_offset(text: &str, row: usize, col: usize) -> usize {
    let mut offset = 0;
    for (i, line) in text.split('\n').enumerate() {
        if i == row { return offset + col.min(line.len()); }
        offset += line.len() + 1;
    }
    text.len()
}

impl App {
    pub(super) fn inline_input(&mut self, c: char) {
        match self.state.active_panel {
            Panel::Body => {
                let body = self.state.current_request.get_body_mut(self.state.body_type);
                let pos = row_col_to_offset(body, self.state.body_vim.cursor_row, self.state.body_vim.cursor_col);
                body.insert(pos, c);
                self.state.body_vim.cursor_col += 1;
            }
            Panel::Request => match self.state.request_edit.focus {
                RequestFocus::Url => {
                    let cursor = self.state.request_edit.url_cursor.min(self.state.current_request.url.len());
                    self.state.current_request.url.insert(cursor, c);
                    self.state.request_edit.url_cursor = cursor + 1;
                    self.state.autocomplete = None;
                }
                RequestFocus::Header(idx) => {
                    if let Some(h) = self.state.current_request.headers.get_mut(idx) {
                        let field = if self.state.request_edit.header_edit_field == 0 { &mut h.name } else { &mut h.value };
                        let cursor = self.state.request_edit.header_edit_cursor.min(field.len());
                        field.insert(cursor, c);
                        self.state.request_edit.header_edit_cursor = cursor + 1;
                    }
                    // Update autocomplete if editing header name
                    if self.state.request_edit.header_edit_field == 0 {
                        if let Some(h) = self.state.current_request.headers.get(idx) {
                            let ac = crate::state::Autocomplete::new(&h.name);
                            self.state.autocomplete = if ac.is_empty() { None } else { Some(ac) };
                        }
                    } else {
                        self.state.autocomplete = None;
                    }
                }
                RequestFocus::Param(idx) => {
                    if let Some(p) = self.state.current_request.query_params.get_mut(idx) {
                        let field = if self.state.request_edit.param_edit_field == 0 { &mut p.key } else { &mut p.value };
                        let cursor = self.state.request_edit.param_edit_cursor.min(field.len());
                        field.insert(cursor, c);
                        self.state.request_edit.param_edit_cursor = cursor + 1;
                    }
                }
                RequestFocus::Cookie(idx) => {
                    if let Some(ck) = self.state.current_request.cookies.get_mut(idx) {
                        let field = if self.state.request_edit.cookie_edit_field == 0 { &mut ck.name } else { &mut ck.value };
                        let cursor = self.state.request_edit.cookie_edit_cursor.min(field.len());
                        field.insert(cursor, c);
                        self.state.request_edit.cookie_edit_cursor = cursor + 1;
                    }
                }
                RequestFocus::PathParam(idx) => {
                    if let Some(p) = self.state.current_request.path_params.get_mut(idx) {
                        let field = if self.state.request_edit.path_param_edit_field == 0 { &mut p.key } else { &mut p.value };
                        let cursor = self.state.request_edit.path_param_edit_cursor.min(field.len());
                        field.insert(cursor, c);
                        self.state.request_edit.path_param_edit_cursor = cursor + 1;
                    }
                }
            },
            Panel::Response if self.state.response_view.tab == ResponseTab::Type && self.state.response_view.type_sub_focus == crate::state::TypeSubFocus::Editor => {
                let text = &mut self.state.response_view.type_text;
                let pos = row_col_to_offset(text, self.state.response_view.type_vim.cursor_row, self.state.response_view.type_vim.cursor_col);
                text.insert(pos, c);
                self.state.response_view.type_vim.cursor_col += 1;
                self.state.response_view.type_locked = true;
            }
            _ => {}
        }
    }

    pub(super) fn inline_backspace(&mut self) {
        match self.state.active_panel {
            Panel::Body => {
                let body = self.state.current_request.get_body_mut(self.state.body_type);
                let pos = row_col_to_offset(body, self.state.body_vim.cursor_row, self.state.body_vim.cursor_col);
                if pos > 0 {
                    let ch = body.as_bytes()[pos - 1];
                    body.remove(pos - 1);
                    if ch == b'\n' {
                        if self.state.body_vim.cursor_row > 0 {
                            self.state.body_vim.cursor_row -= 1;
                            let lines: Vec<&str> = body.lines().collect();
                            self.state.body_vim.cursor_col = lines.get(self.state.body_vim.cursor_row).map(|l| l.len()).unwrap_or(0);
                        }
                    } else {
                        self.state.body_vim.cursor_col = self.state.body_vim.cursor_col.saturating_sub(1);
                    }
                }
            }
            Panel::Request => match self.state.request_edit.focus {
                RequestFocus::Url => {
                    if self.state.request_edit.url_cursor > 0 {
                        self.state.request_edit.url_cursor -= 1;
                        self.state.current_request.url.remove(self.state.request_edit.url_cursor);
                    }
                }
                RequestFocus::Header(idx) => {
                    if self.state.request_edit.header_edit_cursor > 0 {
                        if let Some(h) = self.state.current_request.headers.get_mut(idx) {
                            let field = if self.state.request_edit.header_edit_field == 0 { &mut h.name } else { &mut h.value };
                            self.state.request_edit.header_edit_cursor -= 1;
                            if self.state.request_edit.header_edit_cursor < field.len() {
                                field.remove(self.state.request_edit.header_edit_cursor);
                            }
                        }
                    }
                    // Update autocomplete after backspace
                    if self.state.request_edit.header_edit_field == 0 {
                        if let Some(h) = self.state.current_request.headers.get(idx) {
                            if h.name.is_empty() {
                                self.state.autocomplete = None;
                            } else {
                                let ac = crate::state::Autocomplete::new(&h.name);
                                self.state.autocomplete = if ac.is_empty() { None } else { Some(ac) };
                            }
                        }
                    }
                }
                RequestFocus::Param(idx) => {
                    if self.state.request_edit.param_edit_cursor > 0 {
                        if let Some(p) = self.state.current_request.query_params.get_mut(idx) {
                            let field = if self.state.request_edit.param_edit_field == 0 { &mut p.key } else { &mut p.value };
                            self.state.request_edit.param_edit_cursor -= 1;
                            if self.state.request_edit.param_edit_cursor < field.len() {
                                field.remove(self.state.request_edit.param_edit_cursor);
                            }
                        }
                    }
                }
                RequestFocus::Cookie(idx) => {
                    if self.state.request_edit.cookie_edit_cursor > 0 {
                        if let Some(ck) = self.state.current_request.cookies.get_mut(idx) {
                            let field = if self.state.request_edit.cookie_edit_field == 0 { &mut ck.name } else { &mut ck.value };
                            self.state.request_edit.cookie_edit_cursor -= 1;
                            if self.state.request_edit.cookie_edit_cursor < field.len() {
                                field.remove(self.state.request_edit.cookie_edit_cursor);
                            }
                        }
                    }
                }
                RequestFocus::PathParam(idx) => {
                    if self.state.request_edit.path_param_edit_cursor > 0 {
                        if let Some(p) = self.state.current_request.path_params.get_mut(idx) {
                            let field = if self.state.request_edit.path_param_edit_field == 0 { &mut p.key } else { &mut p.value };
                            self.state.request_edit.path_param_edit_cursor -= 1;
                            if self.state.request_edit.path_param_edit_cursor < field.len() {
                                field.remove(self.state.request_edit.path_param_edit_cursor);
                            }
                        }
                    }
                }
            },
            Panel::Response if self.state.response_view.tab == ResponseTab::Type && self.state.response_view.type_sub_focus == crate::state::TypeSubFocus::Editor => {
                let text = &mut self.state.response_view.type_text;
                let pos = row_col_to_offset(text, self.state.response_view.type_vim.cursor_row, self.state.response_view.type_vim.cursor_col);
                if pos > 0 {
                    let ch = text.as_bytes()[pos - 1];
                    text.remove(pos - 1);
                    if ch == b'\n' {
                        if self.state.response_view.type_vim.cursor_row > 0 {
                            self.state.response_view.type_vim.cursor_row -= 1;
                            let lines: Vec<&str> = text.lines().collect();
                            self.state.response_view.type_vim.cursor_col = lines.get(self.state.response_view.type_vim.cursor_row).map(|l| l.len()).unwrap_or(0);
                        }
                    } else {
                        self.state.response_view.type_vim.cursor_col = self.state.response_view.type_vim.cursor_col.saturating_sub(1);
                    }
                    self.state.response_view.type_locked = true;
                }
            }
            _ => {}
        }
    }

    pub(super) fn inline_delete(&mut self) {
        match self.state.active_panel {
            Panel::Body => {
                let body = self.state.current_request.get_body_mut(self.state.body_type);
                let pos = row_col_to_offset(body, self.state.body_vim.cursor_row, self.state.body_vim.cursor_col);
                if pos < body.len() { body.remove(pos); }
            }
            Panel::Request => match self.state.request_edit.focus {
                RequestFocus::Url => {
                    if self.state.request_edit.url_cursor < self.state.current_request.url.len() {
                        self.state.current_request.url.remove(self.state.request_edit.url_cursor);
                    }
                }
                RequestFocus::Header(idx) => {
                    if let Some(h) = self.state.current_request.headers.get_mut(idx) {
                        let field = if self.state.request_edit.header_edit_field == 0 { &mut h.name } else { &mut h.value };
                        if self.state.request_edit.header_edit_cursor < field.len() { field.remove(self.state.request_edit.header_edit_cursor); }
                    }
                }
                RequestFocus::Param(idx) => {
                    if let Some(p) = self.state.current_request.query_params.get_mut(idx) {
                        let field = if self.state.request_edit.param_edit_field == 0 { &mut p.key } else { &mut p.value };
                        if self.state.request_edit.param_edit_cursor < field.len() { field.remove(self.state.request_edit.param_edit_cursor); }
                    }
                }
                RequestFocus::Cookie(idx) => {
                    if let Some(ck) = self.state.current_request.cookies.get_mut(idx) {
                        let field = if self.state.request_edit.cookie_edit_field == 0 { &mut ck.name } else { &mut ck.value };
                        if self.state.request_edit.cookie_edit_cursor < field.len() { field.remove(self.state.request_edit.cookie_edit_cursor); }
                    }
                }
                RequestFocus::PathParam(idx) => {
                    if let Some(p) = self.state.current_request.path_params.get_mut(idx) {
                        let field = if self.state.request_edit.path_param_edit_field == 0 { &mut p.key } else { &mut p.value };
                        if self.state.request_edit.path_param_edit_cursor < field.len() { field.remove(self.state.request_edit.path_param_edit_cursor); }
                    }
                }
            },
            Panel::Response if self.state.response_view.tab == ResponseTab::Type && self.state.response_view.type_sub_focus == crate::state::TypeSubFocus::Editor => {
                let text = &mut self.state.response_view.type_text;
                let pos = row_col_to_offset(text, self.state.response_view.type_vim.cursor_row, self.state.response_view.type_vim.cursor_col);
                if pos < text.len() {
                    text.remove(pos);
                    self.state.response_view.type_locked = true;
                }
            }
            _ => {}
        }
    }

    pub(super) fn inline_newline(&mut self) {
        if self.state.active_panel == Panel::Response && self.state.response_view.tab == ResponseTab::Type && self.state.response_view.type_sub_focus == crate::state::TypeSubFocus::Editor {
            let text = &mut self.state.response_view.type_text;
            let pos = row_col_to_offset(text, self.state.response_view.type_vim.cursor_row, self.state.response_view.type_vim.cursor_col);

            let lines: Vec<&str> = text.lines().collect();
            let current_line = lines.get(self.state.response_view.type_vim.cursor_row).copied().unwrap_or("");
            let leading_ws: String = current_line.chars().take_while(|c| c.is_whitespace()).collect();

            let char_before = if pos > 0 { text.as_bytes().get(pos - 1).copied() } else { None };
            let extra_indent = match char_before {
                Some(b'{') | Some(b'[') => "  ",
                _ => "",
            };

            let indent = format!("\n{}{}", leading_ws, extra_indent);
            text.insert_str(pos, &indent);
            self.state.response_view.type_vim.cursor_row += 1;
            self.state.response_view.type_vim.cursor_col = leading_ws.len() + extra_indent.len();
            self.state.response_view.type_locked = true;
            return;
        }
        if self.state.active_panel == Panel::Body {
            let body = self.state.current_request.get_body_mut(self.state.body_type);
            let pos = row_col_to_offset(body, self.state.body_vim.cursor_row, self.state.body_vim.cursor_col);

            // Determine indent: copy leading whitespace from current line
            let lines: Vec<&str> = body.lines().collect();
            let current_line = lines.get(self.state.body_vim.cursor_row).copied().unwrap_or("");
            let leading_ws: String = current_line.chars().take_while(|c| c.is_whitespace()).collect();

            // Check if char before cursor is { or [ for extra indent
            let char_before = if pos > 0 { body.as_bytes().get(pos - 1).copied() } else { None };
            let extra_indent = match char_before {
                Some(b'{') | Some(b'[') => "  ",
                _ => "",
            };

            let indent = format!("\n{}{}", leading_ws, extra_indent);
            body.insert_str(pos, &indent);
            self.state.body_vim.cursor_row += 1;
            self.state.body_vim.cursor_col = leading_ws.len() + extra_indent.len();
        }
    }

    pub(super) fn inline_cursor_left(&mut self) {
        match self.state.active_panel {
            Panel::Body => {
                if self.state.body_vim.cursor_col > 0 {
                    self.state.body_vim.cursor_col -= 1;
                } else if self.state.body_vim.cursor_row > 0 {
                    self.state.body_vim.cursor_row -= 1;
                    let body = self.state.current_request.get_body(self.state.body_type);
                    let lines: Vec<&str> = body.lines().collect();
                    self.state.body_vim.cursor_col = lines.get(self.state.body_vim.cursor_row).map(|l| l.len()).unwrap_or(0);
                }
            }
            Panel::Request => match self.state.request_edit.focus {
                RequestFocus::Url => { self.state.request_edit.url_cursor = self.state.request_edit.url_cursor.saturating_sub(1); }
                RequestFocus::Header(_) => { self.state.request_edit.header_edit_cursor = self.state.request_edit.header_edit_cursor.saturating_sub(1); }
                RequestFocus::Param(_) => { self.state.request_edit.param_edit_cursor = self.state.request_edit.param_edit_cursor.saturating_sub(1); }
                RequestFocus::Cookie(_) => { self.state.request_edit.cookie_edit_cursor = self.state.request_edit.cookie_edit_cursor.saturating_sub(1); }
                RequestFocus::PathParam(_) => { self.state.request_edit.path_param_edit_cursor = self.state.request_edit.path_param_edit_cursor.saturating_sub(1); }
            },
            Panel::Response if self.state.response_view.tab == ResponseTab::Type && self.state.response_view.type_sub_focus == crate::state::TypeSubFocus::Editor => {
                if self.state.response_view.type_vim.cursor_col > 0 {
                    self.state.response_view.type_vim.cursor_col -= 1;
                } else if self.state.mode == InputMode::Insert && self.state.response_view.type_vim.cursor_row > 0 {
                    // Wrap to previous line only in insert mode
                    self.state.response_view.type_vim.cursor_row -= 1;
                    let lines: Vec<&str> = self.state.response_view.type_text.lines().collect();
                    self.state.response_view.type_vim.cursor_col = lines.get(self.state.response_view.type_vim.cursor_row).map(|l| l.len()).unwrap_or(0);
                }
                // type_vim hscroll handled by ensure_cursor_visible
            }
            Panel::Response => {
                self.state.response_view.resp_vim.cursor_col = self.state.response_view.resp_vim.cursor_col.saturating_sub(1);
            }
            _ => {}
        }
        // Sync horizontal scroll after cursor movement
        match self.state.active_panel {
            Panel::Body => { self.sync_body_hscroll(); }
            Panel::Response if self.state.response_view.tab != ResponseTab::Type || self.state.response_view.type_sub_focus == crate::state::TypeSubFocus::Preview => { self.sync_resp_hscroll(); }
            _ => {}
        }
    }

    pub(super) fn inline_cursor_right(&mut self) {
        let is_insert = self.state.mode == InputMode::Insert;
        match self.state.active_panel {
            Panel::Body => {
                let body = self.state.current_request.get_body(self.state.body_type);
                let lines: Vec<&str> = body.lines().collect();
                let line_len = lines.get(self.state.body_vim.cursor_row).map(|l| l.len()).unwrap_or(0);
                // In normal mode, cursor stays on last char (max = len-1)
                let max = if is_insert { line_len } else { line_len.saturating_sub(1) };
                if self.state.body_vim.cursor_col < max {
                    self.state.body_vim.cursor_col += 1;
                } else if is_insert && self.state.body_vim.cursor_row + 1 < lines.len() {
                    self.state.body_vim.cursor_row += 1;
                    self.state.body_vim.cursor_col = 0;
                }
            }
            Panel::Request => {
                let len = self.get_request_field_len();
                let max = if is_insert { len } else { len.saturating_sub(1) };
                let cursor = self.get_request_cursor();
                if cursor < max {
                    self.set_request_cursor(cursor + 1);
                }
            }
            Panel::Response if self.state.response_view.tab == ResponseTab::Type && self.state.response_view.type_sub_focus == crate::state::TypeSubFocus::Editor => {
                let lines: Vec<&str> = self.state.response_view.type_text.lines().collect();
                let line_len = lines.get(self.state.response_view.type_vim.cursor_row).map(|l| l.len()).unwrap_or(0);
                let max = if is_insert { line_len } else { line_len.saturating_sub(1) };
                if self.state.response_view.type_vim.cursor_col < max {
                    self.state.response_view.type_vim.cursor_col += 1;
                } else if is_insert && self.state.response_view.type_vim.cursor_row + 1 < lines.len() {
                    self.state.response_view.type_vim.cursor_row += 1;
                    self.state.response_view.type_vim.cursor_col = 0;
                }
                // type_vim hscroll handled by ensure_cursor_visible
            }
            Panel::Response => {
                let lines = self.get_response_lines();
                let line_len = lines.get(self.state.response_view.resp_vim.cursor_row).map(|l| l.len()).unwrap_or(0);
                let max = line_len.saturating_sub(1); // response is always "normal mode"
                if self.state.response_view.resp_vim.cursor_col < max {
                    self.state.response_view.resp_vim.cursor_col += 1;
                }
            }
            _ => {}
        }
        // Sync horizontal scroll after cursor movement
        match self.state.active_panel {
            Panel::Body => { self.sync_body_hscroll(); }
            Panel::Response if self.state.response_view.tab != ResponseTab::Type || self.state.response_view.type_sub_focus == crate::state::TypeSubFocus::Preview => { self.sync_resp_hscroll(); }
            _ => {}
        }
    }

    pub(super) fn body_cursor_up(&mut self) {
        if self.state.body_vim.cursor_row > 0 {
            self.state.body_vim.cursor_row -= 1;
            let body = self.state.current_request.get_body(self.state.body_type);
            let lines: Vec<&str> = body.lines().collect();
            let line_len = lines.get(self.state.body_vim.cursor_row).map(|l| l.len()).unwrap_or(0);
            let max = if self.state.mode == InputMode::Insert { line_len } else { line_len.saturating_sub(1) };
            self.state.body_vim.cursor_col = self.state.body_vim.cursor_col.min(max);
        }
        self.sync_body_scroll(); self.sync_body_hscroll();
    }

    pub(super) fn body_cursor_down(&mut self) {
        let body = self.state.current_request.get_body(self.state.body_type);
        let line_count = body.lines().count().max(1);
        if self.state.body_vim.cursor_row + 1 < line_count {
            self.state.body_vim.cursor_row += 1;
            let lines: Vec<&str> = body.lines().collect();
            let line_len = lines.get(self.state.body_vim.cursor_row).map(|l| l.len()).unwrap_or(0);
            // In normal mode, clamp to last char; in insert mode, allow end position
            let max = if self.state.mode == InputMode::Insert { line_len } else { line_len.saturating_sub(1) };
            self.state.body_vim.cursor_col = self.state.body_vim.cursor_col.min(max);
        }
        self.sync_body_scroll(); self.sync_body_hscroll();
    }

    pub(super) fn inline_cursor_home(&mut self) {
        match self.state.active_panel {
            Panel::Body => { self.state.body_vim.cursor_col = 0; self.sync_body_hscroll(); },
            Panel::Response if self.state.response_view.tab == ResponseTab::Type && self.state.response_view.type_sub_focus == crate::state::TypeSubFocus::Editor && self.state.mode == InputMode::Insert => {
                self.state.response_view.type_vim.cursor_col = 0;
            },
            Panel::Response => { self.state.response_view.resp_vim.cursor_col = 0; self.sync_resp_hscroll(); },
            Panel::Request => match self.state.request_edit.focus {
                RequestFocus::Url => self.state.request_edit.url_cursor = 0,
                RequestFocus::Header(_) => self.state.request_edit.header_edit_cursor = 0,
                RequestFocus::Param(_) => self.state.request_edit.param_edit_cursor = 0,
                RequestFocus::Cookie(_) => self.state.request_edit.cookie_edit_cursor = 0,
                RequestFocus::PathParam(_) => self.state.request_edit.path_param_edit_cursor = 0,
            },
            _ => {}
        }
    }

    pub(super) fn inline_cursor_end(&mut self) {
        let is_insert = self.state.mode == InputMode::Insert;
        match self.state.active_panel {
            Panel::Body => {
                let body = self.state.current_request.get_body(self.state.body_type);
                let lines: Vec<&str> = body.lines().collect();
                let line_len = lines.get(self.state.body_vim.cursor_row).map(|l| l.len()).unwrap_or(0);
                self.state.body_vim.cursor_col = if is_insert { line_len } else { line_len.saturating_sub(1) };
                self.sync_body_hscroll();
            }
            Panel::Response if self.state.response_view.tab == ResponseTab::Type && self.state.response_view.type_sub_focus == crate::state::TypeSubFocus::Editor && is_insert => {
                let lines: Vec<&str> = self.state.response_view.type_text.lines().collect();
                let line_len = lines.get(self.state.response_view.type_vim.cursor_row).map(|l| l.len()).unwrap_or(0);
                self.state.response_view.type_vim.cursor_col = line_len;
            }
            Panel::Response => {
                let lines = self.get_response_lines();
                let line_len = lines.get(self.state.response_view.resp_vim.cursor_row).map(|l| l.len()).unwrap_or(0);
                self.state.response_view.resp_vim.cursor_col = line_len.saturating_sub(1);
                self.sync_resp_hscroll();
            }
            Panel::Request => {
                let len = self.get_request_field_len();
                let end = if is_insert { len } else { len.saturating_sub(1) };
                self.set_request_cursor(end);
            }
            _ => {}
        }
    }

    pub(super) fn inline_tab(&mut self) {
        match self.state.active_panel {
            Panel::Request => {
                // If autocomplete is open, accept it instead of tabbing
                if let Some(ac) = self.state.autocomplete.take() {
                    if let Some((name, value)) = ac.accept() {
                        if let RequestFocus::Header(idx) = self.state.request_edit.focus {
                            if let Some(h) = self.state.current_request.headers.get_mut(idx) {
                                h.name = name.to_string();
                                h.value = value.to_string();
                            }
                        }
                    }
                }
                self.state.autocomplete = None;
                // Toggle between name/value sub-field
                match self.state.request_edit.focus {
                    RequestFocus::Header(_) => self.state.request_edit.header_edit_field = (self.state.request_edit.header_edit_field + 1) % 2,
                    RequestFocus::Param(_) => self.state.request_edit.param_edit_field = (self.state.request_edit.param_edit_field + 1) % 2,
                    RequestFocus::Cookie(_) => self.state.request_edit.cookie_edit_field = (self.state.request_edit.cookie_edit_field + 1) % 2,
                    RequestFocus::PathParam(_) => self.state.request_edit.path_param_edit_field = (self.state.request_edit.path_param_edit_field + 1) % 2,
                    _ => {}
                }
                // Position cursor at end of new sub-field
                self.position_request_cursor_at_end();
            }
            Panel::Body => {
                let body = self.state.current_request.get_body_mut(self.state.body_type);
                let pos = row_col_to_offset(body, self.state.body_vim.cursor_row, self.state.body_vim.cursor_col);
                body.insert_str(pos, "  ");
                self.state.body_vim.cursor_col += 2;
            }
            _ => {}
        }
    }

    pub(super) fn body_cursor_ptrs(&mut self) -> (String, *mut usize, *mut usize) {
        match self.state.active_panel {
            Panel::Response => {
                let t = self.get_response_body_text();
                (t, &mut self.state.response_view.resp_vim.cursor_row as *mut usize, &mut self.state.response_view.resp_vim.cursor_col as *mut usize)
            }
            _ => {
                let t = self.active_body().to_string();
                (t, &mut self.state.body_vim.cursor_row as *mut usize, &mut self.state.body_vim.cursor_col as *mut usize)
            }
        }
    }
}
