use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};

use crate::models::{TextStructure, ReadingState};
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

    pub fn render(&self, frame: &mut Frame, area: Rect, state: &ApplicationState) {
        let block = Block::default()
            .borders(Borders::ALL)
            .title("Content");

        let inner_area = block.inner(area);
        frame.render_widget(block, area);

        if let Some(ref text_structure) = self.text_structure {
            self.render_content(frame, inner_area, text_structure, state);
        } else {
            self.render_empty(frame, inner_area);
        }
    }

    fn render_content(&self, frame: &mut Frame, area: Rect, text_structure: &TextStructure, state: &ApplicationState) {
        let height = area.height as usize;
        let _width = area.width as usize;

        let start_line = state.reading_state.row.saturating_sub(1);
        let end_line = (start_line + height).min(text_structure.text_lines.len());

        let selection_start = state.ui_state.selection_start;

        let visible_lines: Vec<Line> = text_structure.text_lines
            .get(start_line..end_line)
            .unwrap_or(&[])
            .iter()
            .enumerate()
            .map(|(i, line)| {
                let line_num = start_line + i;
                let mut spans = Vec::new();

                if state.config.settings.show_line_numbers {
                    spans.push(Span::styled(
                        format!("{:>4} ", line_num + 1),
                        Style::default().fg(Color::DarkGray),
                    ));
                }
                
                let mut style = Style::default();
                if let Some(selection_start) = selection_start {
                    let selection_end = state.reading_state.row;
                    if (line_num >= selection_start && line_num <= selection_end) || (line_num >= selection_end && line_num <= selection_start) {
                        style = style.add_modifier(Modifier::REVERSED);
                    }
                }

                spans.push(Span::styled(line.clone(), style));
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

    pub fn get_selected_text(&self, start: usize, end: usize) -> String {
        if let Some(text_structure) = &self.text_structure {
            let (start, end) = if start < end { (start, end) } else { (end, start) };
            text_structure.text_lines[start..=end].join("\n")
        } else {
            String::new()
        }
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
    use crate::models::{InlineStyle, TextStructure};
    use std::collections::HashMap;
    use crate::config::Config;

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
        };

        let board = Board::new()
            .with_text_structure(text_structure.clone());

        assert!(board.text_structure.is_some());
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

        board.update_text_structure(text_structure.clone());
        assert_eq!(board.total_lines(), 1);
    }

    #[test]
    fn test_board_default() {
        let board = Board::default();
        assert!(board.text_structure.is_none());
    }
}