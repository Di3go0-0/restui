use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, Paragraph};
use unicode_width::UnicodeWidthStr;

use crate::state::{AppState, InputMode, Panel, RequestFocus, RequestTab};

pub fn render(frame: &mut Frame, state: &AppState, area: Rect) {
    let is_focused = state.active_panel == Panel::Request;
    let is_insert = is_focused && state.mode == InputMode::Insert;
    let t = &state.theme;
    let border_color = t.border_for_mode(is_focused, state.mode);

    let block = Block::default()
        .title(" Request ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.height < 4 {
        return;
    }

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // Method + URL
            Constraint::Length(1), // Separator
            Constraint::Length(1), // Tab bar
            Constraint::Min(1),   // Tab content
        ])
        .split(inner);

    // === Method + URL line ===
    let req = &state.current_request;
    let method_color = t.method_color(req.method);
    let is_url_focused = is_focused && state.request_focus == RequestFocus::Url;
    let url_prefix = if is_url_focused { "▸ " } else { "  " };

    let method_str = format!(" {} ", req.method);
    let method_line = Line::from(vec![
        Span::styled(
            url_prefix,
            Style::default().fg(if is_url_focused { t.accent } else { t.text_dim }),
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
                Style::default().fg(t.text).add_modifier(Modifier::UNDERLINED)
            } else {
                Style::default().fg(t.text)
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

    // === Separator ===
    let sep = Line::from(Span::styled(
        "─".repeat(inner.width as usize),
        Style::default().fg(t.text_dim),
    ));
    frame.render_widget(Paragraph::new(sep), chunks[1]);

    // === Tab bar ===
    let tab_bar = render_tab_bar(state, is_focused);
    frame.render_widget(Paragraph::new(tab_bar), chunks[2]);

    // === Tab content ===
    let content_area = chunks[3];
    match state.request_tab {
        RequestTab::Headers => render_headers_tab(frame, state, content_area, area, is_focused, is_insert),
        RequestTab::Params => render_params_tab(frame, state, content_area, area, is_focused, is_insert),
        RequestTab::Auth => render_placeholder_tab(frame, state, content_area, "Auth configuration coming soon"),
        RequestTab::Cookies => render_placeholder_tab(frame, state, content_area, "Cookie management coming soon"),
    }
}

fn render_tab_bar(state: &AppState, is_focused: bool) -> Line<'static> {
    let t = &state.theme;
    let mut spans = Vec::new();
    spans.push(Span::raw(" "));

    for (i, tab) in RequestTab::ALL.iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled("  ", Style::default().fg(t.text_dim)));
        }

        let is_active = state.request_tab == *tab;
        if is_active {
            spans.push(Span::styled(
                format!("[{}]", tab.label()),
                Style::default()
                    .fg(if is_focused { t.accent } else { t.text })
                    .add_modifier(Modifier::BOLD),
            ));
        } else {
            spans.push(Span::styled(
                tab.label().to_string(),
                Style::default().fg(t.text_dim),
            ));
        }
    }

    // Hint for tab switching
    if is_focused {
        spans.push(Span::styled("  {/}", Style::default().fg(t.text_dim)));
    }

    Line::from(spans)
}

fn render_headers_tab(
    frame: &mut Frame,
    state: &AppState,
    content_area: Rect,
    bounds: Rect,
    is_focused: bool,
    is_insert: bool,
) {
    let t = &state.theme;
    let req = &state.current_request;
    let mut y_offset: u16 = 0;

    if req.headers.is_empty() {
        if y_offset < content_area.height {
            let hint = Line::from(Span::styled(
                "   (none) 'a' to add, 'A' for autocomplete",
                Style::default().fg(t.text_dim),
            ));
            frame.render_widget(
                Paragraph::new(hint),
                Rect::new(content_area.x, content_area.y + y_offset, content_area.width, 1),
            );
        }
        return;
    }

    let mut autocomplete_anchor: Option<(u16, u16)> = None;

    for (i, header) in req.headers.iter().enumerate() {
        if y_offset >= content_area.height {
            break;
        }

        let is_header_focused = is_focused && state.request_focus == RequestFocus::Header(i);
        let prefix = if is_header_focused { "▸" } else { " " };
        let style = if header.enabled {
            Style::default().fg(t.text)
        } else {
            Style::default().fg(t.text_dim)
        };
        let toggle = if header.enabled { "●" } else { "○" };

        let row_y = content_area.y + y_offset;
        let prefix_span = format!(" {} {} ", prefix, toggle);
        let prefix_width = UnicodeWidthStr::width(prefix_span.as_str());

        if is_header_focused && is_insert {
            let name_style = if state.header_edit_field == 0 {
                Style::default().fg(t.accent).add_modifier(Modifier::UNDERLINED)
            } else {
                style.add_modifier(Modifier::BOLD)
            };
            let value_style = if state.header_edit_field == 1 {
                Style::default().fg(t.accent).add_modifier(Modifier::UNDERLINED)
            } else {
                style
            };
            let line = Line::from(vec![
                Span::styled(&prefix_span, Style::default().fg(t.border_insert)),
                Span::styled(&header.name, name_style),
                Span::styled(": ", style),
                Span::styled(&header.value, value_style),
            ]);
            frame.render_widget(
                Paragraph::new(line),
                Rect::new(content_area.x, row_y, content_area.width, 1),
            );

            // Position cursor
            let name_display_width = UnicodeWidthStr::width(header.name.as_str());
            let cursor_before = if state.header_edit_field == 0 {
                &header.name[..header.name.char_indices().nth(state.header_edit_cursor).map(|(i,_)|i).unwrap_or(header.name.len())]
            } else {
                &header.value[..header.value.char_indices().nth(state.header_edit_cursor).map(|(i,_)|i).unwrap_or(header.value.len())]
            };
            let cursor_text_width = UnicodeWidthStr::width(cursor_before) as u16;
            let cursor_x = if state.header_edit_field == 0 {
                content_area.x + prefix_width as u16 + cursor_text_width
            } else {
                content_area.x + prefix_width as u16 + name_display_width as u16 + 2 + cursor_text_width
            };
            if cursor_x < bounds.right() {
                frame.set_cursor_position(Position::new(cursor_x, row_y));
            }

            // Save anchor for autocomplete popup
            if state.header_edit_field == 0 && state.autocomplete.is_some() {
                autocomplete_anchor = Some((content_area.x + prefix_width as u16, row_y + 1));
            }
        } else {
            let prefix_style = Style::default().fg(
                if is_header_focused { t.accent } else { t.text_dim },
            );
            let line = Line::from(vec![
                Span::styled(&prefix_span, prefix_style),
                Span::styled(&header.name, style.add_modifier(Modifier::BOLD)),
                Span::styled(": ", style),
                Span::styled(&header.value, style),
            ]);
            frame.render_widget(
                Paragraph::new(line),
                Rect::new(content_area.x, row_y, content_area.width, 1),
            );
        }

        y_offset += 1;
    }

    // Render autocomplete popup
    if let (Some((ax, ay)), Some(ac)) = (autocomplete_anchor, &state.autocomplete) {
        render_autocomplete_popup(frame, ac, ax, ay, bounds);
    }
}

fn render_params_tab(
    frame: &mut Frame,
    state: &AppState,
    content_area: Rect,
    bounds: Rect,
    is_focused: bool,
    is_insert: bool,
) {
    let t = &state.theme;
    let req = &state.current_request;
    let mut y_offset: u16 = 0;

    if req.query_params.is_empty() {
        if content_area.height > 0 {
            let hint = Line::from(Span::styled(
                "   (none) 'a' to add a query parameter",
                Style::default().fg(t.text_dim),
            ));
            frame.render_widget(
                Paragraph::new(hint),
                Rect::new(content_area.x, content_area.y, content_area.width, 1),
            );
        }
        return;
    }

    for (i, param) in req.query_params.iter().enumerate() {
        if y_offset >= content_area.height {
            break;
        }

        let is_param_focused = is_focused && state.request_focus == RequestFocus::Param(i);
        let prefix = if is_param_focused { "▸" } else { " " };
        let style = if param.enabled {
            Style::default().fg(t.text)
        } else {
            Style::default().fg(t.text_dim)
        };
        let toggle = if param.enabled { "●" } else { "○" };

        let row_y = content_area.y + y_offset;
        let prefix_span = format!(" {} {} ", prefix, toggle);
        let prefix_width = UnicodeWidthStr::width(prefix_span.as_str());

        if is_param_focused && is_insert {
            let key_style = if state.param_edit_field == 0 {
                Style::default().fg(t.accent).add_modifier(Modifier::UNDERLINED)
            } else {
                style.add_modifier(Modifier::BOLD)
            };
            let value_style = if state.param_edit_field == 1 {
                Style::default().fg(t.accent).add_modifier(Modifier::UNDERLINED)
            } else {
                style
            };
            let line = Line::from(vec![
                Span::styled(&prefix_span, Style::default().fg(t.border_insert)),
                Span::styled(&param.key, key_style),
                Span::styled(" = ", style),
                Span::styled(&param.value, value_style),
            ]);
            frame.render_widget(
                Paragraph::new(line),
                Rect::new(content_area.x, row_y, content_area.width, 1),
            );

            // Position cursor
            let key_display_width = UnicodeWidthStr::width(param.key.as_str());
            let cursor_before = if state.param_edit_field == 0 {
                &param.key[..param.key.char_indices().nth(state.param_edit_cursor).map(|(i,_)|i).unwrap_or(param.key.len())]
            } else {
                &param.value[..param.value.char_indices().nth(state.param_edit_cursor).map(|(i,_)|i).unwrap_or(param.value.len())]
            };
            let cursor_text_width = UnicodeWidthStr::width(cursor_before) as u16;
            let cursor_x = if state.param_edit_field == 0 {
                content_area.x + prefix_width as u16 + cursor_text_width
            } else {
                content_area.x + prefix_width as u16 + key_display_width as u16 + 3 + cursor_text_width // " = " is 3 chars
            };
            if cursor_x < bounds.right() {
                frame.set_cursor_position(Position::new(cursor_x, row_y));
            }
        } else {
            let prefix_style = Style::default().fg(
                if is_param_focused { t.accent } else { t.text_dim },
            );
            let line = Line::from(vec![
                Span::styled(&prefix_span, prefix_style),
                Span::styled(&param.key, style.add_modifier(Modifier::BOLD)),
                Span::styled(" = ", style),
                Span::styled(&param.value, style),
            ]);
            frame.render_widget(
                Paragraph::new(line),
                Rect::new(content_area.x, row_y, content_area.width, 1),
            );
        }

        y_offset += 1;
    }
}

fn render_placeholder_tab(
    frame: &mut Frame,
    state: &AppState,
    content_area: Rect,
    message: &str,
) {
    let t = &state.theme;
    if content_area.height > 0 {
        let hint = Line::from(Span::styled(
            format!("   {}", message),
            Style::default().fg(t.text_dim),
        ));
        frame.render_widget(
            Paragraph::new(hint),
            Rect::new(content_area.x, content_area.y, content_area.width, 1),
        );
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
