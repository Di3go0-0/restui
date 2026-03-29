use crate::config::AppConfig;
use crate::model::collection::Collection;
use crate::model::environment::EnvironmentStore;
use crate::model::history::HistoryEntry;
use crate::model::request::Request;
use crate::model::response::Response;
use ratatui::widgets::ListState;
use std::time::Instant;

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
pub enum RequestTab {
    Headers,
    Params,
    Auth,
    Cookies,
}

impl RequestTab {
    pub const ALL: &'static [RequestTab] = &[
        RequestTab::Headers,
        RequestTab::Params,
        RequestTab::Auth,
        RequestTab::Cookies,
    ];

    pub fn label(self) -> &'static str {
        match self {
            RequestTab::Headers => "Headers",
            RequestTab::Params => "Params",
            RequestTab::Auth => "Auth",
            RequestTab::Cookies => "Cookies",
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
    pub history: Vec<HistoryEntry>,
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

    // Viewport heights (updated each frame by UI)
    pub body_visible_height: u16,
    pub resp_visible_height: u16,

    // Pending key for dd
    pub pending_key: Option<(char, Instant)>,

    // Inline autocomplete (for header names)
    pub autocomplete: Option<Autocomplete>,

    // Clipboard (internal)
    pub yank_buffer: String,

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
            history: Vec::new(),
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
            pending_key: None,
            autocomplete: None,
            yank_buffer: String::new(),
            command_palette_open: false,
            command_palette_input: String::new(),
            command_palette_selected: 0,
            overlay: None,
            env_selector_state: ListState::default(),
            theme: crate::theme::Theme::default(),
            config,
            should_quit: false,
            status_message: None,
            collection_items: Vec::new(),
        }
    }

    pub fn set_status(&mut self, msg: impl Into<String>) {
        self.status_message = Some((msg.into(), Instant::now()));
    }

    pub fn validate_body(&mut self) {
        let body = self.current_request.body.as_deref().unwrap_or("");
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
