# restui

A lazygit-style TUI HTTP client. Run HTTP requests from your terminal with vim keybindings.

## Features

- Vim-like keybindings (normal, insert, visual modes)
- Collections from `.http` files
- Request chaining with `{{@request_name.json.path}}` syntax
- Environment variables support
- Syntax-highlighted JSON responses
- Theme support
- Request history
- Cookie management
- Neovim integration via [restui.nvim](https://github.com/Di3go0-0/restui.nvim)

## Installation

### From source

```sh
git clone https://github.com/Di3go0-0/restui.git
cd restui
cargo install --path .
```

This installs the `restui` binary to `~/.cargo/bin/`. Make sure it's in your PATH:

```sh
# Add to your shell config (~/.bashrc, ~/.zshrc, etc.)
export PATH="$HOME/.cargo/bin:$PATH"
```

### Requirements

- Rust 1.75+
- `wl-copy` (Wayland) or `xclip` (X11) for clipboard support

## Usage

```sh
# Open in current directory (scans for .http files)
restui

# Open a specific file
restui --file api.http

# Specify working directory
restui --dir ./my-project

# Use an environment file
restui --env-file env.json
```

## Keybindings

### Global

| Key | Action |
|-----|--------|
| `Ctrl+h/j/k/l` | Navigate between panels |
| `1/2/3/4` | Focus panel (Collections/Request/Body/Response) |
| `Ctrl+r` | Execute request (global) / Redo (in vim edit mode) |
| `qq` | Quit |
| `?` | Help |
| `:` | Command palette |

### Vim Modes (Body / Request fields)

| Key | Action |
|-----|--------|
| `i/I/a/A` | Enter insert mode |
| `Esc` | Exit to normal mode |
| `v` | Visual mode |
| `h/l` | Move cursor left/right |
| `w/b/e` | Word forward/backward/end |
| `0/$` | Line start/end |
| `x` | Delete char under cursor |
| `dd` | Delete line / clear field |
| `yy` | Yank line / field |
| `p` | Paste |
| `u` | Undo |

### Request Panel

| Key | Action |
|-----|--------|
| `j/k` | Navigate between fields |
| `e` | Enter field edit (normal mode) |
| `a` | Add header/param/cookie |
| `space` | Toggle enabled/disabled |
| `]/[` | Cycle HTTP method |
| `{/}` | Switch tab (Headers/Queries/Cookies) |

### Collections

| Key | Action |
|-----|--------|
| `j/k` | Navigate |
| `Enter` | Select request |
| `s` | Save request |
| `S` | Save as new |
| `n` | New collection |

## .http File Format

```http
# @name Get Users
GET https://api.example.com/users
Authorization: Bearer {{token}}

###

# @name Create User
POST https://api.example.com/users
Content-Type: application/json

{
  "name": "John",
  "email": "john@example.com"
}
```

## Neovim Integration

Use [restui.nvim](https://github.com/Di3go0-0/restui.nvim) to open restui in a floating terminal inside Neovim:

```lua
-- lazy.nvim
{
    "Di3go0-0/restui.nvim",
    config = function()
        require("restui").setup()
    end,
    keys = {
        { "<leader>rr", "<cmd>Restui<cr>", desc = "Toggle restui" },
    },
}
```

## License

MIT
