use fuzzy_matcher::skim::SkimMatcherV2;
use fuzzy_matcher::FuzzyMatcher;
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph};

use crate::core::command::{Command, all_commands};
use crate::core::state::AppState;

/// Returns the filtered + scored commands for the current input.
pub fn filtered_commands(input: &str) -> Vec<Command> {
    let commands = all_commands();
    if input.is_empty() {
        return commands;
    }

    let matcher = SkimMatcherV2::default();
    let mut scored: Vec<(i64, Command)> = commands
        .into_iter()
        .filter_map(|cmd| {
            let haystack = format!("{} {}", cmd.name, cmd.description);
            matcher.fuzzy_match(&haystack, input).map(|score| (score, cmd))
        })
        .collect();

    scored.sort_by(|a, b| b.0.cmp(&a.0));
    scored.into_iter().map(|(_, cmd)| cmd).collect()
}

pub fn render(frame: &mut Frame, state: &AppState) {
    let area = centered_rect(50, 60, frame.area());
    frame.render_widget(Clear, area);

    let t = &state.theme;

    let block = Block::default()
        .title(" Command Palette ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(t.border_focused))
        .style(Style::default().bg(t.overlay_bg));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.height < 3 || inner.width < 10 {
        return;
    }

    // Split: input line + separator + results list
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // input
            Constraint::Length(1), // separator
            Constraint::Min(1),   // results
        ])
        .split(inner);

    // Input line with cursor
    let input_line = Line::from(vec![
        Span::styled(" > ", Style::default().fg(t.gutter_active).add_modifier(Modifier::BOLD)),
        Span::styled(
            &state.command_palette.input,
            Style::default().fg(t.text),
        ),
        Span::styled("▌", Style::default().fg(t.gutter_active)),
    ]);
    frame.render_widget(Paragraph::new(input_line), chunks[0]);

    // Separator
    let sep = Line::from(Span::styled(
        "─".repeat(inner.width as usize),
        Style::default().fg(Color::DarkGray),
    ));
    frame.render_widget(Paragraph::new(sep), chunks[1]);

    // Filtered results
    let matches = filtered_commands(&state.command_palette.input);
    let visible_count = matches.len();

    let items: Vec<ListItem> = matches
        .iter()
        .enumerate()
        .map(|(i, cmd)| {
            let is_selected = i == state.command_palette.selected;
            let name_style = if is_selected {
                Style::default()
                    .fg(t.gutter_active)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(t.text)
            };
            let desc_style = if is_selected {
                Style::default().fg(t.accent)
            } else {
                Style::default().fg(t.text_dim)
            };
            let cat_style = Style::default()
                .fg(if is_selected { t.border_focused } else { t.text_dim })
                .add_modifier(Modifier::ITALIC);

            ListItem::new(Line::from(vec![
                Span::styled(format!(" {} ", cmd.name), name_style),
                Span::styled(format!("  {} ", cmd.description), desc_style),
                Span::styled(format!("[{}]", cmd.category), cat_style),
            ]))
        })
        .collect();

    let mut list_state = ListState::default();
    if visible_count > 0 {
        list_state.select(Some(state.command_palette.selected.min(visible_count - 1)));
    }

    let list = List::new(items)
        .highlight_style(
            Style::default()
                .bg(t.bg_highlight)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▸ ");

    frame.render_stateful_widget(list, chunks[2], &mut list_state);

    // Position cursor at end of input
    let cursor_x = chunks[0].x + 3 + state.command_palette.input.len() as u16;
    let cursor_y = chunks[0].y;
    if cursor_x < area.right() {
        frame.set_cursor_position(Position::new(cursor_x, cursor_y));
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
