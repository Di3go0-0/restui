use crate::state::UNDO_STACK_MAX;

use super::inline_edit::{is_word_char, is_punct_char, row_col_to_offset};
use super::App;

impl App {
    pub(super) fn active_body(&self) -> &str {
        self.state.current_request.get_body(self.state.body_type)
    }

    pub(super) fn set_active_body(&mut self, value: Option<String>) {
        self.state.current_request.set_body(self.state.body_type, value);
    }

    /// Save a snapshot of the body for undo. Call before any body mutation.
    pub(super) fn push_body_undo(&mut self) {
        let body = self.active_body().to_string();
        let lines: Vec<String> = if body.is_empty() {
            vec![String::new()]
        } else {
            body.lines().map(String::from).collect()
        };
        self.state.body_vim.undo_stack.push(vimltui::Snapshot {
            lines,
            cursor_row: self.state.body_vim.cursor_row,
            cursor_col: self.state.body_vim.cursor_col,
        });
        self.state.body_vim.redo_stack.clear(); // new edit clears redo history
        // Cap undo history at 100 entries
        if self.state.body_vim.undo_stack.len() > UNDO_STACK_MAX {
            self.state.body_vim.undo_stack.remove(0);
        }
    }

    /// Sync body text from the request into body_vim.lines (without resetting cursor/undo).
    pub(super) fn sync_body_to_vim(&mut self) {
        let body = self.state.current_request.get_body(self.state.body_type).to_string();
        let lines: Vec<String> = if body.is_empty() {
            vec![String::new()]
        } else {
            body.lines().map(String::from).collect()
        };
        self.state.body_vim.lines = lines;
    }

    /// Sync body_vim content back to the request body.
    pub(super) fn sync_vim_to_body(&mut self) {
        let new_body = self.state.body_vim.content();
        let value = if new_body.is_empty() { None } else { Some(new_body) };
        self.state.current_request.set_body(self.state.body_type, value);
    }

    pub(super) fn paste_text_at_cursor(&mut self, text: &str) {
        if text.is_empty() { return; }
        let body = self.state.current_request.get_body_mut(self.state.body_type);
        let pos = row_col_to_offset(body, self.state.body_vim.cursor_row, self.state.body_vim.cursor_col);
        body.insert_str(pos, text);
        // Move cursor to end of pasted text
        let new_lines: usize = text.chars().filter(|c| *c == '\n').count();
        if new_lines > 0 {
            self.state.body_vim.cursor_row += new_lines;
            let last_line = text.rsplit('\n').next().unwrap_or("");
            self.state.body_vim.cursor_col = last_line.len();
        } else {
            self.state.body_vim.cursor_col += text.len();
        }
    }

    pub(super) fn body_word_forward(&mut self) {
        let (text, cursor_row, cursor_col) = self.body_cursor_ptrs();
        let lines: Vec<&str> = text.lines().collect();
        // SAFETY: raw pointers to avoid borrow issues within the same struct
        unsafe {
            if let Some(line) = lines.get(*cursor_row) {
                let bytes = line.as_bytes();
                let mut col = *cursor_col;
                if col < bytes.len() {
                    // Skip current word class
                    if is_word_char(bytes[col]) {
                        while col < bytes.len() && is_word_char(bytes[col]) { col += 1; }
                    } else if is_punct_char(bytes[col]) {
                        while col < bytes.len() && is_punct_char(bytes[col]) { col += 1; }
                    }
                    // Skip whitespace
                    while col < bytes.len() && bytes[col].is_ascii_whitespace() { col += 1; }
                }
                if col >= bytes.len() && *cursor_row + 1 < lines.len() {
                    *cursor_row += 1;
                    *cursor_col = 0;
                } else {
                    *cursor_col = col.min(bytes.len());
                }
            }
        }
    }

    pub(super) fn body_word_backward(&mut self) {
        let (text, cursor_row, cursor_col) = self.body_cursor_ptrs();
        let lines: Vec<&str> = text.lines().collect();
        unsafe {
            if let Some(line) = lines.get(*cursor_row) {
                let bytes = line.as_bytes();
                let mut col = *cursor_col;
                if col == 0 {
                    if *cursor_row > 0 {
                        *cursor_row -= 1;
                        *cursor_col = lines.get(*cursor_row).map(|l| l.len()).unwrap_or(0);
                    }
                    return;
                }
                col = col.saturating_sub(1);
                // Skip whitespace backwards
                while col > 0 && bytes[col].is_ascii_whitespace() { col -= 1; }
                // Skip current word class backwards
                if col > 0 && is_word_char(bytes[col]) {
                    while col > 0 && is_word_char(bytes[col - 1]) { col -= 1; }
                } else if col > 0 && is_punct_char(bytes[col]) {
                    while col > 0 && is_punct_char(bytes[col - 1]) { col -= 1; }
                }
                *cursor_col = col;
            }
        }
    }

    pub(super) fn body_word_end(&mut self) {
        let (text, cursor_row, cursor_col) = self.body_cursor_ptrs();
        let lines: Vec<&str> = text.lines().collect();
        unsafe {
            if let Some(line) = lines.get(*cursor_row) {
                let bytes = line.as_bytes();
                let mut col = *cursor_col;
                if col + 1 >= bytes.len() {
                    // At or past end of line, move to next line
                    if *cursor_row + 1 < lines.len() {
                        *cursor_row += 1;
                        let next_line = lines[*cursor_row].as_bytes();
                        let mut c = 0;
                        // Skip whitespace at start of next line
                        while c < next_line.len() && next_line[c].is_ascii_whitespace() { c += 1; }
                        // Skip word class to find end
                        if c < next_line.len() && is_word_char(next_line[c]) {
                            while c + 1 < next_line.len() && is_word_char(next_line[c + 1]) { c += 1; }
                        } else if c < next_line.len() && is_punct_char(next_line[c]) {
                            while c + 1 < next_line.len() && is_punct_char(next_line[c + 1]) { c += 1; }
                        }
                        *cursor_col = c;
                    }
                    return;
                }
                col += 1;
                // Skip whitespace
                while col < bytes.len() && bytes[col].is_ascii_whitespace() { col += 1; }
                if col >= bytes.len() {
                    *cursor_col = bytes.len().saturating_sub(1);
                    return;
                }
                // Skip word class to find end
                if is_word_char(bytes[col]) {
                    while col + 1 < bytes.len() && is_word_char(bytes[col + 1]) { col += 1; }
                } else if is_punct_char(bytes[col]) {
                    while col + 1 < bytes.len() && is_punct_char(bytes[col + 1]) { col += 1; }
                }
                *cursor_col = col;
            }
        }
    }

    pub(super) fn get_visual_selection(&self) -> String {
        let body = self.state.current_request.get_body(self.state.body_type);
        let (sr, sc, er, ec) = self.visual_range();
        let start = row_col_to_offset(body, sr, sc);
        let end = row_col_to_offset(body, er, ec).min(body.len());
        if start <= end { body[start..end].to_string() } else { String::new() }
    }

    pub(super) fn delete_visual_selection(&mut self) {
        let (sr, sc, er, ec) = self.visual_range();
        let body = self.state.current_request.get_body_mut(self.state.body_type);
        let start = row_col_to_offset(body, sr, sc);
        let end = row_col_to_offset(body, er, ec).min(body.len());
        if start < end { body.drain(start..end); }
        self.state.body_vim.cursor_row = sr;
        self.state.body_vim.cursor_col = sc;
    }

    pub(super) fn visual_range(&self) -> (usize, usize, usize, usize) {
        let (ar, ac) = self.state.body_vim.visual_anchor.unwrap_or((0, 0));
        let (cr, cc) = (self.state.body_vim.cursor_row, self.state.body_vim.cursor_col);
        if (ar, ac) <= (cr, cc) { (ar, ac, cr, cc) } else { (cr, cc, ar, ac) }
    }

    /// Get block (rectangle) selection text from body — each line's column slice joined by newlines.
    pub(super) fn get_block_selection(&self) -> String {
        let body = self.state.current_request.get_body(self.state.body_type);
        let lines: Vec<&str> = body.lines().collect();
        let (ar, ac) = self.state.body_vim.visual_anchor.unwrap_or((0, 0));
        let (cr, cc) = (self.state.body_vim.cursor_row, self.state.body_vim.cursor_col);
        let (min_row, min_col, max_row, max_col) = (ar.min(cr), ac.min(cc), ar.max(cr), ac.max(cc));
        let mut result = Vec::new();
        for row in min_row..=max_row {
            if let Some(line) = lines.get(row) {
                let start = min_col.min(line.len());
                let end = max_col.min(line.len());
                result.push(&line[start..end]);
            }
        }
        result.join("\n")
    }

    /// Delete the block (rectangle) selection from body.
    pub(super) fn delete_block_selection(&mut self) {
        let (ar, ac) = self.state.body_vim.visual_anchor.unwrap_or((0, 0));
        let (cr, cc) = (self.state.body_vim.cursor_row, self.state.body_vim.cursor_col);
        let (min_row, min_col, max_row, max_col) = (ar.min(cr), ac.min(cc), ar.max(cr), ac.max(cc));

        let body = self.state.current_request.get_body_mut(self.state.body_type);
        let mut lines: Vec<String> = body.lines().map(|l| l.to_string()).collect();
        for row in min_row..=max_row {
            if let Some(line) = lines.get_mut(row) {
                let start = min_col.min(line.len());
                let end = max_col.min(line.len());
                line.drain(start..end);
            }
        }
        *body = lines.join("\n");
        self.state.body_vim.cursor_row = min_row;
        self.state.body_vim.cursor_col = min_col;
    }

    /// Get block selection from response (read-only, for yank).
    pub(super) fn get_response_block_selection(&self) -> String {
        let body = self.get_response_body_text();
        let lines: Vec<&str> = body.lines().collect();
        let (ar, ac) = self.state.response_view.resp_vim.visual_anchor.unwrap_or((0, 0));
        let (cr, cc) = (self.state.response_view.resp_vim.cursor_row, self.state.response_view.resp_vim.cursor_col);
        let (min_row, min_col, max_row, max_col) = (ar.min(cr), ac.min(cc), ar.max(cr), ac.max(cc));
        let mut result = Vec::new();
        for row in min_row..=max_row {
            if let Some(line) = lines.get(row) {
                let start = min_col.min(line.len());
                let end = max_col.min(line.len());
                result.push(&line[start..end]);
            }
        }
        result.join("\n")
    }

    pub(super) fn get_response_visual_selection(&self) -> String {
        let body = self.get_response_body_text();
        let (sr, sc, er, ec) = self.resp_visual_range();
        let start = row_col_to_offset(&body, sr, sc);
        let end = row_col_to_offset(&body, er, ec).min(body.len());
        if start <= end { body[start..end].to_string() } else { String::new() }
    }

    pub(super) fn resp_visual_range(&self) -> (usize, usize, usize, usize) {
        let (ar, ac) = self.state.response_view.resp_vim.visual_anchor.unwrap_or((0, 0));
        let (cr, cc) = (self.state.response_view.resp_vim.cursor_row, self.state.response_view.resp_vim.cursor_col);
        if (ar, ac) <= (cr, cc) { (ar, ac, cr, cc) } else { (cr, cc, ar, ac) }
    }

    pub(super) fn delete_body_line(&mut self, row: usize) {
        let body = self.state.current_request.get_body_mut(self.state.body_type);
        let mut lines: Vec<String> = body.lines().map(|l| l.to_string()).collect();
        if row < lines.len() {
            lines.remove(row);
            *body = lines.join("\n");
            let max_row = body.lines().count().saturating_sub(1);
            self.state.body_vim.cursor_row = self.state.body_vim.cursor_row.min(max_row);
            let cur_line_len = body.lines().nth(self.state.body_vim.cursor_row).map(|l| l.len()).unwrap_or(0);
            self.state.body_vim.cursor_col = self.state.body_vim.cursor_col.min(cur_line_len);
        }
    }
}

/// Word forward motion for VimEditor using its internal lines
pub(super) fn vim_editor_word_forward(editor: &mut vimltui::VimEditor) {
    if let Some(line) = editor.lines.get(editor.cursor_row) {
        let bytes = line.as_bytes();
        let mut col = editor.cursor_col;
        if col < bytes.len() {
            if is_word_char(bytes[col]) {
                while col < bytes.len() && is_word_char(bytes[col]) { col += 1; }
            } else if is_punct_char(bytes[col]) {
                while col < bytes.len() && is_punct_char(bytes[col]) { col += 1; }
            }
            while col < bytes.len() && bytes[col].is_ascii_whitespace() { col += 1; }
        }
        if col >= bytes.len() && editor.cursor_row + 1 < editor.lines.len() {
            editor.cursor_row += 1;
            editor.cursor_col = 0;
        } else {
            editor.cursor_col = col.min(bytes.len());
        }
    }
}

/// Word backward motion for VimEditor using its internal lines
pub(super) fn vim_editor_word_backward(editor: &mut vimltui::VimEditor) {
    if let Some(line) = editor.lines.get(editor.cursor_row) {
        let bytes = line.as_bytes();
        let mut col = editor.cursor_col;
        if col == 0 {
            if editor.cursor_row > 0 {
                editor.cursor_row -= 1;
                editor.cursor_col = editor.lines.get(editor.cursor_row).map(|l| l.len()).unwrap_or(0);
            }
            return;
        }
        col = col.saturating_sub(1);
        while col > 0 && bytes[col].is_ascii_whitespace() { col -= 1; }
        if col > 0 && is_word_char(bytes[col]) {
            while col > 0 && is_word_char(bytes[col - 1]) { col -= 1; }
        } else if col > 0 && is_punct_char(bytes[col]) {
            while col > 0 && is_punct_char(bytes[col - 1]) { col -= 1; }
        }
        editor.cursor_col = col;
    }
}

/// Word end motion for VimEditor using its internal lines
pub(super) fn vim_editor_word_end(editor: &mut vimltui::VimEditor) {
    if let Some(line) = editor.lines.get(editor.cursor_row) {
        let bytes = line.as_bytes();
        let mut col = editor.cursor_col;
        if col + 1 >= bytes.len() {
            if editor.cursor_row + 1 < editor.lines.len() {
                editor.cursor_row += 1;
                let next = editor.lines[editor.cursor_row].as_bytes();
                let mut c = 0;
                while c < next.len() && next[c].is_ascii_whitespace() { c += 1; }
                if c < next.len() && is_word_char(next[c]) {
                    while c + 1 < next.len() && is_word_char(next[c + 1]) { c += 1; }
                } else if c < next.len() && is_punct_char(next[c]) {
                    while c + 1 < next.len() && is_punct_char(next[c + 1]) { c += 1; }
                }
                editor.cursor_col = c;
            }
            return;
        }
        col += 1;
        while col < bytes.len() && bytes[col].is_ascii_whitespace() { col += 1; }
        if col >= bytes.len() {
            editor.cursor_col = bytes.len().saturating_sub(1);
            return;
        }
        if is_word_char(bytes[col]) {
            while col + 1 < bytes.len() && is_word_char(bytes[col + 1]) { col += 1; }
        } else if is_punct_char(bytes[col]) {
            while col + 1 < bytes.len() && is_punct_char(bytes[col + 1]) { col += 1; }
        }
        editor.cursor_col = col;
    }
}
