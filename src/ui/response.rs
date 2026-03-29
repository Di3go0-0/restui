use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

use crate::model::response::StatusCategory;
use crate::state::{AppState, InputMode, Panel};

pub fn render(frame: &mut Frame, state: &AppState, area: Rect) {
    let is_focused = state.active_panel == Panel::Response;
    let is_visual = is_focused && state.mode == InputMode::Visual;
    let is_visual_block = is_focused && state.mode == InputMode::VisualBlock;
    let t = &state.theme;
    let border_color = t.border_for_mode(is_focused, state.mode);

    let block = Block::default()
        .title(" [4] Response ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color));

    // Loading state
    if state.request_in_flight {
        let loading = Paragraph::new(Line::from(vec![
            Span::styled(" ⏳ ", Style::default().fg(Color::Yellow)),
            Span::styled("Sending request...", Style::default().fg(Color::Yellow)),
        ]))
        .block(block);
        frame.render_widget(loading, area);
        return;
    }

    // Error state
    if let Some(ref err) = state.last_error {
        let inner = block.inner(area);
        frame.render_widget(block, area);

        let mut lines = vec![
            Line::from(vec![Span::styled(
                " ERROR ",
                Style::default()
                    .fg(Color::White)
                    .bg(Color::Red)
                    .add_modifier(Modifier::BOLD),
            )]),
            Line::from(""),
        ];
        for err_line in err.lines() {
            lines.push(Line::from(Span::styled(
                format!(" {}", err_line),
                Style::default().fg(Color::Red),
            )));
        }
        let paragraph = Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .scroll(state.response_scroll);
        frame.render_widget(paragraph, inner);
        return;
    }

    // No response yet
    let Some(ref resp) = state.current_response else {
        let placeholder = Paragraph::new(Line::from(Span::styled(
            " Press 'r' or Ctrl+R to send request",
            Style::default().fg(Color::DarkGray),
        )))
        .block(block);
        frame.render_widget(placeholder, area);
        return;
    };

    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.height < 4 {
        return;
    }

    // Calculate headers area height
    let headers_height: u16 = if state.response_headers_expanded {
        // Show all headers, capped at half the available space
        let max_h = (inner.height.saturating_sub(4)) / 2;
        let h = resp.headers.len() as u16;
        h.min(max_h).max(1)
    } else {
        1 // Just the "N response headers" line
    };

    // Reserve 1 line for search bar when search is active or has matches
    let has_search_bar = state.search_active
        || (is_focused && !state.search_query.is_empty() && !state.search_matches.is_empty());
    let search_bar_height: u16 = if has_search_bar { 1 } else { 0 };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),              // Status line
            Constraint::Length(headers_height),  // Headers (expanded or count)
            Constraint::Length(1),              // Separator
            Constraint::Min(1),                // Body with line numbers
            Constraint::Length(search_bar_height), // Search bar
        ])
        .split(inner);

    // Status line
    let status_color = match resp.status_category() {
        StatusCategory::Success => Color::Green,
        StatusCategory::Redirect => Color::Cyan,
        StatusCategory::ClientError => Color::Yellow,
        StatusCategory::ServerError => Color::Red,
        StatusCategory::Unknown => Color::DarkGray,
    };

    let status_line = Line::from(vec![
        Span::styled(
            format!(" {} {} ", resp.status, resp.status_text),
            Style::default()
                .fg(Color::Black)
                .bg(status_color)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled(format!("{} ", resp.elapsed_display()), Style::default().fg(t.accent)),
        Span::styled(resp.size_display(), Style::default().fg(t.json_number)),
        Span::raw("  "),
        Span::styled(
            resp.content_type.as_deref().unwrap_or(""),
            Style::default().fg(t.text_dim),
        ),
    ]);
    frame.render_widget(Paragraph::new(status_line), chunks[0]);

    // Headers area
    if state.response_headers_expanded {
        let header_scroll = state.response_headers_scroll;
        let visible = headers_height as usize;
        for vi in 0..visible {
            let idx = header_scroll + vi;
            if idx >= resp.headers.len() {
                break;
            }
            let (name, value) = &resp.headers[idx];
            let y = chunks[1].y + vi as u16;
            let header_line = Line::from(vec![
                Span::styled(
                    format!("  {}", name),
                    Style::default().fg(t.accent).add_modifier(Modifier::BOLD),
                ),
                Span::styled(": ", Style::default().fg(t.text_dim)),
                Span::styled(value.to_string(), Style::default().fg(t.text)),
            ]);
            let line_area = Rect::new(chunks[1].x, y, chunks[1].width, 1);
            frame.render_widget(Paragraph::new(header_line), line_area);
        }
    } else {
        let toggle_hint = if is_focused { " (H to expand)" } else { "" };
        let headers_info = Line::from(Span::styled(
            format!(" {} response headers{}", resp.headers.len(), toggle_hint),
            Style::default().fg(Color::DarkGray),
        ));
        frame.render_widget(Paragraph::new(headers_info), chunks[1]);
    }

    // Separator
    let sep = Line::from(Span::styled(
        "─".repeat(inner.width as usize),
        Style::default().fg(Color::DarkGray),
    ));
    frame.render_widget(Paragraph::new(sep), chunks[2]);

    // Body area with line numbers (like nvim)
    let body_area = chunks[3];
    let body = resp.formatted_body();
    let body_lines: Vec<&str> = body.lines().collect();
    let total_lines = body_lines.len();

    let gutter_width: u16 = 4;
    let text_area_x = body_area.x + gutter_width;
    let text_area_width = body_area.width.saturating_sub(gutter_width);

    let scroll_y = state.response_scroll.0 as usize;
    let visible_height = body_area.height as usize;
    let cursor_row = state.resp_cursor_row;

    // Visual range
    let (vsr, vsc, ver, vec_) = if is_visual {
        resp_visual_range(state)
    } else {
        (0, 0, 0, 0)
    };

    // Visual block range
    let (vb_min_row, vb_min_col, vb_max_row, vb_max_col) = if is_visual_block {
        resp_visual_block_range(state)
    } else {
        (0, 0, 0, 0)
    };

    // Prepare search info for highlighting
    let search_query_lower = state.search_query.to_lowercase();
    let has_search = !search_query_lower.is_empty() && !state.search_matches.is_empty()
        && state.active_panel == Panel::Response;

    for vi in 0..visible_height {
        let line_idx = scroll_y + vi;
        if line_idx >= total_lines {
            break;
        }

        let y = body_area.y + vi as u16;

        // Line number gutter (relative when focused)
        let line_num_str = if is_focused && line_idx == cursor_row {
            format!("{:>3} ", line_idx + 1)
        } else if is_focused {
            let rel = if line_idx > cursor_row {
                line_idx - cursor_row
            } else {
                cursor_row - line_idx
            };
            format!("{:>3} ", rel)
        } else {
            format!("{:>3} ", line_idx + 1)
        };

        let gutter_style = if line_idx == cursor_row && is_focused {
            Style::default().fg(t.gutter_active).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(t.gutter)
        };

        let gutter_area = Rect::new(body_area.x, y, gutter_width, 1);
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(line_num_str, gutter_style))),
            gutter_area,
        );

        // Content
        let line_text = body_lines.get(line_idx).copied().unwrap_or("");
        let content_line = if is_visual && line_idx >= vsr && line_idx <= ver {
            highlight_visual_line(line_text, line_idx, vsr, vsc, ver, vec_)
        } else if is_visual_block && line_idx >= vb_min_row && line_idx <= vb_max_row {
            highlight_block_line(line_text, vb_min_col, vb_max_col)
        } else if has_search {
            highlight_search_line(line_text, line_idx, state, &search_query_lower, t,
                is_focused && line_idx == cursor_row)
        } else if is_focused && line_idx == cursor_row && !is_visual && !is_visual_block {
            // Highlight current line in normal mode
            Line::from(Span::styled(
                line_text.to_string(),
                Style::default().fg(t.text).bg(t.bg_highlight),
            ))
        } else {
            colorize_response_line(line_text, t)
        };

        let content_area = Rect::new(text_area_x, y, text_area_width, 1);
        frame.render_widget(Paragraph::new(content_line), content_area);
    }

    // Cursor in visual mode
    if is_visual || is_visual_block {
        let cursor_screen_row = cursor_row as i32 - scroll_y as i32;
        if cursor_screen_row >= 0 && (cursor_screen_row as u16) < body_area.height {
            let cursor_x = text_area_x + state.resp_cursor_col as u16;
            let cursor_y = body_area.y + cursor_screen_row as u16;
            if cursor_x < inner.right() {
                frame.set_cursor_position(Position::new(cursor_x, cursor_y));
            }
        }
    }

    // Search bar
    if has_search_bar {
        let search_area = chunks[4];
        let match_info = if state.search_matches.is_empty() {
            "No matches".to_string()
        } else {
            format!("{}/{}", state.search_match_idx + 1, state.search_matches.len())
        };
        let search_line = Line::from(vec![
            Span::styled("/", Style::default().fg(t.accent).add_modifier(Modifier::BOLD)),
            Span::styled(state.search_query.clone(), Style::default().fg(t.text)),
            if state.search_active {
                Span::styled("█", Style::default().fg(t.accent))
            } else {
                Span::raw("")
            },
            Span::styled(format!("  {}", match_info), Style::default().fg(t.text_dim)),
        ]);
        frame.render_widget(Paragraph::new(search_line), search_area);
    }
}

fn highlight_search_line<'a>(
    line: &'a str,
    line_idx: usize,
    state: &AppState,
    query_lower: &str,
    t: &crate::theme::Theme,
    is_cursor_line: bool,
) -> Line<'a> {
    if query_lower.is_empty() {
        if is_cursor_line {
            return Line::from(Span::styled(
                line.to_string(),
                Style::default().fg(t.text).bg(t.bg_highlight),
            ));
        }
        return colorize_response_line(line, t);
    }

    let line_lower = line.to_lowercase();
    let query_len = query_lower.len();
    let mut spans: Vec<Span<'a>> = Vec::new();
    let mut pos = 0;

    // Find current match position
    let current_match = state.search_matches.get(state.search_match_idx).copied();

    let match_bg = Color::Yellow;
    let current_match_bg = Color::Rgb(255, 165, 0); // orange
    let match_fg = Color::Black;
    let base_style = if is_cursor_line {
        Style::default().fg(t.text).bg(t.bg_highlight)
    } else {
        Style::default().fg(t.text)
    };

    while pos < line.len() {
        if let Some(found) = line_lower[pos..].find(query_lower) {
            let match_start = pos + found;
            let match_end = (match_start + query_len).min(line.len());

            // Text before match
            if match_start > pos {
                spans.push(Span::styled(line[pos..match_start].to_string(), base_style));
            }

            // Determine if this is the current match
            let is_current = current_match == Some((line_idx, match_start));
            let bg = if is_current { current_match_bg } else { match_bg };

            spans.push(Span::styled(
                line[match_start..match_end].to_string(),
                Style::default().fg(match_fg).bg(bg).add_modifier(Modifier::BOLD),
            ));

            pos = match_end;
        } else {
            // No more matches, rest of line
            spans.push(Span::styled(line[pos..].to_string(), base_style));
            pos = line.len();
        }
    }

    if spans.is_empty() {
        spans.push(Span::styled(String::new(), base_style));
    }

    Line::from(spans)
}

fn resp_visual_block_range(state: &AppState) -> (usize, usize, usize, usize) {
    let (ar, ac) = (state.resp_visual_anchor_row, state.resp_visual_anchor_col);
    let (cr, cc) = (state.resp_cursor_row, state.resp_cursor_col);
    (ar.min(cr), ac.min(cc), ar.max(cr), ac.max(cc))
}

fn highlight_block_line(line: &str, min_col: usize, max_col: usize) -> Line<'_> {
    let start = min_col.min(line.len());
    let end = max_col.min(line.len());

    let before = &line[..start];
    let selected = &line[start..end];
    let after = &line[end..];

    let sel_style = Style::default()
        .bg(Color::Cyan)
        .fg(Color::Black)
        .add_modifier(Modifier::BOLD);

    Line::from(vec![
        Span::styled(before.to_string(), Style::default().fg(Color::White)),
        Span::styled(selected.to_string(), sel_style),
        Span::styled(after.to_string(), Style::default().fg(Color::White)),
    ])
}

fn resp_visual_range(state: &AppState) -> (usize, usize, usize, usize) {
    let (ar, ac) = (state.resp_visual_anchor_row, state.resp_visual_anchor_col);
    let (cr, cc) = (state.resp_cursor_row, state.resp_cursor_col);
    if (ar, ac) <= (cr, cc) {
        (ar, ac, cr, cc)
    } else {
        (cr, cc, ar, ac)
    }
}

fn highlight_visual_line(line: &str, row: usize, sr: usize, sc: usize, er: usize, ec: usize) -> Line<'_> {
    let start_col = if row == sr { sc } else { 0 };
    let end_col = if row == er { ec } else { line.len() };
    let end_col = end_col.min(line.len());
    let start_col = start_col.min(end_col);

    let before = &line[..start_col];
    let selected = &line[start_col..end_col];
    let after = &line[end_col..];

    let sel_style = Style::default()
        .bg(Color::Magenta)
        .fg(Color::White)
        .add_modifier(Modifier::BOLD);

    Line::from(vec![
        Span::styled(before.to_string(), Style::default().fg(Color::White)),
        Span::styled(selected.to_string(), sel_style),
        Span::styled(after.to_string(), Style::default().fg(Color::White)),
    ])
}

fn colorize_response_line<'a>(line: &'a str, t: &crate::theme::Theme) -> Line<'a> {
    let trimmed = line.trim();

    if trimmed.starts_with('"') && trimmed.contains(':') {
        if let Some(colon_pos) = line.find(':') {
            let (key_part, value_part) = line.split_at(colon_pos);
            return Line::from(vec![
                Span::styled(key_part.to_string(), Style::default().fg(t.json_key)),
                Span::styled(":", Style::default().fg(t.text)),
                Span::styled(
                    value_part[1..].to_string(),
                    value_style(value_part[1..].trim(), t),
                ),
            ]);
        }
    }

    Line::from(Span::styled(line.to_string(), Style::default().fg(t.text)))
}

fn value_style(val: &str, t: &crate::theme::Theme) -> Style {
    let trimmed = val.trim().trim_end_matches(',');
    if trimmed == "true" || trimmed == "false" {
        Style::default().fg(t.json_bool)
    } else if trimmed == "null" {
        Style::default().fg(t.text_dim)
    } else if trimmed.starts_with('"') {
        Style::default().fg(t.json_string)
    } else if trimmed.parse::<f64>().is_ok() {
        Style::default().fg(t.json_number)
    } else {
        Style::default().fg(t.text)
    }
}
