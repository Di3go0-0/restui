use crate::model::response::Response;
use crate::state::{Direction, Overlay, Panel};

#[derive(Debug, Clone)]
pub enum Action {
    // Panel navigation
    NavigatePanel(Direction),
    FocusPanel(Panel),

    // Vim mode transitions
    EnterInsertMode,
    EnterInsertModeStart, // I — insert at beginning of line
    EnterAppendMode,      // A — append at end of line
    ExitInsertMode,
    EnterVisualMode,
    ExitVisualMode,

    // New line (vim o/O)
    OpenLineBelow,
    OpenLineAbove,

    // Scrolling
    ScrollUp,
    ScrollDown,
    ScrollTop,
    ScrollBottom,

    // Request panel focus navigation
    RequestFocusUp,
    RequestFocusDown,
    AddHeader,
    DeleteHeader,
    ShowHeaderAutocomplete,

    // Collections
    SelectRequest,
    CreateCollection,
    NextCollection,
    PrevCollection,
    SaveRequest,          // s — overwrite selected request in collection
    SaveRequestAs,        // S — save current as new request in collection
    NewEmptyRequest,      // C — create blank request

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

    // Clipboard
    CopyResponseBody,
    CopyAsCurl,
    YankLine,

    // App
    Quit,
    Tick,
}
