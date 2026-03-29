use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem};

use crate::state::{AppState, Panel};
use crate::ui::{body, collections, command_palette, floating, help, request, response, statusbar};

pub fn render(frame: &mut Frame, state: &AppState) {
    let area = frame.area();

    // Reserve bottom row for status bar
    let main_and_status = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(area);

    let main_area = main_and_status[0];
    let status_area = main_and_status[1];

    // Left panel (collections) | Right area (request + body + response)
    let h_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(20), Constraint::Percentage(80)])
        .split(main_area);

    let left_area = h_chunks[0];
    let right_area = h_chunks[1];

    // Render collections panel
    collections::render(frame, state, left_area);

    // Responsive layout for right area
    if right_area.width > 120 {
        // Wide: [center (req + body) | response] side by side
        let right_h = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(right_area);

        render_center_panels(frame, state, right_h[0]);
        response::render(frame, state, right_h[1]);
    } else {
        // Narrow: stacked vertically
        let right_v = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Percentage(25),
                Constraint::Percentage(35),
                Constraint::Percentage(40),
            ])
            .split(right_area);

        request::render(frame, state, right_v[0]);
        body::render(frame, state, right_v[1]);
        response::render(frame, state, right_v[2]);
    }

    // Status bar
    statusbar::render(frame, state, status_area);

    // Chain autocomplete popup (renders on top of panels but below overlays)
    if state.chain_autocomplete.is_some() {
        let popup_area = match state.active_panel {
            Panel::Request | Panel::Body => right_area,
            _ => main_area,
        };
        render_chain_autocomplete(frame, state, popup_area);
    }

    // Command palette renders on top of everything
    if state.command_palette_open {
        command_palette::render(frame, state);
    }
    // Overlays render on top
    else if let Some(ref overlay) = state.overlay {
        match overlay {
            crate::state::Overlay::Help => help::render(frame, state),
            _ => floating::render(frame, state, overlay),
        }
    }
}

fn render_center_panels(frame: &mut Frame, state: &AppState, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
        .split(area);

    request::render(frame, state, chunks[0]);
    body::render(frame, state, chunks[1]);
}

fn render_chain_autocomplete(frame: &mut Frame, state: &AppState, area: Rect) {
    let Some(ref ac) = state.chain_autocomplete else { return; };
    if ac.items.is_empty() { return; }

    let t = &state.theme;
    let max_items = ac.items.len().min(8);

    let popup_width = 44u16.min(area.width.saturating_sub(4));
    let popup_height = (max_items as u16 + 2).min(area.height.saturating_sub(2));

    if popup_width < 10 || popup_height < 3 { return; }

    let popup_x = area.x + 2;
    let popup_y = area.bottom().saturating_sub(popup_height + 1);

    let popup_area = Rect::new(popup_x, popup_y, popup_width, popup_height);
    frame.render_widget(Clear, popup_area);

    let block = Block::default()
        .title(" {{@chain  Ctrl+n/p \u{2195}  Ctrl+y \u{2713} ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    let items: Vec<ListItem> = ac.items
        .iter()
        .take(max_items)
        .enumerate()
        .map(|(i, (display, _))| {
            let style = if i == ac.selected {
                Style::default().bg(Color::DarkGray).fg(Color::White).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(t.text)
            };
            ListItem::new(Line::from(Span::styled(display.clone(), style)))
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
        .highlight_symbol("\u{25b8} ");

    frame.render_stateful_widget(list, popup_area, &mut list_state);
}
