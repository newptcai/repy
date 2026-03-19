use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::Line,
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph},
};

use crate::theme::Theme;

pub struct SearchWindow;

impl SearchWindow {
    pub fn render(
        frame: &mut Frame,
        area: Rect,
        query: &str,
        results: &[String],
        selected_index: usize,
        theme: &Theme,
    ) {
        let popup_area = super::centered_popup_area(area, 60, 70);

        frame.render_widget(Clear, popup_area);

        let header = Paragraph::new(Line::from(format!("/{}", query)))
            .block(Block::default().title("Search").borders(Borders::ALL).style(theme.base_style()))
            .style(theme.base_style().add_modifier(Modifier::BOLD));

        let header_area = Rect::new(popup_area.x, popup_area.y, popup_area.width, 3);
        frame.render_widget(header, header_area);

        // Set cursor position after / and query text
        frame.set_cursor_position((header_area.x + query.len() as u16 + 2, header_area.y + 1));

        let list_area = Rect::new(
            popup_area.x,
            popup_area.y + 3,
            popup_area.width,
            popup_area.height - 3,
        );

        if results.is_empty() {
            let empty = Paragraph::new("No matches yet")
                .style(theme.base_style().fg(theme.muted_fg))
                .block(Block::default().borders(Borders::ALL).style(theme.base_style()));
            frame.render_widget(empty, list_area);
            return;
        }

        let items: Vec<ListItem> = results
            .iter()
            .enumerate()
            .map(|(i, entry)| {
                let style = if i == selected_index {
                    Style::default()
                        .bg(theme.highlight_bg)
                        .fg(theme.highlight_fg)
                } else {
                    Style::default()
                };
                ListItem::new(Line::from(entry.clone())).style(style)
            })
            .collect();

        let list = List::new(items).block(Block::default().borders(Borders::ALL).style(theme.base_style()));

        frame.render_widget(list, list_area);
    }
}
