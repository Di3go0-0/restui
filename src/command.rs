use crate::action::Action;
use crate::state::Overlay;
use crate::theme::THEME_NAMES;

#[derive(Debug, Clone)]
pub struct Command {
    pub name: &'static str,
    pub description: &'static str,
    pub category: &'static str,
    pub action: Action,
}

/// Central registry of all commands available in the palette.
pub fn all_commands() -> Vec<Command> {
    let mut cmds = vec![
        // HTTP
        Command {
            name: "Run Request",
            description: "Execute the current HTTP request",
            category: "HTTP",
            action: Action::ExecuteRequest,
        },
        Command {
            name: "Next Method",
            description: "Cycle HTTP method forward (GET -> POST -> ...)",
            category: "HTTP",
            action: Action::NextMethod,
        },
        Command {
            name: "Previous Method",
            description: "Cycle HTTP method backward",
            category: "HTTP",
            action: Action::PrevMethod,
        },

        // Collections
        Command {
            name: "Select Request",
            description: "Open selected request from collection",
            category: "Collections",
            action: Action::SelectRequest,
        },
        Command {
            name: "New Collection",
            description: "Create a new .http collection file",
            category: "Collections",
            action: Action::CreateCollection,
        },
        Command {
            name: "Save Request",
            description: "Overwrite selected request in collection",
            category: "Collections",
            action: Action::SaveRequest,
        },
        Command {
            name: "Save Request As",
            description: "Save current request as new entry",
            category: "Collections",
            action: Action::SaveRequestAs,
        },
        Command {
            name: "New Empty Request",
            description: "Create a blank request from scratch",
            category: "Collections",
            action: Action::NewEmptyRequest,
        },
        Command {
            name: "Next Collection",
            description: "Switch to next collection",
            category: "Collections",
            action: Action::NextCollection,
        },
        Command {
            name: "Previous Collection",
            description: "Switch to previous collection",
            category: "Collections",
            action: Action::PrevCollection,
        },

        // Editing
        Command {
            name: "Add Header",
            description: "Add a new empty header to the request",
            category: "Editing",
            action: Action::AddHeader,
        },
        Command {
            name: "Header Autocomplete",
            description: "Show common headers list",
            category: "Editing",
            action: Action::ShowHeaderAutocomplete,
        },
        Command {
            name: "Cycle Body Type",
            description: "Switch body type (JSON -> XML -> Form -> Raw)",
            category: "Editing",
            action: Action::CycleBodyType,
        },

        // Clipboard
        Command {
            name: "Copy Response Body",
            description: "Copy response body to clipboard",
            category: "Clipboard",
            action: Action::CopyResponseBody,
        },
        Command {
            name: "Copy as cURL",
            description: "Copy request as curl command",
            category: "Clipboard",
            action: Action::CopyAsCurl,
        },
        Command {
            name: "Paste from Clipboard",
            description: "Paste system clipboard content",
            category: "Clipboard",
            action: Action::PasteFromClipboard,
        },

        // Environment
        Command {
            name: "Change Environment",
            description: "Open environment selector",
            category: "Environment",
            action: Action::OpenOverlay(Overlay::EnvironmentSelector),
        },

        // Navigation
        Command {
            name: "Focus Collections",
            description: "Switch to collections panel",
            category: "Navigation",
            action: Action::FocusPanel(crate::state::Panel::Collections),
        },
        Command {
            name: "Focus Request",
            description: "Switch to request panel",
            category: "Navigation",
            action: Action::FocusPanel(crate::state::Panel::Request),
        },
        Command {
            name: "Focus Body",
            description: "Switch to body panel",
            category: "Navigation",
            action: Action::FocusPanel(crate::state::Panel::Body),
        },
        Command {
            name: "Focus Response",
            description: "Switch to response panel",
            category: "Navigation",
            action: Action::FocusPanel(crate::state::Panel::Response),
        },

        // Chain Cache TTL
        Command {
            name: "Chain Cache: 5s",
            description: "Re-execute dependency requests every 5 seconds",
            category: "Chain",
            action: Action::SetChainCacheTTL(5),
        },
        Command {
            name: "Chain Cache: 10s",
            description: "Re-execute dependency requests every 10 seconds (default)",
            category: "Chain",
            action: Action::SetChainCacheTTL(10),
        },
        Command {
            name: "Chain Cache: 15s",
            description: "Re-execute dependency requests every 15 seconds",
            category: "Chain",
            action: Action::SetChainCacheTTL(15),
        },
        Command {
            name: "Chain Cache: 30s",
            description: "Re-execute dependency requests every 30 seconds",
            category: "Chain",
            action: Action::SetChainCacheTTL(30),
        },
        Command {
            name: "Chain Cache: 60s",
            description: "Re-execute dependency requests every 60 seconds",
            category: "Chain",
            action: Action::SetChainCacheTTL(60),
        },
        Command {
            name: "Chain Cache: 300s",
            description: "Re-execute dependency requests every 5 minutes",
            category: "Chain",
            action: Action::SetChainCacheTTL(300),
        },
        Command {
            name: "Chain Cache: 3600s",
            description: "Re-execute dependency requests every 1 hour",
            category: "Chain",
            action: Action::SetChainCacheTTL(3600),
        },

        // General
        Command {
            name: "Show Help",
            description: "Toggle help overlay",
            category: "General",
            action: Action::OpenOverlay(Overlay::Help),
        },
        Command {
            name: "Quit",
            description: "Exit restui",
            category: "General",
            action: Action::Quit,
        },
    ];

    // Theme commands — one per theme
    for &theme_name in THEME_NAMES {
        cmds.push(Command {
            // We leak to get a &'static str — tiny, one-time allocation
            name: Box::leak(format!("Theme: {}", theme_name).into_boxed_str()),
            description: Box::leak(format!("Switch to {} color theme", theme_name).into_boxed_str()),
            category: "Theme",
            action: Action::SetTheme(theme_name.to_string()),
        });
    }

    cmds
}
