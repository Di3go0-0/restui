use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, Paragraph};
use unicode_width::UnicodeWidthStr;

use crate::model::request::HttpMethod;
use crate::state::{AppState, InputMode, Panel, RequestFocus};

pub fn render(frame: &mut Frame, state: &AppState, area: Rect) {
    let is_focused = state.active_panel == Panel::Request;
    let is_insert = is_focused && state.mode == InputMode::Insert;
    let border_color = if is_insert {
        Color::Green
    } else if is_focused {
        Color::Cyan
    } else {
        Color::DarkGray
    };

    let block = Block::default()
        .title(" Request ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.height < 3 {
        return;
    }

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // Method + URL
            Constraint::Length(1), // Separator
            Constraint::Min(1),   // Headers
        ])
        .split(inner);

    // Method + URL line
    let req = &state.current_request;
    let method_color = method_to_color(req.method);
    let is_url_focused = is_focused && state.request_focus == RequestFocus::Url;
    let url_prefix = if is_url_focused { "▸ " } else { "  " };

    let method_str = format!(" {} ", req.method);
    let method_line = Line::from(vec![
        Span::styled(
            url_prefix,
            Style::default().fg(if is_url_focused { Color::Cyan } else { Color::DarkGray }),
        ),
        Span::styled(
            &method_str,
            Style::default()
                .fg(Color::Black)
                .bg(method_color)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" "),
        Span::styled(
            &req.url,
            if is_url_focused && is_insert {
                Style::default().fg(Color::White).add_modifier(Modifier::UNDERLINED)
            } else {
                Style::default().fg(Color::White)
            },
        ),
    ]);
    frame.render_widget(Paragraph::new(method_line), chunks[0]);

    // Cursor when editing URL
    if is_insert && state.request_focus == RequestFocus::Url {
        let prefix_display = 2 + UnicodeWidthStr::width(method_str.as_str()) + 1;
        let url_before = &req.url[..req.url.char_indices().nth(state.url_cursor).map(|(i,_)|i).unwrap_or(req.url.len())];
        let cursor_x = inner.x + prefix_display as u16 + UnicodeWidthStr::width(url_before) as u16;
        if cursor_x < area.right() {
            frame.set_cursor_position(Position::new(cursor_x, chunks[0].y));
        }
    }

    // Separator
    let sep = Line::from(Span::styled(
        "─".repeat(inner.width as usize),
        Style::default().fg(Color::DarkGray),
    ));
    frame.render_widget(Paragraph::new(sep), chunks[1]);

    // Headers — render line by line for precise cursor positioning
    let headers_area = chunks[2];
    let mut y_offset: u16 = 0;

    // Title line
    let title_line = Line::from(Span::styled(
        " Headers",
        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
    ));
    if y_offset < headers_area.height {
        frame.render_widget(
            Paragraph::new(title_line),
            Rect::new(headers_area.x, headers_area.y + y_offset, headers_area.width, 1),
        );
        y_offset += 1;
    }

    if req.headers.is_empty() {
        if y_offset < headers_area.height {
            let hint = Line::from(Span::styled(
                "   (none) 'a' to add, 'A' for autocomplete",
                Style::default().fg(Color::DarkGray),
            ));
            frame.render_widget(
                Paragraph::new(hint),
                Rect::new(headers_area.x, headers_area.y + y_offset, headers_area.width, 1),
            );
        }
    } else {
        let mut autocomplete_anchor: Option<(u16, u16)> = None; // (x, y) for popup

        for (i, header) in req.headers.iter().enumerate() {
            if y_offset >= headers_area.height {
                break;
            }

            let is_header_focused = is_focused && state.request_focus == RequestFocus::Header(i);
            let prefix = if is_header_focused { "▸" } else { " " };
            let style = if header.enabled {
                Style::default().fg(Color::White)
            } else {
                Style::default().fg(Color::DarkGray)
            };
            let toggle = if header.enabled { "●" } else { "○" };

            let row_y = headers_area.y + y_offset;
            let prefix_span = format!(" {} {} ", prefix, toggle);
            let prefix_width = UnicodeWidthStr::width(prefix_span.as_str());

            if is_header_focused && is_insert {
                let name_style = if state.header_edit_field == 0 {
                    Style::default().fg(Color::Cyan).add_modifier(Modifier::UNDERLINED)
                } else {
                    style.add_modifier(Modifier::BOLD)
                };
                let value_style = if state.header_edit_field == 1 {
                    Style::default().fg(Color::Cyan).add_modifier(Modifier::UNDERLINED)
                } else {
                    style
                };
                let line = Line::from(vec![
                    Span::styled(&prefix_span, Style::default().fg(Color::Green)),
                    Span::styled(&header.name, name_style),
                    Span::styled(": ", style),
                    Span::styled(&header.value, value_style),
                ]);
                frame.render_widget(
                    Paragraph::new(line),
                    Rect::new(headers_area.x, row_y, headers_area.width, 1),
                );

                // Position cursor using display width, not byte length
                let name_display_width = UnicodeWidthStr::width(header.name.as_str());
                let cursor_before = if state.header_edit_field == 0 {
                    &header.name[..header.name.char_indices().nth(state.header_edit_cursor).map(|(i,_)|i).unwrap_or(header.name.len())]
                } else {
                    &header.value[..header.value.char_indices().nth(state.header_edit_cursor).map(|(i,_)|i).unwrap_or(header.value.len())]
                };
                let cursor_text_width = UnicodeWidthStr::width(cursor_before) as u16;
                let cursor_x = if state.header_edit_field == 0 {
                    headers_area.x + prefix_width as u16 + cursor_text_width
                } else {
                    headers_area.x + prefix_width as u16 + name_display_width as u16 + 2 + cursor_text_width
                };
                if cursor_x < area.right() {
                    frame.set_cursor_position(Position::new(cursor_x, row_y));
                }

                // Save anchor for autocomplete popup
                if state.header_edit_field == 0 && state.autocomplete.is_some() {
                    autocomplete_anchor = Some((headers_area.x + prefix_width as u16, row_y + 1));
                }
            } else {
                let prefix_style = Style::default().fg(
                    if is_header_focused { Color::Cyan } else { Color::DarkGray },
                );
                let line = Line::from(vec![
                    Span::styled(&prefix_span, prefix_style),
                    Span::styled(&header.name, style.add_modifier(Modifier::BOLD)),
                    Span::styled(": ", style),
                    Span::styled(&header.value, style),
                ]);
                frame.render_widget(
                    Paragraph::new(line),
                    Rect::new(headers_area.x, row_y, headers_area.width, 1),
                );
            }

            y_offset += 1;
        }

        // Render autocomplete popup
        if let (Some((ax, ay)), Some(ac)) = (autocomplete_anchor, &state.autocomplete) {
            render_autocomplete_popup(frame, ac, ax, ay, area);
        }
    }

    // Query params
    if !req.query_params.is_empty() && y_offset + 2 < headers_area.height {
        y_offset += 1; // blank line
        let qp_title = Line::from(Span::styled(
            " Query Params",
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
        ));
        frame.render_widget(
            Paragraph::new(qp_title),
            Rect::new(headers_area.x, headers_area.y + y_offset, headers_area.width, 1),
        );
        y_offset += 1;

        for param in &req.query_params {
            if y_offset >= headers_area.height {
                break;
            }
            let style = if param.enabled {
                Style::default().fg(Color::White)
            } else {
                Style::default().fg(Color::DarkGray)
            };
            let toggle = if param.enabled { "●" } else { "○" };
            let line = Line::from(vec![
                Span::styled(format!("   {} ", toggle), Style::default().fg(Color::Cyan)),
                Span::styled(&param.key, style.add_modifier(Modifier::BOLD)),
                Span::styled(" = ", style),
                Span::styled(&param.value, style),
            ]);
            frame.render_widget(
                Paragraph::new(line),
                Rect::new(headers_area.x, headers_area.y + y_offset, headers_area.width, 1),
            );
            y_offset += 1;
        }
    }
}

fn render_autocomplete_popup(
    frame: &mut Frame,
    ac: &crate::state::Autocomplete,
    x: u16,
    y: u16,
    bounds: Rect,
) {
    let max_items = ac.filtered.len().min(8);
    if max_items == 0 {
        return;
    }

    let popup_width = 40u16.min(bounds.right().saturating_sub(x));
    let popup_height = (max_items as u16 + 2).min(bounds.bottom().saturating_sub(y)); // +2 for border
    if popup_width < 10 || popup_height < 3 {
        return;
    }

    let popup_area = Rect::new(x, y, popup_width, popup_height);
    frame.render_widget(Clear, popup_area);

    let block = Block::default()
        .title(" Ctrl+n/p ↕  Ctrl+y ✓ ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    let items: Vec<ListItem> = ac
        .filtered
        .iter()
        .take(max_items)
        .map(|(name, value)| {
            let val_display = if value.is_empty() { "" } else { value.as_str() };
            ListItem::new(Line::from(vec![
                Span::styled(name, Style::default().fg(Color::Yellow)),
                if val_display.is_empty() {
                    Span::raw("")
                } else {
                    Span::styled(format!(": {}", val_display), Style::default().fg(Color::DarkGray))
                },
            ]))
        })
        .collect();

    let mut list_state = ratatui::widgets::ListState::default();
    list_state.select(Some(ac.selected));

    let list = List::new(items)
        .block(block)
        .highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▸ ");

    frame.render_stateful_widget(list, popup_area, &mut list_state);
}

fn method_to_color(method: HttpMethod) -> Color {
    match method {
        HttpMethod::GET => Color::Green,
        HttpMethod::POST => Color::Blue,
        HttpMethod::PUT => Color::Yellow,
        HttpMethod::PATCH => Color::Yellow,
        HttpMethod::DELETE => Color::Red,
        HttpMethod::HEAD => Color::Magenta,
        HttpMethod::OPTIONS => Color::Cyan,
    }
}
