# Changelog

## [0.3.8] - 2026-04-04

### Fixed
- **vimltui version alignment** — bumped vimltui to 0.1.5, which pins ratatui 0.30 to prevent type conflicts when installing without a lockfile.

## [0.3.7] - 2026-04-04

### Fixed
- **Ctrl+Delete acting as Ctrl+H** — enabled crossterm keyboard enhancement protocol (`DISAMBIGUATE_ESCAPE_CODES`) so the terminal sends distinct sequences for keys that share legacy escape codes (e.g., Ctrl+Delete vs Ctrl+H). This prevents Ctrl+Delete from triggering panel navigation in insert mode.

### Changed
- **Upgraded vimltui to 0.1.4** — brings Delete key, Home/End, arrow keys in Insert/Replace, visual mode count prefix (`v10j`), and fixes Ctrl+Char inserting raw characters.

## [0.3.6] - 2026-04-03

### Fixed
- **Body editor: empty line input bug** — typing the first character on an empty line (especially the last line) caused the line number to disappear and blocked further input; root cause was `sync_body_to_vim()` overwriting the buffer before each keystroke using Rust's `.lines()` which strips trailing newlines
- **Bracket highlight overlap** — `%` bracket matching now only highlights the matched bracket, not the one at cursor (which already has the block cursor), so both are visually distinguishable
- **Keybinding SHIFT normalization** — `:`, `?`, and other shifted symbols now match correctly in the keybinding system (crossterm sends them with SHIFT modifier)
- **Underline cursor for `r`/`R`** — body renderer now uses terminal cursor (not visual block) when `pending_replace` or Replace mode is active, so the underline cursor is visible

### Changed
- **Upgraded vimltui to 0.1.3** — brings all features from 0.1.1 through 0.1.3:
  - Autoindent on Enter, correct `p`/`P` linewise paste
  - D/C/Y/X/S/J shortcuts, `%` bracket matching, `;`/`,` repeat find
  - `*`/`#` word search, zz/zt/zb scroll, H/M/L, Ctrl-f/Ctrl-b
  - Ctrl-w/Ctrl-u in insert mode, visual `o`/`c`
  - Text objects: `aw`/`a"`/`a(`/`a{`/`a[` and `i{`/`i[`/`i<`/`ib`/`iB`
  - `:s`/`:%s` regex substitution with live preview + replacement highlighting
  - Smartcase search (all-lowercase → insensitive, any uppercase → sensitive)
  - Replace mode (`R`) with underline cursor, `r` with count (`5rx`)
  - Visual mode case operations: `u` (lower), `U` (upper), `~` (toggle)
  - `g~` + motion operator, `:noh` to clear highlights
  - `CursorShape` API for terminal cursor style
  - Yank highlight flash (150ms)
- **`:` now goes to vim** in Body/Response panels for Ex commands (`:s`, `:w`, `:noh`, `:123`)
- **`?` now goes to vim** in Body/Response panels for backward search
- **Command palette** moved to `Ctrl+p` (works everywhere)
- **Help** available via `F1` in Body/Response panels (where `?` is vim backward search)
- **Response body keybinding changes:**
  - Toggle wrap: `W` → `F2` (frees `W` for vim big-word motion)
  - Environment selector: `p` → `P` (frees `p` for vim paste)
- **Body keybindings cleaned up** — removed 30+ dead vim bindings now handled by vimltui

### Added
- **Vim command line** in body editor — shows `:` commands, `-- INSERT --`, `-- REPLACE --`, search input, pending operators at the bottom of the editor
- **Live substitution preview** — `:s` match highlighting + replacement preview in body renderer
- **Smartcase-aware search highlighting** — body renderer respects case sensitivity of the pattern
- **Yank highlight** in body editor — 150ms flash on yanked text
- **`yank_highlight` theme color** — `Color::Rgb(80, 80, 40)` default across all theme constructors
- **Cursor shape support** — terminal cursor changes to Bar (insert), Underline (replace/r), Block (normal)

## [0.3.5] - 2026-04-03

### Changed
- **Major refactoring of `app/mod.rs`** — split 4,967-line monolith into 13 focused modules
  - `inline_edit.rs`: character-level editing, cursor movement
  - `request_field.rs`: request field cursor/text manipulation
  - `body_edit.rs`: body visual selection, word motions
  - `vim_sync.rs`: vim mode sync, find-char, type validation
  - `collections.rs`: collection CRUD, persistence
  - `autocomplete.rs`: chain/env autocomplete
  - `mode.rs`: vim mode transitions
  - `clipboard_ops.rs`: yank/delete/change/paste operations
  - `overlay.rs`: overlay open/close/navigate/confirm
- **Decomposed `AppState`** — extracted 38 fields into 3 sub-structs:
  - `RequestEditState`: request panel editing state
  - `ResponseViewState`: response panel display state
  - `CollectionsViewState`: collections panel state
- **Organized `src/` directory** — only `main.rs` at root, all modules in directories:
  - `core/`: action, state, config, event, tui, command, http_client
  - `keybindings/`: merged `keybindings.rs` + `keybinding_config.rs`
  - `ui/theme.rs`: moved from root
  - `app/clipboard.rs`, `app/vim_buffer.rs`: moved from root
  - Deleted dead code `highlight.rs`

### Added
- `Shift+W`: toggle word wrap in response panels
- `Shift+O`: export response in response panels

### Fixed
- `Ctrl+J/K` navigation between type editor and response preview in Type tab
- `Ctrl+K` from type editor no longer escapes to Request panel

## [0.3.4] - 2026-04-03

### Changed
- **Vim editor powered by [vimltui](https://crates.io/crates/vimltui)** — replaced local vim module with reusable crate
- Body, response, and type editor panels now delegate vim keys to `VimEditor.handle_key()`
- Removed old `vim_instance.rs`, slimmed `vim_buffer.rs` to word/offset helpers

### Added
- `h`/`l` cursor movement in body panel
- Panel navigation (`Ctrl+H/J/K/L`, `1-4`) works from all vim panels in normal mode
- Escape clears search highlights in normal mode

### Fixed
- Diff view scroll now uses correct line count (`set_content` on enter/exit)

## [0.3.3] - 2026-04-02

### New Features
- **Visual paste (`p` in visual mode)**: select text with `v` + motion, then press `p` to replace the selection with the yank buffer — works in URL, headers, body, and all editable fields
- **System clipboard integration for `p`**: paste now reads from the system clipboard (with fallback to internal yank buffer), so you can copy a URL from your browser and paste it directly with `p`
- **Binary response handling**: responses with binary content types (`image/*`, `audio/*`, `video/*`, etc.) are detected and displayed as an informative colored message instead of crashing
- **Buffer type generation**: binary endpoints now generate a `Buffer` type in the Type tab, with descriptive TypeScript and C# code snippets showing how to consume the response
- **Configurable keybindings**: all keybindings can be customized via `~/.config/restui/keybindings.toml` — only overrides needed, defaults follow vim conventions. Run `restui --dump-keybindings` to generate a full default config
- **Ctrl+S saves request**: `Ctrl+S` now saves the current request (previously toggled SSL). SSL toggle moved to `Ctrl+T`
- **Response history overlay**: press `H` in the response panel to browse up to 5 previous responses per request. Select to load with full vim/type/search support. Panel title shows `[History X/N — HH:MM:SS]`
- **Persistent response history**: response history survives between sessions — stored in `~/.local/share/restui/response_history.json`
- **Export response**: save response body to file via command palette (`:Export Response`). Auto-detects extension from content-type (`.json`, `.html`, `.png`, etc.)
- **Response diff**: press `D` in the response panel to compare the current response against a historical one. Full vim navigation (read-only) with colored additions (+green) and deletions (-red)
- **Request history deduplication**: the global request history (`Shift+H`) no longer fills up with duplicates — same method+URL replaces the previous entry

### Key Remaps
- `Ctrl+S` → save request (was: toggle SSL)
- `Ctrl+T` → toggle SSL (new key)
- `H` in response → response history overlay (was: toggle headers)
- `E` in response → toggle headers (new key)

### Bug Fixes
- **Fix crash on binary responses**: `byte index is not a char boundary` panic when navigating response body with multi-byte or binary content

### Refactor
- **VimInstance abstraction**: new `VimInstance` struct combines `VimBuffer` + `VimModeConfig`, replacing scattered buffer fields across the app
- **Mode-aware vim contexts**: `VimModeConfig` enables per-panel mode restrictions (e.g., read-only panels disable insert mode)
- **Cleaner state management**: `AppState` now uses `VimInstance` fields (`type_vim`, `type_ts_vim`, `type_csharp_vim`, `resp_vim`) instead of separate buffer fields

## [0.3.2] - 2026-03-30

### Improvements
- **Quick quit**: press `q` once to exit (previously required `qq`)

## [0.3.1] - 2026-03-30

### New Features
- **Environment variable autocomplete**: type `{{` in any field to see matching variables from the active environment
- **Add request to collection**: press `a` in the Collections panel to create a new empty GET request and save it directly to the selected collection

### Documentation
- Added "Environment Variables" section to README with env file format, auto-discovery, and keybindings
- Added "Request Chaining" section to README with syntax reference and examples
- Updated help overlay and keybindings table with new shortcuts

## [0.3.0] - 2026-03-29

### Type System — TS & C# Code Generation
- **TypeScript type generation**: auto-generates `type ResponseType = { ... }` from JSON response
- **C# class generation**: auto-generates `public class ResponseType { ... }` with proper properties
- Switch between Type / TS / C# with `[` / `]` sub-tabs
- Syntax coloring for TS (keywords, type names, punctuation) and C# (keywords, types, properties)
- Full vim support in all sub-tabs (normal, visual, insert modes)

### Type Editor — Full Vim
- Complete vim normal mode: `h`/`j`/`k`/`l`, `w`/`b`/`e`, `0`/`$`, `gg`/`G`, `f`/`F`/`t`/`T`
- Edit operations: `dd`, `cc`, `x`, `r`, `s`, `S`, `C`, `D`, `cw`/`cb`, `dw`/`de`/`db`
- Insert modes: `i`/`I`/`a`/`A`/`o`/`O`
- Visual mode with selection highlighting
- Undo/redo: `u` / `Ctrl+R`
- Paste: `p`/`P`, `Ctrl+V` from clipboard

### Type Tab Split View
- Type editor (top 50%) + response body preview (bottom 50%) visible simultaneously
- **Ctrl+J** / **Ctrl+K** to move focus between type editor and response preview
- Response preview: full read-only vim navigation + visual mode + clipboard copy
- Visual indicator (`▸`) shows which section has focus

### Word Wrap
- Toggle via command palette: `:toggle wrap`
- Proper line wrapping preserving syntax colors
- Gutter shows line number on first row, `~` on continuations
- Cursor, visual highlight, and bracket matching work on wrapped lines
- `WRAP` badge in status bar when enabled

### Response Body — Enhanced Vim
- Block cursor shows exact column position in normal mode
- h/l, w/b/e, 0/$ all work with proper horizontal scroll sync
- Scrolloff: cursor stays 2 lines from viewport edge while scrolling
- Visual mode includes character under cursor (off-by-one fix)

### UX Polish
- **Status bar response badge**: persistent HTTP status code (color-coded) + response time (green <200ms, yellow <1s, red >1s)
- **Tree guides in collections**: visual hierarchy with │├└ lines, colored method names (GET=green, POST=blue, etc.)
- **URL colorization**: query params colorized (keys cyan, values green, separators dim)
- **Autocomplete popup**: appears 2 lines below cursor, near the edited field

### Collections & Files
- **`.http/` folder convention**: scans `.http/` subfolder + root directory (backwards compatible)
- New collections created with `n` go into `.http/` (auto-created)
- Environment files also discovered from `.http/` folder

### Request History
- Persistent history saved to `~/.local/share/restui/history.json`
- Press **H** to open history overlay (navigate with `j`/`k`, load with `Enter`)
- Capped at configured `history_limit` (default 100)

### Request Cancellation
- **Esc** cancels in-flight request immediately
- Animated spinner with elapsed time: `⠋ Sending request... 2.3s (Esc to cancel)`

### Themes
- 3 new themes: **Dracula**, **Nord**, **Solarized Dark**
- Total: 8 built-in themes (default, catppuccin, gruvbox, tokyonight, light, dracula, nord, solarized)

### Security
- Shell injection fix in curl export (header values now escaped)
- URL scheme validation (only `http://` and `https://` allowed)

### Code Quality
- Zero compiler warnings
- `VimBuffer` abstraction for reusable vim editing across all panels
- `app.rs` split into focused modules (`execute.rs`, `scroll.rs`, `search.rs`)
- 26 duplicated body-type match blocks → `Request::get_body_mut()`
- `AppState` fields grouped into sub-structs
- File-based tracing/logging with `--debug` flag
- Help overlay (`?`) updated with all keybindings
- Version number displayed in status bar

## [0.2.0] - 2026-03-29

### Vim Editing
- Full vim normal/visual/insert mode for request fields (URL, headers, queries, cookies, params)
- `e` enters field-edit normal mode, `i`/`a`/`A`/`I` enter insert, `v` enters visual
- Block cursor in normal mode, bar cursor in insert mode
- Normal mode cursor clamped to last character (vim behavior)
- Word motions `w`/`b`/`e` respect word boundaries (alphanumeric vs punctuation)
- Change operator: `cc`/`cw`/`ce`/`cb`/`C`/`S`/`c0`
- Delete with motion: `dw`/`de`/`db`/`d$`/`d0`/`dG`/`D`
- Yank with motion: `yw`/`y$`/`y0`/`yG`
- Substitute: `s` (delete char + insert)
- Find in line: `f`/`F`/`t`/`T` + char
- Count prefix: `3w`, `5j`, `2dd`, `Nyy` etc.
- Undo/redo for request field editing (`u` / `Ctrl+R`)
- `Ctrl+R` is redo in vim edit contexts, execute request in global navigation

### Request Panel
- New **Params** tab for path parameters (`:key` or `{key}` resolved in URL)
- Tab order: Headers, Cookies, Queries, Params
- Horizontal scroll for long header/query/cookie/param values

### Body Panel
- Tab bar for body types: JSON, XML, Form, Raw (switch with `{`/`}`)
- Each body tab has independent content (JSON and XML don't share text)
- Auto Content-Type header based on active body tab
- Auto-indent on Enter (copies indentation, extra indent after `{`/`[`)
- Horizontal scroll for long lines

### Response Panel
- **Type** tab: auto-inferred TypeScript-like type schema from JSON responses
- Editable type definitions with enum support (`"active" | "inactive"`)
- Type validation: warnings when response doesn't match expected type
- `R` to regenerate type from response
- Response headers inspector: `H` to expand/collapse all response headers
- Sent request info shown alongside response headers (body, params, queries, cookies)
- Search (`/`) with highlighted matches and `n`/`N` navigation
- Horizontal scroll for long lines

### Chain References
- Smart autocomplete for `{{@request.field}}` syntax
- After `{{@`: suggests available request names
- After `{{@name.`: suggests fields from response type
- Array support: `{{@auth[0].token}}` syntax
- Shows `(no type — execute request first)` when no cached response
- Type navigation for nested fields

### Collections
- Fuzzy search filter (`/`) by request name or URL
- Fold keybindings: `zo`/`zc`/`za` (open/close/toggle), `zM`/`zR` (all)

### UI Polish
- Scrollbar indicator in body and response panels
- Bracket matching: highlights matching `{}`/`[]`/`()` pairs
- Light theme (`themes/light.toml`)
- Environment variable editor overlay (`E` to open)
- Theme inherited from Neovim colorscheme via `--colors` flag

### Neovim Integration
- `restui.nvim` plugin auto-detects nvim colorscheme and passes to restui
- Available at [Di3go0-0/restui.nvim](https://github.com/Di3go0-0/restui.nvim)

### Other
- Zero compiler warnings
- Response cache capped at 50 entries
- MIT License

## [0.1.0] - 2026-03-28

- Initial release
- TUI HTTP client with vim keybindings
- Collections from `.http` files
- Request chaining with `{{@request.path}}` syntax
- Environment variables
- Theme support (default, catppuccin, gruvbox, tokyonight)
- Syntax-highlighted JSON responses
