use std::io::Write;
use std::process::{Command, Stdio};

/// Read text from system clipboard.
pub fn read_clipboard() -> Result<String, String> {
    // Try wl-paste (Wayland)
    if let Some(text) = try_read_command("wl-paste", &["--no-newline"]) {
        return Ok(text);
    }
    // Try xclip (X11)
    if let Some(text) = try_read_command("xclip", &["-selection", "clipboard", "-o"]) {
        return Ok(text);
    }
    // Try xsel (X11)
    if let Some(text) = try_read_command("xsel", &["--clipboard", "--output"]) {
        return Ok(text);
    }
    // Fallback to arboard
    arboard::Clipboard::new()
        .and_then(|mut cb| cb.get_text())
        .map_err(|e| format!("Clipboard error: {}", e))
}

fn try_read_command(cmd: &str, args: &[&str]) -> Option<String> {
    Command::new(cmd)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
}

/// Copy text to system clipboard. Tries native tools first (wl-copy for Wayland,
/// xclip/xsel for X11), falls back to arboard.
pub fn copy_to_clipboard(text: &str) -> Result<(), String> {
    // Try wl-copy (Wayland) — must NOT wait, wl-copy stays alive to serve clipboard
    if try_wl_copy(text) {
        return Ok(());
    }
    // Try xclip — with -selection clipboard, xclip forks to background by default
    if try_pipe_command("xclip", &["-selection", "clipboard"], text) {
        return Ok(());
    }
    // Try xsel
    if try_pipe_command("xsel", &["--clipboard", "--input"], text) {
        return Ok(());
    }
    // Fallback to arboard
    arboard::Clipboard::new()
        .and_then(|mut cb| cb.set_text(text))
        .map_err(|e| format!("Clipboard error: {}", e))
}

/// wl-copy needs to stay running in the background to serve the clipboard.
/// We spawn it and intentionally do NOT wait for it to finish.
fn try_wl_copy(text: &str) -> bool {
    let Ok(mut child) = Command::new("wl-copy")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    else {
        return false;
    };

    let success = if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(text.as_bytes()).is_ok()
    } else {
        false
    };

    // Do NOT call child.wait() — wl-copy must stay alive to serve the clipboard.
    // It will exit on its own when another copy replaces it.
    // We just drop the Child handle, which detaches the process.

    success
}

/// For xclip/xsel: pipe text to stdin and wait for completion.
fn try_pipe_command(cmd: &str, args: &[&str], text: &str) -> bool {
    let Ok(mut child) = Command::new(cmd)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    else {
        return false;
    };
    if let Some(mut stdin) = child.stdin.take() {
        if stdin.write_all(text.as_bytes()).is_err() {
            return false;
        }
    }
    child.wait().is_ok_and(|s| s.success())
}
