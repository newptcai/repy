use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph},
};

use crate::theme::Theme;

pub struct BookmarksWindow;

impl BookmarksWindow {
    pub fn render(
        frame: &mut Frame,
        area: Rect,
        title: &str,
        empty_message: &str,
        entries: &[String],
        selected_index: usize,
        status: Option<&str>,
        filter: Option<&str>,
        theme: &Theme,
    ) {
        let popup_area = Rect::new(
            area.x + area.width / 5,
            area.y + area.height / 6,
            area.width * 3 / 5,
            area.height * 2 / 3,
        );

        frame.render_widget(Clear, popup_area);

        let make_block = || {
            let mut block = Block::default()
                .title(title)
                .borders(Borders::ALL)
                .style(theme.base_style());
            if let Some(filter) = filter {
                block = block.title_bottom(Span::styled(
                    filter.to_string(),
                    Style::default().fg(theme.warning_fg),
                ));
            }
            block
        };

        if entries.is_empty() {
            let message = if filter.is_some() {
                "No matches"
            } else {
                empty_message
            };
            let paragraph = Paragraph::new(message)
                .style(theme.base_style().fg(theme.muted_fg))
                .block(make_block());
            frame.render_widget(paragraph, popup_area);
            return;
        }

        frame.render_widget(make_block(), popup_area);

        let mut list_area = Rect {
            x: popup_area.x + 1,
            y: popup_area.y + 1,
            width: popup_area.width.saturating_sub(2),
            height: popup_area.height.saturating_sub(2),
        };
        if status.is_some() {
            list_area.height = list_area.height.saturating_sub(1);
        }

        let items: Vec<ListItem> = entries
            .iter()
            .map(|entry| ListItem::new(Line::from(entry.clone())))
            .collect();

        let list = List::new(items).highlight_style(
            Style::default()
                .bg(theme.highlight_bg)
                .fg(theme.highlight_fg)
                .add_modifier(Modifier::BOLD),
        );

        let mut state = ListState::default();
        state.select(Some(selected_index));

        frame.render_stateful_widget(list, list_area, &mut state);

        if let Some(message) = status {
            let status_area = Rect::new(
                popup_area.x + 1,
                popup_area.y + popup_area.height - 2,
                popup_area.width - 2,
                1,
            );
            let status_line = Paragraph::new(message).style(Style::default().fg(theme.warning_fg));
            frame.render_widget(status_line, status_area);
        }
    }
}
