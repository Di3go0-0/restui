# Changelog

## [0.3.3] - 2026-04-01

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
