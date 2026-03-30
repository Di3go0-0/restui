use crate::state::{Panel, ResponseTab, TypeSubFocus};

use super::App;

impl App {
    pub(super) fn scroll_down(&mut self) {
        match self.state.active_panel {
            Panel::Collections => {
                let i = self.state.collections_state.selected().map(|i| i + 1).unwrap_or(0);
                let max = self.state.collection_items.len().saturating_sub(1);
                self.state.collections_state.select(Some(i.min(max)));
            }
            Panel::Body => self.body_cursor_down(),
            Panel::Response => {
                if self.state.response_tab == ResponseTab::Type && self.state.type_sub_focus == TypeSubFocus::Editor {
                    self.type_cursor_down();
                } else {
                    self.resp_cursor_down();
                }
            }
            _ => {}
        }
    }

    pub(super) fn scroll_up(&mut self) {
        match self.state.active_panel {
            Panel::Collections => {
                let i = self.state.collections_state.selected().unwrap_or(0).saturating_sub(1);
                self.state.collections_state.select(Some(i));
            }
            Panel::Body => self.body_cursor_up(),
            Panel::Response => {
                if self.state.response_tab == ResponseTab::Type && self.state.type_sub_focus == TypeSubFocus::Editor {
                    self.type_cursor_up();
                } else {
                    self.resp_cursor_up();
                }
            }
            _ => {}
        }
    }

    pub(super) fn scroll_top(&mut self) {
        match self.state.active_panel {
            Panel::Collections => self.state.collections_state.select(Some(0)),
            Panel::Body => { self.state.body_buf.scroll = (0, 0); self.state.body_buf.cursor_row = 0; self.state.body_buf.cursor_col = 0; }
            Panel::Response if self.state.response_tab == ResponseTab::Type && self.state.type_sub_focus == TypeSubFocus::Editor => {
                self.state.type_buf.cursor_row = 0;
                self.state.type_buf.cursor_col = 0;
                self.state.type_buf.scroll = (0, 0);
            }
            Panel::Response => {
                self.state.resp_buf.cursor_row = 0;
                self.state.resp_buf.cursor_col = 0;
                self.state.resp_buf.scroll = (0, 0);
            }
            _ => {}
        }
    }

    pub(super) fn scroll_bottom(&mut self) {
        match self.state.active_panel {
            Panel::Collections => {
                let max = self.state.collection_items.len().saturating_sub(1);
                self.state.collections_state.select(Some(max));
            }
            Panel::Body => {
                let body = self.state.current_request.get_body(self.state.body_type);
                let lines: Vec<&str> = body.lines().collect();
                self.state.body_buf.cursor_row = lines.len().saturating_sub(1);
                self.state.body_buf.cursor_col = 0;
                self.sync_body_scroll(); self.sync_body_hscroll();
            }
            Panel::Response if self.state.response_tab == ResponseTab::Type && self.state.type_sub_focus == TypeSubFocus::Editor => {
                let line_count = self.state.response_type_text.lines().count();
                self.state.type_buf.cursor_row = line_count.saturating_sub(1);
                self.state.type_buf.cursor_col = 0;
                self.state.type_buf.sync_scroll();
            }
            Panel::Response => {
                let lines = self.get_response_lines();
                self.state.resp_buf.cursor_row = lines.len().saturating_sub(1);
                self.state.resp_buf.cursor_col = 0;
                self.sync_resp_scroll(); self.sync_resp_hscroll();
            }
            _ => {}
        }
    }

    pub(super) fn sync_body_scroll(&mut self) {
        let visible = self.state.body_buf.visible_height as usize;
        if visible == 0 { return; }
        let scroll = self.state.body_buf.scroll.0 as usize;
        let row = self.state.body_buf.cursor_row;
        if row < scroll {
            self.state.body_buf.scroll.0 = row as u16;
        } else if row >= scroll + visible {
            self.state.body_buf.scroll.0 = (row - visible + 1) as u16;
        }
    }

    pub(super) fn sync_body_hscroll(&mut self) {
        let col = self.state.body_buf.cursor_col;
        let hscroll = self.state.body_buf.scroll.1 as usize;
        let visible_w = self.state.body_buf.visible_width as usize;
        if visible_w == 0 { return; }
        if col < hscroll {
            self.state.body_buf.scroll.1 = col as u16;
        } else if col >= hscroll + visible_w {
            self.state.body_buf.scroll.1 = (col - visible_w + 1) as u16;
        }
    }

    pub(super) fn sync_resp_scroll(&mut self) {
        let visible = self.state.resp_buf.visible_height as usize;
        if visible == 0 { return; }
        let scroll = self.state.resp_buf.scroll.0 as usize;
        let row = self.state.resp_buf.cursor_row;
        if row < scroll {
            self.state.resp_buf.scroll.0 = row as u16;
        } else if row >= scroll + visible {
            self.state.resp_buf.scroll.0 = (row - visible + 1) as u16;
        }
    }

    pub(super) fn sync_resp_hscroll(&mut self) {
        let col = self.state.resp_buf.cursor_col;
        let hscroll = self.state.resp_buf.scroll.1 as usize;
        let visible_w = self.state.resp_buf.visible_width as usize;
        if visible_w == 0 { return; }
        if col < hscroll {
            self.state.resp_buf.scroll.1 = col as u16;
        } else if col >= hscroll + visible_w {
            self.state.resp_buf.scroll.1 = (col - visible_w + 1) as u16;
        }
    }

    pub(super) fn scroll_half_down(&mut self) {
        let half = 15usize;
        match self.state.active_panel {
            Panel::Body => {
                let body = self.state.current_request.get_body(self.state.body_type);
                let max = body.lines().count().saturating_sub(1);
                self.state.body_buf.cursor_row = (self.state.body_buf.cursor_row + half).min(max);
                self.sync_body_scroll(); self.sync_body_hscroll();
            }
            Panel::Response if self.state.response_tab == ResponseTab::Type && self.state.type_sub_focus == TypeSubFocus::Editor => {
                let max = self.state.response_type_text.lines().count().saturating_sub(1);
                self.state.type_buf.cursor_row = (self.state.type_buf.cursor_row + half).min(max);
                self.state.type_buf.sync_scroll();
            }
            Panel::Response => {
                let max = self.get_response_lines().len().saturating_sub(1);
                self.state.resp_buf.cursor_row = (self.state.resp_buf.cursor_row + half).min(max);
                self.sync_resp_scroll(); self.sync_resp_hscroll();
            }
            _ => {}
        }
    }

    pub(super) fn scroll_half_up(&mut self) {
        let half = 15usize;
        match self.state.active_panel {
            Panel::Body => {
                self.state.body_buf.cursor_row = self.state.body_buf.cursor_row.saturating_sub(half);
                self.sync_body_scroll(); self.sync_body_hscroll();
            }
            Panel::Response if self.state.response_tab == ResponseTab::Type && self.state.type_sub_focus == TypeSubFocus::Editor => {
                self.state.type_buf.cursor_row = self.state.type_buf.cursor_row.saturating_sub(half);
                self.state.type_buf.sync_scroll();
            }
            Panel::Response => {
                self.state.resp_buf.cursor_row = self.state.resp_buf.cursor_row.saturating_sub(half);
                self.sync_resp_scroll(); self.sync_resp_hscroll();
            }
            _ => {}
        }
    }

    // === Response & Type cursor helpers ===

    pub(super) fn get_response_body_text(&self) -> String {
        if let Some(ref resp) = self.state.current_response {
            resp.formatted_body()
        } else {
            String::new()
        }
    }

    pub(super) fn get_response_lines(&self) -> Vec<String> {
        self.get_response_body_text().lines().map(|l| l.to_string()).collect()
    }

    pub(super) fn resp_cursor_down(&mut self) {
        let lines = self.get_response_lines();
        if self.state.resp_buf.cursor_row + 1 < lines.len() {
            self.state.resp_buf.cursor_row += 1;
            let line_len = lines.get(self.state.resp_buf.cursor_row).map(|l| l.len()).unwrap_or(0);
            self.state.resp_buf.cursor_col = self.state.resp_buf.cursor_col.min(line_len);
        }
        self.sync_resp_scroll(); self.sync_resp_hscroll();
    }

    pub(super) fn resp_cursor_up(&mut self) {
        if self.state.resp_buf.cursor_row > 0 {
            self.state.resp_buf.cursor_row -= 1;
            let lines = self.get_response_lines();
            let line_len = lines.get(self.state.resp_buf.cursor_row).map(|l| l.len()).unwrap_or(0);
            self.state.resp_buf.cursor_col = self.state.resp_buf.cursor_col.min(line_len);
        }
        self.sync_resp_scroll(); self.sync_resp_hscroll();
    }

    pub(super) fn type_cursor_up(&mut self) {
        let mode = self.state.mode;
        let text = &self.state.response_type_text;
        self.state.type_buf.move_up(text, mode);
        self.state.type_buf.sync_scroll();
    }

    pub(super) fn type_cursor_down(&mut self) {
        let mode = self.state.mode;
        let text = &self.state.response_type_text;
        self.state.type_buf.move_down(text, mode);
        self.state.type_buf.sync_scroll();
    }

    /// Get mutable references to the active type text and buffer based on type_lang.
    pub(super) fn active_type_refs_mut(&mut self) -> (&str, &mut crate::vim_buffer::VimBuffer) {
        match self.state.type_lang {
            crate::state::TypeLang::Inferred => (&self.state.response_type_text, &mut self.state.type_buf),
            crate::state::TypeLang::TypeScript => (&self.state.type_ts_text, &mut self.state.type_ts_buf),
            crate::state::TypeLang::CSharp => (&self.state.type_csharp_text, &mut self.state.type_csharp_buf),
        }
    }

    /// Get mutable references to both text and buffer for editing.
    pub(super) fn active_type_edit_mut(&mut self) -> (&mut String, &mut crate::vim_buffer::VimBuffer) {
        match self.state.type_lang {
            crate::state::TypeLang::Inferred => (&mut self.state.response_type_text, &mut self.state.type_buf),
            crate::state::TypeLang::TypeScript => (&mut self.state.type_ts_text, &mut self.state.type_ts_buf),
            crate::state::TypeLang::CSharp => (&mut self.state.type_csharp_text, &mut self.state.type_csharp_buf),
        }
    }
}
