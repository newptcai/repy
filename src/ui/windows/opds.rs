use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph},
};

use crate::opds::{OpdsFeed, OpdsEntry, find_epub_link, find_nav_link};
use crate::settings::OpdsCatalogConfig;
use crate::theme::Theme;

pub struct OpdsWindow;

/// The browsing state for the OPDS window.
#[derive(Debug, Clone)]
pub struct OpdsWindowState {
    /// Configured catalogs from settings.
    pub catalogs: Vec<OpdsCatalogConfig>,
    /// The navigation stack: each level is a (url, feed) pair.
    pub nav_stack: Vec<(String, OpdsFeed)>,
    /// Index into catalogs when showing the catalog picker.
    pub catalog_selected: usize,
    /// Index in the current feed's entry list.
    pub entry_selected: usize,
    /// Scroll offset so large feeds are navigable.
    pub scroll_offset: usize,
    /// Status/error message to display at the bottom.
    pub status: Option<String>,
    /// When Some(_), the search input bar is active.
    pub search_input: Option<String>,
    /// Download dir (resolved at open time).
    pub download_dir: std::path::PathBuf,
}

impl OpdsWindowState {
    pub fn new(catalogs: Vec<OpdsCatalogConfig>, download_dir: std::path::PathBuf) -> Self {
        Self {
            catalogs,
            nav_stack: Vec::new(),
            catalog_selected: 0,
            entry_selected: 0,
            scroll_offset: 0,
            status: None,
            search_input: None,
            download_dir,
        }
    }

    /// True when we are at the top (catalog picker) level.
    pub fn at_catalog_picker(&self) -> bool {
        self.nav_stack.is_empty()
    }

    /// The current feed on the stack (if any).
    pub fn current_feed(&self) -> Option<&OpdsFeed> {
        self.nav_stack.last().map(|(_, f)| f)
    }

    /// The current feed's URL.
    pub fn current_url(&self) -> Option<&str> {
        self.nav_stack.last().map(|(u, _)| u.as_str())
    }

    /// Breadcrumb string: "Root > Science Fiction > …"
    pub fn breadcrumb(&self) -> String {
        if self.nav_stack.is_empty() {
            return "Catalogs".to_string();
        }
        self.nav_stack
            .iter()
            .map(|(_, f)| f.title.as_str())
            .collect::<Vec<_>>()
            .join(" > ")
    }

    pub fn move_up(&mut self) {
        if self.at_catalog_picker() {
            if self.catalog_selected > 0 {
                self.catalog_selected -= 1;
                self.clamp_scroll_catalog();
            }
        } else if self.entry_selected > 0 {
            self.entry_selected -= 1;
            self.clamp_scroll_entries();
        }
    }

    pub fn move_down(&mut self) {
        if self.at_catalog_picker() {
            if self.catalog_selected + 1 < self.catalogs.len() {
                self.catalog_selected += 1;
                self.clamp_scroll_catalog();
            }
        } else if let Some(feed) = self.current_feed() {
            if self.entry_selected + 1 < feed.entries.len() {
                self.entry_selected += 1;
                self.clamp_scroll_entries();
            }
        }
    }

    pub fn go_back(&mut self) {
        if !self.at_catalog_picker() {
            self.nav_stack.pop();
            self.entry_selected = 0;
            self.scroll_offset = 0;
        }
    }

    pub fn push_feed(&mut self, url: String, feed: OpdsFeed) {
        self.nav_stack.push((url, feed));
        self.entry_selected = 0;
        self.scroll_offset = 0;
        self.status = None;
    }

    pub fn selected_entry(&self) -> Option<&OpdsEntry> {
        self.current_feed()
            .and_then(|f| f.entries.get(self.entry_selected))
    }

    fn clamp_scroll_entries(&mut self) {
        // Will be adjusted relative to visible height in render; just keep entry_selected valid.
        if let Some(feed) = self.current_feed() {
            if self.entry_selected >= feed.entries.len() {
                self.entry_selected = feed.entries.len().saturating_sub(1);
            }
        }
    }

    fn clamp_scroll_catalog(&mut self) {
        if self.catalog_selected >= self.catalogs.len() {
            self.catalog_selected = self.catalogs.len().saturating_sub(1);
        }
    }
}

impl OpdsWindow {
    pub fn render(frame: &mut Frame, area: Rect, state: &OpdsWindowState, theme: &Theme) {
        let popup = Rect::new(
            area.x + area.width / 10,
            area.y + area.height / 10,
            area.width * 4 / 5,
            area.height * 4 / 5,
        );

        frame.render_widget(Clear, popup);

        // Title bar
        let title = format!(" OPDS: {} ", state.breadcrumb());
        let border = Block::default()
            .title(title.as_str())
            .borders(Borders::ALL)
            .style(theme.base_style());
        frame.render_widget(border, popup);

        let inner = Rect {
            x: popup.x + 1,
            y: popup.y + 1,
            width: popup.width.saturating_sub(2),
            height: popup.height.saturating_sub(2),
        };

        // Reserve rows: 1 hint, 1 blank, list area, 1 blank, 1 summary, 1 status
        let hint_row = inner.y;
        let list_y = inner.y + 2;
        let list_height = inner.height.saturating_sub(if state.status.is_some() { 5 } else { 4 });
        let summary_y = list_y + list_height + 1;
        let status_y = summary_y + 1;

        // Hint line
        let hint_text = if state.search_input.is_some() {
            "Type search query, Enter to search, Esc to cancel"
        } else if state.at_catalog_picker() {
            "[Enter] open catalog   [Esc] close"
        } else {
            "[Enter] open/download   [u/Backspace] back   [s] search   [n/p] next/prev page   [Esc] close"
        };
        let hint = Paragraph::new(hint_text).style(Style::default().fg(theme.muted_fg));
        frame.render_widget(hint, Rect::new(inner.x, hint_row, inner.width, 1));

        // Search input (when active)
        if let Some(ref query) = state.search_input {
            let input_area = Rect::new(inner.x, list_y, inner.width, 3);
            let input = Paragraph::new(format!("/{query}"))
                .block(
                    Block::default()
                        .title("Search")
                        .borders(Borders::ALL)
                        .style(theme.base_style()),
                )
                .style(theme.base_style().add_modifier(Modifier::BOLD));
            frame.render_widget(input, input_area);
            // Position cursor after the /
            frame.set_cursor_position((input_area.x + 1 + query.len() as u16 + 1, input_area.y + 1));
            // Summary area
            Self::render_summary(frame, state, inner, summary_y, theme);
            Self::render_status(frame, state, inner, status_y, theme);
            return;
        }

        // Main list
        let list_area = Rect::new(inner.x, list_y, inner.width, list_height);
        if state.at_catalog_picker() {
            Self::render_catalog_list(frame, state, list_area, theme);
        } else {
            Self::render_entry_list(frame, state, list_area, theme);
        }

        // Summary for selected entry
        Self::render_summary(frame, state, inner, summary_y, theme);
        // Status message
        Self::render_status(frame, state, inner, status_y, theme);
    }

    fn render_catalog_list(
        frame: &mut Frame,
        state: &OpdsWindowState,
        area: Rect,
        theme: &Theme,
    ) {
        if state.catalogs.is_empty() {
            let msg = Paragraph::new("No OPDS catalogs configured. Add entries to opds_catalogs in your config file.")
                .style(theme.base_style().fg(theme.muted_fg));
            frame.render_widget(msg, area);
            return;
        }

        let items: Vec<ListItem> = state
            .catalogs
            .iter()
            .enumerate()
            .map(|(i, cat)| {
                let style = if i == state.catalog_selected {
                    Style::default().bg(theme.highlight_bg).fg(theme.highlight_fg)
                } else {
                    Style::default()
                };
                ListItem::new(Line::from(vec![
                    Span::raw(format!("  {}", cat.name)),
                    Span::styled(
                        format!("  {}", cat.url),
                        Style::default().fg(theme.muted_fg),
                    ),
                ]))
                .style(style)
            })
            .collect();

        let visible_start = state.catalog_selected.saturating_sub(area.height as usize / 2);
        let visible_items: Vec<ListItem> = items
            .into_iter()
            .skip(visible_start)
            .take(area.height as usize)
            .collect();

        frame.render_widget(List::new(visible_items), area);
    }

    fn render_entry_list(
        frame: &mut Frame,
        state: &OpdsWindowState,
        area: Rect,
        theme: &Theme,
    ) {
        let Some(feed) = state.current_feed() else {
            return;
        };

        if feed.entries.is_empty() {
            let msg = Paragraph::new("No entries in this feed.")
                .style(theme.base_style().fg(theme.muted_fg));
            frame.render_widget(msg, area);
            return;
        }

        let items: Vec<ListItem> = feed
            .entries
            .iter()
            .enumerate()
            .map(|(i, entry)| {
                let selected = i == state.entry_selected;
                let style = if selected {
                    Style::default().bg(theme.highlight_bg).fg(theme.highlight_fg)
                } else {
                    Style::default()
                };

                let epub_tag = if find_epub_link(entry).is_some() {
                    Span::styled(" [EPUB]", Style::default().fg(theme.muted_fg))
                } else if find_nav_link(entry).is_some() {
                    Span::styled(" [->]", Style::default().fg(theme.muted_fg))
                } else {
                    Span::raw("")
                };

                let author_str = if entry.authors.is_empty() {
                    String::new()
                } else {
                    format!("  — {}", entry.authors.join(", "))
                };

                let width = area.width as usize;
                let title_max = width.saturating_sub(author_str.len() + 8);
                let title = if entry.title.len() > title_max {
                    format!("{}…", &entry.title[..title_max.saturating_sub(1)])
                } else {
                    entry.title.clone()
                };

                ListItem::new(Line::from(vec![
                    Span::raw(format!("  {title}")),
                    Span::styled(author_str, Style::default().fg(theme.muted_fg)),
                    epub_tag,
                ]))
                .style(style)
            })
            .collect();

        let visible_start = state.entry_selected.saturating_sub(area.height as usize / 2);
        let visible_items: Vec<ListItem> = items
            .into_iter()
            .skip(visible_start)
            .take(area.height as usize)
            .collect();

        frame.render_widget(List::new(visible_items), area);
    }

    fn render_summary(
        frame: &mut Frame,
        state: &OpdsWindowState,
        inner: Rect,
        y: u16,
        theme: &Theme,
    ) {
        if y >= inner.y + inner.height {
            return;
        }
        let summary_area = Rect::new(inner.x, y, inner.width, 1);
        let summary_text = if state.at_catalog_picker() {
            state
                .catalogs
                .get(state.catalog_selected)
                .map(|c| c.url.clone())
                .unwrap_or_default()
        } else {
            state
                .selected_entry()
                .and_then(|e| e.summary.clone())
                .unwrap_or_default()
        };

        // Truncate to fit
        let max = inner.width as usize;
        let display = if summary_text.len() > max {
            format!("{}…", &summary_text[..max.saturating_sub(1)])
        } else {
            summary_text
        };

        let para = Paragraph::new(display).style(Style::default().fg(theme.muted_fg));
        frame.render_widget(para, summary_area);
    }

    fn render_status(
        frame: &mut Frame,
        state: &OpdsWindowState,
        inner: Rect,
        y: u16,
        theme: &Theme,
    ) {
        if let Some(ref msg) = state.status {
            if y >= inner.y + inner.height {
                return;
            }
            let status_area = Rect::new(inner.x, y, inner.width, 1);
            let style = if msg.starts_with("Error") || msg.starts_with("HTTP") {
                Style::default().fg(theme.warning_fg)
            } else {
                Style::default().fg(theme.muted_fg)
            };
            frame.render_widget(Paragraph::new(msg.as_str()).style(style), status_area);
        }
    }
}
