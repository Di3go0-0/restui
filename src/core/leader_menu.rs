use crate::core::action::Action;
use crate::core::state::Panel;

/// Represents a single entry in the leader menu
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct LeaderEntry {
    pub key: char,
    pub description: String,
    pub action: Option<Action>,
    pub submenu: Option<Vec<LeaderEntry>>,
}

/// Tracks the state of the leader menu
#[derive(Debug, Clone)]
pub struct LeaderContext {
    pub active: bool,
    pub menu_visible: bool,
    pub key_sequence: Vec<char>,
    pub pressed_at: Option<std::time::Instant>,
}

impl LeaderContext {
    pub fn new() -> Self {
        Self {
            active: false,
            menu_visible: false,
            key_sequence: Vec::new(),
            pressed_at: None,
        }
    }

    pub fn activate(&mut self) {
        self.active = true;
        self.menu_visible = true;
        self.key_sequence.clear();
        self.pressed_at = Some(std::time::Instant::now());
    }

    pub fn deactivate(&mut self) {
        self.active = false;
        self.menu_visible = false;
        self.key_sequence.clear();
        self.pressed_at = None;
    }

    /// Check if menu has timed out (5 seconds)
    #[allow(dead_code)]
    pub fn is_timed_out(&self) -> bool {
        if let Some(pressed_at) = self.pressed_at {
            pressed_at.elapsed().as_secs() > 5
        } else {
            false
        }
    }
}

impl Default for LeaderContext {
    fn default() -> Self {
        Self::new()
    }
}

/// The leader menu system with mappings for all panels
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct LeaderMenu {
    pub global_entries: Vec<LeaderEntry>,
    pub panel_entries: std::collections::HashMap<Panel, Vec<LeaderEntry>>,
}

impl LeaderMenu {
    pub fn new() -> Self {
        let global_entries = vec![
            LeaderEntry {
                key: 'q',
                description: "Quit application".to_string(),
                action: Some(Action::Quit),
                submenu: None,
            },
            LeaderEntry {
                key: 't',
                description: "Toggle theme selector".to_string(),
                action: Some(Action::CycleTheme),
                submenu: None,
            },
            LeaderEntry {
                key: 'e',
                description: "Open environment selector".to_string(),
                action: Some(Action::OpenOverlay(crate::core::state::Overlay::EnvironmentSelector)),
                submenu: None,
            },
            LeaderEntry {
                key: 'c',
                description: "Open collections manager".to_string(),
                action: Some(Action::OpenOverlay(crate::core::state::Overlay::ThemeSelector { selected: 0 })),
                submenu: None,
            },
            LeaderEntry {
                key: 'h',
                description: "Show help overlay".to_string(),
                action: Some(Action::OpenOverlay(crate::core::state::Overlay::Help)),
                submenu: None,
            },
            LeaderEntry {
                key: '?',
                description: "Show all keybindings".to_string(),
                action: Some(Action::OpenOverlay(crate::core::state::Overlay::EnvironmentEditor {
                    selected: 0,
                    editing_key: false,
                    new_key: String::new(),
                    new_value: String::new(),
                    cursor: 0,
                })),
                submenu: None,
            },
            LeaderEntry {
                key: ':',
                description: "Open command palette".to_string(),
                action: Some(Action::OpenCommandPalette),
                submenu: None,
            },
            LeaderEntry {
                key: '1',
                description: "Navigate to panel 1 (Collections)".to_string(),
                action: Some(Action::FocusPanel(Panel::Collections)),
                submenu: None,
            },
            LeaderEntry {
                key: '2',
                description: "Navigate to panel 2 (Request)".to_string(),
                action: Some(Action::FocusPanel(Panel::Request)),
                submenu: None,
            },
            LeaderEntry {
                key: '3',
                description: "Navigate to panel 3 (Body)".to_string(),
                action: Some(Action::FocusPanel(Panel::Body)),
                submenu: None,
            },
            LeaderEntry {
                key: '4',
                description: "Navigate to panel 4 (Response)".to_string(),
                action: Some(Action::FocusPanel(Panel::Response)),
                submenu: None,
            },
        ];

        let mut panel_entries = std::collections::HashMap::new();

        // Panel 1 (Collections)
        panel_entries.insert(
            Panel::Collections,
            vec![
                LeaderEntry {
                    key: 'n',
                    description: "New collection".to_string(),
                    action: None,
                    submenu: None,
                },
                LeaderEntry {
                    key: 'd',
                    description: "Delete selected item".to_string(),
                    action: None,
                    submenu: None,
                },
                LeaderEntry {
                    key: 'r',
                    description: "Rename selected item".to_string(),
                    action: None,
                    submenu: None,
                },
                LeaderEntry {
                    key: 's',
                    description: "Save request".to_string(),
                    action: None,
                    submenu: None,
                },
                LeaderEntry {
                    key: 'S',
                    description: "Save request as new".to_string(),
                    action: None,
                    submenu: None,
                },
                LeaderEntry {
                    key: 'm',
                    description: "Move request to another collection".to_string(),
                    action: None,
                    submenu: None,
                },
                LeaderEntry {
                    key: 'a',
                    description: "Add new request to collection".to_string(),
                    action: None,
                    submenu: None,
                },
            ],
        );

        // Panel 2 (Request)
        panel_entries.insert(
            Panel::Request,
            vec![
                LeaderEntry {
                    key: 'h',
                    description: "Add/manage headers".to_string(),
                    action: Some(Action::AddHeader),
                    submenu: None,
                },
                LeaderEntry {
                    key: 'c',
                    description: "Add/manage cookies".to_string(),
                    action: Some(Action::AddCookie),
                    submenu: None,
                },
                LeaderEntry {
                    key: 'q',
                    description: "Add/manage query params".to_string(),
                    action: Some(Action::AddParam),
                    submenu: None,
                },
                LeaderEntry {
                    key: 'p',
                    description: "Add/manage path params".to_string(),
                    action: Some(Action::AddPathParam),
                    submenu: None,
                },
                LeaderEntry {
                    key: 'M',
                    description: "Change HTTP method".to_string(),
                    action: Some(Action::NextMethod),
                    submenu: None,
                },
                LeaderEntry {
                    key: 's',
                    description: "Save current request".to_string(),
                    action: Some(Action::SaveRequest),
                    submenu: None,
                },
                LeaderEntry {
                    key: 'b',
                    description: "Toggle body type".to_string(),
                    action: Some(Action::CycleBodyType),
                    submenu: None,
                },
            ],
        );

        // Panel 3 (Body)
        panel_entries.insert(
            Panel::Body,
            vec![
                LeaderEntry {
                    key: 'f',
                    description: "Format/prettify body".to_string(),
                    action: None,
                    submenu: None,
                },
                LeaderEntry {
                    key: 'c',
                    description: "Copy body to clipboard".to_string(),
                    action: None,
                    submenu: None,
                },
                LeaderEntry {
                    key: 'b',
                    description: "Clear body".to_string(),
                    action: None,
                    submenu: None,
                },
                LeaderEntry {
                    key: 't',
                    description: "Change body type".to_string(),
                    action: Some(Action::CycleBodyType),
                    submenu: None,
                },
                LeaderEntry {
                    key: 'j',
                    description: "Focus on body editor".to_string(),
                    action: Some(Action::EnterInsertMode),
                    submenu: None,
                },
                LeaderEntry {
                    key: 's',
                    description: "Save request".to_string(),
                    action: Some(Action::SaveRequest),
                    submenu: None,
                },
                LeaderEntry {
                    key: 'v',
                    description: "Visual mode".to_string(),
                    action: Some(Action::EnterVisualMode),
                    submenu: None,
                },
            ],
        );

        // Panel 4 (Response)
        panel_entries.insert(
            Panel::Response,
            vec![
                LeaderEntry {
                    key: 'h',
                    description: "Toggle headers expandible".to_string(),
                    action: Some(Action::ToggleResponseHeaders),
                    submenu: None,
                },
                LeaderEntry {
                    key: 'd',
                    description: "View diff vs previous response".to_string(),
                    action: None,
                    submenu: None,
                },
                LeaderEntry {
                    key: 'H',
                    description: "Show response history overlay".to_string(),
                    action: Some(Action::OpenOverlay(crate::core::state::Overlay::ResponseHistory { selected: 0 })),
                    submenu: None,
                },
                LeaderEntry {
                    key: 'c',
                    description: "Copy response body".to_string(),
                    action: Some(Action::CopyResponseBody),
                    submenu: None,
                },
                LeaderEntry {
                    key: 'e',
                    description: "Export response to file".to_string(),
                    action: Some(Action::ExportResponse),
                    submenu: None,
                },
                LeaderEntry {
                    key: 'w',
                    description: "Toggle wrap text".to_string(),
                    action: Some(Action::ToggleWrap),
                    submenu: None,
                },
                LeaderEntry {
                    key: 'D',
                    description: "Download response".to_string(),
                    action: None,
                    submenu: None,
                },
                LeaderEntry {
                    key: 't',
                    description: "Show type tab".to_string(),
                    action: Some(Action::ResponseNextTab),
                    submenu: None,
                },
                LeaderEntry {
                    key: 's',
                    description: "Search in response".to_string(),
                    action: Some(Action::StartSearch),
                    submenu: None,
                },
                LeaderEntry {
                    key: 'j',
                    description: "Format JSON".to_string(),
                    action: None,
                    submenu: None,
                },
            ],
        );

        Self {
            global_entries,
            panel_entries,
        }
    }

    /// Get all entries for current panel (global + panel-specific)
    #[allow(dead_code)]
    pub fn get_entries_for_panel(&self, panel: Panel) -> Vec<LeaderEntry> {
        let mut entries = self.global_entries.clone();
        if let Some(panel_specific) = self.panel_entries.get(&panel) {
            entries.extend(panel_specific.clone());
        }
        entries
    }

    /// Find entry by key from sequence
    #[allow(dead_code)]
    pub fn find_entry(&self, panel: Panel, key: char) -> Option<LeaderEntry> {
        let entries = self.get_entries_for_panel(panel);
        entries.into_iter().find(|e| e.key == key)
    }

    /// Build leader menu from keybindings config (for future use)
    #[allow(dead_code)]
    pub fn from_keybindings_config(_kb_config: &crate::keybindings::config::KeybindingsConfig) -> Self {
        Self::new()
    }
}

impl Default for LeaderMenu {
    fn default() -> Self {
        Self::new()
    }
}
