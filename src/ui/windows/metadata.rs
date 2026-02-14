use crate::models::BookMetadata;
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};

pub struct MetadataWindow;

impl MetadataWindow {
    pub fn render(frame: &mut Frame, area: Rect, metadata: Option<&BookMetadata>) {
        let popup_area = Self::centered_popup_area(area, 60, 80);

        frame.render_widget(Clear, popup_area);

        if let Some(metadata) = metadata {
            let content = vec![
                Line::from(Span::styled(
                    "Book Information",
                    Style::default().add_modifier(Modifier::BOLD),
                )),
                Line::from(""),
                Line::from(format!(
                    "Title: {}",
                    metadata.title.as_deref().unwrap_or("Unknown")
                )),
                Line::from(format!(
                    "Author: {}",
                    metadata.creator.as_deref().unwrap_or("Unknown")
                )),
                Line::from(format!(
                    "Publisher: {}",
                    metadata.publisher.as_deref().unwrap_or("Unknown")
                )),
                Line::from(format!(
                    "Date: {}",
                    metadata.date.as_deref().unwrap_or("Unknown")
                )),
                Line::from(format!(
                    "Language: {}",
                    metadata.language.as_deref().unwrap_or("Unknown")
                )),
                Line::from(format!(
                    "Format: {}",
                    metadata.format.as_deref().unwrap_or("Unknown")
                )),
                Line::from(""),
                Line::from("Description:"),
                Line::from(
                    metadata
                        .description
                        .as_deref()
                        .unwrap_or("No description available"),
                ),
                Line::from(""),
                Line::from(Span::styled(
                    "Press any key to close",
                    Style::default().add_modifier(Modifier::ITALIC),
                )),
            ];

            let paragraph = Paragraph::new(content)
                .block(Block::default().title("Metadata").borders(Borders::ALL));

            frame.render_widget(paragraph, popup_area);
        } else {
            let content = vec![
                Line::from("No metadata available"),
                Line::from(""),
                Line::from(Span::styled(
                    "Press any key to close",
                    Style::default().add_modifier(Modifier::ITALIC),
                )),
            ];

            let paragraph = Paragraph::new(content)
                .style(Style::default().fg(Color::DarkGray))
                .block(Block::default().title("Metadata").borders(Borders::ALL));

            frame.render_widget(paragraph, popup_area);
        }
    }

    fn centered_popup_area(area: Rect, width_percent: u16, height_percent: u16) -> Rect {
        let width = (area.width * width_percent) / 100;
        let height = (area.height * height_percent) / 100;
        let x = area.x + (area.width - width) / 2;
        let y = area.y + (area.height - height) / 2;

        Rect::new(x, y, width, height)
    }
}
