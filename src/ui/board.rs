use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};

use crate::models::{TextStructure, ReadingState};

/// Board widget for rendering book text content
pub struct Board {
    text_structure: Option<TextStructure>,
    reading_state: ReadingState,
    show_line_numbers: bool,
    double_spread: bool,
    padding: u16,
}

impl Board {
    pub fn new() -> Self {
        Self {
            text_structure: None,
            reading_state: ReadingState::default(),
            show_line_numbers: false,
            double_spread: false,
            padding: 1,
        }
    }

    pub fn with_text_structure(mut self, text_structure: TextStructure) -> Self {
        self.text_structure = Some(text_structure);
        self
    }

    pub fn with_reading_state(mut self, reading_state: ReadingState) -> Self {
        self.reading_state = reading_state;
        self
    }

    pub fn with_line_numbers(mut self, show: bool) -> Self {
        self.show_line_numbers = show;
        self
    }

    pub fn with_double_spread(mut self, enabled: bool) -> Self {
        self.double_spread = enabled;
        self
    }

    pub fn with_padding(mut self, padding: u16) -> Self {
        self.padding = padding;
        self
    }

    pub fn render(&self, frame: &mut Frame, area: Rect) {
        let block = Block::default()
            .borders(Borders::ALL)
            .title("Content");

        let inner_area = block.inner(area);
        frame.render_widget(block, area);

        if let Some(ref text_structure) = self.text_structure {
            self.render_content(frame, inner_area, text_structure);
        } else {
            self.render_empty(frame, inner_area);
        }
    }

    fn render_content(&self, frame: &mut Frame, area: Rect, text_structure: &TextStructure) {
        let height = area.height as usize;
        let _width = area.width as usize;

        // Calculate visible lines based on reading state
        let start_line = self.reading_state.row.saturating_sub(1);
        let end_line = (start_line + height).min(text_structure.text_lines.len());

        let visible_lines: Vec<Line> = text_structure.text_lines
            .get(start_line..end_line)
            .unwrap_or(&[])
            .iter()
            .enumerate()
            .map(|(i, line)| {
                let mut spans = Vec::new();

                // Add line number if enabled
                if self.show_line_numbers {
                    let line_num = start_line + i + 1;
                    spans.push(Span::styled(
                        format!("{:>4} ", line_num),
                        Style::default().fg(Color::DarkGray),
                    ));
                }

                // Add actual content
                spans.push(Span::raw(line.clone()));

                Line::from(spans)
            })
            .collect();

        let paragraph = Paragraph::new(visible_lines)
            .wrap(Wrap { trim: true })
            .block(Block::default());

        frame.render_widget(paragraph, area);
    }

    fn render_empty(&self, frame: &mut Frame, area: Rect) {
        let empty_text = vec![
            Line::from("No content loaded"),
            Line::from("Open a book to start reading"),
        ];

        let paragraph = Paragraph::new(empty_text)
            .style(Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC))
            .wrap(Wrap { trim: true });

        frame.render_widget(paragraph, area);
    }

    /// Get the total number of lines in the current text structure
    pub fn total_lines(&self) -> usize {
        self.text_structure
            .as_ref()
            .map(|ts| ts.text_lines.len())
            .unwrap_or(0)
    }

    /// Check if a line number is valid
    pub fn is_valid_line(&self, line: usize) -> bool {
        line < self.total_lines()
    }

    /// Get the content of a specific line
    pub fn get_line(&self, line: usize) -> Option<&str> {
        self.text_structure
            .as_ref()
            .and_then(|ts| ts.text_lines.get(line).map(String::as_str))
    }

    /// Update the text structure
    pub fn update_text_structure(&mut self, text_structure: TextStructure) {
        self.text_structure = Some(text_structure);
    }

    /// Update the reading state
    pub fn update_reading_state(&mut self, reading_state: ReadingState) {
        self.reading_state = reading_state;
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
    use crate::models::{InlineStyle, TextStructure};
    use std::collections::HashMap;

    #[test]
    fn test_board_new() {
        let board = Board::new();
        assert!(board.text_structure.is_none());
        assert_eq!(board.reading_state.row, 0);
        assert!(!board.show_line_numbers);
        assert!(!board.double_spread);
        assert_eq!(board.padding, 1);
    }

    #[test]
    fn test_board_builder() {
        let text_structure = TextStructure {
            text_lines: vec!["Line 1".to_string(), "Line 2".to_string()],
            image_maps: HashMap::new(),
            section_rows: HashMap::new(),
            formatting: vec![],
        };

        let reading_state = ReadingState {
            content_index: 0,
            textwidth: 80,
            row: 10,
            rel_pctg: None,
            section: None,
        };

        let board = Board::new()
            .with_text_structure(text_structure.clone())
            .with_reading_state(reading_state)
            .with_line_numbers(true)
            .with_double_spread(true)
            .with_padding(2);

        assert!(board.text_structure.is_some());
        assert_eq!(board.reading_state.row, 10);
        assert!(board.show_line_numbers);
        assert!(board.double_spread);
        assert_eq!(board.padding, 2);
    }

    #[test]
    fn test_board_total_lines() {
        let mut board = Board::new();
        assert_eq!(board.total_lines(), 0);

        let text_structure = TextStructure {
            text_lines: vec!["Line 1".to_string(), "Line 2".to_string(), "Line 3".to_string()],
            image_maps: HashMap::new(),
            section_rows: HashMap::new(),
            formatting: vec![],
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
        };

        let reading_state = ReadingState {
            content_index: 1,
            textwidth: 100,
            row: 5,
            rel_pctg: Some(0.5),
            section: Some("test".to_string()),
        };

        board.update_text_structure(text_structure.clone());
        board.update_reading_state(reading_state);

        assert_eq!(board.total_lines(), 1);
        assert_eq!(board.reading_state.row, 5);
    }

    #[test]
    fn test_board_default() {
        let board = Board::default();
        assert_eq!(board.show_line_numbers, false);
        assert_eq!(board.double_spread, false);
        assert_eq!(board.padding, 1);
    }
}