use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};

use crate::core::state::AppState;

pub fn render(frame: &mut Frame, state: &AppState) {
    let t = &state.theme;
    let area = centered_rect(65, 80, frame.area());
    frame.render_widget(Clear, area);

    let title = format!(" Help — restui v{} ", env!("CARGO_PKG_VERSION"));
    let block = Block::default()
        .title(title)
        .title_style(Style::default().fg(t.accent).add_modifier(Modifier::BOLD))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(t.accent));

    let keybindings = vec![
        (
            "Navigation",
            vec![
                ("Ctrl+h/j/k/l", "Move between panels"),
                ("1/2/3/4", "Focus panel (Coll/Req/Body/Resp)"),
                ("j / k", "Move up/down within panel"),
                ("h / l", "Move left/right"),
                ("w / b / e", "Word forward / backward / end"),
                ("0 / $", "Line start / end"),
                ("g / G", "Jump to top / bottom"),
                ("Ctrl+D / Ctrl+U", "Half page down / up"),
                ("f/F/t/T + char", "Find char forward / backward"),
            ],
        ),
        (
            "Vim Modes",
            vec![
                ("i / I", "Insert at cursor / start of line"),
                ("a / A", "Append after cursor / end of line"),
                ("o / O", "Open line below / above"),
                ("v", "Visual mode"),
                ("Ctrl+V", "Visual block mode"),
                ("Esc", "Return to normal mode"),
            ],
        ),
        (
            "Editing",
            vec![
                ("dd", "Delete line (+ yank)"),
                ("cc", "Change line (delete + insert)"),
                ("x / s", "Delete char / substitute"),
                ("C / D", "Change / delete to end of line"),
                ("cw / dw / yw", "Change / delete / yank word"),
                ("r + char", "Replace character"),
                ("u / Ctrl+R", "Undo / redo"),
                ("p / P", "Paste from yank buffer"),
                ("Ctrl+V (insert)", "Paste from system clipboard"),
                ("yy", "Yank line"),
            ],
        ),
        (
            "Visual Mode",
            vec![
                ("y", "Yank selection (+ clipboard)"),
                ("d / x", "Delete selection"),
                ("h/j/k/l", "Expand selection"),
                ("Esc", "Exit visual mode"),
            ],
        ),
        (
            "Request Panel",
            vec![
                ("{ / }", "Switch tab (Headers/Cookies/Queries/Params)"),
                ("[ / ]", "Cycle HTTP method"),
                ("e", "Enter field-edit normal mode"),
                ("a", "Add header / param / cookie"),
                ("A", "Add from autocomplete"),
                ("Space", "Toggle enabled/disabled"),
                ("Tab", "Switch key/value field"),
            ],
        ),
        (
            "Response Panel",
            vec![
                ("{ / }", "Switch tab (Body / Type)"),
                ("[ / ]", "Switch type lang (Type / TS / C#)"),
                ("Ctrl+J / Ctrl+K", "Move focus: type editor ↔ response preview"),
                ("R", "Regenerate type from response"),
                ("y", "Copy response body"),
                ("Y", "Copy as curl command"),
                ("/", "Search in body"),
                ("n / N", "Next / previous search match"),
            ],
        ),
        (
            "Collections",
            vec![
                ("Enter", "Select request"),
                ("a", "Add new request to collection"),
                ("s / S", "Save / Save As"),
                ("C", "New empty request (editor only)"),
                ("n", "New collection (in .http/ folder)"),
                ("r", "Rename request"),
                ("dd", "Delete request or collection"),
                ("yy / p", "Yank / paste request"),
                ("m", "Move request to collection"),
                ("Space", "Toggle expand/collapse"),
                ("/", "Filter collections"),
            ],
        ),
        (
            "General",
            vec![
                (":", "Command palette"),
                (":toggle wrap", "Toggle word wrap"),
                ("?", "Toggle this help"),
                ("Ctrl+R", "Execute request"),
                ("Ctrl+S", "Toggle SSL insecure mode"),
                ("Esc", "Cancel in-flight request"),
                ("H", "Request history"),
                ("q", "Quit"),
            ],
        ),
    ];

    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::from(""));

    for (section, bindings) in &keybindings {
        // Section header with separator
        lines.push(Line::from(vec![
            Span::styled("  ", Style::default()),
            Span::styled(
                format!(" {} ", section),
                Style::default()
                    .fg(t.gutter_active)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                " ─".repeat(1),
                Style::default().fg(t.text_dim),
            ),
        ]));

        for (key, desc) in bindings {
            lines.push(Line::from(vec![
                Span::styled(
                    format!("    {:17}", key),
                    Style::default()
                        .fg(t.accent)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(*desc, Style::default().fg(t.text)),
            ]));
        }
        lines.push(Line::from(""));
    }

    lines.push(Line::from(vec![
        Span::styled("  ", Style::default()),
        Span::styled("j/k", Style::default().fg(t.accent)),
        Span::styled(" scroll  ", Style::default().fg(t.text_dim)),
        Span::styled("?/Esc", Style::default().fg(t.accent)),
        Span::styled(" close", Style::default().fg(t.text_dim)),
    ]));

    // Clamp scroll to max
    let total_lines = lines.len() as u16;
    let inner_height = area.height.saturating_sub(2); // border top + bottom
    let max_scroll = total_lines.saturating_sub(inner_height);
    let scroll = state.help_scroll.min(max_scroll);

    let paragraph = Paragraph::new(lines)
        .block(block)
        .wrap(Wrap { trim: false })
        .scroll((scroll, 0));
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
