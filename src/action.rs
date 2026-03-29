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
    AddPathParam,
    DeletePathParam,

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
    ExpandCollection,     // zo — open fold
    CollapseCollection,   // zc — close fold
    CollapseAll,          // zM — close all folds
    ExpandAll,            // zR — open all folds
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
    BodyWordEnd,      // e — move to end of current/next word
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
    ChangeLine,               // cc — clear line + enter insert
    ChangeWord,               // cw/ce — delete word forward + enter insert
    ChangeWordBack,           // cb — delete word backward + enter insert
    ChangeToEnd,              // C — delete to end of line + enter insert
    Substitute,               // s — delete char under cursor + enter insert
    DeleteWord,               // dw — delete word forward (stay normal)
    DeleteWordEnd,            // de — delete to end of word (stay normal)
    DeleteWordBack,           // db — delete word backward (stay normal)
    YankWord,                 // yw — yank from cursor to next word start
    YankToEnd,                // y$ — yank from cursor to end of line
    YankToStart,              // y0 — yank from cursor to start of line
    YankToBottom,             // yG — yank from cursor to end of file
    DeleteToEnd,              // d$ / D — delete from cursor to end of line
    DeleteToStart,            // d0 — delete from cursor to start of line
    DeleteToBottom,           // dG — delete from cursor to end of file
    ChangeToStart,            // c0 — delete to start of line + enter insert
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
    BodyNextTab,
    BodyPrevTab,

    // Theme
    #[allow(dead_code)]
    CycleTheme,
    #[allow(dead_code)]
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
    OverlayDelete,

    // Response tabs
    ResponseNextTab,
    ResponsePrevTab,
    RegenerateType,

    // SSL
    ToggleInsecureMode,

    // Clipboard
    CopyResponseBody,
    CopyAsCurl,
    YankLine,

    // Response headers inspector
    ToggleResponseHeaders,

    // Search
    StartSearch,
    SearchInput(char),
    SearchBackspace,
    SearchConfirm,
    SearchCancel,
    SearchNext,
    SearchPrev,

    // Collections filter
    StartCollectionsFilter,
    CollectionsFilterInput(char),
    CollectionsFilterBackspace,
    CollectionsFilterConfirm,
    CollectionsFilterCancel,

    // Count prefix (vim number prefix)
    AccumulateCount(u32),

    // Find char motions
    FindCharForward(char),
    FindCharBackward(char),
    FindCharForwardBefore(char),
    FindCharBackwardAfter(char),

    // App
    Quit,
    Tick,
}
