# Changelog

## [0.2.6] - 2026-03-29

### Type Editor (Response Panel)
- Full vim support in TS and C# sub-tabs (normal, visual, insert modes via swap technique)
- Syntax coloring for TypeScript (`type`/`string`/`number` keywords, type names, punctuation)
- Syntax coloring for C# (`public`/`class`/`get`/`set` keywords, C# types, properties)
- Fixed C# class generation for array-of-objects responses (was generating empty class)
- Visual mode now includes the character under the cursor (off-by-one fix)
- Block cursor shows exact column position in response body normal mode
- `[` / `]` to switch between Type / TS / C# sub-tabs

### Response Body Vim
- h/l horizontal cursor movement works correctly in all views
- w/b/e word motions sync horizontal scroll after moving
- 0/$ line motions sync scroll in response panel
- Scrolloff (2-line margin) tuned for accurate viewport calculation
- Visual yank in Type editor copies to system clipboard

### Word Wrap
- Toggle via command palette: `:toggle wrap`
- Proper line wrapping preserving syntax colors (pre-colorize full line, slice spans per visual row)
- Gutter shows line number on first row, `~` on continuations
- Cursor, visual highlight, and bracket matching work on wrapped lines
- `WRAP` badge in status bar when enabled

### Chain Autocomplete
- Popup appears 2 lines below cursor (was blocking the current line)

### Collections
- `.http/` folder convention: restui scans `.http/` subfolder + root directory
- New collections created with `n` go into `.http/` (auto-created)
- Environment files also discovered from `.http/` folder
- No duplicates when same file exists in both locations
- Method colors (GET=green, POST=blue, etc.) now render correctly in tree view

### Help
- `?` overlay updated with all v0.2.5+ keybindings and features
- Organized into: Navigation, Vim Modes, Editing, Visual Mode, Request/Response Panel, Collections, General

### Code Quality
- Zero compiler warnings (removed 10 unused methods)
- Extracted `Request::get_body_mut()` replacing 26 duplicated body-type match blocks
- Cleaned unused scaffolding from VimBuffer, App, and scroll helpers
- Version displayed in status bar bottom-right

## [0.2.5] - 2026-03-29

### UX Polish
- **Status bar response badge**: shows last HTTP status code (color-coded: green 2xx, yellow 3xx, red 4xx, magenta 5xx) and response time (green <200ms, yellow <1s, red >1s) persistently in the status bar
- **Tree guides in collections**: visual hierarchy with │├└ lines connecting requests to their parent collection
- **URL colorization**: query string params are now colorized (keys in cyan, values in green, `?`/`&`/`=` separators dim) for easy visual parsing
- **3 new themes**: Dracula, Nord, Solarized Dark — switch with `T` or `:theme <name>`

### Themes
- **Dracula**: purple/pink/green/cyan on #282a36
- **Nord**: blue/teal/white on #2e3440
- **Solarized Dark**: yellow/orange/blue on #002b36
- Total: 8 built-in themes (default, catppuccin, gruvbox, tokyonight, light, dracula, nord, solarized)

## [0.2.1] - 2026-03-29

### Type Editor (Response Panel)
- Full vim support in the Type editor: normal, insert, and visual modes
- All vim motions: `h`/`j`/`k`/`l`, `w`/`b`/`e`, `0`/`$`, `gg`/`G`, `f`/`F`/`t`/`T`
- Edit operations: `dd`, `cc`, `x`, `r`, `s`, `S`, `C`, `D`, `cw`/`cb`, `dw`/`de`/`db`
- Insert modes: `i`/`I`/`a`/`A`/`o`/`O`
- Visual mode with selection highlighting
- Undo/redo: `u` / `Ctrl+R`
- Paste: `p`/`P`, `Ctrl+V` from clipboard
- Yank: `yy`, `yw`, `y$`

### Type Tab Split View
- When the Type tab is open, the response body preview is visible below
- **Ctrl+J** moves focus down to the response preview
- **Ctrl+K** moves focus back up to the type editor
- Response preview supports full read-only vim navigation + visual mode + copy
- Visual indicator (`▸`) shows which section has focus
- At edges, Ctrl+J/K falls through to normal panel navigation

### Request History
- Every completed request is saved to persistent history (`~/.local/share/restui/history.json`)
- Press **H** to open the history overlay (navigate with `j`/`k`, load with `Enter`)
- History is capped at the configured `history_limit` (default 100)

### Chain Autocomplete Fixes
- Accepting a suggestion now inserts only the missing suffix (was inserting the full text, duplicating what was already typed)
- Fixed suffix calculation for array-of-objects field suggestions
- Popup now renders near the cursor instead of at a fixed position
- Header autocomplete popup renders near the edited header row

### Request Cancellation
- Press **Esc** during a request to cancel it immediately
- Animated spinner with elapsed time in the status bar while waiting
- Status shows `⠋ Sending request... 2.3s (Esc to cancel)`

### Code Quality & Security
- **Security**: shell injection fix in curl export (header values now escaped)
- **Security**: URL scheme validation (only `http://` and `https://` allowed)
- Magic numbers extracted to named constants
- `app.rs` split into focused modules (`execute.rs`, `scroll.rs`, `search.rs`)
- `AppState` fields grouped into `SearchState`, `CommandPaletteState`
- Body type access consolidated into `Request.get_body()` / `set_body()` / `any_body()`
- Duplicated cache eviction logic extracted to `cache_response()`
- `VimBuffer` abstraction for reusable vim editing across panels
- Body, response, and type editor state migrated to `VimBuffer`
- User-Agent header suggestion uses dynamic crate version
- File-based tracing/logging with `--debug` flag
- Version number shown in bottom-right corner of status bar

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
