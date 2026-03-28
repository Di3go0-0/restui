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
            render_header_autocomplete(frame, suggestions, *selected);
        }
        Overlay::NewCollection { name } => render_new_collection(frame, name),
        Overlay::Help => {}
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
    suggestions: &[(String, String)],
    selected: usize,
) {
    let area = centered_rect(55, 55, frame.area());
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
