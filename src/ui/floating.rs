use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, Paragraph};

use crate::state::{AppState, Overlay};

pub fn render(frame: &mut Frame, state: &AppState, overlay: &Overlay) {
    match overlay {
        Overlay::EnvironmentSelector => render_env_selector(frame, state),
        Overlay::HeaderAutocomplete { suggestions, selected } => {
            render_header_autocomplete(frame, state, suggestions, *selected);
        }
        Overlay::NewCollection { name } => render_new_collection(frame, name),
        Overlay::RenameRequest { name } => render_rename_request(frame, name),
        Overlay::ConfirmDelete { message } => render_confirm_delete(frame, message),
        Overlay::MoveRequest { selected } => render_move_request(frame, state, *selected),
        Overlay::SetCacheTTL { input } => render_cache_ttl(frame, state, input),
        Overlay::ThemeSelector { selected } => render_theme_selector(frame, *selected),
        Overlay::EnvironmentEditor { selected, editing_key, new_key, new_value, cursor } => {
            render_env_editor(frame, state, *selected, *editing_key, new_key, new_value, *cursor);
        }
        Overlay::Help => {}
        Overlay::History { selected } => render_history(frame, state, *selected),
        Overlay::ResponseHistory { selected } => render_response_history(frame, state, *selected),
        Overlay::ResponseDiffSelect { selected } => render_response_diff_select(frame, state, *selected),
    }
}

fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}

fn render_env_selector(frame: &mut Frame, state: &AppState) {
    let area = centered_rect(40, 30, frame.area());
    frame.render_widget(Clear, area);

    let block = Block::default()
        .title(" Select Environment ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    let items: Vec<ListItem> = state
        .environments
        .environments
        .iter()
        .enumerate()
        .map(|(i, env)| {
            let marker = if state.environments.active == Some(i) {
                "● "
            } else {
                "  "
            };
            ListItem::new(Line::from(vec![
                Span::styled(marker, Style::default().fg(Color::Green)),
                Span::styled(&env.name, Style::default().fg(Color::White)),
                Span::styled(
                    format!(" ({} vars)", env.variables.len()),
                    Style::default().fg(Color::DarkGray),
                ),
            ]))
        })
        .collect();

    let mut selector_state = state.env_selector_state.clone();
    let list = List::new(items)
        .block(block)
        .highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▸ ");

    frame.render_stateful_widget(list, area, &mut selector_state);
}

fn render_header_autocomplete(
    frame: &mut Frame,
    state: &AppState,
    suggestions: &[(String, String)],
    selected: usize,
) {
    let full = frame.area();

    // Position near the request panel header being edited
    let right_x = (full.width as u32 * 20 / 100) as u16; // collections panel = 20%
    let header_row = match state.request_edit.focus {
        crate::state::RequestFocus::Header(i) => 3 + i as u16, // border + URL + tab bar + header index
        _ => 3,
    };
    let popup_width = 50u16.min(full.width.saturating_sub(right_x + 2));
    let popup_height = (suggestions.len() as u16 + 2).min(15).min(full.height.saturating_sub(header_row + 2));
    let popup_x = right_x + 2;
    let popup_y = if header_row + 1 + popup_height <= full.height {
        header_row + 1 // below the header row
    } else {
        header_row.saturating_sub(popup_height) // above if no room below
    };

    let area = Rect::new(popup_x, popup_y, popup_width, popup_height);
    frame.render_widget(Clear, area);

    let block = Block::default()
        .title(" Add Header (A) — j/k to select, Enter to add ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    let items: Vec<ListItem> = suggestions
        .iter()
        .map(|(name, value)| {
            let display_val = if value.is_empty() {
                "(empty)".to_string()
            } else {
                value.clone()
            };
            let val_color = if value.is_empty() { Color::DarkGray } else { Color::Green };
            ListItem::new(Line::from(vec![
                Span::styled(name, Style::default().fg(Color::Yellow)),
                Span::styled(": ", Style::default().fg(Color::DarkGray)),
                Span::styled(display_val, Style::default().fg(val_color)),
            ]))
        })
        .collect();

    let mut list_state = ratatui::widgets::ListState::default();
    list_state.select(Some(selected));

    let list = List::new(items)
        .block(block)
        .highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▸ ");

    frame.render_stateful_widget(list, area, &mut list_state);
}

fn render_rename_request(frame: &mut Frame, name: &str) {
    let area = centered_rect(50, 15, frame.area());
    frame.render_widget(Clear, area);

    let block = Block::default()
        .title(" Rename ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let lines = vec![
        Line::from(Span::styled(
            " New name:",
            Style::default().fg(Color::Yellow),
        )),
        Line::from(Span::styled(
            format!(" {}▌", name),
            Style::default().fg(Color::Cyan),
        )),
        Line::from(""),
        Line::from(Span::styled(
            " Enter to confirm, Esc to cancel",
            Style::default().fg(Color::DarkGray),
        )),
    ];
    frame.render_widget(Paragraph::new(lines), inner);
}

fn render_confirm_delete(frame: &mut Frame, message: &str) {
    let area = centered_rect(55, 15, frame.area());
    frame.render_widget(Clear, area);

    let block = Block::default()
        .title(" Confirm Delete ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Red));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let lines = vec![
        Line::from(""),
        Line::from(Span::styled(
            format!(" {}", message),
            Style::default().fg(Color::Red),
        )),
    ];
    frame.render_widget(Paragraph::new(lines), inner);
}

fn render_move_request(frame: &mut Frame, state: &AppState, selected: usize) {
    let area = centered_rect(45, 35, frame.area());
    frame.render_widget(Clear, area);

    let block = Block::default()
        .title(" Move to Collection ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    let items: Vec<ListItem> = state
        .collections
        .iter()
        .enumerate()
        .map(|(i, coll)| {
            let marker = if i == state.collections_view.active { "  (current)" } else { "" };
            ListItem::new(Line::from(vec![
                Span::styled(&coll.name, Style::default().fg(Color::White)),
                Span::styled(
                    format!(" ({} reqs){}", coll.requests.len(), marker),
                    Style::default().fg(Color::DarkGray),
                ),
            ]))
        })
        .collect();

    let mut list_state = ratatui::widgets::ListState::default();
    list_state.select(Some(selected));

    let list = List::new(items)
        .block(block)
        .highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▸ ");

    frame.render_stateful_widget(list, area, &mut list_state);
}

fn render_new_collection(frame: &mut Frame, name: &str) {
    let area = centered_rect(50, 15, frame.area());
    frame.render_widget(Clear, area);

    let block = Block::default()
        .title(" New Collection ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let lines = vec![
        Line::from(Span::styled(
            " Name:",
            Style::default().fg(Color::Yellow),
        )),
        Line::from(Span::styled(
            format!(" {}▌", name),
            Style::default().fg(Color::Cyan),
        )),
        Line::from(""),
        Line::from(Span::styled(
            " Enter to create, Esc to cancel",
            Style::default().fg(Color::DarkGray),
        )),
    ];
    frame.render_widget(Paragraph::new(lines), inner);
}

fn render_cache_ttl(frame: &mut Frame, state: &AppState, input: &str) {
    let area = centered_rect(50, 20, frame.area());
    frame.render_widget(Clear, area);

    let block = Block::default()
        .title(" Set Cache TTL ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let current = state.config.general.chain_cache_ttl;
    let lines = vec![
        Line::from(Span::styled(
            format!(" Current: {}s", current),
            Style::default().fg(Color::DarkGray),
        )),
        Line::from(""),
        Line::from(Span::styled(
            " Time in seconds:",
            Style::default().fg(Color::Yellow),
        )),
        Line::from(Span::styled(
            format!(" {}▌", input),
            Style::default().fg(Color::Cyan),
        )),
        Line::from(""),
        Line::from(Span::styled(
            " Enter to set, Esc to cancel",
            Style::default().fg(Color::DarkGray),
        )),
    ];
    frame.render_widget(Paragraph::new(lines), inner);
}

fn render_theme_selector(frame: &mut Frame, selected: usize) {
    use crate::theme::THEME_NAMES;

    let area = centered_rect(40, 30, frame.area());
    frame.render_widget(Clear, area);

    let block = Block::default()
        .title(" Select Theme ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let items: Vec<ListItem> = THEME_NAMES
        .iter()
        .enumerate()
        .map(|(i, &name)| {
            let marker = if i == selected { "▸ " } else { "  " };
            let style = if i == selected {
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            ListItem::new(Line::from(Span::styled(
                format!("{}{}", marker, name),
                style,
            )))
        })
        .collect();

    let mut list_state = ratatui::widgets::ListState::default();
    list_state.select(Some(selected));

    let list = List::new(items)
        .highlight_style(Style::default().bg(Color::Rgb(40, 40, 50)));

    frame.render_stateful_widget(list, inner, &mut list_state);
}

fn render_env_editor(
    frame: &mut Frame,
    state: &AppState,
    selected: usize,
    editing_key: bool,
    new_key: &str,
    new_value: &str,
    cursor: usize,
) {
    let area = centered_rect(60, 70, frame.area());
    frame.render_widget(Clear, area);

    let env_name = state.environments.active_name();
    let title = format!(" Environment: {} ", env_name);

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let is_editing = cursor > 0 || editing_key;

    // Build lines
    let mut lines: Vec<Line> = Vec::new();

    if let Some(env) = state.environments.active_env() {
        for (i, (key, val)) in env.variables.iter().enumerate() {
            let is_selected = i == selected;
            let marker = if is_selected { "▸ " } else { "  " };

            if is_selected && !editing_key && cursor > 0 {
                // Editing this variable's value
                lines.push(Line::from(vec![
                    Span::styled(marker, Style::default().fg(Color::Cyan)),
                    Span::styled(key, Style::default().fg(Color::Yellow)),
                    Span::styled(" = ", Style::default().fg(Color::DarkGray)),
                    Span::styled(format!("{}|", new_value), Style::default().fg(Color::Green)),
                ]));
            } else if is_selected {
                lines.push(Line::from(vec![
                    Span::styled(marker, Style::default().fg(Color::Cyan)),
                    Span::styled(
                        key,
                        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(" = ", Style::default().fg(Color::DarkGray)),
                    Span::styled(val, Style::default().fg(Color::White)),
                ]));
            } else {
                lines.push(Line::from(vec![
                    Span::styled(marker, Style::default().fg(Color::DarkGray)),
                    Span::styled(key, Style::default().fg(Color::Yellow)),
                    Span::styled(" = ", Style::default().fg(Color::DarkGray)),
                    Span::styled(val, Style::default().fg(Color::White)),
                ]));
            }
        }
    } else {
        lines.push(Line::from(Span::styled(
            "  No active environment. Press 'p' to select one.",
            Style::default().fg(Color::DarkGray),
        )));
    }

    // Show input for adding new variable
    if editing_key {
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled("  New key: ", Style::default().fg(Color::Yellow)),
            Span::styled(format!("{}|", new_key), Style::default().fg(Color::Cyan)),
        ]));
    } else if cursor > 0 && !new_key.is_empty() {
        // Editing value for a new key
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled(format!("  {} = ", new_key), Style::default().fg(Color::Yellow)),
            Span::styled(format!("{}|", new_value), Style::default().fg(Color::Green)),
        ]));
    }

    // Bottom hints
    lines.push(Line::from(""));
    if is_editing {
        lines.push(Line::from(Span::styled(
            "  Enter:confirm  Esc:cancel",
            Style::default().fg(Color::DarkGray),
        )));
    } else {
        lines.push(Line::from(Span::styled(
            "  e:edit  a:add  d:delete  Esc:close",
            Style::default().fg(Color::DarkGray),
        )));
    }

    frame.render_widget(Paragraph::new(lines), inner);
}

fn render_history(frame: &mut Frame, state: &AppState, selected: usize) {
    let area = centered_rect(70, 60, frame.area());
    frame.render_widget(Clear, area);

    let block = Block::default()
        .title(" Request History (j/k  Enter:load  Esc:close) ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if state.history.entries.is_empty() {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "  No history yet. Execute a request first.",
                Style::default().fg(Color::DarkGray),
            ))),
            inner,
        );
        return;
    }

    let items: Vec<ListItem> = state.history.entries
        .iter()
        .enumerate()
        .map(|(i, entry)| {
            let status_color = match entry.status {
                200..=299 => Color::Green,
                300..=399 => Color::Yellow,
                400..=499 => Color::Red,
                500..=599 => Color::Magenta,
                _ => Color::DarkGray,
            };
            let line = Line::from(vec![
                Span::styled(
                    format!("{:>3} ", entry.status),
                    Style::default().fg(status_color).add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("{:<7} ", format!("{}", entry.method)),
                    Style::default().fg(Color::Cyan),
                ),
                Span::raw(&entry.url),
                Span::styled(
                    format!("  {}ms  {}", entry.elapsed_ms, entry.timestamp),
                    Style::default().fg(Color::DarkGray),
                ),
            ]);
            let style = if i == selected {
                Style::default().bg(Color::DarkGray).fg(Color::White)
            } else {
                Style::default()
            };
            ListItem::new(line).style(style)
        })
        .collect();

    let mut list_state = ratatui::widgets::ListState::default();
    list_state.select(Some(selected));

    let list = List::new(items)
        .highlight_style(Style::default().bg(Color::DarkGray).fg(Color::White))
        .highlight_symbol("\u{25b8} ");

    frame.render_stateful_widget(list, inner, &mut list_state);
}

fn render_response_history(frame: &mut Frame, state: &AppState, selected: usize) {
    let area = centered_rect(60, 50, frame.area());
    frame.render_widget(Clear, area);

    let block = Block::default()
        .title(" Response History (j/k  Enter:load  Esc:close) ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let key = state.current_request.name.as_ref().map(|name| {
        let coll = state.collections
            .get(state.collections_view.active)
            .map(|c| c.name.as_str())
            .unwrap_or("_");
        format!("{}/{}", coll, name)
    });

    let entries = match key.as_ref().and_then(|k| state.response_histories.data.get(k)) {
        Some(e) if !e.is_empty() => e,
        _ => {
            frame.render_widget(
                Paragraph::new(Line::from(Span::styled(
                    "  No response history for this request.",
                    Style::default().fg(Color::DarkGray),
                ))),
                inner,
            );
            return;
        }
    };

    let items: Vec<ListItem> = entries
        .iter()
        .enumerate()
        .map(|(i, entry)| {
            let resp = &entry.response;
            let status_color = match resp.status {
                200..=299 => Color::Green,
                300..=399 => Color::Yellow,
                400..=499 => Color::Red,
                500..=599 => Color::Magenta,
                _ => Color::DarkGray,
            };
            let ct = resp.content_type.as_deref().unwrap_or("");
            let line = Line::from(vec![
                Span::styled(
                    format!("{:>3} {} ", resp.status, resp.status_text),
                    Style::default().fg(status_color).add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("{:<8} ", resp.elapsed_display()),
                    Style::default().fg(Color::Cyan),
                ),
                Span::styled(
                    format!("{:<8} ", resp.size_display()),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::styled(ct.to_string(), Style::default().fg(Color::DarkGray)),
                Span::styled(
                    format!("  {}", entry.timestamp.format("%H:%M:%S")),
                    Style::default().fg(Color::DarkGray),
                ),
            ]);
            let style = if i == selected {
                Style::default().bg(Color::DarkGray).fg(Color::White)
            } else {
                Style::default()
            };
            ListItem::new(line).style(style)
        })
        .collect();

    let mut list_state = ratatui::widgets::ListState::default();
    list_state.select(Some(selected));
    let list = List::new(items)
        .highlight_style(Style::default().bg(Color::DarkGray).fg(Color::White))
        .highlight_symbol("\u{25b8} ");
    frame.render_stateful_widget(list, inner, &mut list_state);
}

fn render_response_diff_select(frame: &mut Frame, state: &AppState, selected: usize) {
    let area = centered_rect(60, 50, frame.area());
    frame.render_widget(Clear, area);

    let block = Block::default()
        .title(" Diff: select response to compare (j/k  Enter  Esc) ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let key = state.current_request.name.as_ref().map(|name| {
        let coll = state.collections.get(state.collections_view.active).map(|c| c.name.as_str()).unwrap_or("_");
        format!("{}/{}", coll, name)
    });
    let entries = match key.as_ref().and_then(|k| state.response_histories.data.get(k)) {
        Some(e) if !e.is_empty() => e,
        _ => {
            frame.render_widget(
                Paragraph::new(Line::from(Span::styled("  No history to compare.", Style::default().fg(Color::DarkGray)))),
                inner,
            );
            return;
        }
    };

    let items: Vec<ListItem> = entries.iter().enumerate().map(|(i, entry)| {
        let resp = &entry.response;
        let status_color = match resp.status { 200..=299 => Color::Green, 300..=399 => Color::Yellow, 400..=499 => Color::Red, 500..=599 => Color::Magenta, _ => Color::DarkGray };
        let line = Line::from(vec![
            Span::styled(format!("{:>3} {} ", resp.status, resp.status_text), Style::default().fg(status_color).add_modifier(Modifier::BOLD)),
            Span::styled(format!("{:<8} ", resp.elapsed_display()), Style::default().fg(Color::Cyan)),
            Span::styled(entry.timestamp.format("%H:%M:%S").to_string(), Style::default().fg(Color::DarkGray)),
        ]);
        let style = if i == selected { Style::default().bg(Color::DarkGray).fg(Color::White) } else { Style::default() };
        ListItem::new(line).style(style)
    }).collect();

    let mut list_state = ratatui::widgets::ListState::default();
    list_state.select(Some(selected));
    let list = List::new(items).highlight_style(Style::default().bg(Color::DarkGray).fg(Color::White)).highlight_symbol("\u{25b8} ");
    frame.render_stateful_widget(list, inner, &mut list_state);
}

