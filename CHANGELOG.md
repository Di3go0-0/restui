# Changelog

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
