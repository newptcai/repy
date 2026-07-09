use ratatui::{
    Frame,
    layout::{Alignment, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Paragraph, Wrap},
};

use crate::models::{CHAPTER_BREAK_MARKER, HighlightRange, InlineStyle, LinkEntry, TextStructure};
use crate::theme::Theme;
use crate::ui::reader::ApplicationState;

/// Board widget for rendering book text content
pub struct Board {
    text_structure: Option<TextStructure>,
    /// Cumulative word counts: `word_prefix_sums[i]` is the number of words
    /// in `text_lines[..i]`, so range word counts are O(1) lookups.
    word_prefix_sums: Vec<usize>,
}

impl Board {
    pub fn new() -> Self {
        Self {
            text_structure: None,
            word_prefix_sums: Vec::new(),
        }
    }

    pub fn with_text_structure(mut self, text_structure: TextStructure) -> Self {
        self.text_structure = Some(text_structure);
        self.rebuild_word_prefix_sums();
        self
    }

    pub fn render(
        &self,
        frame: &mut Frame,
        area: Rect,
        state: &ApplicationState,
        content_start_rows: Option<&[usize]>,
        theme: &Theme,
    ) {
        if let Some(ref text_structure) = self.text_structure {
            self.render_content(
                frame,
                area,
                text_structure,
                state,
                content_start_rows,
                theme,
            );
        } else {
            self.render_empty(frame, area, theme);
        }
    }

    fn render_content(
        &self,
        frame: &mut Frame,
        area: Rect,
        text_structure: &TextStructure,
        state: &ApplicationState,
        content_start_rows: Option<&[usize]>,
        theme: &Theme,
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

        // Build per-line byte-range lists that overlay the visual-mode
        // `/`-search matches on top of the existing reader-mode search matches.
        // Visual-mode matches are stored in char coordinates and may span
        // multiple lines, so we slice them down to each visible line.
        let visual_match_ranges: Vec<Vec<(usize, usize)>> = (start_line..end_line)
            .map(|line_num| {
                let line_str = text_structure
                    .text_lines
                    .get(line_num)
                    .map(String::as_str)
                    .unwrap_or("");
                let line_chars = line_str.chars().count();
                let mut ranges: Vec<(usize, usize)> = Vec::new();
                for (s_line, s_col, e_line, e_col) in
                    state.ui_state.visual_search_matches.iter().copied()
                {
                    if line_num < s_line || line_num > e_line {
                        continue;
                    }
                    let start_char = if line_num == s_line { s_col } else { 0 };
                    let end_char = if line_num == e_line { e_col } else { line_chars };
                    let start_char = start_char.min(line_chars);
                    let end_char = end_char.min(line_chars);
                    if start_char >= end_char {
                        continue;
                    }
                    let start_byte = char_col_to_byte(line_str, start_char);
                    let end_byte = char_col_to_byte(line_str, end_char);
                    ranges.push((start_byte, end_byte));
                }
                ranges
            })
            .collect();

        // The line holding the currently selected search hit gets a distinct
        // style so n/p navigation is easy to follow.
        let current_hit_line: Option<usize> = state
            .ui_state
            .search_results
            .get(state.ui_state.selected_search_result)
            .map(|result| result.line);

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

                // Margin indicator: a colored bar on rows covered by a
                // highlight. The 1-col gutter is reserved for the whole book
                // as soon as it has any highlight, so lines never shift.
                if !state.ui_state.highlights.is_empty() {
                    let marker = state
                        .ui_state
                        .highlight_ranges
                        .get(&line_num)
                        .and_then(|ranges| ranges.first());
                    spans.push(match marker {
                        Some(range) => Span::styled(
                            "▎",
                            Style::default().fg(theme.annotation_bg(range.color)),
                        ),
                        None => Span::raw(" "),
                    });
                }

                if state.config.settings.show_line_numbers {
                    spans.push(Span::styled(
                        format!("{:>4} ", line_num + 1),
                        Style::default().fg(theme.muted_fg),
                    ));
                }

                // Merge reader-mode search matches with visual-mode `/`-search
                // matches for this line.
                let combined_search_ranges: Vec<(usize, usize)> = {
                    let mut merged: Vec<(usize, usize)> = state
                        .ui_state
                        .search_matches
                        .get(&line_num)
                        .map(|ranges| ranges.clone())
                        .unwrap_or_default();
                    if let Some(extra) = visual_match_ranges.get(line_num - start_line) {
                        merged.extend(extra.iter().copied());
                    }
                    merged
                };
                let search_ranges_arg: Option<&[(usize, usize)]> = if combined_search_ranges
                    .is_empty()
                {
                    None
                } else {
                    Some(combined_search_ranges.as_slice())
                };

                // Check for TTS character-level underline on this line
                let tts_col_range = state.ui_state.tts_underline_ranges.get(&line_num);
                let line_spans = if let Some(&(tts_start_col, tts_end_col)) = tts_col_range {
                    // Build spans with partial underline for the TTS range
                    let line_spans = self.build_line_spans(
                        line,
                        line_num,
                        Style::default(),
                        formatting,
                        state
                            .ui_state
                            .highlight_ranges
                            .get(&line_num)
                            .map(|ranges| ranges.as_slice()),
                        search_ranges_arg,
                        current_hit_line == Some(line_num),
                        theme,
                    );
                    // Apply underline to the character range within the spans
                    Self::apply_underline_range(line_spans, tts_start_col, tts_end_col)
                } else {
                    self.build_line_spans(
                        line,
                        line_num,
                        Style::default(),
                        formatting,
                        state
                            .ui_state
                            .highlight_ranges
                            .get(&line_num)
                            .map(|ranges| ranges.as_slice()),
                        search_ranges_arg,
                        current_hit_line == Some(line_num),
                        theme,
                    )
                    .into_iter()
                    .map(|span| Span::styled(span.content.to_string(), span.style))
                    .collect()
                };

                let line_is_empty = line.is_empty();
                let cursor_on_line = cursor_pos
                    .map(|(cursor_row, _)| cursor_row == line_num)
                    .unwrap_or(false);

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
                        if line_is_empty && cursor_on_line {
                            spans.push(Self::empty_line_cursor_span());
                        } else {
                            spans.extend(Self::apply_visual_selection_range(
                                line_spans,
                                sel_col_start,
                                sel_col_end,
                                cursor_pos
                                    .filter(|(cursor_row, _)| *cursor_row == line_num)
                                    .map(|(_, cursor_col)| cursor_col),
                            ));
                        }
                        return Line::from(spans);
                    }
                }

                if selection_range.is_none()
                    && let Some((cursor_row, cursor_col)) = cursor_pos
                    && state.ui_state.visual_anchor.is_none()
                    && line_num == cursor_row
                {
                    if line_is_empty {
                        spans.push(Self::empty_line_cursor_span());
                    } else {
                        spans.extend(Self::apply_cursor_range(line_spans, cursor_col));
                    }
                    return Line::from(spans);
                }

                spans.extend(line_spans);
                Line::from(spans)
            })
            .collect();

        let paragraph = Paragraph::new(visible_lines)
            .wrap(Wrap { trim: true })
            .block(Block::default());

        frame.render_widget(paragraph, area);
    }

    fn apply_visual_selection_range(
        spans: Vec<Span<'static>>,
        sel_col_start: usize,
        sel_col_end: usize,
        cursor_col: Option<usize>,
    ) -> Vec<Span<'static>> {
        Self::map_span_char_ranges(spans, |start, end, style| {
            let mut style = style;
            if start < sel_col_end && end > sel_col_start {
                style = style.add_modifier(Modifier::REVERSED);
            }
            if let Some(cursor_col) = cursor_col
                && start <= cursor_col
                && cursor_col < end
            {
                style = style.add_modifier(Modifier::UNDERLINED);
            }
            style
        })
    }

    fn empty_line_cursor_span() -> Span<'static> {
        Span::styled(
            "\u{00A0}".to_string(),
            Style::default()
                .add_modifier(Modifier::REVERSED)
                .add_modifier(Modifier::SLOW_BLINK),
        )
    }

    fn apply_cursor_range(spans: Vec<Span<'static>>, cursor_col: usize) -> Vec<Span<'static>> {
        if spans.is_empty() || spans.iter().all(|s| s.content.is_empty()) {
            return vec![Self::empty_line_cursor_span()];
        }

        Self::map_span_char_ranges(spans, |start, end, style| {
            if start <= cursor_col && cursor_col < end {
                style
                    .add_modifier(Modifier::REVERSED)
                    .add_modifier(Modifier::SLOW_BLINK)
            } else {
                style
            }
        })
    }

    fn map_span_char_ranges<F>(spans: Vec<Span<'static>>, mut style_for: F) -> Vec<Span<'static>>
    where
        F: FnMut(usize, usize, Style) -> Style,
    {
        let mut result = Vec::new();
        let mut char_pos = 0usize;
        for span in spans {
            let span_text = span.content.to_string();
            let chars: Vec<char> = span_text.chars().collect();
            let mut local_start = 0usize;
            while local_start < chars.len() {
                let start = char_pos + local_start;
                let end = start + 1;
                let style = style_for(start, end, span.style);
                result.push(Span::styled(chars[local_start].to_string(), style));
                local_start += 1;
            }
            if chars.is_empty() {
                result.push(Span::styled(span_text, span.style));
            }
            char_pos += chars.len();
        }
        result
    }

    fn render_empty(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let empty_text = vec![
            Line::from("No content loaded"),
            Line::from("Open a book to start reading"),
        ];

        let paragraph = Paragraph::new(empty_text)
            .style(
                Style::default()
                    .fg(theme.muted_fg)
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
        highlight_ranges: Option<&[HighlightRange]>,
        search_ranges: Option<&[(usize, usize)]>,
        is_current_hit: bool,
        theme: &Theme,
    ) -> Vec<Span<'_>> {
        if line.is_empty() {
            return vec![Span::styled(String::new(), base_style)];
        }

        let chars: Vec<char> = line.chars().collect();
        let line_len = chars.len();
        let mut points = vec![0, line_len];
        let line_formatting: Vec<&InlineStyle> = formatting
            .iter()
            .filter(|style| style.row as usize == line_num)
            .collect();

        for style in &line_formatting {
            points.push(style.col as usize);
            points.push((style.col + style.n_letters) as usize);
        }

        if let Some(ranges) = highlight_ranges {
            for range in ranges {
                points.push(range.start_col);
                points.push(range.end_col);
            }
        }

        if let Some(ranges) = search_ranges {
            for (start, end) in ranges {
                points.push(byte_to_char_col(line, *start));
                points.push(byte_to_char_col(line, *end));
            }
        }

        points.retain(|pos| *pos <= line_len);
        points.sort_unstable();
        points.dedup();

        let mut spans = Vec::new();
        for window in points.windows(2) {
            let start = window[0];
            let end = window[1];
            if start >= end {
                continue;
            }
            let segment: String = chars[start..end].iter().collect();

            let mut style = base_style;
            if let Some(ranges) = highlight_ranges
                && let Some(range) = ranges
                    .iter()
                    .find(|range| start >= range.start_col && end <= range.end_col)
            {
                style = style
                    .fg(theme.annotation_highlight_fg)
                    .bg(theme.annotation_bg(range.color));
            }

            if let Some(ranges) = search_ranges
                && ranges.iter().any(|(range_start, range_end)| {
                    let range_start = byte_to_char_col(line, *range_start);
                    let range_end = byte_to_char_col(line, *range_end);
                    start >= range_start && end <= range_end
                })
            {
                style = if is_current_hit {
                    style.fg(theme.search_current_fg).bg(theme.search_current_bg)
                } else {
                    style.fg(theme.search_fg).bg(theme.search_bg)
                };
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

            spans.push(Span::styled(segment, style));
        }

        spans
    }

    /// Take a list of spans and apply UNDERLINED to the character range
    /// [col_start, col_end) across them, splitting spans as needed.
    fn apply_underline_range(
        spans: Vec<Span<'_>>,
        col_start: usize,
        col_end: usize,
    ) -> Vec<Span<'static>> {
        let mut result = Vec::new();
        let mut char_pos = 0usize;

        for span in spans {
            let span_text: String = span.content.to_string();
            let span_chars: Vec<char> = span_text.chars().collect();
            let span_len = span_chars.len();
            let span_end = char_pos + span_len;

            if span_end <= col_start || char_pos >= col_end {
                // Entirely outside underline range
                result.push(Span::styled(span_text, span.style));
            } else if char_pos >= col_start && span_end <= col_end {
                // Entirely inside underline range
                result.push(Span::styled(
                    span_text,
                    span.style.add_modifier(Modifier::UNDERLINED),
                ));
            } else {
                // Partial overlap — split the span
                let ul_start = col_start.max(char_pos) - char_pos;
                let ul_end = col_end.min(span_end) - char_pos;

                if ul_start > 0 {
                    result.push(Span::styled(
                        span_chars[..ul_start].iter().collect::<String>(),
                        span.style,
                    ));
                }
                result.push(Span::styled(
                    span_chars[ul_start..ul_end].iter().collect::<String>(),
                    span.style.add_modifier(Modifier::UNDERLINED),
                ));
                if ul_end < span_len {
                    result.push(Span::styled(
                        span_chars[ul_end..].iter().collect::<String>(),
                        span.style,
                    ));
                }
            }

            char_pos = span_end;
        }

        result
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

    /// Returns the page label at or before `row`, or None if the book has no pagebreak markers.
    pub fn current_page_label(&self, row: usize) -> Option<&str> {
        let map = &self.text_structure.as_ref()?.pagebreak_map;
        if map.is_empty() {
            return None;
        }
        map.iter()
            .filter(|&(&k, _)| k <= row)
            .max_by_key(|&(&k, _)| k)
            .map(|(_, v)| v.as_str())
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
        self.rebuild_word_prefix_sums();
    }

    fn rebuild_word_prefix_sums(&mut self) {
        let lines = self
            .text_structure
            .as_ref()
            .map(|ts| ts.text_lines.as_slice())
            .unwrap_or(&[]);
        let mut sums = Vec::with_capacity(lines.len() + 1);
        let mut total = 0usize;
        sums.push(0);
        for line in lines {
            if line.as_str() != CHAPTER_BREAK_MARKER {
                total += line
                    .split_whitespace()
                    .filter(|word| word.chars().any(|ch| ch.is_alphanumeric()))
                    .count();
            }
            sums.push(total);
        }
        self.word_prefix_sums = sums;
    }

    /// Number of words in `text_lines[start_row..end_row]`, excluding chapter
    /// break markers. O(1) via the prefix-sum cache.
    pub fn words_in_range(&self, start_row: usize, end_row: usize) -> usize {
        let end = end_row.min(self.word_prefix_sums.len().saturating_sub(1));
        if start_row >= end {
            return 0;
        }
        self.word_prefix_sums[end] - self.word_prefix_sums[start_row]
    }
}

fn byte_to_char_col(line: &str, byte_idx: usize) -> usize {
    line.char_indices()
        .take_while(|(idx, _)| *idx < byte_idx)
        .count()
}

fn char_col_to_byte(line: &str, char_col: usize) -> usize {
    line.char_indices()
        .nth(char_col)
        .map(|(b, _)| b)
        .unwrap_or(line.len())
}

impl Default for Board {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{HighlightColor, HighlightRange, TextStructure};
    use crate::theme::ColorTheme;
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
            pagebreak_map: HashMap::new(),
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
            pagebreak_map: HashMap::new(),
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
            pagebreak_map: HashMap::new(),
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
            pagebreak_map: HashMap::new(),
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
            pagebreak_map: HashMap::new(),
        };

        board.update_text_structure(text_structure.clone());
        assert_eq!(board.total_lines(), 1);
    }

    #[test]
    fn test_board_default() {
        let board = Board::default();
        assert!(board.text_structure.is_none());
    }

    #[test]
    fn test_highlight_uses_annotation_colors() {
        let board = Board::new();
        let theme = Theme::for_color_theme(ColorTheme::Default);
        let ranges = vec![HighlightRange {
            highlight_index: 0,
            row: 0,
            start_col: 1,
            end_col: 3,
            color: HighlightColor::Yellow,
        }];

        let spans = board.build_line_spans(
            "abcd",
            0,
            Style::default(),
            &[],
            Some(&ranges),
            None,
            false,
            &theme,
        );

        let highlighted = spans
            .iter()
            .find(|span| span.content.as_ref() == "bc")
            .expect("highlighted span should be split out");
        assert_eq!(highlighted.style.fg, Some(theme.annotation_highlight_fg));
        assert_eq!(highlighted.style.bg, Some(theme.annotation_highlight_bg));
        assert_ne!(highlighted.style.bg, Some(theme.highlight_bg));
    }

    #[test]
    fn test_highlight_color_uses_theme_palette() {
        let board = Board::new();
        let theme = Theme::for_color_theme(ColorTheme::Default);
        let ranges = vec![HighlightRange {
            highlight_index: 0,
            row: 0,
            start_col: 1,
            end_col: 3,
            color: HighlightColor::Green,
        }];

        let spans = board.build_line_spans(
            "abcd",
            0,
            Style::default(),
            &[],
            Some(&ranges),
            None,
            false,
            &theme,
        );

        let highlighted = spans
            .iter()
            .find(|span| span.content.as_ref() == "bc")
            .expect("highlighted span should be split out");
        assert_eq!(highlighted.style.bg, Some(theme.annotation_green_bg));
    }

    #[test]
    fn test_current_search_hit_uses_distinct_colors() {
        let board = Board::new();
        let theme = Theme::for_color_theme(ColorTheme::Default);
        let search_ranges = vec![(1usize, 3usize)];

        let normal = board.build_line_spans(
            "abcd",
            0,
            Style::default(),
            &[],
            None,
            Some(&search_ranges),
            false,
            &theme,
        );
        let current = board.build_line_spans(
            "abcd",
            0,
            Style::default(),
            &[],
            None,
            Some(&search_ranges),
            true,
            &theme,
        );

        let normal_hit = normal
            .iter()
            .find(|span| span.content.as_ref() == "bc")
            .expect("search span should be split out");
        let current_hit = current
            .iter()
            .find(|span| span.content.as_ref() == "bc")
            .expect("search span should be split out");
        assert_eq!(normal_hit.style.bg, Some(theme.search_bg));
        assert_eq!(current_hit.style.bg, Some(theme.search_current_bg));
        assert_ne!(normal_hit.style.bg, current_hit.style.bg);
    }

    #[test]
    fn test_visual_cursor_preserves_highlight_on_same_line() {
        let board = Board::new();
        let theme = Theme::for_color_theme(ColorTheme::Default);
        let ranges = vec![HighlightRange {
            highlight_index: 0,
            row: 0,
            start_col: 0,
            end_col: 4,
            color: HighlightColor::Yellow,
        }];
        let spans = board.build_line_spans(
            "abcd",
            0,
            Style::default(),
            &[],
            Some(&ranges),
            None,
            false,
            &theme,
        );

        let cursor_spans = Board::apply_cursor_range(
            spans
                .into_iter()
                .map(|span| Span::styled(span.content.to_string(), span.style))
                .collect(),
            2,
        );

        assert_eq!(cursor_spans[0].style.bg, Some(theme.annotation_highlight_bg));
        assert_eq!(cursor_spans[1].style.bg, Some(theme.annotation_highlight_bg));
        assert_eq!(cursor_spans[2].style.bg, Some(theme.annotation_highlight_bg));
        assert!(cursor_spans[2].style.add_modifier.contains(Modifier::REVERSED));
        assert_eq!(cursor_spans[3].style.bg, Some(theme.annotation_highlight_bg));
    }
}
