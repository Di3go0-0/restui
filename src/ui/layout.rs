use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout};

use crate::state::AppState;
use crate::ui::{body, collections, floating, help, request, response, statusbar};

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

    // Overlays render on top
    if let Some(ref overlay) = state.overlay {
        match overlay {
            crate::state::Overlay::Help => help::render(frame, state),
            _ => floating::render(frame, state, overlay),
        }
    }
}

fn render_center_panels(frame: &mut Frame, state: &AppState, area: ratatui::layout::Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
        .split(area);

    request::render(frame, state, chunks[0]);
    body::render(frame, state, chunks[1]);
}
