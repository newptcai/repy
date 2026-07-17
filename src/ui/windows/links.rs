use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Padding, Paragraph, Wrap},
};

use crate::models::{CHAPTER_BREAK_MARKER, LinkEntry};
use crate::theme::Theme;
use crate::ui::board::Board;

pub struct LinksWindow;

impl LinksWindow {
    fn append_preview_line(preview_text: &mut String, trimmed: &str) {
        if preview_text.is_empty() || preview_text.ends_with('\n') {
            preview_text.push_str(trimmed);
            return;
        }

        if preview_text.ends_with('-')
            && preview_text
                .chars()
                .rev()
                .nth(1)
                .is_some_and(|ch| ch.is_ascii_alphabetic())
            && trimmed
                .chars()
                .next()
                .is_some_and(|ch| ch.is_ascii_lowercase())
        {
            preview_text.pop();
        } else {
            preview_text.push(' ');
        }
        preview_text.push_str(trimmed);
    }

    pub fn build_preview_text(entry: &LinkEntry, board: &Board) -> String {
        Self::build_preview_text_with_limit(entry, board, 8)
    }

    pub fn build_preview_text_with_limit(
        entry: &LinkEntry,
        board: &Board,
        line_limit: usize,
    ) -> String {
        let Some(target_row) = entry.target_row else {
            return format!("URL: {}", entry.url);
        };

        let mut preview_text = String::new();
        let mut lines_shown = 0;
        let mut offset = 0;
        while lines_shown < line_limit {
            let Some(line) = board.get_line(target_row + offset) else {
                break;
            };
            offset += 1;
            let trimmed = line.trim();
            if trimmed == CHAPTER_BREAK_MARKER {
                break;
            }
            if trimmed.is_empty() {
                if !preview_text.is_empty() && !preview_text.ends_with("\n\n") {
                    preview_text.push_str("\n\n");
                }
            } else {
                Self::append_preview_line(&mut preview_text, trimmed);
            }
            lines_shown += 1;
        }

        preview_text
    }

    pub fn render(
        frame: &mut Frame,
        area: Rect,
        entries: &[LinkEntry],
        selected_index: usize,
        board: &Board,
        theme: &Theme,
    ) {
        let popup_area = super::centered_popup_area(area, 60, 70);

        frame.render_widget(Clear, popup_area);

        if entries.is_empty() {
            let block = Block::default()
                .title("Links")
                .borders(Borders::ALL)
                .padding(Padding::horizontal(1))
                .style(theme.base_style());
            let paragraph = Paragraph::new("No links on this page")
                .style(theme.base_style().fg(theme.muted_fg))
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
            .padding(Padding::new(1, 1, 0, 1))
            .style(theme.base_style());

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

                let is_internal = entry.target_row.is_some() || !entry.url.contains("://");

                let display_text = if is_internal {
                    if entry.label.starts_with("^{") && entry.label.ends_with('}') {
                        let num = &entry.label[2..entry.label.len() - 1];
                        format!("Footnote {}", num)
                    } else if (entry.url.contains("fn") || entry.url.contains("note"))
                        && entry.label.len() <= 4
                    {
                        format!("Footnote ({})", entry.label)
                    } else {
                        entry.label.clone()
                    }
                } else if entry.label == entry.url {
                    entry.url.clone()
                } else {
                    format!("{} ({})", entry.label, entry.url)
                };

                let line = if is_internal {
                    Line::from(Span::raw(display_text))
                } else {
                    Line::from(vec![
                        Span::raw(display_text),
                        Span::styled(" ↗", Style::default().fg(theme.external_link_fg)),
                    ])
                };

                ListItem::new(line).style(style)
            })
            .collect();

        let list = List::new(items).block(block);

        let mut state = ListState::default();
        state.select(Some(selected_index));

        frame.render_stateful_widget(list, list_area, &mut state);

        // Status line in list area
        let status_area = Rect::new(
            list_area.x + 2,
            list_area.y + list_area.height - 2,
            list_area.width - 4,
            1,
        );
        let status_line = Paragraph::new("Enter: follow  y: copy  q: close")
            .style(Style::default().fg(theme.warning_fg));
        frame.render_widget(status_line, status_area);

        // Preview Area
        let preview_block = Block::default()
            .title(Span::styled(
                " Preview ",
                Style::default().add_modifier(Modifier::BOLD),
            ))
            .borders(Borders::ALL)
            .padding(Padding::horizontal(1))
            .style(theme.base_style());

        let mut preview_text = String::new();
        if let Some(entry) = entries.get(selected_index) {
            preview_text = Self::build_preview_text(entry, board);
        }

        let preview = Paragraph::new(preview_text)
            .block(preview_block)
            .wrap(Wrap { trim: true });
        frame.render_widget(preview, preview_area);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::TextStructure;

    fn board_with_lines(lines: &[&str]) -> Board {
        Board::new().with_text_structure(TextStructure {
            text_lines: lines.iter().map(|line| line.to_string()).collect(),
            ..Default::default()
        })
    }

    fn link_to(row: usize) -> LinkEntry {
        LinkEntry {
            row: 0,
            source_offset: None,
            label: "note".to_string(),
            url: "#note".to_string(),
            target_row: Some(row),
        }
    }

    #[test]
    fn preview_removes_soft_hyphenation_when_joining_wrapped_lines() {
        let board = board_with_lines(&[
            "[25]. Most of these men were famous, philosophically inspired martyrs",
            "executed under Domit-",
            "ian. It says a lot about the changed face of the imperial throne.",
        ]);

        let preview = LinksWindow::build_preview_text(&link_to(0), &board);

        assert!(preview.contains("executed under Domitian. It says"));
        assert!(!preview.contains("Domit- ian"));
    }

    #[test]
    fn preview_keeps_real_hyphenated_breaks() {
        let board = board_with_lines(&["A well-", "Known title keeps its hyphen."]);

        let preview = LinksWindow::build_preview_text(&link_to(0), &board);

        assert_eq!(preview, "A well- Known title keeps its hyphen.");
    }
}
