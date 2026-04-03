mod autocomplete;
mod body_edit;
mod clipboard_ops;
mod collections;
mod execute;
mod inline_edit;
mod mode;
mod overlay;
mod request_field;
mod scroll;
mod search;
mod vim_sync;

use body_edit::{vim_editor_word_forward, vim_editor_word_backward, vim_editor_word_end};

use anyhow::Result;
use tokio::sync::mpsc;

use crate::action::Action;
use crate::config::AppConfig;
use crate::event::{AppEvent, EventHandler};
use crate::keybindings;
use crate::model::request::{Header, PathParam, QueryParam, Request};
use crate::parser;
use crate::state::{AppState, InputMode, Overlay, Panel, RequestFocus, RequestTab, ResponseTab, COMMON_HEADERS, WIDE_LAYOUT_THRESHOLD, STATUS_MESSAGE_TTL, PENDING_KEY_TIMEOUT, EVENT_TICK_RATE};
use crate::tui::Tui;
use crate::ui;
use vimltui::{VimMode, VisualKind};
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
            Action::EnterInsertMode | Action::EnterInsertModeStart | Action::EnterAppendMode | Action::EnterAppendModeEnd |
            Action::ExitInsertMode | Action::EnterRequestFieldEdit | Action::ExitRequestFieldEdit |
            Action::EnterVisualMode | Action::EnterVisualBlockMode | Action::ExitVisualMode => {
                return self.handle_mode_transition(action, count);
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

            // === Clipboard Operations (Visual Yank/Delete/Paste, Yank, Delete, Change, etc.) ===
            Action::VisualYank | Action::VisualDelete | Action::VisualPaste |
            Action::Paste | Action::PasteFromClipboard |
            Action::YankLine | Action::YankWord | Action::YankToEnd | Action::YankToStart | Action::YankToBottom |
            Action::DeleteLine | Action::DeleteCharUnderCursor | Action::DeleteWord | Action::DeleteWordEnd | Action::DeleteWordBack | Action::DeleteToEnd | Action::DeleteToStart | Action::DeleteToBottom |
            Action::ChangeLine | Action::ChangeWord | Action::ChangeWordBack | Action::ChangeToEnd | Action::ChangeToStart |
            Action::ReplaceChar(_) | Action::Substitute |
            Action::CopyResponseBody | Action::CopyAsCurl => {
                return self.handle_clipboard_ops(action, count);
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
            Action::OpenOverlay(_) | Action::CloseOverlay |
            Action::OverlayUp | Action::OverlayDown | Action::OverlayConfirm |
            Action::OverlayInput(_) | Action::OverlayBackspace | Action::OverlayDelete => {
                return self.handle_overlay(action, count);
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
