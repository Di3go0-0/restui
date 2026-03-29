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
        let cursor_screen = estimate_cursor_screen_pos(state, right_area);
        render_chain_autocomplete(frame, state, frame.area(), cursor_screen);
    }

    // Command palette renders on top of everything
    if state.command_palette.open {
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

/// Estimate the screen position (x, y) of the text cursor in the active panel.
fn estimate_cursor_screen_pos(state: &AppState, right_area: Rect) -> (u16, u16) {
    let gutter = 4u16; // line number gutter width
    let border = 1u16;
    match state.active_panel {
        Panel::Body => {
            // Body panel is in the lower part of right_area
            // Approximate: body starts after request panel (~40% in wide, ~35% in narrow)
            let body_y_offset = if right_area.width > 120 {
                // Wide layout: body is ~60% of center column which is left half
                (right_area.height as u32 * 40 / 100) as u16
            } else {
                // Narrow layout: body starts at ~25%
                (right_area.height as u32 * 25 / 100) as u16
            };
            let cursor_row_on_screen = (state.body_cursor_row as u16).saturating_sub(state.body_scroll.0);
            let cursor_col_on_screen = (state.body_cursor_col as u16).saturating_sub(state.body_scroll.1);
            let x = right_area.x + border + gutter + cursor_col_on_screen;
            let y = right_area.y + body_y_offset + border + 1 + cursor_row_on_screen; // +1 for body tab bar
            (x.min(right_area.right()), y.min(right_area.bottom()))
        }
        Panel::Request => {
            // Request panel is at the top of right_area
            let x = right_area.x + border + 10; // rough offset for field label
            let y = right_area.y + border + 2; // header area
            (x, y)
        }
        _ => {
            (right_area.x + 2, right_area.y + 2)
        }
    }
}

fn render_chain_autocomplete(frame: &mut Frame, state: &AppState, area: Rect, cursor_pos: (u16, u16)) {
    let Some(ref ac) = state.chain_autocomplete else { return; };
    if ac.items.is_empty() { return; }

    let t = &state.theme;
    let max_items = ac.items.len().min(8);

    let popup_width = 44u16.min(area.width.saturating_sub(4));
    let popup_height = (max_items as u16 + 2).min(area.height.saturating_sub(2));

    if popup_width < 10 || popup_height < 3 { return; }

    // Position below and to the right of the cursor
    let (cx, cy) = cursor_pos;
    let popup_x = cx.min(area.right().saturating_sub(popup_width));
    let popup_y = if cy + 1 + popup_height <= area.bottom() {
        cy + 1 // below cursor
    } else {
        cy.saturating_sub(popup_height) // above cursor if no room below
    };

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
