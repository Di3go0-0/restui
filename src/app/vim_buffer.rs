// ── Vim word-class helpers (used by request field editing) ──────────────────

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
