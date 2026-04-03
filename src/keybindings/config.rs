use std::collections::HashMap;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use serde::Deserialize;

// ── Key representation ─────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct KeyBind {
    pub code: KeyCode,
    pub modifiers: KeyModifiers,
}

#[allow(dead_code)]
impl KeyBind {
    pub fn new(code: KeyCode, modifiers: KeyModifiers) -> Self {
        Self { code, modifiers }
    }

    pub fn char(c: char) -> Self {
        Self { code: KeyCode::Char(c), modifiers: KeyModifiers::NONE }
    }

    pub fn ctrl(c: char) -> Self {
        Self { code: KeyCode::Char(c), modifiers: KeyModifiers::CONTROL }
    }

    pub fn key(code: KeyCode) -> Self {
        Self { code, modifiers: KeyModifiers::NONE }
    }

    /// Normalize a crossterm KeyEvent into a KeyBind.
    /// Strips SHIFT from uppercase letter chars since crossterm reports
    /// Shift+g as KeyCode::Char('G') with SHIFT modifier.
    pub fn from_event(event: KeyEvent) -> Self {
        let mut modifiers = event.modifiers;
        let code = event.code;

        // Normalize: strip SHIFT for characters that already encode the shift
        // (uppercase letters, shifted symbols like : ? ! @ # $ etc.)
        if let KeyCode::Char(c) = code {
            if c.is_ascii_uppercase() || !c.is_ascii_lowercase() {
                modifiers -= KeyModifiers::SHIFT;
            }
        }
        // Normalize: BackTab already implies Shift, strip it
        if code == KeyCode::BackTab {
            modifiers -= KeyModifiers::SHIFT;
        }

        Self { code, modifiers }
    }

    /// Parse a key string like "Ctrl+r", "Shift+g", "Esc", "j", "$"
    pub fn parse(s: &str) -> Result<Self, String> {
        let s = s.trim();
        let parts: Vec<&str> = s.split('+').collect();

        let mut modifiers = KeyModifiers::NONE;
        let key_part;

        if parts.len() == 1 {
            key_part = parts[0];
        } else {
            // All but last are modifiers
            for &part in &parts[..parts.len() - 1] {
                match part.to_lowercase().as_str() {
                    "ctrl" | "control" => modifiers |= KeyModifiers::CONTROL,
                    "alt" => modifiers |= KeyModifiers::ALT,
                    "shift" => modifiers |= KeyModifiers::SHIFT,
                    _ => return Err(format!("Unknown modifier: {}", part)),
                }
            }
            key_part = parts[parts.len() - 1];
        }

        let code = match key_part {
            "Esc" | "Escape" => KeyCode::Esc,
            "Enter" | "Return" | "CR" => KeyCode::Enter,
            "Tab" => KeyCode::Tab,
            "BackTab" => KeyCode::BackTab,
            "Backspace" | "BS" => KeyCode::Backspace,
            "Delete" | "Del" => KeyCode::Delete,
            "Up" => KeyCode::Up,
            "Down" => KeyCode::Down,
            "Left" => KeyCode::Left,
            "Right" => KeyCode::Right,
            "Home" => KeyCode::Home,
            "End" => KeyCode::End,
            "PageUp" => KeyCode::PageUp,
            "PageDown" => KeyCode::PageDown,
            "Space" => KeyCode::Char(' '),
            s if s.starts_with('F') && s.len() > 1 => {
                if let Ok(n) = s[1..].parse::<u8>() {
                    KeyCode::F(n)
                } else {
                    KeyCode::Char('F')
                }
            }
            s if s.chars().count() == 1 => {
                let c = s.chars().next().unwrap();
                // If Shift modifier was explicit and char is lowercase, uppercase it
                if modifiers.contains(KeyModifiers::SHIFT) && c.is_ascii_lowercase() {
                    modifiers -= KeyModifiers::SHIFT;
                    KeyCode::Char(c.to_ascii_uppercase())
                } else {
                    KeyCode::Char(c)
                }
            }
            _ => return Err(format!("Unknown key: {}", key_part)),
        };

        Ok(Self { code, modifiers })
    }

    /// Convert back to display string
    pub fn to_string_repr(&self) -> String {
        let mut parts = Vec::new();
        if self.modifiers.contains(KeyModifiers::CONTROL) {
            parts.push("Ctrl".to_string());
        }
        if self.modifiers.contains(KeyModifiers::ALT) {
            parts.push("Alt".to_string());
        }
        let key_str = match self.code {
            KeyCode::Esc => "Esc".to_string(),
            KeyCode::Enter => "Enter".to_string(),
            KeyCode::Tab => "Tab".to_string(),
            KeyCode::BackTab => "BackTab".to_string(),
            KeyCode::Backspace => "Backspace".to_string(),
            KeyCode::Delete => "Delete".to_string(),
            KeyCode::Up => "Up".to_string(),
            KeyCode::Down => "Down".to_string(),
            KeyCode::Left => "Left".to_string(),
            KeyCode::Right => "Right".to_string(),
            KeyCode::Home => "Home".to_string(),
            KeyCode::End => "End".to_string(),
            KeyCode::PageUp => "PageUp".to_string(),
            KeyCode::PageDown => "PageDown".to_string(),
            KeyCode::F(n) => format!("F{}", n),
            KeyCode::Char(' ') => "Space".to_string(),
            KeyCode::Char(c) => {
                if c.is_ascii_uppercase() && !self.modifiers.contains(KeyModifiers::CONTROL) {
                    format!("Shift+{}", c.to_ascii_lowercase())
                } else {
                    c.to_string()
                }
            }
            _ => "?".to_string(),
        };
        parts.push(key_str);
        parts.join("+")
    }
}

// ── TOML deserialization ───────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum TomlKeyValue {
    Single(String),
    Multiple(Vec<String>),
}

impl TomlKeyValue {
    fn keys(&self) -> Vec<&str> {
        match self {
            TomlKeyValue::Single(s) => vec![s.as_str()],
            TomlKeyValue::Multiple(v) => v.iter().map(|s| s.as_str()).collect(),
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct KeybindingsToml {
    #[serde(default)]
    pub global: HashMap<String, TomlKeyValue>,
    #[serde(default)]
    pub collections: HashMap<String, TomlKeyValue>,
    #[serde(default)]
    pub request: HashMap<String, TomlKeyValue>,
    #[serde(default)]
    pub request_field: HashMap<String, TomlKeyValue>,
    #[serde(default)]
    pub body: HashMap<String, TomlKeyValue>,
    #[serde(default)]
    pub response_body: HashMap<String, TomlKeyValue>,
    #[serde(default)]
    pub response_type_preview: HashMap<String, TomlKeyValue>,
    #[serde(default)]
    pub response_type_editor: HashMap<String, TomlKeyValue>,
    #[serde(default)]
    pub visual: HashMap<String, TomlKeyValue>,
    #[serde(default)]
    pub insert: HashMap<String, TomlKeyValue>,
    #[serde(default)]
    pub command_palette: HashMap<String, TomlKeyValue>,
    #[serde(default)]
    pub search: HashMap<String, TomlKeyValue>,
    #[serde(default)]
    pub collections_filter: HashMap<String, TomlKeyValue>,
    #[serde(default)]
    pub overlay: HashMap<String, TomlKeyValue>,
}

// ── Resolved runtime config ────────────────────────────────────────────────

/// Built once at startup, used for O(1) key lookups.
pub struct KeybindingsConfig {
    pub global: HashMap<KeyBind, String>,
    pub collections: HashMap<KeyBind, String>,
    pub request: HashMap<KeyBind, String>,
    pub request_field: HashMap<KeyBind, String>,
    pub body: HashMap<KeyBind, String>,
    pub response_body: HashMap<KeyBind, String>,
    pub response_type_preview: HashMap<KeyBind, String>,
    pub response_type_editor: HashMap<KeyBind, String>,
    pub visual: HashMap<KeyBind, String>,
    pub insert: HashMap<KeyBind, String>,
    pub command_palette: HashMap<KeyBind, String>,
    pub search: HashMap<KeyBind, String>,
    pub collections_filter: HashMap<KeyBind, String>,
    pub overlay: HashMap<KeyBind, String>,
}

impl KeybindingsConfig {
    pub fn lookup<'a>(context: &'a HashMap<KeyBind, String>, key: &KeyBind) -> Option<&'a str> {
        context.get(key).map(|s| s.as_str())
    }
}

// ── Default bindings ───────────────────────────────────────────────────────

/// Helper: insert one or more keys for an action into a context map
fn bind(map: &mut HashMap<String, TomlKeyValue>, action: &str, keys: &[&str]) {
    let val = if keys.len() == 1 {
        TomlKeyValue::Single(keys[0].to_string())
    } else {
        TomlKeyValue::Multiple(keys.iter().map(|s| s.to_string()).collect())
    };
    map.insert(action.to_string(), val);
}

pub fn default_bindings() -> KeybindingsToml {
    let mut t = KeybindingsToml::default();

    // ── Global ──
    {
        let m = &mut t.global;
        bind(m, "quit", &["q"]);
        bind(m, "help", &["?", "F1"]);
        bind(m, "ctrl_r", &["Ctrl+r"]);  // context-dependent: redo in vim edit, execute request otherwise
        bind(m, "ctrl_v", &["Ctrl+v"]); // context-dependent: visual block in normal, paste in insert
        bind(m, "scroll_half_down", &["Ctrl+d"]);
        bind(m, "scroll_half_up", &["Ctrl+u"]);
        bind(m, "save_request", &["Ctrl+s"]);
        bind(m, "toggle_insecure", &["Ctrl+t"]);
        bind(m, "navigate_left", &["Ctrl+h"]);
        bind(m, "navigate_down", &["Ctrl+j"]);
        bind(m, "navigate_up", &["Ctrl+k"]);
        bind(m, "navigate_right", &["Ctrl+l"]);
        bind(m, "open_theme_selector", &["Shift+t"]);
        bind(m, "open_history", &["Shift+h"]);
        bind(m, "open_env_editor", &["Shift+e"]);
        bind(m, "open_command_palette", &[":", "Ctrl+p"]);
        bind(m, "focus_panel_1", &["1"]);
        bind(m, "focus_panel_2", &["2"]);
        bind(m, "focus_panel_3", &["3"]);
        bind(m, "focus_panel_4", &["4"]);
        bind(m, "cancel_request", &["Esc"]);
    }

    // ── Collections ──
    {
        let m = &mut t.collections;
        bind(m, "scroll_down", &["j", "Down"]);
        bind(m, "scroll_up", &["k", "Up"]);
        bind(m, "scroll_top", &["g"]);
        bind(m, "scroll_bottom", &["Shift+g"]);
        bind(m, "toggle_collapse", &["Space"]);
        bind(m, "select_request", &["Enter"]);
        bind(m, "create_collection", &["n"]);
        bind(m, "save_request", &["s"]);
        bind(m, "save_request_as", &["Shift+s"]);
        bind(m, "add_request", &["a"]);
        bind(m, "new_empty_request", &["Shift+c"]);
        bind(m, "rename_request", &["r"]);
        bind(m, "delete_pending", &["d"]);
        bind(m, "move_request", &["m"]);
        bind(m, "yank_pending", &["y"]);
        bind(m, "paste_request", &["p"]);
        bind(m, "copy_as_curl", &["Shift+y"]);
        bind(m, "next_collection", &["Shift+l", "}"]);
        bind(m, "prev_collection", &["Shift+h", "{"]);
        bind(m, "start_filter", &["/"]);
        bind(m, "fold_pending", &["z"]);
    }

    // ── Request (panel navigation, not field editing) ──
    {
        let m = &mut t.request;
        bind(m, "focus_down", &["j", "Down"]);
        bind(m, "focus_up", &["k", "Up"]);
        bind(m, "next_method", &["]"]);
        bind(m, "prev_method", &["["]);
        bind(m, "next_tab", &["}"]);
        bind(m, "prev_tab", &["{"]);
        bind(m, "toggle_enabled", &["Space"]);
        bind(m, "enter_field_edit", &["e"]);
        bind(m, "add_item", &["a"]);
        bind(m, "show_autocomplete", &["Shift+a"]);
        bind(m, "delete_pending", &["d"]);
        bind(m, "delete_item", &["x"]);
        bind(m, "open_env_selector", &["p"]);
        bind(m, "copy_response", &["y"]);
        bind(m, "copy_as_curl", &["Shift+y"]);
    }

    // ── Request field edit (vim normal inside a field) ──
    {
        let m = &mut t.request_field;
        bind(m, "cursor_left", &["h", "Left"]);
        bind(m, "cursor_right", &["l", "Right"]);
        bind(m, "word_forward", &["w"]);
        bind(m, "word_backward", &["b"]);
        bind(m, "word_end", &["e"]);
        bind(m, "line_home", &["0", "Home"]);
        bind(m, "line_end", &["$", "End"]);
        bind(m, "enter_insert", &["i"]);
        bind(m, "enter_insert_start", &["Shift+i"]);
        bind(m, "enter_append", &["a"]);
        bind(m, "enter_append_end", &["Shift+a"]);
        bind(m, "enter_visual", &["v"]);
        bind(m, "delete_char", &["x"]);
        bind(m, "substitute", &["s"]);
        bind(m, "change_to_end", &["Shift+c"]);
        bind(m, "delete_to_end", &["Shift+d"]);
        bind(m, "change_pending", &["c"]);
        bind(m, "delete_pending", &["d"]);
        bind(m, "replace_pending", &["r"]);
        bind(m, "yank_pending", &["y"]);
        bind(m, "undo", &["u"]);
        bind(m, "paste", &["p", "Shift+p"]);
        bind(m, "find_forward", &["f"]);
        bind(m, "find_backward", &["Shift+f"]);
        bind(m, "find_before", &["t"]);
        bind(m, "find_after", &["Shift+t"]);
        bind(m, "tab", &["Tab"]);
        bind(m, "exit_field_edit", &["Esc"]);
    }

    // ── Body (app-level only; all vim keys handled by vimltui) ──
    {
        let m = &mut t.body;
        bind(m, "next_tab", &["}"]);
        bind(m, "prev_tab", &["{"]);
        bind(m, "start_search", &["/"]);
        bind(m, "search_next", &["n"]);
        bind(m, "search_prev", &["Shift+n"]);
    }

    // ── Response Body tab ──
    {
        let m = &mut t.response_body;
        bind(m, "scroll_down", &["j", "Down"]);
        bind(m, "scroll_up", &["k", "Up"]);
        bind(m, "cursor_left", &["h", "Left"]);
        bind(m, "cursor_right", &["l", "Right"]);
        bind(m, "scroll_top", &["g"]);
        bind(m, "scroll_bottom", &["Shift+g"]);
        bind(m, "word_forward", &["w"]);
        bind(m, "word_backward", &["b"]);
        bind(m, "word_end", &["e"]);
        bind(m, "line_home", &["0", "Home"]);
        bind(m, "line_end", &["$", "End"]);
        bind(m, "enter_visual", &["v"]);
        bind(m, "copy_response", &["y"]);
        bind(m, "copy_as_curl", &["Shift+y"]);
        bind(m, "find_forward", &["f"]);
        bind(m, "find_backward", &["Shift+f"]);
        bind(m, "find_before", &["t"]);
        bind(m, "find_after", &["Shift+t"]);
        bind(m, "open_env_selector", &["Shift+p"]);
        bind(m, "toggle_headers", &["Shift+e"]);
        bind(m, "response_history", &["Shift+h"]);
        bind(m, "response_diff", &["Shift+d"]);
        bind(m, "start_search", &["/"]);
        bind(m, "search_next", &["n"]);
        bind(m, "search_prev", &["Shift+n"]);
        bind(m, "next_tab", &["}"]);
        bind(m, "prev_tab", &["{"]);
        bind(m, "toggle_wrap", &["F2"]);
        bind(m, "export_response", &["Shift+o"]);
    }

    // ── Response Type Preview ──
    {
        let m = &mut t.response_type_preview;
        bind(m, "scroll_down", &["j", "Down"]);
        bind(m, "scroll_up", &["k", "Up"]);
        bind(m, "cursor_left", &["h", "Left"]);
        bind(m, "cursor_right", &["l", "Right"]);
        bind(m, "scroll_top", &["g"]);
        bind(m, "scroll_bottom", &["Shift+g"]);
        bind(m, "word_forward", &["w"]);
        bind(m, "word_backward", &["b"]);
        bind(m, "word_end", &["e"]);
        bind(m, "line_home", &["0", "Home"]);
        bind(m, "line_end", &["$", "End"]);
        bind(m, "enter_visual", &["v"]);
        bind(m, "copy_response", &["y"]);
        bind(m, "find_forward", &["f"]);
        bind(m, "find_backward", &["Shift+f"]);
        bind(m, "find_before", &["t"]);
        bind(m, "find_after", &["Shift+t"]);
        bind(m, "start_search", &["/"]);
        bind(m, "search_next", &["n"]);
        bind(m, "search_prev", &["Shift+n"]);
        bind(m, "next_tab", &["}"]);
        bind(m, "prev_tab", &["{"]);
        bind(m, "type_lang_next", &["]"]);
        bind(m, "type_lang_prev", &["["]);
        bind(m, "toggle_wrap", &["F2"]);
        bind(m, "export_response", &["Shift+o"]);
    }

    // ── Response Type Editor ──
    {
        let m = &mut t.response_type_editor;
        bind(m, "scroll_down", &["j", "Down"]);
        bind(m, "scroll_up", &["k", "Up"]);
        bind(m, "cursor_left", &["h", "Left"]);
        bind(m, "cursor_right", &["l", "Right"]);
        bind(m, "scroll_top", &["g"]);
        bind(m, "scroll_bottom", &["Shift+g"]);
        bind(m, "word_forward", &["w"]);
        bind(m, "word_backward", &["b"]);
        bind(m, "word_end", &["e"]);
        bind(m, "line_home", &["0", "Home"]);
        bind(m, "line_end", &["$", "End"]);
        bind(m, "enter_insert", &["i"]);
        bind(m, "enter_insert_start", &["Shift+i"]);
        bind(m, "enter_append", &["a"]);
        bind(m, "enter_append_end", &["Shift+a"]);
        bind(m, "open_line_below", &["o"]);
        bind(m, "open_line_above", &["Shift+o"]);
        bind(m, "enter_visual", &["v"]);
        bind(m, "delete_char", &["x"]);
        bind(m, "substitute", &["s"]);
        bind(m, "change_line", &["Shift+s"]);
        bind(m, "change_to_end", &["Shift+c"]);
        bind(m, "delete_to_end_line", &["Shift+d"]);
        bind(m, "change_pending", &["c"]);
        bind(m, "replace_pending", &["r"]);
        bind(m, "delete_pending", &["d"]);
        bind(m, "yank_pending", &["y"]);
        bind(m, "undo", &["u"]);
        bind(m, "paste", &["p", "Shift+p"]);
        bind(m, "find_forward", &["f"]);
        bind(m, "find_backward", &["Shift+f"]);
        bind(m, "find_before", &["t"]);
        bind(m, "find_after", &["Shift+t"]);
        bind(m, "regenerate_type", &["Shift+r"]);
        bind(m, "next_tab", &["}"]);
        bind(m, "prev_tab", &["{"]);
        bind(m, "type_lang_next", &["]"]);
        bind(m, "type_lang_prev", &["["]);
        bind(m, "toggle_wrap", &["Shift+w"]);
        bind(m, "export_response", &["Shift+o"]);
    }

    // ── Visual mode ──
    {
        let m = &mut t.visual;
        bind(m, "exit_visual", &["Esc"]);
        bind(m, "yank", &["y"]);
        bind(m, "delete", &["d", "x"]);
        bind(m, "paste", &["p", "Shift+p"]);
        bind(m, "cursor_down", &["j", "Down"]);
        bind(m, "cursor_up", &["k", "Up"]);
        bind(m, "cursor_left", &["h", "Left"]);
        bind(m, "cursor_right", &["l", "Right"]);
        bind(m, "word_forward", &["w"]);
        bind(m, "word_backward", &["b"]);
        bind(m, "word_end", &["e"]);
        bind(m, "scroll_top", &["g"]);
        bind(m, "scroll_bottom", &["Shift+g"]);
        bind(m, "line_home", &["0", "Home"]);
        bind(m, "line_end", &["$", "End"]);
        bind(m, "find_forward", &["f"]);
        bind(m, "find_backward", &["Shift+f"]);
        bind(m, "find_before", &["t"]);
        bind(m, "find_after", &["Shift+t"]);
        bind(m, "navigate_left", &["Ctrl+h"]);
        bind(m, "navigate_down", &["Ctrl+j"]);
        bind(m, "navigate_up", &["Ctrl+k"]);
        bind(m, "navigate_right", &["Ctrl+l"]);
    }

    // ── Insert mode ──
    {
        let m = &mut t.insert;
        bind(m, "exit_insert", &["Esc"]);
        bind(m, "navigate_left", &["Ctrl+h"]);
        bind(m, "navigate_down", &["Ctrl+j"]);
        bind(m, "navigate_up", &["Ctrl+k"]);
        bind(m, "navigate_right", &["Ctrl+l"]);
        bind(m, "autocomplete_next", &["Ctrl+n"]);
        bind(m, "autocomplete_prev", &["Ctrl+p"]);
        bind(m, "autocomplete_accept", &["Ctrl+y"]);
    }

    // ── Command palette ──
    {
        let m = &mut t.command_palette;
        bind(m, "close", &["Esc"]);
        bind(m, "confirm", &["Enter"]);
        bind(m, "nav_up", &["Up", "BackTab", "Ctrl+p", "Ctrl+k"]);
        bind(m, "nav_down", &["Down", "Tab", "Ctrl+n", "Ctrl+j"]);
    }

    // ── Search ──
    {
        let m = &mut t.search;
        bind(m, "cancel", &["Esc"]);
        bind(m, "confirm", &["Enter"]);
    }

    // ── Collections filter ──
    {
        let m = &mut t.collections_filter;
        bind(m, "cancel", &["Esc"]);
        bind(m, "confirm", &["Enter"]);
    }

    // ── Overlay ──
    {
        let m = &mut t.overlay;
        bind(m, "close", &["Esc"]);
        bind(m, "nav_down", &["j", "Down", "Ctrl+n", "Ctrl+j"]);
        bind(m, "nav_up", &["k", "Up", "Ctrl+p", "Ctrl+k"]);
        bind(m, "confirm", &["Enter"]);
    }

    t
}

// ── Build resolved config ──────────────────────────────────────────────────

/// Build a reverse map: key -> action name, from a TOML section (action -> keys).
fn resolve_section(section: &HashMap<String, TomlKeyValue>) -> HashMap<KeyBind, String> {
    let mut map = HashMap::new();
    for (action, val) in section {
        for key_str in val.keys() {
            if let Ok(kb) = KeyBind::parse(key_str) {
                map.insert(kb, action.clone());
            }
        }
    }
    map
}

/// Merge user overrides into defaults. For each action the user defines,
/// their keys completely replace the defaults for that action.
fn merge_section(
    defaults: &HashMap<String, TomlKeyValue>,
    user: &HashMap<String, TomlKeyValue>,
) -> HashMap<String, TomlKeyValue> {
    let mut merged = defaults.clone();
    for (action, val) in user {
        merged.insert(action.clone(), val.clone());
    }
    merged
}

pub fn build_config(user: Option<KeybindingsToml>) -> KeybindingsConfig {
    let defaults = default_bindings();
    let user = user.unwrap_or_default();

    let merge_and_resolve = |d: &HashMap<String, TomlKeyValue>, u: &HashMap<String, TomlKeyValue>| {
        resolve_section(&merge_section(d, u))
    };

    KeybindingsConfig {
        global: merge_and_resolve(&defaults.global, &user.global),
        collections: merge_and_resolve(&defaults.collections, &user.collections),
        request: merge_and_resolve(&defaults.request, &user.request),
        request_field: merge_and_resolve(&defaults.request_field, &user.request_field),
        body: merge_and_resolve(&defaults.body, &user.body),
        response_body: merge_and_resolve(&defaults.response_body, &user.response_body),
        response_type_preview: merge_and_resolve(&defaults.response_type_preview, &user.response_type_preview),
        response_type_editor: merge_and_resolve(&defaults.response_type_editor, &user.response_type_editor),
        visual: merge_and_resolve(&defaults.visual, &user.visual),
        insert: merge_and_resolve(&defaults.insert, &user.insert),
        command_palette: merge_and_resolve(&defaults.command_palette, &user.command_palette),
        search: merge_and_resolve(&defaults.search, &user.search),
        collections_filter: merge_and_resolve(&defaults.collections_filter, &user.collections_filter),
        overlay: merge_and_resolve(&defaults.overlay, &user.overlay),
    }
}

// ── Load from file ─────────────────────────────────────────────────────────

pub fn load_keybindings_toml() -> Result<Option<KeybindingsToml>, String> {
    let path = crate::core::config::config_dir().join("keybindings.toml");
    if !path.exists() {
        return Ok(None);
    }
    let content = std::fs::read_to_string(&path)
        .map_err(|e| format!("Failed to read {}: {}", path.display(), e))?;
    match toml::from_str::<KeybindingsToml>(&content) {
        Ok(toml) => Ok(Some(toml)),
        Err(e) => {
            let msg = format!("keybindings.toml syntax error: {}", e);
            Err(msg)
        }
    }
}

// ── Generate default TOML ──────────────────────────────────────────────────

pub fn generate_default_toml() -> String {
    let defaults = default_bindings();
    let mut out = String::from("# restui keybindings configuration\n# Only include the keys you want to change — defaults are used for the rest.\n# Key format: \"Ctrl+r\", \"Shift+g\", \"Alt+x\", \"Esc\", \"Enter\", \"Tab\", \"Space\"\n# Multiple keys: action = [\"j\", \"Down\"]\n\n");

    fn section_to_toml(out: &mut String, name: &str, section: &HashMap<String, TomlKeyValue>) {
        out.push_str(&format!("[{}]\n", name));
        let mut entries: Vec<_> = section.iter().collect();
        entries.sort_by_key(|(k, _)| (*k).clone());
        for (action, val) in entries {
            match val {
                TomlKeyValue::Single(s) => out.push_str(&format!("{} = \"{}\"\n", action, s)),
                TomlKeyValue::Multiple(v) => {
                    let keys: Vec<String> = v.iter().map(|s| format!("\"{}\"", s)).collect();
                    out.push_str(&format!("{} = [{}]\n", action, keys.join(", ")));
                }
            }
        }
        out.push('\n');
    }

    section_to_toml(&mut out, "global", &defaults.global);
    section_to_toml(&mut out, "collections", &defaults.collections);
    section_to_toml(&mut out, "request", &defaults.request);
    section_to_toml(&mut out, "request_field", &defaults.request_field);
    section_to_toml(&mut out, "body", &defaults.body);
    section_to_toml(&mut out, "response_body", &defaults.response_body);
    section_to_toml(&mut out, "response_type_preview", &defaults.response_type_preview);
    section_to_toml(&mut out, "response_type_editor", &defaults.response_type_editor);
    section_to_toml(&mut out, "visual", &defaults.visual);
    section_to_toml(&mut out, "insert", &defaults.insert);
    section_to_toml(&mut out, "command_palette", &defaults.command_palette);
    section_to_toml(&mut out, "search", &defaults.search);
    section_to_toml(&mut out, "collections_filter", &defaults.collections_filter);
    section_to_toml(&mut out, "overlay", &defaults.overlay);

    out
}
