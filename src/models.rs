use chrono::{DateTime, Utc};
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
pub enum Direction {
    Forward,
    Backward,
}

impl Default for Direction {
    fn default() -> Self {
        Direction::Forward
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct InlineStyle {
    pub row: u16,
    pub col: u16,
    pub n_letters: u16,
    pub attr: u16, // This will likely be replaced by ratatui::style::Style later
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct BookMetadata {
    pub title: Option<String>,
    pub creator: Option<String>,
    pub description: Option<String>,
    pub publisher: Option<String>,
    pub date: Option<String>,
    pub language: Option<String>,
    pub format: Option<String>,
    pub identifier: Option<String>,
    pub source: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LibraryItem {
    pub last_read: DateTime<Utc>,
    pub filepath: String,
    pub title: Option<String>,
    pub author: Option<String>,
    pub reading_progress: Option<f32>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct ReadingState {
    pub content_index: usize,
    pub textwidth: usize,
    pub row: usize,
    pub rel_pctg: Option<f32>,
    pub section: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct SearchData {
    pub direction: Direction,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct LettersCount {
    pub all: usize,
    pub cumulative: Vec<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct CharPos {
    pub row: u16,
    pub col: u16,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TextMark {
    pub start: CharPos,
    pub end: Option<CharPos>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextSpan {
    pub start: CharPos,
    pub n_letters: u16,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TocEntry {
    pub label: String,
    pub content_index: usize,
    pub section: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct TextStructure {
    pub text_lines: Vec<String>,
    pub image_maps: HashMap<usize, String>,
    pub section_rows: HashMap<String, usize>,
    pub formatting: Vec<InlineStyle>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct NoUpdate;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_direction_default() {
        assert_eq!(Direction::default(), Direction::Forward);
    }

    #[test]
    fn test_book_metadata_default() {
        let metadata = BookMetadata::default();
        assert_eq!(metadata.title, None);
        assert_eq!(metadata.creator, None);
        assert_eq!(metadata.description, None);
        assert_eq!(metadata.publisher, None);
        assert_eq!(metadata.date, None);
        assert_eq!(metadata.language, None);
        assert_eq!(metadata.format, None);
        assert_eq!(metadata.identifier, None);
        assert_eq!(metadata.source, None);
    }

    #[test]
    fn test_reading_state_default() {
        let state = ReadingState::default();
        assert_eq!(state.content_index, 0);
        assert_eq!(state.textwidth, 0);
        assert_eq!(state.row, 0);
        assert_eq!(state.rel_pctg, None);
        assert_eq!(state.section, None);
    }

    #[test]
    fn test_search_data_default() {
        let search_data = SearchData::default();
        assert_eq!(search_data.direction, Direction::Forward);
        assert_eq!(search_data.value, "");
    }

    #[test]
    fn test_letters_count_default() {
        let letters_count = LettersCount::default();
        assert_eq!(letters_count.all, 0);
        assert!(letters_count.cumulative.is_empty());
    }

    #[test]
    fn test_char_pos_default() {
        let char_pos = CharPos::default();
        assert_eq!(char_pos.row, 0);
        assert_eq!(char_pos.col, 0);
    }

    #[test]
    fn test_text_structure_default() {
        let text_structure = TextStructure::default();
        assert!(text_structure.text_lines.is_empty());
        assert!(text_structure.image_maps.is_empty());
        assert!(text_structure.section_rows.is_empty());
        assert!(text_structure.formatting.is_empty());
    }

    #[test]
    fn test_no_update_default() {
        let no_update = NoUpdate::default();
        assert_eq!(no_update, NoUpdate);
    }
}