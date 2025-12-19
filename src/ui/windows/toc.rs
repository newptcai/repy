use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph},
    Frame,
};

use crate::models::TocEntry;

pub struct TocWindow;

impl TocWindow {
    pub fn render(frame: &mut Frame, area: Rect, entries: &[TocEntry], selected_index: usize) {
        let popup_area = Self::centered_popup_area(area, 50, 80);

        frame.render_widget(Clear, popup_area);

        if entries.is_empty() {
            let empty_text = vec![
                Line::from("No table of contents available"),
                Line::from(""),
                Line::from(Span::styled("Press any key to close", Style::default().add_modifier(Modifier::ITALIC))),
            ];

            let paragraph = Paragraph::new(empty_text)
                .style(Style::default().fg(Color::DarkGray))
                .block(Block::default().title("Table of Contents").borders(Borders::ALL));

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

                let content = if let Some(section) = &entry.section {
                    format!("{} ({})", entry.label, section)
                } else {
                    entry.label.clone()
                };

                ListItem::new(Line::from(content)).style(style)
            })
            .collect();

        let list = List::new(items)
            .block(Block::default().title("Table of Contents").borders(Borders::ALL));

        frame.render_widget(list, popup_area);
    }

    fn centered_popup_area(area: Rect, width_percent: u16, height_percent: u16) -> Rect {
        let width = (area.width * width_percent) / 100;
        let height = (area.height * height_percent) / 100;
        let x = area.x + (area.width - width) / 2;
        let y = area.y + (area.height - height) / 2;

        Rect::new(x, y, width, height)
    }
}
