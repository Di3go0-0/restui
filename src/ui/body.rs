use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::state::{AppState, BodyType, InputMode, Panel};

pub fn render(frame: &mut Frame, state: &AppState, area: Rect) {
    let is_focused = state.active_panel == Panel::Body;
    let is_insert = is_focused && state.mode == InputMode::Insert;
    let is_visual = is_focused && state.mode == InputMode::Visual;
    let is_visual_block = is_focused && state.mode == InputMode::VisualBlock;
    let is_normal_focused = is_focused && state.mode == InputMode::Normal;
    let t = &state.theme;
    let border_color = t.border_for_mode(is_focused, state.mode);

    let title = if let Some(ref err) = state.body_validation_error {
        format!(" [3] Body ⚠ {} ", err)
    } else {
        " [3] Body ".to_string()
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

    let outer_inner = block.inner(area);
    frame.render_widget(block, area);

    if outer_inner.width < 4 || outer_inner.height < 2 {
        return;
    }

    // Tab bar + content layout
    let body_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // Tab bar
            Constraint::Min(1),   // Body content
        ])
        .split(outer_inner);

    // Render tab bar
    let tab_bar = render_body_tab_bar(state, is_focused);
    frame.render_widget(Paragraph::new(tab_bar), body_chunks[0]);

    let outer_inner = body_chunks[1];

    // Reserve space for search bar if needed
    let has_search_bar = (state.search.active && state.active_panel == Panel::Body)
        || (is_focused && !state.search.query.is_empty() && !state.search.matches.is_empty()
            && state.active_panel == Panel::Body);
    let search_bar_height: u16 = if has_search_bar { 1 } else { 0 };

    let inner = if search_bar_height > 0 && outer_inner.height > 2 {
        Rect::new(outer_inner.x, outer_inner.y, outer_inner.width,
                   outer_inner.height.saturating_sub(search_bar_height))
    } else {
        outer_inner
    };

    let body_text = state.current_request.get_body(state.body_type);

    if body_text.is_empty() && !is_insert {
        let placeholder = Paragraph::new(Line::from(Span::styled(
            " Press 'i' to start typing, Ctrl+V to paste",
            Style::default().fg(Color::DarkGray),
        )));
        frame.render_widget(placeholder, inner);
        return;
    }

    // Line number gutter width
    let body_lines: Vec<&str> = if body_text.is_empty() {
        vec![""]
    } else {
        let mut lines: Vec<&str> = body_text.lines().collect();
        if body_text.ends_with('\n') {
            lines.push("");
        }
        lines
    };
    let total_lines = body_lines.len();
    let gutter_width: u16 = 4; // "NNN "
    let text_area_x = inner.x + gutter_width;
    let text_area_width = inner.width.saturating_sub(gutter_width);

    let scroll_y = state.body_vim.buffer.scroll.0 as usize;
    let hscroll = state.body_vim.buffer.scroll.1 as usize;
    let visible_height = inner.height as usize;
    let cursor_row = state.body_vim.buffer.cursor_row;

    // Compute bracket match
    let matched_bracket = if is_focused {
        find_matching_bracket(&body_lines, state.body_vim.buffer.cursor_row, state.body_vim.buffer.cursor_col)
    } else {
        None
    };
    let bracket_style = Style::default()
        .fg(Color::Black)
        .bg(t.accent)
        .add_modifier(Modifier::BOLD);

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

        // Content — apply horizontal scroll or wrap
        let full_line_text = body_lines.get(line_idx).copied().unwrap_or("");
        let line_text: String = if state.wrap_enabled {
            // Wrap: show from start, truncate to width (visual wrap line 0)
            full_line_text.chars().take(text_area_width as usize).collect()
        } else if full_line_text.len() > hscroll {
            full_line_text[hscroll..].chars().take(text_area_width as usize).collect()
        } else {
            String::new()
        };
        let line_text_ref = line_text.as_str();

        // Adjust cursor col and visual anchors for hscroll
        let adj_cursor_col = state.body_vim.buffer.cursor_col.saturating_sub(hscroll);

        // Prepare search info
        let search_query_lower = state.search.query.to_lowercase();
        let has_body_search = !search_query_lower.is_empty()
            && !state.search.matches.is_empty()
            && state.active_panel == Panel::Body;

        let content_line = if is_visual {
            let (sr, sc, er, ec) = visual_range(state);
            let adj_sc = sc.saturating_sub(hscroll);
            let adj_ec = ec.saturating_sub(hscroll);
            if line_idx >= sr && line_idx <= er {
                highlight_visual_line(line_text_ref, line_idx, sr, adj_sc, er, adj_ec)
            } else {
                colorize_json_line(line_text_ref, t)
            }
        } else if is_visual_block {
            let (min_row, min_col, max_row, max_col) = visual_block_range(state);
            let adj_min_col = min_col.saturating_sub(hscroll);
            let adj_max_col = max_col.saturating_sub(hscroll);
            if line_idx >= min_row && line_idx <= max_row {
                highlight_block_line(line_text_ref, adj_min_col, adj_max_col)
            } else {
                colorize_json_line(line_text_ref, t)
            }
        } else if has_body_search {
            highlight_body_search_line(line_text_ref, line_idx, state, &search_query_lower, t,
                is_normal_focused && line_idx == cursor_row, hscroll)
        } else {
            // Highlight current line background in normal mode + block cursor
            if is_normal_focused && line_idx == cursor_row {
                render_normal_cursor_line(line_text_ref, adj_cursor_col, t)
            } else {
                colorize_json_line(line_text_ref, t)
            }
        };

        let content_area = Rect::new(text_area_x, y, text_area_width, 1);
        frame.render_widget(Paragraph::new(content_line), content_area);

        // Bracket highlighting (both cursor bracket and matched bracket)
        if is_focused {
            let highlight_positions: [(usize, usize); 2] = [
                (state.body_vim.buffer.cursor_row, state.body_vim.buffer.cursor_col),
                matched_bracket.unwrap_or((usize::MAX, usize::MAX)),
            ];
            for &(br, bc) in &highlight_positions {
                if br == line_idx && bc >= hscroll {
                    // Check if the char at (br, bc) is actually a bracket
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
        let scrollbar_area = Rect::new(text_area_x, inner.y, text_area_width, inner.height);
        render_scrollbar(frame, scrollbar_area, scroll_y, total_lines, visible_height, t.text_dim);
    }

    // Cursor position
    if is_insert || is_visual || is_visual_block {
        let cursor_screen_row = cursor_row as i32 - scroll_y as i32;
        if cursor_screen_row >= 0 && (cursor_screen_row as u16) < inner.height {
            let cursor_x = text_area_x + state.body_vim.buffer.cursor_col.saturating_sub(hscroll) as u16;
            let cursor_y = inner.y + cursor_screen_row as u16;
            if cursor_x < inner.right() {
                frame.set_cursor_position(Position::new(cursor_x, cursor_y));
            }
        }
    }

    // Search bar
    if has_search_bar && outer_inner.height > 2 {
        let search_area = Rect::new(
            outer_inner.x,
            outer_inner.y + outer_inner.height.saturating_sub(1),
            outer_inner.width,
            1,
        );
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

fn render_body_tab_bar(state: &AppState, is_focused: bool) -> Line<'static> {
    let t = &state.theme;
    let mut spans = Vec::new();
    spans.push(Span::raw(" "));
    let tabs = [BodyType::Json, BodyType::Xml, BodyType::FormUrlEncoded, BodyType::Plain];
    for (i, tab) in tabs.iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled("  ", Style::default().fg(t.text_dim)));
        }
        let is_active = state.body_type == *tab;
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
    if is_focused {
        spans.push(Span::styled("  {/}", Style::default().fg(t.text_dim)));
    }
    Line::from(spans)
}

fn visual_range(state: &AppState) -> (usize, usize, usize, usize) {
    let (ar, ac) = (state.body_vim.buffer.visual_anchor_row, state.body_vim.buffer.visual_anchor_col);
    let (cr, cc) = (state.body_vim.buffer.cursor_row, state.body_vim.buffer.cursor_col);
    if (ar, ac) <= (cr, cc) {
        (ar, ac, cr, cc)
    } else {
        (cr, cc, ar, ac)
    }
}

/// Calculate the rectangle for Visual Block selection: (min_row, min_col, max_row, max_col)
fn visual_block_range(state: &AppState) -> (usize, usize, usize, usize) {
    let (ar, ac) = (state.body_vim.buffer.visual_anchor_row, state.body_vim.buffer.visual_anchor_col);
    let (cr, cc) = (state.body_vim.buffer.cursor_row, state.body_vim.buffer.cursor_col);
    (ar.min(cr), ac.min(cc), ar.max(cr), ac.max(cc))
}

/// Highlight a rectangular column range within a line for Visual Block mode.
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

fn highlight_visual_line(line: &str, row: usize, sr: usize, sc: usize, er: usize, ec: usize) -> Line<'static> {
    let start_col = if row == sr { sc } else { 0 };
    let end_col = if row == er { (ec + 1).min(line.len()) } else { line.len() };
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

/// Render a line with block cursor (inverted char) at cursor_col, with highlighted background.
fn render_normal_cursor_line(line: &str, cursor_col: usize, t: &crate::theme::Theme) -> Line<'static> {
    let line_style = Style::default().fg(t.text).bg(t.bg_highlight);
    let cursor_style = Style::default().fg(Color::Black).bg(t.text); // inverted block cursor

    if line.is_empty() {
        // Show a block cursor on empty line
        return Line::from(vec![
            Span::styled(" ", cursor_style),
        ]);
    }

    let col = cursor_col.min(line.len());
    let before = &line[..col];

    if col >= line.len() {
        // Cursor is at end of line — show block cursor as space after text
        return Line::from(vec![
            Span::styled(before.to_string(), line_style),
            Span::styled(" ", cursor_style),
        ]);
    }

    // Get the char at cursor position
    let cursor_char = &line[col..col + 1]; // safe for ASCII; for multi-byte we'd need char_indices
    let after = &line[col + 1..];

    Line::from(vec![
        Span::styled(before.to_string(), line_style),
        Span::styled(cursor_char.to_string(), cursor_style),
        Span::styled(after.to_string(), line_style),
    ])
}

fn colorize_json_line(line: &str, t: &crate::theme::Theme) -> Line<'static> {
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

fn highlight_body_search_line(
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
            return render_normal_cursor_line(line, state.body_vim.buffer.cursor_col.saturating_sub(hscroll), t);
        }
        return colorize_json_line(line, t);
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

            // Adjust match_start by hscroll to compare with absolute search match positions
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

pub fn find_matching_bracket(lines: &[&str], row: usize, col: usize) -> Option<(usize, usize)> {
    let line = lines.get(row)?;
    let ch = line.as_bytes().get(col)?;
    let (target, direction): (u8, i32) = match ch {
        b'{' => (b'}', 1),
        b'}' => (b'{', -1),
        b'[' => (b']', 1),
        b']' => (b'[', -1),
        b'(' => (b')', 1),
        b')' => (b'(', -1),
        _ => return None,
    };
    let mut depth: i32 = 1;
    let mut r = row;
    let mut c = col;
    loop {
        if direction > 0 {
            c += 1;
            if c >= lines.get(r).map_or(0, |l| l.len()) {
                r += 1;
                c = 0;
                if r >= lines.len() {
                    return None;
                }
            }
        } else {
            if c == 0 {
                if r == 0 {
                    return None;
                }
                r -= 1;
                c = lines.get(r).map_or(0, |l| l.len().saturating_sub(1));
            } else {
                c -= 1;
            }
        }
        let b = *lines.get(r)?.as_bytes().get(c)?;
        if b == *ch {
            depth += 1;
        }
        if b == target {
            depth -= 1;
            if depth == 0 {
                return Some((r, c));
            }
        }
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
