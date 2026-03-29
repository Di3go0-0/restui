use crate::state::Panel;

use super::App;

impl App {
    pub(super) fn recalculate_search_matches(&mut self) {
        self.state.search.matches.clear();
        self.state.search.match_idx = 0;
        if self.state.search.query.is_empty() {
            return;
        }
        let query = self.state.search.query.to_lowercase();
        let text = match self.state.active_panel {
            Panel::Response => {
                if let Some(ref resp) = self.state.current_response {
                    resp.formatted_body()
                } else {
                    return;
                }
            }
            Panel::Body => {
                self.active_body().to_string()
            }
            _ => return,
        };
        for (row, line) in text.lines().enumerate() {
            let line_lower = line.to_lowercase();
            let mut start = 0;
            while let Some(pos) = line_lower[start..].find(&query) {
                self.state.search.matches.push((row, start + pos));
                start += pos + 1;
            }
        }
        // Jump to first match
        if !self.state.search.matches.is_empty() {
            self.jump_to_current_search_match();
        }
    }

    pub(super) fn jump_to_current_search_match(&mut self) {
        if let Some(&(row, col)) = self.state.search.matches.get(self.state.search.match_idx) {
            match self.state.active_panel {
                Panel::Response => {
                    self.state.resp_cursor_row = row;
                    self.state.resp_cursor_col = col;
                    let visible = self.state.resp_visible_height as usize;
                    if row < self.state.response_scroll.0 as usize {
                        self.state.response_scroll.0 = row as u16;
                    } else if row >= self.state.response_scroll.0 as usize + visible {
                        self.state.response_scroll.0 = (row.saturating_sub(visible / 2)) as u16;
                    }
                }
                Panel::Body => {
                    self.state.body_buf.cursor_row = row;
                    self.state.body_buf.cursor_col = col;
                    let visible = self.state.body_buf.visible_height as usize;
                    if row < self.state.body_buf.scroll.0 as usize {
                        self.state.body_buf.scroll.0 = row as u16;
                    } else if row >= self.state.body_buf.scroll.0 as usize + visible {
                        self.state.body_buf.scroll.0 = (row.saturating_sub(visible / 2)) as u16;
                    }
                }
                _ => {}
            }
        }
    }
}
