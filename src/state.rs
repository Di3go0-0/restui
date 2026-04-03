use std::collections::HashMap;
use std::time::{Duration, Instant};

use ratatui::widgets::ListState;

use crate::config::AppConfig;
use crate::keybinding_config::KeybindingsConfig;
use crate::model::response::ResponseHistories;
use crate::model::collection::Collection;
use crate::model::environment::EnvironmentStore;
use crate::model::history::History;
use crate::model::request::Request;
use crate::model::response::Response;
use vimltui::VimEditor;
use vimltui::VimModeConfig;

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
    History {
        selected: usize,
    },
    EnvironmentEditor {
        selected: usize,
        editing_key: bool,
        new_key: String,
        new_value: String,
        cursor: usize,
    },
    ResponseHistory {
        selected: usize,
    },
    ResponseDiffSelect {
        selected: usize,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutocompleteKind {
    Chain,
    Env,
}

#[derive(Debug, Clone)]
pub struct ChainAutocomplete {
    pub items: Vec<(String, String)>,  // (display_text, insert_text)
    pub selected: usize,
    #[allow(dead_code)]
    pub anchor_panel: Panel,  // which panel triggered it
    pub kind: AutocompleteKind,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TypeLang {
    #[default]
    Inferred,
    TypeScript,
    CSharp,
}

impl TypeLang {
    pub fn label(self) -> &'static str {
        match self {
            TypeLang::Inferred => "Type",
            TypeLang::TypeScript => "TS",
            TypeLang::CSharp => "C#",
        }
    }
    pub fn next(self) -> Self {
        match self {
            TypeLang::Inferred => TypeLang::TypeScript,
            TypeLang::TypeScript => TypeLang::CSharp,
            TypeLang::CSharp => TypeLang::Inferred,
        }
    }
    pub fn prev(self) -> Self {
        match self {
            TypeLang::Inferred => TypeLang::CSharp,
            TypeLang::TypeScript => TypeLang::Inferred,
            TypeLang::CSharp => TypeLang::TypeScript,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TypeSubFocus {
    #[default]
    Editor,
    Preview,
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

// ── Grouped sub-states ──────────────────────────────────────────────────────

#[derive(Debug, Default)]
pub struct SearchState {
    pub query: String,
    pub active: bool,
    pub matches: Vec<(usize, usize)>,
    pub match_idx: usize,
}

#[derive(Debug, Default)]
pub struct CommandPaletteState {
    pub open: bool,
    pub input: String,
    pub selected: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestFocus {
    Url,
    Header(usize),
    Param(usize),
    Cookie(usize),
    PathParam(usize),
}

pub struct RequestEditState {
    pub tab: RequestTab,
    pub focus: RequestFocus,
    pub url_cursor: usize,
    pub header_edit_cursor: usize,
    pub header_edit_field: u8,
    pub param_edit_cursor: usize,
    pub param_edit_field: u8,
    pub cookie_edit_cursor: usize,
    pub cookie_edit_field: u8,
    pub path_param_edit_cursor: usize,
    pub path_param_edit_field: u8,
    pub field_editing: bool,
    pub visual_anchor: usize,
    pub undo_stack: Vec<(RequestFocus, u8, String, usize)>,
    pub redo_stack: Vec<(RequestFocus, u8, String, usize)>,
}

impl RequestEditState {
    pub fn new() -> Self {
        Self {
            tab: RequestTab::Headers,
            focus: RequestFocus::Url,
            url_cursor: 0,
            header_edit_cursor: 0,
            header_edit_field: 0,
            param_edit_cursor: 0,
            param_edit_field: 0,
            cookie_edit_cursor: 0,
            cookie_edit_field: 0,
            path_param_edit_cursor: 0,
            path_param_edit_field: 0,
            field_editing: false,
            visual_anchor: 0,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
        }
    }
}

pub struct ResponseViewState {
    pub tab: ResponseTab,
    pub response_type: Option<crate::model::response_type::JsonType>,
    pub type_text: String,
    pub type_locked: bool,
    pub type_validation_errors: Vec<String>,
    pub type_vim: VimEditor,
    pub type_sub_focus: TypeSubFocus,
    pub type_lang: TypeLang,
    pub type_ts_text: String,
    pub type_csharp_text: String,
    pub type_ts_vim: VimEditor,
    pub type_csharp_vim: VimEditor,
    pub headers_expanded: bool,
    pub headers_scroll: usize,
    pub resp_vim: VimEditor,
    pub resp_hscroll: usize,
    pub resp_visible_width: usize,
}

impl ResponseViewState {
    pub fn new() -> Self {
        Self {
            tab: ResponseTab::Body,
            response_type: None,
            type_text: String::new(),
            type_locked: false,
            type_validation_errors: Vec::new(),
            type_vim: VimEditor::new("", VimModeConfig::default()),
            type_sub_focus: TypeSubFocus::default(),
            type_lang: TypeLang::default(),
            type_ts_text: String::new(),
            type_csharp_text: String::new(),
            type_ts_vim: VimEditor::new("", VimModeConfig::default()),
            type_csharp_vim: VimEditor::new("", VimModeConfig::default()),
            headers_expanded: false,
            headers_scroll: 0,
            resp_vim: VimEditor::new("", VimModeConfig::read_only()),
            resp_hscroll: 0,
            resp_visible_width: 80,
        }
    }
}

pub struct CollectionsViewState {
    pub active: usize,
    pub list_state: ListState,
    pub expanded: std::collections::HashSet<usize>,
    pub items: Vec<String>,
    pub filter: String,
    pub filter_active: bool,
}

impl CollectionsViewState {
    pub fn new() -> Self {
        let mut list_state = ListState::default();
        list_state.select(Some(0));
        Self {
            active: 0,
            list_state,
            expanded: {
                let mut s = std::collections::HashSet::new();
                s.insert(0);
                s
            },
            items: Vec::new(),
            filter: String::new(),
            filter_active: false,
        }
    }
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
    pub history: History,

    // Current request
    pub current_request: Request,
    pub current_response: Option<Response>,
    pub last_error: Option<String>,
    pub request_in_flight: bool,
    pub request_started_at: Option<Instant>,
    pub request_abort_handle: Option<tokio::task::AbortHandle>,
    pub body_type: BodyType,
    pub body_validation_error: Option<String>,

    // Sub-states
    pub request_edit: RequestEditState,
    pub response_view: ResponseViewState,
    pub collections_view: CollectionsViewState,

    // Body vim editor (all modes)
    pub body_vim: VimEditor,
    pub body_hscroll: usize,
    pub body_visible_width: usize,

    // Pending key for dd (only used in Collections/Request panels now)
    pub pending_key: Option<(char, Instant)>,

    // Inline autocomplete (for header names)
    pub autocomplete: Option<Autocomplete>,

    // Chain reference autocomplete (for {{@...}} syntax)
    pub chain_autocomplete: Option<ChainAutocomplete>,

    // Clipboard (internal)
    pub yank_buffer: String,
    pub yanked_request: Option<Request>,

    // Response cache for request chaining: key = "collection/request_name", value = (Response, cached_at)
    pub response_cache: HashMap<String, (Response, Instant)>,

    // Command Palette
    pub command_palette: CommandPaletteState,

    // Overlays
    pub overlay: Option<Overlay>,
    pub env_selector_state: ListState,

    // Theme
    pub theme: crate::theme::Theme,

    // Config
    pub config: AppConfig,

    // Word wrap (global toggle)
    pub wrap_enabled: bool,

    // Search
    pub search: SearchState,

    // Bracket matching: (row, col) of the matching bracket, None if no match
    #[allow(dead_code)]
    pub matched_bracket: Option<(usize, usize)>,

    // Count prefix (vim number prefix for repeatable motions)
    pub count_prefix: Option<u32>,

    // Last response info (for status bar badge)
    pub last_response_info: Option<(u16, u64)>, // (status_code, elapsed_ms)

    // Misc
    pub should_quit: bool,
    pub status_message: Option<(String, Instant)>,

    // Response history per request (max 5 previous responses, persisted)
    pub response_histories: ResponseHistories,
    /// When viewing a historical response: Some((index, total, timestamp)). None = viewing current/live response.
    pub viewing_history: Option<(usize, usize, String)>,
    /// When viewing a diff: Some((diff_text, timestamp_label)). Rendered in response panel with read-only vim.
    pub viewing_diff: Option<(String, String)>,

    // Keybindings
    pub keybindings: KeybindingsConfig,

    // Help scroll
    pub help_scroll: u16,
}

impl AppState {
    pub fn new(config: AppConfig, keybindings: KeybindingsConfig) -> Self {
        Self {
            active_panel: Panel::Collections,
            mode: InputMode::Normal,
            is_wide_layout: true,
            last_middle_panel: Panel::Request,
            collections: Vec::new(),
            environments: EnvironmentStore::default(),
            history: History::load(&crate::config::data_dir().join("history.json")),
            current_request: Request::default(),
            current_response: None,
            last_error: None,
            request_in_flight: false,
            request_started_at: None,
            request_abort_handle: None,
            body_type: BodyType::Json,
            body_validation_error: None,
            request_edit: RequestEditState::new(),
            response_view: ResponseViewState::new(),
            collections_view: CollectionsViewState::new(),
            body_vim: VimEditor::new("", VimModeConfig::default()),
            body_hscroll: 0,
            body_visible_width: 80,
            pending_key: None,
            autocomplete: None,
            chain_autocomplete: None,
            yank_buffer: String::new(),
            yanked_request: None,
            response_cache: HashMap::new(),
            command_palette: CommandPaletteState::default(),
            overlay: None,
            env_selector_state: ListState::default(),
            theme: crate::theme::Theme::default(),
            config,
            wrap_enabled: false,
            search: SearchState::default(),
            matched_bracket: None,
            count_prefix: None,
            last_response_info: None,
            should_quit: false,
            status_message: None,
            response_histories: ResponseHistories::load(&crate::config::data_dir().join("response_history.json")),
            viewing_history: None,
            viewing_diff: None,
            keybindings,
            help_scroll: 0,
        }
    }

    pub fn set_status(&mut self, msg: impl Into<String>) {
        self.status_message = Some((msg.into(), Instant::now()));
    }

    pub fn validate_body(&mut self) {
        let body = self.current_request.get_body(self.body_type);
        self.body_validation_error = self.body_type.validate(body);
    }

    /// Get the VimEditor for the active panel
    #[allow(dead_code)]
    pub fn active_vim(&mut self) -> &mut VimEditor {
        match self.active_panel {
            Panel::Body => &mut self.body_vim,
            Panel::Response if self.response_view.tab == ResponseTab::Type => &mut self.response_view.type_vim,
            Panel::Response => &mut self.response_view.resp_vim,
            _ => &mut self.body_vim, // fallback
        }
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
    ("User-Agent", concat!("restui/", env!("CARGO_PKG_VERSION"))),
    ("X-API-Key", ""),
    ("X-Request-ID", ""),
];
