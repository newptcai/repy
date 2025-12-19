use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
    Frame,
};

use crate::models::{BookMetadata, TocEntry};

pub struct TocWindow;

impl TocWindow {
    pub fn render(
        frame: &mut Frame,
        area: Rect,
        entries: &[TocEntry],
        selected_index: usize,
        metadata: Option<&BookMetadata>,
    ) {
        let popup_area = Self::centered_popup_area(area, 60, 70);

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

        // Header title from book metadata, falling back to a generic label
        let book_title = metadata
            .and_then(|m| m.title.as_deref())
            .unwrap_or("Table of Contents");

        let mut lines: Vec<Line> = Vec::new();

        // Book title at the top, styled similarly to other popups
        lines.push(Line::from(Span::styled(
            format!(" {}", book_title),
            Style::default().add_modifier(Modifier::BOLD),
        )));

        // Indented table of contents entries; only show the label, not the raw section/html
        for (i, entry) in entries.iter().enumerate() {
            let style = if i == selected_index {
                Style::default().bg(Color::Blue).fg(Color::White)
            } else {
                Style::default()
            };

            let content = format!("   {}", entry.label);
            lines.push(Line::from(Span::styled(content, style)));
        }

        let paragraph = Paragraph::new(lines)
            .block(Block::default().title("Table of Contents").borders(Borders::ALL));

        frame.render_widget(paragraph, popup_area);
    }

    fn centered_popup_area(area: Rect, width_percent: u16, height_percent: u16) -> Rect {
        let width = (area.width * width_percent) / 100;
        let height = (area.height * height_percent) / 100;
        let x = area.x + (area.width - width) / 2;
        let y = area.y + (area.height - height) / 2;

        Rect::new(x, y, width, height)
    }
}
