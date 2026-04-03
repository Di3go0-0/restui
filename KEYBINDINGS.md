# Keybindings

All keybindings in restui can be customized via `~/.config/restui/keybindings.toml`.

## Quick start

Generate the default config:

```bash
restui --dump-keybindings > ~/.config/restui/keybindings.toml
```

You only need to include the keys you want to change. Any action not defined in your file will use the default.

## Key format

| Format | Example | Description |
|--------|---------|-------------|
| Single char | `"j"` | Regular key |
| Shift | `"Shift+g"` or `"G"` | Uppercase / shifted key |
| Ctrl | `"Ctrl+r"` | Control modifier |
| Alt | `"Alt+x"` | Alt modifier |
| Special | `"Esc"`, `"Enter"`, `"Tab"`, `"Space"` | Named keys |
| Arrow | `"Up"`, `"Down"`, `"Left"`, `"Right"` | Arrow keys |
| Other | `"Home"`, `"End"`, `"Backspace"`, `"Delete"` | Navigation keys |
| Function | `"F1"` .. `"F12"` | Function keys |
| Multiple | `["j", "Down"]` | Multiple keys for one action |

## Contexts

Keybindings are organized by context. Each context is a TOML section:

| Section | When it applies |
|---------|-----------------|
| `[global]` | Always active (ctrl shortcuts, panel switching, quit) |
| `[collections]` | Collections panel in normal mode |
| `[request]` | Request panel navigation (not editing a field) |
| `[request_field]` | Vim normal mode inside a request field (URL, header, etc.) |
| `[body]` | Body panel in normal mode |
| `[response_body]` | Response panel, Body tab |
| `[response_type_preview]` | Response panel, Type tab, preview sub-focus |
| `[response_type_editor]` | Response panel, Type tab, editor sub-focus |
| `[visual]` | Visual mode (any panel) |
| `[insert]` | Insert mode (any panel) |
| `[command_palette]` | Command palette (`:`) |
| `[search]` | Search mode (`/`) |
| `[collections_filter]` | Collections filter mode |
| `[overlay]` | Overlays (help, theme selector, history, etc.) |

## Example: custom config

```toml
# ~/.config/restui/keybindings.toml
# Only override what you need

[global]
quit = "Ctrl+q"

[collections]
scroll_down = ["j", "Down", "Ctrl+n"]
scroll_up = ["k", "Up", "Ctrl+p"]

[body]
paste = "Ctrl+p"
undo = "Ctrl+z"
```

## Pending keys (vim operators)

Actions like `delete_pending`, `change_pending`, `yank_pending`, and `replace_pending` start a two-key sequence. The first key (the operator) is configurable. The second key (the motion) follows vim grammar and is not configurable:

| Sequence | Action |
|----------|--------|
| `dd` | Delete line |
| `dw` | Delete word forward |
| `db` | Delete word backward |
| `d$` | Delete to end of line |
| `d0` | Delete to start of line |
| `dG` | Delete to end of file |
| `cc` | Change line |
| `cw` / `ce` | Change word |
| `cb` | Change word backward |
| `c$` | Change to end of line |
| `c0` | Change to start of line |
| `yy` | Yank line |
| `yw` | Yank word |
| `y$` | Yank to end of line |
| `y0` | Yank to start of line |
| `yG` | Yank to end of file |
| `r<char>` | Replace character under cursor |
| `f<char>` | Find char forward |
| `F<char>` | Find char backward |
| `t<char>` | Find char forward (before) |
| `T<char>` | Find char backward (after) |

## Syntax errors

If your config has a syntax error, restui will print the error with the line number and exit:

```
keybindings.toml syntax error: TOML parse error at line 5, column 10
  |
5 | quit = {invalid}
  |          ^
expected a value
```

## Full default config

```toml
[global]
cancel_request = "Esc"
execute_request = "Ctrl+r"
focus_panel_1 = "1"
focus_panel_2 = "2"
focus_panel_3 = "3"
focus_panel_4 = "4"
help = ["?", "F1"]
navigate_down = "Ctrl+j"
navigate_left = "Ctrl+h"
navigate_right = "Ctrl+l"
navigate_up = "Ctrl+k"
open_command_palette = [":", "Ctrl+p"]
open_env_editor = "Shift+e"
open_history = "Shift+h"
open_theme_selector = "Shift+t"
paste_clipboard = "Ctrl+v"
quit = "q"
redo = "Ctrl+r"
save_request = "Ctrl+s"
scroll_half_down = "Ctrl+d"
scroll_half_up = "Ctrl+u"
toggle_insecure = "Ctrl+t"
visual_block = "Ctrl+v"

[collections]
add_request = "a"
copy_as_curl = "Shift+y"
create_collection = "n"
delete_pending = "d"
fold_pending = "z"
move_request = "m"
new_empty_request = "Shift+c"
next_collection = ["Shift+l", "}"]
paste_request = "p"
prev_collection = ["Shift+h", "{"]
rename_request = "r"
save_request = "s"
save_request_as = "Shift+s"
scroll_bottom = "Shift+g"
scroll_down = ["j", "Down"]
scroll_top = "g"
scroll_up = ["k", "Up"]
select_request = "Enter"
start_filter = "/"
toggle_collapse = "Space"
yank_pending = "y"

[request]
add_item = "a"
copy_as_curl = "Shift+y"
copy_response = "y"
delete_item = "x"
delete_pending = "d"
enter_field_edit = "e"
focus_down = ["j", "Down"]
focus_up = ["k", "Up"]
next_method = "]"
next_tab = "}"
open_env_selector = "p"
prev_method = "["
prev_tab = "{"
show_autocomplete = "Shift+a"
toggle_enabled = "Space"

[request_field]
change_pending = "c"
change_to_end = "Shift+c"
cursor_left = ["h", "Left"]
cursor_right = ["l", "Right"]
delete_char = "x"
delete_pending = "d"
delete_to_end = "Shift+d"
enter_append = "a"
enter_append_end = "Shift+a"
enter_insert = "i"
enter_insert_start = "Shift+i"
enter_visual = "v"
exit_field_edit = "Esc"
find_after = "Shift+t"
find_backward = "Shift+f"
find_before = "t"
find_forward = "f"
line_end = ["$", "End"]
line_home = ["0", "Home"]
paste = ["p", "Shift+p"]
replace_pending = "r"
substitute = "s"
tab = "Tab"
undo = "u"
word_backward = "b"
word_end = "e"
word_forward = "w"
yank_pending = "y"

[body]
# Body uses vimltui for ALL vim operations.
# Only app-level keys are configured here:
next_tab = "}"
prev_tab = "{"
start_search = "/"
search_next = "n"
search_prev = "Shift+n"
# All vim keys (h/j/k/l, w/b/e, d/c/y, i/a/o, :, ?, *, #,
# f/F/t/T, ;/,, %, r/R, J, D/C/Y/X/S, u, p/P, gg/G,
# zz/zt/zb, H/M/L, Ctrl-f/b, Ctrl-w/u, visual u/U/~,
# gu/gU/g~, :s substitution, etc.) are handled by vimltui.

[response_body]
copy_as_curl = "Shift+y"
copy_response = "y"
cursor_left = ["h", "Left"]
cursor_right = ["l", "Right"]
enter_visual = "v"
export_response = "Shift+o"
find_after = "Shift+t"
find_backward = "Shift+f"
find_before = "t"
find_forward = "f"
line_end = ["$", "End"]
line_home = ["0", "Home"]
next_tab = "}"
open_env_selector = "Shift+p"
prev_tab = "{"
response_diff = "Shift+d"
response_history = "Shift+h"
scroll_bottom = "Shift+g"
scroll_down = ["j", "Down"]
scroll_top = "g"
scroll_up = ["k", "Up"]
search_next = "n"
search_prev = "Shift+n"
start_search = "/"
toggle_headers = "Shift+e"
toggle_wrap = "F2"
word_backward = "b"
word_end = "e"
word_forward = "w"

[response_type_preview]
copy_response = "y"
cursor_left = ["h", "Left"]
cursor_right = ["l", "Right"]
enter_visual = "v"
export_response = "Shift+o"
find_after = "Shift+t"
find_backward = "Shift+f"
find_before = "t"
find_forward = "f"
line_end = ["$", "End"]
line_home = ["0", "Home"]
next_tab = "}"
prev_tab = "{"
scroll_bottom = "Shift+g"
scroll_down = ["j", "Down"]
scroll_top = "g"
scroll_up = ["k", "Up"]
search_next = "n"
search_prev = "Shift+n"
start_search = "/"
toggle_wrap = "F2"
type_lang_next = "]"
type_lang_prev = "["
word_backward = "b"
word_end = "e"
word_forward = "w"

[response_type_editor]
change_line = "Shift+s"
change_pending = "c"
change_to_end = "Shift+c"
cursor_left = ["h", "Left"]
cursor_right = ["l", "Right"]
delete_char = "x"
delete_pending = "d"
delete_to_end_line = "Shift+d"
enter_append = "a"
enter_append_end = "Shift+a"
enter_insert = "i"
enter_insert_start = "Shift+i"
enter_visual = "v"
find_after = "Shift+t"
find_backward = "Shift+f"
find_before = "t"
find_forward = "f"
line_end = ["$", "End"]
line_home = ["0", "Home"]
next_tab = "}"
open_line_above = "Shift+o"
open_line_below = "o"
paste = ["p", "Shift+p"]
prev_tab = "{"
regenerate_type = "Shift+r"
replace_pending = "r"
scroll_bottom = "Shift+g"
scroll_down = ["j", "Down"]
scroll_top = "g"
scroll_up = ["k", "Up"]
substitute = "s"
type_lang_next = "]"
type_lang_prev = "["
undo = "u"
word_backward = "b"
word_end = "e"
word_forward = "w"
yank_pending = "y"

[visual]
cursor_down = ["j", "Down"]
cursor_left = ["h", "Left"]
cursor_right = ["l", "Right"]
cursor_up = ["k", "Up"]
delete = ["d", "x"]
exit_visual = "Esc"
find_after = "Shift+t"
find_backward = "Shift+f"
find_before = "t"
find_forward = "f"
line_end = ["$", "End"]
line_home = ["0", "Home"]
navigate_down = "Ctrl+j"
navigate_left = "Ctrl+h"
navigate_right = "Ctrl+l"
navigate_up = "Ctrl+k"
paste = ["p", "Shift+p"]
scroll_bottom = "Shift+g"
scroll_top = "g"
word_backward = "b"
word_end = "e"
word_forward = "w"
yank = "y"

[insert]
autocomplete_accept = "Ctrl+y"
autocomplete_next = "Ctrl+n"
autocomplete_prev = "Ctrl+p"
exit_insert = "Esc"
navigate_down = "Ctrl+j"
navigate_left = "Ctrl+h"
navigate_right = "Ctrl+l"
navigate_up = "Ctrl+k"

[command_palette]
close = "Esc"
confirm = "Enter"
nav_down = ["Down", "Tab", "Ctrl+n", "Ctrl+j"]
nav_up = ["Up", "BackTab", "Ctrl+p", "Ctrl+k"]

[search]
cancel = "Esc"
confirm = "Enter"

[collections_filter]
cancel = "Esc"
confirm = "Enter"

[overlay]
close = "Esc"
confirm = "Enter"
nav_down = ["j", "Down", "Ctrl+n", "Ctrl+j"]
nav_up = ["k", "Up", "Ctrl+p", "Ctrl+k"]
```
