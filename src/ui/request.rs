use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, Paragraph};
use unicode_width::UnicodeWidthStr;

use crate::core::state::{AppState, InputMode, Panel, RequestFocus, RequestTab};

pub fn render(frame: &mut Frame, state: &AppState, area: Rect) {
    let is_focused = state.active_panel == Panel::Request;
    let is_insert = is_focused && state.mode == InputMode::Insert;
    let is_field_edit = is_focused && state.request_edit.field_editing;
    let is_editing = is_insert || (is_field_edit && state.mode == InputMode::Normal);
    let is_visual = is_field_edit && state.mode == InputMode::Visual;
    let t = &state.theme;
    let border_color = t.border_for_mode(is_focused, state.mode);

    let block = Block::default()
        .title(" [2] Request ")
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
    let is_url_focused = is_focused && state.request_edit.focus == RequestFocus::Url;
    let url_prefix = if is_url_focused { "▸ " } else { "  " };

    let method_str = format!(" {} ", req.method);
    let prefix_display_width = UnicodeWidthStr::width(url_prefix) + UnicodeWidthStr::width(method_str.as_str()) + 1; // +1 for space after method

    // Build display URL: in edit modes show raw url, otherwise show url + enabled params
    let is_url_editing = is_url_focused && (is_editing || is_visual);
    let display_url = if is_url_editing {
        req.url.clone()
    } else {
        build_display_url(&req.url, &req.query_params)
    };

    // Horizontal scroll: derive scroll offset so cursor stays visible
    let url_area_width = (inner.width as usize).saturating_sub(prefix_display_width);
    let scroll = if is_url_editing && url_area_width > 0 {
        let cursor = state.request_edit.url_cursor;
        if cursor < url_area_width {
            0
        } else {
            cursor - url_area_width + 1
        }
    } else {
        // When not editing, scroll to start
        0
    };
    let visible_url = if display_url.len() > scroll {
        let start_byte = display_url.char_indices().nth(scroll).map(|(i,_)|i).unwrap_or(display_url.len());
        &display_url[start_byte..]
    } else {
        ""
    };
    // Truncate to available width, reserving space for overflow indicator
    let url_char_count = visible_url.chars().count();
    let url_overflows = !is_url_editing && url_char_count > url_area_width && url_area_width > 1;
    let take_width = if url_overflows { url_area_width - 1 } else { url_area_width };
    let truncated_url: String = visible_url.chars().take(take_width).collect();

    let url_base_style = if is_url_editing {
        Style::default().fg(t.text).add_modifier(Modifier::UNDERLINED)
    } else {
        Style::default().fg(t.text)
    };

    // Build URL spans with block cursor / visual highlight
    let url_spans = if is_url_focused && (is_editing || is_visual) && !is_insert {
        let cursor_in_visible = state.request_edit.url_cursor.saturating_sub(scroll);
        build_field_spans(&truncated_url, cursor_in_visible,
            if is_visual { Some((state.request_edit.visual_anchor.saturating_sub(scroll), cursor_in_visible)) } else { None },
            url_base_style, t)
    } else {
        colorize_url(&truncated_url, t)
    };

    let mut method_spans = vec![
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
    ];
    method_spans.extend(url_spans);
    if url_overflows {
        method_spans.push(Span::styled("…", Style::default().fg(t.text_dim)));
    }
    frame.render_widget(Paragraph::new(Line::from(method_spans)), chunks[0]);

    // Cursor when in insert mode (blinking line cursor)
    if is_insert && state.request_edit.focus == RequestFocus::Url {
        let cursor_visual = state.request_edit.url_cursor.saturating_sub(scroll);
        let cursor_x = inner.x + prefix_display_width as u16 + cursor_visual as u16;
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
    match state.request_edit.tab {
        RequestTab::Headers => render_headers_tab(frame, state, content_area, area, is_focused, is_insert, is_editing, is_visual),
        RequestTab::Cookies => render_cookies_tab(frame, state, content_area, area, is_focused, is_insert, is_editing, is_visual),
        RequestTab::Queries => render_queries_tab(frame, state, content_area, area, is_focused, is_insert, is_editing, is_visual),
        RequestTab::Params => render_path_params_tab(frame, state, content_area, area, is_focused, is_insert, is_editing, is_visual),
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

        let is_active = state.request_edit.tab == *tab;
        if is_active {
            spans.push(Span::styled(
                format!("[{}]", tab.label()),
                Style::default()
                    .fg(if is_focused { t.accent } else { t.text })
                    .bg(t.bg_highlight)
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
    is_editing: bool,
    is_visual: bool,
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

        let is_header_focused = is_focused && state.request_edit.focus == RequestFocus::Header(i);
        let prefix = if is_header_focused { "▸" } else { " " };
        let style = if header.enabled {
            Style::default().fg(t.text)
        } else {
            Style::default().fg(t.text_dim)
        };
        let toggle = if header.enabled { "●" } else { "○" };

        let toggle_style = if header.enabled {
            Style::default().fg(t.status_ok)
        } else {
            Style::default().fg(t.text_dim)
        };

        let row_y = content_area.y + y_offset;
        let prefix_span = format!(" {} {} ", prefix, toggle);
        let prefix_part = format!(" {} ", prefix);
        let toggle_part = format!("{} ", toggle);
        let prefix_width = UnicodeWidthStr::width(prefix_span.as_str());

        if is_header_focused && (is_editing || is_visual) {
            let name_style = if state.request_edit.header_edit_field == 0 {
                Style::default().fg(t.accent).add_modifier(Modifier::UNDERLINED)
            } else {
                style.add_modifier(Modifier::BOLD)
            };
            let value_style = if state.request_edit.header_edit_field == 1 {
                Style::default().fg(t.accent).add_modifier(Modifier::UNDERLINED)
            } else {
                style
            };

            let active_style = if state.request_edit.header_edit_field == 0 { name_style } else { value_style };

            // Compute available width and hscroll for the active field
            let separator_width = 2usize; // ": "
            let name_display_width = UnicodeWidthStr::width(header.name.as_str());
            let (name_field_scroll, visible_name) = if state.request_edit.header_edit_field == 0 {
                let avail = (content_area.width as usize).saturating_sub(prefix_width + separator_width + 1);
                field_hscroll(&header.name, state.request_edit.header_edit_cursor, avail)
            } else {
                (0, header.name.clone())
            };
            let (value_field_scroll, visible_value) = if state.request_edit.header_edit_field == 1 {
                let avail = (content_area.width as usize).saturating_sub(prefix_width + name_display_width + separator_width);
                field_hscroll(&header.value, state.request_edit.header_edit_cursor, avail)
            } else {
                (0, header.value.clone())
            };

            let adj_cursor = state.request_edit.header_edit_cursor.saturating_sub(
                if state.request_edit.header_edit_field == 0 { name_field_scroll } else { value_field_scroll }
            );
            let adj_anchor = state.request_edit.visual_anchor.saturating_sub(
                if state.request_edit.header_edit_field == 0 { name_field_scroll } else { value_field_scroll }
            );

            // Build spans — use block cursor / visual highlight for non-insert modes
            let (name_spans, value_spans) = if !is_insert && state.request_edit.header_edit_field == 0 {
                (build_field_spans(&visible_name, adj_cursor,
                    if is_visual { Some((adj_anchor, adj_cursor)) } else { None },
                    active_style, t), vec![Span::styled(visible_value, value_style)])
            } else if !is_insert && state.request_edit.header_edit_field == 1 {
                (vec![Span::styled(visible_name, name_style)],
                 build_field_spans(&visible_value, adj_cursor,
                    if is_visual { Some((adj_anchor, adj_cursor)) } else { None },
                    active_style, t))
            } else {
                (vec![Span::styled(visible_name, name_style)], vec![Span::styled(visible_value, value_style)])
            };

            let mut spans = vec![
                Span::styled(&prefix_part, Style::default().fg(t.border_insert)),
                Span::styled(&toggle_part, if header.enabled {
                    Style::default().fg(t.status_ok)
                } else {
                    Style::default().fg(t.border_insert)
                }),
            ];
            spans.extend(name_spans);
            spans.push(Span::styled(": ", style));
            spans.extend(value_spans);
            let line = Line::from(spans);
            frame.render_widget(
                Paragraph::new(line),
                Rect::new(content_area.x, row_y, content_area.width, 1),
            );

            // Position cursor (only blinking line cursor in insert mode)
            if is_insert {
                let field_scroll = if state.request_edit.header_edit_field == 0 { name_field_scroll } else { value_field_scroll };
                let cursor_visual = state.request_edit.header_edit_cursor.saturating_sub(field_scroll);
                let cursor_x = if state.request_edit.header_edit_field == 0 {
                    content_area.x + prefix_width as u16 + cursor_visual as u16
                } else {
                    content_area.x + prefix_width as u16 + name_display_width as u16 + 2 + cursor_visual as u16
                };
                if cursor_x < bounds.right() {
                    frame.set_cursor_position(Position::new(cursor_x, row_y));
                }
            }

            // Save anchor for autocomplete popup
            if state.request_edit.header_edit_field == 0 && state.autocomplete.is_some() {
                autocomplete_anchor = Some((content_area.x + prefix_width as u16, row_y + 1));
            }
        } else {
            let prefix_style = Style::default().fg(
                if is_header_focused { t.accent } else { t.text_dim },
            );
            let line = Line::from(vec![
                Span::styled(&prefix_part, prefix_style),
                Span::styled(&toggle_part, toggle_style),
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

fn render_queries_tab(
    frame: &mut Frame,
    state: &AppState,
    content_area: Rect,
    bounds: Rect,
    is_focused: bool,
    is_insert: bool,
    is_editing: bool,
    is_visual: bool,
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

        let is_param_focused = is_focused && state.request_edit.focus == RequestFocus::Param(i);
        let prefix = if is_param_focused { "▸" } else { " " };
        let style = if param.enabled {
            Style::default().fg(t.text)
        } else {
            Style::default().fg(t.text_dim)
        };
        let toggle = if param.enabled { "●" } else { "○" };

        let toggle_style = if param.enabled {
            Style::default().fg(t.status_ok)
        } else {
            Style::default().fg(t.text_dim)
        };

        let row_y = content_area.y + y_offset;
        let prefix_span = format!(" {} {} ", prefix, toggle);
        let prefix_part = format!(" {} ", prefix);
        let toggle_part = format!("{} ", toggle);
        let prefix_width = UnicodeWidthStr::width(prefix_span.as_str());

        if is_param_focused && (is_editing || is_visual) {
            let key_style = if state.request_edit.param_edit_field == 0 {
                Style::default().fg(t.accent).add_modifier(Modifier::UNDERLINED)
            } else {
                style.add_modifier(Modifier::BOLD)
            };
            let value_style = if state.request_edit.param_edit_field == 1 {
                Style::default().fg(t.accent).add_modifier(Modifier::UNDERLINED)
            } else {
                style
            };

            let active_style = if state.request_edit.param_edit_field == 0 { key_style } else { value_style };
            let separator_width = 3usize; // " = "
            let key_display_width = UnicodeWidthStr::width(param.key.as_str());
            let (key_field_scroll, visible_key) = if state.request_edit.param_edit_field == 0 {
                let avail = (content_area.width as usize).saturating_sub(prefix_width + separator_width + 1);
                field_hscroll(&param.key, state.request_edit.param_edit_cursor, avail)
            } else {
                (0, param.key.clone())
            };
            let (value_field_scroll, visible_value) = if state.request_edit.param_edit_field == 1 {
                let avail = (content_area.width as usize).saturating_sub(prefix_width + key_display_width + separator_width);
                field_hscroll(&param.value, state.request_edit.param_edit_cursor, avail)
            } else {
                (0, param.value.clone())
            };

            let adj_cursor = state.request_edit.param_edit_cursor.saturating_sub(
                if state.request_edit.param_edit_field == 0 { key_field_scroll } else { value_field_scroll }
            );
            let adj_anchor = state.request_edit.visual_anchor.saturating_sub(
                if state.request_edit.param_edit_field == 0 { key_field_scroll } else { value_field_scroll }
            );

            let (key_spans, value_spans) = if !is_insert && state.request_edit.param_edit_field == 0 {
                (build_field_spans(&visible_key, adj_cursor,
                    if is_visual { Some((adj_anchor, adj_cursor)) } else { None },
                    active_style, t), vec![Span::styled(visible_value, value_style)])
            } else if !is_insert && state.request_edit.param_edit_field == 1 {
                (vec![Span::styled(visible_key, key_style)],
                 build_field_spans(&visible_value, adj_cursor,
                    if is_visual { Some((adj_anchor, adj_cursor)) } else { None },
                    active_style, t))
            } else {
                (vec![Span::styled(visible_key, key_style)], vec![Span::styled(visible_value, value_style)])
            };

            let mut spans = vec![
                Span::styled(&prefix_part, Style::default().fg(t.border_insert)),
                Span::styled(&toggle_part, if param.enabled {
                    Style::default().fg(t.status_ok)
                } else {
                    Style::default().fg(t.border_insert)
                }),
            ];
            spans.extend(key_spans);
            spans.push(Span::styled(" = ", style));
            spans.extend(value_spans);
            frame.render_widget(
                Paragraph::new(Line::from(spans)),
                Rect::new(content_area.x, row_y, content_area.width, 1),
            );

            // Position cursor (only in insert mode)
            if is_insert {
                let field_scroll = if state.request_edit.param_edit_field == 0 { key_field_scroll } else { value_field_scroll };
                let cursor_visual = state.request_edit.param_edit_cursor.saturating_sub(field_scroll);
                let cursor_x = if state.request_edit.param_edit_field == 0 {
                    content_area.x + prefix_width as u16 + cursor_visual as u16
                } else {
                    content_area.x + prefix_width as u16 + key_display_width as u16 + 3 + cursor_visual as u16
                };
                if cursor_x < bounds.right() {
                    frame.set_cursor_position(Position::new(cursor_x, row_y));
                }
            }
        } else {
            let prefix_style = Style::default().fg(
                if is_param_focused { t.accent } else { t.text_dim },
            );
            let line = Line::from(vec![
                Span::styled(&prefix_part, prefix_style),
                Span::styled(&toggle_part, toggle_style),
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

fn render_cookies_tab(
    frame: &mut Frame,
    state: &AppState,
    content_area: Rect,
    bounds: Rect,
    is_focused: bool,
    is_insert: bool,
    is_editing: bool,
    is_visual: bool,
) {
    let t = &state.theme;
    let req = &state.current_request;
    let mut y_offset: u16 = 0;

    if req.cookies.is_empty() {
        if content_area.height > 0 {
            let hint = Line::from(Span::styled(
                "   (none) 'a' to add a cookie",
                Style::default().fg(t.text_dim),
            ));
            frame.render_widget(
                Paragraph::new(hint),
                Rect::new(content_area.x, content_area.y, content_area.width, 1),
            );
        }
        return;
    }

    for (i, cookie) in req.cookies.iter().enumerate() {
        if y_offset >= content_area.height {
            break;
        }

        let is_cookie_focused = is_focused && state.request_edit.focus == RequestFocus::Cookie(i);
        let prefix = if is_cookie_focused { "▸" } else { " " };
        let style = if cookie.enabled {
            Style::default().fg(t.text)
        } else {
            Style::default().fg(t.text_dim)
        };
        let toggle = if cookie.enabled { "●" } else { "○" };

        let toggle_style = if cookie.enabled {
            Style::default().fg(t.status_ok)
        } else {
            Style::default().fg(t.text_dim)
        };

        let row_y = content_area.y + y_offset;
        let prefix_span = format!(" {} {} ", prefix, toggle);
        let prefix_part = format!(" {} ", prefix);
        let toggle_part = format!("{} ", toggle);
        let prefix_width = UnicodeWidthStr::width(prefix_span.as_str());

        if is_cookie_focused && (is_editing || is_visual) {
            let name_style = if state.request_edit.cookie_edit_field == 0 {
                Style::default().fg(t.accent).add_modifier(Modifier::UNDERLINED)
            } else {
                style.add_modifier(Modifier::BOLD)
            };
            let value_style = if state.request_edit.cookie_edit_field == 1 {
                Style::default().fg(t.accent).add_modifier(Modifier::UNDERLINED)
            } else {
                style
            };

            let active_style = if state.request_edit.cookie_edit_field == 0 { name_style } else { value_style };
            let separator_width = 1usize; // "="
            let name_display_width = UnicodeWidthStr::width(cookie.name.as_str());
            let (name_field_scroll, visible_name) = if state.request_edit.cookie_edit_field == 0 {
                let avail = (content_area.width as usize).saturating_sub(prefix_width + separator_width + 1);
                field_hscroll(&cookie.name, state.request_edit.cookie_edit_cursor, avail)
            } else {
                (0, cookie.name.clone())
            };
            let (value_field_scroll, visible_value) = if state.request_edit.cookie_edit_field == 1 {
                let avail = (content_area.width as usize).saturating_sub(prefix_width + name_display_width + separator_width);
                field_hscroll(&cookie.value, state.request_edit.cookie_edit_cursor, avail)
            } else {
                (0, cookie.value.clone())
            };

            let adj_cursor = state.request_edit.cookie_edit_cursor.saturating_sub(
                if state.request_edit.cookie_edit_field == 0 { name_field_scroll } else { value_field_scroll }
            );
            let adj_anchor = state.request_edit.visual_anchor.saturating_sub(
                if state.request_edit.cookie_edit_field == 0 { name_field_scroll } else { value_field_scroll }
            );

            let (name_spans, value_spans) = if !is_insert && state.request_edit.cookie_edit_field == 0 {
                (build_field_spans(&visible_name, adj_cursor,
                    if is_visual { Some((adj_anchor, adj_cursor)) } else { None },
                    active_style, t), vec![Span::styled(visible_value, value_style)])
            } else if !is_insert && state.request_edit.cookie_edit_field == 1 {
                (vec![Span::styled(visible_name, name_style)],
                 build_field_spans(&visible_value, adj_cursor,
                    if is_visual { Some((adj_anchor, adj_cursor)) } else { None },
                    active_style, t))
            } else {
                (vec![Span::styled(visible_name, name_style)], vec![Span::styled(visible_value, value_style)])
            };

            let mut spans = vec![
                Span::styled(&prefix_part, Style::default().fg(t.border_insert)),
                Span::styled(&toggle_part, if cookie.enabled {
                    Style::default().fg(t.status_ok)
                } else {
                    Style::default().fg(t.border_insert)
                }),
            ];
            spans.extend(name_spans);
            spans.push(Span::styled("=", style));
            spans.extend(value_spans);
            frame.render_widget(
                Paragraph::new(Line::from(spans)),
                Rect::new(content_area.x, row_y, content_area.width, 1),
            );

            // Position cursor (only in insert mode)
            if is_insert {
                let field_scroll = if state.request_edit.cookie_edit_field == 0 { name_field_scroll } else { value_field_scroll };
                let cursor_visual = state.request_edit.cookie_edit_cursor.saturating_sub(field_scroll);
                let cursor_x = if state.request_edit.cookie_edit_field == 0 {
                    content_area.x + prefix_width as u16 + cursor_visual as u16
                } else {
                    content_area.x + prefix_width as u16 + name_display_width as u16 + 1 + cursor_visual as u16
                };
                if cursor_x < bounds.right() {
                    frame.set_cursor_position(Position::new(cursor_x, row_y));
                }
            }
        } else {
            let prefix_style = Style::default().fg(
                if is_cookie_focused { t.accent } else { t.text_dim },
            );
            let line = Line::from(vec![
                Span::styled(&prefix_part, prefix_style),
                Span::styled(&toggle_part, toggle_style),
                Span::styled(&cookie.name, style.add_modifier(Modifier::BOLD)),
                Span::styled("=", style),
                Span::styled(&cookie.value, style),
            ]);
            frame.render_widget(
                Paragraph::new(line),
                Rect::new(content_area.x, row_y, content_area.width, 1),
            );
        }

        y_offset += 1;
    }
}


fn render_path_params_tab(
    frame: &mut Frame,
    state: &AppState,
    content_area: Rect,
    bounds: Rect,
    is_focused: bool,
    is_insert: bool,
    is_editing: bool,
    is_visual: bool,
) {
    let t = &state.theme;
    let req = &state.current_request;
    let mut y_offset: u16 = 0;

    if req.path_params.is_empty() {
        if content_area.height > 0 {
            let hint = Line::from(Span::styled(
                "   (none) 'a' to add a path parameter",
                Style::default().fg(t.text_dim),
            ));
            frame.render_widget(
                Paragraph::new(hint),
                Rect::new(content_area.x, content_area.y, content_area.width, 1),
            );
        }
        return;
    }

    for (i, param) in req.path_params.iter().enumerate() {
        if y_offset >= content_area.height {
            break;
        }

        let is_param_focused = is_focused && state.request_edit.focus == RequestFocus::PathParam(i);
        let prefix = if is_param_focused { "▸" } else { " " };
        let style = if param.enabled {
            Style::default().fg(t.text)
        } else {
            Style::default().fg(t.text_dim)
        };
        let toggle = if param.enabled { "●" } else { "○" };

        let toggle_style = if param.enabled {
            Style::default().fg(t.status_ok)
        } else {
            Style::default().fg(t.text_dim)
        };

        let row_y = content_area.y + y_offset;
        let prefix_span = format!(" {} {} ", prefix, toggle);
        let prefix_part = format!(" {} ", prefix);
        let toggle_part = format!("{} ", toggle);
        let prefix_width = UnicodeWidthStr::width(prefix_span.as_str());

        if is_param_focused && (is_editing || is_visual) {
            let key_style = if state.request_edit.path_param_edit_field == 0 {
                Style::default().fg(t.accent).add_modifier(Modifier::UNDERLINED)
            } else {
                style.add_modifier(Modifier::BOLD)
            };
            let value_style = if state.request_edit.path_param_edit_field == 1 {
                Style::default().fg(t.accent).add_modifier(Modifier::UNDERLINED)
            } else {
                style
            };

            let active_style = if state.request_edit.path_param_edit_field == 0 { key_style } else { value_style };
            let separator_width = 3usize; // " = "
            let key_display_width = UnicodeWidthStr::width(param.key.as_str());
            let (key_field_scroll, visible_key) = if state.request_edit.path_param_edit_field == 0 {
                let avail = (content_area.width as usize).saturating_sub(prefix_width + separator_width + 1);
                field_hscroll(&param.key, state.request_edit.path_param_edit_cursor, avail)
            } else {
                (0, param.key.clone())
            };
            let (value_field_scroll, visible_value) = if state.request_edit.path_param_edit_field == 1 {
                let avail = (content_area.width as usize).saturating_sub(prefix_width + key_display_width + separator_width);
                field_hscroll(&param.value, state.request_edit.path_param_edit_cursor, avail)
            } else {
                (0, param.value.clone())
            };

            let adj_cursor = state.request_edit.path_param_edit_cursor.saturating_sub(
                if state.request_edit.path_param_edit_field == 0 { key_field_scroll } else { value_field_scroll }
            );
            let adj_anchor = state.request_edit.visual_anchor.saturating_sub(
                if state.request_edit.path_param_edit_field == 0 { key_field_scroll } else { value_field_scroll }
            );

            let (key_spans, value_spans) = if !is_insert && state.request_edit.path_param_edit_field == 0 {
                (build_field_spans(&visible_key, adj_cursor,
                    if is_visual { Some((adj_anchor, adj_cursor)) } else { None },
                    active_style, t), vec![Span::styled(visible_value, value_style)])
            } else if !is_insert && state.request_edit.path_param_edit_field == 1 {
                (vec![Span::styled(visible_key, key_style)],
                 build_field_spans(&visible_value, adj_cursor,
                    if is_visual { Some((adj_anchor, adj_cursor)) } else { None },
                    active_style, t))
            } else {
                (vec![Span::styled(visible_key, key_style)], vec![Span::styled(visible_value, value_style)])
            };

            let mut spans = vec![
                Span::styled(&prefix_part, Style::default().fg(t.border_insert)),
                Span::styled(&toggle_part, if param.enabled {
                    Style::default().fg(t.status_ok)
                } else {
                    Style::default().fg(t.border_insert)
                }),
            ];
            spans.extend(key_spans);
            spans.push(Span::styled(" = ", style));
            spans.extend(value_spans);
            frame.render_widget(
                Paragraph::new(Line::from(spans)),
                Rect::new(content_area.x, row_y, content_area.width, 1),
            );

            // Position cursor (only in insert mode)
            if is_insert {
                let field_scroll = if state.request_edit.path_param_edit_field == 0 { key_field_scroll } else { value_field_scroll };
                let cursor_visual = state.request_edit.path_param_edit_cursor.saturating_sub(field_scroll);
                let cursor_x = if state.request_edit.path_param_edit_field == 0 {
                    content_area.x + prefix_width as u16 + cursor_visual as u16
                } else {
                    content_area.x + prefix_width as u16 + key_display_width as u16 + 3 + cursor_visual as u16
                };
                if cursor_x < bounds.right() {
                    frame.set_cursor_position(Position::new(cursor_x, row_y));
                }
            }
        } else {
            let prefix_style = Style::default().fg(
                if is_param_focused { t.accent } else { t.text_dim },
            );
            let line = Line::from(vec![
                Span::styled(&prefix_part, prefix_style),
                Span::styled(&toggle_part, toggle_style),
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

fn render_autocomplete_popup(
    frame: &mut Frame,
    ac: &crate::core::state::Autocomplete,
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

/// Compute horizontal scroll offset for a field based on cursor position and available width.
/// Returns (scroll_offset, visible_text).
fn field_hscroll(text: &str, cursor: usize, available_width: usize) -> (usize, String) {
    if available_width == 0 {
        return (0, String::new());
    }
    let scroll = if cursor >= available_width {
        cursor - available_width + 1
    } else {
        0
    };
    let visible: String = if text.len() > scroll {
        text[scroll..].chars().take(available_width).collect()
    } else {
        String::new()
    };
    (scroll, visible)
}

/// Build a display URL that includes enabled query params appended to base URL.
/// When not in insert mode on the URL, this gives a preview of the final URL.
fn colorize_url<'a>(url: &'a str, t: &crate::ui::theme::Theme) -> Vec<Span<'a>> {
    if let Some(q_pos) = url.find('?') {
        let base = &url[..q_pos];
        let query = &url[q_pos..];
        let mut spans = vec![Span::styled(base, Style::default().fg(t.text))];
        // Colorize each part of the query string
        for (i, segment) in query.split_inclusive(&['?', '&'][..]).enumerate() {
            if i == 0 && segment.starts_with('?') {
                spans.push(Span::styled("?", Style::default().fg(t.text_dim)));
                let rest = &segment[1..];
                if let Some(eq) = rest.find('=') {
                    spans.push(Span::styled(&rest[..eq], Style::default().fg(t.json_key)));
                    spans.push(Span::styled("=", Style::default().fg(t.text_dim)));
                    spans.push(Span::styled(&rest[eq+1..], Style::default().fg(t.json_string)));
                } else {
                    spans.push(Span::styled(rest, Style::default().fg(t.json_key)));
                }
            } else if segment.ends_with('&') || segment.ends_with('?') {
                let sep = &segment[segment.len()-1..];
                let part = &segment[..segment.len()-1];
                if let Some(eq) = part.find('=') {
                    spans.push(Span::styled(&part[..eq], Style::default().fg(t.json_key)));
                    spans.push(Span::styled("=", Style::default().fg(t.text_dim)));
                    spans.push(Span::styled(&part[eq+1..], Style::default().fg(t.json_string)));
                } else {
                    spans.push(Span::styled(part, Style::default().fg(t.json_key)));
                }
                spans.push(Span::styled(sep, Style::default().fg(t.text_dim)));
            } else {
                if let Some(eq) = segment.find('=') {
                    spans.push(Span::styled(&segment[..eq], Style::default().fg(t.json_key)));
                    spans.push(Span::styled("=", Style::default().fg(t.text_dim)));
                    spans.push(Span::styled(&segment[eq+1..], Style::default().fg(t.json_string)));
                } else {
                    spans.push(Span::styled(segment, Style::default().fg(t.json_key)));
                }
            }
        }
        spans
    } else {
        vec![Span::styled(url, Style::default().fg(t.text))]
    }
}

fn build_display_url(base_url: &str, params: &[crate::model::request::QueryParam]) -> String {
    let enabled: Vec<_> = params.iter().filter(|p| p.enabled && !p.key.is_empty()).collect();
    if enabled.is_empty() {
        return base_url.to_string();
    }
    let qs: Vec<String> = enabled.iter().map(|p| {
        if p.value.is_empty() {
            p.key.clone()
        } else {
            format!("{}={}", p.key, p.value)
        }
    }).collect();
    // If URL already has a ?, append with &; otherwise use ?
    if base_url.contains('?') {
        format!("{}&{}", base_url, qs.join("&"))
    } else {
        format!("{}?{}", base_url, qs.join("&"))
    }
}

/// Build spans for a field with block cursor and optional visual selection.
/// `cursor_pos` is the character position for the block cursor.
/// `visual_range` is Some((anchor, cursor)) for visual mode selection.
fn build_field_spans<'a>(
    text: &'a str,
    cursor_pos: usize,
    visual_range: Option<(usize, usize)>,
    base_style: Style,
    t: &crate::ui::theme::Theme,
) -> Vec<Span<'a>> {
    if text.is_empty() {
        // Show block cursor on empty field
        return vec![Span::styled(" ", Style::default().fg(t.overlay_bg).bg(t.text))];
    }

    let chars: Vec<char> = text.chars().collect();
    let cursor = cursor_pos.min(chars.len().saturating_sub(1));

    if let Some((anchor, _)) = visual_range {
        // Visual mode: highlight selection range
        let sel_start = anchor.min(cursor);
        let sel_end = (anchor.max(cursor) + 1).min(chars.len());
        let visual_style = Style::default().fg(t.overlay_bg).bg(t.accent);

        let mut spans = Vec::new();
        if sel_start > 0 {
            let before: String = chars[..sel_start].iter().collect();
            spans.push(Span::styled(before, base_style));
        }
        let selected: String = chars[sel_start..sel_end].iter().collect();
        spans.push(Span::styled(selected, visual_style));
        if sel_end < chars.len() {
            let after: String = chars[sel_end..].iter().collect();
            spans.push(Span::styled(after, base_style));
        }
        spans
    } else {
        // Normal mode: block cursor on current char
        let cursor_style = Style::default().fg(t.overlay_bg).bg(t.text);

        let mut spans = Vec::new();
        if cursor > 0 {
            let before: String = chars[..cursor].iter().collect();
            spans.push(Span::styled(before, base_style));
        }
        let cursor_char: String = chars[cursor..cursor + 1].iter().collect();
        spans.push(Span::styled(cursor_char, cursor_style));
        if cursor + 1 < chars.len() {
            let after: String = chars[cursor + 1..].iter().collect();
            spans.push(Span::styled(after, base_style));
        }
        spans
    }
}
