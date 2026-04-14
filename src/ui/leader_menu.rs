use ratatui::{
    layout::{Alignment, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
    Frame,
};

use crate::core::state::AppState;

/// Renders the leader menu as a floating overlay with transparent background
pub fn render_leader_menu(f: &mut Frame, state: &AppState, _area: Rect) {
    if !state.leader_context.menu_visible {
        return;
    }

    // Get entries for current panel
    let entries = state.leader_menu.get_entries_for_panel(state.active_panel);

    // Filter to only global entries if menu just opened
    let entries_to_show: Vec<_> = if state.leader_context.key_sequence.is_empty() {
        // Show all entries (global + panel-specific)
        entries
    } else {
        // If we have a sequence, we could do submenu filtering here
        // For now, just show all
        entries
    };

    // Build menu content with better spacing
    let mut lines = vec![];
    
    // Header line
    lines.push(Line::from(Span::styled(
        "───────────────────────────",
        Style::default().fg(Color::Cyan),
    )));

    for entry in entries_to_show {
        let key_str = format!("{}", entry.key);
        let key_span = Span::styled(
            format!("  [{}]", key_str),
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        );

        let desc_span = Span::styled(
            format!("  {}", entry.description),
            Style::default().fg(Color::White),
        );

        let submenu_indicator = if entry.submenu.is_some() {
            Span::styled(" ➜", Style::default().fg(Color::Yellow))
        } else {
            Span::raw("")
        };

        lines.push(Line::from(vec![key_span, desc_span, submenu_indicator]));
    }
    
    // Footer line
    lines.push(Line::from(Span::styled(
        "───────────────────────────",
        Style::default().fg(Color::Cyan),
    )));

    // Calculate dimensions for the menu box
    let menu_width = 42; // Slightly larger for better readability
    let menu_height = (lines.len() as u16 + 2).min(22); // +2 for borders, max 22 lines

    // Position in bottom-right corner
    let area = f.area();
    let menu_x = area.width.saturating_sub(menu_width + 2);
    let menu_y = area.height.saturating_sub(menu_height + 2);

    let menu_rect = Rect {
        x: menu_x,
        y: menu_y,
        width: menu_width,
        height: menu_height,
    };

    // First, clear the area to show terminal background (not the UI below)
    f.render_widget(Clear, menu_rect);

    // Create the block with border - use double borders for emphasis
    let block = Block::default()
        .title(" ⌘ Leader Menu ")
        .title_alignment(Alignment::Center)
        .borders(Borders::ALL)
        .style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD));

    // Create paragraph with the menu items
    let paragraph = Paragraph::new(lines)
        .block(block)
        .alignment(Alignment::Left);

    f.render_widget(paragraph, menu_rect);
}
