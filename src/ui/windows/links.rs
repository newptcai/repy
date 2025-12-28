use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Style},
    text::Line,
    widgets::{Block, Borders, Clear, List, ListItem, Padding, Paragraph, Wrap},
    Frame,
};

use crate::models::LinkEntry;
use crate::ui::board::Board;

pub struct LinksWindow;

impl LinksWindow {
    pub fn render(
        frame: &mut Frame,
        area: Rect,
        entries: &[LinkEntry],
        selected_index: usize,
        board: &Board,
    ) {
        let popup_area = Rect::new(
            area.x + area.width / 5,
            area.y + area.height / 6,
            area.width * 3 / 5,
            area.height * 2 / 3,
        );

        frame.render_widget(Clear, popup_area);

        if entries.is_empty() {
            let block = Block::default()
                .title("Links")
                .borders(Borders::ALL)
                .padding(Padding::horizontal(1));
            let paragraph = Paragraph::new("No links on this page")
                .style(Style::default().fg(Color::DarkGray))
                .block(block);
            frame.render_widget(paragraph, popup_area);
            return;
        }

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(3),
                Constraint::Length(10), // Preview height
            ])
            .split(popup_area);

        let list_area = chunks[0];
        let preview_area = chunks[1];

        let block = Block::default()
            .title("Links")
            .borders(Borders::ALL)
            .padding(Padding::horizontal(1));

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

        let list = List::new(items).block(block);
        frame.render_widget(list, list_area);

        // Status line in list area
        let status_area = Rect::new(
            list_area.x + 2,
            list_area.y + list_area.height - 2,
            list_area.width - 4,
            1,
        );
        let status_line = Paragraph::new("Enter: follow  y: copy  q: close")
            .style(Style::default().fg(Color::Yellow));
        frame.render_widget(status_line, status_area);

        // Preview Area
        let preview_block = Block::default()
            .title("Preview")
            .borders(Borders::ALL)
            .padding(Padding::horizontal(1));

        let mut preview_text = String::new();
        if let Some(entry) = entries.get(selected_index) {
            if let Some(target_row) = entry.target_row {
                for i in 0..8 {
                    if let Some(line) = board.get_line(target_row + i) {
                        if !preview_text.is_empty() {
                            preview_text.push('\n');
                        }
                        preview_text.push_str(line);
                    }
                }
            } else {
                preview_text = entry.url.clone();
            }
        }

        let preview = Paragraph::new(preview_text)
            .block(preview_block)
            .wrap(Wrap { trim: true });
        frame.render_widget(preview, preview_area);
    }
}