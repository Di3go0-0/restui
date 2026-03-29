use anyhow::Result;
use std::path::PathBuf;
use tokio::sync::mpsc;

use crate::action::Action;
use crate::config::AppConfig;
use crate::event::{AppEvent, EventHandler};
use crate::http_client;
use crate::keybindings;
use crate::model::collection::{Collection, FileFormat};
use crate::model::request::{Header, QueryParam, Request};
use crate::parser;
use crate::state::{AppState, InputMode, Overlay, Panel, RequestFocus, RequestTab, COMMON_HEADERS};
use crate::tui::Tui;
use crate::ui;

pub struct App {
    pub state: AppState,
    action_tx: mpsc::UnboundedSender<Action>,
    action_rx: mpsc::UnboundedReceiver<Action>,
}

impl App {
    pub fn new(config: AppConfig) -> Self {
        let (action_tx, action_rx) = mpsc::unbounded_channel();
        Self {
            state: AppState::new(config),
            action_tx,
            action_rx,
        }
    }

    pub fn load_collections(&mut self, dirs: &[PathBuf]) {
        let collections = parser::scan_directories(dirs);
        self.state.collections = collections;
        self.rebuild_collection_items();
        if let Some(collection) = self.state.collections.first() {
            if let Some(req) = collection.requests.first() {
                self.state.current_request = req.clone();
            }
        }
    }

    fn rebuild_collection_items(&mut self) {
        let mut items = Vec::new();
        for (ci, collection) in self.state.collections.iter().enumerate() {
            let marker = if ci == self.state.active_collection { "●" } else { "○" };
            items.push(format!("{} {}", marker, collection.display_name()));
            if ci == self.state.active_collection {
                for req in &collection.requests {
                    items.push(format!("  {} {}", req.method, req.display_name()));
                }
            }
        }
        self.state.collection_items = items;
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
        let tick_rate = std::time::Duration::from_millis(250);
        let mut events = EventHandler::new(tick_rate);

        loop {
            if let Ok(size) = terminal.size() {
                let right_width = (size.width as u32 * 80 / 100) as u16;
                self.state.is_wide_layout = right_width > 120;

                // Calculate visible heights for scroll-follow
                let main_h = size.height.saturating_sub(1); // status bar
                if right_width > 120 {
                    // Wide: center is 50% of right, body is 60% of center
                    let center_h = main_h;
                    self.state.body_visible_height = (center_h as u32 * 60 / 100) as u16;
                    self.state.resp_visible_height = main_h;
                } else {
                    // Narrow: body 35%, response 40%
                    self.state.body_visible_height = (main_h as u32 * 35 / 100) as u16;
                    self.state.resp_visible_height = (main_h as u32 * 40 / 100) as u16;
                }
                // Account for borders and internal layout (approx 5 lines for response header area)
                self.state.body_visible_height = self.state.body_visible_height.saturating_sub(2);
                self.state.resp_visible_height = self.state.resp_visible_height.saturating_sub(5);
            }

            terminal.draw(|frame| {
                ui::layout::render(frame, &self.state);
            })?;

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
        match action {
            Action::Quit => self.state.should_quit = true,
            Action::Tick => {
                if let Some((_, instant)) = &self.state.status_message {
                    if instant.elapsed() > std::time::Duration::from_secs(5) {
                        self.state.status_message = None;
                    }
                }
                if let Some((_, instant)) = self.state.pending_key {
                    if instant.elapsed() > std::time::Duration::from_millis(500) {
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
            }
            Action::FocusPanel(panel) => {
                self.state.active_panel = panel;
                self.state.mode = InputMode::Normal;
            }

            // === Vim Mode Transitions ===
            Action::EnterInsertMode => {
                match self.state.active_panel {
                    Panel::Body => {
                        self.state.mode = InputMode::Insert;
                        self.position_body_cursor_at_end();
                    }
                    Panel::Request => {
                        self.state.mode = InputMode::Insert;
                        self.position_request_cursor_at_end();
                    }
                    _ => {}
                }
            }
            Action::EnterInsertModeStart => {
                if self.state.active_panel == Panel::Body {
                    self.state.mode = InputMode::Insert;
                    self.state.body_cursor_col = 0;
                }
            }
            Action::EnterAppendMode => {
                if self.state.active_panel == Panel::Body {
                    self.state.mode = InputMode::Insert;
                    let body = self.state.current_request.body.as_deref().unwrap_or("");
                    let lines: Vec<&str> = body.lines().collect();
                    let line_len = lines.get(self.state.body_cursor_row).map(|l| l.len()).unwrap_or(0);
                    self.state.body_cursor_col = line_len;
                }
            }
            Action::OpenLineBelow => {
                if self.state.active_panel == Panel::Body {
                    let body = self.state.current_request.body.get_or_insert_with(String::new);
                    let lines: Vec<&str> = body.lines().collect();
                    let line_end_offset = if self.state.body_cursor_row < lines.len() {
                        let mut off = 0;
                        for (i, line) in lines.iter().enumerate() {
                            off += line.len();
                            if i == self.state.body_cursor_row { break; }
                            off += 1;
                        }
                        off
                    } else {
                        body.len()
                    };
                    body.insert(line_end_offset, '\n');
                    self.state.body_cursor_row += 1;
                    self.state.body_cursor_col = 0;
                    self.state.mode = InputMode::Insert;
                }
            }
            Action::OpenLineAbove => {
                if self.state.active_panel == Panel::Body {
                    let body = self.state.current_request.body.get_or_insert_with(String::new);
                    let line_start = row_col_to_offset(body, self.state.body_cursor_row, 0);
                    body.insert(line_start, '\n');
                    self.state.body_cursor_col = 0;
                    self.state.mode = InputMode::Insert;
                }
            }
            Action::ExitInsertMode => {
                // Sync query params from URL when leaving insert on URL field
                if self.state.active_panel == Panel::Request && self.state.request_focus == RequestFocus::Url {
                    self.sync_params_from_url();
                }
                self.state.mode = InputMode::Normal;
                self.state.autocomplete = None;
                self.state.validate_body();
            }
            Action::EnterVisualMode => {
                match self.state.active_panel {
                    Panel::Body => {
                        self.state.mode = InputMode::Visual;
                        self.state.visual_anchor_row = self.state.body_cursor_row;
                        self.state.visual_anchor_col = self.state.body_cursor_col;
                    }
                    Panel::Response => {
                        self.state.mode = InputMode::Visual;
                        self.state.resp_visual_anchor_row = self.state.resp_cursor_row;
                        self.state.resp_visual_anchor_col = self.state.resp_cursor_col;
                    }
                    _ => {}
                }
            }
            Action::ExitVisualMode => self.state.mode = InputMode::Normal,

            // === Inline Autocomplete ===
            Action::AutocompleteNext => {
                if let Some(ref mut ac) = self.state.autocomplete {
                    ac.next();
                } else {
                    // Open autocomplete if editing header name
                    self.try_open_autocomplete();
                }
            }
            Action::AutocompletePrev => {
                if let Some(ref mut ac) = self.state.autocomplete {
                    ac.prev();
                } else {
                    self.try_open_autocomplete();
                }
            }
            Action::AutocompleteAccept => {
                if let Some(ac) = self.state.autocomplete.take() {
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
            Action::ScrollDown => self.scroll_down(),
            Action::ScrollUp => self.scroll_up(),
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
                // Get current name of the selected item
                if let Some(flat_idx) = self.state.collections_state.selected() {
                    let current_name = if flat_idx == 0 {
                        // Collection header
                        self.state.collections.get(self.state.active_collection)
                            .map(|c| c.name.clone()).unwrap_or_default()
                    } else if let Some(coll) = self.state.collections.get(self.state.active_collection) {
                        let req_idx = flat_idx - 1;
                        coll.requests.get(req_idx)
                            .and_then(|r| r.name.clone())
                            .unwrap_or_default()
                    } else {
                        String::new()
                    };
                    self.state.overlay = Some(Overlay::RenameRequest { name: current_name });
                }
            }
            Action::DeleteSelected => {
                if let Some(flat_idx) = self.state.collections_state.selected() {
                    let msg = if flat_idx == 0 {
                        let coll_name = self.state.collections.get(self.state.active_collection)
                            .map(|c| c.name.clone()).unwrap_or_default();
                        format!("Delete collection '{}'? (y/Enter to confirm, Esc/n to cancel)", coll_name)
                    } else if let Some(coll) = self.state.collections.get(self.state.active_collection) {
                        let req_idx = flat_idx - 1;
                        let req_name = coll.requests.get(req_idx)
                            .map(|r| r.display_name()).unwrap_or_default();
                        format!("Delete request '{}'? (y/Enter to confirm, Esc/n to cancel)", req_name)
                    } else {
                        return Ok(());
                    };
                    self.state.overlay = Some(Overlay::ConfirmDelete { message: msg });
                }
            }
            Action::MoveRequest => {
                if let Some(flat_idx) = self.state.collections_state.selected() {
                    if flat_idx > 0 && self.state.collections.len() > 1 {
                        self.state.overlay = Some(Overlay::MoveRequest { selected: 0 });
                    } else {
                        self.state.set_status("Select a request first (not a collection header)");
                    }
                }
            }
            Action::NewEmptyRequest => {
                self.state.current_request = Request::default();
                self.state.current_response = None;
                self.state.last_error = None;
                self.state.body_cursor_row = 0;
                self.state.body_cursor_col = 0;
                self.state.request_focus = RequestFocus::Url;
                self.state.set_status("New empty request");
            }

            // === Inline Text Editing ===
            Action::InlineInput(c) => self.inline_input(c),
            Action::InlineBackspace => self.inline_backspace(),
            Action::InlineDelete => self.inline_delete(),
            Action::InlineNewline => self.inline_newline(),
            Action::InlineCursorLeft => self.inline_cursor_left(),
            Action::InlineCursorRight => self.inline_cursor_right(),
            Action::InlineCursorUp => match self.state.active_panel {
                Panel::Response => self.resp_cursor_up(),
                _ => self.body_cursor_up(),
            },
            Action::InlineCursorDown => match self.state.active_panel {
                Panel::Response => self.resp_cursor_down(),
                _ => self.body_cursor_down(),
            },
            Action::InlineCursorHome => self.inline_cursor_home(),
            Action::InlineCursorEnd => self.inline_cursor_end(),
            Action::InlineTab => self.inline_tab(),

            // === Body Vim Motions ===
            Action::BodyWordForward => self.body_word_forward(),
            Action::BodyWordBackward => self.body_word_backward(),
            Action::BodyLineHome => { self.state.body_cursor_col = 0; }
            Action::BodyLineEnd => self.inline_cursor_end(),

            // === Visual Mode ===
            Action::VisualYank => {
                let text = match self.state.active_panel {
                    Panel::Body => Some(self.get_visual_selection()),
                    Panel::Response => Some(self.get_response_visual_selection()),
                    _ => None,
                };
                if let Some(text) = text {
                    self.state.yank_buffer = text.clone();
                    if let Ok(mut clipboard) = arboard::Clipboard::new() {
                        let _ = clipboard.set_text(&text);
                    }
                    self.state.mode = InputMode::Normal;
                    self.state.set_status("Yanked");
                }
            }
            Action::VisualDelete => {
                if self.state.active_panel == Panel::Body {
                    let text = self.get_visual_selection();
                    self.state.yank_buffer = text;
                    self.delete_visual_selection();
                    self.state.mode = InputMode::Normal;
                }
            }
            Action::Paste => {
                if self.state.active_panel == Panel::Body {
                    let paste = self.state.yank_buffer.clone();
                    self.paste_text_at_cursor(&paste);
                }
            }
            Action::PasteFromClipboard => {
                if let Ok(mut clipboard) = arboard::Clipboard::new() {
                    if let Ok(text) = clipboard.get_text() {
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
                            if self.state.current_request.body.as_deref().unwrap_or("").is_empty() {
                                self.state.current_request.body = Some(text.clone());
                                self.state.body_cursor_row = 0;
                                self.state.body_cursor_col = 0;
                            } else {
                                self.paste_text_at_cursor(&text);
                            }
                            self.state.validate_body();
                            self.state.set_status("Pasted from clipboard");
                        }
                    }
                }
            }
            Action::YankLine => {
                self.state.pending_key = None;
                if self.state.active_panel == Panel::Body {
                    let body = self.state.current_request.body.as_deref().unwrap_or("");
                    let lines: Vec<&str> = body.lines().collect();
                    if let Some(line) = lines.get(self.state.body_cursor_row) {
                        self.state.yank_buffer = format!("{}\n", line);
                        if let Ok(mut clipboard) = arboard::Clipboard::new() {
                            let _ = clipboard.set_text(&self.state.yank_buffer);
                        }
                        self.delete_body_line(self.state.body_cursor_row);
                        self.state.set_status("Line deleted");
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

            // === Request Execution ===
            Action::ExecuteRequest => {
                self.execute_request().await;
            }
            Action::RequestCompleted(response) => {
                self.state.request_in_flight = false;
                self.state.last_error = None;
                let status = response.status;
                let elapsed = response.elapsed_display();
                self.state.current_response = Some(*response);
                self.state.active_panel = Panel::Response;
                self.state.response_scroll = (0, 0);
                self.state.set_status(format!("{} - {}", status, elapsed));
            }
            Action::RequestFailed(err) => {
                self.state.request_in_flight = false;
                self.state.last_error = Some(err.clone());
                self.state.current_response = None;
                self.state.active_panel = Panel::Response;
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

            // === Overlays ===
            Action::OpenOverlay(overlay) => {
                if matches!(overlay, Overlay::EnvironmentSelector) {
                    self.state.env_selector_state.select(Some(self.state.environments.active.unwrap_or(0)));
                }
                self.state.overlay = Some(overlay);
            }
            Action::CloseOverlay => { self.state.overlay = None; }
            Action::OverlayUp => {
                match &mut self.state.overlay {
                    Some(Overlay::EnvironmentSelector) => {
                        let i = self.state.env_selector_state.selected().unwrap_or(0).saturating_sub(1);
                        self.state.env_selector_state.select(Some(i));
                    }
                    Some(Overlay::HeaderAutocomplete { selected, .. }) => { *selected = selected.saturating_sub(1); }
                    Some(Overlay::MoveRequest { selected }) => { *selected = selected.saturating_sub(1); }
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
                            let path = PathBuf::from(&filename);
                            let content = format!("### {}\nGET https://example.com\n", name.trim());
                            let _ = std::fs::write(&path, &content);
                            if let Ok(requests) = crate::parser::http::parse(&content) {
                                self.state.collections.push(Collection { name: name.trim().to_string(), path, requests, format: FileFormat::Http });
                                self.state.active_collection = self.state.collections.len() - 1;
                                self.rebuild_collection_items();
                                self.state.set_status(format!("Created: {}", filename));
                            }
                        }
                    }
                    Some(Overlay::RenameRequest { name }) => {
                        if !name.trim().is_empty() {
                            if let Some(flat_idx) = self.state.collections_state.selected() {
                                if flat_idx == 0 {
                                    // Rename collection (rename the file)
                                    if let Some(coll) = self.state.collections.get_mut(self.state.active_collection) {
                                        let old_path = coll.path.clone();
                                        let new_filename = format!("{}.http", name.trim());
                                        let new_path = old_path.with_file_name(&new_filename);
                                        if std::fs::rename(&old_path, &new_path).is_ok() {
                                            coll.name = name.trim().to_string();
                                            coll.path = new_path;
                                            self.rebuild_collection_items();
                                            self.state.set_status(format!("Collection renamed to '{}'", name.trim()));
                                        } else {
                                            self.state.set_status("Failed to rename collection file");
                                        }
                                    }
                                } else if let Some(coll) = self.state.collections.get_mut(self.state.active_collection) {
                                    let req_idx = flat_idx - 1;
                                    if let Some(req) = coll.requests.get_mut(req_idx) {
                                        req.name = Some(name.trim().to_string());
                                        // Also update current_request if it's the loaded one
                                        self.state.current_request.name = Some(name.trim().to_string());
                                        self.persist_collection(self.state.active_collection);
                                        self.rebuild_collection_items();
                                        self.state.set_status(format!("Request renamed to '{}'", name.trim()));
                                    }
                                }
                            }
                        }
                    }
                    Some(Overlay::ConfirmDelete { .. }) => {
                        if let Some(flat_idx) = self.state.collections_state.selected() {
                            if flat_idx == 0 {
                                // Delete entire collection
                                if let Some(coll) = self.state.collections.get(self.state.active_collection) {
                                    let _ = std::fs::remove_file(&coll.path);
                                    let name = coll.name.clone();
                                    self.state.collections.remove(self.state.active_collection);
                                    if self.state.active_collection >= self.state.collections.len() && self.state.active_collection > 0 {
                                        self.state.active_collection -= 1;
                                    }
                                    self.rebuild_collection_items();
                                    self.state.collections_state.select(Some(0));
                                    // Load first request of new active collection
                                    if let Some(coll) = self.state.collections.get(self.state.active_collection) {
                                        if let Some(req) = coll.requests.first() {
                                            self.state.current_request = req.clone();
                                        }
                                    } else {
                                        self.state.current_request = Request::default();
                                    }
                                    self.state.current_response = None;
                                    self.state.set_status(format!("Deleted collection '{}'", name));
                                }
                            } else if let Some(coll) = self.state.collections.get_mut(self.state.active_collection) {
                                let req_idx = flat_idx - 1;
                                if req_idx < coll.requests.len() {
                                    let name = coll.requests[req_idx].display_name();
                                    coll.requests.remove(req_idx);
                                    self.persist_collection(self.state.active_collection);
                                    self.rebuild_collection_items();
                                    // Adjust selection
                                    let max = self.state.collection_items.len().saturating_sub(1);
                                    let new_sel = flat_idx.min(max);
                                    self.state.collections_state.select(Some(new_sel));
                                    self.state.set_status(format!("Deleted request '{}'", name));
                                }
                            }
                        }
                    }
                    Some(Overlay::MoveRequest { selected: target_coll }) => {
                        if let Some(flat_idx) = self.state.collections_state.selected() {
                            if flat_idx > 0 && target_coll != self.state.active_collection {
                                let req_idx = flat_idx - 1;
                                if let Some(coll) = self.state.collections.get(self.state.active_collection) {
                                    if req_idx < coll.requests.len() {
                                        let req = coll.requests[req_idx].clone();
                                        let req_name = req.display_name();
                                        // Remove from source
                                        self.state.collections.get_mut(self.state.active_collection).unwrap().requests.remove(req_idx);
                                        self.persist_collection(self.state.active_collection);
                                        // Add to target
                                        let target_name = self.state.collections.get(target_coll).map(|c| c.name.clone()).unwrap_or_default();
                                        self.state.collections.get_mut(target_coll).unwrap().requests.push(req);
                                        self.persist_collection(target_coll);
                                        self.rebuild_collection_items();
                                        self.state.set_status(format!("Moved '{}' → '{}'", req_name, target_name));
                                    }
                                }
                            } else if target_coll == self.state.active_collection {
                                self.state.set_status("Cannot move to same collection");
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
                    _ => {}
                }
            }
            Action::OverlayBackspace => {
                match self.state.overlay {
                    Some(Overlay::NewCollection { ref mut name }) => { name.pop(); }
                    Some(Overlay::RenameRequest { ref mut name }) => { name.pop(); }
                    _ => {}
                }
            }

            // === Clipboard ===
            Action::CopyResponseBody => {
                if let Some(ref resp) = self.state.current_response {
                    if let Ok(mut clipboard) = arboard::Clipboard::new() {
                        let _ = clipboard.set_text(resp.formatted_body());
                        self.state.set_status("Response body copied");
                    }
                }
            }
            Action::CopyAsCurl => {
                let resolved = self.resolve_request(&self.state.current_request);
                let curl = http_client::to_curl(&resolved);
                if let Ok(mut clipboard) = arboard::Clipboard::new() {
                    let _ = clipboard.set_text(curl);
                    self.state.set_status("Curl command copied");
                }
            }

            // === Theme (direct set) ===
            Action::SetTheme(name) => {
                self.state.theme = crate::theme::load_theme(&name);
                self.state.set_status(format!("Theme: {}", self.state.theme.name));
            }

            // === Command Palette ===
            Action::OpenCommandPalette => {
                self.state.command_palette_open = true;
                self.state.command_palette_input.clear();
                self.state.command_palette_selected = 0;
            }
            Action::CommandPaletteClose => {
                self.state.command_palette_open = false;
            }
            Action::CommandPaletteInput(c) => {
                self.state.command_palette_input.push(c);
                self.state.command_palette_selected = 0;
            }
            Action::CommandPaletteBackspace => {
                self.state.command_palette_input.pop();
                self.state.command_palette_selected = 0;
            }
            Action::CommandPaletteUp => {
                self.state.command_palette_selected =
                    self.state.command_palette_selected.saturating_sub(1);
            }
            Action::CommandPaletteDown => {
                let count = crate::ui::command_palette::filtered_commands(
                    &self.state.command_palette_input,
                ).len();
                if count > 0 {
                    self.state.command_palette_selected =
                        (self.state.command_palette_selected + 1).min(count - 1);
                }
            }
            Action::CommandPaletteConfirm => {
                let matches = crate::ui::command_palette::filtered_commands(
                    &self.state.command_palette_input,
                );
                let selected = self.state.command_palette_selected;
                self.state.command_palette_open = false;
                if let Some(cmd) = matches.get(selected) {
                    let action = cmd.action.clone();
                    Box::pin(self.update(action)).await?;
                }
            }
        }
        Ok(())
    }

    // === Helpers ===

    fn position_body_cursor_at_end(&mut self) {
        let body = self.state.current_request.body.get_or_insert_with(String::new);
        let lines: Vec<&str> = body.lines().collect();
        if lines.is_empty() {
            self.state.body_cursor_row = 0;
            self.state.body_cursor_col = 0;
        } else {
            self.state.body_cursor_row = lines.len() - 1;
            self.state.body_cursor_col = lines.last().map(|l| l.len()).unwrap_or(0);
        }
    }

    fn position_request_cursor_at_end(&mut self) {
        match self.state.request_focus {
            RequestFocus::Url => {
                self.state.url_cursor = self.state.current_request.url.len();
            }
            RequestFocus::Header(idx) => {
                if let Some(h) = self.state.current_request.headers.get(idx) {
                    self.state.header_edit_cursor = if self.state.header_edit_field == 0 { h.name.len() } else { h.value.len() };
                }
            }
            RequestFocus::Param(idx) => {
                if let Some(p) = self.state.current_request.query_params.get(idx) {
                    self.state.param_edit_cursor = if self.state.param_edit_field == 0 { p.key.len() } else { p.value.len() };
                }
            }
            RequestFocus::Cookie(idx) => {
                if let Some(c) = self.state.current_request.cookies.get(idx) {
                    self.state.cookie_edit_cursor = if self.state.cookie_edit_field == 0 { c.name.len() } else { c.value.len() };
                }
            }
        }
    }

    fn paste_text_at_cursor(&mut self, text: &str) {
        if text.is_empty() { return; }
        let body = self.state.current_request.body.get_or_insert_with(String::new);
        let pos = row_col_to_offset(body, self.state.body_cursor_row, self.state.body_cursor_col);
        body.insert_str(pos, text);
        // Move cursor to end of pasted text
        let new_lines: usize = text.chars().filter(|c| *c == '\n').count();
        if new_lines > 0 {
            self.state.body_cursor_row += new_lines;
            let last_line = text.rsplit('\n').next().unwrap_or("");
            self.state.body_cursor_col = last_line.len();
        } else {
            self.state.body_cursor_col += text.len();
        }
    }

    fn save_current_request_over_selected(&mut self) {
        if let Some(flat_idx) = self.state.collections_state.selected() {
            if let Some(coll) = self.state.collections.get_mut(self.state.active_collection) {
                if flat_idx > 0 {
                    let req_idx = flat_idx - 1;
                    if req_idx < coll.requests.len() {
                        coll.requests[req_idx] = self.state.current_request.clone();
                        self.persist_collection(self.state.active_collection);
                        self.rebuild_collection_items();
                        self.state.set_status("Request saved");
                        return;
                    }
                }
            }
        }
        self.state.set_status("No request selected to overwrite");
    }

    fn save_current_request_as_new(&mut self) {
        if let Some(coll) = self.state.collections.get_mut(self.state.active_collection) {
            let mut new_req = self.state.current_request.clone();
            let name = new_req.name.as_deref().unwrap_or("Untitled");
            new_req.name = Some(format!("{} (copy)", name));
            coll.requests.push(new_req);
            self.persist_collection(self.state.active_collection);
            self.rebuild_collection_items();
            self.state.set_status("Saved as new request");
        }
    }

    /// Extract query params from the URL and merge them into the params list.
    /// Preserves existing disabled params and avoids duplicates.
    fn sync_params_from_url(&mut self) {
        let url = &self.state.current_request.url;
        if let Some((base, query)) = url.split_once('?') {
            let base_url = base.to_string();
            let url_params: Vec<(String, String)> = query
                .split('&')
                .filter_map(|pair| {
                    let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
                    if key.is_empty() { None } else { Some((key.to_string(), value.to_string())) }
                })
                .collect();

            // Merge: add new params from URL, mark existing ones
            let mut existing = self.state.current_request.query_params.clone();
            for (key, value) in &url_params {
                let found = existing.iter().any(|p| p.key == *key && p.value == *value);
                if !found {
                    existing.push(QueryParam {
                        key: key.clone(),
                        value: value.clone(),
                        enabled: true,
                    });
                }
            }

            self.state.current_request.query_params = existing;
            self.state.current_request.url = base_url;
        }
    }

    fn persist_collection(&self, idx: usize) {
        if let Some(coll) = self.state.collections.get(idx) {
            let content = crate::parser::http::serialize(&coll.requests);
            if let Err(e) = std::fs::write(&coll.path, &content) {
                // Can't set_status here since &self is immutable, but the file write error
                // is unlikely in practice. Log to stderr instead.
                eprintln!("Failed to save collection: {}", e);
            }
        }
    }

    fn switch_active_collection(&mut self) {
        self.rebuild_collection_items();
        self.state.collections_state.select(Some(0));
        if let Some(coll) = self.state.collections.get(self.state.active_collection) {
            if let Some(req) = coll.requests.first() {
                self.state.current_request = req.clone();
                self.state.current_response = None;
                self.state.last_error = None;
            }
            self.state.set_status(format!("Collection: {}", coll.name));
        }
    }

    fn inline_input(&mut self, c: char) {
        match self.state.active_panel {
            Panel::Body => {
                let body = self.state.current_request.body.get_or_insert_with(String::new);
                let pos = row_col_to_offset(body, self.state.body_cursor_row, self.state.body_cursor_col);
                body.insert(pos, c);
                self.state.body_cursor_col += 1;
            }
            Panel::Request => match self.state.request_focus {
                RequestFocus::Url => {
                    let cursor = self.state.url_cursor.min(self.state.current_request.url.len());
                    self.state.current_request.url.insert(cursor, c);
                    self.state.url_cursor = cursor + 1;
                    self.state.autocomplete = None;
                }
                RequestFocus::Header(idx) => {
                    if let Some(h) = self.state.current_request.headers.get_mut(idx) {
                        let field = if self.state.header_edit_field == 0 { &mut h.name } else { &mut h.value };
                        let cursor = self.state.header_edit_cursor.min(field.len());
                        field.insert(cursor, c);
                        self.state.header_edit_cursor = cursor + 1;
                    }
                    // Update autocomplete if editing header name
                    if self.state.header_edit_field == 0 {
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
                        let field = if self.state.param_edit_field == 0 { &mut p.key } else { &mut p.value };
                        let cursor = self.state.param_edit_cursor.min(field.len());
                        field.insert(cursor, c);
                        self.state.param_edit_cursor = cursor + 1;
                    }
                }
                RequestFocus::Cookie(idx) => {
                    if let Some(ck) = self.state.current_request.cookies.get_mut(idx) {
                        let field = if self.state.cookie_edit_field == 0 { &mut ck.name } else { &mut ck.value };
                        let cursor = self.state.cookie_edit_cursor.min(field.len());
                        field.insert(cursor, c);
                        self.state.cookie_edit_cursor = cursor + 1;
                    }
                }
            },
            _ => {}
        }
    }

    fn try_open_autocomplete(&mut self) {
        if self.state.active_panel == Panel::Request {
            if let RequestFocus::Header(idx) = self.state.request_focus {
                if self.state.header_edit_field == 0 {
                    if let Some(h) = self.state.current_request.headers.get(idx) {
                        let ac = crate::state::Autocomplete::new(&h.name);
                        self.state.autocomplete = if ac.is_empty() { None } else { Some(ac) };
                    }
                }
            }
        }
    }

    fn inline_backspace(&mut self) {
        match self.state.active_panel {
            Panel::Body => {
                let body = self.state.current_request.body.get_or_insert_with(String::new);
                let pos = row_col_to_offset(body, self.state.body_cursor_row, self.state.body_cursor_col);
                if pos > 0 {
                    let ch = body.as_bytes()[pos - 1];
                    body.remove(pos - 1);
                    if ch == b'\n' {
                        if self.state.body_cursor_row > 0 {
                            self.state.body_cursor_row -= 1;
                            let lines: Vec<&str> = body.lines().collect();
                            self.state.body_cursor_col = lines.get(self.state.body_cursor_row).map(|l| l.len()).unwrap_or(0);
                        }
                    } else {
                        self.state.body_cursor_col = self.state.body_cursor_col.saturating_sub(1);
                    }
                }
            }
            Panel::Request => match self.state.request_focus {
                RequestFocus::Url => {
                    if self.state.url_cursor > 0 {
                        self.state.url_cursor -= 1;
                        self.state.current_request.url.remove(self.state.url_cursor);
                    }
                }
                RequestFocus::Header(idx) => {
                    if self.state.header_edit_cursor > 0 {
                        if let Some(h) = self.state.current_request.headers.get_mut(idx) {
                            let field = if self.state.header_edit_field == 0 { &mut h.name } else { &mut h.value };
                            self.state.header_edit_cursor -= 1;
                            if self.state.header_edit_cursor < field.len() {
                                field.remove(self.state.header_edit_cursor);
                            }
                        }
                    }
                    // Update autocomplete after backspace
                    if self.state.header_edit_field == 0 {
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
                    if self.state.param_edit_cursor > 0 {
                        if let Some(p) = self.state.current_request.query_params.get_mut(idx) {
                            let field = if self.state.param_edit_field == 0 { &mut p.key } else { &mut p.value };
                            self.state.param_edit_cursor -= 1;
                            if self.state.param_edit_cursor < field.len() {
                                field.remove(self.state.param_edit_cursor);
                            }
                        }
                    }
                }
                RequestFocus::Cookie(idx) => {
                    if self.state.cookie_edit_cursor > 0 {
                        if let Some(ck) = self.state.current_request.cookies.get_mut(idx) {
                            let field = if self.state.cookie_edit_field == 0 { &mut ck.name } else { &mut ck.value };
                            self.state.cookie_edit_cursor -= 1;
                            if self.state.cookie_edit_cursor < field.len() {
                                field.remove(self.state.cookie_edit_cursor);
                            }
                        }
                    }
                }
            },
            _ => {}
        }
    }

    fn inline_delete(&mut self) {
        match self.state.active_panel {
            Panel::Body => {
                let body = self.state.current_request.body.get_or_insert_with(String::new);
                let pos = row_col_to_offset(body, self.state.body_cursor_row, self.state.body_cursor_col);
                if pos < body.len() { body.remove(pos); }
            }
            Panel::Request => match self.state.request_focus {
                RequestFocus::Url => {
                    if self.state.url_cursor < self.state.current_request.url.len() {
                        self.state.current_request.url.remove(self.state.url_cursor);
                    }
                }
                RequestFocus::Header(idx) => {
                    if let Some(h) = self.state.current_request.headers.get_mut(idx) {
                        let field = if self.state.header_edit_field == 0 { &mut h.name } else { &mut h.value };
                        if self.state.header_edit_cursor < field.len() { field.remove(self.state.header_edit_cursor); }
                    }
                }
                RequestFocus::Param(idx) => {
                    if let Some(p) = self.state.current_request.query_params.get_mut(idx) {
                        let field = if self.state.param_edit_field == 0 { &mut p.key } else { &mut p.value };
                        if self.state.param_edit_cursor < field.len() { field.remove(self.state.param_edit_cursor); }
                    }
                }
                RequestFocus::Cookie(idx) => {
                    if let Some(ck) = self.state.current_request.cookies.get_mut(idx) {
                        let field = if self.state.cookie_edit_field == 0 { &mut ck.name } else { &mut ck.value };
                        if self.state.cookie_edit_cursor < field.len() { field.remove(self.state.cookie_edit_cursor); }
                    }
                }
            },
            _ => {}
        }
    }

    fn inline_newline(&mut self) {
        if self.state.active_panel == Panel::Body {
            let body = self.state.current_request.body.get_or_insert_with(String::new);
            let pos = row_col_to_offset(body, self.state.body_cursor_row, self.state.body_cursor_col);
            body.insert(pos, '\n');
            self.state.body_cursor_row += 1;
            self.state.body_cursor_col = 0;
        }
    }

    fn inline_cursor_left(&mut self) {
        match self.state.active_panel {
            Panel::Body => {
                if self.state.body_cursor_col > 0 {
                    self.state.body_cursor_col -= 1;
                } else if self.state.body_cursor_row > 0 {
                    self.state.body_cursor_row -= 1;
                    let body = self.state.current_request.body.as_deref().unwrap_or("");
                    let lines: Vec<&str> = body.lines().collect();
                    self.state.body_cursor_col = lines.get(self.state.body_cursor_row).map(|l| l.len()).unwrap_or(0);
                }
            }
            Panel::Request => match self.state.request_focus {
                RequestFocus::Url => { self.state.url_cursor = self.state.url_cursor.saturating_sub(1); }
                RequestFocus::Header(_) => { self.state.header_edit_cursor = self.state.header_edit_cursor.saturating_sub(1); }
                RequestFocus::Param(_) => { self.state.param_edit_cursor = self.state.param_edit_cursor.saturating_sub(1); }
                RequestFocus::Cookie(_) => { self.state.cookie_edit_cursor = self.state.cookie_edit_cursor.saturating_sub(1); }
            },
            Panel::Response => {
                self.state.resp_cursor_col = self.state.resp_cursor_col.saturating_sub(1);
            }
            _ => {}
        }
    }

    fn inline_cursor_right(&mut self) {
        match self.state.active_panel {
            Panel::Body => {
                let body = self.state.current_request.body.as_deref().unwrap_or("");
                let lines: Vec<&str> = body.lines().collect();
                let line_len = lines.get(self.state.body_cursor_row).map(|l| l.len()).unwrap_or(0);
                if self.state.body_cursor_col < line_len {
                    self.state.body_cursor_col += 1;
                } else if self.state.body_cursor_row + 1 < lines.len() {
                    self.state.body_cursor_row += 1;
                    self.state.body_cursor_col = 0;
                }
            }
            Panel::Request => match self.state.request_focus {
                RequestFocus::Url => {
                    if self.state.url_cursor < self.state.current_request.url.len() { self.state.url_cursor += 1; }
                }
                RequestFocus::Header(idx) => {
                    if let Some(h) = self.state.current_request.headers.get(idx) {
                        let len = if self.state.header_edit_field == 0 { h.name.len() } else { h.value.len() };
                        if self.state.header_edit_cursor < len { self.state.header_edit_cursor += 1; }
                    }
                }
                RequestFocus::Param(idx) => {
                    if let Some(p) = self.state.current_request.query_params.get(idx) {
                        let len = if self.state.param_edit_field == 0 { p.key.len() } else { p.value.len() };
                        if self.state.param_edit_cursor < len { self.state.param_edit_cursor += 1; }
                    }
                }
                RequestFocus::Cookie(idx) => {
                    if let Some(ck) = self.state.current_request.cookies.get(idx) {
                        let len = if self.state.cookie_edit_field == 0 { ck.name.len() } else { ck.value.len() };
                        if self.state.cookie_edit_cursor < len { self.state.cookie_edit_cursor += 1; }
                    }
                }
            },
            Panel::Response => {
                let lines = self.get_response_lines();
                let line_len = lines.get(self.state.resp_cursor_row).map(|l| l.len()).unwrap_or(0);
                if self.state.resp_cursor_col < line_len {
                    self.state.resp_cursor_col += 1;
                }
            }
            _ => {}
        }
    }

    fn body_cursor_up(&mut self) {
        if self.state.body_cursor_row > 0 {
            self.state.body_cursor_row -= 1;
            let body = self.state.current_request.body.as_deref().unwrap_or("");
            let lines: Vec<&str> = body.lines().collect();
            let line_len = lines.get(self.state.body_cursor_row).map(|l| l.len()).unwrap_or(0);
            self.state.body_cursor_col = self.state.body_cursor_col.min(line_len);
        }
        // Scroll follow: keep cursor in viewport
        if (self.state.body_cursor_row as u16) < self.state.body_scroll.0 {
            self.state.body_scroll.0 = self.state.body_cursor_row as u16;
        }
    }

    fn body_cursor_down(&mut self) {
        let body = self.state.current_request.body.as_deref().unwrap_or("");
        let line_count = body.lines().count().max(1);
        if self.state.body_cursor_row + 1 < line_count {
            self.state.body_cursor_row += 1;
            let lines: Vec<&str> = body.lines().collect();
            let line_len = lines.get(self.state.body_cursor_row).map(|l| l.len()).unwrap_or(0);
            self.state.body_cursor_col = self.state.body_cursor_col.min(line_len);
        }
        // Scroll follow: keep cursor in viewport
        let visible = self.state.body_visible_height as usize;
        let scroll = self.state.body_scroll.0 as usize;
        if visible > 0 && self.state.body_cursor_row >= scroll + visible {
            self.state.body_scroll.0 = (self.state.body_cursor_row - visible + 1) as u16;
        }
    }

    fn inline_cursor_home(&mut self) {
        match self.state.active_panel {
            Panel::Body => self.state.body_cursor_col = 0,
            Panel::Response => self.state.resp_cursor_col = 0,
            Panel::Request => match self.state.request_focus {
                RequestFocus::Url => self.state.url_cursor = 0,
                RequestFocus::Header(_) => self.state.header_edit_cursor = 0,
                RequestFocus::Param(_) => self.state.param_edit_cursor = 0,
                RequestFocus::Cookie(_) => self.state.cookie_edit_cursor = 0,
            },
            _ => {}
        }
    }

    fn inline_cursor_end(&mut self) {
        match self.state.active_panel {
            Panel::Body => {
                let body = self.state.current_request.body.as_deref().unwrap_or("");
                let lines: Vec<&str> = body.lines().collect();
                self.state.body_cursor_col = lines.get(self.state.body_cursor_row).map(|l| l.len()).unwrap_or(0);
            }
            Panel::Response => {
                let lines = self.get_response_lines();
                self.state.resp_cursor_col = lines.get(self.state.resp_cursor_row).map(|l| l.len()).unwrap_or(0);
            }
            Panel::Request => match self.state.request_focus {
                RequestFocus::Url => self.state.url_cursor = self.state.current_request.url.len(),
                RequestFocus::Header(idx) => {
                    if let Some(h) = self.state.current_request.headers.get(idx) {
                        self.state.header_edit_cursor = if self.state.header_edit_field == 0 { h.name.len() } else { h.value.len() };
                    }
                }
                RequestFocus::Param(idx) => {
                    if let Some(p) = self.state.current_request.query_params.get(idx) {
                        self.state.param_edit_cursor = if self.state.param_edit_field == 0 { p.key.len() } else { p.value.len() };
                    }
                }
                RequestFocus::Cookie(idx) => {
                    if let Some(ck) = self.state.current_request.cookies.get(idx) {
                        self.state.cookie_edit_cursor = if self.state.cookie_edit_field == 0 { ck.name.len() } else { ck.value.len() };
                    }
                }
            },
            _ => {}
        }
    }

    fn inline_tab(&mut self) {
        match self.state.active_panel {
            Panel::Request => {
                // If autocomplete is open, accept it instead of tabbing
                if let Some(ac) = self.state.autocomplete.take() {
                    if let Some((name, value)) = ac.accept() {
                        if let RequestFocus::Header(idx) = self.state.request_focus {
                            if let Some(h) = self.state.current_request.headers.get_mut(idx) {
                                h.name = name.to_string();
                                h.value = value.to_string();
                            }
                        }
                    }
                }
                self.state.autocomplete = None;
                match self.state.request_focus {
                    RequestFocus::Header(idx) => {
                        self.state.header_edit_field = (self.state.header_edit_field + 1) % 2;
                        if let Some(h) = self.state.current_request.headers.get(idx) {
                            self.state.header_edit_cursor = if self.state.header_edit_field == 0 { h.name.len() } else { h.value.len() };
                        }
                    }
                    RequestFocus::Param(idx) => {
                        self.state.param_edit_field = (self.state.param_edit_field + 1) % 2;
                        if let Some(p) = self.state.current_request.query_params.get(idx) {
                            self.state.param_edit_cursor = if self.state.param_edit_field == 0 { p.key.len() } else { p.value.len() };
                        }
                    }
                    RequestFocus::Cookie(idx) => {
                        self.state.cookie_edit_field = (self.state.cookie_edit_field + 1) % 2;
                        if let Some(ck) = self.state.current_request.cookies.get(idx) {
                            self.state.cookie_edit_cursor = if self.state.cookie_edit_field == 0 { ck.name.len() } else { ck.value.len() };
                        }
                    }
                    _ => {}
                }
            }
            Panel::Body => {
                let body = self.state.current_request.body.get_or_insert_with(String::new);
                let pos = row_col_to_offset(body, self.state.body_cursor_row, self.state.body_cursor_col);
                body.insert_str(pos, "  ");
                self.state.body_cursor_col += 2;
            }
            _ => {}
        }
    }

    fn body_word_forward(&mut self) {
        let (text, cursor_row, cursor_col) = match self.state.active_panel {
            Panel::Response => {
                let t = self.get_response_body_text();
                (t, &mut self.state.resp_cursor_row as *mut usize, &mut self.state.resp_cursor_col as *mut usize)
            }
            _ => {
                let t = self.state.current_request.body.as_deref().unwrap_or("").to_string();
                (t, &mut self.state.body_cursor_row as *mut usize, &mut self.state.body_cursor_col as *mut usize)
            }
        };
        let lines: Vec<&str> = text.lines().collect();
        // SAFETY: we're just using raw pointers to avoid borrow issues within the same struct
        unsafe {
            if let Some(line) = lines.get(*cursor_row) {
                let bytes = line.as_bytes();
                let mut col = *cursor_col;
                while col < bytes.len() && !bytes[col].is_ascii_whitespace() { col += 1; }
                while col < bytes.len() && bytes[col].is_ascii_whitespace() { col += 1; }
                if col >= bytes.len() && *cursor_row + 1 < lines.len() {
                    *cursor_row += 1;
                    *cursor_col = 0;
                } else {
                    *cursor_col = col.min(bytes.len());
                }
            }
        }
    }

    fn body_word_backward(&mut self) {
        let (text, cursor_row, cursor_col) = match self.state.active_panel {
            Panel::Response => {
                let t = self.get_response_body_text();
                (t, &mut self.state.resp_cursor_row as *mut usize, &mut self.state.resp_cursor_col as *mut usize)
            }
            _ => {
                let t = self.state.current_request.body.as_deref().unwrap_or("").to_string();
                (t, &mut self.state.body_cursor_row as *mut usize, &mut self.state.body_cursor_col as *mut usize)
            }
        };
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
                while col > 0 && bytes[col].is_ascii_whitespace() { col -= 1; }
                while col > 0 && !bytes[col - 1].is_ascii_whitespace() { col -= 1; }
                *cursor_col = col;
            }
        }
    }

    fn get_visual_selection(&self) -> String {
        let body = self.state.current_request.body.as_deref().unwrap_or("");
        let (sr, sc, er, ec) = self.visual_range();
        let start = row_col_to_offset(body, sr, sc);
        let end = row_col_to_offset(body, er, ec).min(body.len());
        if start <= end { body[start..end].to_string() } else { String::new() }
    }

    fn delete_visual_selection(&mut self) {
        let (sr, sc, er, ec) = self.visual_range();
        let body = self.state.current_request.body.get_or_insert_with(String::new);
        let start = row_col_to_offset(body, sr, sc);
        let end = row_col_to_offset(body, er, ec).min(body.len());
        if start < end { body.drain(start..end); }
        self.state.body_cursor_row = sr;
        self.state.body_cursor_col = sc;
    }

    fn visual_range(&self) -> (usize, usize, usize, usize) {
        let (ar, ac) = (self.state.visual_anchor_row, self.state.visual_anchor_col);
        let (cr, cc) = (self.state.body_cursor_row, self.state.body_cursor_col);
        if (ar, ac) <= (cr, cc) { (ar, ac, cr, cc) } else { (cr, cc, ar, ac) }
    }

    fn delete_body_line(&mut self, row: usize) {
        let body = self.state.current_request.body.get_or_insert_with(String::new);
        let mut lines: Vec<String> = body.lines().map(|l| l.to_string()).collect();
        if row < lines.len() {
            lines.remove(row);
            *body = lines.join("\n");
            let max_row = body.lines().count().saturating_sub(1);
            self.state.body_cursor_row = self.state.body_cursor_row.min(max_row);
            let cur_line_len = body.lines().nth(self.state.body_cursor_row).map(|l| l.len()).unwrap_or(0);
            self.state.body_cursor_col = self.state.body_cursor_col.min(cur_line_len);
        }
    }

    async fn execute_request(&mut self) {
        // Allow re-sending: cancel conceptually the old one
        self.state.request_in_flight = true;
        self.state.last_error = None;
        self.state.set_status("Sending request...");

        let mut resolved = self.resolve_request(&self.state.current_request);

        // Auto-inject Content-Type if body exists and no Content-Type header set
        let body_text = resolved.body.as_deref().unwrap_or("").trim();
        if !body_text.is_empty() {
            let has_ct = resolved.headers.iter().any(|h| h.enabled && h.name.eq_ignore_ascii_case("content-type"));
            if !has_ct {
                resolved.headers.push(Header {
                    name: "Content-Type".to_string(),
                    value: self.state.body_type.content_type().to_string(),
                    enabled: true,
                });
            }
        }

        // Trim body — don't send empty
        if body_text.is_empty() {
            resolved.body = None;
        }

        let config = self.state.config.general.clone();
        let tx = self.action_tx.clone();

        tokio::spawn(async move {
            match http_client::execute(&resolved, &config).await {
                Ok(resp) => { let _ = tx.send(Action::RequestCompleted(Box::new(resp))); }
                Err(e) => { let _ = tx.send(Action::RequestFailed(e.to_string())); }
            }
        });
    }

    fn resolve_request(&self, req: &Request) -> Request {
        let env = &self.state.environments;
        Request {
            name: req.name.clone(),
            method: req.method,
            url: env.resolve(&req.url),
            headers: req.headers.iter().map(|h| Header { name: h.name.clone(), value: env.resolve(&h.value), enabled: h.enabled }).collect(),
            query_params: req.query_params.iter().map(|p| crate::model::request::QueryParam { key: p.key.clone(), value: env.resolve(&p.value), enabled: p.enabled }).collect(),
            cookies: req.cookies.iter().map(|c| crate::model::request::Cookie { name: c.name.clone(), value: env.resolve(&c.value), enabled: c.enabled }).collect(),
            body: req.body.as_ref().map(|b| env.resolve(b)),
            source_file: req.source_file.clone(),
            source_line: req.source_line,
        }
    }

    fn scroll_down(&mut self) {
        match self.state.active_panel {
            Panel::Collections => {
                let i = self.state.collections_state.selected().map(|i| i + 1).unwrap_or(0);
                let max = self.state.collection_items.len().saturating_sub(1);
                self.state.collections_state.select(Some(i.min(max)));
            }
            Panel::Body => self.body_cursor_down(),
            Panel::Response => self.resp_cursor_down(),
            _ => {}
        }
    }

    fn scroll_up(&mut self) {
        match self.state.active_panel {
            Panel::Collections => {
                let i = self.state.collections_state.selected().unwrap_or(0).saturating_sub(1);
                self.state.collections_state.select(Some(i));
            }
            Panel::Body => self.body_cursor_up(),
            Panel::Response => self.resp_cursor_up(),
            _ => {}
        }
    }

    fn scroll_top(&mut self) {
        match self.state.active_panel {
            Panel::Collections => self.state.collections_state.select(Some(0)),
            Panel::Body => { self.state.body_scroll = (0, 0); self.state.body_cursor_row = 0; self.state.body_cursor_col = 0; }
            Panel::Response => {
                self.state.resp_cursor_row = 0;
                self.state.resp_cursor_col = 0;
                self.state.response_scroll = (0, 0);
            }
            _ => {}
        }
    }

    fn scroll_bottom(&mut self) {
        match self.state.active_panel {
            Panel::Collections => {
                let max = self.state.collection_items.len().saturating_sub(1);
                self.state.collections_state.select(Some(max));
            }
            Panel::Body => {
                let body = self.state.current_request.body.as_deref().unwrap_or("");
                let lines: Vec<&str> = body.lines().collect();
                self.state.body_cursor_row = lines.len().saturating_sub(1);
                self.state.body_cursor_col = lines.last().map(|l| l.len()).unwrap_or(0);
            }
            Panel::Response => {
                let lines = self.get_response_lines();
                self.state.resp_cursor_row = lines.len().saturating_sub(1);
                self.state.resp_cursor_col = 0;
            }
            _ => {}
        }
    }

    fn scroll_half_down(&mut self) {
        let half = 15usize;
        match self.state.active_panel {
            Panel::Body => {
                let body = self.state.current_request.body.as_deref().unwrap_or("");
                let max = body.lines().count().saturating_sub(1);
                self.state.body_cursor_row = (self.state.body_cursor_row + half).min(max);
                self.state.body_scroll.0 = self.state.body_scroll.0.saturating_add(half as u16);
            }
            Panel::Response => {
                let max = self.get_response_lines().len().saturating_sub(1);
                self.state.resp_cursor_row = (self.state.resp_cursor_row + half).min(max);
                self.state.response_scroll.0 = self.state.response_scroll.0.saturating_add(half as u16);
            }
            _ => {}
        }
    }

    fn scroll_half_up(&mut self) {
        let half = 15usize;
        match self.state.active_panel {
            Panel::Body => {
                self.state.body_cursor_row = self.state.body_cursor_row.saturating_sub(half);
                self.state.body_scroll.0 = self.state.body_scroll.0.saturating_sub(half as u16);
            }
            Panel::Response => {
                self.state.resp_cursor_row = self.state.resp_cursor_row.saturating_sub(half);
                self.state.response_scroll.0 = self.state.response_scroll.0.saturating_sub(half as u16);
            }
            _ => {}
        }
    }

    // === Response cursor helpers ===

    fn get_response_body_text(&self) -> String {
        if let Some(ref resp) = self.state.current_response {
            resp.formatted_body()
        } else {
            String::new()
        }
    }

    fn get_response_lines(&self) -> Vec<String> {
        self.get_response_body_text().lines().map(|l| l.to_string()).collect()
    }

    fn resp_cursor_down(&mut self) {
        let lines = self.get_response_lines();
        if self.state.resp_cursor_row + 1 < lines.len() {
            self.state.resp_cursor_row += 1;
            let line_len = lines.get(self.state.resp_cursor_row).map(|l| l.len()).unwrap_or(0);
            self.state.resp_cursor_col = self.state.resp_cursor_col.min(line_len);
        }
        // Keep cursor in viewport (scroll follows cursor)
        let scroll = self.state.response_scroll.0 as usize;
        let visible = self.state.resp_visible_height as usize;
        if visible > 0 && self.state.resp_cursor_row >= scroll + visible {
            self.state.response_scroll.0 = (self.state.resp_cursor_row - visible + 1) as u16;
        }
    }

    fn resp_cursor_up(&mut self) {
        if self.state.resp_cursor_row > 0 {
            self.state.resp_cursor_row -= 1;
            let lines = self.get_response_lines();
            let line_len = lines.get(self.state.resp_cursor_row).map(|l| l.len()).unwrap_or(0);
            self.state.resp_cursor_col = self.state.resp_cursor_col.min(line_len);
        }
        // Keep cursor in viewport
        if (self.state.resp_cursor_row as u16) < self.state.response_scroll.0 {
            self.state.response_scroll.0 = self.state.resp_cursor_row as u16;
        }
    }

    fn get_response_visual_selection(&self) -> String {
        let body = self.get_response_body_text();
        let (sr, sc, er, ec) = self.resp_visual_range();
        let start = row_col_to_offset(&body, sr, sc);
        let end = row_col_to_offset(&body, er, ec).min(body.len());
        if start <= end { body[start..end].to_string() } else { String::new() }
    }

    fn resp_visual_range(&self) -> (usize, usize, usize, usize) {
        let (ar, ac) = (self.state.resp_visual_anchor_row, self.state.resp_visual_anchor_col);
        let (cr, cc) = (self.state.resp_cursor_row, self.state.resp_cursor_col);
        if (ar, ac) <= (cr, cc) { (ar, ac, cr, cc) } else { (cr, cc, ar, ac) }
    }

    fn select_request_by_flat_index(&mut self, flat_idx: usize) {
        if let Some(coll) = self.state.collections.get(self.state.active_collection) {
            if flat_idx > 0 {
                let req_idx = flat_idx - 1;
                if let Some(req) = coll.requests.get(req_idx) {
                    self.state.current_request = req.clone();
                    self.state.current_response = None;
                    self.state.last_error = None;
                    return;
                }
            }
        }
    }
}

fn row_col_to_offset(text: &str, row: usize, col: usize) -> usize {
    let mut offset = 0;
    for (i, line) in text.split('\n').enumerate() {
        if i == row { return offset + col.min(line.len()); }
        offset += line.len() + 1;
    }
    text.len()
}
