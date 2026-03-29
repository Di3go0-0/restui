use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

use crate::state::AppState;

pub fn render(frame: &mut Frame, _state: &AppState) {
    let area = centered_rect(65, 75, frame.area());
    frame.render_widget(Clear, area);

    let block = Block::default()
        .title(" Help — restui ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    let keybindings = vec![
        (
            "Navigation",
            vec![
                ("Ctrl+h/j/k/l", "Move between panels"),
                ("j / k", "Move within panel"),
                ("g / G", "Jump to top/bottom"),
            ],
        ),
        (
            "Editing (Request)",
            vec![
                ("{ / }", "Switch tab (Headers/Params/Auth/Cookies)"),
                ("i / e", "Enter insert mode"),
                ("a", "Add new header or param"),
                ("A", "Add header from autocomplete"),
                ("dd / x", "Delete selected item"),
                ("Space", "Toggle enabled/disabled"),
                ("Tab", "Switch key/value field"),
                ("] / [", "Cycle HTTP method"),
                ("Esc", "Return to normal mode"),
            ],
        ),
        (
            "Editing (Body)",
            vec![
                ("i", "Enter insert mode"),
                ("v", "Enter visual mode"),
                ("Tab", "Insert indentation"),
                ("dd", "Delete + yank line"),
                ("yy", "Yank line"),
                ("p", "Paste from yank buffer"),
                ("Up/Down", "Move cursor (insert)"),
            ],
        ),
        (
            "Visual Mode (Body)",
            vec![
                ("y", "Yank selection"),
                ("d / x", "Delete selection"),
                ("h/j/k/l", "Expand selection"),
                ("0 / $", "Home / End of line"),
                ("Esc", "Exit visual mode"),
            ],
        ),
        (
            "HTTP Method (Request panel)",
            vec![
                ("]", "Next method (GET→POST→...)"),
                ("[", "Previous method"),
            ],
        ),
        (
            "Actions",
            vec![
                ("r / Ctrl+R", "Run request (global)"),
                ("y", "Copy response body"),
                ("Y", "Copy as curl command"),
                ("p", "Select environment"),
            ],
        ),
        (
            "Collections (Navigate)",
            vec![
                ("Enter", "Select request"),
                ("j / k", "Move up/down"),
                ("{ / }", "Switch between collections"),
            ],
        ),
        (
            "Collections (CRUD)",
            vec![
                ("s", "Sobrescribir — save over selected request, persist to disk"),
                ("S", "Guardar como — copy current as new request in collection"),
                ("C", "Nuevo vacío — clear all fields, blank request from scratch"),
                ("n", "Nueva colección — create a new .http collection file"),
            ],
        ),
        (
            "General",
            vec![
                (":", "Open command palette"),
                ("?", "Toggle help"),
                ("qq", "Quit (press q twice)"),
                ("T", "Cycle theme"),
                ("Ctrl+R", "Execute request (any panel)"),
                ("Ctrl+V", "Paste from system clipboard"),
                ("Ctrl+D", "Scroll half page down"),
                ("Ctrl+U", "Scroll half page up"),
                ("Ctrl+S", "Toggle SSL insecure mode"),
                ("Ctrl+N / Ctrl+P", "Navigate menus/overlays (cmp-style)"),
            ],
        ),
    ];

    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::from(""));

    for (section, bindings) in &keybindings {
        lines.push(Line::from(Span::styled(
            format!("  {}", section),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )));

        for (key, desc) in bindings {
            lines.push(Line::from(vec![
                Span::styled(
                    format!("    {:17}", key),
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(*desc, Style::default().fg(Color::White)),
            ]));
        }
        lines.push(Line::from(""));
    }

    lines.push(Line::from(Span::styled(
        "  Press ? or Esc to close",
        Style::default().fg(Color::DarkGray),
    )));

    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(paragraph, area);
}

fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}
