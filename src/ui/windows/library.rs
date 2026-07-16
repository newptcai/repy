use ratatui::{
    Frame,
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Wrap},
};
use ratatui_image::{Resize, StatefulImage, protocol::StatefulProtocol};

use crate::models::{LibraryEntry, LibrarySortMode};
use crate::theme::Theme;

/// Minimum popup width before metadata is placed beside the book list.
const DETAILS_SIDE_MIN_POPUP_WIDTH: u16 = 50;

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
        details: Option<&LibraryEntry>,
        cover: Option<&mut StatefulProtocol>,
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

        let hint =
            Paragraph::new("HINT: Enter open  c details  f format  d delete  R refresh  s sort")
                .style(Style::default().fg(theme.warning_fg));
        frame.render_widget(hint, hint_area);

        let mut list_area = Rect {
            x: inner_area.x,
            y: inner_area.y + 2,
            width: inner_area.width,
            height: inner_area.height.saturating_sub(2),
        };

        if let Some(entry) = details
            && popup_area.width >= DETAILS_SIDE_MIN_POPUP_WIDTH
        {
            let panel_width = (inner_area.width * 2 / 5).clamp(24, 38);
            list_area.width = list_area.width.saturating_sub(panel_width);
            let panel_area = Rect {
                x: list_area.x + list_area.width,
                y: list_area.y,
                width: panel_width,
                height: list_area.height,
            };
            let panel_block = Block::default()
                .title(" Details ")
                .borders(Borders::LEFT)
                .style(theme.base_style());
            let panel_inner = panel_block.inner(panel_area);
            frame.render_widget(panel_block, panel_area);
            Self::render_details(frame, panel_inner, entry, cover, theme);
        } else if let Some(entry) = details {
            // On narrow terminals, retain most of the popup for the list and
            // show a compact metadata strip below it.
            let details_height = (list_area.height / 3).clamp(4, 8);
            list_area.height = list_area.height.saturating_sub(details_height);
            let details_area = Rect {
                x: list_area.x,
                y: list_area.y + list_area.height,
                width: list_area.width,
                height: details_height,
            };
            Self::render_details(frame, details_area, entry, None, theme);
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

        let list = List::new(items);

        frame.render_widget(list, list_area);
    }

    fn render_details(
        frame: &mut Frame,
        area: Rect,
        entry: &LibraryEntry,
        cover: Option<&mut StatefulProtocol>,
        theme: &Theme,
    ) {
        let mut text_area = area;
        if let Some(protocol) = cover {
            let max_cover_height = area.height.saturating_mul(2) / 5;
            let bounds = ratatui::layout::Size::new(area.width, max_cover_height);
            let fitted = protocol.size_for(Resize::Fit(None), bounds);
            let cover_area = Rect::new(
                area.x + area.width.saturating_sub(fitted.width) / 2,
                area.y,
                fitted.width.min(area.width),
                fitted.height.min(max_cover_height),
            );
            frame.render_stateful_widget(StatefulImage::default(), cover_area, protocol);
            text_area.y = cover_area.y + cover_area.height + 1;
            text_area.height = area.bottom().saturating_sub(text_area.y);
        }

        let mut lines = Vec::new();
        let title = entry.title.as_deref().unwrap_or_else(|| {
            std::path::Path::new(&entry.filepath)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("Unknown title")
        });
        lines.push(Line::styled(title, Style::default().fg(theme.warning_fg)));
        if let Some(author) = &entry.author {
            lines.push(Line::from(format!("Author: {author}")));
        }
        if let Some(series) = &entry.series {
            let index = entry
                .series_index
                .map(|n| format!(" #{n}"))
                .unwrap_or_default();
            lines.push(Line::from(format!("Series: {series}{index}")));
        }
        if !entry.tags.is_empty() {
            lines.push(Line::from(format!("Tags: {}", entry.tags.join(", "))));
        }
        let formats = entry
            .formats
            .iter()
            .filter_map(|path| std::path::Path::new(path).extension()?.to_str())
            .map(str::to_uppercase)
            .collect::<Vec<_>>();
        if !formats.is_empty() {
            lines.push(Line::from(format!("Formats: {}", formats.join(", "))));
        }
        if let Some(language) = &entry.language {
            lines.push(Line::from(format!("Language: {language}")));
        }
        if let Some(publisher) = &entry.publisher {
            lines.push(Line::from(format!("Publisher: {publisher}")));
        }
        if let Some(description) = &entry.description {
            lines.push(Line::from(""));
            lines.push(Line::from(description.as_str()));
        }
        frame.render_widget(
            Paragraph::new(lines)
                .style(theme.base_style())
                .wrap(Wrap { trim: true }),
            text_area,
        );
    }
}
