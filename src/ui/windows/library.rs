use ratatui::{
    Frame,
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph},
};

use crate::models::LibrarySortMode;
use crate::theme::Theme;

pub struct LibraryWindow;

impl LibraryWindow {
    #[allow(clippy::too_many_arguments)]
    pub fn render(
        frame: &mut Frame,
        area: Rect,
        entries: &[String],
        selected_index: usize,
        filter: Option<&str>,
        sort_mode: LibrarySortMode,
        scanning: bool,
        theme: &Theme,
    ) {
        let popup_area = Rect::new(
            area.x + area.width / 6,
            area.y + area.height / 8,
            area.width * 2 / 3,
            area.height * 3 / 4,
        );

        frame.render_widget(Clear, popup_area);

        let title = if scanning {
            format!("Library — by {} (scanning…)", sort_mode.label())
        } else {
            format!("Library — by {}", sort_mode.label())
        };

        let make_block = || {
            let mut block = Block::default()
                .title(title.clone())
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
            } else if scanning {
                "Scanning library directories…"
            } else {
                "No books yet — open an EPUB or set library_directories in the config"
            };
            let paragraph = Paragraph::new(message)
                .style(theme.base_style().fg(theme.muted_fg))
                .block(make_block());
            frame.render_widget(paragraph, popup_area);
            return;
        }

        frame.render_widget(make_block(), popup_area);

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

        let hint = Paragraph::new("HINT: Enter: open  d: delete  s: sort  /: filter")
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
