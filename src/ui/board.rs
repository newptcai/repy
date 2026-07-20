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
    /// Cumulative character counts: `char_prefix_sums[i]` is the number of
    /// characters in `text_lines[..i]` (chapter-break markers excluded). This
    /// is a width-independent measure of reading progress, used for KOReader
    /// sync percentages.
    char_prefix_sums: Vec<usize>,
}

impl Board {
    pub fn new() -> Self {
        Self {
            text_structure: None,
            word_prefix_sums: Vec::new(),
            char_prefix_sums: Vec::new(),
        }
    }

    pub fn with_text_structure(mut self, text_structure: TextStructure) -> Self {
        self.text_structure = Some(text_structure);
        self.rebuild_prefix_sums();
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

    /// The `[start, end)` line window the reader view draws for the current
    /// state and viewport height, accounting for chapter clamping when
    /// `seamless_between_chapters` is off. `(0, 0)` without a loaded book.
    pub fn visible_window(
        &self,
        state: &ApplicationState,
        content_start_rows: Option<&[usize]>,
        height: usize,
    ) -> (usize, usize) {
        match &self.text_structure {
            Some(ts) => Self::visible_window_for(ts, state, content_start_rows, height),
            None => (0, 0),
        }
    }

    fn visible_window_for(
        text_structure: &TextStructure,
        state: &ApplicationState,
        content_start_rows: Option<&[usize]>,
        height: usize,
    ) -> (usize, usize) {
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
        (start_line, end_line)
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

        let (start_line, end_line) =
            Self::visible_window_for(text_structure, state, content_start_rows, height);

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

        // Build per-line character-range lists that overlay the visual-mode
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
                    let end_char = if line_num == e_line {
                        e_col
                    } else {
                        line_chars
                    };
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

        // Keep the selected hit's exact projected ranges separate so every
        // touched row is styled, without promoting other hits on that row.
        let current_hit_ranges = state
            .ui_state
            .search_results
            .get(state.ui_state.selected_search_result)
            .map(|result| result.per_row.as_slice())
            .unwrap_or(&[]);

        // Keep annotation markers outside the paragraph that contains the
        // book text. Prepending the marker as a span makes it participate in
        // Paragraph wrapping, which can push a full-width line onto an extra
        // visual row and misalign every line below it.
        let has_highlight_gutter = !state.ui_state.highlights.is_empty() && area.width > 0;
        let text_area = if has_highlight_gutter {
            Rect {
                x: area.x.saturating_add(1),
                width: area.width.saturating_sub(1),
                ..area
            }
        } else {
            area
        };

        if has_highlight_gutter {
            let marker_lines: Vec<Line> = (start_line..end_line)
                .map(|line_num| {
                    let marker = state
                        .ui_state
                        .highlight_ranges
                        .get(&line_num)
                        .and_then(|ranges| ranges.first());
                    match marker {
                        Some(range) => Line::from(Span::styled(
                            "▎",
                            Style::default().fg(theme.annotation_bg(range.color)),
                        )),
                        None => Line::raw(" "),
                    }
                })
                .collect();
            let gutter_area = Rect { width: 1, ..area };
            frame.render_widget(Paragraph::new(marker_lines), gutter_area);
        }

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
                        .cloned()
                        .unwrap_or_default();
                    if let Some(extra) = visual_match_ranges.get(line_num - start_line) {
                        merged.extend(extra.iter().copied());
                    }
                    merged
                };
                let search_ranges_arg: Option<&[(usize, usize)]> =
                    if combined_search_ranges.is_empty() {
                        None
                    } else {
                        Some(combined_search_ranges.as_slice())
                    };
                let current_ranges: Vec<(usize, usize)> = current_hit_ranges
                    .iter()
                    .filter_map(|&(row, start, end)| (row == line_num).then_some((start, end)))
                    .collect();
                let current_ranges_arg =
                    (!current_ranges.is_empty()).then_some(current_ranges.as_slice());

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
                        current_ranges_arg,
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
                        current_ranges_arg,
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

        // `text_lines` are already wrapped by the parser to the configured
        // reading width. Wrapping them again here creates extra visual rows
        // that have no corresponding row in formatting, highlight, cursor,
        // or image coordinates.
        let paragraph = Paragraph::new(visible_lines).block(Block::default());

        frame.render_widget(paragraph, text_area);
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
        current_search_ranges: Option<&[(usize, usize)]>,
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
                points.push(*start);
                points.push(*end);
            }
        }
        if let Some(ranges) = current_search_ranges {
            for (start, end) in ranges {
                points.push(*start);
                points.push(*end);
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
                && ranges
                    .iter()
                    .any(|(range_start, range_end)| start >= *range_start && end <= *range_end)
            {
                let is_current = current_search_ranges.is_some_and(|current| {
                    current
                        .iter()
                        .any(|(range_start, range_end)| start >= *range_start && end <= *range_end)
                });
                style = if is_current {
                    style
                        .fg(theme.search_current_fg)
                        .bg(theme.search_current_bg)
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

    /// Rows reserved for rendering the image whose placeholder is on `line`
    /// (placeholder row included), when inline images are enabled.
    pub fn image_block_rows(&self, line: usize) -> Option<usize> {
        self.text_structure
            .as_ref()?
            .image_block_rows
            .get(&line)
            .copied()
    }

    /// The inline-image block that strictly contains `row` — i.e. `row` is
    /// one of the block's reserved lines but not its placeholder row —
    /// returned as `(block_start, rows)`. Empty when inline images are off
    /// (no blocks are reserved then).
    pub fn image_block_containing(&self, row: usize) -> Option<(usize, usize)> {
        self.text_structure
            .as_ref()?
            .image_block_rows
            .iter()
            .find(|&(&start, &rows)| start < row && row < start + rows)
            .map(|(&start, &rows)| (start, rows))
    }

    pub fn image_src(&self, line: usize) -> Option<String> {
        self.text_structure
            .as_ref()
            .and_then(|ts| ts.image_maps.get(&line).cloned())
    }

    pub fn is_typography_spacing_row(&self, row: usize) -> bool {
        self.text_structure
            .as_ref()
            .is_some_and(|ts| ts.typography_spacing_rows.contains(&row))
    }

    pub fn paragraph_starts(&self) -> &[usize] {
        self.text_structure
            .as_ref()
            .map(|ts| ts.paragraph_starts.as_slice())
            .unwrap_or(&[])
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
            if text_structure.typography_spacing_rows.contains(&start_row) {
                return String::new();
            }
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
            if text_structure.typography_spacing_rows.contains(&row) {
                continue;
            }
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
        self.rebuild_prefix_sums();
    }

    fn rebuild_prefix_sums(&mut self) {
        let lines = self
            .text_structure
            .as_ref()
            .map(|ts| ts.text_lines.as_slice())
            .unwrap_or(&[]);
        let mut word_sums = Vec::with_capacity(lines.len() + 1);
        let mut char_sums = Vec::with_capacity(lines.len() + 1);
        let mut word_total = 0usize;
        let mut char_total = 0usize;
        word_sums.push(0);
        char_sums.push(0);
        for line in lines {
            if line.as_str() != CHAPTER_BREAK_MARKER {
                word_total += line
                    .split_whitespace()
                    .filter(|word| word.chars().any(|ch| ch.is_alphanumeric()))
                    .count();
                // Layout-only indentation and justification spaces must not
                // perturb semantic progress (notably KOReader sync).
                let normalized = line.split_whitespace().collect::<Vec<_>>().join(" ");
                char_total += normalized.chars().count();
            }
            word_sums.push(word_total);
            char_sums.push(char_total);
        }
        self.word_prefix_sums = word_sums;
        self.char_prefix_sums = char_sums;
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

    /// Fraction of the book's characters that precede `row` — a
    /// width-independent reading-progress measure in `[0.0, 1.0]`. Matches how
    /// KOReader derives an EPUB's content-proportional percentage, so it can be
    /// exchanged over kosync. Returns `0.0` for an empty book.
    pub fn content_fraction(&self, row: usize) -> f64 {
        let total = self.char_prefix_sums.last().copied().unwrap_or(0);
        if total == 0 {
            return 0.0;
        }
        let idx = row.min(self.char_prefix_sums.len().saturating_sub(1));
        self.char_prefix_sums[idx] as f64 / total as f64
    }

    /// Inverse of [`content_fraction`](Self::content_fraction): the row that
    /// begins at (or contains) the character at `fraction` through the book.
    /// Clamped to a valid row index; returns `0` for an empty book.
    pub fn row_for_fraction(&self, fraction: f64) -> usize {
        let total_lines = self.total_lines();
        if total_lines == 0 {
            return 0;
        }
        let total = self.char_prefix_sums.last().copied().unwrap_or(0);
        if total == 0 {
            return 0;
        }
        let target = (fraction.clamp(0.0, 1.0) * total as f64).round() as usize;
        // `char_prefix_sums[k + 1]` is the cumulative char count through row
        // `k`; the first row whose cumulative reaches `target` contains it.
        let row = self.char_prefix_sums[1..].partition_point(|&cum| cum <= target);
        row.min(total_lines - 1)
    }

    /// The global row at `fraction` of the way through the character span of
    /// the chapter `[start_row, end_row)`. Used to place a KOReader XPointer's
    /// within-chapter position without letting global percentage drift leak in.
    pub fn row_for_chapter_fraction(
        &self,
        start_row: usize,
        end_row: usize,
        fraction: f64,
    ) -> usize {
        let total = self.char_prefix_sums.last().copied().unwrap_or(0);
        if total == 0 {
            return start_row;
        }
        let start_chars = self.chars_before(start_row);
        let end_chars = self.chars_before(end_row);
        let span = end_chars.saturating_sub(start_chars);
        let target = start_chars as f64 + fraction.clamp(0.0, 1.0) * span as f64;
        self.row_for_fraction(target / total as f64)
    }

    /// Cumulative character count before `row` (clamped).
    fn chars_before(&self, row: usize) -> usize {
        let idx = row.min(self.char_prefix_sums.len().saturating_sub(1));
        self.char_prefix_sums.get(idx).copied().unwrap_or(0)
    }
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
            source_map: Default::default(),
            text_lines: vec!["Line 1".to_string(), "Line 2".to_string()],
            image_maps: HashMap::new(),
            section_rows: HashMap::new(),
            section_offsets: HashMap::new(),
            formatting: vec![],
            links: vec![],
            pagebreak_map: HashMap::new(),
            image_block_rows: HashMap::new(),
            paragraph_starts: Vec::new(),
            typography_spacing_rows: std::collections::HashSet::new(),
        };

        let board = Board::new().with_text_structure(text_structure.clone());

        assert!(board.text_structure.is_some());
    }

    #[test]
    fn test_image_block_containing() {
        let mut image_block_rows = HashMap::new();
        image_block_rows.insert(10, 5); // block spans rows 10..15
        let text_structure = TextStructure {
            source_map: Default::default(),
            text_lines: vec![String::new(); 30],
            image_maps: HashMap::new(),
            section_rows: HashMap::new(),
            section_offsets: HashMap::new(),
            formatting: vec![],
            links: vec![],
            pagebreak_map: HashMap::new(),
            image_block_rows,
            paragraph_starts: Vec::new(),
            typography_spacing_rows: std::collections::HashSet::new(),
        };
        let board = Board::new().with_text_structure(text_structure);

        // The placeholder row itself is not "inside" (a window starting
        // there shows the whole block).
        assert_eq!(board.image_block_containing(10), None);
        assert_eq!(board.image_block_containing(11), Some((10, 5)));
        assert_eq!(board.image_block_containing(14), Some((10, 5)));
        // First row past the block.
        assert_eq!(board.image_block_containing(15), None);
        assert_eq!(board.image_block_containing(9), None);
    }

    #[test]
    fn test_board_total_lines() {
        let mut board = Board::new();
        assert_eq!(board.total_lines(), 0);

        let text_structure = TextStructure {
            source_map: Default::default(),
            text_lines: vec![
                "Line 1".to_string(),
                "Line 2".to_string(),
                "Line 3".to_string(),
            ],
            image_maps: HashMap::new(),
            section_rows: HashMap::new(),
            section_offsets: HashMap::new(),
            formatting: vec![],
            links: vec![],
            pagebreak_map: HashMap::new(),
            image_block_rows: HashMap::new(),
            paragraph_starts: Vec::new(),
            typography_spacing_rows: std::collections::HashSet::new(),
        };

        board.update_text_structure(text_structure);
        assert_eq!(board.total_lines(), 3);
    }

    #[test]
    fn test_board_is_valid_line() {
        let mut board = Board::new();
        assert!(!board.is_valid_line(0));

        let text_structure = TextStructure {
            source_map: Default::default(),
            text_lines: vec!["Line 1".to_string(), "Line 2".to_string()],
            image_maps: HashMap::new(),
            section_rows: HashMap::new(),
            section_offsets: HashMap::new(),
            formatting: vec![],
            links: vec![],
            pagebreak_map: HashMap::new(),
            image_block_rows: HashMap::new(),
            paragraph_starts: Vec::new(),
            typography_spacing_rows: std::collections::HashSet::new(),
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
            source_map: Default::default(),
            text_lines: vec!["Line 1".to_string(), "Line 2".to_string()],
            image_maps: HashMap::new(),
            section_rows: HashMap::new(),
            section_offsets: HashMap::new(),
            formatting: vec![],
            links: vec![],
            pagebreak_map: HashMap::new(),
            image_block_rows: HashMap::new(),
            paragraph_starts: Vec::new(),
            typography_spacing_rows: std::collections::HashSet::new(),
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
            source_map: Default::default(),
            text_lines: vec!["New line".to_string()],
            image_maps: HashMap::new(),
            section_rows: HashMap::new(),
            section_offsets: HashMap::new(),
            formatting: vec![],
            links: vec![],
            pagebreak_map: HashMap::new(),
            image_block_rows: HashMap::new(),
            paragraph_starts: Vec::new(),
            typography_spacing_rows: std::collections::HashSet::new(),
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
            None,
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
            None,
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
            None,
            &theme,
        );
        let current = board.build_line_spans(
            "abcd",
            0,
            Style::default(),
            &[],
            None,
            Some(&search_ranges),
            Some(&search_ranges),
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
    fn current_search_style_applies_only_to_selected_ranges_on_every_row() {
        let board = Board::new();
        let theme = Theme::for_color_theme(ColorTheme::Default);
        let all_ranges = vec![(0, 2), (3, 5)];
        let selected = vec![(3, 5)];

        for row in 4..7 {
            let spans = board.build_line_spans(
                "ab cd",
                row,
                Style::default(),
                &[],
                None,
                Some(&all_ranges),
                Some(&selected),
                &theme,
            );
            let other = spans.iter().find(|span| span.content == "ab").unwrap();
            let current = spans.iter().find(|span| span.content == "cd").unwrap();
            assert_eq!(other.style.bg, Some(theme.search_bg));
            assert_eq!(current.style.bg, Some(theme.search_current_bg));
        }
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
            None,
            &theme,
        );

        let cursor_spans = Board::apply_cursor_range(
            spans
                .into_iter()
                .map(|span| Span::styled(span.content.to_string(), span.style))
                .collect(),
            2,
        );

        assert_eq!(
            cursor_spans[0].style.bg,
            Some(theme.annotation_highlight_bg)
        );
        assert_eq!(
            cursor_spans[1].style.bg,
            Some(theme.annotation_highlight_bg)
        );
        assert_eq!(
            cursor_spans[2].style.bg,
            Some(theme.annotation_highlight_bg)
        );
        assert!(
            cursor_spans[2]
                .style
                .add_modifier
                .contains(Modifier::REVERSED)
        );
        assert_eq!(
            cursor_spans[3].style.bg,
            Some(theme.annotation_highlight_bg)
        );
    }

    fn board_from_lines(lines: &[&str]) -> Board {
        let text_structure = TextStructure {
            source_map: Default::default(),
            text_lines: lines.iter().map(|s| s.to_string()).collect(),
            image_maps: HashMap::new(),
            section_rows: HashMap::new(),
            section_offsets: HashMap::new(),
            formatting: vec![],
            links: vec![],
            pagebreak_map: HashMap::new(),
            image_block_rows: HashMap::new(),
            paragraph_starts: Vec::new(),
            typography_spacing_rows: std::collections::HashSet::new(),
        };
        Board::new().with_text_structure(text_structure)
    }

    #[test]
    fn test_content_fraction_endpoints_and_monotonic() {
        let board = board_from_lines(&["alpha", "bravo", "charlie", "delta"]);
        assert_eq!(board.content_fraction(0), 0.0);
        // The last row starts after everything before it; still < 1.0 because
        // the final line's own characters are not yet "behind" the reader.
        let last = board.content_fraction(board.total_lines() - 1);
        assert!(last > 0.0 && last < 1.0);
        // Non-decreasing across rows.
        let mut previous = 0.0;
        for row in 0..board.total_lines() {
            let fraction = board.content_fraction(row);
            assert!(fraction >= previous, "fraction decreased at row {row}");
            previous = fraction;
        }
        // A row past the end clamps to the full-book total (all chars behind).
        assert_eq!(board.content_fraction(board.total_lines()), 1.0);
    }

    #[test]
    fn test_content_fraction_skips_markers_and_blanks() {
        // Chapter-break markers and blank padding rows contribute no
        // characters, so the fraction is unchanged across them.
        let board = board_from_lines(&["alpha", CHAPTER_BREAK_MARKER, "", "bravo"]);
        assert_eq!(board.content_fraction(1), board.content_fraction(2));
        assert_eq!(board.content_fraction(2), board.content_fraction(3));
        // "alpha" (5) precedes row 3; "bravo" (5) is the remaining half.
        assert!((board.content_fraction(3) - 0.5).abs() < 1e-9);
    }

    #[test]
    fn test_row_for_fraction_round_trips() {
        let board = board_from_lines(&["alpha", "bravo", "charlie", "delta", "echo"]);
        for row in 0..board.total_lines() {
            let fraction = board.content_fraction(row);
            assert_eq!(
                board.row_for_fraction(fraction),
                row,
                "round trip failed at row {row}"
            );
        }
        assert_eq!(board.row_for_fraction(0.0), 0);
        assert_eq!(board.row_for_fraction(1.0), board.total_lines() - 1);
    }

    #[test]
    fn test_content_fraction_width_independent() {
        // Re-wrapping the same text into different rows preserves the fraction
        // at a shared content boundary, because the measure counts characters
        // rather than rows. (Space-free tokens here isolate the invariant; real
        // wrapping additionally drops one space per break, a bounded skew.)
        let narrow = board_from_lines(&["alpha", "bravo", "charlie", "delta"]);
        let wide = board_from_lines(&["alphabravo", "charliedelta"]);
        assert_eq!(
            *narrow.char_prefix_sums.last().unwrap(),
            *wide.char_prefix_sums.last().unwrap()
        );
        // Boundary before "charlie" is 10/22 of the book in both layouts.
        assert!((narrow.content_fraction(2) - wide.content_fraction(1)).abs() < 1e-9);
    }

    #[test]
    fn test_content_fraction_empty_book() {
        let board = Board::new();
        assert_eq!(board.content_fraction(0), 0.0);
        assert_eq!(board.row_for_fraction(0.5), 0);
    }
}
