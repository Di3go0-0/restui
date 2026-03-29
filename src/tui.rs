use anyhow::Result;
use crossterm::{
    cursor,
    event::DisableMouseCapture,
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};
use std::io::{self, Stdout};

pub type Tui = Terminal<CrosstermBackend<Stdout>>;

pub fn init() -> Result<Tui> {
    enable_raw_mode()?;
    // Explicitly disable mouse capture so the terminal doesn't send
    // mouse escape sequences that cause render glitches on click.
    execute!(io::stdout(), EnterAlternateScreen, DisableMouseCapture)?;
    let backend = CrosstermBackend::new(io::stdout());
    let terminal = Terminal::new(backend)?;
    Ok(terminal)
}

pub fn restore() -> Result<()> {
    disable_raw_mode()?;
    execute!(io::stdout(), LeaveAlternateScreen, cursor::Show, cursor::SetCursorStyle::DefaultUserShape)?;
    Ok(())
}
