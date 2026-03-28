use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use std::time::Duration;

use crate::state::{AppState, InputMode, Panel};

pub fn render(frame: &mut Frame, state: &AppState, area: Rect) {
    let mode_span = match state.mode {
        InputMode::Normal => Span::styled(
            " NORMAL ",
            Style::default()
                .fg(Color::Black)
                .bg(Color::Blue)
                .add_modifier(Modifier::BOLD),
        ),
        InputMode::Insert => Span::styled(
            " INSERT ",
            Style::default()
                .fg(Color::Black)
                .bg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ),
        InputMode::Visual => Span::styled(
            " VISUAL ",
            Style::default()
                .fg(Color::Black)
                .bg(Color::Magenta)
                .add_modifier(Modifier::BOLD),
        ),
    };

    let env_name = state.environments.active_name();

    let mut spans = vec![
        mode_span,
        Span::raw(" "),
        Span::styled(
            format!(" ENV: {} ", env_name),
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" "),
        Span::styled(
            format!(" {} ", state.active_panel.title()),
            Style::default().fg(Color::Black).bg(Color::DarkGray),
        ),
        Span::raw(" "),
        Span::styled(
            format!(" {} ", state.current_request.method),
            Style::default()
                .fg(Color::Black)
                .bg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" "),
    ];

    // Cursor position for body/response panels
    let show_cursor_pos = match state.active_panel {
        Panel::Body => state.mode == InputMode::Insert || state.mode == InputMode::Visual,
        Panel::Response => true, // Always show position in response
        _ => false,
    };
    if show_cursor_pos {
        let (row, col) = match state.active_panel {
            Panel::Response => (state.resp_cursor_row, state.resp_cursor_col),
            _ => (state.body_cursor_row, state.body_cursor_col),
        };
        spans.push(Span::styled(
            format!(" {}:{} ", row + 1, col + 1),
            Style::default()
                .fg(Color::Black)
                .bg(Color::DarkGray),
        ));
        spans.push(Span::raw(" "));
    }

    if let Some((ref msg, ref instant)) = state.status_message {
        if instant.elapsed() < Duration::from_secs(5) {
            spans.push(Span::styled(msg, Style::default().fg(Color::Yellow)));
        }
    }

    let hints = match state.mode {
        InputMode::Normal => match state.active_panel {
            Panel::Request => " i:edit  a/A:add header  dd:del  [/]:method  Ctrl+R:run ",
            Panel::Body => " i:insert  v:visual  o:new line  t:body type  Ctrl+V:paste  Ctrl+R:run ",
            Panel::Collections => " Enter:sel  s:save  S:save-as  C:new-empty  n:new-coll  {/}:switch ",
            Panel::Response => " j/k:move  v:visual  w/b:word  y:copy  Y:curl  Ctrl+R:run ",
        },
        InputMode::Insert => match state.active_panel {
            Panel::Request => " Esc:normal  Tab:next field  Enter:confirm ",
            Panel::Body => " Esc:normal  Tab:indent ",
            _ => " Esc:normal ",
        },
        InputMode::Visual => " y:yank  d:delete  Esc:cancel  hjkl:select ",
    };

    let hints_len = hints.len() as u16;
    let left_len: u16 = spans.iter().map(|s| s.content.len() as u16).sum();
    let padding = area.width.saturating_sub(left_len + hints_len);

    spans.push(Span::raw(" ".repeat(padding as usize)));
    spans.push(Span::styled(hints, Style::default().fg(Color::DarkGray)));

    let line = Line::from(spans);
    frame.render_widget(Paragraph::new(line), area);
}
