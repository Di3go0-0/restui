use crate::model::response::Response;
use crate::state::{Direction, Overlay, Panel};

#[derive(Debug, Clone)]
pub enum Action {
    // Panel navigation
    NavigatePanel(Direction),
    FocusPanel(Panel),

    // Vim mode transitions
    EnterInsertMode,
    EnterInsertModeStart,  // I — insert at beginning of line
    EnterAppendMode,       // a — append after cursor
    EnterAppendModeEnd,    // A — append at end of line
    ExitInsertMode,
    EnterRequestFieldEdit,   // e — enter vim normal mode inside a request field
    ExitRequestFieldEdit,    // Esc from field-edit normal mode
    EnterVisualMode,
    EnterVisualBlockMode,  // Ctrl+V — block selection
    ExitVisualMode,

    // New line (vim o/O)
    OpenLineBelow,
    OpenLineAbove,

    // Scrolling
    ScrollUp,
    ScrollDown,
    ScrollHalfUp,
    ScrollHalfDown,
    ScrollTop,
    ScrollBottom,

    // Request panel focus navigation
    RequestFocusUp,
    RequestFocusDown,
    AddHeader,
    DeleteHeader,
    ShowHeaderAutocomplete,
    RequestNextTab,
    RequestPrevTab,
    ToggleItemEnabled,
    AddParam,
    DeleteParam,
    AddCookie,
    DeleteCookie,

    // Collections
    SelectRequest,
    CreateCollection,
    NextCollection,
    PrevCollection,
    SaveRequest,          // s — overwrite selected request in collection
    SaveRequestAs,        // S — save current as new request in collection
    NewEmptyRequest,      // C — create blank request
    RenameRequest,        // R — rename selected request
    DeleteSelected,       // D — delete selected request or collection
    MoveRequest,          // m — move request to another collection
    ToggleCollapse,       // Space on collection header — expand/collapse
    YankRequest,          // yy on a request — copy to clipboard
    PasteRequest,         // p — paste yanked request into current collection

    // Inline text editing (insert mode)
    InlineInput(char),
    InlineBackspace,
    InlineDelete,
    InlineNewline,
    InlineCursorLeft,
    InlineCursorRight,
    InlineCursorUp,
    InlineCursorDown,
    InlineTab,
    InlineCursorHome,
    InlineCursorEnd,

    // Body-specific normal mode motions
    BodyWordForward,  // w
    BodyWordBackward, // b
    BodyLineHome,     // 0
    BodyLineEnd,      // $

    // Visual mode actions
    VisualYank,
    VisualDelete,
    Paste,
    PasteFromClipboard,

    // Vim edit commands
    ReplaceChar(char),        // r + char — replace character under cursor
    DeleteCharUnderCursor,    // x — delete char under cursor in normal mode
    DeleteLine,               // dd — delete line (yank + remove)
    Undo,                     // u — undo last body edit
    Redo,                     // Ctrl+r — redo last undone edit

    // Inline autocomplete (Ctrl+n, Ctrl+p, Ctrl+y)
    AutocompleteNext,
    AutocompletePrev,
    AutocompleteAccept,

    // Pending key (for dd)
    PendingKey(char),

    // Method cycling
    NextMethod,
    PrevMethod,

    // Body type cycling
    CycleBodyType,

    // Theme
    CycleTheme,
    SetTheme(String),

    // Command Palette
    OpenCommandPalette,
    CommandPaletteInput(char),
    CommandPaletteBackspace,
    CommandPaletteUp,
    CommandPaletteDown,
    CommandPaletteConfirm,
    CommandPaletteClose,

    // Request lifecycle
    ExecuteRequest,
    RequestCompleted(Box<Response>),
    RequestFailed(String),

    // Overlays
    OpenOverlay(Overlay),
    CloseOverlay,
    OverlayUp,
    OverlayDown,
    OverlayConfirm,
    OverlayInput(char),
    OverlayBackspace,

    // SSL
    ToggleInsecureMode,

    // Clipboard
    CopyResponseBody,
    CopyAsCurl,
    YankLine,

    // App
    Quit,
    Tick,
}
