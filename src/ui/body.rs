use ratatui::Frame;
use ratatui::layout::{Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::state::{AppState, InputMode, Panel};

pub fn render(frame: &mut Frame, state: &AppState, area: Rect) {
    let is_focused = state.active_panel == Panel::Body;
    let is_insert = is_focused && state.mode == InputMode::Insert;
    let is_visual = is_focused && state.mode == InputMode::Visual;
    let is_normal_focused = is_focused && state.mode == InputMode::Normal;
    let t = &state.theme;
    let border_color = t.border_for_mode(is_focused, state.mode);

    let body_type_label = state.body_type.label();
    let title = if let Some(ref err) = state.body_validation_error {
        format!(" Body [{}] ⚠ {} ", body_type_label, err)
    } else {
        format!(" Body [{}] ", body_type_label)
    };
    let title_style = if state.body_validation_error.is_some() {
        Style::default().fg(Color::Red)
    } else {
        Style::default().fg(border_color)
    };

    let block = Block::default()
        .title(title)
        .title_style(title_style)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.width < 4 || inner.height < 1 {
        return;
    }

    let body_text = state.current_request.body.as_deref().unwrap_or("");

    if body_text.is_empty() && !is_insert {
        let placeholder = Paragraph::new(Line::from(Span::styled(
            " Press 'i' to start typing, Ctrl+V to paste",
            Style::default().fg(Color::DarkGray),
        )));
        frame.render_widget(placeholder, inner);
        return;
    }

    // Line number gutter width
    let body_lines: Vec<&str> = if body_text.is_empty() { vec![""] } else { body_text.lines().collect() };
    let total_lines = body_lines.len();
    let gutter_width: u16 = 4; // "NNN "
    let text_area_x = inner.x + gutter_width;
    let text_area_width = inner.width.saturating_sub(gutter_width);

    let scroll_y = state.body_scroll.0 as usize;
    let visible_height = inner.height as usize;
    let cursor_row = state.body_cursor_row;

    // Render line by line for gutter + content
    for (vi, screen_row) in (0..visible_height).enumerate() {
        let line_idx = scroll_y + vi;
        if line_idx >= total_lines {
            break;
        }

        let y = inner.y + screen_row as u16;

        // Line number gutter (relative numbers when focused)
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

        let gutter_span = Span::styled(line_num_str, gutter_style);
        let gutter_line = Line::from(vec![gutter_span]);
        let gutter_area = Rect::new(inner.x, y, gutter_width, 1);
        frame.render_widget(Paragraph::new(gutter_line), gutter_area);

        // Content
        let line_text = body_lines.get(line_idx).copied().unwrap_or("");
        let content_line = if is_visual {
            let (sr, sc, er, ec) = visual_range(state);
            if line_idx >= sr && line_idx <= er {
                highlight_visual_line(line_text, line_idx, sr, sc, er, ec)
            } else {
                colorize_json_line(line_text, t)
            }
        } else {
            // Highlight current line background in normal mode
            if is_normal_focused && line_idx == cursor_row {
                Line::from(Span::styled(
                    line_text.to_string(),
                    Style::default().fg(t.text).bg(t.bg_highlight),
                ))
            } else {
                colorize_json_line(line_text, t)
            }
        };

        let content_area = Rect::new(text_area_x, y, text_area_width, 1);
        frame.render_widget(Paragraph::new(content_line), content_area);
    }

    // Cursor position
    if is_insert || is_visual {
        let cursor_screen_row = cursor_row as i32 - scroll_y as i32;
        if cursor_screen_row >= 0 && (cursor_screen_row as u16) < inner.height {
            let cursor_x = text_area_x + state.body_cursor_col as u16;
            let cursor_y = inner.y + cursor_screen_row as u16;
            if cursor_x < inner.right() {
                frame.set_cursor_position(Position::new(cursor_x, cursor_y));
            }
        }
    }
}

fn visual_range(state: &AppState) -> (usize, usize, usize, usize) {
    let (ar, ac) = (state.visual_anchor_row, state.visual_anchor_col);
    let (cr, cc) = (state.body_cursor_row, state.body_cursor_col);
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

fn colorize_json_line<'a>(line: &'a str, t: &crate::theme::Theme) -> Line<'a> {
    let trimmed = line.trim();

    if trimmed.starts_with('"') && trimmed.contains(':') {
        if let Some(colon_pos) = line.find(':') {
            let (key_part, value_part) = line.split_at(colon_pos);
            return Line::from(vec![
                Span::styled(key_part.to_string(), Style::default().fg(t.json_key)),
                Span::styled(":", Style::default().fg(t.text)),
                Span::styled(
                    value_part[1..].to_string(),
                    style_for_value(value_part[1..].trim(), t),
                ),
            ]);
        }
    }

    Line::from(Span::styled(line.to_string(), Style::default().fg(t.text)))
}

fn style_for_value(val: &str, t: &crate::theme::Theme) -> Style {
    let trimmed = val.trim().trim_end_matches(',');
    if trimmed == "true" || trimmed == "false" {
        Style::default().fg(t.json_bool)
    } else if trimmed == "null" {
        Style::default().fg(t.text_dim).add_modifier(Modifier::ITALIC)
    } else if trimmed.starts_with('"') {
        Style::default().fg(t.json_string)
    } else if trimmed.parse::<f64>().is_ok() {
        Style::default().fg(t.json_number)
    } else {
        Style::default().fg(t.text)
    }
}
