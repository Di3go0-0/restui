use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use std::time::Duration;

use crate::state::{AppState, InputMode, Panel};

pub fn render(frame: &mut Frame, state: &AppState, area: Rect) {
    let t = &state.theme;

    let mode_span = match state.mode {
        InputMode::Normal => Span::styled(
            " NORMAL ",
            Style::default()
                .fg(Color::Black)
                .bg(t.border_focused)
                .add_modifier(Modifier::BOLD),
        ),
        InputMode::Insert => Span::styled(
            " INSERT ",
            Style::default()
                .fg(Color::Black)
                .bg(t.border_insert)
                .add_modifier(Modifier::BOLD),
        ),
        InputMode::Visual => Span::styled(
            " VISUAL ",
            Style::default()
                .fg(Color::Black)
                .bg(t.border_visual)
                .add_modifier(Modifier::BOLD),
        ),
    };

    let env_name = state.environments.active_name();
    let method_color = t.method_color(state.current_request.method);

    let mut spans = vec![
        mode_span,
        Span::raw(" "),
        Span::styled(
            format!(" ENV: {} ", env_name),
            Style::default()
                .fg(Color::Black)
                .bg(t.accent)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" "),
        Span::styled(
            format!(" {} ", state.active_panel.title()),
            Style::default().fg(t.text).bg(t.bg_highlight),
        ),
        Span::raw(" "),
        Span::styled(
            format!(" {} ", state.current_request.method),
            Style::default()
                .fg(Color::Black)
                .bg(method_color)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" "),
    ];

    // SSL indicator
    if !state.config.general.verify_ssl {
        spans.push(Span::styled(
            " INSECURE ",
            Style::default()
                .fg(Color::Black)
                .bg(Color::Red)
                .add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::raw(" "));
    }

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
            spans.push(Span::styled(msg, Style::default().fg(t.gutter_active)));
        }
    }

    let hints = match state.mode {
        InputMode::Normal => match state.active_panel {
            Panel::Request => " i:edit  a:add  {/}:tab  [/]:method  Ctrl+R:run  ?:help ",
            Panel::Body => " i:insert  v:visual  o:line  t:type  Ctrl+R:run  ?:help ",
            Panel::Collections => " r:rename  dd:del  yy:copy  p:paste  Sp:fold  ?:help ",
            Panel::Response => " j/k:move  v:visual  y:copy  Y:curl  ?:help ",
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
    spans.push(Span::styled(hints, Style::default().fg(t.text_dim)));

    let line = Line::from(spans);
    frame.render_widget(Paragraph::new(line), area);
}
