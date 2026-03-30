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

    // Responsive layout for right area — compute areas for all panels
    let (request_area, body_area) = if right_area.width > 120 {
        let right_h = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(right_area);

        render_center_panels(frame, state, right_h[0]);
        response::render(frame, state, right_h[1]);

        // In wide layout, center column is split 40/60
        let center_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
            .split(right_h[0]);
        (center_chunks[0], center_chunks[1])
    } else {
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
        (right_v[0], right_v[1])
    };

    // Status bar
    statusbar::render(frame, state, status_area);

    // Chain autocomplete popup (renders on top of panels but below overlays)
    if state.chain_autocomplete.is_some() {
        let cursor_screen = compute_cursor_screen_pos(state, request_area, body_area);
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

/// Compute the screen position (x, y) of the cursor using real panel areas.
fn compute_cursor_screen_pos(state: &AppState, request_area: Rect, body_area: Rect) -> (u16, u16) {
    let gutter = 4u16;
    let border = 1u16;
    match state.active_panel {
        Panel::Body => {
            let cursor_row_on_screen = (state.body_buf.cursor_row as u16).saturating_sub(state.body_buf.scroll.0);
            let cursor_col_on_screen = (state.body_buf.cursor_col as u16).saturating_sub(state.body_buf.scroll.1);
            let x = body_area.x + border + gutter + cursor_col_on_screen;
            let y = body_area.y + border + 1 + cursor_row_on_screen; // +1 for tab bar
            (x.min(body_area.right()), y.min(body_area.bottom()))
        }
        Panel::Request => {
            let field_row = match state.request_focus {
                crate::state::RequestFocus::Url => 1, // URL row after method
                crate::state::RequestFocus::Header(i) => 3 + i as u16, // tab bar + headers
                crate::state::RequestFocus::Param(i) => 3 + i as u16,
                crate::state::RequestFocus::Cookie(i) => 3 + i as u16,
                crate::state::RequestFocus::PathParam(i) => 3 + i as u16,
            };
            let cursor = state.url_cursor as u16;
            let x = request_area.x + border + 10 + cursor; // label offset
            let y = request_area.y + field_row;
            (x.min(request_area.right()), y.min(request_area.bottom()))
        }
        _ => {
            (body_area.x + 2, body_area.y + 2)
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
    let popup_y = if cy + 3 + popup_height <= area.bottom() {
        cy + 2 // 2 lines below cursor so it doesn't block the current line
    } else {
        cy.saturating_sub(popup_height + 1) // above cursor if no room below
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
