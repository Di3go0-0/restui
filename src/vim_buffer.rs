use crate::state::{InputMode, UNDO_STACK_MAX};

/// Number of lines to keep visible above/below cursor (like vim scrolloff).
pub const SCROLLOFF: usize = 2;

// ── Vim word-class helpers ──────────────────────────────────────────────────

fn is_word_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

fn is_punct_char(b: u8) -> bool {
    !b.is_ascii_whitespace() && !is_word_char(b)
}

/// Find the end of the current word forward from `col` in `line` bytes.
/// Skips current word class + trailing whitespace (like vim `w`/`dw`).
pub fn word_end_forward(bytes: &[u8], col: usize) -> usize {
    let mut end = col;
    if end < bytes.len() {
        if is_word_char(bytes[end]) {
            while end < bytes.len() && is_word_char(bytes[end]) { end += 1; }
        } else if is_punct_char(bytes[end]) {
            while end < bytes.len() && is_punct_char(bytes[end]) { end += 1; }
        }
        while end < bytes.len() && bytes[end].is_ascii_whitespace() { end += 1; }
    }
    end
}

/// Find the start of the current word backward from `col` in `line` bytes.
pub fn word_start_backward(bytes: &[u8], col: usize) -> usize {
    if col == 0 { return 0; }
    let mut start = col.saturating_sub(1);
    while start > 0 && bytes[start].is_ascii_whitespace() { start -= 1; }
    if start > 0 && is_word_char(bytes[start]) {
        while start > 0 && is_word_char(bytes[start - 1]) { start -= 1; }
    } else if start > 0 && is_punct_char(bytes[start]) {
        while start > 0 && is_punct_char(bytes[start - 1]) { start -= 1; }
    }
    start
}

pub fn row_col_to_offset(text: &str, row: usize, col: usize) -> usize {
    let mut offset = 0;
    for (i, line) in text.split('\n').enumerate() {
        if i == row {
            return offset + col.min(line.len());
        }
        offset += line.len() + 1;
    }
    text.len()
}

// ── VimBuffer ───────────────────────────────────────────────────────────────

#[derive(Debug)]
pub struct VimBuffer {
    pub cursor_row: usize,
    pub cursor_col: usize,
    pub scroll: (u16, u16),
    pub visual_anchor_row: usize,
    pub visual_anchor_col: usize,
    pub visible_height: u16,
    pub visible_width: u16,
    pub undo_stack: Vec<(String, usize, usize)>,
    pub redo_stack: Vec<(String, usize, usize)>,
}

impl Default for VimBuffer {
    fn default() -> Self {
        Self {
            cursor_row: 0,
            cursor_col: 0,
            scroll: (0, 0),
            visual_anchor_row: 0,
            visual_anchor_col: 0,
            visible_height: 20,
            visible_width: 80,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
        }
    }
}

impl VimBuffer {
    // ── Cursor movement ─────────────────────────────────────────────────

    pub fn move_left(&mut self, _text: &str, mode: InputMode) {
        if self.cursor_col > 0 {
            self.cursor_col -= 1;
        } else if mode == InputMode::Normal && self.cursor_row > 0 {
            // Don't wrap in normal mode
        }
    }

    pub fn move_right(&mut self, text: &str, mode: InputMode) {
        let lines: Vec<&str> = text.lines().collect();
        if let Some(line) = lines.get(self.cursor_row) {
            let max = if mode == InputMode::Insert {
                line.len()
            } else {
                line.len().saturating_sub(1)
            };
            if self.cursor_col < max {
                self.cursor_col += 1;
            }
        }
    }

    pub fn move_up(&mut self, text: &str, mode: InputMode) {
        if self.cursor_row > 0 {
            self.cursor_row -= 1;
            self.clamp_col(text, mode);
        }
    }

    pub fn move_down(&mut self, text: &str, mode: InputMode) {
        let line_count = text.lines().count().max(1);
        if self.cursor_row + 1 < line_count {
            self.cursor_row += 1;
            self.clamp_col(text, mode);
        }
    }

    pub fn home(&mut self) {
        self.cursor_col = 0;
    }

    pub fn end(&mut self, text: &str, mode: InputMode) {
        let lines: Vec<&str> = text.lines().collect();
        if let Some(line) = lines.get(self.cursor_row) {
            self.cursor_col = if mode == InputMode::Insert {
                line.len()
            } else {
                line.len().saturating_sub(1)
            };
        }
    }

    fn clamp_col(&mut self, text: &str, mode: InputMode) {
        let lines: Vec<&str> = text.lines().collect();
        if let Some(line) = lines.get(self.cursor_row) {
            let max = if mode == InputMode::Insert {
                line.len()
            } else {
                line.len().saturating_sub(1)
            };
            self.cursor_col = self.cursor_col.min(max);
        }
    }

    // ── Word motions ────────────────────────────────────────────────────

    pub fn word_forward(&mut self, text: &str) {
        let lines: Vec<&str> = text.lines().collect();
        if let Some(line) = lines.get(self.cursor_row) {
            let bytes = line.as_bytes();
            let mut col = self.cursor_col;
            if col < bytes.len() {
                if is_word_char(bytes[col]) {
                    while col < bytes.len() && is_word_char(bytes[col]) { col += 1; }
                } else if is_punct_char(bytes[col]) {
                    while col < bytes.len() && is_punct_char(bytes[col]) { col += 1; }
                }
                while col < bytes.len() && bytes[col].is_ascii_whitespace() { col += 1; }
            }
            if col >= bytes.len() && self.cursor_row + 1 < lines.len() {
                self.cursor_row += 1;
                self.cursor_col = 0;
            } else {
                self.cursor_col = col.min(bytes.len());
            }
        }
    }

    pub fn word_backward(&mut self, text: &str) {
        let lines: Vec<&str> = text.lines().collect();
        if let Some(line) = lines.get(self.cursor_row) {
            let bytes = line.as_bytes();
            let mut col = self.cursor_col;
            if col == 0 {
                if self.cursor_row > 0 {
                    self.cursor_row -= 1;
                    self.cursor_col = lines.get(self.cursor_row).map(|l| l.len()).unwrap_or(0);
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
            self.cursor_col = col;
        }
    }

    pub fn word_end(&mut self, text: &str) {
        let lines: Vec<&str> = text.lines().collect();
        if let Some(line) = lines.get(self.cursor_row) {
            let bytes = line.as_bytes();
            let mut col = self.cursor_col;
            if col + 1 >= bytes.len() {
                if self.cursor_row + 1 < lines.len() {
                    self.cursor_row += 1;
                    let next = lines[self.cursor_row].as_bytes();
                    let mut c = 0;
                    while c < next.len() && next[c].is_ascii_whitespace() { c += 1; }
                    if c < next.len() && is_word_char(next[c]) {
                        while c + 1 < next.len() && is_word_char(next[c + 1]) { c += 1; }
                    } else if c < next.len() && is_punct_char(next[c]) {
                        while c + 1 < next.len() && is_punct_char(next[c + 1]) { c += 1; }
                    }
                    self.cursor_col = c;
                }
                return;
            }
            col += 1;
            while col < bytes.len() && bytes[col].is_ascii_whitespace() { col += 1; }
            if col >= bytes.len() {
                self.cursor_col = bytes.len().saturating_sub(1);
                return;
            }
            if is_word_char(bytes[col]) {
                while col + 1 < bytes.len() && is_word_char(bytes[col + 1]) { col += 1; }
            } else if is_punct_char(bytes[col]) {
                while col + 1 < bytes.len() && is_punct_char(bytes[col + 1]) { col += 1; }
            }
            self.cursor_col = col;
        }
    }

    // ── Find char ───────────────────────────────────────────────────────

    pub fn find_char_forward(&mut self, text: &str, target: char, before: bool) {
        let lines: Vec<&str> = text.lines().collect();
        if let Some(line) = lines.get(self.cursor_row) {
            let bytes = line.as_bytes();
            let start = self.cursor_col + 1;
            for i in start..bytes.len() {
                if bytes[i] == target as u8 {
                    self.cursor_col = if before {
                        i.saturating_sub(1).max(start.saturating_sub(1))
                    } else {
                        i
                    };
                    break;
                }
            }
        }
    }

    pub fn find_char_backward(&mut self, text: &str, target: char, after: bool) {
        let lines: Vec<&str> = text.lines().collect();
        if let Some(line) = lines.get(self.cursor_row) {
            let bytes = line.as_bytes();
            let col = self.cursor_col;
            if col > 0 {
                for i in (0..col).rev() {
                    if bytes[i] == target as u8 {
                        self.cursor_col = if after { (i + 1).min(col) } else { i };
                        break;
                    }
                }
            }
        }
    }

    // ── Text editing ────────────────────────────────────────────────────

    pub fn insert_char(&mut self, text: &mut String, c: char) {
        let offset = row_col_to_offset(text, self.cursor_row, self.cursor_col);
        text.insert(offset, c);
        self.cursor_col += 1;
    }

    pub fn backspace(&mut self, text: &mut String) {
        if self.cursor_col > 0 {
            let offset = row_col_to_offset(text, self.cursor_row, self.cursor_col);
            if offset > 0 {
                text.remove(offset - 1);
                self.cursor_col -= 1;
            }
        } else if self.cursor_row > 0 {
            // Join with previous line
            let offset = row_col_to_offset(text, self.cursor_row, 0);
            if offset > 0 {
                let prev_line_len = text.lines().nth(self.cursor_row - 1).map(|l| l.len()).unwrap_or(0);
                text.remove(offset - 1); // remove the \n
                self.cursor_row -= 1;
                self.cursor_col = prev_line_len;
            }
        }
    }

    pub fn delete_char(&mut self, text: &mut String) {
        let offset = row_col_to_offset(text, self.cursor_row, self.cursor_col);
        if offset < text.len() {
            text.remove(offset);
            // Clamp cursor
            let lines: Vec<&str> = text.lines().collect();
            if let Some(line) = lines.get(self.cursor_row) {
                if self.cursor_col > 0 && self.cursor_col >= line.len() {
                    self.cursor_col = line.len().saturating_sub(1);
                }
            }
        }
    }

    pub fn newline(&mut self, text: &mut String) {
        // Determine indent from current line
        let indent = text.lines()
            .nth(self.cursor_row)
            .map(|line| {
                let trimmed = line.trim_start();
                &line[..line.len() - trimmed.len()]
            })
            .unwrap_or("")
            .to_string();

        let offset = row_col_to_offset(text, self.cursor_row, self.cursor_col);
        text.insert_str(offset, &format!("\n{}", indent));
        self.cursor_row += 1;
        self.cursor_col = indent.len();
    }

    pub fn delete_line(&mut self, text: &mut String) -> String {
        let lines: Vec<String> = text.lines().map(|l| l.to_string()).collect();
        if self.cursor_row < lines.len() {
            let yanked = lines[self.cursor_row].clone();
            let mut new_lines: Vec<String> = lines;
            new_lines.remove(self.cursor_row);
            *text = new_lines.join("\n");
            let max_row = text.lines().count().saturating_sub(1);
            self.cursor_row = self.cursor_row.min(max_row);
            self.clamp_col_raw(text);
            yanked
        } else {
            String::new()
        }
    }

    pub fn change_line(&mut self, text: &mut String) -> String {
        let lines: Vec<&str> = text.lines().collect();
        if let Some(line) = lines.get(self.cursor_row) {
            let yanked = line.to_string();
            let indent = {
                let trimmed = line.trim_start();
                line[..line.len() - trimmed.len()].to_string()
            };
            // Replace line content with just indent
            let mut new_lines: Vec<String> = text.lines().map(|l| l.to_string()).collect();
            new_lines[self.cursor_row] = indent.clone();
            *text = new_lines.join("\n");
            self.cursor_col = indent.len();
            yanked
        } else {
            String::new()
        }
    }

    pub fn open_line_below(&mut self, text: &mut String) {
        let indent = text.lines()
            .nth(self.cursor_row)
            .map(|line| {
                let trimmed = line.trim_start();
                line[..line.len() - trimmed.len()].to_string()
            })
            .unwrap_or_default();
        let line_end = row_col_to_offset(text, self.cursor_row, usize::MAX);
        text.insert_str(line_end, &format!("\n{}", indent));
        self.cursor_row += 1;
        self.cursor_col = indent.len();
    }

    pub fn open_line_above(&mut self, text: &mut String) {
        let indent = text.lines()
            .nth(self.cursor_row)
            .map(|line| {
                let trimmed = line.trim_start();
                line[..line.len() - trimmed.len()].to_string()
            })
            .unwrap_or_default();
        let line_start = row_col_to_offset(text, self.cursor_row, 0);
        text.insert_str(line_start, &format!("{}\n", indent));
        self.cursor_col = indent.len();
    }

    pub fn replace_char(&mut self, text: &mut String, c: char) {
        let offset = row_col_to_offset(text, self.cursor_row, self.cursor_col);
        if offset < text.len() {
            let ch = text.as_bytes()[offset];
            if ch != b'\n' {
                text.remove(offset);
                text.insert(offset, c);
            }
        }
    }

    pub fn delete_to_end(&mut self, text: &mut String) -> String {
        let lines: Vec<&str> = text.lines().collect();
        if let Some(line) = lines.get(self.cursor_row) {
            let yanked = line[self.cursor_col..].to_string();
            let start = row_col_to_offset(text, self.cursor_row, self.cursor_col);
            let end = row_col_to_offset(text, self.cursor_row, line.len());
            text.drain(start..end);
            self.clamp_col_raw(text);
            yanked
        } else {
            String::new()
        }
    }

    pub fn paste(&mut self, text: &mut String, content: &str) {
        let offset = row_col_to_offset(text, self.cursor_row, self.cursor_col);
        text.insert_str(offset, content);
        self.cursor_col += content.len();
    }

    // ── Visual selection ────────────────────────────────────────────────

    pub fn start_visual(&mut self) {
        self.visual_anchor_row = self.cursor_row;
        self.visual_anchor_col = self.cursor_col;
    }

    pub fn visual_range(&self) -> (usize, usize, usize, usize) {
        let (ar, ac) = (self.visual_anchor_row, self.visual_anchor_col);
        let (cr, cc) = (self.cursor_row, self.cursor_col);
        if (ar, ac) <= (cr, cc) {
            (ar, ac, cr, cc)
        } else {
            (cr, cc, ar, ac)
        }
    }

    pub fn get_visual_selection(&self, text: &str) -> String {
        let (sr, sc, er, ec) = self.visual_range();
        let start = row_col_to_offset(text, sr, sc);
        let end = row_col_to_offset(text, er, ec).min(text.len());
        if start <= end { text[start..end].to_string() } else { String::new() }
    }

    pub fn delete_visual_selection(&mut self, text: &mut String) -> String {
        let (sr, sc, er, ec) = self.visual_range();
        let start = row_col_to_offset(text, sr, sc);
        let end = row_col_to_offset(text, er, ec).min(text.len());
        let yanked = if start <= end {
            text.drain(start..end).collect()
        } else {
            String::new()
        };
        self.cursor_row = sr;
        self.cursor_col = sc;
        self.clamp_col_raw(text);
        yanked
    }

    // ── Undo / Redo ─────────────────────────────────────────────────────

    pub fn push_undo(&mut self, text: &str) {
        self.undo_stack.push((text.to_string(), self.cursor_row, self.cursor_col));
        self.redo_stack.clear();
        if self.undo_stack.len() > UNDO_STACK_MAX {
            self.undo_stack.remove(0);
        }
    }

    pub fn undo(&mut self, text: &mut String) -> bool {
        if let Some((snapshot, row, col)) = self.undo_stack.pop() {
            self.redo_stack.push((text.clone(), self.cursor_row, self.cursor_col));
            *text = snapshot;
            self.cursor_row = row;
            self.cursor_col = col;
            true
        } else {
            false
        }
    }

    pub fn redo(&mut self, text: &mut String) -> bool {
        if let Some((snapshot, row, col)) = self.redo_stack.pop() {
            self.undo_stack.push((text.clone(), self.cursor_row, self.cursor_col));
            *text = snapshot;
            self.cursor_row = row;
            self.cursor_col = col;
            true
        } else {
            false
        }
    }

    // ── Scroll sync ─────────────────────────────────────────────────────

    pub fn sync_scroll(&mut self) {
        let visible = self.visible_height as usize;
        if visible <= SCROLLOFF * 2 { return; }
        let scroll = self.scroll.0 as usize;
        if self.cursor_row < scroll + SCROLLOFF {
            self.scroll.0 = self.cursor_row.saturating_sub(SCROLLOFF) as u16;
        } else if self.cursor_row >= scroll + visible - SCROLLOFF {
            self.scroll.0 = (self.cursor_row - visible + SCROLLOFF + 1) as u16;
        }
    }

    pub fn sync_hscroll(&mut self) {
        let visible_w = self.visible_width as usize;
        if visible_w == 0 { return; }
        let hscroll = self.scroll.1 as usize;
        if self.cursor_col < hscroll {
            self.scroll.1 = self.cursor_col as u16;
        } else if self.cursor_col >= hscroll + visible_w {
            self.scroll.1 = (self.cursor_col - visible_w + 1) as u16;
        }
    }

    // ── Helpers ──────────────────────────────────────────────────────────

    fn clamp_col_raw(&mut self, text: &str) {
        let lines: Vec<&str> = text.lines().collect();
        if let Some(line) = lines.get(self.cursor_row) {
            self.cursor_col = self.cursor_col.min(line.len().saturating_sub(1));
        } else {
            self.cursor_col = 0;
        }
    }

    pub fn reset(&mut self) {
        self.cursor_row = 0;
        self.cursor_col = 0;
        self.scroll = (0, 0);
        self.undo_stack.clear();
        self.redo_stack.clear();
    }
}
