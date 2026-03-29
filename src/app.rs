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
use crate::state::{AppState, BodyType, InputMode, Overlay, Panel, RequestFocus, RequestTab, ResponseTab, COMMON_HEADERS};
use crate::tui::Tui;
use crate::ui;
use crossterm::cursor::SetCursorStyle;

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

    fn active_body(&self) -> &str {
        let req = &self.state.current_request;
        match self.state.body_type {
            BodyType::Json => req.body_json.as_deref().unwrap_or(""),
            BodyType::Xml => req.body_xml.as_deref().unwrap_or(""),
            BodyType::FormUrlEncoded => req.body_form.as_deref().unwrap_or(""),
            BodyType::Plain => req.body_raw.as_deref().unwrap_or(""),
        }
    }

    fn set_active_body(&mut self, value: Option<String>) {
        let req = &mut self.state.current_request;
        match self.state.body_type {
            BodyType::Json => req.body_json = value,
            BodyType::Xml => req.body_xml = value,
            BodyType::FormUrlEncoded => req.body_form = value,
            BodyType::Plain => req.body_raw = value,
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
        let filter = self.state.collections_filter.to_lowercase();
        let has_filter = !filter.is_empty();
        let mut items = Vec::new();
        for (ci, collection) in self.state.collections.iter().enumerate() {
            let expanded = self.state.expanded_collections.contains(&ci);

            if has_filter {
                // Check if any request in this collection matches the filter
                let matching_requests: Vec<&crate::model::request::Request> = collection.requests.iter()
                    .filter(|req| {
                        req.display_name().to_lowercase().contains(&filter)
                            || req.url.to_lowercase().contains(&filter)
                    })
                    .collect();
                if matching_requests.is_empty() {
                    continue; // skip collections with no matching requests
                }
                let arrow = if expanded { "▼" } else { "▶" };
                let marker = if ci == self.state.active_collection { "●" } else { "○" };
                items.push(format!("{} {} {}", arrow, marker, collection.display_name()));
                if expanded {
                    for req in &collection.requests {
                        let name_match = req.display_name().to_lowercase().contains(&filter);
                        let url_match = req.url.to_lowercase().contains(&filter);
                        if name_match || url_match {
                            items.push(format!("  {} {}", req.method, req.display_name()));
                        }
                    }
                }
            } else {
                let arrow = if expanded { "▼" } else { "▶" };
                let marker = if ci == self.state.active_collection { "●" } else { "○" };
                items.push(format!("{} {} {}", arrow, marker, collection.display_name()));
                if expanded {
                    for req in &collection.requests {
                        items.push(format!("  {} {}", req.method, req.display_name()));
                    }
                }
            }
        }
        self.state.collection_items = items;
    }

    /// Maps a flat list index to (collection_index, Option<request_index>).
    /// Returns None if out of bounds.
    fn flat_idx_to_coll_req(&self, flat_idx: usize) -> Option<(usize, Option<usize>)> {
        let filter = self.state.collections_filter.to_lowercase();
        let has_filter = !filter.is_empty();
        let mut idx = 0;
        for (ci, collection) in self.state.collections.iter().enumerate() {
            if has_filter {
                let has_match = collection.requests.iter().any(|req| {
                    req.display_name().to_lowercase().contains(&filter)
                        || req.url.to_lowercase().contains(&filter)
                });
                if !has_match {
                    continue;
                }
            }
            if idx == flat_idx {
                return Some((ci, None)); // collection header
            }
            idx += 1;
            if self.state.expanded_collections.contains(&ci) {
                for ri in 0..collection.requests.len() {
                    if has_filter {
                        let req = &collection.requests[ri];
                        let name_match = req.display_name().to_lowercase().contains(&filter);
                        let url_match = req.url.to_lowercase().contains(&filter);
                        if !name_match && !url_match {
                            continue;
                        }
                    }
                    if idx == flat_idx {
                        return Some((ci, Some(ri)));
                    }
                    idx += 1;
                }
            }
        }
        None
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

                // Calculate visible widths for horizontal scroll-follow
                // Body and response panels share the right side; subtract gutter (4) + border (2)
                if right_width > 120 {
                    // Wide: center panel is ~40% of right
                    self.state.body_visible_width = (right_width as u32 * 40 / 100) as u16;
                    self.state.resp_visible_width = (right_width as u32 * 50 / 100) as u16;
                } else {
                    // Narrow: body/response take full right width
                    self.state.body_visible_width = right_width;
                    self.state.resp_visible_width = right_width;
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
                self.state.request_field_editing = false;
            }
            Action::FocusPanel(panel) => {
                self.state.active_panel = panel;
                self.state.mode = InputMode::Normal;
                self.state.request_field_editing = false;
            }

            // === Vim Mode Transitions ===
            Action::EnterInsertMode => {
                match self.state.active_panel {
                    Panel::Body => {
                        self.push_body_undo();
                        self.state.mode = InputMode::Insert;
                        self.position_body_cursor_at_end();
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
                    _ => {}
                }
            }
            Action::EnterInsertModeStart => {
                match self.state.active_panel {
                    Panel::Body => {
                        self.push_body_undo();
                        self.state.mode = InputMode::Insert;
                        self.state.body_cursor_col = 0;
                    }
                    Panel::Request => {
                        self.push_request_undo();
                        self.state.mode = InputMode::Insert;
                        self.state.request_field_editing = true;
                        self.set_request_cursor(0);
                    }
                    _ => {}
                }
            }
            Action::EnterAppendMode => {
                match self.state.active_panel {
                    Panel::Body => {
                        self.push_body_undo();
                        self.state.mode = InputMode::Insert;
                        let body = { let bt = self.state.body_type; match bt { BodyType::Json => self.state.current_request.body_json.as_deref().unwrap_or(""), BodyType::Xml => self.state.current_request.body_xml.as_deref().unwrap_or(""), BodyType::FormUrlEncoded => self.state.current_request.body_form.as_deref().unwrap_or(""), BodyType::Plain => self.state.current_request.body_raw.as_deref().unwrap_or("") } };
                        let lines: Vec<&str> = body.lines().collect();
                        let line_len = lines.get(self.state.body_cursor_row).map(|l| l.len()).unwrap_or(0);
                        self.state.body_cursor_col = (self.state.body_cursor_col + 1).min(line_len);
                    }
                    Panel::Request => {
                        self.push_request_undo();
                        self.state.mode = InputMode::Insert;
                        self.state.request_field_editing = true;
                        let cursor = self.get_request_cursor();
                        let len = self.get_request_field_len();
                        self.set_request_cursor((cursor + 1).min(len));
                    }
                    _ => {}
                }
            }
            Action::EnterAppendModeEnd => {
                match self.state.active_panel {
                    Panel::Body => {
                        self.push_body_undo();
                        self.state.mode = InputMode::Insert;
                        let body = { let bt = self.state.body_type; match bt { BodyType::Json => self.state.current_request.body_json.as_deref().unwrap_or(""), BodyType::Xml => self.state.current_request.body_xml.as_deref().unwrap_or(""), BodyType::FormUrlEncoded => self.state.current_request.body_form.as_deref().unwrap_or(""), BodyType::Plain => self.state.current_request.body_raw.as_deref().unwrap_or("") } };
                        let lines: Vec<&str> = body.lines().collect();
                        let line_len = lines.get(self.state.body_cursor_row).map(|l| l.len()).unwrap_or(0);
                        self.state.body_cursor_col = line_len;
                    }
                    Panel::Request => {
                        self.push_request_undo();
                        self.state.mode = InputMode::Insert;
                        self.state.request_field_editing = true;
                        let len = self.get_request_field_len();
                        self.set_request_cursor(len);
                    }
                    _ => {}
                }
            }
            Action::OpenLineBelow => {
                if self.state.active_panel == Panel::Body {
                    self.push_body_undo();
                    let bt = self.state.body_type; let body = match bt { BodyType::Json => self.state.current_request.body_json.get_or_insert_with(String::new), BodyType::Xml => self.state.current_request.body_xml.get_or_insert_with(String::new), BodyType::FormUrlEncoded => self.state.current_request.body_form.get_or_insert_with(String::new), BodyType::Plain => self.state.current_request.body_raw.get_or_insert_with(String::new) };
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
                    self.push_body_undo();
                    let bt = self.state.body_type; let body = match bt { BodyType::Json => self.state.current_request.body_json.get_or_insert_with(String::new), BodyType::Xml => self.state.current_request.body_xml.get_or_insert_with(String::new), BodyType::FormUrlEncoded => self.state.current_request.body_form.get_or_insert_with(String::new), BodyType::Plain => self.state.current_request.body_raw.get_or_insert_with(String::new) };
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
                // Clamp cursor to last char (normal mode can't be past end)
                match self.state.active_panel {
                    Panel::Body => {
                        let body = { let bt = self.state.body_type; match bt { BodyType::Json => self.state.current_request.body_json.as_deref().unwrap_or(""), BodyType::Xml => self.state.current_request.body_xml.as_deref().unwrap_or(""), BodyType::FormUrlEncoded => self.state.current_request.body_form.as_deref().unwrap_or(""), BodyType::Plain => self.state.current_request.body_raw.as_deref().unwrap_or("") } };
                        let lines: Vec<&str> = body.lines().collect();
                        let line_len = lines.get(self.state.body_cursor_row).map(|l| l.len()).unwrap_or(0);
                        if line_len > 0 {
                            self.state.body_cursor_col = self.state.body_cursor_col.min(line_len - 1);
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
                        self.state.visual_anchor_row = self.state.body_cursor_row;
                        self.state.visual_anchor_col = self.state.body_cursor_col;
                    }
                    Panel::Response => {
                        self.state.mode = InputMode::Visual;
                        self.state.resp_visual_anchor_row = self.state.resp_cursor_row;
                        self.state.resp_visual_anchor_col = self.state.resp_cursor_col;
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
                        self.state.visual_anchor_row = self.state.body_cursor_row;
                        self.state.visual_anchor_col = self.state.body_cursor_col;
                    }
                    Panel::Response => {
                        self.state.mode = InputMode::VisualBlock;
                        self.state.resp_visual_anchor_row = self.state.resp_cursor_row;
                        self.state.resp_visual_anchor_col = self.state.resp_cursor_col;
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
                self.state.body_cursor_row = 0;
                self.state.body_cursor_col = 0;
                self.state.request_focus = RequestFocus::Url;
                self.state.set_status("New empty request");
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
            Action::InlineInput(c) => self.inline_input(c),
            Action::InlineBackspace => self.inline_backspace(),
            Action::InlineDelete => self.inline_delete(),
            Action::InlineNewline => self.inline_newline(),
            Action::InlineCursorLeft => { for _ in 0..count { self.inline_cursor_left(); } }
            Action::InlineCursorRight => { for _ in 0..count { self.inline_cursor_right(); } }
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

            // === Body/Request Vim Motions ===
            Action::BodyWordForward => {
                for _ in 0..count {
                    if self.state.active_panel == Panel::Request && self.state.request_field_editing {
                        self.request_word_forward();
                    } else {
                        self.body_word_forward();
                    }
                }
            }
            Action::BodyWordBackward => {
                for _ in 0..count {
                    if self.state.active_panel == Panel::Request && self.state.request_field_editing {
                        self.request_word_backward();
                    } else {
                        self.body_word_backward();
                    }
                }
            }
            Action::BodyWordEnd => {
                for _ in 0..count {
                    if self.state.active_panel == Panel::Request && self.state.request_field_editing {
                        self.request_word_end();
                    } else {
                        self.body_word_end();
                    }
                }
            }
            Action::BodyLineHome => {
                if self.state.active_panel == Panel::Request && self.state.request_field_editing {
                    self.set_request_cursor(0);
                } else {
                    self.state.body_cursor_col = 0;
                }
            }
            Action::BodyLineEnd => self.inline_cursor_end(),

            // === Visual Mode ===
            Action::VisualYank => {
                let is_block = self.state.mode == InputMode::VisualBlock;
                let text = match self.state.active_panel {
                    Panel::Body if is_block => Some(self.get_block_selection()),
                    Panel::Body => Some(self.get_visual_selection()),
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
            Action::Paste => {
                if self.state.active_panel == Panel::Body {
                    self.push_body_undo();
                    let paste = self.state.yank_buffer.clone();
                    self.paste_text_at_cursor(&paste);
                } else if self.state.active_panel == Panel::Request && self.state.request_field_editing {
                    self.push_request_undo();
                    let paste = self.state.yank_buffer.clone();
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
                        let body = { let bt = self.state.body_type; match bt { BodyType::Json => self.state.current_request.body_json.as_deref().unwrap_or(""), BodyType::Xml => self.state.current_request.body_xml.as_deref().unwrap_or(""), BodyType::FormUrlEncoded => self.state.current_request.body_form.as_deref().unwrap_or(""), BodyType::Plain => self.state.current_request.body_raw.as_deref().unwrap_or("") } };
                        let lines: Vec<&str> = body.lines().collect();
                        if let Some(line) = lines.get(self.state.body_cursor_row) {
                            let line_text = line.to_string();
                            self.state.yank_buffer = format!("{}\n", line_text);
                            let _ = crate::clipboard::copy_to_clipboard(&line_text);
                            self.state.set_status("Yanked line");
                        }
                    }
                    Panel::Response => {
                        // yy on response: copy the current line
                        let text = self.get_response_body_text();
                        let lines: Vec<&str> = text.lines().collect();
                        if let Some(line) = lines.get(self.state.resp_cursor_row) {
                            let line_text = line.to_string();
                            self.state.yank_buffer = format!("{}\n", line_text);
                            let _ = crate::clipboard::copy_to_clipboard(&line_text);
                            self.state.set_status("Yanked line");
                        }
                    }
                    _ => {}
                }
            }
            Action::DeleteLine => {
                self.state.pending_key = None;
                if self.state.active_panel == Panel::Body {
                    let body = { let bt = self.state.body_type; match bt { BodyType::Json => self.state.current_request.body_json.as_deref().unwrap_or(""), BodyType::Xml => self.state.current_request.body_xml.as_deref().unwrap_or(""), BodyType::FormUrlEncoded => self.state.current_request.body_form.as_deref().unwrap_or(""), BodyType::Plain => self.state.current_request.body_raw.as_deref().unwrap_or("") } };
                    let lines: Vec<&str> = body.lines().collect();
                    if let Some(line) = lines.get(self.state.body_cursor_row) {
                        self.state.yank_buffer = format!("{}\n", line);
                        let _ = crate::clipboard::copy_to_clipboard(&self.state.yank_buffer);
                        self.push_body_undo();
                        self.delete_body_line(self.state.body_cursor_row);
                        self.state.set_status("Line deleted");
                    }
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
                if self.state.active_panel == Panel::Body {
                    self.push_body_undo();
                    let bt = self.state.body_type; let body = match bt { BodyType::Json => self.state.current_request.body_json.get_or_insert_with(String::new), BodyType::Xml => self.state.current_request.body_xml.get_or_insert_with(String::new), BodyType::FormUrlEncoded => self.state.current_request.body_form.get_or_insert_with(String::new), BodyType::Plain => self.state.current_request.body_raw.get_or_insert_with(String::new) };
                    let pos = row_col_to_offset(body, self.state.body_cursor_row, self.state.body_cursor_col);
                    if pos < body.len() {
                        let ch = body.as_bytes()[pos];
                        if ch != b'\n' {
                            body.remove(pos);
                            // Clamp cursor if at end of line now
                            let lines: Vec<&str> = body.lines().collect();
                            let line_len = lines.get(self.state.body_cursor_row).map(|l| l.len()).unwrap_or(0);
                            self.state.body_cursor_col = self.state.body_cursor_col.min(line_len.saturating_sub(1).max(0));
                        }
                    }
                } else if self.state.active_panel == Panel::Request && self.state.request_field_editing {
                    self.push_request_undo();
                    self.delete_request_char_under_cursor();
                }
            }
            Action::ReplaceChar(c) => {
                self.state.pending_key = None;
                if self.state.active_panel == Panel::Body {
                    self.push_body_undo();
                    let bt = self.state.body_type; let body = match bt { BodyType::Json => self.state.current_request.body_json.get_or_insert_with(String::new), BodyType::Xml => self.state.current_request.body_xml.get_or_insert_with(String::new), BodyType::FormUrlEncoded => self.state.current_request.body_form.get_or_insert_with(String::new), BodyType::Plain => self.state.current_request.body_raw.get_or_insert_with(String::new) };
                    let pos = row_col_to_offset(body, self.state.body_cursor_row, self.state.body_cursor_col);
                    if pos < body.len() && body.as_bytes()[pos] != b'\n' {
                        body.remove(pos);
                        body.insert(pos, c);
                    }
                }
            }
            Action::ChangeLine => {
                self.state.pending_key = None;
                if self.state.active_panel == Panel::Body {
                    self.push_body_undo();
                    let bt = self.state.body_type; let body = match bt { BodyType::Json => self.state.current_request.body_json.get_or_insert_with(String::new), BodyType::Xml => self.state.current_request.body_xml.get_or_insert_with(String::new), BodyType::FormUrlEncoded => self.state.current_request.body_form.get_or_insert_with(String::new), BodyType::Plain => self.state.current_request.body_raw.get_or_insert_with(String::new) };
                    let lines: Vec<&str> = body.lines().collect();
                    let row = self.state.body_cursor_row;
                    if row < lines.len() {
                        let line_text = lines[row].to_string();
                        self.state.yank_buffer = line_text.clone();
                        let _ = crate::clipboard::copy_to_clipboard(&line_text);
                        // Replace line content with empty
                        let offset = row_col_to_offset(body, row, 0);
                        let end = offset + lines[row].len();
                        body.drain(offset..end);
                    }
                    self.state.body_cursor_col = 0;
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
                if self.state.active_panel == Panel::Body {
                    self.push_body_undo();
                    let body_text = self.active_body().to_string();
                    let lines: Vec<&str> = body_text.lines().collect();
                    let row = self.state.body_cursor_row;
                    if let Some(line) = lines.get(row) {
                        let bytes = line.as_bytes();
                        let col = self.state.body_cursor_col;
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
                        let bt = self.state.body_type; let body = match bt { BodyType::Json => self.state.current_request.body_json.get_or_insert_with(String::new), BodyType::Xml => self.state.current_request.body_xml.get_or_insert_with(String::new), BodyType::FormUrlEncoded => self.state.current_request.body_form.get_or_insert_with(String::new), BodyType::Plain => self.state.current_request.body_raw.get_or_insert_with(String::new) };
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
                if self.state.active_panel == Panel::Body {
                    self.push_body_undo();
                    let body_text = self.active_body().to_string();
                    let lines: Vec<&str> = body_text.lines().collect();
                    let row = self.state.body_cursor_row;
                    if let Some(line) = lines.get(row) {
                        let bytes = line.as_bytes();
                        let col = self.state.body_cursor_col;
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
                        let bt = self.state.body_type; let body = match bt { BodyType::Json => self.state.current_request.body_json.get_or_insert_with(String::new), BodyType::Xml => self.state.current_request.body_xml.get_or_insert_with(String::new), BodyType::FormUrlEncoded => self.state.current_request.body_form.get_or_insert_with(String::new), BodyType::Plain => self.state.current_request.body_raw.get_or_insert_with(String::new) };
                        let start = row_col_to_offset(body, row, start_col);
                        let end = row_col_to_offset(body, row, col);
                        body.drain(start..end);
                        self.state.body_cursor_col = start_col;
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
                if self.state.active_panel == Panel::Body {
                    self.push_body_undo();
                    let bt = self.state.body_type; let body = match bt { BodyType::Json => self.state.current_request.body_json.get_or_insert_with(String::new), BodyType::Xml => self.state.current_request.body_xml.get_or_insert_with(String::new), BodyType::FormUrlEncoded => self.state.current_request.body_form.get_or_insert_with(String::new), BodyType::Plain => self.state.current_request.body_raw.get_or_insert_with(String::new) };
                    let lines: Vec<&str> = body.lines().collect();
                    let row = self.state.body_cursor_row;
                    let col = self.state.body_cursor_col;
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
                if self.state.active_panel == Panel::Body {
                    self.push_body_undo();
                    let bt = self.state.body_type; let body = match bt { BodyType::Json => self.state.current_request.body_json.get_or_insert_with(String::new), BodyType::Xml => self.state.current_request.body_xml.get_or_insert_with(String::new), BodyType::FormUrlEncoded => self.state.current_request.body_form.get_or_insert_with(String::new), BodyType::Plain => self.state.current_request.body_raw.get_or_insert_with(String::new) };
                    let pos = row_col_to_offset(body, self.state.body_cursor_row, self.state.body_cursor_col);
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
                if self.state.active_panel == Panel::Body {
                    self.push_body_undo();
                    let body_text = self.active_body().to_string();
                    let lines: Vec<&str> = body_text.lines().collect();
                    let row = self.state.body_cursor_row;
                    if let Some(line) = lines.get(row) {
                        let bytes = line.as_bytes();
                        let col = self.state.body_cursor_col;
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
                        let bt = self.state.body_type; let body = match bt { BodyType::Json => self.state.current_request.body_json.get_or_insert_with(String::new), BodyType::Xml => self.state.current_request.body_xml.get_or_insert_with(String::new), BodyType::FormUrlEncoded => self.state.current_request.body_form.get_or_insert_with(String::new), BodyType::Plain => self.state.current_request.body_raw.get_or_insert_with(String::new) };
                        let start = row_col_to_offset(body, row, col);
                        let end = row_col_to_offset(body, row, end_col);
                        body.drain(start..end);
                        // Clamp cursor
                        let lines2: Vec<&str> = body.lines().collect();
                        let line_len = lines2.get(row).map(|l| l.len()).unwrap_or(0);
                        self.state.body_cursor_col = col.min(line_len.saturating_sub(1).max(0));
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
                if self.state.active_panel == Panel::Body {
                    self.push_body_undo();
                    let body_text = self.active_body().to_string();
                    let lines: Vec<&str> = body_text.lines().collect();
                    let row = self.state.body_cursor_row;
                    if let Some(line) = lines.get(row) {
                        let bytes = line.as_bytes();
                        let col = self.state.body_cursor_col;
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
                        let bt = self.state.body_type; let body = match bt { BodyType::Json => self.state.current_request.body_json.get_or_insert_with(String::new), BodyType::Xml => self.state.current_request.body_xml.get_or_insert_with(String::new), BodyType::FormUrlEncoded => self.state.current_request.body_form.get_or_insert_with(String::new), BodyType::Plain => self.state.current_request.body_raw.get_or_insert_with(String::new) };
                        let start = row_col_to_offset(body, row, col);
                        let end = row_col_to_offset(body, row, end_col);
                        body.drain(start..end);
                        let lines2: Vec<&str> = body.lines().collect();
                        let line_len = lines2.get(row).map(|l| l.len()).unwrap_or(0);
                        self.state.body_cursor_col = col.min(line_len.saturating_sub(1).max(0));
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
                if self.state.active_panel == Panel::Body {
                    self.push_body_undo();
                    let body_text = self.active_body().to_string();
                    let lines: Vec<&str> = body_text.lines().collect();
                    let row = self.state.body_cursor_row;
                    if let Some(line) = lines.get(row) {
                        let bytes = line.as_bytes();
                        let col = self.state.body_cursor_col;
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
                        let bt = self.state.body_type; let body = match bt { BodyType::Json => self.state.current_request.body_json.get_or_insert_with(String::new), BodyType::Xml => self.state.current_request.body_xml.get_or_insert_with(String::new), BodyType::FormUrlEncoded => self.state.current_request.body_form.get_or_insert_with(String::new), BodyType::Plain => self.state.current_request.body_raw.get_or_insert_with(String::new) };
                        let start = row_col_to_offset(body, row, start_col);
                        let end = row_col_to_offset(body, row, col);
                        body.drain(start..end);
                        self.state.body_cursor_col = start_col;
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
                if self.state.active_panel == Panel::Body {
                    let body = { let bt = self.state.body_type; match bt { BodyType::Json => self.state.current_request.body_json.as_deref().unwrap_or(""), BodyType::Xml => self.state.current_request.body_xml.as_deref().unwrap_or(""), BodyType::FormUrlEncoded => self.state.current_request.body_form.as_deref().unwrap_or(""), BodyType::Plain => self.state.current_request.body_raw.as_deref().unwrap_or("") } };
                    let lines: Vec<&str> = body.lines().collect();
                    let row = self.state.body_cursor_row;
                    if let Some(line) = lines.get(row) {
                        let bytes = line.as_bytes();
                        let col = self.state.body_cursor_col;
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
                if self.state.active_panel == Panel::Body {
                    if let Some((snapshot, row, col)) = self.state.body_undo_stack.pop() {
                        let current_body = self.active_body().to_string();
                        self.state.body_redo_stack.push((current_body, self.state.body_cursor_row, self.state.body_cursor_col));
                        self.set_active_body(if snapshot.is_empty() { None } else { Some(snapshot) });
                        self.state.body_cursor_row = row;
                        self.state.body_cursor_col = col;
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
                if self.state.active_panel == Panel::Body {
                    if let Some((snapshot, row, col)) = self.state.body_redo_stack.pop() {
                        let current_body = self.active_body().to_string();
                        self.state.body_undo_stack.push((current_body, self.state.body_cursor_row, self.state.body_cursor_col));
                        self.set_active_body(if snapshot.is_empty() { None } else { Some(snapshot) });
                        self.state.body_cursor_row = row;
                        self.state.body_cursor_col = col;
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
                self.state.body_cursor_row = 0;
                self.state.body_cursor_col = 0;
                self.state.body_undo_stack.clear();
                self.state.body_redo_stack.clear();
                self.state.validate_body();
            }
            Action::BodyPrevTab => {
                self.state.body_type = self.state.body_type.prev();
                self.state.body_cursor_row = 0;
                self.state.body_cursor_col = 0;
                self.state.body_undo_stack.clear();
                self.state.body_redo_stack.clear();
                self.state.validate_body();
            }

            // === Response Tabs ===
            Action::ResponseNextTab => {
                self.state.response_tab = self.state.response_tab.next();
            }
            Action::ResponsePrevTab => {
                self.state.response_tab = self.state.response_tab.prev();
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

                // Cache response for request chaining
                if let Some(ref name) = self.state.current_request.name {
                    let collection_name = self.state.collections
                        .get(self.state.active_collection)
                        .map(|c| c.name.as_str())
                        .unwrap_or("_");
                    let key = format!("{}/{}", collection_name, name);
                    self.state.response_cache.insert(key, ((*response).clone(), std::time::Instant::now()));
                    // Cap response cache at 50 entries, evicting oldest first
                    while self.state.response_cache.len() > 50 {
                        if let Some(oldest_key) = self.state.response_cache.iter()
                            .min_by_key(|(_, (_, ts))| *ts)
                            .map(|(k, _)| k.clone())
                        {
                            self.state.response_cache.remove(&oldest_key);
                        } else {
                            break;
                        }
                    }
                }

                self.state.current_response = Some(*response);
                self.state.response_scroll = (0, 0);

                // Infer type from JSON response
                if let Some(ref resp) = self.state.current_response {
                    if let Ok(json_val) = serde_json::from_str::<serde_json::Value>(&resp.body) {
                        self.state.response_type = Some(crate::model::response_type::JsonType::infer(&json_val));
                    } else {
                        self.state.response_type = None;
                    }
                }
                self.state.type_scroll = 0;

                self.state.set_status(format!("{} - {}", status, elapsed));
            }
            Action::RequestFailed(err) => {
                self.state.request_in_flight = false;
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

            // === Overlays ===
            Action::OpenOverlay(overlay) => {
                if matches!(overlay, Overlay::EnvironmentSelector) {
                    self.state.env_selector_state.select(Some(self.state.environments.active.unwrap_or(0)));
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

            // === Response Headers Inspector ===
            Action::ToggleResponseHeaders => {
                self.state.response_headers_expanded = !self.state.response_headers_expanded;
                self.state.response_headers_scroll = 0;
            }

            // === Search ===
            Action::StartSearch => {
                self.state.search_active = true;
                self.state.search_query.clear();
                self.state.search_matches.clear();
                self.state.search_match_idx = 0;
            }
            Action::SearchInput(c) => {
                self.state.search_query.push(c);
                self.recalculate_search_matches();
            }
            Action::SearchBackspace => {
                self.state.search_query.pop();
                self.recalculate_search_matches();
            }
            Action::SearchConfirm => {
                self.state.search_active = false;
                // Keep matches highlighted and current position
            }
            Action::SearchCancel => {
                self.state.search_active = false;
                self.state.search_query.clear();
                self.state.search_matches.clear();
                self.state.search_match_idx = 0;
            }
            Action::SearchNext => {
                if !self.state.search_matches.is_empty() {
                    self.state.search_match_idx =
                        (self.state.search_match_idx + 1) % self.state.search_matches.len();
                    self.jump_to_current_search_match();
                }
            }
            Action::SearchPrev => {
                if !self.state.search_matches.is_empty() {
                    let len = self.state.search_matches.len();
                    self.state.search_match_idx =
                        (self.state.search_match_idx + len - 1) % len;
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

    // === Helpers ===

    fn recalculate_search_matches(&mut self) {
        self.state.search_matches.clear();
        self.state.search_match_idx = 0;
        if self.state.search_query.is_empty() {
            return;
        }
        let query = self.state.search_query.to_lowercase();
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
                self.state.search_matches.push((row, start + pos));
                start += pos + 1;
            }
        }
        // Jump to first match
        if !self.state.search_matches.is_empty() {
            self.jump_to_current_search_match();
        }
    }

    fn jump_to_current_search_match(&mut self) {
        if let Some(&(row, col)) = self.state.search_matches.get(self.state.search_match_idx) {
            match self.state.active_panel {
                Panel::Response => {
                    self.state.resp_cursor_row = row;
                    self.state.resp_cursor_col = col;
                    // Scroll to make the match visible
                    let visible = self.state.resp_visible_height as usize;
                    if row < self.state.response_scroll.0 as usize {
                        self.state.response_scroll.0 = row as u16;
                    } else if row >= self.state.response_scroll.0 as usize + visible {
                        self.state.response_scroll.0 = (row.saturating_sub(visible / 2)) as u16;
                    }
                }
                Panel::Body => {
                    self.state.body_cursor_row = row;
                    self.state.body_cursor_col = col;
                    let visible = self.state.body_visible_height as usize;
                    if row < self.state.body_scroll.0 as usize {
                        self.state.body_scroll.0 = row as u16;
                    } else if row >= self.state.body_scroll.0 as usize + visible {
                        self.state.body_scroll.0 = (row.saturating_sub(visible / 2)) as u16;
                    }
                }
                _ => {}
            }
        }
    }

    /// Save a snapshot of the body for undo. Call before any body mutation.
    fn push_body_undo(&mut self) {
        let body = self.active_body().to_string();
        self.state.body_undo_stack.push((body, self.state.body_cursor_row, self.state.body_cursor_col));
        self.state.body_redo_stack.clear(); // new edit clears redo history
        // Cap undo history at 100 entries
        if self.state.body_undo_stack.len() > 100 {
            self.state.body_undo_stack.remove(0);
        }
    }

    /// Save a snapshot of the current request field for undo.
    fn push_request_undo(&mut self) {
        let focus = self.state.request_focus;
        let edit_field = match focus {
            RequestFocus::Header(_) => self.state.header_edit_field,
            RequestFocus::Param(_) => self.state.param_edit_field,
            RequestFocus::Cookie(_) => self.state.cookie_edit_field,
            RequestFocus::PathParam(_) => self.state.path_param_edit_field,
            RequestFocus::Url => 0,
        };
        let text = self.get_request_field_text();
        let cursor = self.get_request_cursor();
        self.state.request_undo_stack.push((focus, edit_field, text, cursor));
        self.state.request_redo_stack.clear();
        if self.state.request_undo_stack.len() > 100 {
            self.state.request_undo_stack.remove(0);
        }
    }

    /// Restore a request field from an undo/redo snapshot.
    fn set_request_field_text(&mut self, focus: RequestFocus, edit_field: u8, text: String) {
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

    fn position_body_cursor_at_end(&mut self) {
        let bt = self.state.body_type; let body = match bt { BodyType::Json => self.state.current_request.body_json.get_or_insert_with(String::new), BodyType::Xml => self.state.current_request.body_xml.get_or_insert_with(String::new), BodyType::FormUrlEncoded => self.state.current_request.body_form.get_or_insert_with(String::new), BodyType::Plain => self.state.current_request.body_raw.get_or_insert_with(String::new) };
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
        let len = self.get_request_field_len();
        // In normal mode, cursor sits on last char; in insert mode, after last char
        let end = if self.state.mode == InputMode::Insert { len } else { len.saturating_sub(1) };
        self.set_request_cursor(end);
    }


    fn paste_text_at_cursor(&mut self, text: &str) {
        if text.is_empty() { return; }
        let bt = self.state.body_type; let body = match bt { BodyType::Json => self.state.current_request.body_json.get_or_insert_with(String::new), BodyType::Xml => self.state.current_request.body_xml.get_or_insert_with(String::new), BodyType::FormUrlEncoded => self.state.current_request.body_form.get_or_insert_with(String::new), BodyType::Plain => self.state.current_request.body_raw.get_or_insert_with(String::new) };
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
            if let Some((ci, Some(ri))) = self.flat_idx_to_coll_req(flat_idx) {
                if let Some(coll) = self.state.collections.get_mut(ci) {
                    if ri < coll.requests.len() {
                        coll.requests[ri] = self.state.current_request.clone();
                        self.persist_collection(ci);
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
        self.state.expanded_collections.insert(self.state.active_collection);
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
                let bt = self.state.body_type; let body = match bt { BodyType::Json => self.state.current_request.body_json.get_or_insert_with(String::new), BodyType::Xml => self.state.current_request.body_xml.get_or_insert_with(String::new), BodyType::FormUrlEncoded => self.state.current_request.body_form.get_or_insert_with(String::new), BodyType::Plain => self.state.current_request.body_raw.get_or_insert_with(String::new) };
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
                RequestFocus::PathParam(idx) => {
                    if let Some(p) = self.state.current_request.path_params.get_mut(idx) {
                        let field = if self.state.path_param_edit_field == 0 { &mut p.key } else { &mut p.value };
                        let cursor = self.state.path_param_edit_cursor.min(field.len());
                        field.insert(cursor, c);
                        self.state.path_param_edit_cursor = cursor + 1;
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
                let bt = self.state.body_type; let body = match bt { BodyType::Json => self.state.current_request.body_json.get_or_insert_with(String::new), BodyType::Xml => self.state.current_request.body_xml.get_or_insert_with(String::new), BodyType::FormUrlEncoded => self.state.current_request.body_form.get_or_insert_with(String::new), BodyType::Plain => self.state.current_request.body_raw.get_or_insert_with(String::new) };
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
                RequestFocus::PathParam(idx) => {
                    if self.state.path_param_edit_cursor > 0 {
                        if let Some(p) = self.state.current_request.path_params.get_mut(idx) {
                            let field = if self.state.path_param_edit_field == 0 { &mut p.key } else { &mut p.value };
                            self.state.path_param_edit_cursor -= 1;
                            if self.state.path_param_edit_cursor < field.len() {
                                field.remove(self.state.path_param_edit_cursor);
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
                let bt = self.state.body_type; let body = match bt { BodyType::Json => self.state.current_request.body_json.get_or_insert_with(String::new), BodyType::Xml => self.state.current_request.body_xml.get_or_insert_with(String::new), BodyType::FormUrlEncoded => self.state.current_request.body_form.get_or_insert_with(String::new), BodyType::Plain => self.state.current_request.body_raw.get_or_insert_with(String::new) };
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
                RequestFocus::PathParam(idx) => {
                    if let Some(p) = self.state.current_request.path_params.get_mut(idx) {
                        let field = if self.state.path_param_edit_field == 0 { &mut p.key } else { &mut p.value };
                        if self.state.path_param_edit_cursor < field.len() { field.remove(self.state.path_param_edit_cursor); }
                    }
                }
            },
            _ => {}
        }
    }

    fn inline_newline(&mut self) {
        if self.state.active_panel == Panel::Body {
            let bt = self.state.body_type; let body = match bt { BodyType::Json => self.state.current_request.body_json.get_or_insert_with(String::new), BodyType::Xml => self.state.current_request.body_xml.get_or_insert_with(String::new), BodyType::FormUrlEncoded => self.state.current_request.body_form.get_or_insert_with(String::new), BodyType::Plain => self.state.current_request.body_raw.get_or_insert_with(String::new) };
            let pos = row_col_to_offset(body, self.state.body_cursor_row, self.state.body_cursor_col);

            // Determine indent: copy leading whitespace from current line
            let lines: Vec<&str> = body.lines().collect();
            let current_line = lines.get(self.state.body_cursor_row).copied().unwrap_or("");
            let leading_ws: String = current_line.chars().take_while(|c| c.is_whitespace()).collect();

            // Check if char before cursor is { or [ for extra indent
            let char_before = if pos > 0 { body.as_bytes().get(pos - 1).copied() } else { None };
            let extra_indent = match char_before {
                Some(b'{') | Some(b'[') => "  ",
                _ => "",
            };

            let indent = format!("\n{}{}", leading_ws, extra_indent);
            body.insert_str(pos, &indent);
            self.state.body_cursor_row += 1;
            self.state.body_cursor_col = leading_ws.len() + extra_indent.len();
        }
    }

    fn inline_cursor_left(&mut self) {
        match self.state.active_panel {
            Panel::Body => {
                if self.state.body_cursor_col > 0 {
                    self.state.body_cursor_col -= 1;
                } else if self.state.body_cursor_row > 0 {
                    self.state.body_cursor_row -= 1;
                    let body = { let bt = self.state.body_type; match bt { BodyType::Json => self.state.current_request.body_json.as_deref().unwrap_or(""), BodyType::Xml => self.state.current_request.body_xml.as_deref().unwrap_or(""), BodyType::FormUrlEncoded => self.state.current_request.body_form.as_deref().unwrap_or(""), BodyType::Plain => self.state.current_request.body_raw.as_deref().unwrap_or("") } };
                    let lines: Vec<&str> = body.lines().collect();
                    self.state.body_cursor_col = lines.get(self.state.body_cursor_row).map(|l| l.len()).unwrap_or(0);
                }
            }
            Panel::Request => match self.state.request_focus {
                RequestFocus::Url => { self.state.url_cursor = self.state.url_cursor.saturating_sub(1); }
                RequestFocus::Header(_) => { self.state.header_edit_cursor = self.state.header_edit_cursor.saturating_sub(1); }
                RequestFocus::Param(_) => { self.state.param_edit_cursor = self.state.param_edit_cursor.saturating_sub(1); }
                RequestFocus::Cookie(_) => { self.state.cookie_edit_cursor = self.state.cookie_edit_cursor.saturating_sub(1); }
                RequestFocus::PathParam(_) => { self.state.path_param_edit_cursor = self.state.path_param_edit_cursor.saturating_sub(1); }
            },
            Panel::Response => {
                self.state.resp_cursor_col = self.state.resp_cursor_col.saturating_sub(1);
            }
            _ => {}
        }
        // Sync horizontal scroll after cursor movement
        match self.state.active_panel {
            Panel::Body => { self.sync_body_hscroll(); }
            Panel::Response => { self.sync_resp_hscroll(); }
            _ => {}
        }
    }

    fn inline_cursor_right(&mut self) {
        let is_insert = self.state.mode == InputMode::Insert;
        match self.state.active_panel {
            Panel::Body => {
                let body = { let bt = self.state.body_type; match bt { BodyType::Json => self.state.current_request.body_json.as_deref().unwrap_or(""), BodyType::Xml => self.state.current_request.body_xml.as_deref().unwrap_or(""), BodyType::FormUrlEncoded => self.state.current_request.body_form.as_deref().unwrap_or(""), BodyType::Plain => self.state.current_request.body_raw.as_deref().unwrap_or("") } };
                let lines: Vec<&str> = body.lines().collect();
                let line_len = lines.get(self.state.body_cursor_row).map(|l| l.len()).unwrap_or(0);
                // In normal mode, cursor stays on last char (max = len-1)
                let max = if is_insert { line_len } else { line_len.saturating_sub(1) };
                if self.state.body_cursor_col < max {
                    self.state.body_cursor_col += 1;
                } else if is_insert && self.state.body_cursor_row + 1 < lines.len() {
                    self.state.body_cursor_row += 1;
                    self.state.body_cursor_col = 0;
                }
            }
            Panel::Request => {
                let len = self.get_request_field_len();
                let max = if is_insert { len } else { len.saturating_sub(1) };
                let cursor = self.get_request_cursor();
                if cursor < max {
                    self.set_request_cursor(cursor + 1);
                }
            }
            Panel::Response => {
                let lines = self.get_response_lines();
                let line_len = lines.get(self.state.resp_cursor_row).map(|l| l.len()).unwrap_or(0);
                let max = line_len.saturating_sub(1); // response is always "normal mode"
                if self.state.resp_cursor_col < max {
                    self.state.resp_cursor_col += 1;
                }
            }
            _ => {}
        }
        // Sync horizontal scroll after cursor movement
        match self.state.active_panel {
            Panel::Body => { self.sync_body_hscroll(); }
            Panel::Response => { self.sync_resp_hscroll(); }
            _ => {}
        }
    }

    fn body_cursor_up(&mut self) {
        if self.state.body_cursor_row > 0 {
            self.state.body_cursor_row -= 1;
            let body = { let bt = self.state.body_type; match bt { BodyType::Json => self.state.current_request.body_json.as_deref().unwrap_or(""), BodyType::Xml => self.state.current_request.body_xml.as_deref().unwrap_or(""), BodyType::FormUrlEncoded => self.state.current_request.body_form.as_deref().unwrap_or(""), BodyType::Plain => self.state.current_request.body_raw.as_deref().unwrap_or("") } };
            let lines: Vec<&str> = body.lines().collect();
            let line_len = lines.get(self.state.body_cursor_row).map(|l| l.len()).unwrap_or(0);
            let max = if self.state.mode == InputMode::Insert { line_len } else { line_len.saturating_sub(1) };
            self.state.body_cursor_col = self.state.body_cursor_col.min(max);
        }
        self.sync_body_scroll(); self.sync_body_hscroll();
    }

    fn body_cursor_down(&mut self) {
        let body = { let bt = self.state.body_type; match bt { BodyType::Json => self.state.current_request.body_json.as_deref().unwrap_or(""), BodyType::Xml => self.state.current_request.body_xml.as_deref().unwrap_or(""), BodyType::FormUrlEncoded => self.state.current_request.body_form.as_deref().unwrap_or(""), BodyType::Plain => self.state.current_request.body_raw.as_deref().unwrap_or("") } };
        let line_count = body.lines().count().max(1);
        if self.state.body_cursor_row + 1 < line_count {
            self.state.body_cursor_row += 1;
            let lines: Vec<&str> = body.lines().collect();
            let line_len = lines.get(self.state.body_cursor_row).map(|l| l.len()).unwrap_or(0);
            // In normal mode, clamp to last char; in insert mode, allow end position
            let max = if self.state.mode == InputMode::Insert { line_len } else { line_len.saturating_sub(1) };
            self.state.body_cursor_col = self.state.body_cursor_col.min(max);
        }
        self.sync_body_scroll(); self.sync_body_hscroll();
    }

    fn inline_cursor_home(&mut self) {
        match self.state.active_panel {
            Panel::Body => { self.state.body_cursor_col = 0; self.sync_body_hscroll(); },
            Panel::Response => { self.state.resp_cursor_col = 0; self.sync_resp_hscroll(); },
            Panel::Request => match self.state.request_focus {
                RequestFocus::Url => self.state.url_cursor = 0,
                RequestFocus::Header(_) => self.state.header_edit_cursor = 0,
                RequestFocus::Param(_) => self.state.param_edit_cursor = 0,
                RequestFocus::Cookie(_) => self.state.cookie_edit_cursor = 0,
                RequestFocus::PathParam(_) => self.state.path_param_edit_cursor = 0,
            },
            _ => {}
        }
    }

    fn inline_cursor_end(&mut self) {
        let is_insert = self.state.mode == InputMode::Insert;
        match self.state.active_panel {
            Panel::Body => {
                let body = { let bt = self.state.body_type; match bt { BodyType::Json => self.state.current_request.body_json.as_deref().unwrap_or(""), BodyType::Xml => self.state.current_request.body_xml.as_deref().unwrap_or(""), BodyType::FormUrlEncoded => self.state.current_request.body_form.as_deref().unwrap_or(""), BodyType::Plain => self.state.current_request.body_raw.as_deref().unwrap_or("") } };
                let lines: Vec<&str> = body.lines().collect();
                let line_len = lines.get(self.state.body_cursor_row).map(|l| l.len()).unwrap_or(0);
                self.state.body_cursor_col = if is_insert { line_len } else { line_len.saturating_sub(1) };
                self.sync_body_hscroll();
            }
            Panel::Response => {
                let lines = self.get_response_lines();
                let line_len = lines.get(self.state.resp_cursor_row).map(|l| l.len()).unwrap_or(0);
                self.state.resp_cursor_col = line_len.saturating_sub(1);
                self.sync_resp_hscroll();
            }
            Panel::Request => {
                let len = self.get_request_field_len();
                let end = if is_insert { len } else { len.saturating_sub(1) };
                self.set_request_cursor(end);
            }
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
                // Toggle between name/value sub-field
                match self.state.request_focus {
                    RequestFocus::Header(_) => self.state.header_edit_field = (self.state.header_edit_field + 1) % 2,
                    RequestFocus::Param(_) => self.state.param_edit_field = (self.state.param_edit_field + 1) % 2,
                    RequestFocus::Cookie(_) => self.state.cookie_edit_field = (self.state.cookie_edit_field + 1) % 2,
                    RequestFocus::PathParam(_) => self.state.path_param_edit_field = (self.state.path_param_edit_field + 1) % 2,
                    _ => {}
                }
                // Position cursor at end of new sub-field
                self.position_request_cursor_at_end();
            }
            Panel::Body => {
                let bt = self.state.body_type; let body = match bt { BodyType::Json => self.state.current_request.body_json.get_or_insert_with(String::new), BodyType::Xml => self.state.current_request.body_xml.get_or_insert_with(String::new), BodyType::FormUrlEncoded => self.state.current_request.body_form.get_or_insert_with(String::new), BodyType::Plain => self.state.current_request.body_raw.get_or_insert_with(String::new) };
                let pos = row_col_to_offset(body, self.state.body_cursor_row, self.state.body_cursor_col);
                body.insert_str(pos, "  ");
                self.state.body_cursor_col += 2;
            }
            _ => {}
        }
    }

    fn body_cursor_ptrs(&mut self) -> (String, *mut usize, *mut usize) {
        match self.state.active_panel {
            Panel::Response => {
                let t = self.get_response_body_text();
                (t, &mut self.state.resp_cursor_row as *mut usize, &mut self.state.resp_cursor_col as *mut usize)
            }
            _ => {
                let t = self.active_body().to_string();
                (t, &mut self.state.body_cursor_row as *mut usize, &mut self.state.body_cursor_col as *mut usize)
            }
        }
    }

    fn body_word_forward(&mut self) {
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

    fn body_word_backward(&mut self) {
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

    fn body_word_end(&mut self) {
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

    // === Request field-edit helpers ===

    fn get_request_cursor(&self) -> usize {
        match self.state.request_focus {
            RequestFocus::Url => self.state.url_cursor,
            RequestFocus::Header(_) => self.state.header_edit_cursor,
            RequestFocus::Param(_) => self.state.param_edit_cursor,
            RequestFocus::Cookie(_) => self.state.cookie_edit_cursor,
            RequestFocus::PathParam(_) => self.state.path_param_edit_cursor,
        }
    }

    fn set_request_cursor(&mut self, pos: usize) {
        match self.state.request_focus {
            RequestFocus::Url => self.state.url_cursor = pos,
            RequestFocus::Header(_) => self.state.header_edit_cursor = pos,
            RequestFocus::Param(_) => self.state.param_edit_cursor = pos,
            RequestFocus::Cookie(_) => self.state.cookie_edit_cursor = pos,
            RequestFocus::PathParam(_) => self.state.path_param_edit_cursor = pos,
        }
    }

    fn get_request_field_len(&self) -> usize {
        self.get_request_field_text().len()
    }

    fn get_request_field_text(&self) -> String {
        match self.state.request_focus {
            RequestFocus::Url => self.state.current_request.url.clone(),
            RequestFocus::Header(idx) => {
                self.state.current_request.headers.get(idx).map(|h| {
                    if self.state.header_edit_field == 0 { h.name.clone() } else { h.value.clone() }
                }).unwrap_or_default()
            }
            RequestFocus::Param(idx) => {
                self.state.current_request.query_params.get(idx).map(|p| {
                    if self.state.param_edit_field == 0 { p.key.clone() } else { p.value.clone() }
                }).unwrap_or_default()
            }
            RequestFocus::Cookie(idx) => {
                self.state.current_request.cookies.get(idx).map(|c| {
                    if self.state.cookie_edit_field == 0 { c.name.clone() } else { c.value.clone() }
                }).unwrap_or_default()
            }
            RequestFocus::PathParam(idx) => {
                self.state.current_request.path_params.get(idx).map(|p| {
                    if self.state.path_param_edit_field == 0 { p.key.clone() } else { p.value.clone() }
                }).unwrap_or_default()
            }
        }
    }

    fn clear_request_field(&mut self) {
        match self.state.request_focus {
            RequestFocus::Url => self.state.current_request.url.clear(),
            RequestFocus::Header(idx) => {
                if let Some(h) = self.state.current_request.headers.get_mut(idx) {
                    if self.state.header_edit_field == 0 { h.name.clear(); } else { h.value.clear(); }
                }
            }
            RequestFocus::Param(idx) => {
                if let Some(p) = self.state.current_request.query_params.get_mut(idx) {
                    if self.state.param_edit_field == 0 { p.key.clear(); } else { p.value.clear(); }
                }
            }
            RequestFocus::Cookie(idx) => {
                if let Some(c) = self.state.current_request.cookies.get_mut(idx) {
                    if self.state.cookie_edit_field == 0 { c.name.clear(); } else { c.value.clear(); }
                }
            }
            RequestFocus::PathParam(idx) => {
                if let Some(p) = self.state.current_request.path_params.get_mut(idx) {
                    if self.state.path_param_edit_field == 0 { p.key.clear(); } else { p.value.clear(); }
                }
            }
        }
    }

    /// Drain a range [start..end) from the currently focused request field.
    fn drain_request_field(&mut self, start: usize, end: usize) {
        if start >= end { return; }
        match self.state.request_focus {
            RequestFocus::Url => { self.state.current_request.url.drain(start..end); }
            RequestFocus::Header(idx) => {
                if let Some(h) = self.state.current_request.headers.get_mut(idx) {
                    let field = if self.state.header_edit_field == 0 { &mut h.name } else { &mut h.value };
                    field.drain(start..end);
                }
            }
            RequestFocus::Param(idx) => {
                if let Some(p) = self.state.current_request.query_params.get_mut(idx) {
                    let field = if self.state.param_edit_field == 0 { &mut p.key } else { &mut p.value };
                    field.drain(start..end);
                }
            }
            RequestFocus::Cookie(idx) => {
                if let Some(c) = self.state.current_request.cookies.get_mut(idx) {
                    let field = if self.state.cookie_edit_field == 0 { &mut c.name } else { &mut c.value };
                    field.drain(start..end);
                }
            }
            RequestFocus::PathParam(idx) => {
                if let Some(p) = self.state.current_request.path_params.get_mut(idx) {
                    let field = if self.state.path_param_edit_field == 0 { &mut p.key } else { &mut p.value };
                    field.drain(start..end);
                }
            }
        }
    }

    fn get_request_visual_selection(&self) -> String {
        let text = self.get_request_field_text();
        let cursor = self.get_request_cursor();
        let anchor = self.state.request_visual_anchor;
        let start = cursor.min(anchor);
        let end = (cursor.max(anchor) + 1).min(text.len());
        if start <= end { text[start..end].to_string() } else { String::new() }
    }

    fn delete_request_visual_selection(&mut self) {
        let cursor = self.get_request_cursor();
        let anchor = self.state.request_visual_anchor;
        let start = cursor.min(anchor);
        let end = (cursor.max(anchor) + 1).min(self.get_request_field_len());
        match self.state.request_focus {
            RequestFocus::Url => { self.state.current_request.url.drain(start..end); }
            RequestFocus::Header(idx) => {
                if let Some(h) = self.state.current_request.headers.get_mut(idx) {
                    let field = if self.state.header_edit_field == 0 { &mut h.name } else { &mut h.value };
                    field.drain(start..end);
                }
            }
            RequestFocus::Param(idx) => {
                if let Some(p) = self.state.current_request.query_params.get_mut(idx) {
                    let field = if self.state.param_edit_field == 0 { &mut p.key } else { &mut p.value };
                    field.drain(start..end);
                }
            }
            RequestFocus::Cookie(idx) => {
                if let Some(c) = self.state.current_request.cookies.get_mut(idx) {
                    let field = if self.state.cookie_edit_field == 0 { &mut c.name } else { &mut c.value };
                    field.drain(start..end);
                }
            }
            RequestFocus::PathParam(idx) => {
                if let Some(p) = self.state.current_request.path_params.get_mut(idx) {
                    let field = if self.state.path_param_edit_field == 0 { &mut p.key } else { &mut p.value };
                    field.drain(start..end);
                }
            }
        }
        self.set_request_cursor(start);
    }

    fn delete_request_char_under_cursor(&mut self) {
        let cursor = self.get_request_cursor();
        let len = self.get_request_field_len();
        if cursor >= len { return; }
        match self.state.request_focus {
            RequestFocus::Url => { self.state.current_request.url.remove(cursor); }
            RequestFocus::Header(idx) => {
                if let Some(h) = self.state.current_request.headers.get_mut(idx) {
                    let field = if self.state.header_edit_field == 0 { &mut h.name } else { &mut h.value };
                    field.remove(cursor);
                }
            }
            RequestFocus::Param(idx) => {
                if let Some(p) = self.state.current_request.query_params.get_mut(idx) {
                    let field = if self.state.param_edit_field == 0 { &mut p.key } else { &mut p.value };
                    field.remove(cursor);
                }
            }
            RequestFocus::Cookie(idx) => {
                if let Some(c) = self.state.current_request.cookies.get_mut(idx) {
                    let field = if self.state.cookie_edit_field == 0 { &mut c.name } else { &mut c.value };
                    field.remove(cursor);
                }
            }
            RequestFocus::PathParam(idx) => {
                if let Some(p) = self.state.current_request.path_params.get_mut(idx) {
                    let field = if self.state.path_param_edit_field == 0 { &mut p.key } else { &mut p.value };
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

    fn paste_request_text(&mut self, text: &str) {
        // Filter newlines out for single-line fields
        let clean: String = text.chars().filter(|c| *c != '\n' && *c != '\r').collect();
        let cursor = self.get_request_cursor();
        match self.state.request_focus {
            RequestFocus::Url => { self.state.current_request.url.insert_str(cursor, &clean); }
            RequestFocus::Header(idx) => {
                if let Some(h) = self.state.current_request.headers.get_mut(idx) {
                    let field = if self.state.header_edit_field == 0 { &mut h.name } else { &mut h.value };
                    field.insert_str(cursor, &clean);
                }
            }
            RequestFocus::Param(idx) => {
                if let Some(p) = self.state.current_request.query_params.get_mut(idx) {
                    let field = if self.state.param_edit_field == 0 { &mut p.key } else { &mut p.value };
                    field.insert_str(cursor, &clean);
                }
            }
            RequestFocus::Cookie(idx) => {
                if let Some(c) = self.state.current_request.cookies.get_mut(idx) {
                    let field = if self.state.cookie_edit_field == 0 { &mut c.name } else { &mut c.value };
                    field.insert_str(cursor, &clean);
                }
            }
            RequestFocus::PathParam(idx) => {
                if let Some(p) = self.state.current_request.path_params.get_mut(idx) {
                    let field = if self.state.path_param_edit_field == 0 { &mut p.key } else { &mut p.value };
                    field.insert_str(cursor, &clean);
                }
            }
        }
        self.set_request_cursor(cursor + clean.len());
    }

    fn request_word_forward(&mut self) {
        let text = self.get_request_field_text();
        let bytes = text.as_bytes();
        let mut col = self.get_request_cursor();
        if col < bytes.len() {
            if is_word_char(bytes[col]) {
                while col < bytes.len() && is_word_char(bytes[col]) { col += 1; }
            } else if is_punct_char(bytes[col]) {
                while col < bytes.len() && is_punct_char(bytes[col]) { col += 1; }
            }
            while col < bytes.len() && bytes[col].is_ascii_whitespace() { col += 1; }
        }
        self.set_request_cursor(col.min(bytes.len()));
    }

    fn request_word_backward(&mut self) {
        let text = self.get_request_field_text();
        let bytes = text.as_bytes();
        let mut col = self.get_request_cursor();
        if col == 0 { return; }
        col = col.saturating_sub(1);
        while col > 0 && bytes[col].is_ascii_whitespace() { col -= 1; }
        if col > 0 && is_word_char(bytes[col]) {
            while col > 0 && is_word_char(bytes[col - 1]) { col -= 1; }
        } else if col > 0 && is_punct_char(bytes[col]) {
            while col > 0 && is_punct_char(bytes[col - 1]) { col -= 1; }
        }
        self.set_request_cursor(col);
    }

    fn request_word_end(&mut self) {
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
        } else if is_punct_char(bytes[col]) {
            while col + 1 < bytes.len() && is_punct_char(bytes[col + 1]) { col += 1; }
        }
        self.set_request_cursor(col);
    }

    fn get_visual_selection(&self) -> String {
        let body = { let bt = self.state.body_type; match bt { BodyType::Json => self.state.current_request.body_json.as_deref().unwrap_or(""), BodyType::Xml => self.state.current_request.body_xml.as_deref().unwrap_or(""), BodyType::FormUrlEncoded => self.state.current_request.body_form.as_deref().unwrap_or(""), BodyType::Plain => self.state.current_request.body_raw.as_deref().unwrap_or("") } };
        let (sr, sc, er, ec) = self.visual_range();
        let start = row_col_to_offset(body, sr, sc);
        let end = row_col_to_offset(body, er, ec).min(body.len());
        if start <= end { body[start..end].to_string() } else { String::new() }
    }

    fn delete_visual_selection(&mut self) {
        let (sr, sc, er, ec) = self.visual_range();
        let bt = self.state.body_type; let body = match bt { BodyType::Json => self.state.current_request.body_json.get_or_insert_with(String::new), BodyType::Xml => self.state.current_request.body_xml.get_or_insert_with(String::new), BodyType::FormUrlEncoded => self.state.current_request.body_form.get_or_insert_with(String::new), BodyType::Plain => self.state.current_request.body_raw.get_or_insert_with(String::new) };
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

    /// Get block (rectangle) selection text from body — each line's column slice joined by newlines.
    fn get_block_selection(&self) -> String {
        let body = { let bt = self.state.body_type; match bt { BodyType::Json => self.state.current_request.body_json.as_deref().unwrap_or(""), BodyType::Xml => self.state.current_request.body_xml.as_deref().unwrap_or(""), BodyType::FormUrlEncoded => self.state.current_request.body_form.as_deref().unwrap_or(""), BodyType::Plain => self.state.current_request.body_raw.as_deref().unwrap_or("") } };
        let lines: Vec<&str> = body.lines().collect();
        let (ar, ac) = (self.state.visual_anchor_row, self.state.visual_anchor_col);
        let (cr, cc) = (self.state.body_cursor_row, self.state.body_cursor_col);
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
    fn delete_block_selection(&mut self) {
        let (ar, ac) = (self.state.visual_anchor_row, self.state.visual_anchor_col);
        let (cr, cc) = (self.state.body_cursor_row, self.state.body_cursor_col);
        let (min_row, min_col, max_row, max_col) = (ar.min(cr), ac.min(cc), ar.max(cr), ac.max(cc));

        let bt = self.state.body_type; let body = match bt { BodyType::Json => self.state.current_request.body_json.get_or_insert_with(String::new), BodyType::Xml => self.state.current_request.body_xml.get_or_insert_with(String::new), BodyType::FormUrlEncoded => self.state.current_request.body_form.get_or_insert_with(String::new), BodyType::Plain => self.state.current_request.body_raw.get_or_insert_with(String::new) };
        let mut lines: Vec<String> = body.lines().map(|l| l.to_string()).collect();
        for row in min_row..=max_row {
            if let Some(line) = lines.get_mut(row) {
                let start = min_col.min(line.len());
                let end = max_col.min(line.len());
                line.drain(start..end);
            }
        }
        *body = lines.join("\n");
        self.state.body_cursor_row = min_row;
        self.state.body_cursor_col = min_col;
    }

    /// Get block selection from response (read-only, for yank).
    fn get_response_block_selection(&self) -> String {
        let body = self.get_response_body_text();
        let lines: Vec<&str> = body.lines().collect();
        let (ar, ac) = (self.state.resp_visual_anchor_row, self.state.resp_visual_anchor_col);
        let (cr, cc) = (self.state.resp_cursor_row, self.state.resp_cursor_col);
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

    fn delete_body_line(&mut self, row: usize) {
        let bt = self.state.body_type; let body = match bt { BodyType::Json => self.state.current_request.body_json.get_or_insert_with(String::new), BodyType::Xml => self.state.current_request.body_xml.get_or_insert_with(String::new), BodyType::FormUrlEncoded => self.state.current_request.body_form.get_or_insert_with(String::new), BodyType::Plain => self.state.current_request.body_raw.get_or_insert_with(String::new) };
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

        let mut resolved = self.resolve_env_vars(&self.state.current_request);

        // Resolve chain references {{@request_name.json.path}}
        let mut resolving_stack = Vec::new();
        if let Some(ref name) = self.state.current_request.name {
            let coll_name = self.state.collections
                .get(self.state.active_collection)
                .map(|c| c.name.clone())
                .unwrap_or_default();
            resolving_stack.push(format!("{}/{}", coll_name, name));
        }

        match self.resolve_chains_in_request(&mut resolved, &mut resolving_stack).await {
            Ok(()) => {}
            Err(err) => {
                self.state.request_in_flight = false;
                self.state.last_error = Some(err.clone());
                self.state.set_status(format!("Error: {}", err));
                return;
            }
        }

        // Resolve path params in URL
        resolved.url = http_client::resolve_path_params(&resolved.url, &resolved.path_params);

        // Pick the active body based on body_type
        let active_body_ref = match self.state.body_type {
            BodyType::Json => &resolved.body_json,
            BodyType::Xml => &resolved.body_xml,
            BodyType::FormUrlEncoded => &resolved.body_form,
            BodyType::Plain => &resolved.body_raw,
        };
        let body_text_trimmed = active_body_ref.as_deref().unwrap_or("").trim().to_string();

        // Auto-inject Content-Type if body exists and no Content-Type header set
        if !body_text_trimmed.is_empty() {
            let has_ct = resolved.headers.iter().any(|h| h.enabled && h.name.eq_ignore_ascii_case("content-type"));
            if !has_ct {
                resolved.headers.push(Header {
                    name: "Content-Type".to_string(),
                    value: self.state.body_type.content_type().to_string(),
                    enabled: true,
                });
            }
        }

        // Clear all body fields, then set only the active one (trimmed, non-empty)
        resolved.body_json = None;
        resolved.body_xml = None;
        resolved.body_form = None;
        resolved.body_raw = None;
        if !body_text_trimmed.is_empty() {
            match self.state.body_type {
                BodyType::Json => resolved.body_json = Some(body_text_trimmed),
                BodyType::Xml => resolved.body_xml = Some(body_text_trimmed),
                BodyType::FormUrlEncoded => resolved.body_form = Some(body_text_trimmed),
                BodyType::Plain => resolved.body_raw = Some(body_text_trimmed),
            }
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

    fn resolve_env_vars(&self, req: &Request) -> Request {
        let env = &self.state.environments;
        Request {
            name: req.name.clone(),
            method: req.method,
            url: env.resolve(&req.url),
            headers: req.headers.iter().map(|h| Header { name: h.name.clone(), value: env.resolve(&h.value), enabled: h.enabled }).collect(),
            query_params: req.query_params.iter().map(|p| crate::model::request::QueryParam { key: p.key.clone(), value: env.resolve(&p.value), enabled: p.enabled }).collect(),
            cookies: req.cookies.iter().map(|c| crate::model::request::Cookie { name: c.name.clone(), value: env.resolve(&c.value), enabled: c.enabled }).collect(),
            path_params: req.path_params.iter().map(|p| crate::model::request::PathParam { key: p.key.clone(), value: env.resolve(&p.value), enabled: p.enabled }).collect(),
            body_json: req.body_json.as_ref().map(|b| env.resolve(b)),
            body_xml: req.body_xml.as_ref().map(|b| env.resolve(b)),
            body_form: req.body_form.as_ref().map(|b| env.resolve(b)),
            body_raw: req.body_raw.as_ref().map(|b| env.resolve(b)),
            source_file: req.source_file.clone(),
            source_line: req.source_line,
        }
    }

    /// Resolve all `{{@...}}` chain references in a request's fields.
    fn resolve_chains_in_request<'a>(
        &'a mut self,
        req: &'a mut Request,
        resolving: &'a mut Vec<String>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), String>> + 'a>> {
        Box::pin(async move {
        use crate::model::chain::find_chain_refs;

        // Check if any field has chain refs before doing work
        let has_refs = |s: &str| !find_chain_refs(s).is_empty();
        let need_resolve = has_refs(&req.url)
            || req.headers.iter().any(|h| has_refs(&h.value))
            || req.query_params.iter().any(|p| has_refs(&p.value))
            || req.cookies.iter().any(|c| has_refs(&c.value))
            || req.body_json.as_deref().map(|b| has_refs(b)).unwrap_or(false)
            || req.body_xml.as_deref().map(|b| has_refs(b)).unwrap_or(false)
            || req.body_form.as_deref().map(|b| has_refs(b)).unwrap_or(false)
            || req.body_raw.as_deref().map(|b| has_refs(b)).unwrap_or(false);

        if !need_resolve {
            return Ok(());
        }

        self.state.set_status("Resolving dependencies...");

        req.url = self.resolve_chains_in_str(&req.url, resolving).await?;

        for i in 0..req.headers.len() {
            let val = req.headers[i].value.clone();
            req.headers[i].value = self.resolve_chains_in_str(&val, resolving).await?;
        }
        for i in 0..req.query_params.len() {
            let val = req.query_params[i].value.clone();
            req.query_params[i].value = self.resolve_chains_in_str(&val, resolving).await?;
        }
        for i in 0..req.cookies.len() {
            let val = req.cookies[i].value.clone();
            req.cookies[i].value = self.resolve_chains_in_str(&val, resolving).await?;
        }
        for i in 0..req.path_params.len() {
            let val = req.path_params[i].value.clone();
            req.path_params[i].value = self.resolve_chains_in_str(&val, resolving).await?;
        }
        if let Some(ref body) = req.body_json.clone() {
            req.body_json = Some(self.resolve_chains_in_str(body, resolving).await?);
        }
        if let Some(ref body) = req.body_xml.clone() {
            req.body_xml = Some(self.resolve_chains_in_str(body, resolving).await?);
        }
        if let Some(ref body) = req.body_form.clone() {
            req.body_form = Some(self.resolve_chains_in_str(body, resolving).await?);
        }
        if let Some(ref body) = req.body_raw.clone() {
            req.body_raw = Some(self.resolve_chains_in_str(body, resolving).await?);
        }

        Ok(())
        }) // Box::pin
    }

    /// Resolve all `{{@...}}` references in a single string value.
    async fn resolve_chains_in_str(
        &mut self,
        value: &str,
        resolving: &mut Vec<String>,
    ) -> Result<String, String> {
        use crate::model::chain::{find_chain_refs, parse_chain_ref, extract_json_value, ChainError};

        let refs = find_chain_refs(value);
        if refs.is_empty() {
            return Ok(value.to_string());
        }

        let mut result = value.to_string();

        // Process in reverse order to preserve byte offsets
        for (start, end, inner) in refs.into_iter().rev() {
            let chain_ref = parse_chain_ref(&inner).ok_or_else(|| {
                format!("Chain error: invalid reference syntax '{{{{@{}}}}}'", inner)
            })?;

            // Build cache key
            // Find and clone the dependency request (clone early to avoid borrow issues)
            let (coll_idx, dep_request_clone) = {
                let (ci, req) = self.find_request_by_name(
                    &chain_ref.request_name,
                    chain_ref.collection.as_deref(),
                ).ok_or_else(|| {
                    ChainError::RequestNotFound { name: chain_ref.request_name.clone() }.to_string()
                })?;
                (ci, req.clone())
            };

            let coll_name = self.state.collections[coll_idx].name.clone();
            let cache_key = format!("{}/{}", coll_name, chain_ref.request_name);

            // Check for circular dependency
            if resolving.contains(&cache_key) {
                let mut chain = resolving.clone();
                chain.push(cache_key);
                return Err(ChainError::CircularDependency { chain }.to_string());
            }

            // Check cache with TTL
            let ttl = std::time::Duration::from_secs(self.state.config.general.chain_cache_ttl);
            let cached_valid = self.state.response_cache.get(&cache_key)
                .is_some_and(|(_, cached_at)| cached_at.elapsed() < ttl);

            if !cached_valid {
                // Remove stale cache entry if expired
                self.state.response_cache.remove(&cache_key);

                // Need to execute the dependency
                self.state.set_status(format!("Resolving: {}...", chain_ref.request_name));

                let mut resolved_dep = self.resolve_env_vars(&dep_request_clone);

                // Auto-inject Content-Type for dependency if it has a body
                let dep_body = resolved_dep.body_json.as_deref()
                    .or(resolved_dep.body_xml.as_deref())
                    .or(resolved_dep.body_form.as_deref())
                    .or(resolved_dep.body_raw.as_deref())
                    .unwrap_or("").trim();
                if !dep_body.is_empty() {
                    let has_ct = resolved_dep.headers.iter().any(|h| h.enabled && h.name.eq_ignore_ascii_case("content-type"));
                    if !has_ct {
                        resolved_dep.headers.push(crate::model::request::Header {
                            name: "Content-Type".to_string(),
                            value: "application/json".to_string(),
                            enabled: true,
                        });
                    }
                }

                // Recursively resolve chains in the dependency
                resolving.push(cache_key.clone());
                self.resolve_chains_in_request(&mut resolved_dep, resolving).await?;
                resolving.pop();

                // Execute the dependency
                let config = self.state.config.general.clone();
                let resp = http_client::execute(&resolved_dep, &config).await
                    .map_err(|e| ChainError::DependencyFailed {
                        request_name: chain_ref.request_name.clone(),
                        error: e.to_string(),
                    }.to_string())?;

                // Only cache successful responses (2xx)
                if resp.status >= 200 && resp.status < 300 {
                    self.state.response_cache.insert(cache_key.clone(), (resp, std::time::Instant::now()));
                    // Cap response cache at 50 entries, evicting oldest first
                    while self.state.response_cache.len() > 50 {
                        if let Some(oldest_key) = self.state.response_cache.iter()
                            .min_by_key(|(_, (_, ts))| *ts)
                            .map(|(k, _)| k.clone())
                        {
                            self.state.response_cache.remove(&oldest_key);
                        } else {
                            break;
                        }
                    }
                } else {
                    return Err(format!(
                        "Chain error: dependency '{}' returned {} {}",
                        chain_ref.request_name, resp.status, resp.status_text
                    ));
                }
            }

            // Extract value from cached response
            let (resp, _) = self.state.response_cache.get(&cache_key).unwrap();
            let extracted = extract_json_value(&resp.body, &chain_ref.json_path)
                .map_err(|e| match e {
                    ChainError::JsonPathNotFound { .. } => {
                        format!("Chain error: path '{}' not found in response from '{}'",
                            chain_ref.json_path, chain_ref.request_name)
                    }
                    other => other.to_string(),
                })?;

            result.replace_range(start..end, &extracted);
        }

        Ok(result)
    }

    /// Find a request by name across collections.
    /// If `collection` is Some, search only that collection.
    /// Otherwise, search active collection first, then all others.
    fn find_request_by_name(&self, name: &str, collection: Option<&str>) -> Option<(usize, &Request)> {
        if let Some(coll_name) = collection {
            // Search specific collection
            for (ci, coll) in self.state.collections.iter().enumerate() {
                if coll.name == coll_name {
                    for req in &coll.requests {
                        if req.name.as_deref() == Some(name) {
                            return Some((ci, req));
                        }
                    }
                }
            }
            return None;
        }

        // Search active collection first
        if let Some(coll) = self.state.collections.get(self.state.active_collection) {
            for req in &coll.requests {
                if req.name.as_deref() == Some(name) {
                    return Some((self.state.active_collection, req));
                }
            }
        }

        // Search all other collections
        for (ci, coll) in self.state.collections.iter().enumerate() {
            if ci == self.state.active_collection {
                continue;
            }
            for req in &coll.requests {
                if req.name.as_deref() == Some(name) {
                    return Some((ci, req));
                }
            }
        }

        None
    }

    fn scroll_down(&mut self) {
        match self.state.active_panel {
            Panel::Collections => {
                let i = self.state.collections_state.selected().map(|i| i + 1).unwrap_or(0);
                let max = self.state.collection_items.len().saturating_sub(1);
                self.state.collections_state.select(Some(i.min(max)));
            }
            Panel::Body => self.body_cursor_down(),
            Panel::Response => {
                if self.state.response_tab == ResponseTab::Type {
                    self.state.type_scroll = self.state.type_scroll.saturating_add(1);
                } else {
                    self.resp_cursor_down();
                }
            }
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
            Panel::Response => {
                if self.state.response_tab == ResponseTab::Type {
                    self.state.type_scroll = self.state.type_scroll.saturating_sub(1);
                } else {
                    self.resp_cursor_up();
                }
            }
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
                let body = { let bt = self.state.body_type; match bt { BodyType::Json => self.state.current_request.body_json.as_deref().unwrap_or(""), BodyType::Xml => self.state.current_request.body_xml.as_deref().unwrap_or(""), BodyType::FormUrlEncoded => self.state.current_request.body_form.as_deref().unwrap_or(""), BodyType::Plain => self.state.current_request.body_raw.as_deref().unwrap_or("") } };
                let lines: Vec<&str> = body.lines().collect();
                self.state.body_cursor_row = lines.len().saturating_sub(1);
                self.state.body_cursor_col = 0;
                self.sync_body_scroll(); self.sync_body_hscroll();
            }
            Panel::Response => {
                let lines = self.get_response_lines();
                self.state.resp_cursor_row = lines.len().saturating_sub(1);
                self.state.resp_cursor_col = 0;
                self.sync_resp_scroll(); self.sync_resp_hscroll();
            }
            _ => {}
        }
    }

    /// Ensure body_scroll keeps cursor visible (scrolloff-like behavior).
    fn sync_body_scroll(&mut self) {
        let visible = self.state.body_visible_height as usize;
        if visible == 0 { return; }
        let scroll = self.state.body_scroll.0 as usize;
        let row = self.state.body_cursor_row;
        if row < scroll {
            self.state.body_scroll.0 = row as u16;
        } else if row >= scroll + visible {
            self.state.body_scroll.0 = (row - visible + 1) as u16;
        }
    }

    /// Ensure body horizontal scroll keeps cursor visible.
    fn sync_body_hscroll(&mut self) {
        let col = self.state.body_cursor_col;
        let hscroll = self.state.body_scroll.1 as usize;
        let visible_w = self.state.body_visible_width as usize;
        if visible_w == 0 { return; }
        if col < hscroll {
            self.state.body_scroll.1 = col as u16;
        } else if col >= hscroll + visible_w {
            self.state.body_scroll.1 = (col - visible_w + 1) as u16;
        }
    }

    /// Ensure response_scroll keeps cursor visible.
    fn sync_resp_scroll(&mut self) {
        let visible = self.state.resp_visible_height as usize;
        if visible == 0 { return; }
        let scroll = self.state.response_scroll.0 as usize;
        let row = self.state.resp_cursor_row;
        if row < scroll {
            self.state.response_scroll.0 = row as u16;
        } else if row >= scroll + visible {
            self.state.response_scroll.0 = (row - visible + 1) as u16;
        }
    }

    /// Ensure response horizontal scroll keeps cursor visible.
    fn sync_resp_hscroll(&mut self) {
        let col = self.state.resp_cursor_col;
        let hscroll = self.state.response_scroll.1 as usize;
        let visible_w = self.state.resp_visible_width as usize;
        if visible_w == 0 { return; }
        if col < hscroll {
            self.state.response_scroll.1 = col as u16;
        } else if col >= hscroll + visible_w {
            self.state.response_scroll.1 = (col - visible_w + 1) as u16;
        }
    }

    fn scroll_half_down(&mut self) {
        let half = 15usize;
        match self.state.active_panel {
            Panel::Body => {
                let body = { let bt = self.state.body_type; match bt { BodyType::Json => self.state.current_request.body_json.as_deref().unwrap_or(""), BodyType::Xml => self.state.current_request.body_xml.as_deref().unwrap_or(""), BodyType::FormUrlEncoded => self.state.current_request.body_form.as_deref().unwrap_or(""), BodyType::Plain => self.state.current_request.body_raw.as_deref().unwrap_or("") } };
                let max = body.lines().count().saturating_sub(1);
                self.state.body_cursor_row = (self.state.body_cursor_row + half).min(max);
                self.sync_body_scroll(); self.sync_body_hscroll();
            }
            Panel::Response => {
                let max = self.get_response_lines().len().saturating_sub(1);
                self.state.resp_cursor_row = (self.state.resp_cursor_row + half).min(max);
                self.sync_resp_scroll(); self.sync_resp_hscroll();
            }
            _ => {}
        }
    }

    fn scroll_half_up(&mut self) {
        let half = 15usize;
        match self.state.active_panel {
            Panel::Body => {
                self.state.body_cursor_row = self.state.body_cursor_row.saturating_sub(half);
                self.sync_body_scroll(); self.sync_body_hscroll();
            }
            Panel::Response => {
                self.state.resp_cursor_row = self.state.resp_cursor_row.saturating_sub(half);
                self.sync_resp_scroll(); self.sync_resp_hscroll();
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
        self.sync_resp_scroll(); self.sync_resp_hscroll();
    }

    fn resp_cursor_up(&mut self) {
        if self.state.resp_cursor_row > 0 {
            self.state.resp_cursor_row -= 1;
            let lines = self.get_response_lines();
            let line_len = lines.get(self.state.resp_cursor_row).map(|l| l.len()).unwrap_or(0);
            self.state.resp_cursor_col = self.state.resp_cursor_col.min(line_len);
        }
        self.sync_resp_scroll(); self.sync_resp_hscroll();
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
        if let Some((ci, Some(ri))) = self.flat_idx_to_coll_req(flat_idx) {
            if let Some(req) = self.state.collections.get(ci).and_then(|c| c.requests.get(ri)) {
                self.state.current_request = req.clone();
                self.state.current_response = None;
                self.state.last_error = None;
                self.state.active_collection = ci;
            }
        }
    }

    /// Find char forward on the current line (f/t motion).
    /// If `before` is true, stop one position before the found char (t motion).
    fn find_char_forward(&mut self, target: char, before: bool) {
        match self.state.active_panel {
            Panel::Body => {
                let body = { let bt = self.state.body_type; match bt { BodyType::Json => self.state.current_request.body_json.as_deref().unwrap_or(""), BodyType::Xml => self.state.current_request.body_xml.as_deref().unwrap_or(""), BodyType::FormUrlEncoded => self.state.current_request.body_form.as_deref().unwrap_or(""), BodyType::Plain => self.state.current_request.body_raw.as_deref().unwrap_or("") } };
                let lines: Vec<&str> = body.lines().collect();
                if let Some(line) = lines.get(self.state.body_cursor_row) {
                    let bytes = line.as_bytes();
                    let start = self.state.body_cursor_col + 1;
                    for i in start..bytes.len() {
                        if bytes[i] == target as u8 {
                            self.state.body_cursor_col = if before { i.saturating_sub(1).max(start.saturating_sub(1)) } else { i };
                            break;
                        }
                    }
                }
            }
            Panel::Request if self.state.request_field_editing => {
                let text = self.get_request_field_text();
                let bytes = text.as_bytes();
                let cursor = self.get_request_cursor();
                let start = cursor + 1;
                for i in start..bytes.len() {
                    if bytes[i] == target as u8 {
                        self.set_request_cursor(if before { i.saturating_sub(1).max(start.saturating_sub(1)) } else { i });
                        break;
                    }
                }
            }
            Panel::Response => {
                let text = self.get_response_body_text();
                let lines: Vec<&str> = text.lines().collect();
                if let Some(line) = lines.get(self.state.resp_cursor_row) {
                    let bytes = line.as_bytes();
                    let start = self.state.resp_cursor_col + 1;
                    for i in start..bytes.len() {
                        if bytes[i] == target as u8 {
                            self.state.resp_cursor_col = if before { i.saturating_sub(1).max(start.saturating_sub(1)) } else { i };
                            break;
                        }
                    }
                }
            }
            _ => {}
        }
    }

    /// Find char backward on the current line (F/T motion).
    /// If `after` is true, stop one position after the found char (T motion).
    fn find_char_backward(&mut self, target: char, after: bool) {
        match self.state.active_panel {
            Panel::Body => {
                let body = { let bt = self.state.body_type; match bt { BodyType::Json => self.state.current_request.body_json.as_deref().unwrap_or(""), BodyType::Xml => self.state.current_request.body_xml.as_deref().unwrap_or(""), BodyType::FormUrlEncoded => self.state.current_request.body_form.as_deref().unwrap_or(""), BodyType::Plain => self.state.current_request.body_raw.as_deref().unwrap_or("") } };
                let lines: Vec<&str> = body.lines().collect();
                if let Some(line) = lines.get(self.state.body_cursor_row) {
                    let bytes = line.as_bytes();
                    let col = self.state.body_cursor_col;
                    if col > 0 {
                        for i in (0..col).rev() {
                            if bytes[i] == target as u8 {
                                self.state.body_cursor_col = if after { (i + 1).min(col) } else { i };
                                break;
                            }
                        }
                    }
                }
            }
            Panel::Request if self.state.request_field_editing => {
                let text = self.get_request_field_text();
                let bytes = text.as_bytes();
                let cursor = self.get_request_cursor();
                if cursor > 0 {
                    for i in (0..cursor).rev() {
                        if bytes[i] == target as u8 {
                            self.set_request_cursor(if after { (i + 1).min(cursor) } else { i });
                            break;
                        }
                    }
                }
            }
            Panel::Response => {
                let text = self.get_response_body_text();
                let lines: Vec<&str> = text.lines().collect();
                if let Some(line) = lines.get(self.state.resp_cursor_row) {
                    let bytes = line.as_bytes();
                    let col = self.state.resp_cursor_col;
                    if col > 0 {
                        for i in (0..col).rev() {
                            if bytes[i] == target as u8 {
                                self.state.resp_cursor_col = if after { (i + 1).min(col) } else { i };
                                break;
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

// Vim word-class helpers
fn is_word_char(b: u8) -> bool { b.is_ascii_alphanumeric() || b == b'_' }
fn is_punct_char(b: u8) -> bool { !b.is_ascii_whitespace() && !is_word_char(b) }

fn row_col_to_offset(text: &str, row: usize, col: usize) -> usize {
    let mut offset = 0;
    for (i, line) in text.split('\n').enumerate() {
        if i == row { return offset + col.min(line.len()); }
        offset += line.len() + 1;
    }
    text.len()
}
