use ratatui::{
    Frame,
    layout::Rect,
    style::Style,
    text::Line,
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph},
};

use crate::theme::Theme;

pub struct LibraryWindow;

impl LibraryWindow {
    pub fn render(
        frame: &mut Frame,
        area: Rect,
        entries: &[String],
        selected_index: usize,
        theme: &Theme,
    ) {
        let popup_area = Rect::new(
            area.x + area.width / 6,
            area.y + area.height / 8,
            area.width * 2 / 3,
            area.height * 3 / 4,
        );

        frame.render_widget(Clear, popup_area);

        if entries.is_empty() {
            let paragraph = Paragraph::new("No history yet")
                .style(theme.base_style().fg(theme.muted_fg))
                .block(Block::default().title("Library").borders(Borders::ALL).style(theme.base_style()));
            frame.render_widget(paragraph, popup_area);
            return;
        }

        let border_block = Block::default().title("Library").borders(Borders::ALL).style(theme.base_style());
        frame.render_widget(border_block, popup_area);

        let inner_area = Rect {
            x: popup_area.x + 1,
            y: popup_area.y + 1,
            width: popup_area.width.saturating_sub(2),
            height: popup_area.height.saturating_sub(2),
        };

        let hint_area = Rect {
            x: inner_area.x,
            y: inner_area.y,
            width: inner_area.width,
            height: 1,
        };

        let hint = Paragraph::new("HINT: Press 'd' to delete.")
            .style(Style::default().fg(theme.warning_fg));
        frame.render_widget(hint, hint_area);

        let list_area = Rect {
            x: inner_area.x,
            y: inner_area.y + 2,
            width: inner_area.width,
            height: inner_area.height.saturating_sub(2),
        };

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

        let list = List::new(items);

        frame.render_widget(list, list_area);
    }
}
