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
    let border_color = if is_visual {
        Color::Magenta
    } else if is_focused {
        Color::Cyan
    } else {
        Color::DarkGray
    };

    let block = Block::default()
        .title(" Response ")
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

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // Status line
            Constraint::Length(1), // Headers + content-type
            Constraint::Length(1), // Separator
            Constraint::Min(1),   // Body with line numbers
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
        Span::styled(format!("{} ", resp.elapsed_display()), Style::default().fg(Color::Cyan)),
        Span::styled(resp.size_display(), Style::default().fg(Color::Magenta)),
        Span::raw("  "),
        Span::styled(
            resp.content_type.as_deref().unwrap_or(""),
            Style::default().fg(Color::DarkGray),
        ),
    ]);
    frame.render_widget(Paragraph::new(status_line), chunks[0]);

    // Headers count
    let headers_info = Line::from(Span::styled(
        format!(" {} response headers", resp.headers.len()),
        Style::default().fg(Color::DarkGray),
    ));
    frame.render_widget(Paragraph::new(headers_info), chunks[1]);

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
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::DarkGray)
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
        } else if is_focused && line_idx == cursor_row && !is_visual {
            // Highlight current line in normal mode
            Line::from(Span::styled(
                line_text.to_string(),
                Style::default().fg(Color::White).bg(Color::Rgb(40, 40, 50)),
            ))
        } else {
            colorize_response_line(line_text)
        };

        let content_area = Rect::new(text_area_x, y, text_area_width, 1);
        frame.render_widget(Paragraph::new(content_line), content_area);
    }

    // Cursor in visual mode
    if is_visual {
        let cursor_screen_row = cursor_row as i32 - scroll_y as i32;
        if cursor_screen_row >= 0 && (cursor_screen_row as u16) < body_area.height {
            let cursor_x = text_area_x + state.resp_cursor_col as u16;
            let cursor_y = body_area.y + cursor_screen_row as u16;
            if cursor_x < inner.right() {
                frame.set_cursor_position(Position::new(cursor_x, cursor_y));
            }
        }
    }
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

fn colorize_response_line(line: &str) -> Line<'_> {
    let trimmed = line.trim();

    if trimmed.starts_with('"') && trimmed.contains(':') {
        if let Some(colon_pos) = line.find(':') {
            let (key_part, value_part) = line.split_at(colon_pos);
            return Line::from(vec![
                Span::styled(key_part.to_string(), Style::default().fg(Color::Cyan)),
                Span::styled(":", Style::default().fg(Color::White)),
                Span::styled(
                    value_part[1..].to_string(),
                    value_style(value_part[1..].trim()),
                ),
            ]);
        }
    }

    Line::from(Span::styled(line.to_string(), Style::default().fg(Color::White)))
}

fn value_style(val: &str) -> Style {
    let trimmed = val.trim().trim_end_matches(',');
    if trimmed == "true" || trimmed == "false" {
        Style::default().fg(Color::Yellow)
    } else if trimmed == "null" {
        Style::default().fg(Color::DarkGray)
    } else if trimmed.starts_with('"') {
        Style::default().fg(Color::Green)
    } else if trimmed.parse::<f64>().is_ok() {
        Style::default().fg(Color::Magenta)
    } else {
        Style::default().fg(Color::White)
    }
}
