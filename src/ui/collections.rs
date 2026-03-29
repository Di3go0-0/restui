use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph};

use crate::state::{AppState, Panel};

pub fn render(frame: &mut Frame, state: &AppState, area: Rect) {
    let is_focused = state.active_panel == Panel::Collections;
    let t = &state.theme;
    let border_color = if is_focused { t.border_focused } else { t.border_unfocused };

    let coll_count = state.collections.len();
    let title = if coll_count > 0 {
        format!(
            " Collections ({}/{}) ",
            state.active_collection + 1,
            coll_count
        )
    } else {
        " Collections ".to_string()
    };

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color));

    let inner = block.inner(area);

    if state.collections.is_empty() {
        frame.render_widget(block, area);
        let lines = vec![
            Line::from(""),
            Line::from(Span::styled(
                " No .http files found",
                Style::default().fg(t.text_dim),
            )),
            Line::from(""),
            Line::from(Span::styled(
                " Press 'n' to create one",
                Style::default().fg(t.gutter_active),
            )),
        ];
        let p = Paragraph::new(lines);
        frame.render_widget(p, inner);
        return;
    }

    // Build items from collection_items (already computed by app)
    let items: Vec<ListItem> = state
        .collection_items
        .iter()
        .enumerate()
        .map(|(i, item)| {
            let is_collection_header = !item.starts_with("  ");
            if is_collection_header {
                // Collection header line: "● name" or "○ name"
                ListItem::new(Line::from(vec![Span::styled(
                    item.clone(),
                    Style::default()
                        .fg(t.gutter_active)
                        .add_modifier(Modifier::BOLD),
                )]))
            } else {
                // Request line: "  METHOD url"
                let trimmed = item.trim_start();
                let (method, rest) = trimmed
                    .split_once(' ')
                    .unwrap_or((trimmed, ""));
                let method_color = match method {
                    "GET" => t.method_get,
                    "POST" => t.method_post,
                    "PUT" => t.method_put,
                    "PATCH" => t.method_patch,
                    "DELETE" => t.method_delete,
                    "HEAD" => t.method_head,
                    "OPTIONS" => t.method_options,
                    _ => t.text,
                };
                ListItem::new(Line::from(vec![
                    Span::raw("  "),
                    Span::styled(
                        format!("{:7}", method),
                        Style::default().fg(method_color),
                    ),
                    Span::raw(" "),
                    Span::styled(
                        truncate_url(rest, area.width.saturating_sub(16) as usize),
                        Style::default().fg(t.text),
                    ),
                ]))
            }
        })
        .collect();

    // Split: list + bottom hints
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(3)])
        .split(inner);

    let mut list_state = state.collections_state.clone();
    let list = List::new(items)
        .highlight_style(
            Style::default()
                .bg(t.bg_highlight)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▸ ");

    frame.render_widget(block, area);
    frame.render_stateful_widget(list, chunks[0], &mut list_state);

    // Hints at bottom
    if is_focused {
        let hints = vec![
            Line::from(vec![
                Span::styled(" s", Style::default().fg(t.accent)),
                Span::styled(":save ", Style::default().fg(t.text_dim)),
                Span::styled("R", Style::default().fg(t.accent)),
                Span::styled(":rename ", Style::default().fg(t.text_dim)),
                Span::styled("D", Style::default().fg(t.accent)),
                Span::styled(":del ", Style::default().fg(t.text_dim)),
                Span::styled("yy", Style::default().fg(t.accent)),
                Span::styled(":copy ", Style::default().fg(t.text_dim)),
                Span::styled("p", Style::default().fg(t.accent)),
                Span::styled(":paste", Style::default().fg(t.text_dim)),
            ]),
            Line::from(vec![
                Span::styled(" m", Style::default().fg(t.accent)),
                Span::styled(":move ", Style::default().fg(t.text_dim)),
                Span::styled("n", Style::default().fg(t.accent)),
                Span::styled(":coll ", Style::default().fg(t.text_dim)),
                Span::styled("Sp", Style::default().fg(t.accent)),
                Span::styled(":fold ", Style::default().fg(t.text_dim)),
                Span::styled("{/}", Style::default().fg(t.accent)),
                Span::styled(":switch", Style::default().fg(t.text_dim)),
            ]),
        ];
        let hints_p = Paragraph::new(hints);
        frame.render_widget(hints_p, chunks[1]);
    }
}

fn truncate_url(url: &str, max_len: usize) -> String {
    if url.len() <= max_len {
        url.to_string()
    } else if max_len > 3 {
        format!("{}...", &url[..max_len - 3])
    } else {
        url[..max_len].to_string()
    }
}
