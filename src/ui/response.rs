use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

use crate::model::response::StatusCategory;
use crate::state::{AppState, Panel};

pub fn render(frame: &mut Frame, state: &AppState, area: Rect) {
    let is_focused = state.active_panel == Panel::Response;
    let border_color = if is_focused {
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
            Span::styled(
                "Sending request...",
                Style::default().fg(Color::Yellow),
            ),
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

    if inner.height < 3 {
        return;
    }

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // Status line
            Constraint::Length(1), // Headers summary
            Constraint::Length(1), // Separator
            Constraint::Min(1),   // Body
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
        Span::styled(
            format!("{} ", resp.elapsed_display()),
            Style::default().fg(Color::Cyan),
        ),
        Span::styled(
            resp.size_display(),
            Style::default().fg(Color::Magenta),
        ),
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

    // Response body with syntax coloring
    let body = resp.formatted_body();
    let body_lines: Vec<Line> = body
        .lines()
        .map(|l| colorize_response_line(l))
        .collect();

    let body_paragraph = Paragraph::new(body_lines)
        .wrap(Wrap { trim: false })
        .scroll(state.response_scroll);

    frame.render_widget(body_paragraph, chunks[3]);
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

    Line::from(Span::styled(
        line.to_string(),
        Style::default().fg(Color::White),
    ))
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
