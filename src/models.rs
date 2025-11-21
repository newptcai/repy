use chrono::{DateTime, Utc};
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
pub enum Direction {
    Forward,
    Backward,
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

#[derive(Debug, Clone, PartialEq, Default)]
pub struct CharPos {
    pub row: u16,
    pub col: u16,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TextMark {
    pub start: CharPos,
    pub end: Option<CharPos>,
}

#[derive(Debug, Clone, PartialEq)]
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