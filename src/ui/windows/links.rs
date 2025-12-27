use ratatui::{
    layout::Rect,
    style::{Color, Style},
    text::Line,
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph},
    Frame,
};

use crate::models::LinkEntry;

pub struct LinksWindow;

impl LinksWindow {
    pub fn render(frame: &mut Frame, area: Rect, entries: &[LinkEntry], selected_index: usize) {
        let popup_area = Rect::new(
            area.x + area.width / 5,
            area.y + area.height / 6,
            area.width * 3 / 5,
            area.height * 2 / 3,
        );

        frame.render_widget(Clear, popup_area);

        if entries.is_empty() {
            let paragraph = Paragraph::new("No links on this page")
                .style(Style::default().fg(Color::DarkGray))
                .block(Block::default().title("Links").borders(Borders::ALL));
            frame.render_widget(paragraph, popup_area);
            return;
        }

        let items: Vec<ListItem> = entries
            .iter()
            .enumerate()
            .map(|(i, entry)| {
                let style = if i == selected_index {
                    Style::default().bg(Color::Blue).fg(Color::White)
                } else {
                    Style::default()
                };
                let label = if entry.label == entry.url {
                    entry.url.clone()
                } else {
                    format!("{} -> {}", entry.label, entry.url)
                };
                ListItem::new(Line::from(label)).style(style)
            })
            .collect();

        let list = List::new(items)
            .block(Block::default().title("Links").borders(Borders::ALL));

        frame.render_widget(list, popup_area);

        let status_area = Rect::new(
            popup_area.x + 1,
            popup_area.y + popup_area.height - 2,
            popup_area.width - 2,
            1,
        );
        let status_line = Paragraph::new("Enter: follow  y: copy  q: close")
            .style(Style::default().fg(Color::Yellow));
        frame.render_widget(status_line, status_area);
    }
}
