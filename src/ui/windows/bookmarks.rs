use ratatui::{
    Frame,
    layout::Rect,
    style::Style,
    text::Line,
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph},
};

use crate::theme::Theme;

pub struct BookmarksWindow;

impl BookmarksWindow {
    pub fn render(
        frame: &mut Frame,
        area: Rect,
        entries: &[String],
        selected_index: usize,
        status: Option<&str>,
        theme: &Theme,
    ) {
        let popup_area = Rect::new(
            area.x + area.width / 5,
            area.y + area.height / 6,
            area.width * 3 / 5,
            area.height * 2 / 3,
        );

        frame.render_widget(Clear, popup_area);

        if entries.is_empty() {
            let paragraph = Paragraph::new("No bookmarks yet")
                .style(theme.base_style().fg(theme.muted_fg))
                .block(Block::default().title("Bookmarks").borders(Borders::ALL).style(theme.base_style()));
            frame.render_widget(paragraph, popup_area);
            return;
        }

        let items: Vec<ListItem> = entries
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

        let list = List::new(items).block(
            Block::default()
                .title("Bookmarks")
                .borders(Borders::ALL)
                .style(theme.base_style()),
        );

        frame.render_widget(list, popup_area);

        if let Some(message) = status {
            let status_area = Rect::new(
                popup_area.x + 1,
                popup_area.y + popup_area.height - 2,
                popup_area.width - 2,
                1,
            );
            let status_line =
                Paragraph::new(message).style(Style::default().fg(theme.warning_fg));
            frame.render_widget(status_line, status_area);
        }
    }
}
