use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph};

use crate::core::state::{AppState, Panel};

pub fn render(frame: &mut Frame, state: &AppState, area: Rect) {
    let is_focused = state.active_panel == Panel::Collections;
    let t = &state.theme;
    let border_color = if is_focused { t.border_focused } else { t.border_unfocused };

    let coll_count = state.collections.len();
    let has_filter = !state.collections_view.filter.is_empty() && !state.collections_view.filter_active;
    let title = if has_filter {
        format!(
            " [1] Collections (filter: \"{}\") ",
            state.collections_view.filter
        )
    } else if coll_count > 0 {
        format!(
            " [1] Collections ({}/{}) ",
            state.collections_view.active + 1,
            coll_count
        )
    } else {
        " [1] Collections ".to_string()
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

    // Build items from collections_view.items (already computed by app)
    let items: Vec<ListItem> = state
        .collections_view.items
        .iter()
        .enumerate()
        .map(|(_i, item)| {
            let is_collection_header = item.starts_with('▼') || item.starts_with('▶');
            if is_collection_header {
                // Collection header line: "● name" or "○ name"
                ListItem::new(Line::from(vec![Span::styled(
                    format!(" {}", item),
                    Style::default()
                        .fg(t.gutter_active)
                        .add_modifier(Modifier::BOLD),
                )]))
            } else {
                // Request line: "│ ├ METHOD name" or "│ └ METHOD name"
                // Extract tree prefix, method, and name
                let (tree_prefix, after_tree) = if item.starts_with("│") {
                    // Find the method after tree chars (│ ├ or │ └)
                    let after = item.trim_start_matches(|c: char| "│├└ ".contains(c));
                    let prefix_len = item.len() - after.len();
                    (&item[..prefix_len], after)
                } else {
                    let trimmed = item.trim_start();
                    let prefix_len = item.len() - trimmed.len();
                    (&item[..prefix_len], trimmed)
                };
                let (method, rest) = after_tree
                    .split_once(' ')
                    .unwrap_or((after_tree, ""));
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
                    Span::styled(format!(" {}", tree_prefix), Style::default().fg(t.text_dim)),
                    Span::styled(
                        format!("{:7}", method),
                        Style::default().fg(method_color),
                    ),
                    Span::raw(" "),
                    Span::styled(
                        truncate_url(rest, area.width.saturating_sub(17) as usize),
                        Style::default().fg(t.text),
                    ),
                ]))
            }
        })
        .collect();

    // Determine if we need filter bar space
    let show_filter_bar = state.collections_view.filter_active;
    let bottom_height = if is_focused { 3 } else { 0 } + if show_filter_bar { 1 } else { 0 };

    // Split: list + bottom (hints + optional filter bar)
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(bottom_height)])
        .split(inner);

    let mut list_state = state.collections_view.list_state.clone();
    let list = List::new(items)
        .highlight_style(
            Style::default()
                .bg(t.bg_highlight)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▸ ");

    frame.render_widget(block, area);
    frame.render_stateful_widget(list, chunks[0], &mut list_state);

    // Bottom section: hints + filter bar
    if is_focused || show_filter_bar {
        let mut bottom_constraints = Vec::new();
        if show_filter_bar {
            bottom_constraints.push(Constraint::Length(1));
        }
        if is_focused {
            bottom_constraints.push(Constraint::Length(3));
        }

        let bottom_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints(bottom_constraints)
            .split(chunks[1]);

        let mut chunk_idx = 0;

        // Filter bar
        if show_filter_bar {
            let filter_line = Line::from(vec![
                Span::styled("/", Style::default().fg(t.accent)),
                Span::styled(
                    &state.collections_view.filter,
                    Style::default().fg(t.text),
                ),
                Span::styled(
                    "_",
                    Style::default().fg(t.accent).add_modifier(Modifier::SLOW_BLINK),
                ),
            ]);
            let filter_p = Paragraph::new(filter_line);
            frame.render_widget(filter_p, bottom_chunks[chunk_idx]);
            chunk_idx += 1;
        }

        // Hints
        if is_focused && chunk_idx < bottom_chunks.len() {
            let hints = vec![
                Line::from(vec![
                    Span::styled(" s", Style::default().fg(t.accent)),
                    Span::styled(":save ", Style::default().fg(t.text_dim)),
                    Span::styled("r", Style::default().fg(t.accent)),
                    Span::styled(":rename ", Style::default().fg(t.text_dim)),
                    Span::styled("dd", Style::default().fg(t.accent)),
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
                    Span::styled("/", Style::default().fg(t.accent)),
                    Span::styled(":filter ", Style::default().fg(t.text_dim)),
                    Span::styled("{/}", Style::default().fg(t.accent)),
                    Span::styled(":switch", Style::default().fg(t.text_dim)),
                ]),
            ];
            let hints_p = Paragraph::new(hints);
            frame.render_widget(hints_p, bottom_chunks[chunk_idx]);
        }
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
