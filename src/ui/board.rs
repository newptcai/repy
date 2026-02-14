use ratatui::{
    Frame,
    layout::{Alignment, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Paragraph, Wrap},
};

use crate::models::{CHAPTER_BREAK_MARKER, InlineStyle, LinkEntry, TextStructure};
use crate::ui::reader::ApplicationState;

/// Board widget for rendering book text content
pub struct Board {
    text_structure: Option<TextStructure>,
}

impl Board {
    pub fn new() -> Self {
        Self {
            text_structure: None,
        }
    }

    pub fn with_text_structure(mut self, text_structure: TextStructure) -> Self {
        self.text_structure = Some(text_structure);
        self
    }

    pub fn render(
        &self,
        frame: &mut Frame,
        area: Rect,
        state: &ApplicationState,
        content_start_rows: Option<&[usize]>,
    ) {
        if let Some(ref text_structure) = self.text_structure {
            self.render_content(frame, area, text_structure, state, content_start_rows);
        } else {
            self.render_empty(frame, area);
        }
    }

    fn render_content(
        &self,
        frame: &mut Frame,
        area: Rect,
        text_structure: &TextStructure,
        state: &ApplicationState,
        content_start_rows: Option<&[usize]>,
    ) {
        let height = area.height as usize;
        let _width = area.width as usize;

        let mut start_line = state.reading_state.row.saturating_sub(1);
        let mut chapter_end = text_structure.text_lines.len().saturating_sub(1);
        if let Some(content_start_rows) = content_start_rows {
            if content_start_rows
                .binary_search(&state.reading_state.row)
                .is_ok()
            {
                // Avoid showing the previous chapter's trailing line at chapter boundaries.
                start_line = state.reading_state.row;
            }
            if !state.config.settings.seamless_between_chapters && !content_start_rows.is_empty() {
                let mut index = 0;
                for (i, start) in content_start_rows.iter().enumerate() {
                    if *start <= state.reading_state.row {
                        index = i;
                    } else {
                        break;
                    }
                }
                let chapter_start = content_start_rows[index];
                chapter_end = if index + 1 < content_start_rows.len() {
                    content_start_rows[index + 1].saturating_sub(1)
                } else {
                    text_structure.text_lines.len().saturating_sub(1)
                };
                if start_line < chapter_start {
                    start_line = chapter_start;
                }
            }
        }
        let end_line = (start_line + height).min(text_structure.text_lines.len());
        let end_line = if state.config.settings.seamless_between_chapters {
            end_line
        } else {
            end_line.min(chapter_end.saturating_add(1))
        };

        let selection_range: Option<((usize, usize), (usize, usize))> =
            match (state.ui_state.visual_anchor, state.ui_state.visual_cursor) {
                (Some(anchor), Some(cursor)) => {
                    if anchor <= cursor {
                        Some((anchor, cursor))
                    } else {
                        Some((cursor, anchor))
                    }
                }
                _ => None,
            };
        let cursor_pos = state.ui_state.visual_cursor;
        let formatting = &text_structure.formatting;

        let visible_lines: Vec<Line> = text_structure
            .text_lines
            .get(start_line..end_line)
            .unwrap_or(&[])
            .iter()
            .enumerate()
            .map(|(i, line)| {
                let line_num = start_line + i;
                let mut spans = Vec::new();

                if line == CHAPTER_BREAK_MARKER {
                    return Line::raw("***").alignment(Alignment::Center);
                }

                if text_structure.image_maps.contains_key(&line_num) {
                    return Line::raw(line).alignment(Alignment::Center);
                }

                if state.config.settings.show_line_numbers {
                    spans.push(Span::styled(
                        format!("{:>4} ", line_num + 1),
                        Style::default().fg(Color::DarkGray),
                    ));
                }

                if let Some(((sel_start_row, sel_start_col), (sel_end_row, sel_end_col))) =
                    selection_range
                {
                    if line_num >= sel_start_row && line_num <= sel_end_row {
                        let line_len = line.chars().count();
                        let sel_col_start = if line_num == sel_start_row {
                            sel_start_col.min(line_len)
                        } else {
                            0
                        };
                        let sel_col_end = if line_num == sel_end_row {
                            sel_end_col.saturating_add(1).min(line_len)
                        } else {
                            line_len
                        };
                        spans.extend(
                            self.build_visual_selection_spans(
                                line,
                                sel_col_start,
                                sel_col_end,
                                cursor_pos
                                    .filter(|(cursor_row, _)| *cursor_row == line_num)
                                    .map(|(_, cursor_col)| cursor_col),
                            ),
                        );
                        return Line::from(spans);
                    }
                }

                // Cursor-only mode: show block cursor on single character
                // (visual_cursor is Some but visual_anchor is None)
                if selection_range.is_none() {
                    if let Some((cursor_row, cursor_col)) = cursor_pos {
                        if state.ui_state.visual_anchor.is_none() && line_num == cursor_row {
                            let chars: Vec<char> = line.chars().collect();
                            let col = cursor_col.min(chars.len().saturating_sub(1));
                            if col > 0 {
                                spans.push(Span::raw(
                                    chars[..col].iter().collect::<String>(),
                                ));
                            }
                            // Blinking block cursor character
                            let cursor_style = Style::default()
                                .add_modifier(Modifier::REVERSED)
                                .add_modifier(Modifier::SLOW_BLINK);
                            if !chars.is_empty() {
                                spans.push(Span::styled(
                                    chars[col..=col].iter().collect::<String>(),
                                    cursor_style,
                                ));
                            } else {
                                // Use non-breaking space so Wrap{trim:true} doesn't strip it
                                spans.push(Span::styled(
                                    "\u{00A0}".to_string(),
                                    cursor_style,
                                ));
                            }
                            if col + 1 < chars.len() {
                                spans.push(Span::raw(
                                    chars[col + 1..].iter().collect::<String>(),
                                ));
                            }
                            return Line::from(spans);
                        }
                    }
                }

                let line_spans = self.build_line_spans(
                    line,
                    line_num,
                    Style::default(),
                    formatting,
                    state
                        .ui_state
                        .search_matches
                        .get(&line_num)
                        .map(|ranges| ranges.as_slice()),
                );
                spans.extend(line_spans);
                Line::from(spans)
            })
            .collect();

        let paragraph = Paragraph::new(visible_lines)
            .wrap(Wrap { trim: true })
            .block(Block::default());

        frame.render_widget(paragraph, area);
    }

    fn build_visual_selection_spans(
        &self,
        line: &str,
        sel_col_start: usize,
        sel_col_end: usize,
        cursor_col: Option<usize>,
    ) -> Vec<Span<'static>> {
        let chars: Vec<char> = line.chars().collect();
        let line_len = chars.len();
        let start = sel_col_start.min(line_len);
        let end = sel_col_end.min(line_len);
        let mut spans = Vec::new();
        let base_style = Style::default();
        let selected_style = base_style.add_modifier(Modifier::REVERSED);

        if start > 0 {
            spans.push(Span::styled(
                chars[..start].iter().collect::<String>(),
                base_style,
            ));
        }

        if start < end {
            if let Some(cursor_col) = cursor_col {
                if cursor_col >= start && cursor_col < end {
                    if start < cursor_col {
                        spans.push(Span::styled(
                            chars[start..cursor_col].iter().collect::<String>(),
                            selected_style,
                        ));
                    }
                    spans.push(Span::styled(
                        chars[cursor_col..=cursor_col].iter().collect::<String>(),
                        selected_style.add_modifier(Modifier::UNDERLINED),
                    ));
                    if cursor_col + 1 < end {
                        spans.push(Span::styled(
                            chars[cursor_col + 1..end].iter().collect::<String>(),
                            selected_style,
                        ));
                    }
                } else {
                    spans.push(Span::styled(
                        chars[start..end].iter().collect::<String>(),
                        selected_style,
                    ));
                }
            } else {
                spans.push(Span::styled(
                    chars[start..end].iter().collect::<String>(),
                    selected_style,
                ));
            }
        }

        if end < line_len {
            spans.push(Span::styled(
                chars[end..].iter().collect::<String>(),
                base_style,
            ));
        }

        if spans.is_empty() {
            spans.push(Span::styled(String::new(), base_style));
        }

        spans
    }

    fn render_empty(&self, frame: &mut Frame, area: Rect) {
        let empty_text = vec![
            Line::from("No content loaded"),
            Line::from("Open a book to start reading"),
        ];

        let paragraph = Paragraph::new(empty_text)
            .style(
                Style::default()
                    .fg(Color::DarkGray)
                    .add_modifier(Modifier::ITALIC),
            )
            .wrap(Wrap { trim: true });

        frame.render_widget(paragraph, area);
    }

    fn build_line_spans(
        &self,
        line: &str,
        line_num: usize,
        base_style: Style,
        formatting: &[InlineStyle],
        search_ranges: Option<&[(usize, usize)]>,
    ) -> Vec<Span<'_>> {
        if line.is_empty() {
            return vec![Span::styled(String::new(), base_style)];
        }

        let mut points = vec![0, line.len()];
        let line_formatting: Vec<&InlineStyle> = formatting
            .iter()
            .filter(|style| style.row as usize == line_num)
            .collect();

        for style in &line_formatting {
            points.push(style.col as usize);
            points.push((style.col + style.n_letters) as usize);
        }

        if let Some(ranges) = search_ranges {
            for (start, end) in ranges {
                points.push(*start);
                points.push(*end);
            }
        }

        points.retain(|pos| *pos <= line.len());
        points.sort_unstable();
        points.dedup();

        let mut spans = Vec::new();
        for window in points.windows(2) {
            let start = window[0];
            let end = window[1];
            if start >= end {
                continue;
            }
            let Some(segment) = line.get(start..end) else {
                continue;
            };

            let mut style = base_style;
            if let Some(ranges) = search_ranges
                && ranges
                    .iter()
                    .any(|(range_start, range_end)| start >= *range_start && end <= *range_end)
            {
                style = style.fg(Color::Black).bg(Color::Yellow);
            }

            for inline in &line_formatting {
                let inline_start = inline.col as usize;
                let inline_end = inline_start.saturating_add(inline.n_letters as usize);
                if start >= inline_start && end <= inline_end {
                    match inline.attr {
                        1 => {
                            style = style.add_modifier(Modifier::BOLD);
                        }
                        2 => {
                            style = style.add_modifier(Modifier::ITALIC);
                        }
                        _ => {}
                    }
                }
            }

            spans.push(Span::styled(segment.to_string(), style));
        }

        spans
    }

    pub fn total_lines(&self) -> usize {
        self.text_structure
            .as_ref()
            .map(|ts| ts.text_lines.len())
            .unwrap_or(0)
    }

    pub fn is_valid_line(&self, line: usize) -> bool {
        line < self.total_lines()
    }

    pub fn get_line(&self, line: usize) -> Option<&str> {
        self.text_structure
            .as_ref()
            .and_then(|ts| ts.text_lines.get(line).map(String::as_str))
    }

    pub fn lines(&self) -> Option<&[String]> {
        self.text_structure
            .as_ref()
            .map(|ts| ts.text_lines.as_slice())
    }

    pub fn text_structure_ref(&self) -> Option<&TextStructure> {
        self.text_structure.as_ref()
    }

    pub fn line_char_count(&self, row: usize) -> usize {
        self.text_structure
            .as_ref()
            .and_then(|ts| ts.text_lines.get(row))
            .map(|line| line.chars().count())
            .unwrap_or(0)
    }

    pub fn link_count_in_range(&self, start: usize, end: usize) -> usize {
        self.text_structure
            .as_ref()
            .map(|ts| {
                ts.links
                    .iter()
                    .filter(|link| link.row >= start && link.row < end)
                    .count()
            })
            .unwrap_or(0)
    }

    pub fn links_in_range(&self, start: usize, end: usize) -> Vec<LinkEntry> {
        self.text_structure
            .as_ref()
            .map(|ts| {
                ts.links
                    .iter()
                    .filter(|link| link.row >= start && link.row < end)
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn section_rows(&self) -> Option<&std::collections::HashMap<String, usize>> {
        self.text_structure.as_ref().map(|ts| &ts.section_rows)
    }

    pub fn section_row(&self, id: &str) -> Option<usize> {
        self.text_structure
            .as_ref()
            .and_then(|ts| ts.section_rows.get(id).copied())
    }

    pub fn image_src(&self, line: usize) -> Option<String> {
        self.text_structure
            .as_ref()
            .and_then(|ts| ts.image_maps.get(&line).cloned())
    }

    pub fn get_selected_text_range(&self, start: (usize, usize), end: (usize, usize)) -> String {
        let Some(text_structure) = &self.text_structure else {
            return String::new();
        };
        let (start, end) = if start <= end {
            (start, end)
        } else {
            (end, start)
        };
        let (start_row, start_col) = start;
        let (end_row, end_col) = end;
        if start_row >= text_structure.text_lines.len()
            || end_row >= text_structure.text_lines.len()
        {
            return String::new();
        }

        if start_row == end_row {
            let chars: Vec<char> = text_structure.text_lines[start_row].chars().collect();
            if chars.is_empty() || start_col >= chars.len() {
                return String::new();
            }
            let capped_end = end_col.min(chars.len().saturating_sub(1));
            if start_col > capped_end {
                return String::new();
            }
            return chars[start_col..=capped_end].iter().collect();
        }

        let mut result = String::new();
        for row in start_row..=end_row {
            let chars: Vec<char> = text_structure.text_lines[row].chars().collect();
            if row == start_row {
                if start_col < chars.len() {
                    result.extend(chars[start_col..].iter());
                }
                result.push('\n');
            } else if row == end_row {
                if !chars.is_empty() {
                    let capped_end = end_col.min(chars.len().saturating_sub(1));
                    result.extend(chars[..=capped_end].iter());
                }
            } else {
                result.extend(chars.iter());
                result.push('\n');
            }
        }
        result
    }

    pub fn update_text_structure(&mut self, text_structure: TextStructure) {
        self.text_structure = Some(text_structure);
    }
}

impl Default for Board {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::TextStructure;
    use std::collections::HashMap;

    #[test]
    fn test_board_new() {
        let board = Board::new();
        assert!(board.text_structure.is_none());
    }

    #[test]
    fn test_board_builder() {
        let text_structure = TextStructure {
            text_lines: vec!["Line 1".to_string(), "Line 2".to_string()],
            image_maps: HashMap::new(),
            section_rows: HashMap::new(),
            formatting: vec![],
            links: vec![],
        };

        let board = Board::new().with_text_structure(text_structure.clone());

        assert!(board.text_structure.is_some());
    }

    #[test]
    fn test_board_total_lines() {
        let mut board = Board::new();
        assert_eq!(board.total_lines(), 0);

        let text_structure = TextStructure {
            text_lines: vec![
                "Line 1".to_string(),
                "Line 2".to_string(),
                "Line 3".to_string(),
            ],
            image_maps: HashMap::new(),
            section_rows: HashMap::new(),
            formatting: vec![],
            links: vec![],
        };

        board.update_text_structure(text_structure);
        assert_eq!(board.total_lines(), 3);
    }

    #[test]
    fn test_board_is_valid_line() {
        let mut board = Board::new();
        assert!(!board.is_valid_line(0));

        let text_structure = TextStructure {
            text_lines: vec!["Line 1".to_string(), "Line 2".to_string()],
            image_maps: HashMap::new(),
            section_rows: HashMap::new(),
            formatting: vec![],
            links: vec![],
        };

        board.update_text_structure(text_structure);
        assert!(board.is_valid_line(0));
        assert!(board.is_valid_line(1));
        assert!(!board.is_valid_line(2));
    }

    #[test]
    fn test_board_get_line() {
        let mut board = Board::new();
        assert_eq!(board.get_line(0), None);

        let text_structure = TextStructure {
            text_lines: vec!["Line 1".to_string(), "Line 2".to_string()],
            image_maps: HashMap::new(),
            section_rows: HashMap::new(),
            formatting: vec![],
            links: vec![],
        };

        board.update_text_structure(text_structure);
        assert_eq!(board.get_line(0), Some("Line 1"));
        assert_eq!(board.get_line(1), Some("Line 2"));
        assert_eq!(board.get_line(2), None);
    }

    #[test]
    fn test_board_update_methods() {
        let mut board = Board::new();

        let text_structure = TextStructure {
            text_lines: vec!["New line".to_string()],
            image_maps: HashMap::new(),
            section_rows: HashMap::new(),
            formatting: vec![],
            links: vec![],
        };

        board.update_text_structure(text_structure.clone());
        assert_eq!(board.total_lines(), 1);
    }

    #[test]
    fn test_board_default() {
        let board = Board::default();
        assert!(board.text_structure.is_none());
    }
}
