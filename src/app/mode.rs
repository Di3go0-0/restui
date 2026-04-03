use anyhow::Result;

use crate::action::Action;
use crate::state::{InputMode, Panel, RequestFocus, ResponseTab};
use vimltui::{VimMode, VisualKind};

use super::App;

impl App {
    pub(super) fn handle_mode_transition(&mut self, action: Action, _count: usize) -> Result<()> {
        match action {
            Action::EnterInsertMode => {
                match self.state.active_panel {
                    Panel::Body => {
                        self.push_body_undo();
                        self.state.mode = InputMode::Insert;
                        // i inserts at current cursor position (don't move to end)
                    }
                    Panel::Request => {
                        self.push_request_undo();
                        self.state.mode = InputMode::Insert;
                        // If already in field-edit mode, keep cursor position; otherwise go to end
                        if !self.state.request_field_editing {
                            self.position_request_cursor_at_end();
                        }
                        self.state.request_field_editing = true;
                    }
                    Panel::Response if self.state.response_tab == ResponseTab::Type && self.state.type_sub_focus == crate::state::TypeSubFocus::Editor => {
                        self.state.type_vim.save_undo();
                        self.state.mode = InputMode::Insert;
                    }
                    _ => {}
                }
            }
            Action::EnterInsertModeStart => {
                match self.state.active_panel {
                    Panel::Body => {
                        self.push_body_undo();
                        self.state.mode = InputMode::Insert;
                        self.state.body_vim.cursor_col = 0;
                    }
                    Panel::Request => {
                        self.push_request_undo();
                        self.state.mode = InputMode::Insert;
                        self.state.request_field_editing = true;
                        self.set_request_cursor(0);
                    }
                    Panel::Response if self.state.response_tab == ResponseTab::Type && self.state.type_sub_focus == crate::state::TypeSubFocus::Editor => {
                        self.state.type_vim.save_undo();
                        self.state.type_vim.cursor_col = 0;
                        self.state.mode = InputMode::Insert;
                    }
                    _ => {}
                }
            }
            Action::EnterAppendMode => {
                match self.state.active_panel {
                    Panel::Body => {
                        self.push_body_undo();
                        self.state.mode = InputMode::Insert;
                        let body = self.state.current_request.get_body(self.state.body_type);
                        let lines: Vec<&str> = body.lines().collect();
                        let line_len = lines.get(self.state.body_vim.cursor_row).map(|l| l.len()).unwrap_or(0);
                        self.state.body_vim.cursor_col = (self.state.body_vim.cursor_col + 1).min(line_len);
                    }
                    Panel::Request => {
                        self.push_request_undo();
                        self.state.mode = InputMode::Insert;
                        self.state.request_field_editing = true;
                        let cursor = self.get_request_cursor();
                        let len = self.get_request_field_len();
                        self.set_request_cursor((cursor + 1).min(len));
                    }
                    Panel::Response if self.state.response_tab == ResponseTab::Type && self.state.type_sub_focus == crate::state::TypeSubFocus::Editor => {
                        self.state.type_vim.save_undo();
                        {
                            let line_len = self.state.type_vim.current_line_len();
                            let max = line_len.saturating_sub(1);
                            if self.state.type_vim.cursor_col < max {
                                self.state.type_vim.cursor_col += 1;
                            }
                        }
                        self.state.mode = InputMode::Insert;
                    }
                    _ => {}
                }
            }
            Action::EnterAppendModeEnd => {
                match self.state.active_panel {
                    Panel::Body => {
                        self.push_body_undo();
                        self.state.mode = InputMode::Insert;
                        let body = self.state.current_request.get_body(self.state.body_type);
                        let lines: Vec<&str> = body.lines().collect();
                        let line_len = lines.get(self.state.body_vim.cursor_row).map(|l| l.len()).unwrap_or(0);
                        self.state.body_vim.cursor_col = line_len;
                    }
                    Panel::Request => {
                        self.push_request_undo();
                        self.state.mode = InputMode::Insert;
                        self.state.request_field_editing = true;
                        let len = self.get_request_field_len();
                        self.set_request_cursor(len);
                    }
                    Panel::Response if self.state.response_tab == ResponseTab::Type && self.state.type_sub_focus == crate::state::TypeSubFocus::Editor => {
                        self.state.type_vim.save_undo();
                        self.state.type_vim.cursor_col = self.state.type_vim.current_line_len();
                        self.state.mode = InputMode::Insert;
                    }
                    _ => {}
                }
            }
            Action::ExitInsertMode => {
                // Sync query params from URL when leaving insert on URL field
                if self.state.active_panel == Panel::Request && self.state.request_focus == RequestFocus::Url {
                    self.sync_params_from_url();
                }
                self.state.mode = InputMode::Normal;
                self.state.autocomplete = None;
                self.state.chain_autocomplete = None;
                self.state.validate_body();
                // Clamp cursor to last char (normal mode can't be past end)
                match self.state.active_panel {
                    Panel::Body => {
                        let body = self.state.current_request.get_body(self.state.body_type);
                        let lines: Vec<&str> = body.lines().collect();
                        let line_len = lines.get(self.state.body_vim.cursor_row).map(|l| l.len()).unwrap_or(0);
                        if line_len > 0 {
                            self.state.body_vim.cursor_col = self.state.body_vim.cursor_col.min(line_len - 1);
                        }
                    }
                    Panel::Request => {
                        self.state.request_field_editing = true;
                        let len = self.get_request_field_len();
                        if len > 0 {
                            let cursor = self.get_request_cursor();
                            self.set_request_cursor(cursor.min(len - 1));
                        }
                    }
                    Panel::Response if self.state.response_tab == ResponseTab::Type && self.state.type_sub_focus == crate::state::TypeSubFocus::Editor => {
                        // Clamp cursor for type editor
                        let lines: Vec<&str> = self.state.response_type_text.lines().collect();
                        let line_len = lines.get(self.state.type_vim.cursor_row).map(|l| l.len()).unwrap_or(0);
                        if line_len > 0 {
                            self.state.type_vim.cursor_col = self.state.type_vim.cursor_col.min(line_len - 1);
                        }
                        // Validate after editing
                        self.validate_response_type();
                    }
                    _ => {}
                }
            }
            Action::EnterRequestFieldEdit => {
                if self.state.active_panel == Panel::Request {
                    self.state.request_field_editing = true;
                    self.position_request_cursor_at_end();
                }
            }
            Action::ExitRequestFieldEdit => {
                self.state.request_field_editing = false;
                // Sync query params from URL when leaving field edit
                if self.state.active_panel == Panel::Request && self.state.request_focus == RequestFocus::Url {
                    self.sync_params_from_url();
                }
            }
            Action::EnterVisualMode => {
                match self.state.active_panel {
                    Panel::Body => {
                        self.state.mode = InputMode::Visual;
                        self.state.body_vim.visual_anchor = Some((self.state.body_vim.cursor_row, self.state.body_vim.cursor_col));
                    }
                    Panel::Response if self.state.response_tab == ResponseTab::Type && self.state.type_sub_focus == crate::state::TypeSubFocus::Editor => {
                        self.state.mode = InputMode::Visual;
                        self.state.type_vim.visual_anchor = Some((self.state.type_vim.cursor_row, self.state.type_vim.cursor_col));
                        self.state.type_vim.mode = VimMode::Visual(VisualKind::Char);
                    }
                    Panel::Response => {
                        self.state.mode = InputMode::Visual;
                        self.state.resp_vim.visual_anchor = Some((self.state.resp_vim.cursor_row, self.state.resp_vim.cursor_col));
                    }
                    Panel::Request if self.state.request_field_editing => {
                        self.state.mode = InputMode::Visual;
                        self.state.request_visual_anchor = self.get_request_cursor();
                    }
                    _ => {}
                }
            }
            Action::EnterVisualBlockMode => {
                match self.state.active_panel {
                    Panel::Body => {
                        self.state.mode = InputMode::VisualBlock;
                        self.state.body_vim.visual_anchor = Some((self.state.body_vim.cursor_row, self.state.body_vim.cursor_col));
                    }
                    Panel::Response => {
                        self.state.mode = InputMode::VisualBlock;
                        self.state.resp_vim.visual_anchor = Some((self.state.resp_vim.cursor_row, self.state.resp_vim.cursor_col));
                    }
                    _ => {}
                }
            }
            Action::ExitVisualMode => {
                self.state.mode = InputMode::Normal;
                // Stay in field-edit mode when exiting visual in Request panel
                if self.state.active_panel == Panel::Request {
                    self.state.request_field_editing = true;
                }
            }
            _ => {}
        }
        Ok(())
    }
}
