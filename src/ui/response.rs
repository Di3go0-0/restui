use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

use crate::model::response::StatusCategory;
use crate::state::{AppState, InputMode, Panel, ResponseTab};
use crate::ui::body::find_matching_bracket;

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
            .scroll(state.resp_buf.scroll);
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

    let current_tab = state.response_tab;
    if current_tab != ResponseTab::Type {
        render_body_tab(frame, state, resp, inner, is_focused, is_visual, is_visual_block);
    } else {
        render_type_tab(frame, state, resp, inner, is_focused);
    }
}

fn render_response_tab_bar(state: &AppState, is_focused: bool) -> Line<'static> {
    let t = &state.theme;
    let tabs = [ResponseTab::Body, ResponseTab::Type];
    let mut spans: Vec<Span<'static>> = Vec::new();
    spans.push(Span::raw(" "));

    for (i, tab) in tabs.iter().enumerate() {
        let label = match tab {
            ResponseTab::Body => "Body",
            ResponseTab::Type => "Type",
        };
        let is_active = *tab == state.response_tab;

        if is_active {
            spans.push(Span::styled(
                format!("[{}]", label),
                Style::default()
                    .fg(if is_focused { t.accent } else { t.text })
                    .add_modifier(Modifier::BOLD),
            ));
        } else {
            spans.push(Span::styled(
                label.to_string(),
                Style::default().fg(t.text_dim),
            ));
        }

        if i < tabs.len() - 1 {
            spans.push(Span::raw("  "));
        }
    }

    spans.push(Span::styled("  {/}", Style::default().fg(t.text_dim)));

    Line::from(spans)
}

fn build_status_line(resp: &crate::model::response::Response, t: &crate::theme::Theme) -> Line<'static> {
    let status_color = match resp.status_category() {
        StatusCategory::Success => Color::Green,
        StatusCategory::Redirect => Color::Cyan,
        StatusCategory::ClientError => Color::Yellow,
        StatusCategory::ServerError => Color::Red,
        StatusCategory::Unknown => Color::DarkGray,
    };

    Line::from(vec![
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
            resp.content_type.as_deref().unwrap_or("").to_string(),
            Style::default().fg(t.text_dim),
        ),
    ])
}

fn build_request_info(state: &AppState) -> Vec<Line<'static>> {
    let t = &state.theme;
    let req = &state.current_request;
    let mut lines: Vec<Line<'static>> = Vec::new();

    // Sent headers
    let enabled_headers: Vec<_> = req.headers.iter().filter(|h| h.enabled).collect();
    if !enabled_headers.is_empty() {
        lines.push(Line::from(Span::styled("  Headers:", Style::default().fg(t.gutter_active))));
        for h in enabled_headers {
            lines.push(Line::from(vec![
                Span::styled(format!("    {}", h.name), Style::default().fg(t.json_key)),
                Span::styled(": ", Style::default().fg(t.text_dim)),
                Span::styled(h.value.clone(), Style::default().fg(t.text)),
            ]));
        }
    }

    // Sent query params
    let enabled_params: Vec<_> = req.query_params.iter().filter(|p| p.enabled).collect();
    if !enabled_params.is_empty() {
        lines.push(Line::from(Span::styled("  Queries:", Style::default().fg(t.gutter_active))));
        for p in enabled_params {
            lines.push(Line::from(vec![
                Span::styled(format!("    {}", p.key), Style::default().fg(t.json_key)),
                Span::styled(" = ", Style::default().fg(t.text_dim)),
                Span::styled(p.value.clone(), Style::default().fg(t.text)),
            ]));
        }
    }

    // Sent path params
    let enabled_path: Vec<_> = req.path_params.iter().filter(|p| p.enabled).collect();
    if !enabled_path.is_empty() {
        lines.push(Line::from(Span::styled("  Params:", Style::default().fg(t.gutter_active))));
        for p in enabled_path {
            lines.push(Line::from(vec![
                Span::styled(format!("    {}", p.key), Style::default().fg(t.json_key)),
                Span::styled(" = ", Style::default().fg(t.text_dim)),
                Span::styled(p.value.clone(), Style::default().fg(t.text)),
            ]));
        }
    }

    // Sent body (preview - first few lines)
    let body = req.get_body_opt(state.body_type);
    if let Some(body_text) = body {
        let trimmed = body_text.trim();
        if !trimmed.is_empty() {
            lines.push(Line::from(vec![
                Span::styled(format!("  Body [{}]:", state.body_type.label()), Style::default().fg(t.gutter_active)),
            ]));
            for (i, line) in trimmed.lines().take(5).enumerate() {
                lines.push(Line::from(Span::styled(
                    format!("    {}", line),
                    Style::default().fg(t.text_dim),
                )));
                if i == 4 && trimmed.lines().count() > 5 {
                    lines.push(Line::from(Span::styled(
                        "    ...".to_string(),
                        Style::default().fg(t.text_dim),
                    )));
                }
            }
        }
    }

    // Sent cookies
    let enabled_cookies: Vec<_> = req.cookies.iter().filter(|c| c.enabled).collect();
    if !enabled_cookies.is_empty() {
        lines.push(Line::from(Span::styled("  Cookies:", Style::default().fg(t.gutter_active))));
        for c in enabled_cookies {
            lines.push(Line::from(vec![
                Span::styled(format!("    {}", c.name), Style::default().fg(t.json_key)),
                Span::styled("=", Style::default().fg(t.text_dim)),
                Span::styled(c.value.clone(), Style::default().fg(t.text)),
            ]));
        }
    }

    lines
}

#[allow(clippy::too_many_arguments)]
fn render_body_tab(
    frame: &mut Frame,
    state: &AppState,
    resp: &crate::model::response::Response,
    inner: Rect,
    is_focused: bool,
    is_visual: bool,
    is_visual_block: bool,
) {
    let status_line = build_status_line(resp, &state.theme);
    let tab_bar = render_response_tab_bar(state, is_focused);
    let t = &state.theme;

    // Build request info lines (shown when headers expanded)
    let request_info_lines = build_request_info(state);

    // Calculate headers area height
    let headers_height: u16 = if state.response_headers_expanded {
        let max_h = (inner.height.saturating_sub(5)) / 2;
        let total = resp.headers.len() + if request_info_lines.is_empty() { 0 } else { request_info_lines.len() + 1 }; // +1 for separator
        (total as u16).min(max_h).max(1)
    } else {
        1
    };

    let has_search_bar = state.search.active
        || (is_focused && !state.search.query.is_empty() && !state.search.matches.is_empty());
    let search_bar_height: u16 = if has_search_bar { 1 } else { 0 };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),              // Status line
            Constraint::Length(1),              // Tab bar
            Constraint::Length(headers_height), // Headers
            Constraint::Length(1),              // Separator
            Constraint::Min(1),                // Body with line numbers
            Constraint::Length(search_bar_height), // Search bar
        ])
        .split(inner);

    frame.render_widget(Paragraph::new(status_line), chunks[0]);
    frame.render_widget(Paragraph::new(tab_bar), chunks[1]);

    // Headers area
    if state.response_headers_expanded {
        let header_scroll = state.response_headers_scroll;
        let visible = headers_height as usize;
        // Build combined lines: response headers + separator + request info
        let mut all_lines: Vec<Line<'static>> = Vec::new();
        for (name, value) in &resp.headers {
            all_lines.push(Line::from(vec![
                Span::styled(
                    format!("  {}", name),
                    Style::default().fg(t.accent).add_modifier(Modifier::BOLD),
                ),
                Span::styled(": ", Style::default().fg(t.text_dim)),
                Span::styled(value.to_string(), Style::default().fg(t.text)),
            ]));
        }
        if !request_info_lines.is_empty() {
            all_lines.push(Line::from(Span::styled(
                format!("  ── Sent Request ──"),
                Style::default().fg(t.gutter_active).add_modifier(Modifier::BOLD),
            )));
            all_lines.extend(request_info_lines);
        }
        for vi in 0..visible {
            let idx = header_scroll + vi;
            if idx >= all_lines.len() { break; }
            let y = chunks[2].y + vi as u16;
            let line_area = Rect::new(chunks[2].x, y, chunks[2].width, 1);
            frame.render_widget(Paragraph::new(all_lines[idx].clone()), line_area);
        }
    } else {
        let toggle_hint = if is_focused { " (H to expand)" } else { "" };
        let headers_info = Line::from(Span::styled(
            format!(" {} response headers{}", resp.headers.len(), toggle_hint),
            Style::default().fg(Color::DarkGray),
        ));
        frame.render_widget(Paragraph::new(headers_info), chunks[2]);
    }

    // Separator
    let sep = Line::from(Span::styled(
        "─".repeat(inner.width as usize),
        Style::default().fg(Color::DarkGray),
    ));
    frame.render_widget(Paragraph::new(sep), chunks[3]);

    // Body area with line numbers
    let body_area = chunks[4];
    render_response_body(frame, state, resp, body_area, inner, is_focused, is_visual, is_visual_block);

    // Search bar
    if has_search_bar {
        let search_area = chunks[5];
        let match_info = if state.search.matches.is_empty() {
            "No matches".to_string()
        } else {
            format!("{}/{}", state.search.match_idx + 1, state.search.matches.len())
        };
        let search_line = Line::from(vec![
            Span::styled("/", Style::default().fg(t.accent).add_modifier(Modifier::BOLD)),
            Span::styled(state.search.query.clone(), Style::default().fg(t.text)),
            if state.search.active {
                Span::styled("█", Style::default().fg(t.accent))
            } else {
                Span::raw("")
            },
            Span::styled(format!("  {}", match_info), Style::default().fg(t.text_dim)),
        ]);
        frame.render_widget(Paragraph::new(search_line), search_area);
    }
}

fn render_type_tab(
    frame: &mut Frame,
    state: &AppState,
    resp: &crate::model::response::Response,
    inner: Rect,
    is_focused: bool,
) {
    let status_line = build_status_line(resp, &state.theme);
    let tab_bar = render_response_tab_bar(state, is_focused);

    let errors = &state.type_validation_errors;
    let validation_height: u16 = if errors.is_empty() { 0 } else { (errors.len() as u16 + 1).min(5) };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),                    // Status line
            Constraint::Length(1),                    // Tab bar
            Constraint::Length(1),                    // Separator
            Constraint::Percentage(50),               // Type editor
            Constraint::Length(validation_height),    // Validation warnings
            Constraint::Length(1),                    // Separator
            Constraint::Min(1),                       // Response body preview
        ])
        .split(inner);

    frame.render_widget(Paragraph::new(status_line), chunks[0]);
    frame.render_widget(Paragraph::new(tab_bar), chunks[1]);

    // Separator
    let sep_str = "─".repeat(inner.width as usize);
    let sep1 = Line::from(Span::styled(
        sep_str.clone(),
        Style::default().fg(Color::DarkGray),
    ));
    frame.render_widget(Paragraph::new(sep1), chunks[2]);

    // Type editor area — cursor only when sub-focus is Editor
    let editor_focused = is_focused && state.type_sub_focus == crate::state::TypeSubFocus::Editor;
    render_type_editor(frame, state, chunks[3], editor_focused);

    // Sub-focus indicator
    if is_focused {
        let indicator = if state.type_sub_focus == crate::state::TypeSubFocus::Editor {
            "▸ Type (Ctrl+J → preview)"
        } else {
            "  Type"
        };
        let ind_area = Rect::new(chunks[2].x, chunks[2].y, indicator.len() as u16, 1);
        frame.render_widget(Paragraph::new(Span::styled(indicator, Style::default().fg(Color::Cyan))), ind_area);
    }

    // Validation warnings
    if !errors.is_empty() {
        let val_area = chunks[4];
        let mut val_lines: Vec<Line<'static>> = Vec::new();
        val_lines.push(Line::from(Span::styled(
            format!(" Validation ({} issues):", errors.len()),
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
        )));
        for err in errors.iter().take(4) {
            val_lines.push(Line::from(Span::styled(
                format!("  * {}", err),
                Style::default().fg(Color::Red),
            )));
        }
        let val_paragraph = Paragraph::new(val_lines);
        frame.render_widget(val_paragraph, val_area);
    }

    // Separator
    let sep2 = Line::from(Span::styled(
        sep_str,
        Style::default().fg(Color::DarkGray),
    ));
    frame.render_widget(Paragraph::new(sep2), chunks[5]);

    // Response body preview — cursor when sub-focus is Preview
    let preview_focused = is_focused && state.type_sub_focus == crate::state::TypeSubFocus::Preview;
    let preview_visual = preview_focused && state.mode == InputMode::Visual;
    let preview_visual_block = preview_focused && state.mode == InputMode::VisualBlock;
    let preview_area = chunks[6];

    // Sub-focus indicator for preview
    if is_focused {
        let indicator = if state.type_sub_focus == crate::state::TypeSubFocus::Preview {
            "▸ Response (Ctrl+K → type)"
        } else {
            "  Response"
        };
        let ind_area = Rect::new(chunks[5].x, chunks[5].y, indicator.len() as u16, 1);
        frame.render_widget(Paragraph::new(Span::styled(indicator, Style::default().fg(Color::Cyan))), ind_area);
    }

    render_response_body(frame, state, resp, preview_area, inner, preview_focused, preview_visual, preview_visual_block);
}

fn render_type_editor(
    frame: &mut Frame,
    state: &AppState,
    type_area: Rect,
    is_focused: bool,
) {
    let t = &state.theme;
    let is_insert = is_focused && state.mode == InputMode::Insert && state.response_tab == ResponseTab::Type;
    let text = &state.response_type_text;

    if text.is_empty() && state.response_type.is_none() {
        let placeholder = Paragraph::new(Line::from(Span::styled(
            " (no type - execute a request to see the response type)".to_string(),
            Style::default().fg(Color::DarkGray),
        )));
        frame.render_widget(placeholder, type_area);
        return;
    }

    let text_lines: Vec<&str> = if text.is_empty() { vec![""] } else { text.lines().collect() };
    let total_lines = text_lines.len();
    let visible_height = type_area.height as usize;
    let scroll = (state.type_buf.scroll.0 as usize).min(total_lines.saturating_sub(visible_height));

    let gutter_width: u16 = 4;
    let text_area_x = type_area.x + gutter_width;
    let text_area_width = type_area.width.saturating_sub(gutter_width);

    let locked_indicator = if state.response_type_locked { " [locked]" } else { "" };
    let _ = locked_indicator; // used below in hint line if needed

    for vi in 0..visible_height {
        let line_idx = scroll + vi;
        if line_idx >= total_lines { break; }

        let y = type_area.y + vi as u16;

        // Gutter (line number)
        let is_cursor_line = is_focused && line_idx == state.type_buf.cursor_row;
        let gutter_style = if is_cursor_line {
            Style::default().fg(t.gutter_active)
        } else {
            Style::default().fg(t.gutter)
        };
        let line_num = format!("{:>3} ", line_idx + 1);
        let gutter_area = Rect::new(type_area.x, y, gutter_width, 1);
        frame.render_widget(Paragraph::new(Span::styled(line_num, gutter_style)), gutter_area);

        // Text content
        let line_text = text_lines.get(line_idx).copied().unwrap_or("");
        let line_area = Rect::new(text_area_x, y, text_area_width, 1);

        // Colorize the line
        let colored_line = colorize_type_line(line_text, t);
        frame.render_widget(Paragraph::new(colored_line), line_area);

        // Cursor rendering
        if is_focused && line_idx == state.type_buf.cursor_row {
            let col = state.type_buf.cursor_col;
            if col < text_area_width as usize {
                let cursor_x = text_area_x + col as u16;
                if is_insert {
                    // Insert mode: thin bar cursor via terminal
                    frame.set_cursor_position(Position::new(cursor_x, y));
                } else {
                    // Normal mode: block cursor highlight
                    let ch = line_text.chars().nth(col).unwrap_or(' ');
                    let cursor_area = Rect::new(cursor_x, y, 1, 1);
                    frame.render_widget(
                        Paragraph::new(Span::styled(
                            ch.to_string(),
                            Style::default().fg(Color::Black).bg(t.text),
                        )),
                        cursor_area,
                    );
                }
            }
        }
    }

    // Scrollbar
    if total_lines > visible_height {
        render_scrollbar(frame, type_area, scroll, total_lines, visible_height, t.text_dim);
    }
}

fn colorize_type_line(line: &str, t: &crate::theme::Theme) -> Line<'static> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    let trimmed = line.trim();

    // Type keywords to colorize
    let type_keywords = ["string", "number", "boolean", "null"];

    // Check if the line is a field: "  fieldname: type,"
    if let Some(colon_pos) = trimmed.find(": ") {
        let field_name = trimmed[..colon_pos].trim();
        let type_part = trimmed[colon_pos + 2..].trim();

        // Leading whitespace
        let leading_ws = &line[..line.len() - line.trim_start().len()];
        if !leading_ws.is_empty() {
            spans.push(Span::raw(leading_ws.to_string()));
        }

        // Field name
        spans.push(Span::styled(
            field_name.to_string(),
            Style::default().fg(t.accent),
        ));
        spans.push(Span::styled(": ", Style::default().fg(t.text_dim)));

        // Type value
        let type_no_comma = type_part.trim_end_matches(',');
        let has_comma = type_part.ends_with(',');

        // Check if it's an enum: "val1" | "val2" | ...
        if type_no_comma.contains('|') && type_no_comma.contains('"') {
            colorize_enum_spans(type_no_comma, t, &mut spans);
        } else {
            let type_color = type_keyword_color(type_no_comma, t);
            spans.push(Span::styled(type_no_comma.to_string(), Style::default().fg(type_color)));
        }
        if has_comma {
            spans.push(Span::styled(",", Style::default().fg(t.text_dim)));
        }

        return Line::from(spans);
    }

    // Check for standalone enum line: "val1" | "val2" | ...
    if trimmed.contains('|') && trimmed.contains('"') {
        let leading_ws = &line[..line.len() - line.trim_start().len()];
        if !leading_ws.is_empty() {
            spans.push(Span::raw(leading_ws.to_string()));
        }
        let no_comma = trimmed.trim_end_matches(',');
        colorize_enum_spans(no_comma, t, &mut spans);
        if trimmed.ends_with(',') {
            spans.push(Span::styled(",", Style::default().fg(t.text_dim)));
        }
        return Line::from(spans);
    }

    // Check for bracket lines or pure type lines
    if trimmed == "{" || trimmed == "}" || trimmed.starts_with("}[]") || trimmed.ends_with("},") || trimmed.ends_with("{") {
        let leading_ws = &line[..line.len() - line.trim_start().len()];
        if !leading_ws.is_empty() {
            spans.push(Span::raw(leading_ws.to_string()));
        }
        spans.push(Span::styled(trimmed.to_string(), Style::default().fg(t.text)));
        return Line::from(spans);
    }

    // Pure type keyword line (e.g., top-level "string" or "number[]")
    for kw in &type_keywords {
        let no_comma = trimmed.trim_end_matches(',');
        let arr_kw = format!("{}[]", kw);
        if no_comma == *kw || no_comma == arr_kw {
            let leading_ws = &line[..line.len() - line.trim_start().len()];
            if !leading_ws.is_empty() {
                spans.push(Span::raw(leading_ws.to_string()));
            }
            let color = type_keyword_color(no_comma, t);
            spans.push(Span::styled(no_comma.to_string(), Style::default().fg(color)));
            if trimmed.ends_with(',') {
                spans.push(Span::styled(",", Style::default().fg(t.text_dim)));
            }
            return Line::from(spans);
        }
    }

    // Fallback
    Line::from(Span::styled(line.to_string(), Style::default().fg(t.text)))
}

fn colorize_enum_spans(enum_text: &str, t: &crate::theme::Theme, spans: &mut Vec<Span<'static>>) {
    let parts: Vec<&str> = enum_text.split('|').collect();
    for (i, part) in parts.iter().enumerate() {
        let trimmed_part = part.trim();
        spans.push(Span::styled(
            trimmed_part.to_string(),
            Style::default().fg(t.json_string),
        ));
        if i < parts.len() - 1 {
            spans.push(Span::styled(" | ", Style::default().fg(t.text_dim)));
        }
    }
}

fn type_keyword_color(kw: &str, t: &crate::theme::Theme) -> Color {
    let base = kw.trim_end_matches("[]");
    match base {
        "string" => t.json_string,
        "number" => t.json_number,
        "boolean" => t.json_bool,
        "null" => t.text_dim,
        "array" => t.accent,
        "object" => t.accent,
        _ => t.text,
    }
}

#[allow(clippy::too_many_arguments)]
fn render_response_body(
    frame: &mut Frame,
    state: &AppState,
    resp: &crate::model::response::Response,
    body_area: Rect,
    inner: Rect,
    is_focused: bool,
    is_visual: bool,
    is_visual_block: bool,
) {
    let t = &state.theme;
    let body = resp.formatted_body();
    let body_lines: Vec<&str> = body.lines().collect();
    let total_lines = body_lines.len();

    let gutter_width: u16 = 4;
    let text_area_x = body_area.x + gutter_width;
    let text_area_width = body_area.width.saturating_sub(gutter_width);

    let scroll_y = state.resp_buf.scroll.0 as usize;
    let hscroll = state.resp_buf.scroll.1 as usize;
    let visible_height = body_area.height as usize;
    let cursor_row = state.resp_buf.cursor_row;

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
    let search_query_lower = state.search.query.to_lowercase();
    let has_search = !search_query_lower.is_empty() && !state.search.matches.is_empty()
        && state.active_panel == Panel::Response;

    // Compute bracket match for response panel
    let matched_bracket = if is_focused {
        find_matching_bracket(&body_lines, state.resp_buf.cursor_row, state.resp_buf.cursor_col)
    } else {
        None
    };
    let bracket_style = Style::default()
        .fg(Color::Black)
        .bg(t.accent)
        .add_modifier(Modifier::BOLD);

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

        // Content - apply horizontal scroll
        let full_line_text = body_lines.get(line_idx).copied().unwrap_or("");
        let line_text: String = if full_line_text.len() > hscroll {
            full_line_text[hscroll..].chars().take(text_area_width as usize).collect()
        } else {
            String::new()
        };
        let line_text_ref = line_text.as_str();

        let content_line = if is_visual && line_idx >= vsr && line_idx <= ver {
            let adj_vsc = vsc.saturating_sub(hscroll);
            let adj_vec = vec_.saturating_sub(hscroll);
            highlight_visual_line(line_text_ref, line_idx, vsr, adj_vsc, ver, adj_vec)
        } else if is_visual_block && line_idx >= vb_min_row && line_idx <= vb_max_row {
            let adj_min_col = vb_min_col.saturating_sub(hscroll);
            let adj_max_col = vb_max_col.saturating_sub(hscroll);
            highlight_block_line(line_text_ref, adj_min_col, adj_max_col)
        } else if has_search {
            highlight_search_line(line_text_ref, line_idx, state, &search_query_lower, t,
                is_focused && line_idx == cursor_row, hscroll)
        } else if is_focused && line_idx == cursor_row && !is_visual && !is_visual_block {
            Line::from(Span::styled(
                line_text,
                Style::default().fg(t.text).bg(t.bg_highlight),
            ))
        } else {
            colorize_response_line(line_text_ref, t)
        };

        let content_area = Rect::new(text_area_x, y, text_area_width, 1);
        frame.render_widget(Paragraph::new(content_line), content_area);

        // Bracket highlighting
        if is_focused {
            let highlight_positions: [(usize, usize); 2] = [
                (state.resp_buf.cursor_row, state.resp_buf.cursor_col),
                matched_bracket.unwrap_or((usize::MAX, usize::MAX)),
            ];
            for &(br, bc) in &highlight_positions {
                if br == line_idx && bc >= hscroll {
                    if let Some(ch) = body_lines.get(br).and_then(|l| l.as_bytes().get(bc)) {
                        if matches!(ch, b'{' | b'}' | b'[' | b']' | b'(' | b')') {
                            let bx = text_area_x + (bc - hscroll) as u16;
                            if bx < content_area.right() {
                                let buf = frame.buffer_mut();
                                if bx < buf.area().right() && y < buf.area().bottom() {
                                    buf[(bx, y)].set_style(bracket_style);
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // Scrollbar
    if total_lines > visible_height && text_area_width > 1 {
        let scrollbar_area = Rect::new(text_area_x, body_area.y, text_area_width, body_area.height);
        render_scrollbar(frame, scrollbar_area, scroll_y, total_lines, visible_height, t.text_dim);
    }

    // Cursor in visual mode
    if is_visual || is_visual_block {
        let cursor_screen_row = cursor_row as i32 - scroll_y as i32;
        if cursor_screen_row >= 0 && (cursor_screen_row as u16) < body_area.height {
            let cursor_x = text_area_x + state.resp_buf.cursor_col.saturating_sub(hscroll) as u16;
            let cursor_y = body_area.y + cursor_screen_row as u16;
            if cursor_x < inner.right() {
                frame.set_cursor_position(Position::new(cursor_x, cursor_y));
            }
        }
    }
}

fn highlight_search_line(
    line: &str,
    line_idx: usize,
    state: &AppState,
    query_lower: &str,
    t: &crate::theme::Theme,
    is_cursor_line: bool,
    hscroll: usize,
) -> Line<'static> {
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
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut pos = 0;

    let current_match = state.search.matches.get(state.search.match_idx).copied();

    let match_bg = Color::Yellow;
    let current_match_bg = Color::Rgb(255, 165, 0);
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

            if match_start > pos {
                spans.push(Span::styled(line[pos..match_start].to_string(), base_style));
            }

            let is_current = current_match == Some((line_idx, match_start + hscroll));
            let bg = if is_current { current_match_bg } else { match_bg };

            spans.push(Span::styled(
                line[match_start..match_end].to_string(),
                Style::default().fg(match_fg).bg(bg).add_modifier(Modifier::BOLD),
            ));

            pos = match_end;
        } else {
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
    let (ar, ac) = (state.resp_buf.visual_anchor_row, state.resp_buf.visual_anchor_col);
    let (cr, cc) = (state.resp_buf.cursor_row, state.resp_buf.cursor_col);
    (ar.min(cr), ac.min(cc), ar.max(cr), ac.max(cc))
}

fn highlight_block_line(line: &str, min_col: usize, max_col: usize) -> Line<'static> {
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
    let (ar, ac) = (state.resp_buf.visual_anchor_row, state.resp_buf.visual_anchor_col);
    let (cr, cc) = (state.resp_buf.cursor_row, state.resp_buf.cursor_col);
    if (ar, ac) <= (cr, cc) {
        (ar, ac, cr, cc)
    } else {
        (cr, cc, ar, ac)
    }
}

fn highlight_visual_line(line: &str, row: usize, sr: usize, sc: usize, er: usize, ec: usize) -> Line<'static> {
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

fn colorize_response_line(line: &str, t: &crate::theme::Theme) -> Line<'static> {
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

fn render_scrollbar(frame: &mut Frame, area: Rect, scroll_y: usize, total_lines: usize, visible_height: usize, color: Color) {
    if total_lines <= visible_height || visible_height == 0 {
        return;
    }
    let x = area.right().saturating_sub(1);
    let thumb_size = (visible_height * visible_height / total_lines).max(1);
    let max_scroll = total_lines.saturating_sub(visible_height);
    let track_space = visible_height.saturating_sub(thumb_size);
    let thumb_start = if max_scroll > 0 {
        scroll_y * track_space / max_scroll
    } else {
        0
    };
    let thumb_end = thumb_start + thumb_size;

    for vi in 0..visible_height {
        let y = area.y + vi as u16;
        if x < area.right() {
            let ch = if vi >= thumb_start && vi < thumb_end { "█" } else { "▐" };
            let style = if vi >= thumb_start && vi < thumb_end {
                Style::default().fg(color)
            } else {
                Style::default().fg(color).add_modifier(Modifier::DIM)
            };
            let buf = frame.buffer_mut();
            if x < buf.area().right() && y < buf.area().bottom() {
                buf[(x, y)].set_symbol(ch).set_style(style);
            }
        }
    }
}
