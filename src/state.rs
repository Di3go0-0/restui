use std::collections::HashMap;
use std::time::{Duration, Instant};

use ratatui::widgets::ListState;

use crate::config::AppConfig;
use crate::model::collection::Collection;
use crate::model::environment::EnvironmentStore;
use crate::model::request::Request;
use crate::model::response::Response;

// ── Application-wide constants ──────────────────────────────────────────────
pub const RESPONSE_CACHE_MAX: usize = 50;
pub const UNDO_STACK_MAX: usize = 100;
pub const WIDE_LAYOUT_THRESHOLD: u16 = 120;
pub const STATUS_MESSAGE_TTL: Duration = Duration::from_secs(5);
pub const PENDING_KEY_TIMEOUT: Duration = Duration::from_millis(500);
pub const EVENT_TICK_RATE: Duration = Duration::from_millis(250);
pub const MAX_REDIRECTS: usize = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Panel {
    Collections,
    Request,
    Body,
    Response,
}

impl Panel {
    pub fn title(self) -> &'static str {
        match self {
            Panel::Collections => "Collections",
            Panel::Request => "Request",
            Panel::Body => "Body",
            Panel::Response => "Response",
        }
    }

    pub fn navigate(self, dir: Direction, is_wide: bool, last_middle: Panel) -> Panel {
        if is_wide {
            match (self, dir) {
                (Panel::Collections, Direction::Right) => Panel::Request,
                (Panel::Collections, Direction::Down) => Panel::Request,
                (Panel::Request, Direction::Left) => Panel::Collections,
                (Panel::Request, Direction::Right) => Panel::Response,
                (Panel::Request, Direction::Down) => Panel::Body,
                (Panel::Body, Direction::Left) => Panel::Collections,
                (Panel::Body, Direction::Right) => Panel::Response,
                (Panel::Body, Direction::Up) => Panel::Request,
                (Panel::Response, Direction::Left) => last_middle,
                (Panel::Response, Direction::Up) => Panel::Request,
                (Panel::Response, Direction::Down) => Panel::Body,
                (panel, _) => panel,
            }
        } else {
            match (self, dir) {
                (Panel::Collections, Direction::Right) => Panel::Request,
                (Panel::Request, Direction::Left) => Panel::Collections,
                (Panel::Request, Direction::Down) => Panel::Body,
                (Panel::Body, Direction::Left) => Panel::Collections,
                (Panel::Body, Direction::Up) => Panel::Request,
                (Panel::Body, Direction::Down) => Panel::Response,
                (Panel::Response, Direction::Left) => Panel::Collections,
                (Panel::Response, Direction::Up) => Panel::Body,
                (panel, _) => panel,
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Left,
    Right,
    Up,
    Down,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputMode {
    Normal,
    Insert,
    Visual,
    VisualBlock,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BodyType {
    Json,
    Xml,
    FormUrlEncoded,
    Plain,
}

impl BodyType {
    pub fn label(self) -> &'static str {
        match self {
            BodyType::Json => "JSON",
            BodyType::Xml => "XML",
            BodyType::FormUrlEncoded => "Form",
            BodyType::Plain => "Raw",
        }
    }

    pub fn content_type(self) -> &'static str {
        match self {
            BodyType::Json => "application/json",
            BodyType::Xml => "application/xml",
            BodyType::FormUrlEncoded => "application/x-www-form-urlencoded",
            BodyType::Plain => "text/plain",
        }
    }

    pub fn next(self) -> Self {
        match self {
            BodyType::Json => BodyType::Xml,
            BodyType::Xml => BodyType::FormUrlEncoded,
            BodyType::FormUrlEncoded => BodyType::Plain,
            BodyType::Plain => BodyType::Json,
        }
    }

    pub fn prev(self) -> Self {
        match self {
            BodyType::Json => BodyType::Plain,
            BodyType::Xml => BodyType::Json,
            BodyType::FormUrlEncoded => BodyType::Xml,
            BodyType::Plain => BodyType::FormUrlEncoded,
        }
    }

    pub fn validate(self, body: &str) -> Option<String> {
        if body.trim().is_empty() {
            return None;
        }
        match self {
            BodyType::Json => {
                match serde_json::from_str::<serde_json::Value>(body) {
                    Ok(_) => None,
                    Err(e) => Some(format!("JSON: {}", e)),
                }
            }
            BodyType::Xml => {
                // Basic XML check: starts with < and has matching tags
                let trimmed = body.trim();
                if !trimmed.starts_with('<') {
                    Some("XML: must start with '<'".to_string())
                } else {
                    None
                }
            }
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub enum Overlay {
    Help,
    EnvironmentSelector,
    HeaderAutocomplete {
        suggestions: Vec<(String, String)>,
        selected: usize,
    },
    NewCollection {
        name: String,
    },
    RenameRequest {
        name: String,
    },
    MoveRequest {
        selected: usize,
    },
    ConfirmDelete {
        message: String,
    },
    SetCacheTTL {
        input: String,
    },
    ThemeSelector {
        selected: usize,
    },
    EnvironmentEditor {
        selected: usize,
        editing_key: bool,
        new_key: String,
        new_value: String,
        cursor: usize,
    },
}

#[derive(Debug, Clone)]
pub struct ChainAutocomplete {
    pub items: Vec<(String, String)>,  // (display_text, insert_text)
    pub selected: usize,
    #[allow(dead_code)]
    pub anchor_panel: Panel,  // which panel triggered it
}

impl ChainAutocomplete {
    pub fn next(&mut self) {
        if !self.items.is_empty() {
            self.selected = (self.selected + 1) % self.items.len();
        }
    }
    pub fn prev(&mut self) {
        if !self.items.is_empty() {
            self.selected = (self.selected + self.items.len() - 1) % self.items.len();
        }
    }
    pub fn accept(&self) -> Option<&str> {
        self.items.get(self.selected).map(|(_, insert)| insert.as_str())
    }
}

#[derive(Debug, Clone)]
pub struct Autocomplete {
    pub filtered: Vec<(String, String)>, // (header_name, default_value)
    pub selected: usize,
}

impl Autocomplete {
    pub fn new(query: &str) -> Self {
        let query_lower = query.to_lowercase();
        let filtered: Vec<(String, String)> = COMMON_HEADERS
            .iter()
            .filter(|(name, _)| name.to_lowercase().starts_with(&query_lower))
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        Self {
            filtered,
            selected: 0,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.filtered.is_empty()
    }

    pub fn next(&mut self) {
        if !self.filtered.is_empty() {
            self.selected = (self.selected + 1) % self.filtered.len();
        }
    }

    pub fn prev(&mut self) {
        if !self.filtered.is_empty() {
            self.selected = if self.selected == 0 {
                self.filtered.len() - 1
            } else {
                self.selected - 1
            };
        }
    }

    pub fn accept(&self) -> Option<(&str, &str)> {
        self.filtered
            .get(self.selected)
            .map(|(k, v)| (k.as_str(), v.as_str()))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResponseTab {
    Body,
    Type,
}

impl ResponseTab {
    pub fn next(self) -> Self {
        match self {
            ResponseTab::Body => ResponseTab::Type,
            ResponseTab::Type => ResponseTab::Body,
        }
    }
    pub fn prev(self) -> Self {
        self.next()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestTab {
    Headers,
    Cookies,
    Queries,
    Params,
}

impl RequestTab {
    pub const ALL: &'static [RequestTab] = &[
        RequestTab::Headers,
        RequestTab::Cookies,
        RequestTab::Queries,
        RequestTab::Params,
    ];

    pub fn label(self) -> &'static str {
        match self {
            RequestTab::Headers => "Headers",
            RequestTab::Cookies => "Cookies",
            RequestTab::Queries => "Queries",
            RequestTab::Params => "Params",
        }
    }

    pub fn next(self) -> Self {
        let all = Self::ALL;
        let idx = all.iter().position(|&t| t == self).unwrap_or(0);
        all[(idx + 1) % all.len()]
    }

    pub fn prev(self) -> Self {
        let all = Self::ALL;
        let idx = all.iter().position(|&t| t == self).unwrap_or(0);
        all[(idx + all.len() - 1) % all.len()]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestFocus {
    Url,
    Header(usize),
    Param(usize),
    Cookie(usize),
    PathParam(usize),
}

pub struct AppState {
    // Navigation
    pub active_panel: Panel,
    pub mode: InputMode,

    // Layout
    pub is_wide_layout: bool,
    pub last_middle_panel: Panel,

    // Data
    pub collections: Vec<Collection>,
    pub environments: EnvironmentStore,
    pub active_collection: usize,

    // Current request
    pub current_request: Request,
    pub current_response: Option<Response>,
    pub last_error: Option<String>,
    pub request_in_flight: bool,
    pub body_type: BodyType,
    pub body_validation_error: Option<String>,

    // Panel state
    pub collections_state: ListState,
    pub body_scroll: (u16, u16),
    pub response_scroll: (u16, u16),

    // Request panel tabs & inline editing
    pub request_tab: RequestTab,
    pub request_focus: RequestFocus,
    pub url_cursor: usize,
    pub header_edit_cursor: usize,
    pub header_edit_field: u8, // 0=name, 1=value
    pub param_edit_cursor: usize,
    pub param_edit_field: u8, // 0=key, 1=value
    pub cookie_edit_cursor: usize,
    pub cookie_edit_field: u8, // 0=name, 1=value
    pub path_param_edit_cursor: usize,
    pub path_param_edit_field: u8, // 0=key, 1=value
    pub request_field_editing: bool,   // true = vim normal mode inside a field
    pub request_visual_anchor: usize,  // visual selection anchor for request fields

    // Body inline editing
    pub body_cursor_row: usize,
    pub body_cursor_col: usize,

    // Visual mode selection (body)
    pub visual_anchor_row: usize,
    pub visual_anchor_col: usize,

    // Response cursor (for visual mode in response)
    pub resp_cursor_row: usize,
    pub resp_cursor_col: usize,
    pub resp_visual_anchor_row: usize,
    pub resp_visual_anchor_col: usize,

    // Viewport heights/widths (updated each frame by UI)
    pub body_visible_height: u16,
    pub resp_visible_height: u16,
    pub body_visible_width: u16,
    pub resp_visible_width: u16,

    // Pending key for dd
    pub pending_key: Option<(char, Instant)>,

    // Inline autocomplete (for header names)
    pub autocomplete: Option<Autocomplete>,

    // Chain reference autocomplete (for {{@...}} syntax)
    pub chain_autocomplete: Option<ChainAutocomplete>,

    // Clipboard (internal)
    pub yank_buffer: String,
    pub yanked_request: Option<Request>,

    // Undo/Redo history for body editing
    pub body_undo_stack: Vec<(String, usize, usize)>, // (body_snapshot, cursor_row, cursor_col)
    pub body_redo_stack: Vec<(String, usize, usize)>,

    // Undo/Redo history for request field editing
    // (focus, edit_field, field_text, cursor_pos)
    pub request_undo_stack: Vec<(RequestFocus, u8, String, usize)>,
    pub request_redo_stack: Vec<(RequestFocus, u8, String, usize)>,

    // Response cache for request chaining: key = "collection/request_name", value = (Response, cached_at)
    pub response_cache: HashMap<String, (Response, Instant)>,

    // Collections expand/collapse
    pub expanded_collections: std::collections::HashSet<usize>,

    // Command Palette
    pub command_palette_open: bool,
    pub command_palette_input: String,
    pub command_palette_selected: usize,

    // Overlays
    pub overlay: Option<Overlay>,
    pub env_selector_state: ListState,

    // Theme
    pub theme: crate::theme::Theme,

    // Config
    pub config: AppConfig,

    // Response headers inspector
    pub response_headers_expanded: bool,
    pub response_headers_scroll: usize,

    // Search
    pub search_query: String,
    pub search_active: bool,
    pub search_matches: Vec<(usize, usize)>,
    pub search_match_idx: usize,

    // Collections filter
    pub collections_filter: String,
    pub collections_filter_active: bool,

    // Response type inference
    pub response_type: Option<crate::model::response_type::JsonType>,
    pub response_tab: ResponseTab,
    pub type_scroll: usize,
    pub response_type_text: String,
    pub response_type_locked: bool,
    pub type_cursor_row: usize,
    pub type_cursor_col: usize,
    pub type_validation_errors: Vec<String>,

    // Bracket matching: (row, col) of the matching bracket, None if no match
    #[allow(dead_code)]
    pub matched_bracket: Option<(usize, usize)>,

    // Count prefix (vim number prefix for repeatable motions)
    pub count_prefix: Option<u32>,

    // Misc
    pub should_quit: bool,
    pub status_message: Option<(String, Instant)>,
    pub collection_items: Vec<String>,
}

impl AppState {
    pub fn new(config: AppConfig) -> Self {
        let mut collections_state = ListState::default();
        collections_state.select(Some(0));

        Self {
            active_panel: Panel::Collections,
            mode: InputMode::Normal,
            is_wide_layout: true,
            last_middle_panel: Panel::Request,
            collections: Vec::new(),
            environments: EnvironmentStore::default(),
            active_collection: 0,
            current_request: Request::default(),
            current_response: None,
            last_error: None,
            request_in_flight: false,
            body_type: BodyType::Json,
            body_validation_error: None,
            collections_state,
            body_scroll: (0, 0),
            response_scroll: (0, 0),
            request_tab: RequestTab::Headers,
            request_focus: RequestFocus::Url,
            url_cursor: 0,
            header_edit_cursor: 0,
            header_edit_field: 0,
            param_edit_cursor: 0,
            param_edit_field: 0,
            cookie_edit_cursor: 0,
            cookie_edit_field: 0,
            path_param_edit_cursor: 0,
            path_param_edit_field: 0,
            request_field_editing: false,
            request_visual_anchor: 0,
            body_cursor_row: 0,
            body_cursor_col: 0,
            visual_anchor_row: 0,
            visual_anchor_col: 0,
            resp_cursor_row: 0,
            resp_cursor_col: 0,
            resp_visual_anchor_row: 0,
            resp_visual_anchor_col: 0,
            body_visible_height: 20,
            resp_visible_height: 20,
            body_visible_width: 80,
            resp_visible_width: 80,
            pending_key: None,
            autocomplete: None,
            chain_autocomplete: None,
            yank_buffer: String::new(),
            yanked_request: None,
            body_undo_stack: Vec::new(),
            body_redo_stack: Vec::new(),
            request_undo_stack: Vec::new(),
            request_redo_stack: Vec::new(),
            response_cache: HashMap::new(),
            expanded_collections: {
                let mut s = std::collections::HashSet::new();
                s.insert(0); // expand first collection by default
                s
            },
            command_palette_open: false,
            command_palette_input: String::new(),
            command_palette_selected: 0,
            overlay: None,
            env_selector_state: ListState::default(),
            theme: crate::theme::Theme::default(),
            config,
            response_headers_expanded: false,
            response_headers_scroll: 0,
            search_query: String::new(),
            search_active: false,
            search_matches: Vec::new(),
            search_match_idx: 0,
            collections_filter: String::new(),
            collections_filter_active: false,
            response_type: None,
            response_tab: ResponseTab::Body,
            type_scroll: 0,
            response_type_text: String::new(),
            response_type_locked: false,
            type_cursor_row: 0,
            type_cursor_col: 0,
            type_validation_errors: Vec::new(),
            matched_bracket: None,
            count_prefix: None,
            should_quit: false,
            status_message: None,
            collection_items: Vec::new(),
        }
    }

    pub fn set_status(&mut self, msg: impl Into<String>) {
        self.status_message = Some((msg.into(), Instant::now()));
    }

    pub fn validate_body(&mut self) {
        let body = match self.body_type {
            BodyType::Json => self.current_request.body_json.as_deref().unwrap_or(""),
            BodyType::Xml => self.current_request.body_xml.as_deref().unwrap_or(""),
            BodyType::FormUrlEncoded => self.current_request.body_form.as_deref().unwrap_or(""),
            BodyType::Plain => self.current_request.body_raw.as_deref().unwrap_or(""),
        };
        self.body_validation_error = self.body_type.validate(body);
    }
}

pub const COMMON_HEADERS: &[(&str, &str)] = &[
    ("Authorization", "Bearer "),
    ("Content-Type", "application/json"),
    ("Accept", "application/json"),
    ("Accept-Encoding", "gzip, deflate, br"),
    ("Cache-Control", "no-cache"),
    ("Connection", "keep-alive"),
    ("Content-Length", ""),
    ("Cookie", ""),
    ("Host", ""),
    ("Origin", ""),
    ("Referer", ""),
    ("User-Agent", "restui/0.1"),
    ("X-API-Key", ""),
    ("X-Request-ID", ""),
];
