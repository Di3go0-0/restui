use ratatui::style::Color;
use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Clone, Deserialize)]
struct ThemeFile {
    #[serde(default = "d_border_focused")]
    border_focused: String,
    #[serde(default = "d_border_unfocused")]
    border_unfocused: String,
    #[serde(default = "d_status_ok")]
    status_ok: String,
    #[serde(default = "d_status_client_error")]
    status_client_error: String,
    #[serde(default = "d_status_server_error")]
    status_server_error: String,
    #[serde(default = "d_method_get")]
    method_get: String,
    #[serde(default = "d_method_post")]
    method_post: String,
    #[serde(default = "d_method_put")]
    method_put: String,
    #[serde(default = "d_method_delete")]
    method_delete: String,
    #[serde(default = "d_method_patch")]
    method_patch: String,
    #[serde(default = "d_method_head")]
    method_head: String,
    #[serde(default = "d_method_options")]
    method_options: String,
    #[serde(default = "d_bg_highlight")]
    bg_highlight: String,
    #[serde(default = "d_gutter")]
    gutter: String,
    #[serde(default = "d_gutter_active")]
    gutter_active: String,
    #[serde(default = "d_text")]
    text: String,
    #[serde(default = "d_text_dim")]
    text_dim: String,
    #[serde(default = "d_overlay_bg")]
    overlay_bg: String,
    #[serde(default = "d_accent")]
    accent: String,
    #[serde(default = "d_key_hint")]
    key_hint: String,
    #[serde(default = "d_json_key")]
    json_key: String,
    #[serde(default = "d_json_string")]
    json_string: String,
    #[serde(default = "d_json_number")]
    json_number: String,
    #[serde(default = "d_json_bool")]
    json_bool: String,
}

fn d_border_focused() -> String { "#89b4fa".into() }
fn d_border_unfocused() -> String { "#585b70".into() }
fn d_status_ok() -> String { "#a6e3a1".into() }
fn d_status_client_error() -> String { "#f9e2af".into() }
fn d_status_server_error() -> String { "#f38ba8".into() }
fn d_method_get() -> String { "#a6e3a1".into() }
fn d_method_post() -> String { "#89b4fa".into() }
fn d_method_put() -> String { "#f9e2af".into() }
fn d_method_delete() -> String { "#f38ba8".into() }
fn d_method_patch() -> String { "#f9e2af".into() }
fn d_method_head() -> String { "#cba6f7".into() }
fn d_method_options() -> String { "#89dceb".into() }
fn d_bg_highlight() -> String { "#282830".into() }
fn d_gutter() -> String { "#585b70".into() }
fn d_gutter_active() -> String { "#f9e2af".into() }
fn d_text() -> String { "#cdd6f4".into() }
fn d_text_dim() -> String { "#6c7086".into() }
fn d_overlay_bg() -> String { "#1e1e2e".into() }
fn d_accent() -> String { "#89b4fa".into() }
fn d_key_hint() -> String { "#89b4fa".into() }
fn d_json_key() -> String { "#89b4fa".into() }
fn d_json_string() -> String { "#a6e3a1".into() }
fn d_json_number() -> String { "#fab387".into() }
fn d_json_bool() -> String { "#f9e2af".into() }

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct Theme {
    pub name: String,
    pub border_focused: Color,
    pub border_unfocused: Color,
    pub border_insert: Color,
    pub border_visual: Color,
    pub status_ok: Color,
    pub status_client_error: Color,
    pub status_server_error: Color,
    pub method_get: Color,
    pub method_post: Color,
    pub method_put: Color,
    pub method_delete: Color,
    pub method_patch: Color,
    pub method_head: Color,
    pub method_options: Color,
    pub bg_highlight: Color,
    pub gutter: Color,
    pub gutter_active: Color,
    pub text: Color,
    pub text_dim: Color,
    pub overlay_bg: Color,
    pub accent: Color,
    pub key_hint: Color,
    pub json_key: Color,
    pub json_string: Color,
    pub json_number: Color,
    pub json_bool: Color,
    pub yank_highlight: Color,
}

impl Default for Theme {
    fn default() -> Self {
        Self::load_builtin("default")
    }
}

impl Theme {
    pub fn method_color(&self, method: crate::model::request::HttpMethod) -> Color {
        use crate::model::request::HttpMethod;
        match method {
            HttpMethod::GET => self.method_get,
            HttpMethod::POST => self.method_post,
            HttpMethod::PUT => self.method_put,
            HttpMethod::PATCH => self.method_patch,
            HttpMethod::DELETE => self.method_delete,
            HttpMethod::HEAD => self.method_head,
            HttpMethod::OPTIONS => self.method_options,
        }
    }

    pub fn border_for_mode(&self, focused: bool, mode: crate::core::state::InputMode) -> Color {
        if !focused {
            return self.border_unfocused;
        }
        match mode {
            crate::core::state::InputMode::Insert => self.border_insert,
            crate::core::state::InputMode::Visual | crate::core::state::InputMode::VisualBlock => self.border_visual,
            crate::core::state::InputMode::Normal => self.border_focused,
        }
    }

    fn load_builtin(name: &str) -> Self {
        let path = theme_path(name);
        if path.exists() {
            if let Ok(content) = std::fs::read_to_string(&path) {
                if let Ok(file) = toml::from_str::<ThemeFile>(&content) {
                    return Self::from_file(name, &file);
                }
            }
        }
        Self::fallback(name)
    }

    fn from_file(name: &str, f: &ThemeFile) -> Self {
        Self {
            name: name.to_string(),
            border_focused: parse_hex(&f.border_focused),
            border_unfocused: parse_hex(&f.border_unfocused),
            border_insert: Color::Green,
            border_visual: Color::Magenta,
            status_ok: parse_hex(&f.status_ok),
            status_client_error: parse_hex(&f.status_client_error),
            status_server_error: parse_hex(&f.status_server_error),
            method_get: parse_hex(&f.method_get),
            method_post: parse_hex(&f.method_post),
            method_put: parse_hex(&f.method_put),
            method_delete: parse_hex(&f.method_delete),
            method_patch: parse_hex(&f.method_patch),
            method_head: parse_hex(&f.method_head),
            method_options: parse_hex(&f.method_options),
            bg_highlight: parse_hex(&f.bg_highlight),
            gutter: parse_hex(&f.gutter),
            gutter_active: parse_hex(&f.gutter_active),
            text: parse_hex(&f.text),
            text_dim: parse_hex(&f.text_dim),
            overlay_bg: parse_hex(&f.overlay_bg),
            accent: parse_hex(&f.accent),
            key_hint: parse_hex(&f.key_hint),
            json_key: parse_hex(&f.json_key),
            json_string: parse_hex(&f.json_string),
            json_number: parse_hex(&f.json_number),
            json_bool: parse_hex(&f.json_bool),
            yank_highlight: Color::Rgb(80, 80, 40),
        }
    }

    fn fallback(name: &str) -> Self {
        Self {
            name: name.to_string(),
            border_focused: Color::Cyan,
            border_unfocused: Color::DarkGray,
            border_insert: Color::Green,
            border_visual: Color::Magenta,
            status_ok: Color::Green,
            status_client_error: Color::Yellow,
            status_server_error: Color::Red,
            method_get: Color::Green,
            method_post: Color::Blue,
            method_put: Color::Yellow,
            method_delete: Color::Red,
            method_patch: Color::Yellow,
            method_head: Color::Magenta,
            method_options: Color::Cyan,
            bg_highlight: Color::Rgb(40, 40, 50),
            gutter: Color::DarkGray,
            gutter_active: Color::Yellow,
            text: Color::White,
            text_dim: Color::DarkGray,
            overlay_bg: Color::Rgb(30, 30, 46),
            accent: Color::Cyan,
            key_hint: Color::Cyan,
            json_key: Color::Cyan,
            json_string: Color::Green,
            json_number: Color::Magenta,
            json_bool: Color::Yellow,
            yank_highlight: Color::Rgb(80, 80, 40),
        }
    }

    /// Build a theme from nvim highlight colors passed as key=value pairs.
    /// Format: "bg=#1e1e2e,fg=#cdd6f4,accent=#89b4fa,border=#585b70,..."
    pub fn from_nvim_colors(colors_str: &str) -> Self {
        use std::collections::HashMap;
        let map: HashMap<&str, &str> = colors_str
            .split(',')
            .filter_map(|pair| {
                let mut parts = pair.splitn(2, '=');
                Some((parts.next()?.trim(), parts.next()?.trim()))
            })
            .collect();

        let get = |key: &str| -> Option<Color> {
            map.get(key).map(|v| parse_hex(v))
        };

        // Base colors with fallbacks
        let fg = get("fg").unwrap_or(Color::White);
        let bg = get("bg").unwrap_or(Color::Rgb(30, 30, 46));
        let accent = get("accent").unwrap_or(Color::Cyan);
        let dim = get("dim").unwrap_or(Color::DarkGray);
        let border = get("border").unwrap_or(dim);
        let green = get("green").unwrap_or(Color::Green);
        let yellow = get("yellow").unwrap_or(Color::Yellow);
        let red = get("red").unwrap_or(Color::Red);
        let blue = get("blue").unwrap_or(accent);
        let magenta = get("magenta").unwrap_or(Color::Magenta);
        let cyan = get("cyan").unwrap_or(Color::Cyan);
        let orange = get("orange").unwrap_or(yellow);
        let bg_hl = get("bg_hl").unwrap_or_else(|| lighten_color(bg, 10));
        let gutter = get("gutter").unwrap_or(dim);
        let gutter_active = get("gutter_active").unwrap_or(yellow);
        let string_color = get("string").unwrap_or(green);
        let number_color = get("number").unwrap_or(orange);
        let keyword_color = get("keyword").unwrap_or(accent);
        let boolean_color = get("boolean").unwrap_or(yellow);

        Self {
            name: "nvim".to_string(),
            border_focused: accent,
            border_unfocused: border,
            border_insert: green,
            border_visual: magenta,
            status_ok: green,
            status_client_error: yellow,
            status_server_error: red,
            method_get: green,
            method_post: blue,
            method_put: yellow,
            method_delete: red,
            method_patch: orange,
            method_head: magenta,
            method_options: cyan,
            bg_highlight: bg_hl,
            gutter,
            gutter_active,
            text: fg,
            text_dim: dim,
            overlay_bg: bg,
            accent,
            key_hint: accent,
            json_key: keyword_color,
            json_string: string_color,
            json_number: number_color,
            json_bool: boolean_color,
            yank_highlight: Color::Rgb(80, 80, 40),
        }
    }
}

fn lighten_color(color: Color, amount: u8) -> Color {
    match color {
        Color::Rgb(r, g, b) => Color::Rgb(
            r.saturating_add(amount),
            g.saturating_add(amount),
            b.saturating_add(amount),
        ),
        _ => Color::Rgb(40, 40, 50),
    }
}

pub const THEME_NAMES: &[&str] = &["default", "catppuccin", "gruvbox", "tokyonight", "light", "dracula", "nord", "solarized"];

pub fn load_theme(name: &str) -> Theme {
    Theme::load_builtin(name)
}

pub fn next_theme_name(current: &str) -> &'static str {
    let idx = THEME_NAMES.iter().position(|&n| n == current).unwrap_or(0);
    THEME_NAMES[(idx + 1) % THEME_NAMES.len()]
}

fn theme_path(name: &str) -> PathBuf {
    let filename = format!("themes/{}.toml", name);

    // 1. Relative to CWD
    let p = PathBuf::from(&filename);
    if p.exists() { return p; }

    // 2. User config dir (~/.config/restui/themes/)
    let p = crate::core::config::config_dir().join(&filename);
    if p.exists() { return p; }

    // 3. Next to the executable
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let p = dir.join(&filename);
            if p.exists() { return p; }
        }
    }

    // 4. Compiled-in source directory (for cargo run from any CWD)
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(&filename);
    if p.exists() { return p; }

    PathBuf::from(&filename)
}

fn parse_hex(hex: &str) -> Color {
    let hex = hex.trim_start_matches('#');
    if hex.len() == 6 {
        let r = u8::from_str_radix(&hex[0..2], 16).unwrap_or(128);
        let g = u8::from_str_radix(&hex[2..4], 16).unwrap_or(128);
        let b = u8::from_str_radix(&hex[4..6], 16).unwrap_or(128);
        Color::Rgb(r, g, b)
    } else {
        Color::DarkGray
    }
}
