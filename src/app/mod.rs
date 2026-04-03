mod autocomplete;
mod body_edit;
mod collections;
mod execute;
mod inline_edit;
mod request_field;
mod scroll;
mod search;
mod vim_sync;

use inline_edit::{is_word_char, is_punct_char, row_col_to_offset};
use body_edit::{vim_editor_word_forward, vim_editor_word_backward, vim_editor_word_end};

use anyhow::Result;
use std::path::PathBuf;
use tokio::sync::mpsc;

use crate::action::Action;
use crate::config::AppConfig;
use crate::event::{AppEvent, EventHandler};
use crate::http_client;
use crate::keybindings;
use crate::model::collection::{Collection, FileFormat};
use crate::model::request::{Header, PathParam, QueryParam, Request};
use crate::parser;
use crate::state::{AppState, InputMode, Overlay, Panel, RequestFocus, RequestTab, ResponseTab, COMMON_HEADERS, WIDE_LAYOUT_THRESHOLD, STATUS_MESSAGE_TTL, PENDING_KEY_TIMEOUT, EVENT_TICK_RATE};
use crate::tui::Tui;
use crate::ui;
use crate::vim_buffer::row_col_to_offset as vim_row_col_to_offset;
use vimltui::{VimMode, VisualKind, Register};
use crossterm::cursor::SetCursorStyle;

pub struct App {
    pub state: AppState,
    action_tx: mpsc::UnboundedSender<Action>,
    action_rx: mpsc::UnboundedReceiver<Action>,
}

impl App {
    pub fn new(config: AppConfig, keybindings: crate::keybinding_config::KeybindingsConfig) -> Self {
        let (action_tx, action_rx) = mpsc::unbounded_channel();
        Self {
            state: AppState::new(config, keybindings),
            action_tx,
            action_rx,
        }
    }

    pub fn load_environments(&mut self, env_file: Option<&str>) {
        self.state.environments = parser::load_environments(env_file);
        if let Some(ref default_name) = self.state.config.general.default_environment {
            for (i, env) in self.state.environments.environments.iter().enumerate() {
                if env.name == *default_name {
                    self.state.environments.active = Some(i);
                    break;
                }
            }
        }
    }

    pub async fn run(&mut self, terminal: &mut Tui) -> Result<()> {
        let tick_rate = EVENT_TICK_RATE;
        let mut events = EventHandler::new(tick_rate);

        loop {
            if let Ok(size) = terminal.size() {
                let right_width = (size.width as u32 * 80 / 100) as u16;
                self.state.is_wide_layout = right_width > WIDE_LAYOUT_THRESHOLD;

                // Calculate visible heights for scroll-follow
                let main_h = size.height.saturating_sub(1); // status bar
                if right_width > WIDE_LAYOUT_THRESHOLD {
                    // Wide: center is 50% of right, body is 60% of center
                    let center_h = main_h;
                    self.state.body_vim.visible_height = (center_h as u32 * 60 / 100) as usize;
                    self.state.resp_vim.visible_height = main_h as usize;
                } else {
                    // Narrow: body 35%, response 40%
                    self.state.body_vim.visible_height = (main_h as u32 * 35 / 100) as usize;
                    self.state.resp_vim.visible_height = (main_h as u32 * 40 / 100) as usize;
                }
                // Account for borders + tab bar + internal chrome
                self.state.body_vim.visible_height = self.state.body_vim.visible_height.saturating_sub(4);
                // Account for borders + status line + tab bar + separator
                self.state.resp_vim.visible_height = self.state.resp_vim.visible_height.saturating_sub(6);
                // When Type tab is open, response preview only gets ~half minus separators
                if self.state.response_tab == ResponseTab::Type {
                    self.state.resp_vim.visible_height = (self.state.resp_vim.visible_height / 2).saturating_sub(1);
                }

                // Calculate visible widths for horizontal scroll-follow
                // Body and response panels share the right side; subtract gutter (4) + border (2)
                if right_width > WIDE_LAYOUT_THRESHOLD {
                    // Wide: center panel is ~40% of right
                    self.state.body_visible_width = (right_width as u32 * 40 / 100) as usize;
                    self.state.resp_visible_width = (right_width as u32 * 50 / 100) as usize;
                } else {
                    // Narrow: body/response take full right width
                    self.state.body_visible_width = right_width as usize;
                    self.state.resp_visible_width = right_width as usize;
                }
                self.state.body_visible_width = self.state.body_visible_width.saturating_sub(6); // gutter(4) + borders(2)
                self.state.resp_visible_width = self.state.resp_visible_width.saturating_sub(6);
            }

            terminal.draw(|frame| {
                ui::layout::render(frame, &self.state);
            })?;

            // Set terminal cursor shape: Bar for insert, Block for normal/visual
            crossterm::execute!(
                std::io::stdout(),
                if self.state.mode == InputMode::Insert {
                    SetCursorStyle::SteadyBar
                } else {
                    SetCursorStyle::SteadyBlock
                }
            )?;

            tokio::select! {
                event = events.next() => {
                    match event? {
                        AppEvent::Key(key) => {
                            if let Some(action) = keybindings::map_key(key, &self.state) {
                                self.action_tx.send(action)?;
                            }
                        }
                        AppEvent::Tick => {
                            self.action_tx.send(Action::Tick)?;
                        }
                        AppEvent::Resize(_, _) => {}
                    }
                }
                Some(action) = self.action_rx.recv() => {
                    self.update(action).await?;
                }
            }

            if self.state.should_quit {
                events.stop();
                break;
            }
        }
        Ok(())
    }

    async fn update(&mut self, action: Action) -> Result<()> {
        // Extract count prefix (consume it for all actions except AccumulateCount itself)
        let count = match action {
            Action::AccumulateCount(_) => 1,
            _ => self.state.count_prefix.take().unwrap_or(1) as usize,
        };

        match action {
            Action::Quit => self.state.should_quit = true,
            Action::Tick => {
                // Update spinner + elapsed time for in-flight requests
                if let Some(started) = self.state.request_started_at {
                    let elapsed = started.elapsed();
                    let spinner = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
                    let idx = (elapsed.as_millis() / 100) as usize % spinner.len();
                    self.state.set_status(format!(
                        "{} Sending request... {:.1}s (Esc to cancel)",
                        spinner[idx],
                        elapsed.as_secs_f64()
                    ));
                } else if let Some((_, instant)) = &self.state.status_message {
                    if instant.elapsed() > STATUS_MESSAGE_TTL {
                        self.state.status_message = None;
                    }
                }
                if let Some((_, instant)) = self.state.pending_key {
                    if instant.elapsed() > PENDING_KEY_TIMEOUT {
                        self.state.pending_key = None;
                    }
                }
            }

            // === Panel Navigation ===
            Action::NavigatePanel(direction) => {
                if self.state.active_panel == Panel::Request || self.state.active_panel == Panel::Body {
                    self.state.last_middle_panel = self.state.active_panel;
                }
                let target = self.state.active_panel.navigate(direction, self.state.is_wide_layout, self.state.last_middle_panel);
                self.state.active_panel = target;
                self.state.mode = InputMode::Normal;
                self.state.pending_key = None;
                self.state.request_field_editing = false;
                self.state.chain_autocomplete = None;
                self.state.type_sub_focus = crate::state::TypeSubFocus::Editor;
            }
            Action::FocusPanel(panel) => {
                self.state.active_panel = panel;
                self.state.mode = InputMode::Normal;
                self.state.request_field_editing = false;
                self.state.chain_autocomplete = None;
                self.state.type_sub_focus = crate::state::TypeSubFocus::Editor;
            }

            // === Vim Mode Transitions ===
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

            // === Inline Autocomplete ===
            Action::AutocompleteNext => {
                if let Some(ref mut ac) = self.state.chain_autocomplete {
                    ac.next();
                } else if let Some(ref mut ac) = self.state.autocomplete {
                    ac.next();
                } else {
                    // Open autocomplete if editing header name
                    self.try_open_autocomplete();
                }
            }
            Action::AutocompletePrev => {
                if let Some(ref mut ac) = self.state.chain_autocomplete {
                    ac.prev();
                } else if let Some(ref mut ac) = self.state.autocomplete {
                    ac.prev();
                } else {
                    self.try_open_autocomplete();
                }
            }
            Action::AutocompleteAccept => {
                if let Some(ac) = self.state.chain_autocomplete.take() {
                    if let Some(text) = ac.accept() {
                        if !text.is_empty() {
                            for c in text.chars() {
                                self.inline_input(c);
                            }
                        }
                    }
                    // Re-trigger chain autocomplete after insertion (e.g., after inserting request name, user may want to type '.')
                    self.try_chain_autocomplete();
                } else if let Some(ac) = self.state.autocomplete.take() {
                    if let Some((name, value)) = ac.accept() {
                        if self.state.active_panel == Panel::Request {
                            if let RequestFocus::Header(idx) = self.state.request_focus {
                                if let Some(h) = self.state.current_request.headers.get_mut(idx) {
                                    h.name = name.to_string();
                                    h.value = value.to_string();
                                    // Jump to value field
                                    self.state.header_edit_field = 1;
                                    self.state.header_edit_cursor = value.len();
                                }
                            }
                        }
                    }
                }
            }

            // === Pending Key ===
            Action::PendingKey(c) => {
                self.state.pending_key = Some((c, std::time::Instant::now()));
            }

            // === Scrolling ===
            Action::ScrollDown => { for _ in 0..count { self.scroll_down(); } }
            Action::ScrollUp => { for _ in 0..count { self.scroll_up(); } }
            Action::ScrollHalfDown => self.scroll_half_down(),
            Action::ScrollHalfUp => self.scroll_half_up(),
            Action::ScrollTop => self.scroll_top(),
            Action::ScrollBottom => self.scroll_bottom(),

            // === Request Panel Focus ===
            Action::RequestFocusDown => {
                match self.state.request_tab {
                    RequestTab::Headers => {
                        let hc = self.state.current_request.headers.len();
                        self.state.request_focus = match self.state.request_focus {
                            RequestFocus::Url => if hc > 0 { RequestFocus::Header(0) } else { RequestFocus::Url },
                            RequestFocus::Header(i) => if i + 1 < hc { RequestFocus::Header(i + 1) } else { RequestFocus::Header(i) },
                            _ => self.state.request_focus,
                        };
                    }
                    RequestTab::Queries => {
                        let pc = self.state.current_request.query_params.len();
                        self.state.request_focus = match self.state.request_focus {
                            RequestFocus::Url => if pc > 0 { RequestFocus::Param(0) } else { RequestFocus::Url },
                            RequestFocus::Param(i) => if i + 1 < pc { RequestFocus::Param(i + 1) } else { RequestFocus::Param(i) },
                            _ => self.state.request_focus,
                        };
                    }
                    RequestTab::Cookies => {
                        let cc = self.state.current_request.cookies.len();
                        self.state.request_focus = match self.state.request_focus {
                            RequestFocus::Url => if cc > 0 { RequestFocus::Cookie(0) } else { RequestFocus::Url },
                            RequestFocus::Cookie(i) => if i + 1 < cc { RequestFocus::Cookie(i + 1) } else { RequestFocus::Cookie(i) },
                            _ => self.state.request_focus,
                        };
                    }
                    RequestTab::Params => {
                        let pc = self.state.current_request.path_params.len();
                        self.state.request_focus = match self.state.request_focus {
                            RequestFocus::Url => if pc > 0 { RequestFocus::PathParam(0) } else { RequestFocus::Url },
                            RequestFocus::PathParam(i) => if i + 1 < pc { RequestFocus::PathParam(i + 1) } else { RequestFocus::PathParam(i) },
                            _ => self.state.request_focus,
                        };
                    }
                }
            }
            Action::RequestFocusUp => {
                self.state.request_focus = match self.state.request_focus {
                    RequestFocus::Url => RequestFocus::Url,
                    RequestFocus::Header(0) => RequestFocus::Url,
                    RequestFocus::Header(i) => RequestFocus::Header(i - 1),
                    RequestFocus::Param(0) => RequestFocus::Url,
                    RequestFocus::Param(i) => RequestFocus::Param(i - 1),
                    RequestFocus::Cookie(0) => RequestFocus::Url,
                    RequestFocus::Cookie(i) => RequestFocus::Cookie(i - 1),
                    RequestFocus::PathParam(0) => RequestFocus::Url,
                    RequestFocus::PathParam(i) => RequestFocus::PathParam(i - 1),
                };
            }
            Action::RequestNextTab => {
                self.state.request_tab = self.state.request_tab.next();
                self.state.request_focus = RequestFocus::Url;
            }
            Action::RequestPrevTab => {
                self.state.request_tab = self.state.request_tab.prev();
                self.state.request_focus = RequestFocus::Url;
            }
            Action::ToggleItemEnabled => {
                match self.state.request_focus {
                    RequestFocus::Header(idx) => {
                        if let Some(h) = self.state.current_request.headers.get_mut(idx) {
                            h.enabled = !h.enabled;
                        }
                    }
                    RequestFocus::Param(idx) => {
                        if let Some(p) = self.state.current_request.query_params.get_mut(idx) {
                            p.enabled = !p.enabled;
                        }
                    }
                    RequestFocus::Cookie(idx) => {
                        if let Some(c) = self.state.current_request.cookies.get_mut(idx) {
                            c.enabled = !c.enabled;
                        }
                    }
                    RequestFocus::PathParam(idx) => {
                        if let Some(p) = self.state.current_request.path_params.get_mut(idx) {
                            p.enabled = !p.enabled;
                        }
                    }
                    _ => {}
                }
            }
            Action::AddHeader => {
                self.state.current_request.headers.push(Header { name: String::new(), value: String::new(), enabled: true });
                let idx = self.state.current_request.headers.len() - 1;
                self.state.request_focus = RequestFocus::Header(idx);
                self.state.header_edit_field = 0;
                self.state.header_edit_cursor = 0;
                self.state.mode = InputMode::Insert;
            }
            Action::AddParam => {
                self.state.current_request.query_params.push(QueryParam { key: String::new(), value: String::new(), enabled: true });
                let idx = self.state.current_request.query_params.len() - 1;
                self.state.request_focus = RequestFocus::Param(idx);
                self.state.param_edit_field = 0;
                self.state.param_edit_cursor = 0;
                self.state.mode = InputMode::Insert;
            }
            Action::DeleteHeader => {
                self.state.pending_key = None;
                if let RequestFocus::Header(idx) = self.state.request_focus {
                    if idx < self.state.current_request.headers.len() {
                        self.state.current_request.headers.remove(idx);
                        self.state.request_focus = if self.state.current_request.headers.is_empty() {
                            RequestFocus::Url
                        } else {
                            RequestFocus::Header(idx.min(self.state.current_request.headers.len() - 1))
                        };
                        self.state.set_status("Header deleted");
                    }
                }
            }
            Action::DeleteParam => {
                self.state.pending_key = None;
                if let RequestFocus::Param(idx) = self.state.request_focus {
                    if idx < self.state.current_request.query_params.len() {
                        self.state.current_request.query_params.remove(idx);
                        self.state.request_focus = if self.state.current_request.query_params.is_empty() {
                            RequestFocus::Url
                        } else {
                            RequestFocus::Param(idx.min(self.state.current_request.query_params.len() - 1))
                        };
                        self.state.set_status("Param deleted");
                    }
                }
            }
            Action::AddCookie => {
                self.state.current_request.cookies.push(crate::model::request::Cookie { name: String::new(), value: String::new(), enabled: true });
                let idx = self.state.current_request.cookies.len() - 1;
                self.state.request_focus = RequestFocus::Cookie(idx);
                self.state.cookie_edit_field = 0;
                self.state.cookie_edit_cursor = 0;
                self.state.mode = InputMode::Insert;
            }
            Action::DeleteCookie => {
                self.state.pending_key = None;
                if let RequestFocus::Cookie(idx) = self.state.request_focus {
                    if idx < self.state.current_request.cookies.len() {
                        self.state.current_request.cookies.remove(idx);
                        self.state.request_focus = if self.state.current_request.cookies.is_empty() {
                            RequestFocus::Url
                        } else {
                            RequestFocus::Cookie(idx.min(self.state.current_request.cookies.len() - 1))
                        };
                        self.state.set_status("Cookie deleted");
                    }
                }
            }
            Action::AddPathParam => {
                self.state.current_request.path_params.push(PathParam { key: String::new(), value: String::new(), enabled: true });
                let idx = self.state.current_request.path_params.len() - 1;
                self.state.request_focus = RequestFocus::PathParam(idx);
                self.state.path_param_edit_field = 0;
                self.state.path_param_edit_cursor = 0;
                self.state.mode = InputMode::Insert;
            }
            Action::DeletePathParam => {
                self.state.pending_key = None;
                if let RequestFocus::PathParam(idx) = self.state.request_focus {
                    if idx < self.state.current_request.path_params.len() {
                        self.state.current_request.path_params.remove(idx);
                        self.state.request_focus = if self.state.current_request.path_params.is_empty() {
                            RequestFocus::Url
                        } else {
                            RequestFocus::PathParam(idx.min(self.state.current_request.path_params.len() - 1))
                        };
                        self.state.set_status("Path param deleted");
                    }
                }
            }
            Action::ShowHeaderAutocomplete => {
                let suggestions: Vec<(String, String)> = COMMON_HEADERS.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
                self.state.overlay = Some(Overlay::HeaderAutocomplete { suggestions, selected: 0 });
            }

            // === Collections ===
            Action::SelectRequest => {
                if let Some(flat_idx) = self.state.collections_state.selected() {
                    self.select_request_by_flat_index(flat_idx);
                    self.state.active_panel = Panel::Request;
                    self.state.request_focus = RequestFocus::Url;
                }
            }
            Action::CreateCollection => {
                self.state.overlay = Some(Overlay::NewCollection { name: String::new() });
            }
            Action::NextCollection => {
                if !self.state.collections.is_empty() {
                    self.state.active_collection = (self.state.active_collection + 1) % self.state.collections.len();
                    self.switch_active_collection();
                }
            }
            Action::PrevCollection => {
                if !self.state.collections.is_empty() {
                    self.state.active_collection = if self.state.active_collection == 0 { self.state.collections.len() - 1 } else { self.state.active_collection - 1 };
                    self.switch_active_collection();
                }
            }

            // === Collection CRUD ===
            Action::SaveRequest => {
                self.save_current_request_over_selected();
            }
            Action::SaveRequestAs => {
                self.save_current_request_as_new();
            }
            Action::RenameRequest => {
                if let Some(flat_idx) = self.state.collections_state.selected() {
                    let current_name = match self.flat_idx_to_coll_req(flat_idx) {
                        Some((ci, None)) => self.state.collections.get(ci).map(|c| c.name.clone()).unwrap_or_default(),
                        Some((ci, Some(ri))) => self.state.collections.get(ci)
                            .and_then(|c| c.requests.get(ri))
                            .and_then(|r| r.name.clone())
                            .unwrap_or_default(),
                        None => String::new(),
                    };
                    self.state.overlay = Some(Overlay::RenameRequest { name: current_name });
                }
            }
            Action::DeleteSelected => {
                if let Some(flat_idx) = self.state.collections_state.selected() {
                    let msg = match self.flat_idx_to_coll_req(flat_idx) {
                        Some((ci, None)) => {
                            let name = self.state.collections.get(ci).map(|c| c.name.clone()).unwrap_or_default();
                            format!("Delete collection '{}'? (y/Enter, Esc/n to cancel)", name)
                        }
                        Some((ci, Some(ri))) => {
                            let name = self.state.collections.get(ci)
                                .and_then(|c| c.requests.get(ri))
                                .map(|r| r.display_name()).unwrap_or_default();
                            format!("Delete request '{}'? (y/Enter, Esc/n to cancel)", name)
                        }
                        None => return Ok(()),
                    };
                    self.state.overlay = Some(Overlay::ConfirmDelete { message: msg });
                }
            }
            Action::MoveRequest => {
                if let Some(flat_idx) = self.state.collections_state.selected() {
                    if let Some((_, Some(_))) = self.flat_idx_to_coll_req(flat_idx) {
                        if self.state.collections.len() > 1 {
                            self.state.overlay = Some(Overlay::MoveRequest { selected: 0 });
                        }
                    } else {
                        self.state.set_status("Select a request first (not a collection header)");
                    }
                }
            }
            Action::NewEmptyRequest => {
                self.state.current_request = Request::default();
                self.state.current_response = None;
                self.state.last_error = None;
                self.state.body_vim.set_content("");
                self.state.request_focus = RequestFocus::Url;
                self.state.set_status("New empty request");
            }
            Action::AddRequestToCollection => {
                // Determine which collection is selected
                let ci = if let Some(flat_idx) = self.state.collections_state.selected() {
                    match self.flat_idx_to_coll_req(flat_idx) {
                        Some((ci, _)) => Some(ci),
                        None => None,
                    }
                } else {
                    None
                };

                if let Some(ci) = ci {
                    // Generate unique name
                    let existing_names: Vec<String> = self.state.collections[ci]
                        .requests.iter()
                        .filter_map(|r| r.name.clone())
                        .collect();
                    let mut name = "New Request".to_string();
                    let mut counter = 2;
                    while existing_names.contains(&name) {
                        name = format!("New Request ({})", counter);
                        counter += 1;
                    }

                    let mut new_req = Request::default();
                    new_req.name = Some(name.clone());

                    // Add to collection and persist
                    self.state.collections[ci].requests.push(new_req.clone());
                    self.persist_collection(ci);
                    self.state.active_collection = ci;

                    // Load into editor
                    self.state.current_request = new_req;
                    self.state.current_response = None;
                    self.state.last_error = None;
                    self.state.body_vim.set_content("");
                    self.state.request_focus = RequestFocus::Url;

                    // Expand and select the new request
                    self.state.expanded_collections.insert(ci);
                    self.rebuild_collection_items();

                    let coll_name = self.state.collections[ci].name.clone();
                    self.state.set_status(format!("Created: {} in {}", name, coll_name));
                } else {
                    self.state.set_status("Select a collection first");
                }
            }
            Action::ToggleCollapse => {
                if let Some(flat_idx) = self.state.collections_state.selected() {
                    if let Some((ci, None)) = self.flat_idx_to_coll_req(flat_idx) {
                        if self.state.expanded_collections.contains(&ci) {
                            self.state.expanded_collections.remove(&ci);
                        } else {
                            self.state.expanded_collections.insert(ci);
                        }
                        self.rebuild_collection_items();
                    }
                }
            }
            Action::YankRequest => {
                self.state.pending_key = None;
                if let Some(flat_idx) = self.state.collections_state.selected() {
                    if let Some((ci, Some(ri))) = self.flat_idx_to_coll_req(flat_idx) {
                        if let Some(req) = self.state.collections.get(ci).and_then(|c| c.requests.get(ri)) {
                            self.state.yanked_request = Some(req.clone());
                            self.state.set_status(format!("Yanked: {}", req.display_name()));
                        }
                    } else {
                        self.state.set_status("Place cursor on a request to yank");
                    }
                }
            }
            Action::PasteRequest => {
                if let Some(req) = self.state.yanked_request.clone() {
                    // Paste into the collection where cursor is
                    if let Some(flat_idx) = self.state.collections_state.selected() {
                        let target_ci = self.flat_idx_to_coll_req(flat_idx)
                            .map(|(ci, _)| ci)
                            .unwrap_or(self.state.active_collection);
                        if let Some(coll) = self.state.collections.get_mut(target_ci) {
                            let mut new_req = req;
                            let name = new_req.name.as_deref().unwrap_or("Untitled");
                            new_req.name = Some(format!("{} (copy)", name));
                            coll.requests.push(new_req);
                            self.persist_collection(target_ci);
                            // Expand target if collapsed
                            self.state.expanded_collections.insert(target_ci);
                            self.rebuild_collection_items();
                            self.state.set_status("Request pasted");
                        }
                    }
                } else {
                    self.state.set_status("Nothing to paste (yy on a request first)");
                }
            }

            // === Inline Text Editing ===
            Action::InlineInput(c) => {
                self.inline_input(c);
                self.try_chain_autocomplete();
            }
            Action::InlineBackspace => {
                self.inline_backspace();
                self.try_chain_autocomplete();
            }
            Action::InlineDelete => self.inline_delete(),
            Action::InlineNewline => self.inline_newline(),
            Action::InlineCursorLeft => { for _ in 0..count { self.inline_cursor_left(); } }
            Action::InlineCursorRight => { for _ in 0..count { self.inline_cursor_right(); } }
            Action::InlineCursorUp => match self.state.active_panel {
                Panel::Response if self.state.response_tab == ResponseTab::Type && self.state.type_sub_focus == crate::state::TypeSubFocus::Editor => self.type_cursor_up(),
                Panel::Response => self.resp_cursor_up(),
                _ => self.body_cursor_up(),
            },
            Action::InlineCursorDown => match self.state.active_panel {
                Panel::Response if self.state.response_tab == ResponseTab::Type && self.state.type_sub_focus == crate::state::TypeSubFocus::Editor => self.type_cursor_down(),
                Panel::Response => self.resp_cursor_down(),
                _ => self.body_cursor_down(),
            },
            Action::InlineCursorHome => self.inline_cursor_home(),
            Action::InlineCursorEnd => self.inline_cursor_end(),
            Action::InlineTab => self.inline_tab(),

            // === Body/Request/Type Vim Motions ===
            Action::BodyWordForward => {
                for _ in 0..count {
                    if self.state.active_panel == Panel::Request && self.state.request_field_editing {
                        self.request_word_forward();
                    } else if self.state.active_panel == Panel::Response && self.state.response_tab == ResponseTab::Type && self.state.type_sub_focus == crate::state::TypeSubFocus::Editor {
                        vim_editor_word_forward(&mut self.state.type_vim);
                        self.state.type_vim.ensure_cursor_visible();
                    } else {
                        self.body_word_forward();
                    }
                }
                // Sync hscroll after word motion
                match self.state.active_panel {
                    Panel::Body => { self.sync_body_hscroll(); self.sync_body_scroll(); }
                    Panel::Response => { self.sync_resp_hscroll(); self.sync_resp_scroll(); }
                    _ => {}
                }
            }
            Action::BodyWordBackward => {
                for _ in 0..count {
                    if self.state.active_panel == Panel::Request && self.state.request_field_editing {
                        self.request_word_backward();
                    } else if self.state.active_panel == Panel::Response && self.state.response_tab == ResponseTab::Type && self.state.type_sub_focus == crate::state::TypeSubFocus::Editor {
                        vim_editor_word_backward(&mut self.state.type_vim);
                        self.state.type_vim.ensure_cursor_visible();
                    } else {
                        self.body_word_backward();
                    }
                }
                match self.state.active_panel {
                    Panel::Body => { self.sync_body_hscroll(); self.sync_body_scroll(); }
                    Panel::Response => { self.sync_resp_hscroll(); self.sync_resp_scroll(); }
                    _ => {}
                }
            }
            Action::BodyWordEnd => {
                for _ in 0..count {
                    if self.state.active_panel == Panel::Request && self.state.request_field_editing {
                        self.request_word_end();
                    } else if self.state.active_panel == Panel::Response && self.state.response_tab == ResponseTab::Type && self.state.type_sub_focus == crate::state::TypeSubFocus::Editor {
                        vim_editor_word_end(&mut self.state.type_vim);
                        self.state.type_vim.ensure_cursor_visible();
                    } else {
                        self.body_word_end();
                    }
                }
                match self.state.active_panel {
                    Panel::Body => { self.sync_body_hscroll(); self.sync_body_scroll(); }
                    Panel::Response => { self.sync_resp_hscroll(); self.sync_resp_scroll(); }
                    _ => {}
                }
            }
            Action::BodyLineHome => {
                if self.state.active_panel == Panel::Request && self.state.request_field_editing {
                    self.set_request_cursor(0);
                } else if self.state.active_panel == Panel::Response && self.state.response_tab == ResponseTab::Type && self.state.type_sub_focus == crate::state::TypeSubFocus::Editor {
                    self.state.type_vim.cursor_col = 0;
                } else if self.state.active_panel == Panel::Response {
                    self.state.resp_vim.cursor_col = 0;
                    self.sync_resp_hscroll();
                } else {
                    self.state.body_vim.cursor_col = 0;
                    self.sync_body_hscroll();
                }
            }
            Action::BodyLineEnd => self.inline_cursor_end(),

            // === Visual Mode ===
            Action::VisualYank => {
                let is_block = self.state.mode == InputMode::VisualBlock;
                let text = match self.state.active_panel {
                    Panel::Body if is_block => Some(self.get_block_selection()),
                    Panel::Body => Some(self.get_visual_selection()),
                    Panel::Response if self.state.response_tab == ResponseTab::Type && self.state.type_sub_focus == crate::state::TypeSubFocus::Editor => {
                        Some(self.state.type_vim.selected_text().unwrap_or_default())
                    }
                    Panel::Response if is_block => Some(self.get_response_block_selection()),
                    Panel::Response => Some(self.get_response_visual_selection()),
                    Panel::Request if self.state.request_field_editing => Some(self.get_request_visual_selection()),
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
                    Panel::Response if self.state.response_tab == ResponseTab::Type && self.state.type_sub_focus == crate::state::TypeSubFocus::Editor => {
                        let text = self.state.type_vim.selected_text().unwrap_or_default();
                        self.state.yank_buffer = text;
                        let _ = crate::clipboard::copy_to_clipboard(&self.state.yank_buffer);
                        self.state.type_vim.visual_delete();
                        self.sync_type_vim_text();
                        self.state.response_type_locked = true;
                        self.state.mode = InputMode::Normal;
                    }
                    Panel::Request if self.state.request_field_editing => {
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
                        Panel::Response if self.state.response_tab == ResponseTab::Type && self.state.type_sub_focus == crate::state::TypeSubFocus::Editor => {
                            self.state.type_vim.visual_delete();
                            self.state.type_vim.unnamed_register = Register { content: paste.clone(), linewise: paste.ends_with('\n') };
                            self.state.type_vim.paste_after();
                            self.sync_type_vim_text();
                            self.state.response_type_locked = true;
                            self.state.mode = InputMode::Normal;
                        }
                        Panel::Request if self.state.request_field_editing => {
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
                if self.state.active_panel == Panel::Response && self.state.response_tab == ResponseTab::Type && self.state.type_sub_focus == crate::state::TypeSubFocus::Editor {
                    self.state.type_vim.save_undo();
                    self.state.type_vim.unnamed_register = Register { content: paste.clone(), linewise: paste.ends_with('\n') };
                    self.state.type_vim.paste_after();
                    self.sync_type_vim_text();
                    self.state.response_type_locked = true;
                } else if self.state.active_panel == Panel::Body {
                    self.push_body_undo();
                    self.paste_text_at_cursor(&paste);
                } else if self.state.active_panel == Panel::Request && self.state.request_field_editing {
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
                    } else if self.state.active_panel == Panel::Response && self.state.response_tab == ResponseTab::Type && self.state.type_sub_focus == crate::state::TypeSubFocus::Editor {
                        self.state.type_vim.save_undo();
                        self.state.type_vim.unnamed_register = Register { content: text.clone(), linewise: text.ends_with('\n') };
                        self.state.type_vim.paste_after();
                        self.sync_type_vim_text();
                        self.state.response_type_locked = true;
                        self.state.set_status("Pasted from clipboard");
                    }
                }
            }
            Action::YankLine => {
                self.state.pending_key = None;
                match self.state.active_panel {
                    Panel::Request if self.state.request_field_editing => {
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
                    Panel::Response if self.state.response_tab == ResponseTab::Type && self.state.type_sub_focus == crate::state::TypeSubFocus::Editor => {
                        let lines: Vec<&str> = self.state.response_type_text.lines().collect();
                        let row = self.state.type_vim.cursor_row;
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
                        let row = self.state.resp_vim.cursor_row;
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
                } else if self.state.active_panel == Panel::Response && self.state.response_tab == ResponseTab::Type && self.state.type_sub_focus == crate::state::TypeSubFocus::Editor {
                    self.state.type_vim.save_undo();
                    let row = self.state.type_vim.cursor_row;
                    let yanked = self.state.type_vim.delete_line(row).unwrap_or_default();
                    self.sync_type_vim_text();
                    self.state.yank_buffer = format!("{}\n", yanked);
                    let _ = crate::clipboard::copy_to_clipboard(&self.state.yank_buffer);
                    self.state.response_type_locked = true;
                    self.state.type_vim.ensure_cursor_visible();
                    self.state.set_status("Line deleted");
                } else if self.state.active_panel == Panel::Request && self.state.request_field_editing {
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
                if self.state.active_panel == Panel::Response && self.state.response_tab == ResponseTab::Type && self.state.type_sub_focus == crate::state::TypeSubFocus::Editor {
                    self.state.type_vim.save_undo();
                    self.state.type_vim.delete_char_at_cursor();
                    self.sync_type_vim_text();
                    self.state.response_type_locked = true;
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
                } else if self.state.active_panel == Panel::Request && self.state.request_field_editing {
                    self.push_request_undo();
                    self.delete_request_char_under_cursor();
                }
            }
            Action::ReplaceChar(c) => {
                self.state.pending_key = None;
                if self.state.active_panel == Panel::Response && self.state.response_tab == ResponseTab::Type && self.state.type_sub_focus == crate::state::TypeSubFocus::Editor {
                    self.state.type_vim.save_undo();
                    {
                        let row = self.state.type_vim.cursor_row;
                        let col = self.state.type_vim.cursor_col;
                        if row < self.state.type_vim.lines.len() && col < self.state.type_vim.lines[row].len() {
                            self.state.type_vim.lines[row].remove(col);
                            self.state.type_vim.lines[row].insert(col, c);
                            self.state.type_vim.modified = true;
                            self.sync_type_vim_text();
                        }
                    }
                    self.state.response_type_locked = true;
                } else if self.state.active_panel == Panel::Body {
                    self.push_body_undo();
                    let body = self.state.current_request.get_body_mut(self.state.body_type);
                    let pos = row_col_to_offset(body, self.state.body_vim.cursor_row, self.state.body_vim.cursor_col);
                    if pos < body.len() && body.as_bytes()[pos] != b'\n' {
                        body.remove(pos);
                        body.insert(pos, c);
                    }
                } else if self.state.active_panel == Panel::Request && self.state.request_field_editing {
                    self.push_request_undo();
                    let cursor = self.get_request_cursor();
                    let len = self.get_request_field_len();
                    if cursor < len {
                        self.replace_request_char_at(cursor, c);
                    }
                }
            }
            Action::ChangeLine => {
                self.state.pending_key = None;
                if self.state.active_panel == Panel::Response && self.state.response_tab == ResponseTab::Type && self.state.type_sub_focus == crate::state::TypeSubFocus::Editor {
                    self.state.type_vim.save_undo();
                    let row = self.state.type_vim.cursor_row;
                    let yanked = if row < self.state.type_vim.lines.len() {
                        let line = std::mem::take(&mut self.state.type_vim.lines[row]);
                        self.state.type_vim.cursor_col = 0;
                        self.state.type_vim.modified = true;
                        line
                    } else { String::new() };
                    self.sync_type_vim_text();
                    self.state.yank_buffer = yanked;
                    let _ = crate::clipboard::copy_to_clipboard(&self.state.yank_buffer);
                    self.state.response_type_locked = true;
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
                } else if self.state.active_panel == Panel::Request && self.state.request_field_editing {
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
                if self.state.active_panel == Panel::Response && self.state.response_tab == ResponseTab::Type && self.state.type_sub_focus == crate::state::TypeSubFocus::Editor {
                    self.state.type_vim.save_undo();
                    let lines: Vec<&str> = self.state.response_type_text.lines().collect();
                    let row = self.state.type_vim.cursor_row;
                    let col = self.state.type_vim.cursor_col;
                    if let Some(line) = lines.get(row) {
                        let end_col = crate::vim_buffer::word_end_forward(line.as_bytes(), col);
                        let deleted = &line[col..end_col];
                        self.state.yank_buffer = deleted.to_string();
                        let _ = crate::clipboard::copy_to_clipboard(&self.state.yank_buffer);
                        let start = vim_row_col_to_offset(&self.state.response_type_text, row, col);
                        let end = vim_row_col_to_offset(&self.state.response_type_text, row, end_col);
                        self.state.response_type_text.drain(start..end);
                    }
                    self.state.response_type_locked = true;
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
                } else if self.state.active_panel == Panel::Request && self.state.request_field_editing {
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
                if self.state.active_panel == Panel::Response && self.state.response_tab == ResponseTab::Type && self.state.type_sub_focus == crate::state::TypeSubFocus::Editor {
                    self.state.type_vim.save_undo();
                    let lines: Vec<&str> = self.state.response_type_text.lines().collect();
                    let row = self.state.type_vim.cursor_row;
                    let col = self.state.type_vim.cursor_col;
                    if let Some(line) = lines.get(row) {
                        let start_col = crate::vim_buffer::word_start_backward(line.as_bytes(), col);
                        let deleted = &line[start_col..col];
                        self.state.yank_buffer = deleted.to_string();
                        let _ = crate::clipboard::copy_to_clipboard(&self.state.yank_buffer);
                        let start = vim_row_col_to_offset(&self.state.response_type_text, row, start_col);
                        let end = vim_row_col_to_offset(&self.state.response_type_text, row, col);
                        self.state.response_type_text.drain(start..end);
                        self.state.type_vim.cursor_col = start_col;
                    }
                    self.state.response_type_locked = true;
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
                } else if self.state.active_panel == Panel::Request && self.state.request_field_editing {
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
                if self.state.active_panel == Panel::Response && self.state.response_tab == ResponseTab::Type && self.state.type_sub_focus == crate::state::TypeSubFocus::Editor {
                    self.state.type_vim.save_undo();
                    let yanked = {
                        let row = self.state.type_vim.cursor_row;
                        let col = self.state.type_vim.cursor_col;
                        let line_len = self.state.type_vim.current_line_len();
                        let text = self.state.type_vim.delete_range(col, line_len, row);
                        self.sync_type_vim_text();
                        text
                    };
                    self.state.yank_buffer = yanked;
                    let _ = crate::clipboard::copy_to_clipboard(&self.state.yank_buffer);
                    self.state.response_type_locked = true;
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
                } else if self.state.active_panel == Panel::Request && self.state.request_field_editing {
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
            Action::Substitute => {
                if self.state.active_panel == Panel::Response && self.state.response_tab == ResponseTab::Type && self.state.type_sub_focus == crate::state::TypeSubFocus::Editor {
                    self.state.type_vim.save_undo();
                    self.state.type_vim.delete_char_at_cursor();
                    self.sync_type_vim_text();
                    self.state.response_type_locked = true;
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
                } else if self.state.active_panel == Panel::Request && self.state.request_field_editing {
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
            Action::DeleteWord => {
                self.state.pending_key = None;
                if self.state.active_panel == Panel::Response && self.state.response_tab == ResponseTab::Type && self.state.type_sub_focus == crate::state::TypeSubFocus::Editor {
                    self.state.type_vim.save_undo();
                    let lines: Vec<&str> = self.state.response_type_text.lines().collect();
                    let row = self.state.type_vim.cursor_row;
                    let col = self.state.type_vim.cursor_col;
                    if let Some(line) = lines.get(row) {
                        let end_col = crate::vim_buffer::word_end_forward(line.as_bytes(), col);
                        self.state.yank_buffer = line[col..end_col].to_string();
                        let _ = crate::clipboard::copy_to_clipboard(&self.state.yank_buffer);
                        let start = vim_row_col_to_offset(&self.state.response_type_text, row, col);
                        let end = vim_row_col_to_offset(&self.state.response_type_text, row, end_col);
                        self.state.response_type_text.drain(start..end);
                    }
                    self.state.response_type_locked = true;
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
                } else if self.state.active_panel == Panel::Request && self.state.request_field_editing {
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
                if self.state.active_panel == Panel::Response && self.state.response_tab == ResponseTab::Type && self.state.type_sub_focus == crate::state::TypeSubFocus::Editor {
                    self.state.type_vim.save_undo();
                    let lines: Vec<&str> = self.state.response_type_text.lines().collect();
                    let row = self.state.type_vim.cursor_row;
                    let col = self.state.type_vim.cursor_col;
                    if let Some(line) = lines.get(row) {
                        let end_col = crate::vim_buffer::word_end_forward(line.as_bytes(), col);
                        self.state.yank_buffer = line[col..end_col].to_string();
                        let _ = crate::clipboard::copy_to_clipboard(&self.state.yank_buffer);
                        let start = vim_row_col_to_offset(&self.state.response_type_text, row, col);
                        let end = vim_row_col_to_offset(&self.state.response_type_text, row, end_col);
                        self.state.response_type_text.drain(start..end);
                    }
                    self.state.response_type_locked = true;
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
                } else if self.state.active_panel == Panel::Request && self.state.request_field_editing {
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
                if self.state.active_panel == Panel::Response && self.state.response_tab == ResponseTab::Type && self.state.type_sub_focus == crate::state::TypeSubFocus::Editor {
                    self.state.type_vim.save_undo();
                    let lines: Vec<&str> = self.state.response_type_text.lines().collect();
                    let row = self.state.type_vim.cursor_row;
                    let col = self.state.type_vim.cursor_col;
                    if let Some(line) = lines.get(row) {
                        let start_col = crate::vim_buffer::word_start_backward(line.as_bytes(), col);
                        self.state.yank_buffer = line[start_col..col].to_string();
                        let _ = crate::clipboard::copy_to_clipboard(&self.state.yank_buffer);
                        let start = vim_row_col_to_offset(&self.state.response_type_text, row, start_col);
                        let end = vim_row_col_to_offset(&self.state.response_type_text, row, col);
                        self.state.response_type_text.drain(start..end);
                        self.state.type_vim.cursor_col = start_col;
                    }
                    self.state.response_type_locked = true;
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
                } else if self.state.active_panel == Panel::Request && self.state.request_field_editing {
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
            Action::YankWord => {
                self.state.pending_key = None;
                if self.state.active_panel == Panel::Response && self.state.response_tab == ResponseTab::Type && self.state.type_sub_focus == crate::state::TypeSubFocus::Editor {
                    let lines: Vec<&str> = self.state.response_type_text.lines().collect();
                    let row = self.state.type_vim.cursor_row;
                    let col = self.state.type_vim.cursor_col;
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
                } else if self.state.active_panel == Panel::Request && self.state.request_field_editing {
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
            Action::Undo => {
                if self.state.active_panel == Panel::Response && self.state.response_tab == ResponseTab::Type && self.state.type_sub_focus == crate::state::TypeSubFocus::Editor {
                    if !self.state.type_vim.undo_stack.is_empty() {
                        self.state.type_vim.undo();
                        self.sync_type_vim_text();
                        self.state.response_type_locked = true;
                        self.state.set_status("Undo");
                    } else {
                        self.state.set_status("Already at oldest change");
                    }
                } else if self.state.active_panel == Panel::Body {
                    if let Some(snapshot) = self.state.body_vim.undo_stack.pop() {
                        let current_body = self.active_body().to_string();
                        let cur_lines: Vec<String> = if current_body.is_empty() { vec![String::new()] } else { current_body.lines().map(String::from).collect() };
                        self.state.body_vim.redo_stack.push(vimltui::Snapshot {
                            lines: cur_lines,
                            cursor_row: self.state.body_vim.cursor_row,
                            cursor_col: self.state.body_vim.cursor_col,
                        });
                        let restored = snapshot.lines.join("\n");
                        self.set_active_body(if restored.is_empty() { None } else { Some(restored) });
                        self.state.body_vim.cursor_row = snapshot.cursor_row;
                        self.state.body_vim.cursor_col = snapshot.cursor_col;
                        self.state.set_status("Undo");
                    } else {
                        self.state.set_status("Already at oldest change");
                    }
                } else if self.state.active_panel == Panel::Request && self.state.request_field_editing {
                    if let Some((focus, edit_field, text, cursor)) = self.state.request_undo_stack.pop() {
                        // Push current state to redo
                        let cur_text = self.get_request_field_text();
                        let cur_cursor = self.get_request_cursor();
                        let cur_focus = self.state.request_focus;
                        let cur_ef = match cur_focus {
                            RequestFocus::Header(_) => self.state.header_edit_field,
                            RequestFocus::Param(_) => self.state.param_edit_field,
                            RequestFocus::Cookie(_) => self.state.cookie_edit_field,
                            RequestFocus::PathParam(_) => self.state.path_param_edit_field,
                            RequestFocus::Url => 0,
                        };
                        self.state.request_redo_stack.push((cur_focus, cur_ef, cur_text, cur_cursor));
                        self.set_request_field_text(focus, edit_field, text);
                        self.state.request_focus = focus;
                        self.set_request_cursor(cursor);
                        self.state.set_status("Undo");
                    } else {
                        self.state.set_status("Already at oldest change");
                    }
                }
            }
            Action::Redo => {
                if self.state.active_panel == Panel::Response && self.state.response_tab == ResponseTab::Type && self.state.type_sub_focus == crate::state::TypeSubFocus::Editor {
                    if !self.state.type_vim.redo_stack.is_empty() {
                        self.state.type_vim.redo();
                        self.sync_type_vim_text();
                        self.state.response_type_locked = true;
                        self.state.set_status("Redo");
                    } else {
                        self.state.set_status("Already at newest change");
                    }
                } else if self.state.active_panel == Panel::Body {
                    if let Some(snapshot) = self.state.body_vim.redo_stack.pop() {
                        let current_body = self.active_body().to_string();
                        let cur_lines: Vec<String> = if current_body.is_empty() { vec![String::new()] } else { current_body.lines().map(String::from).collect() };
                        self.state.body_vim.undo_stack.push(vimltui::Snapshot {
                            lines: cur_lines,
                            cursor_row: self.state.body_vim.cursor_row,
                            cursor_col: self.state.body_vim.cursor_col,
                        });
                        let restored = snapshot.lines.join("\n");
                        self.set_active_body(if restored.is_empty() { None } else { Some(restored) });
                        self.state.body_vim.cursor_row = snapshot.cursor_row;
                        self.state.body_vim.cursor_col = snapshot.cursor_col;
                        self.state.set_status("Redo");
                    } else {
                        self.state.set_status("Already at newest change");
                    }
                } else if self.state.active_panel == Panel::Request && self.state.request_field_editing {
                    if let Some((focus, edit_field, text, cursor)) = self.state.request_redo_stack.pop() {
                        let cur_text = self.get_request_field_text();
                        let cur_cursor = self.get_request_cursor();
                        let cur_focus = self.state.request_focus;
                        let cur_ef = match cur_focus {
                            RequestFocus::Header(_) => self.state.header_edit_field,
                            RequestFocus::Param(_) => self.state.param_edit_field,
                            RequestFocus::Cookie(_) => self.state.cookie_edit_field,
                            RequestFocus::PathParam(_) => self.state.path_param_edit_field,
                            RequestFocus::Url => 0,
                        };
                        self.state.request_undo_stack.push((cur_focus, cur_ef, cur_text, cur_cursor));
                        self.set_request_field_text(focus, edit_field, text);
                        self.state.request_focus = focus;
                        self.set_request_cursor(cursor);
                        self.state.set_status("Redo");
                    } else {
                        self.state.set_status("Already at newest change");
                    }
                }
            }

            // === Method Cycling ===
            Action::NextMethod => {
                self.state.current_request.method = self.state.current_request.method.next();
                self.state.set_status(format!("Method: {}", self.state.current_request.method));
            }
            Action::PrevMethod => {
                self.state.current_request.method = self.state.current_request.method.prev();
                self.state.set_status(format!("Method: {}", self.state.current_request.method));
            }

            // === Theme ===
            Action::CycleTheme => {
                let next = crate::theme::next_theme_name(&self.state.theme.name);
                self.state.theme = crate::theme::load_theme(next);
                self.state.set_status(format!("Theme: {}", self.state.theme.name));
            }

            // === Body Type ===
            Action::CycleBodyType => {
                self.state.body_type = self.state.body_type.next();
                self.state.validate_body();
                self.state.set_status(format!("Body: {}", self.state.body_type.label()));
            }
            Action::BodyNextTab => {
                self.state.body_type = self.state.body_type.next();
                let body = self.state.current_request.get_body(self.state.body_type).to_string();
                self.state.body_vim.set_content(&body);
                self.state.validate_body();
            }
            Action::BodyPrevTab => {
                self.state.body_type = self.state.body_type.prev();
                let body = self.state.current_request.get_body(self.state.body_type).to_string();
                self.state.body_vim.set_content(&body);
                self.state.validate_body();
            }

            // === Body Vim Delegation ===
            Action::BodyVimInput(key) => {
                if self.state.active_panel == Panel::Body {
                    self.sync_body_to_vim();
                    // Sync app mode into body_vim before handling key
                    self.state.body_vim.mode = match self.state.mode {
                        InputMode::Normal => VimMode::Normal,
                        InputMode::Insert => VimMode::Insert,
                        InputMode::Visual => VimMode::Visual(VisualKind::Char),
                        InputMode::VisualBlock => VimMode::Visual(VisualKind::Block),
                    };
                    let action = self.state.body_vim.handle_key(key);
                    self.sync_vim_to_body();
                    self.sync_mode_from_vim();
                    self.state.validate_body();

                    match action {
                        vimltui::EditorAction::Save => {
                            self.state.set_status("Body saved");
                        }
                        _ => {}
                    }
                }
            }

            Action::RespVimInput(key) => {
                if self.state.active_panel == Panel::Response {
                    // Sync response body into resp_vim
                    let resp_text = self.get_response_body_text();
                    self.state.resp_vim.lines = if resp_text.is_empty() {
                        vec![String::new()]
                    } else {
                        resp_text.lines().map(String::from).collect()
                    };
                    self.state.resp_vim.mode = match self.state.mode {
                        InputMode::Normal => VimMode::Normal,
                        InputMode::Insert => VimMode::Insert,
                        InputMode::Visual => VimMode::Visual(VisualKind::Char),
                        InputMode::VisualBlock => VimMode::Visual(VisualKind::Block),
                    };
                    let _action = self.state.resp_vim.handle_key(key);
                    // Read-only: no text sync back
                    self.sync_mode_from_vim_resp();
                }
            }

            Action::TypeVimInput(key) => {
                if self.state.active_panel == Panel::Response {
                    // Sync type text into type_vim
                    let type_text = self.state.response_type_text.clone();
                    self.state.type_vim.lines = if type_text.is_empty() {
                        vec![String::new()]
                    } else {
                        type_text.lines().map(String::from).collect()
                    };
                    self.state.type_vim.mode = match self.state.mode {
                        InputMode::Normal => VimMode::Normal,
                        InputMode::Insert => VimMode::Insert,
                        InputMode::Visual => VimMode::Visual(VisualKind::Char),
                        InputMode::VisualBlock => VimMode::Visual(VisualKind::Block),
                    };
                    let _action = self.state.type_vim.handle_key(key);
                    // Sync text back
                    self.state.response_type_text = self.state.type_vim.content();
                    self.sync_mode_from_vim_type();
                }
            }

            // === Response Tabs ===
            Action::ResponseNextTab => {
                self.state.response_tab = self.state.response_tab.next();
                self.state.type_sub_focus = crate::state::TypeSubFocus::Editor;
            }
            Action::ResponsePrevTab => {
                self.state.response_tab = self.state.response_tab.prev();
                self.state.type_sub_focus = crate::state::TypeSubFocus::Editor;
            }
            Action::TypeSubFocusDown => {
                self.state.type_sub_focus = crate::state::TypeSubFocus::Preview;
                self.state.mode = InputMode::Normal;
            }
            Action::TypeSubFocusUp => {
                self.state.type_sub_focus = crate::state::TypeSubFocus::Editor;
                self.state.mode = InputMode::Normal;
            }
            Action::TypeLangNext => {
                self.swap_type_lang_out();
                self.state.type_lang = self.state.type_lang.next();
                self.swap_type_lang_in();
                self.state.mode = InputMode::Normal;
            }
            Action::TypeLangPrev => {
                self.swap_type_lang_out();
                self.state.type_lang = self.state.type_lang.prev();
                self.swap_type_lang_in();
                self.state.mode = InputMode::Normal;
            }
            Action::RegenerateType => {
                self.state.response_type_locked = false;
                if let Some(ref resp) = self.state.current_response {
                    if let Ok(json_val) = serde_json::from_str::<serde_json::Value>(&resp.body) {
                        let rt = crate::model::response_type::JsonType::infer(&json_val);
                        self.state.response_type_text = rt.to_display_lines(0).join("\n");
                        let type_name = self.state.current_request.name.as_deref()
                            .map(|n| { let mut c = n.chars(); match c.next() { None => String::new(), Some(f) => f.to_uppercase().to_string() + c.as_str() } })
                            .unwrap_or_else(|| "ResponseType".to_string());
                        self.state.type_ts_text = rt.to_typescript(&type_name);
                        self.state.type_csharp_text = rt.to_csharp(&type_name);
                        self.state.response_type = Some(rt);
                    }
                }
                self.state.type_validation_errors.clear();
                self.state.type_vim.cursor_row = 0;
                self.state.type_vim.cursor_col = 0;
                self.state.type_ts_vim.set_content("");
                self.state.type_csharp_vim.set_content("");
                self.state.set_status("Type regenerated from response");
            }

            // === Request Execution ===
            Action::ExecuteRequest => {
                self.execute_request().await;
            }
            Action::CancelRequest => {
                self.cancel_request();
            }
            Action::RequestCompleted(response) => {
                self.state.request_in_flight = false;
                self.state.request_started_at = None;
                self.state.request_abort_handle = None;
                self.state.last_error = None;
                let status = response.status;
                let elapsed = response.elapsed_display();
                self.state.last_response_info = Some((status, response.elapsed.as_millis() as u64));

                // Cache response for request chaining
                if let Some(ref name) = self.state.current_request.name {
                    let collection_name = self.state.collections
                        .get(self.state.active_collection)
                        .map(|c| c.name.as_str())
                        .unwrap_or("_");
                    let key = format!("{}/{}", collection_name, name);
                    self.cache_response(key.clone(), (*response).clone());

                    // Store in per-request response history (max 5)
                    let entry = crate::model::response::ResponseHistoryEntry {
                        response: (*response).clone(),
                        timestamp: chrono::Local::now(),
                    };
                    let history = self.state.response_histories.data.entry(key).or_insert_with(std::collections::VecDeque::new);
                    history.push_front(entry);
                    if history.len() > 5 {
                        history.pop_back();
                    }
                    self.state.response_histories.save(&crate::config::data_dir().join("response_history.json"));
                }

                self.state.current_response = Some(*response);
                self.state.viewing_history = None;
                self.state.resp_vim.scroll_offset = 0; self.state.resp_hscroll = 0;

                // Infer type from response
                if let Some(ref resp) = self.state.current_response {
                    if resp.body_bytes.is_some() {
                        // Binary response — type is Buffer
                        self.state.response_type = Some(crate::model::response_type::JsonType::Buffer);
                    } else if let Ok(json_val) = serde_json::from_str::<serde_json::Value>(&resp.body) {
                        self.state.response_type = Some(crate::model::response_type::JsonType::infer(&json_val));
                    } else {
                        self.state.response_type = None;
                    }
                }
                self.state.type_vim.scroll_offset = 0;

                // Auto-generate type text (unless user has locked it)
                if !self.state.response_type_locked {
                    if let Some(ref rt) = self.state.response_type {
                        let type_name = self.state.current_request.name.as_deref()
                            .map(|n| { let mut c = n.chars(); match c.next() { None => String::new(), Some(f) => f.to_uppercase().to_string() + c.as_str() } })
                            .unwrap_or_else(|| "ResponseType".to_string());

                        if matches!(rt, crate::model::response_type::JsonType::Buffer) {
                            let ct = self.state.current_response.as_ref()
                                .and_then(|r| r.content_type.as_deref())
                                .unwrap_or("application/octet-stream");
                            self.state.response_type_text = format!("Buffer ({})", ct);
                            self.state.type_ts_text = format!(
                                "// Content-Type: {ct}\n\
                                 // Returns binary data as a Buffer\n\
                                 type {type_name} = Buffer | ArrayBuffer\n\
                                 \n\
                                 // Usage:\n\
                                 // const res = await fetch(url)\n\
                                 // const buffer = await res.arrayBuffer()");
                            self.state.type_csharp_text = format!(
                                "// Content-Type: {ct}\n\
                                 // Returns binary data as a byte array\n\
                                 \n\
                                 // Usage:\n\
                                 // HttpResponseMessage response = await client.GetAsync(url);\n\
                                 // byte[] {name} = await response.Content.ReadAsByteArrayAsync();\n\
                                 // or\n\
                                 // Stream {name} = await response.Content.ReadAsStreamAsync();",
                                name = type_name.to_lowercase());
                        } else {
                            self.state.response_type_text = rt.to_display_lines(0).join("\n");
                            self.state.type_ts_text = rt.to_typescript(&type_name);
                            self.state.type_csharp_text = rt.to_csharp(&type_name);
                        }
                    } else {
                        self.state.response_type_text.clear();
                        self.state.type_ts_text.clear();
                        self.state.type_csharp_text.clear();
                    }
                    self.state.type_validation_errors.clear();
                    self.state.type_ts_vim.set_content("");
                    self.state.type_csharp_vim.set_content("");
                } else {
                    self.validate_response_type();
                }

                // Save to history
                {
                    let entry = crate::model::history::HistoryEntry {
                        method: self.state.current_request.method,
                        url: self.state.current_request.url.clone(),
                        name: self.state.current_request.name.clone(),
                        status,
                        status_text: self.state.current_response.as_ref().map(|r| r.status_text.clone()).unwrap_or_default(),
                        elapsed_ms: self.state.current_response.as_ref().map(|r| r.elapsed.as_millis() as u64).unwrap_or(0),
                        size_bytes: self.state.current_response.as_ref().map(|r| r.size_bytes).unwrap_or(0),
                        timestamp: chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
                        body_preview: self.state.current_response.as_ref()
                            .map(|r| r.body.chars().take(200).collect())
                            .unwrap_or_default(),
                    };
                    let limit = self.state.config.general.history_limit;
                    self.state.history.add(entry, limit);
                    self.state.history.save(&crate::config::data_dir().join("history.json"));
                }

                self.state.set_status(format!("{} - {}", status, elapsed));
            }
            Action::RequestFailed(err) => {
                self.state.request_in_flight = false;
                self.state.request_started_at = None;
                self.state.request_abort_handle = None;
                self.state.last_error = Some(err.clone());
                self.state.current_response = None;
                self.state.set_status(format!("Error: {}", err));
            }

            // === SSL ===
            Action::ToggleInsecureMode => {
                self.state.config.general.verify_ssl = !self.state.config.general.verify_ssl;
                if self.state.config.general.verify_ssl {
                    self.state.set_status("SSL: Strict (certificates verified)");
                } else {
                    self.state.set_status("SSL: Insecure (certificates NOT verified)");
                }
            }

            Action::ToggleWrap => {
                self.state.wrap_enabled = !self.state.wrap_enabled;
                if self.state.wrap_enabled {
                    self.state.set_status("Wrap: ON");
                } else {
                    self.state.set_status("Wrap: OFF");
                }
            }

            Action::ExitDiffView => {
                self.state.viewing_diff = None;
                // Restore resp_vim lines from the current response body
                if let Some(ref resp) = self.state.current_response {
                    self.state.resp_vim.set_content(&resp.formatted_body());
                } else {
                    self.state.resp_vim.set_content("");
                }
                self.state.resp_hscroll = 0;
            }
            Action::ExportResponse => {
                if let Some(ref resp) = self.state.current_response {
                    let ext = match resp.content_type.as_deref() {
                        Some(ct) if ct.contains("json") => "json",
                        Some(ct) if ct.contains("html") => "html",
                        Some(ct) if ct.contains("xml") => "xml",
                        Some(ct) if ct.contains("image/png") => "png",
                        Some(ct) if ct.contains("image/jpeg") || ct.contains("image/jpg") => "jpg",
                        Some(ct) if ct.contains("image/gif") => "gif",
                        Some(ct) if ct.contains("image/webp") => "webp",
                        Some(ct) if ct.contains("pdf") => "pdf",
                        _ => "txt",
                    };
                    let ts = chrono::Local::now().format("%H%M%S");
                    let filename = format!("response_{}.{}", ts, ext);
                    let result = if let Some(ref bytes) = resp.body_bytes {
                        std::fs::write(&filename, bytes)
                    } else {
                        std::fs::write(&filename, resp.formatted_body())
                    };
                    match result {
                        Ok(()) => self.state.set_status(format!("Exported to {}", filename)),
                        Err(e) => self.state.set_status(format!("Export failed: {}", e)),
                    }
                } else {
                    self.state.set_status("No response to export");
                }
            }

            // === Overlays ===
            Action::OpenOverlay(overlay) => {
                if matches!(overlay, Overlay::EnvironmentSelector) {
                    self.state.env_selector_state.select(Some(self.state.environments.active.unwrap_or(0)));
                }
                if matches!(overlay, Overlay::Help) {
                    self.state.help_scroll = 0;
                }
                self.state.overlay = Some(overlay);
            }
            Action::CloseOverlay => {
                // For EnvironmentEditor: if editing, cancel edit instead of closing
                if let Some(Overlay::EnvironmentEditor { cursor, editing_key, .. }) = &self.state.overlay {
                    if *cursor > 0 || *editing_key {
                        if let Some(Overlay::EnvironmentEditor { ref mut cursor, ref mut editing_key, ref mut new_key, ref mut new_value, .. }) = self.state.overlay {
                            *cursor = 0;
                            *editing_key = false;
                            *new_key = String::new();
                            *new_value = String::new();
                        }
                        return Ok(());
                    }
                }
                self.state.overlay = None;
            }
            Action::OverlayUp => {
                match &mut self.state.overlay {
                    Some(Overlay::EnvironmentSelector) => {
                        let i = self.state.env_selector_state.selected().unwrap_or(0).saturating_sub(1);
                        self.state.env_selector_state.select(Some(i));
                    }
                    Some(Overlay::HeaderAutocomplete { selected, .. }) => { *selected = selected.saturating_sub(1); }
                    Some(Overlay::MoveRequest { selected }) => { *selected = selected.saturating_sub(1); }
                    Some(Overlay::ThemeSelector { selected }) => { *selected = selected.saturating_sub(1); }
                    Some(Overlay::History { selected }) => { *selected = selected.saturating_sub(1); }
                    Some(Overlay::ResponseHistory { selected }) => { *selected = selected.saturating_sub(1); }
                    Some(Overlay::ResponseDiffSelect { selected }) => { *selected = selected.saturating_sub(1); }
                    Some(Overlay::Help) => { self.state.help_scroll = self.state.help_scroll.saturating_sub(1); }
                    Some(Overlay::EnvironmentEditor { selected, cursor, .. }) if *cursor == 0 => {
                        *selected = selected.saturating_sub(1);
                    }
                    _ => {}
                }
            }
            Action::OverlayDown => {
                match &mut self.state.overlay {
                    Some(Overlay::EnvironmentSelector) => {
                        let i = self.state.env_selector_state.selected().map(|i| i + 1).unwrap_or(0);
                        let max = self.state.environments.environments.len().saturating_sub(1);
                        self.state.env_selector_state.select(Some(i.min(max)));
                    }
                    Some(Overlay::HeaderAutocomplete { selected, suggestions }) => {
                        *selected = (*selected + 1).min(suggestions.len().saturating_sub(1));
                    }
                    Some(Overlay::MoveRequest { selected }) => {
                        let max = self.state.collections.len().saturating_sub(1);
                        *selected = (*selected + 1).min(max);
                    }
                    Some(Overlay::ThemeSelector { selected }) => {
                        let max = crate::theme::THEME_NAMES.len().saturating_sub(1);
                        *selected = (*selected + 1).min(max);
                    }
                    Some(Overlay::History { selected }) => {
                        let max = self.state.history.entries.len().saturating_sub(1);
                        *selected = (*selected + 1).min(max);
                    }
                    Some(Overlay::ResponseHistory { selected }) => {
                        let key = self.state.current_request.name.as_ref().map(|name| {
                            let coll = self.state.collections.get(self.state.active_collection).map(|c| c.name.as_str()).unwrap_or("_");
                            format!("{}/{}", coll, name)
                        });
                        let max = key.and_then(|k| self.state.response_histories.data.get(&k).map(|h| h.len())).unwrap_or(0usize).saturating_sub(1);
                        *selected = (*selected + 1).min(max);
                    }
                    Some(Overlay::ResponseDiffSelect { selected }) => {
                        let key = self.state.current_request.name.as_ref().map(|name| {
                            let coll = self.state.collections.get(self.state.active_collection).map(|c| c.name.as_str()).unwrap_or("_");
                            format!("{}/{}", coll, name)
                        });
                        let max = key.and_then(|k| self.state.response_histories.data.get(&k).map(|h| h.len())).unwrap_or(0usize).saturating_sub(1);
                        *selected = (*selected + 1).min(max);
                    }
                    Some(Overlay::Help) => { self.state.help_scroll += 1; }
                    Some(Overlay::EnvironmentEditor { selected, cursor, .. }) if *cursor == 0 => {
                        if let Some(active_idx) = self.state.environments.active {
                            let max = self.state.environments.environments[active_idx].variables.len().saturating_sub(1);
                            *selected = (*selected + 1).min(max);
                        }
                    }
                    _ => {}
                }
            }
            Action::OverlayConfirm => {
                let overlay = self.state.overlay.take();
                match overlay {
                    Some(Overlay::EnvironmentSelector) => {
                        if let Some(idx) = self.state.env_selector_state.selected() {
                            if idx < self.state.environments.environments.len() {
                                self.state.environments.active = Some(idx);
                                let name = self.state.environments.environments[idx].name.clone();
                                self.state.set_status(format!("Environment: {}", name));
                            }
                        }
                    }
                    Some(Overlay::HeaderAutocomplete { suggestions, selected }) => {
                        if let Some((name, value)) = suggestions.get(selected) {
                            self.state.current_request.headers.push(Header { name: name.clone(), value: value.clone(), enabled: true });
                            let idx = self.state.current_request.headers.len() - 1;
                            self.state.request_focus = RequestFocus::Header(idx);
                            self.state.header_edit_field = 1;
                            self.state.header_edit_cursor = value.len();
                            self.state.mode = InputMode::Insert;
                        }
                    }
                    Some(Overlay::NewCollection { name }) => {
                        if !name.trim().is_empty() {
                            let filename = format!("{}.http", name.trim());
                            // Create in .http/ folder (convention)
                            let http_dir = PathBuf::from(".http");
                            let _ = std::fs::create_dir_all(&http_dir);
                            let path = http_dir.join(&filename);
                            let content = format!("### {}\nGET https://example.com\n", name.trim());
                            let _ = std::fs::write(&path, &content);
                            if let Ok(requests) = crate::parser::http::parse(&content) {
                                self.state.collections.push(Collection { name: name.trim().to_string(), path, requests, format: FileFormat::Http });
                                self.state.active_collection = self.state.collections.len() - 1;
                                self.rebuild_collection_items();
                                self.state.set_status(format!("Created: .http/{}", filename));
                            }
                        }
                    }
                    Some(Overlay::RenameRequest { name }) => {
                        if !name.trim().is_empty() {
                            if let Some(flat_idx) = self.state.collections_state.selected() {
                                match self.flat_idx_to_coll_req(flat_idx) {
                                    Some((ci, None)) => {
                                        // Rename collection file
                                        if let Some(coll) = self.state.collections.get_mut(ci) {
                                            let old_path = coll.path.clone();
                                            let new_filename = format!("{}.http", name.trim());
                                            let new_path = old_path.with_file_name(&new_filename);
                                            if std::fs::rename(&old_path, &new_path).is_ok() {
                                                coll.name = name.trim().to_string();
                                                coll.path = new_path;
                                                self.rebuild_collection_items();
                                                self.state.set_status(format!("Renamed → '{}'", name.trim()));
                                            } else {
                                                self.state.set_status("Failed to rename file");
                                            }
                                        }
                                    }
                                    Some((ci, Some(ri))) => {
                                        if let Some(req) = self.state.collections.get_mut(ci).and_then(|c| c.requests.get_mut(ri)) {
                                            req.name = Some(name.trim().to_string());
                                            self.state.current_request.name = Some(name.trim().to_string());
                                            self.persist_collection(ci);
                                            self.rebuild_collection_items();
                                            self.state.set_status(format!("Renamed → '{}'", name.trim()));
                                        }
                                    }
                                    None => {}
                                }
                            }
                        }
                    }
                    Some(Overlay::ConfirmDelete { .. }) => {
                        if let Some(flat_idx) = self.state.collections_state.selected() {
                            match self.flat_idx_to_coll_req(flat_idx) {
                                Some((ci, None)) => {
                                    // Delete entire collection
                                    if let Some(coll) = self.state.collections.get(ci) {
                                        let _ = std::fs::remove_file(&coll.path);
                                        let coll_name = coll.name.clone();
                                        self.state.expanded_collections.remove(&ci);
                                        self.state.collections.remove(ci);
                                        if self.state.active_collection >= self.state.collections.len() && self.state.active_collection > 0 {
                                            self.state.active_collection -= 1;
                                        }
                                        self.rebuild_collection_items();
                                        self.state.collections_state.select(Some(0));
                                        if let Some(coll) = self.state.collections.get(self.state.active_collection) {
                                            if let Some(req) = coll.requests.first() {
                                                self.state.current_request = req.clone();
                                            }
                                        } else {
                                            self.state.current_request = Request::default();
                                        }
                                        let body = self.state.current_request.get_body(self.state.body_type).to_string();
                                        self.state.body_vim.set_content(&body);
                                        self.state.current_response = None;
                                        self.state.set_status(format!("Deleted collection '{}'", coll_name));
                                    }
                                }
                                Some((ci, Some(ri))) => {
                                    if let Some(coll) = self.state.collections.get_mut(ci) {
                                        if ri < coll.requests.len() {
                                            let req_name = coll.requests[ri].display_name();
                                            coll.requests.remove(ri);
                                            self.persist_collection(ci);
                                            self.rebuild_collection_items();
                                            let max = self.state.collection_items.len().saturating_sub(1);
                                            self.state.collections_state.select(Some(flat_idx.min(max)));
                                            self.state.set_status(format!("Deleted '{}'", req_name));
                                        }
                                    }
                                }
                                None => {}
                            }
                        }
                    }
                    Some(Overlay::MoveRequest { selected: target_coll }) => {
                        if let Some(flat_idx) = self.state.collections_state.selected() {
                            if let Some((src_ci, Some(ri))) = self.flat_idx_to_coll_req(flat_idx) {
                                if target_coll != src_ci {
                                    if let Some(req) = self.state.collections.get(src_ci).and_then(|c| c.requests.get(ri)).cloned() {
                                        let req_name = req.display_name();
                                        self.state.collections.get_mut(src_ci).unwrap().requests.remove(ri);
                                        self.persist_collection(src_ci);
                                        let target_name = self.state.collections.get(target_coll).map(|c| c.name.clone()).unwrap_or_default();
                                        self.state.collections.get_mut(target_coll).unwrap().requests.push(req);
                                        self.persist_collection(target_coll);
                                        self.state.expanded_collections.insert(target_coll);
                                        self.rebuild_collection_items();
                                        self.state.set_status(format!("Moved '{}' → '{}'", req_name, target_name));
                                    }
                                } else {
                                    self.state.set_status("Cannot move to same collection");
                                }
                            }
                        }
                    }
                    Some(Overlay::ThemeSelector { selected }) => {
                        if let Some(&name) = crate::theme::THEME_NAMES.get(selected) {
                            self.state.theme = crate::theme::load_theme(name);
                            self.state.set_status(format!("Theme: {}", name));
                        }
                    }
                    Some(Overlay::EnvironmentEditor { selected, editing_key, new_key, new_value, cursor }) => {
                        if let Some(active_idx) = self.state.environments.active {
                            if editing_key {
                                // Was adding a new variable: key phase done, now enter value phase
                                if !new_key.is_empty() {
                                    // Switch to value editing phase
                                    self.state.overlay = Some(Overlay::EnvironmentEditor {
                                        selected,
                                        editing_key: false,
                                        new_key: new_key.clone(),
                                        new_value: String::new(),
                                        cursor: 1, // non-zero = editing value
                                    });
                                    return Ok(());
                                }
                            } else if cursor > 0 && !new_key.is_empty() {
                                // Adding new variable: value phase done
                                self.state.environments.environments[active_idx].variables.insert(new_key.clone(), new_value.clone());
                                self.state.set_status(format!("Added: {} = {}", new_key, new_value));
                            } else if cursor > 0 {
                                // Was editing an existing value
                                let env = &mut self.state.environments.environments[active_idx];
                                if let Some((key, val)) = env.variables.get_index_mut(selected) {
                                    let key_name = key.clone();
                                    *val = new_value.clone();
                                    self.state.set_status(format!("Updated: {}", key_name));
                                }
                            } else {
                                // Not editing yet: start editing the selected variable's value
                                let env = &self.state.environments.environments[active_idx];
                                if let Some((_key, val)) = env.variables.get_index(selected) {
                                    let val_clone = val.clone();
                                    let val_len = val_clone.len();
                                    self.state.overlay = Some(Overlay::EnvironmentEditor {
                                        selected,
                                        editing_key: false,
                                        new_key: String::new(),
                                        new_value: val_clone,
                                        cursor: val_len + 1, // non-zero = editing
                                    });
                                    return Ok(());
                                }
                            }
                        }
                    }
                    Some(Overlay::History { selected }) => {
                        // Load selected history entry into current request fields
                        if let Some(entry) = self.state.history.entries.get(selected) {
                            self.state.current_request.method = entry.method;
                            self.state.current_request.url = entry.url.clone();
                            if entry.name.is_some() {
                                self.state.current_request.name = entry.name.clone();
                            }
                            self.state.current_response = None;
                            self.state.last_error = None;
                            self.state.set_status(format!("Loaded: {} {}", entry.method, entry.url));
                        }
                    }
                    Some(Overlay::SetCacheTTL { input }) => {
                        if let Ok(secs) = input.parse::<u64>() {
                            if secs > 0 {
                                self.state.config.general.chain_cache_ttl = secs;
                                self.state.response_cache.clear();
                                self.state.set_status(format!("Chain cache TTL: {}s", secs));
                            } else {
                                self.state.set_status("TTL must be > 0");
                            }
                        } else {
                            self.state.set_status("Invalid number");
                        }
                    }
                    Some(Overlay::ResponseDiffSelect { selected }) => {
                        // Diff current response vs selected historical response
                        if let Some(ref current) = self.state.current_response {
                            if let Some(ref name) = self.state.current_request.name {
                                let collection_name = self.state.collections
                                    .get(self.state.active_collection)
                                    .map(|c| c.name.as_str())
                                    .unwrap_or("_");
                                let key = format!("{}/{}", collection_name, name);
                                if let Some(history) = self.state.response_histories.data.get(&key) {
                                    if let Some(entry) = history.get(selected) {
                                        let current_body = current.formatted_body();
                                        let old_body = entry.response.formatted_body();
                                        let diff = similar::TextDiff::from_lines(&old_body, &current_body);
                                        let mut diff_text = String::new();
                                        for change in diff.iter_all_changes() {
                                            let prefix = match change.tag() {
                                                similar::ChangeTag::Equal => "  ",
                                                similar::ChangeTag::Insert => "+ ",
                                                similar::ChangeTag::Delete => "- ",
                                            };
                                            diff_text.push_str(prefix);
                                            diff_text.push_str(change.to_string_lossy().trim_end_matches('\n'));
                                            diff_text.push('\n');
                                        }
                                        let ts = entry.timestamp.format("%H:%M:%S").to_string();
                                        self.state.resp_vim.set_content(&diff_text);
                                        self.state.viewing_diff = Some((diff_text, ts));
                                        self.state.resp_hscroll = 0;
                                    }
                                }
                            }
                        }
                    }
                    Some(Overlay::ResponseHistory { selected }) => {
                        // Load selected historical response
                        if let Some(ref name) = self.state.current_request.name {
                            let collection_name = self.state.collections
                                .get(self.state.active_collection)
                                .map(|c| c.name.as_str())
                                .unwrap_or("_");
                            let key = format!("{}/{}", collection_name, name);
                            if let Some(history) = self.state.response_histories.data.get(&key) {
                                if let Some(entry) = history.get(selected) {
                                    self.state.current_response = Some(entry.response.clone());
                                    self.state.resp_vim.scroll_offset = 0; self.state.resp_hscroll = 0;
                                    // Re-infer type
                                    if let Some(ref resp) = self.state.current_response {
                                        if resp.body_bytes.is_some() {
                                            self.state.response_type = Some(crate::model::response_type::JsonType::Buffer);
                                        } else if let Ok(json_val) = serde_json::from_str::<serde_json::Value>(&resp.body) {
                                            self.state.response_type = Some(crate::model::response_type::JsonType::infer(&json_val));
                                        } else {
                                            self.state.response_type = None;
                                        }
                                    }
                                    let total = history.len();
                                    let ts = entry.timestamp.format("%H:%M:%S").to_string();
                                    self.state.viewing_history = Some((selected + 1, total, ts));
                                    self.state.set_status(format!("History {}/{} — {}", selected + 1, total, entry.timestamp.format("%H:%M:%S")));
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
            Action::OverlayInput(c) => {
                match self.state.overlay {
                    Some(Overlay::NewCollection { ref mut name }) => { name.push(c); }
                    Some(Overlay::RenameRequest { ref mut name }) => { name.push(c); }
                    Some(Overlay::SetCacheTTL { ref mut input }) => {
                        if c.is_ascii_digit() { input.push(c); }
                    }
                    Some(Overlay::EnvironmentEditor { ref mut editing_key, ref mut new_key, ref mut new_value, ref mut cursor, .. }) => {
                        if *cursor == 0 && !*editing_key && c == 'a' {
                            // Start adding a new variable: enter key input mode
                            *editing_key = true;
                            *new_key = String::new();
                            *new_value = String::new();
                            *cursor = 1;
                        } else if *editing_key {
                            // Typing the key name
                            new_key.push(c);
                        } else if *cursor > 0 {
                            // Typing the value
                            new_value.push(c);
                        }
                    }
                    _ => {}
                }
            }
            Action::OverlayBackspace => {
                match self.state.overlay {
                    Some(Overlay::NewCollection { ref mut name }) => { name.pop(); }
                    Some(Overlay::RenameRequest { ref mut name }) => { name.pop(); }
                    Some(Overlay::SetCacheTTL { ref mut input }) => { input.pop(); }
                    Some(Overlay::EnvironmentEditor { ref mut editing_key, ref mut new_key, ref mut new_value, ref mut cursor, .. }) => {
                        if *editing_key {
                            new_key.pop();
                        } else if *cursor > 0 {
                            new_value.pop();
                        }
                    }
                    _ => {}
                }
            }
            Action::OverlayDelete => {
                if let Some(Overlay::EnvironmentEditor { selected, .. }) = &self.state.overlay {
                    let selected = *selected;
                    if let Some(active_idx) = self.state.environments.active {
                        let env = &mut self.state.environments.environments[active_idx];
                        if selected < env.variables.len() {
                            let key = env.variables.get_index(selected).map(|(k, _)| k.clone());
                            if let Some(key) = key {
                                env.variables.shift_remove(&key);
                                self.state.set_status(format!("Deleted: {}", key));
                                // Adjust selected index
                                if let Some(Overlay::EnvironmentEditor { selected: ref mut sel, .. }) = self.state.overlay {
                                    let max = self.state.environments.environments[active_idx].variables.len().saturating_sub(1);
                                    *sel = (*sel).min(max);
                                }
                            }
                        }
                    }
                }
            }

            // === Clipboard ===
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

            // === Theme (direct set) ===
            Action::SetTheme(name) => {
                self.state.theme = crate::theme::load_theme(&name);
                self.state.set_status(format!("Theme: {}", self.state.theme.name));
            }

            // === Command Palette ===
            Action::OpenCommandPalette => {
                self.state.command_palette.open = true;
                self.state.command_palette.input.clear();
                self.state.command_palette.selected = 0;
            }
            Action::CommandPaletteClose => {
                self.state.command_palette.open = false;
            }
            Action::CommandPaletteInput(c) => {
                self.state.command_palette.input.push(c);
                self.state.command_palette.selected = 0;
            }
            Action::CommandPaletteBackspace => {
                self.state.command_palette.input.pop();
                self.state.command_palette.selected = 0;
            }
            Action::CommandPaletteUp => {
                self.state.command_palette.selected =
                    self.state.command_palette.selected.saturating_sub(1);
            }
            Action::CommandPaletteDown => {
                let count = crate::ui::command_palette::filtered_commands(
                    &self.state.command_palette.input,
                ).len();
                if count > 0 {
                    self.state.command_palette.selected =
                        (self.state.command_palette.selected + 1).min(count - 1);
                }
            }
            Action::CommandPaletteConfirm => {
                let matches = crate::ui::command_palette::filtered_commands(
                    &self.state.command_palette.input,
                );
                let selected = self.state.command_palette.selected;
                self.state.command_palette.open = false;
                if let Some(cmd) = matches.get(selected) {
                    let action = cmd.action.clone();
                    Box::pin(self.update(action)).await?;
                }
            }

            // === Response Headers Inspector ===
            Action::ToggleResponseHeaders => {
                self.state.response_headers_expanded = !self.state.response_headers_expanded;
                self.state.response_headers_scroll = 0;
            }

            // === Search ===
            Action::StartSearch => {
                self.state.search.active = true;
                self.state.search.query.clear();
                self.state.search.matches.clear();
                self.state.search.match_idx = 0;
            }
            Action::SearchInput(c) => {
                self.state.search.query.push(c);
                self.recalculate_search_matches();
            }
            Action::SearchBackspace => {
                self.state.search.query.pop();
                self.recalculate_search_matches();
            }
            Action::SearchConfirm => {
                self.state.search.active = false;
                // Keep matches highlighted and current position
            }
            Action::SearchCancel => {
                self.state.search.active = false;
                self.state.search.query.clear();
                self.state.search.matches.clear();
                self.state.search.match_idx = 0;
            }
            Action::SearchNext => {
                if !self.state.search.matches.is_empty() {
                    self.state.search.match_idx =
                        (self.state.search.match_idx + 1) % self.state.search.matches.len();
                    self.jump_to_current_search_match();
                }
            }
            Action::SearchPrev => {
                if !self.state.search.matches.is_empty() {
                    let len = self.state.search.matches.len();
                    self.state.search.match_idx =
                        (self.state.search.match_idx + len - 1) % len;
                    self.jump_to_current_search_match();
                }
            }

            // === Fold actions ===
            Action::ExpandCollection => {
                if let Some(flat_idx) = self.state.collections_state.selected() {
                    if let Some((ci, _)) = self.flat_idx_to_coll_req(flat_idx) {
                        self.state.expanded_collections.insert(ci);
                        self.rebuild_collection_items();
                    }
                }
            }
            Action::CollapseCollection => {
                if let Some(flat_idx) = self.state.collections_state.selected() {
                    if let Some((ci, _)) = self.flat_idx_to_coll_req(flat_idx) {
                        self.state.expanded_collections.remove(&ci);
                        self.rebuild_collection_items();
                    }
                }
            }
            Action::CollapseAll => {
                self.state.expanded_collections.clear();
                self.rebuild_collection_items();
                self.state.collections_state.select(Some(0));
            }
            Action::ExpandAll => {
                for ci in 0..self.state.collections.len() {
                    self.state.expanded_collections.insert(ci);
                }
                self.rebuild_collection_items();
            }

            // === Collections filter ===
            Action::StartCollectionsFilter => {
                self.state.collections_filter_active = true;
                self.state.collections_filter.clear();
            }
            Action::CollectionsFilterInput(c) => {
                self.state.collections_filter.push(c);
                self.rebuild_collection_items();
                self.state.collections_state.select(Some(0));
            }
            Action::CollectionsFilterBackspace => {
                self.state.collections_filter.pop();
                self.rebuild_collection_items();
                self.state.collections_state.select(Some(0));
            }
            Action::CollectionsFilterConfirm => {
                self.state.collections_filter_active = false;
            }
            Action::CollectionsFilterCancel => {
                self.state.collections_filter_active = false;
                self.state.collections_filter.clear();
                self.rebuild_collection_items();
                self.state.collections_state.select(Some(0));
            }

            // === Count prefix ===
            Action::AccumulateCount(digit) => {
                self.state.count_prefix = Some(self.state.count_prefix.unwrap_or(0) * 10 + digit);
            }

            // === Yank/Delete/Change with direction ===
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
                    Panel::Request if self.state.request_field_editing => {
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
                    Panel::Request if self.state.request_field_editing => {
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
                    Panel::Request if self.state.request_field_editing => {
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
            Action::DeleteToEnd => {
                self.state.pending_key = None;
                match self.state.active_panel {
                    Panel::Response if self.state.response_tab == ResponseTab::Type && self.state.type_sub_focus == crate::state::TypeSubFocus::Editor => {
                        self.state.type_vim.save_undo();
                        let yanked = {
                        let row = self.state.type_vim.cursor_row;
                        let col = self.state.type_vim.cursor_col;
                        let line_len = self.state.type_vim.current_line_len();
                        let text = self.state.type_vim.delete_range(col, line_len, row);
                        self.sync_type_vim_text();
                        text
                    };
                        self.state.yank_buffer = yanked;
                        let _ = crate::clipboard::copy_to_clipboard(&self.state.yank_buffer);
                        self.state.response_type_locked = true;
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
                    Panel::Request if self.state.request_field_editing => {
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
                    Panel::Request if self.state.request_field_editing => {
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
                    Panel::Request if self.state.request_field_editing => {
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
                    Panel::Request if self.state.request_field_editing => {
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

            // === Find char motions ===
            Action::FindCharForward(c) => {
                self.state.pending_key = None;
                self.find_char_forward(c, false);
            }
            Action::FindCharBackward(c) => {
                self.state.pending_key = None;
                self.find_char_backward(c, false);
            }
            Action::FindCharForwardBefore(c) => {
                self.state.pending_key = None;
                self.find_char_forward(c, true);
            }
            Action::FindCharBackwardAfter(c) => {
                self.state.pending_key = None;
                self.find_char_backward(c, true);
            }
        }
        Ok(())
    }
}
