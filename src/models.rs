use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, PartialEq, Default)]
pub enum Direction {
    #[default]
    Forward,
    Backward,
    Up,
    Down,
    Left,
    Right,
    PageUp,
    PageDown,
    HalfPageUp,
    HalfPageDown,
    Home,
    End,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub enum WindowType {
    #[default]
    Reader,
    Help,
    Dictionary,
    Toc,
    Bookmarks,
    Library,
    OpdsCatalogs,
    OpdsFeed,
    OpdsSearchInput,
    OpdsDetails,
    Search,
    Links,
    Metadata,
    Settings,
    SettingsTextInput,
    Images,
    ImageView,
    Statistics,
    Visual,
    DictionaryCommandInput,
    Highlights,
    HighlightCommentEditor,
    ConfirmDeleteHighlight,
    ConfirmSyncProgress,
    LinkPreview,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct ReadingStatsTotals {
    pub seconds: i64,
    pub rows: i64,
    pub words: i64,
    pub sessions: i64,
}

impl ReadingStatsTotals {
    pub fn words_per_minute(&self) -> Option<f64> {
        if self.seconds > 0 && self.words > 0 {
            Some(self.words as f64 / (self.seconds as f64 / 60.0))
        } else {
            None
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct ReadingStatistics {
    pub book_title: Option<String>,
    pub book_author: Option<String>,
    pub book: ReadingStatsTotals,
    pub global: ReadingStatsTotals,
    pub current_streak_days: usize,
    pub longest_streak_days: usize,
    pub estimated_book_minutes_left: Option<i64>,
    pub estimated_chapter_minutes_left: Option<i64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct InlineStyle {
    pub row: u16,
    pub col: u16,
    pub n_letters: u16,
    pub attr: u32, // This will likely be replaced by ratatui::style::Style later
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

/// An ebook file found by the library directory scanner.
#[derive(Debug, Clone, PartialEq)]
pub struct ScannedBook {
    pub filepath: String,
    pub title: Option<String>,
    pub author: Option<String>,
    /// Stable scanner identity shared by every format in one Calibre record.
    pub book_key: String,
    pub series: Option<String>,
    pub series_index: Option<f32>,
    pub tags: Vec<String>,
    pub language: Option<String>,
    pub publisher: Option<String>,
    pub description: Option<String>,
    /// Canonical paths for every discovered format, in preference order.
    pub formats: Vec<String>,
    pub cover_path: Option<String>,
}

/// One physical file in the persistent library scan cache.
#[derive(Debug, Clone, PartialEq)]
pub struct LibraryCacheEntry {
    pub filepath: String,
    pub library_root: String,
    pub book_key: String,
    pub mtime: i64,
    pub metadata_mtime: i64,
    pub cover_mtime: i64,
    pub title: Option<String>,
    pub author: Option<String>,
    pub series: Option<String>,
    pub series_index: Option<f32>,
    pub tags: Vec<String>,
    pub language: Option<String>,
    pub publisher: Option<String>,
    pub description: Option<String>,
    pub cover_path: Option<String>,
}

/// A row in the library window: the merge of reading history and on-disk
/// scanned books, keyed by canonical filepath.
#[derive(Debug, Clone, PartialEq)]
pub struct LibraryEntry {
    pub filepath: String,
    pub book_key: String,
    pub title: Option<String>,
    pub author: Option<String>,
    pub series: Option<String>,
    pub series_index: Option<f32>,
    pub tags: Vec<String>,
    pub language: Option<String>,
    pub publisher: Option<String>,
    pub description: Option<String>,
    pub formats: Vec<String>,
    pub cover_path: Option<String>,
    /// Present only for books with reading history.
    pub history_filepath: Option<String>,
    pub last_read: Option<DateTime<Utc>>,
    pub reading_progress: Option<f32>,
    /// False for history entries whose file no longer exists on disk.
    pub on_disk: bool,
}

impl LibraryEntry {
    /// Sort key: title if known, otherwise the file name.
    pub fn display_title(&self) -> String {
        match &self.title {
            Some(title) => title.to_lowercase(),
            None => std::path::Path::new(&self.filepath)
                .file_name()
                .map(|n| n.to_string_lossy().to_lowercase())
                .unwrap_or_else(|| self.filepath.to_lowercase()),
        }
    }

    pub fn searchable_text(&self) -> String {
        format!(
            "{} {} {} {} {}",
            self.title.as_deref().unwrap_or_default(),
            self.author.as_deref().unwrap_or_default(),
            self.series.as_deref().unwrap_or_default(),
            self.tags.join(" "),
            self.filepath
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LibrarySortMode {
    #[default]
    Recent,
    Title,
    Author,
    Series,
    Progress,
}

impl LibrarySortMode {
    pub fn next(self) -> Self {
        match self {
            Self::Recent => Self::Title,
            Self::Title => Self::Author,
            Self::Author => Self::Series,
            Self::Series => Self::Progress,
            Self::Progress => Self::Recent,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Recent => "recent",
            Self::Title => "title",
            Self::Author => "author",
            Self::Series => "series",
            Self::Progress => "progress",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ReadingState {
    pub content_index: usize,
    /// Chapter-local character offset in the normalized source text.
    pub source_offset: Option<usize>,
    pub textwidth: usize,
    pub row: usize,
    pub rel_pctg: Option<f32>,
    pub section: Option<String>,
}

impl Default for ReadingState {
    fn default() -> Self {
        Self {
            content_index: 0,
            source_offset: None,
            textwidth: crate::settings::DEFAULT_TEXT_WIDTH,
            row: 0,
            rel_pctg: None,
            section: None,
        }
    }
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

impl TextMark {
    /// Assert validity and check if the mark is unterminated
    /// eg. <div><i>This is italic text</div>
    /// Missing </i> tag
    pub fn is_valid(&self) -> bool {
        if let Some(end) = self.end {
            if self.start.row == end.row {
                self.start.col <= end.col
            } else {
                self.start.row < end.row
            }
        } else {
            false
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextSpan {
    pub start: CharPos,
    pub n_letters: u16,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BookIdentity {
    pub book_id: String,
    pub identifier: Option<String>,
    pub title: Option<String>,
    pub creator: Option<String>,
    pub spine_hrefs_hash: String,
    pub content_fingerprints_hash: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Highlight {
    pub id: String,
    pub book_id: String,
    pub content_index: usize,
    pub spine_href: String,
    pub exact: String,
    pub prefix: String,
    pub suffix: String,
    pub approx_offset: usize,
    pub normalization_version: i64,
    pub color: String,
    pub comment: Option<String>,
    pub comment_format: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub resolution_status: String,
}

/// Palette of highlight colors (KOReader-style set). The actual RGB values
/// are resolved per color theme in `theme.rs`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HighlightColor {
    #[default]
    Yellow,
    Green,
    Blue,
    Pink,
    Purple,
}

impl HighlightColor {
    pub fn from_name(name: &str) -> Self {
        match name {
            "green" => HighlightColor::Green,
            "blue" => HighlightColor::Blue,
            "pink" => HighlightColor::Pink,
            "purple" => HighlightColor::Purple,
            _ => HighlightColor::Yellow,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            HighlightColor::Yellow => "yellow",
            HighlightColor::Green => "green",
            HighlightColor::Blue => "blue",
            HighlightColor::Pink => "pink",
            HighlightColor::Purple => "purple",
        }
    }

    pub fn next(self) -> Self {
        match self {
            HighlightColor::Yellow => HighlightColor::Green,
            HighlightColor::Green => HighlightColor::Blue,
            HighlightColor::Blue => HighlightColor::Pink,
            HighlightColor::Pink => HighlightColor::Purple,
            HighlightColor::Purple => HighlightColor::Yellow,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HighlightRange {
    pub highlight_index: usize,
    pub row: usize,
    pub start_col: usize,
    pub end_col: usize,
    pub color: HighlightColor,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LinkEntry {
    pub row: usize,
    pub label: String,
    pub url: String,
    pub target_row: Option<usize>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TocEntry {
    pub label: String,
    pub content_index: usize,
    pub section: Option<String>,
}

/// Per-chapter bidirectional projection between wrapped rows and char offsets
/// into the normalized chapter source text.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SourceMap {
    /// One entry per chapter-local wrapped row. Synthetic rows use an empty
    /// span at the source offset carried from the preceding text row.
    pub row_spans: Vec<(u32, u32)>,
    pub source_len: u32,
    pub source_text: String,
    pub normalization_version: i64,
}

impl SourceMap {
    /// Return the source offset at the start of `row`. Rows beyond the mapped
    /// parser output (for example, chapter-break padding) clamp to source end.
    pub fn offset_for_row(&self, row: usize) -> usize {
        self.row_spans
            .get(row)
            .map_or(self.source_len as usize, |&(start, _)| start as usize)
    }

    /// Project a source offset back to a wrapped row. At boundaries and in
    /// separator gaps, prefer the following non-empty text row.
    pub fn row_for_offset(&self, offset: usize) -> usize {
        if self.row_spans.is_empty() {
            return 0;
        }

        let offset = offset.min(self.source_len as usize) as u32;
        let candidate = self.row_spans.partition_point(|&(_, end)| end <= offset);

        if let Some((row, _)) = self
            .row_spans
            .iter()
            .enumerate()
            .skip(candidate)
            .find(|&(_, &(start, end))| start < end && start <= offset)
        {
            return row;
        }

        self.row_spans
            .iter()
            .enumerate()
            .rfind(|&(_, &(start, end))| start < end && start <= offset)
            .map_or_else(
                || candidate.min(self.row_spans.len().saturating_sub(1)),
                |(row, _)| row,
            )
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct TextStructure {
    pub text_lines: Vec<String>,
    pub image_maps: HashMap<usize, String>,
    pub section_rows: HashMap<String, usize>,
    pub formatting: Vec<InlineStyle>,
    pub links: Vec<LinkEntry>,
    pub pagebreak_map: HashMap<usize, String>,
    /// Rows reserved for rendering an image inline, keyed by the image's
    /// placeholder row (same key as `image_maps`). The value is the total
    /// block height in rows, placeholder line included. Empty when inline
    /// images are off or the image's dimensions were unknown at parse time.
    pub image_block_rows: HashMap<usize, usize>,
    /// Starts of logical prose paragraphs, in absolute document rows.
    pub paragraph_starts: Vec<usize>,
    /// Blank rows inserted by typography options, in absolute document rows.
    pub typography_spacing_rows: HashSet<usize>,
    /// Chapter-local source coordinates. Combined book structures leave this
    /// empty; the reader retains this map on each chapter structure.
    pub source_map: SourceMap,
}

pub const CHAPTER_BREAK_MARKER: &str = "<repy:chapter-break>";

#[derive(Debug, Clone, PartialEq, Default)]
pub struct NoUpdate;

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    #[test]
    fn test_direction_default() {
        assert_eq!(Direction::default(), Direction::Forward);
    }

    #[test]
    fn test_direction_equality() {
        assert_eq!(Direction::Forward, Direction::Forward);
        assert_eq!(Direction::Backward, Direction::Backward);
        assert_ne!(Direction::Forward, Direction::Backward);
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
    fn test_book_metadata_with_values() {
        let metadata = BookMetadata {
            title: Some("Test Book".to_string()),
            creator: Some("Test Author".to_string()),
            description: Some("A test book".to_string()),
            publisher: Some("Test Publisher".to_string()),
            date: Some("2023-01-01".to_string()),
            language: Some("en".to_string()),
            format: Some("epub".to_string()),
            identifier: Some("test-id".to_string()),
            source: Some("test-source".to_string()),
        };

        assert_eq!(metadata.title, Some("Test Book".to_string()));
        assert_eq!(metadata.creator, Some("Test Author".to_string()));
        assert_eq!(metadata.description, Some("A test book".to_string()));
        assert_eq!(metadata.publisher, Some("Test Publisher".to_string()));
        assert_eq!(metadata.date, Some("2023-01-01".to_string()));
        assert_eq!(metadata.language, Some("en".to_string()));
        assert_eq!(metadata.format, Some("epub".to_string()));
        assert_eq!(metadata.identifier, Some("test-id".to_string()));
        assert_eq!(metadata.source, Some("test-source".to_string()));
    }

    #[test]
    fn test_library_item_creation() {
        let now = Utc::now();
        let item = LibraryItem {
            last_read: now,
            filepath: "/path/to/book.epub".to_string(),
            title: Some("Test Book".to_string()),
            author: Some("Test Author".to_string()),
            reading_progress: Some(0.5),
        };

        assert_eq!(item.last_read, now);
        assert_eq!(item.filepath, "/path/to/book.epub");
        assert_eq!(item.title, Some("Test Book".to_string()));
        assert_eq!(item.author, Some("Test Author".to_string()));
        assert_eq!(item.reading_progress, Some(0.5));
    }

    #[test]
    fn test_reading_state_default() {
        let state = ReadingState::default();
        assert_eq!(state.content_index, 0);
        assert_eq!(state.source_offset, None);
        assert_eq!(state.textwidth, 80);
        assert_eq!(state.row, 0);
        assert_eq!(state.rel_pctg, None);
        assert_eq!(state.section, None);
    }

    #[test]
    fn test_reading_state_with_values() {
        let state = ReadingState {
            content_index: 5,
            source_offset: Some(250),
            textwidth: 80,
            row: 100,
            rel_pctg: Some(0.75),
            section: Some("chapter-2".to_string()),
        };

        assert_eq!(state.content_index, 5);
        assert_eq!(state.source_offset, Some(250));
        assert_eq!(state.textwidth, 80);
        assert_eq!(state.row, 100);
        assert_eq!(state.rel_pctg, Some(0.75));
        assert_eq!(state.section, Some("chapter-2".to_string()));
    }

    #[test]
    fn test_search_data_default() {
        let search_data = SearchData::default();
        assert_eq!(search_data.direction, Direction::Forward);
        assert_eq!(search_data.value, "");
    }

    #[test]
    fn test_search_data_with_values() {
        let search_data = SearchData {
            direction: Direction::Backward,
            value: "test search".to_string(),
        };

        assert_eq!(search_data.direction, Direction::Backward);
        assert_eq!(search_data.value, "test search");
    }

    #[test]
    fn test_letters_count_default() {
        let letters_count = LettersCount::default();
        assert_eq!(letters_count.all, 0);
        assert!(letters_count.cumulative.is_empty());
    }

    #[test]
    fn test_letters_count_with_values() {
        let cumulative = vec![0, 50, 89, 120];
        let letters_count = LettersCount {
            all: 120,
            cumulative: cumulative.clone(),
        };

        assert_eq!(letters_count.all, 120);
        assert_eq!(letters_count.cumulative, cumulative);
    }

    #[test]
    fn test_char_pos_default() {
        let char_pos = CharPos::default();
        assert_eq!(char_pos.row, 0);
        assert_eq!(char_pos.col, 0);
    }

    #[test]
    fn test_char_pos_with_values() {
        let char_pos = CharPos { row: 5, col: 10 };
        assert_eq!(char_pos.row, 5);
        assert_eq!(char_pos.col, 10);
    }

    #[test]
    fn test_text_mark_creation() {
        let start = CharPos { row: 0, col: 3 };
        let end = CharPos { row: 1, col: 4 };
        let text_mark = TextMark {
            start,
            end: Some(end),
        };

        assert_eq!(text_mark.start, start);
        assert_eq!(text_mark.end, Some(end));
    }

    #[test]
    fn test_text_mark_no_end() {
        let start = CharPos { row: 0, col: 3 };
        let text_mark = TextMark { start, end: None };
        assert_eq!(text_mark.start, start);
        assert_eq!(text_mark.end, None);
    }

    #[test]
    fn test_text_mark_is_valid_same_row() {
        // Valid case: same row, start col <= end col
        let text_mark = TextMark {
            start: CharPos { row: 0, col: 3 },
            end: Some(CharPos { row: 0, col: 10 }),
        };
        assert!(text_mark.is_valid());

        // Valid case: same row, start col == end col
        let text_mark = TextMark {
            start: CharPos { row: 0, col: 5 },
            end: Some(CharPos { row: 0, col: 5 }),
        };
        assert!(text_mark.is_valid());

        // Invalid case: same row, start col > end col
        let text_mark = TextMark {
            start: CharPos { row: 0, col: 10 },
            end: Some(CharPos { row: 0, col: 3 }),
        };
        assert!(!text_mark.is_valid());
    }

    #[test]
    fn test_text_mark_is_valid_different_rows() {
        // Valid case: start row < end row
        let text_mark = TextMark {
            start: CharPos { row: 0, col: 10 },
            end: Some(CharPos { row: 1, col: 3 }),
        };
        assert!(text_mark.is_valid());

        // Valid case: start row << end row
        let text_mark = TextMark {
            start: CharPos { row: 2, col: 50 },
            end: Some(CharPos { row: 10, col: 5 }),
        };
        assert!(text_mark.is_valid());

        // Invalid case: start row > end row
        let text_mark = TextMark {
            start: CharPos { row: 5, col: 3 },
            end: Some(CharPos { row: 2, col: 10 }),
        };
        assert!(!text_mark.is_valid());
    }

    #[test]
    fn test_text_mark_is_valid_no_end() {
        // Invalid case: no end position
        let text_mark = TextMark {
            start: CharPos { row: 0, col: 3 },
            end: None,
        };
        assert!(!text_mark.is_valid());
    }

    #[test]
    fn test_text_span_creation() {
        let start = CharPos { row: 0, col: 3 };
        let text_span = TextSpan {
            start,
            n_letters: 10,
        };

        assert_eq!(text_span.start, start);
        assert_eq!(text_span.n_letters, 10);
    }

    #[test]
    fn test_inline_style_creation() {
        let style = InlineStyle {
            row: 3,
            col: 4,
            n_letters: 5,
            attr: 0x210000, // curses.A_BOLD equivalent placeholder
        };

        assert_eq!(style.row, 3);
        assert_eq!(style.col, 4);
        assert_eq!(style.n_letters, 5);
        assert_eq!(style.attr, 0x210000);
    }

    #[test]
    fn test_toc_entry_creation() {
        let entry = TocEntry {
            label: "Chapter 1".to_string(),
            content_index: 0,
            section: Some("chapter-1".to_string()),
        };

        assert_eq!(entry.label, "Chapter 1");
        assert_eq!(entry.content_index, 0);
        assert_eq!(entry.section, Some("chapter-1".to_string()));
    }

    #[test]
    fn test_toc_entry_no_section() {
        let entry = TocEntry {
            label: "Introduction".to_string(),
            content_index: 0,
            section: None,
        };

        assert_eq!(entry.label, "Introduction");
        assert_eq!(entry.content_index, 0);
        assert_eq!(entry.section, None);
    }

    #[test]
    fn test_text_structure_default() {
        let text_structure = TextStructure::default();
        assert!(text_structure.text_lines.is_empty());
        assert!(text_structure.image_maps.is_empty());
        assert!(text_structure.section_rows.is_empty());
        assert!(text_structure.formatting.is_empty());
        assert!(text_structure.links.is_empty());
        assert!(text_structure.pagebreak_map.is_empty());
    }

    #[test]
    fn test_text_structure_with_data() {
        let mut image_maps = std::collections::HashMap::new();
        image_maps.insert(10, "image1.jpg".to_string());

        let mut section_rows = std::collections::HashMap::new();
        section_rows.insert("chapter-1".to_string(), 5);

        let formatting = vec![InlineStyle {
            row: 0,
            col: 0,
            n_letters: 5,
            attr: 0x210000,
        }];

        let text_structure = TextStructure {
            source_map: Default::default(),
            text_lines: vec!["Line 1 of text".to_string(), "Line 2 of text".to_string()],
            image_maps,
            section_rows,
            formatting,
            links: vec![LinkEntry {
                row: 1,
                label: "Example".to_string(),
                url: "https://example.com".to_string(),
                target_row: None,
            }],
            pagebreak_map: std::collections::HashMap::new(),
            image_block_rows: std::collections::HashMap::new(),
            paragraph_starts: Vec::new(),
            typography_spacing_rows: std::collections::HashSet::new(),
        };

        assert_eq!(text_structure.text_lines.len(), 2);
        assert_eq!(text_structure.text_lines[0], "Line 1 of text");
        assert_eq!(
            text_structure.image_maps.get(&10),
            Some(&"image1.jpg".to_string())
        );
        assert_eq!(text_structure.section_rows.get("chapter-1"), Some(&5));
        assert_eq!(text_structure.formatting.len(), 1);
        assert_eq!(text_structure.formatting[0].n_letters, 5);
        assert_eq!(text_structure.links.len(), 1);
        assert_eq!(text_structure.links[0].url, "https://example.com");
    }

    #[test]
    fn test_no_update_default() {
        let no_update = NoUpdate::default();
        assert_eq!(no_update, NoUpdate);
    }

    // Edge case tests
    #[test]
    fn test_edge_cases() {
        // Test maximum u16 values for CharPos
        let max_pos = CharPos {
            row: u16::MAX,
            col: u16::MAX,
        };
        assert_eq!(max_pos.row, u16::MAX);
        assert_eq!(max_pos.col, u16::MAX);

        // Test empty cumulative vector
        let letters_count = LettersCount {
            all: 100,
            cumulative: vec![],
        };
        assert_eq!(letters_count.all, 100);
        assert!(letters_count.cumulative.is_empty());

        // Test TextMark with same start and end
        let same_pos = CharPos { row: 5, col: 10 };
        let same_mark = TextMark {
            start: same_pos,
            end: Some(same_pos),
        };
        assert!(same_mark.is_valid());
    }

    // Test clones work correctly
    #[test]
    fn test_clone_functionality() {
        let original = BookMetadata {
            title: Some("Original Title".to_string()),
            creator: None,
            description: None,
            publisher: None,
            date: None,
            language: None,
            format: None,
            identifier: None,
            source: None,
        };

        let cloned = original.clone();
        assert_eq!(original, cloned);
        assert_eq!(original.title, cloned.title);
    }
}
