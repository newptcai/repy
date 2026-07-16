use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::Line,
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph},
};

use crate::theme::Theme;

pub struct SettingsWindow;

impl SettingsWindow {
    /// Render the settings list grouped into labelled sections.
    ///
    /// `entries` are the setting rows in flat navigation order; `sections`
    /// pairs each section title with how many consecutive `entries` it owns
    /// (the counts must sum to `entries.len()`). `selected_index` indexes into
    /// `entries`, unaffected by the interleaved section headers.
    pub fn render(
        frame: &mut Frame,
        area: Rect,
        entries: &[String],
        sections: &[(&str, usize)],
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
        let block = Block::default()
            .title("Settings")
            .borders(Borders::ALL)
            .style(theme.base_style());
        let inner = block.inner(popup_area);
        frame.render_widget(block, popup_area);
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1), Constraint::Length(1)])
            .split(inner);
        let footer = Paragraph::new("Enter: change/edit  r: reset  Sync: pull-only")
            .style(theme.base_style().fg(theme.muted_fg));

        if entries.is_empty() {
            let paragraph = Paragraph::new("No settings available")
                .style(theme.base_style().fg(theme.muted_fg));
            frame.render_widget(paragraph, rows[0]);
            frame.render_widget(footer, rows[1]);
            return;
        }

        let header_style = theme
            .base_style()
            .fg(theme.info_fg)
            .add_modifier(Modifier::BOLD);

        // Interleave a header line before each section's settings. `entry_index`
        // walks `entries`; `selected_row` records where the selected entry lands
        // in the interleaved list so the list can scroll it into view.
        let mut items: Vec<ListItem> = Vec::with_capacity(entries.len() + sections.len());
        let mut entry_index = 0;
        let mut selected_row = 0;
        let mut push_entry = |items: &mut Vec<ListItem>, entry: &str, index: usize| {
            if index == selected_index {
                selected_row = items.len();
            }
            items.push(ListItem::new(Line::from(format!("   {entry}"))).style(theme.base_style()));
        };
        for (title, count) in sections {
            if entry_index >= entries.len() {
                break;
            }
            items.push(ListItem::new(Line::from(format!(" {title}"))).style(header_style));
            for _ in 0..*count {
                let Some(entry) = entries.get(entry_index) else {
                    break;
                };
                push_entry(&mut items, entry, entry_index);
                entry_index += 1;
            }
        }
        // Any settings not claimed by a section still render, ungrouped.
        while let Some(entry) = entries.get(entry_index) {
            push_entry(&mut items, entry, entry_index);
            entry_index += 1;
        }

        let list = List::new(items).highlight_style(
            Style::default()
                .bg(theme.highlight_bg)
                .fg(theme.highlight_fg),
        );
        let mut state = ListState::default().with_selected(Some(selected_row));
        frame.render_stateful_widget(list, rows[0], &mut state);
        frame.render_widget(footer, rows[1]);
    }
}
