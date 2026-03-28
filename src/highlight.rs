use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};

/// Simple JSON syntax highlighting without external dependencies.
/// For a future version, this can be replaced with syntect for richer highlighting.
pub fn highlight_json(text: &str) -> Vec<Line<'static>> {
    text.lines()
        .map(|line| {
            let owned = line.to_string();
            let trimmed = owned.trim().to_string();

            if trimmed.starts_with('"') && trimmed.contains(':') {
                if let Some(colon_pos) = owned.find(':') {
                    let key = owned[..colon_pos].to_string();
                    let value = owned[colon_pos + 1..].to_string();
                    let style = value_style(value.trim());
                    return Line::from(vec![
                        Span::styled(key, Style::default().fg(Color::Cyan)),
                        Span::styled(":".to_string(), Style::default().fg(Color::White)),
                        Span::styled(value, style),
                    ]);
                }
            }

            if trimmed == "{" || trimmed == "}" || trimmed == "[" || trimmed == "]"
                || trimmed == "{}" || trimmed == "[]"
                || trimmed.ends_with('{') || trimmed.ends_with('[')
            {
                return Line::from(Span::styled(
                    owned,
                    Style::default().fg(Color::White),
                ));
            }

            let style = value_style(&trimmed);
            Line::from(Span::styled(owned, style))
        })
        .collect()
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
