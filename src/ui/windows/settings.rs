use ratatui::{
    layout::Rect,
    style::{Color, Style},
    text::Line,
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph},
    Frame,
};

pub struct SettingsWindow;

impl SettingsWindow {
    pub fn render(frame: &mut Frame, area: Rect, entries: &[String], selected_index: usize) {
        let popup_area = Rect::new(
            area.x + area.width / 6,
            area.y + area.height / 8,
            area.width * 2 / 3,
            area.height * 3 / 4,
        );

        frame.render_widget(Clear, popup_area);

        if entries.is_empty() {
            let paragraph = Paragraph::new("No settings available")
                .style(Style::default().fg(Color::DarkGray))
                .block(Block::default().title("Settings").borders(Borders::ALL));
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
                ListItem::new(Line::from(entry.clone())).style(style)
            })
            .collect();

        let list = List::new(items)
            .block(Block::default().title("Settings").borders(Borders::ALL));

        frame.render_widget(list, popup_area);
    }
}
