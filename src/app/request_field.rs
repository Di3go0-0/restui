use crate::core::state::{RequestFocus, UNDO_STACK_MAX};

use super::inline_edit::is_word_char;
use super::App;

impl App {
    pub(super) fn get_request_cursor(&self) -> usize {
        match self.state.request_edit.focus {
            RequestFocus::Url => self.state.request_edit.url_cursor,
            RequestFocus::Header(_) => self.state.request_edit.header_edit_cursor,
            RequestFocus::Param(_) => self.state.request_edit.param_edit_cursor,
            RequestFocus::Cookie(_) => self.state.request_edit.cookie_edit_cursor,
            RequestFocus::PathParam(_) => self.state.request_edit.path_param_edit_cursor,
        }
    }

    pub(super) fn set_request_cursor(&mut self, pos: usize) {
        match self.state.request_edit.focus {
            RequestFocus::Url => self.state.request_edit.url_cursor = pos,
            RequestFocus::Header(_) => self.state.request_edit.header_edit_cursor = pos,
            RequestFocus::Param(_) => self.state.request_edit.param_edit_cursor = pos,
            RequestFocus::Cookie(_) => self.state.request_edit.cookie_edit_cursor = pos,
            RequestFocus::PathParam(_) => self.state.request_edit.path_param_edit_cursor = pos,
        }
    }

    pub(super) fn get_request_field_len(&self) -> usize {
        self.get_request_field_text().len()
    }

    pub(super) fn get_request_field_text(&self) -> String {
        match self.state.request_edit.focus {
            RequestFocus::Url => self.state.current_request.url.clone(),
            RequestFocus::Header(idx) => {
                self.state.current_request.headers.get(idx).map(|h| {
                    if self.state.request_edit.header_edit_field == 0 { h.name.clone() } else { h.value.clone() }
                }).unwrap_or_default()
            }
            RequestFocus::Param(idx) => {
                self.state.current_request.query_params.get(idx).map(|p| {
                    if self.state.request_edit.param_edit_field == 0 { p.key.clone() } else { p.value.clone() }
                }).unwrap_or_default()
            }
            RequestFocus::Cookie(idx) => {
                self.state.current_request.cookies.get(idx).map(|c| {
                    if self.state.request_edit.cookie_edit_field == 0 { c.name.clone() } else { c.value.clone() }
                }).unwrap_or_default()
            }
            RequestFocus::PathParam(idx) => {
                self.state.current_request.path_params.get(idx).map(|p| {
                    if self.state.request_edit.path_param_edit_field == 0 { p.key.clone() } else { p.value.clone() }
                }).unwrap_or_default()
            }
        }
    }

    pub(super) fn clear_request_field(&mut self) {
        match self.state.request_edit.focus {
            RequestFocus::Url => self.state.current_request.url.clear(),
            RequestFocus::Header(idx) => {
                if let Some(h) = self.state.current_request.headers.get_mut(idx) {
                    if self.state.request_edit.header_edit_field == 0 { h.name.clear(); } else { h.value.clear(); }
                }
            }
            RequestFocus::Param(idx) => {
                if let Some(p) = self.state.current_request.query_params.get_mut(idx) {
                    if self.state.request_edit.param_edit_field == 0 { p.key.clear(); } else { p.value.clear(); }
                }
            }
            RequestFocus::Cookie(idx) => {
                if let Some(c) = self.state.current_request.cookies.get_mut(idx) {
                    if self.state.request_edit.cookie_edit_field == 0 { c.name.clear(); } else { c.value.clear(); }
                }
            }
            RequestFocus::PathParam(idx) => {
                if let Some(p) = self.state.current_request.path_params.get_mut(idx) {
                    if self.state.request_edit.path_param_edit_field == 0 { p.key.clear(); } else { p.value.clear(); }
                }
            }
        }
    }

    /// Drain a range [start..end) from the currently focused request field.
    pub(super) fn drain_request_field(&mut self, start: usize, end: usize) {
        if start >= end { return; }
        match self.state.request_edit.focus {
            RequestFocus::Url => { self.state.current_request.url.drain(start..end); }
            RequestFocus::Header(idx) => {
                if let Some(h) = self.state.current_request.headers.get_mut(idx) {
                    let field = if self.state.request_edit.header_edit_field == 0 { &mut h.name } else { &mut h.value };
                    field.drain(start..end);
                }
            }
            RequestFocus::Param(idx) => {
                if let Some(p) = self.state.current_request.query_params.get_mut(idx) {
                    let field = if self.state.request_edit.param_edit_field == 0 { &mut p.key } else { &mut p.value };
                    field.drain(start..end);
                }
            }
            RequestFocus::Cookie(idx) => {
                if let Some(c) = self.state.current_request.cookies.get_mut(idx) {
                    let field = if self.state.request_edit.cookie_edit_field == 0 { &mut c.name } else { &mut c.value };
                    field.drain(start..end);
                }
            }
            RequestFocus::PathParam(idx) => {
                if let Some(p) = self.state.current_request.path_params.get_mut(idx) {
                    let field = if self.state.request_edit.path_param_edit_field == 0 { &mut p.key } else { &mut p.value };
                    field.drain(start..end);
                }
            }
        }
    }

    pub(super) fn get_request_visual_selection(&self) -> String {
        let text = self.get_request_field_text();
        let cursor = self.get_request_cursor();
        let anchor = self.state.request_edit.visual_anchor;
        let start = cursor.min(anchor);
        let end = (cursor.max(anchor) + 1).min(text.len());
        if start <= end { text[start..end].to_string() } else { String::new() }
    }

    pub(super) fn delete_request_visual_selection(&mut self) {
        let cursor = self.get_request_cursor();
        let anchor = self.state.request_edit.visual_anchor;
        let start = cursor.min(anchor);
        let end = (cursor.max(anchor) + 1).min(self.get_request_field_len());
        match self.state.request_edit.focus {
            RequestFocus::Url => { self.state.current_request.url.drain(start..end); }
            RequestFocus::Header(idx) => {
                if let Some(h) = self.state.current_request.headers.get_mut(idx) {
                    let field = if self.state.request_edit.header_edit_field == 0 { &mut h.name } else { &mut h.value };
                    field.drain(start..end);
                }
            }
            RequestFocus::Param(idx) => {
                if let Some(p) = self.state.current_request.query_params.get_mut(idx) {
                    let field = if self.state.request_edit.param_edit_field == 0 { &mut p.key } else { &mut p.value };
                    field.drain(start..end);
                }
            }
            RequestFocus::Cookie(idx) => {
                if let Some(c) = self.state.current_request.cookies.get_mut(idx) {
                    let field = if self.state.request_edit.cookie_edit_field == 0 { &mut c.name } else { &mut c.value };
                    field.drain(start..end);
                }
            }
            RequestFocus::PathParam(idx) => {
                if let Some(p) = self.state.current_request.path_params.get_mut(idx) {
                    let field = if self.state.request_edit.path_param_edit_field == 0 { &mut p.key } else { &mut p.value };
                    field.drain(start..end);
                }
            }
        }
        self.set_request_cursor(start);
    }

    pub(super) fn delete_request_char_under_cursor(&mut self) {
        let cursor = self.get_request_cursor();
        let len = self.get_request_field_len();
        if cursor >= len { return; }
        match self.state.request_edit.focus {
            RequestFocus::Url => { self.state.current_request.url.remove(cursor); }
            RequestFocus::Header(idx) => {
                if let Some(h) = self.state.current_request.headers.get_mut(idx) {
                    let field = if self.state.request_edit.header_edit_field == 0 { &mut h.name } else { &mut h.value };
                    field.remove(cursor);
                }
            }
            RequestFocus::Param(idx) => {
                if let Some(p) = self.state.current_request.query_params.get_mut(idx) {
                    let field = if self.state.request_edit.param_edit_field == 0 { &mut p.key } else { &mut p.value };
                    field.remove(cursor);
                }
            }
            RequestFocus::Cookie(idx) => {
                if let Some(c) = self.state.current_request.cookies.get_mut(idx) {
                    let field = if self.state.request_edit.cookie_edit_field == 0 { &mut c.name } else { &mut c.value };
                    field.remove(cursor);
                }
            }
            RequestFocus::PathParam(idx) => {
                if let Some(p) = self.state.current_request.path_params.get_mut(idx) {
                    let field = if self.state.request_edit.path_param_edit_field == 0 { &mut p.key } else { &mut p.value };
                    field.remove(cursor);
                }
            }
        }
        // Clamp cursor
        let new_len = self.get_request_field_len();
        if cursor >= new_len && new_len > 0 {
            self.set_request_cursor(new_len - 1);
        }
    }

    pub(super) fn replace_request_char_at(&mut self, pos: usize, c: char) {
        match self.state.request_edit.focus {
            RequestFocus::Url => {
                if pos < self.state.current_request.url.len() {
                    self.state.current_request.url.remove(pos);
                    self.state.current_request.url.insert(pos, c);
                }
            }
            RequestFocus::Header(idx) => {
                if let Some(h) = self.state.current_request.headers.get_mut(idx) {
                    let field = if self.state.request_edit.header_edit_field == 0 { &mut h.name } else { &mut h.value };
                    if pos < field.len() { field.remove(pos); field.insert(pos, c); }
                }
            }
            RequestFocus::Param(idx) => {
                if let Some(p) = self.state.current_request.query_params.get_mut(idx) {
                    let field = if self.state.request_edit.param_edit_field == 0 { &mut p.key } else { &mut p.value };
                    if pos < field.len() { field.remove(pos); field.insert(pos, c); }
                }
            }
            RequestFocus::Cookie(idx) => {
                if let Some(ck) = self.state.current_request.cookies.get_mut(idx) {
                    let field = if self.state.request_edit.cookie_edit_field == 0 { &mut ck.name } else { &mut ck.value };
                    if pos < field.len() { field.remove(pos); field.insert(pos, c); }
                }
            }
            RequestFocus::PathParam(idx) => {
                if let Some(p) = self.state.current_request.path_params.get_mut(idx) {
                    let field = if self.state.request_edit.path_param_edit_field == 0 { &mut p.key } else { &mut p.value };
                    if pos < field.len() { field.remove(pos); field.insert(pos, c); }
                }
            }
        }
    }

    pub(super) fn paste_request_text(&mut self, text: &str) {
        // Filter newlines out for single-line fields
        let clean: String = text.chars().filter(|c| *c != '\n' && *c != '\r').collect();
        let cursor = self.get_request_cursor();
        match self.state.request_edit.focus {
            RequestFocus::Url => { self.state.current_request.url.insert_str(cursor, &clean); }
            RequestFocus::Header(idx) => {
                if let Some(h) = self.state.current_request.headers.get_mut(idx) {
                    let field = if self.state.request_edit.header_edit_field == 0 { &mut h.name } else { &mut h.value };
                    field.insert_str(cursor, &clean);
                }
            }
            RequestFocus::Param(idx) => {
                if let Some(p) = self.state.current_request.query_params.get_mut(idx) {
                    let field = if self.state.request_edit.param_edit_field == 0 { &mut p.key } else { &mut p.value };
                    field.insert_str(cursor, &clean);
                }
            }
            RequestFocus::Cookie(idx) => {
                if let Some(c) = self.state.current_request.cookies.get_mut(idx) {
                    let field = if self.state.request_edit.cookie_edit_field == 0 { &mut c.name } else { &mut c.value };
                    field.insert_str(cursor, &clean);
                }
            }
            RequestFocus::PathParam(idx) => {
                if let Some(p) = self.state.current_request.path_params.get_mut(idx) {
                    let field = if self.state.request_edit.path_param_edit_field == 0 { &mut p.key } else { &mut p.value };
                    field.insert_str(cursor, &clean);
                }
            }
        }
        self.set_request_cursor(cursor + clean.len());
    }

    pub(super) fn request_word_forward(&mut self) {
        let text = self.get_request_field_text();
        let bytes = text.as_bytes();
        let mut col = self.get_request_cursor();
        if col < bytes.len() {
            if is_word_char(bytes[col]) {
                while col < bytes.len() && is_word_char(bytes[col]) { col += 1; }
            } else if super::inline_edit::is_punct_char(bytes[col]) {
                while col < bytes.len() && super::inline_edit::is_punct_char(bytes[col]) { col += 1; }
            }
            while col < bytes.len() && bytes[col].is_ascii_whitespace() { col += 1; }
        }
        self.set_request_cursor(col.min(bytes.len()));
    }

    pub(super) fn request_word_backward(&mut self) {
        let text = self.get_request_field_text();
        let bytes = text.as_bytes();
        let mut col = self.get_request_cursor();
        if col == 0 { return; }
        col = col.saturating_sub(1);
        while col > 0 && bytes[col].is_ascii_whitespace() { col -= 1; }
        if col > 0 && is_word_char(bytes[col]) {
            while col > 0 && is_word_char(bytes[col - 1]) { col -= 1; }
        } else if col > 0 && super::inline_edit::is_punct_char(bytes[col]) {
            while col > 0 && super::inline_edit::is_punct_char(bytes[col - 1]) { col -= 1; }
        }
        self.set_request_cursor(col);
    }

    pub(super) fn request_word_end(&mut self) {
        let text = self.get_request_field_text();
        let bytes = text.as_bytes();
        let mut col = self.get_request_cursor();
        if col + 1 >= bytes.len() { return; }
        col += 1;
        while col < bytes.len() && bytes[col].is_ascii_whitespace() { col += 1; }
        if col >= bytes.len() {
            self.set_request_cursor(bytes.len().saturating_sub(1));
            return;
        }
        if is_word_char(bytes[col]) {
            while col + 1 < bytes.len() && is_word_char(bytes[col + 1]) { col += 1; }
        } else if super::inline_edit::is_punct_char(bytes[col]) {
            while col + 1 < bytes.len() && super::inline_edit::is_punct_char(bytes[col + 1]) { col += 1; }
        }
        self.set_request_cursor(col);
    }

    /// Save a snapshot of the current request field for undo.
    pub(super) fn push_request_undo(&mut self) {
        let focus = self.state.request_edit.focus;
        let edit_field = match focus {
            RequestFocus::Header(_) => self.state.request_edit.header_edit_field,
            RequestFocus::Param(_) => self.state.request_edit.param_edit_field,
            RequestFocus::Cookie(_) => self.state.request_edit.cookie_edit_field,
            RequestFocus::PathParam(_) => self.state.request_edit.path_param_edit_field,
            RequestFocus::Url => 0,
        };
        let text = self.get_request_field_text();
        let cursor = self.get_request_cursor();
        self.state.request_edit.undo_stack.push((focus, edit_field, text, cursor));
        self.state.request_edit.redo_stack.clear();
        if self.state.request_edit.undo_stack.len() > UNDO_STACK_MAX {
            self.state.request_edit.undo_stack.remove(0);
        }
    }

    /// Restore a request field from an undo/redo snapshot.
    pub(super) fn set_request_field_text(&mut self, focus: RequestFocus, edit_field: u8, text: String) {
        match focus {
            RequestFocus::Url => self.state.current_request.url = text,
            RequestFocus::Header(idx) => {
                if let Some(h) = self.state.current_request.headers.get_mut(idx) {
                    if edit_field == 0 { h.name = text; } else { h.value = text; }
                }
            }
            RequestFocus::Param(idx) => {
                if let Some(p) = self.state.current_request.query_params.get_mut(idx) {
                    if edit_field == 0 { p.key = text; } else { p.value = text; }
                }
            }
            RequestFocus::Cookie(idx) => {
                if let Some(c) = self.state.current_request.cookies.get_mut(idx) {
                    if edit_field == 0 { c.name = text; } else { c.value = text; }
                }
            }
            RequestFocus::PathParam(idx) => {
                if let Some(p) = self.state.current_request.path_params.get_mut(idx) {
                    if edit_field == 0 { p.key = text; } else { p.value = text; }
                }
            }
        }
    }

    pub(super) fn position_request_cursor_at_end(&mut self) {
        let len = self.get_request_field_len();
        // In normal mode, cursor sits on last char; in insert mode, after last char
        let end = if self.state.mode == crate::core::state::InputMode::Insert { len } else { len.saturating_sub(1) };
        self.set_request_cursor(end);
    }

    #[allow(dead_code)]
    pub(super) fn position_body_cursor_at_end(&mut self) {
        let body = self.state.current_request.get_body_mut(self.state.body_type);
        let lines: Vec<&str> = body.lines().collect();
        if lines.is_empty() {
            self.state.body_vim.cursor_row = 0;
            self.state.body_vim.cursor_col = 0;
        } else {
            self.state.body_vim.cursor_row = lines.len() - 1;
            self.state.body_vim.cursor_col = lines.last().map(|l| l.len()).unwrap_or(0);
        }
    }
}
