use arboard::Clipboard;
use regex::Regex;
use serde::Deserialize;
use serde_json::Value;
use std::cell::RefCell;
use std::collections::HashMap;
use std::io;
use std::rc::Rc;
use std::sync::LazyLock;
use std::time::{Duration, Instant};

use chrono::{DateTime, Local, NaiveDate, Utc};
use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::{
    Frame, Terminal,
    backend::{Backend, CrosstermBackend},
    layout::{Constraint, Layout, Rect},
    style::Style,
    text::Line,
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};

use crate::annotations::{self, COMMENT_MAX_CHARS, NORMALIZATION_VERSION};
use crate::config::Config;
use crate::ebook::{Ebook, Epub, build_chapter_break};
use crate::logging;
use crate::models::{
    BookIdentity, BookMetadata, CHAPTER_BREAK_MARKER, Direction as AppDirection, Highlight,
    HighlightColor, HighlightRange, LibraryItem, LinkEntry, ReadingState, ReadingStatistics,
    SearchData, TextStructure, TocEntry, WindowType,
};
use crate::settings::DICT_PRESET_LIST;
use crate::state::State;
use crate::theme::{ColorTheme, Theme};
use crate::ui::board::Board;
use crate::ui::windows::{
    bookmarks::BookmarksWindow, dictionary::DictionaryWindow, fuzzy_filter_indices,
    help::HelpWindow, images::ImagesWindow, library::LibraryWindow, links::LinksWindow,
    metadata::MetadataWindow, search::SearchWindow, settings::SettingsWindow,
    statistics::StatisticsWindow, toc::TocWindow,
};

/// Regex to strip textwrap syllable-split hyphenation artifacts from TTS text.
/// Matches letter + hyphen + whitespace + lowercase letter (e.g. "ex- ample").
static RE_TTS_HYPHEN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"([A-Za-z])-\s+([a-z])").unwrap());
const READING_IDLE_TIMEOUT: Duration = Duration::from_secs(5 * 60);
/// Floor for the jump-detection threshold in reading statistics, used when
/// the terminal size is unknown or smaller than a typical screen.
const READING_JUMP_MIN_THRESHOLD_ROWS: usize = 50;
const DEFAULT_READING_WPM: f64 = 250.0;

fn previous_grapheme_boundary(text: &str, cursor: usize) -> usize {
    use unicode_segmentation::UnicodeSegmentation;
    text[..cursor]
        .grapheme_indices(true)
        .last()
        .map(|(idx, _)| idx)
        .unwrap_or(0)
}

fn next_grapheme_boundary(text: &str, cursor: usize) -> usize {
    use unicode_segmentation::UnicodeSegmentation;
    text[cursor..]
        .grapheme_indices(true)
        .nth(1)
        .map(|(idx, _)| cursor + idx)
        .unwrap_or(text.len())
}

fn key_matches_binding(key: &KeyEvent, binding: &str) -> bool {
    let mut chars = binding.chars();
    let Some(expected) = chars.next() else {
        return false;
    };
    if chars.next().is_some() {
        return false;
    }
    matches!(key.code, KeyCode::Char(actual) if actual == expected)
        && !key.modifiers.contains(KeyModifiers::CONTROL)
        && !key.modifiers.contains(KeyModifiers::ALT)
}

fn wrapped_cursor_position(text: &str, cursor: usize, wrap_width: u16) -> (u16, u16) {
    use unicode_segmentation::UnicodeSegmentation;
    use unicode_width::UnicodeWidthStr;

    let wrap_width = wrap_width.max(1) as usize;
    let mut row = 0usize;
    let mut col = 0usize;
    let before = &text[..cursor.min(text.len())];
    for grapheme in before.graphemes(true) {
        if grapheme == "\n" {
            row += 1;
            col = 0;
            continue;
        }
        let width = if grapheme == "\t" {
            4
        } else {
            UnicodeWidthStr::width(grapheme).max(1)
        };
        if col > 0 && col + width > wrap_width {
            row += 1;
            col = 0;
        }
        col += width;
    }
    (
        row.min(u16::MAX as usize) as u16,
        col.min(u16::MAX as usize) as u16,
    )
}

/// Application state that encompasses all UI and reading state
#[derive(Debug, Clone)]
pub struct ApplicationState {
    pub reading_state: ReadingState,
    pub config: Config,
    pub search_data: Option<SearchData>,
    pub ui_state: UiState,
    pub should_quit: bool,
    pub count_prefix: String, // For command repetition (e.g., "5j")
    pub jump_history: Vec<usize>,
    pub jump_history_index: usize,
    pub marks: HashMap<char, ReadingState>,
    pub book_color_theme: Option<ColorTheme>,
}

impl ApplicationState {
    pub fn new(config: Config) -> Self {
        Self {
            reading_state: ReadingState::default(),
            config,
            search_data: None,
            ui_state: UiState::new(),
            should_quit: false,
            count_prefix: String::new(),
            jump_history: Vec::new(),
            jump_history_index: 0,
            marks: HashMap::new(),
            book_color_theme: None,
        }
    }

    pub fn theme(&self) -> Theme {
        Theme::for_color_theme(self.effective_color_theme())
    }

    pub fn effective_color_theme(&self) -> ColorTheme {
        self.book_color_theme
            .unwrap_or(self.config.settings.color_theme)
    }

    pub fn record_jump(&mut self) {
        let current_row = self.reading_state.row;

        // If we are in the middle of history (index < len), truncate the future
        if self.jump_history_index < self.jump_history.len() {
            self.jump_history.truncate(self.jump_history_index);
        }

        // Avoid duplicate consecutive entries
        if self.jump_history.last() != Some(&current_row) {
            self.jump_history.push(current_row);
            // Limit history size (optional, e.g., 100 entries)
            if self.jump_history.len() > 100 {
                self.jump_history.remove(0);
            }
        }

        self.jump_history_index = self.jump_history.len();
    }

    pub fn jump_back(&mut self) {
        if self.jump_history.is_empty() {
            return;
        }

        if self.jump_history_index == self.jump_history.len() {
            let current_row = self.reading_state.row;
            if self.jump_history.last() != Some(&current_row) {
                self.jump_history.push(current_row);
            }
            // We are now at the "tip". To jump back, we start from the last element.
            self.jump_history_index = self.jump_history.len().saturating_sub(1);
        }

        if self.jump_history_index > 0 {
            self.jump_history_index -= 1;
            let row = self.jump_history[self.jump_history_index];
            self.reading_state.row = row;
        }
    }

    pub fn jump_forward(&mut self) {
        if self.jump_history.is_empty() {
            return;
        }

        if self.jump_history_index + 1 < self.jump_history.len() {
            self.jump_history_index += 1;
            let row = self.jump_history[self.jump_history_index];
            self.reading_state.row = row;
        }
    }
}

/// UI-specific state management
#[derive(Debug, Clone)]
pub struct UiState {
    pub active_window: WindowType,
    pub show_help: bool,
    pub show_toc: bool,
    pub show_bookmarks: bool,
    pub show_library: bool,
    pub show_search: bool,
    pub show_links: bool,
    pub show_images: bool,
    pub show_metadata: bool,
    pub show_statistics: bool,
    pub show_dictionary: bool,
    pub show_settings: bool,
    pub show_highlights: bool,
    pub search_query: String,
    /// True once Enter confirmed the query (j/k then navigate results).
    pub search_committed: bool,
    /// Reader row when the search window opened; restored on Esc while typing.
    pub search_origin_row: usize,
    /// Persisted search history, most recent first (loaded when `/` opens).
    pub search_history: Vec<String>,
    /// Position while browsing history with Up/Down (None = editing draft).
    pub search_history_index: Option<usize>,
    /// The query being typed before history browsing started.
    pub search_history_draft: String,
    pub search_results: Vec<SearchResult>,
    pub search_matches: HashMap<usize, Vec<(usize, usize)>>,
    pub selected_search_result: usize,
    pub toc_entries: Vec<TocEntry>,
    pub toc_selected_index: usize,
    /// True while the user is typing a `/`-filter query in a list window.
    pub list_filter_active: bool,
    /// The fuzzy-filter query for the currently open list window.
    pub list_filter_query: String,
    /// Original indices of items matching the filter, best score first.
    /// `None` means no filter is applied and selection indices are direct.
    pub list_filter_indices: Option<Vec<usize>>,
    pub bookmarks: Vec<(String, ReadingState)>,
    pub bookmarks_selected_index: usize,
    pub book_identity: Option<BookIdentity>,
    pub highlights: Vec<Highlight>,
    pub highlights_selected_index: usize,
    pub highlight_ranges: HashMap<usize, Vec<HighlightRange>>,
    pub highlight_comment_buffer: String,
    pub highlight_comment_cursor: usize,
    pub highlight_comment_editing_id: Option<String>,
    pub pending_delete_highlight: Option<Highlight>,
    /// Color used for the next created highlight (last used wins).
    pub next_highlight_color: HighlightColor,
    pub links: Vec<LinkEntry>,
    pub links_selected_index: usize,
    pub link_preview: Option<LinkEntry>,
    pub images_list: Vec<(usize, String)>,
    pub images_selected_index: usize,
    pub library_items: Vec<LibraryItem>,
    pub library_selected_index: usize,
    pub metadata: Option<BookMetadata>,
    pub statistics: ReadingStatistics,
    pub dictionary_word: String,
    pub dictionary_definition: String,
    pub dictionary_client_used: String,
    pub dictionary_scroll_offset: u16,
    pub dictionary_command_query: String,
    pub settings_selected_index: usize,
    pub dictionary_loading: bool,
    pub dictionary_is_wikipedia: bool,
    pub message: Option<String>,
    pub message_type: MessageType,
    pub message_time: Option<Instant>,
    pub visual_anchor: Option<(usize, usize)>,
    pub visual_cursor: Option<(usize, usize)>,
    pub help_scroll_offset: u16,
    pub tts_active: bool,
    /// Per-line underline ranges for the TTS chunk being read.
    /// Maps line_num -> (start_col, end_col_exclusive) in characters.
    pub tts_underline_ranges: HashMap<usize, (usize, usize)>,
    pub tts_converting: bool,
    pub tts_anim_frame: usize,
    /// True while the user is typing a `/`-search query inside cursor/selection mode.
    pub visual_search_input_active: bool,
    /// Query last submitted (or being typed) for visual-mode `/`-search.
    pub visual_search_query: String,
    /// Matches found by the last visual-mode `/`-search, in absolute line coordinates.
    /// Each entry is `(start_line, start_col, end_line, end_col_exclusive)` with char-based columns.
    pub visual_search_matches: Vec<(usize, usize, usize, usize)>,
    pub visual_search_selected: usize,
    /// Set after `f`/`F`/`t`/`T` in cursor/selection mode; the next char
    /// keypress becomes the find target. Stores the count typed before the
    /// motion key (e.g. `2` in `2fa`) so it survives the intermediate key.
    pub pending_visual_find: Option<(VisualFindDirection, u32)>,
    pub pending_mark_command: Option<PendingMarkCommand>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PendingMarkCommand {
    Set,
    Jump,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VisualFindDirection {
    Forward,
    Backward,
    TillForward,
    TillBackward,
}

impl Default for UiState {
    fn default() -> Self {
        Self::new()
    }
}

impl UiState {
    pub fn new() -> Self {
        Self {
            active_window: WindowType::Reader,
            show_help: false,
            show_toc: false,
            show_bookmarks: false,
            show_library: false,
            show_search: false,
            show_links: false,
            show_images: false,
            show_metadata: false,
            show_statistics: false,
            show_dictionary: false,
            show_settings: false,
            show_highlights: false,
            search_query: String::new(),
            search_committed: false,
            search_origin_row: 0,
            search_history: Vec::new(),
            search_history_index: None,
            search_history_draft: String::new(),
            search_results: Vec::new(),
            search_matches: HashMap::new(),
            selected_search_result: 0,
            toc_entries: Vec::new(),
            toc_selected_index: 0,
            list_filter_active: false,
            list_filter_query: String::new(),
            list_filter_indices: None,
            bookmarks: Vec::new(),
            bookmarks_selected_index: 0,
            book_identity: None,
            highlights: Vec::new(),
            highlights_selected_index: 0,
            highlight_ranges: HashMap::new(),
            highlight_comment_buffer: String::new(),
            highlight_comment_cursor: 0,
            highlight_comment_editing_id: None,
            pending_delete_highlight: None,
            next_highlight_color: HighlightColor::default(),
            links: Vec::new(),
            links_selected_index: 0,
            link_preview: None,
            images_list: Vec::new(),
            images_selected_index: 0,
            library_items: Vec::new(),
            library_selected_index: 0,
            metadata: None,
            statistics: ReadingStatistics::default(),
            dictionary_word: String::new(),
            dictionary_definition: String::new(),
            dictionary_client_used: String::new(),
            dictionary_scroll_offset: 0,
            dictionary_command_query: String::new(),
            settings_selected_index: 0,
            dictionary_loading: false,
            dictionary_is_wikipedia: false,
            message: None,
            message_type: MessageType::Info,
            message_time: None,
            visual_anchor: None,
            visual_cursor: None,
            help_scroll_offset: 0,
            tts_active: false,
            tts_underline_ranges: HashMap::new(),
            tts_converting: false,
            tts_anim_frame: 0,
            visual_search_input_active: false,
            visual_search_query: String::new(),
            visual_search_matches: Vec::new(),
            visual_search_selected: 0,
            pending_visual_find: None,
            pending_mark_command: None,
        }
    }

    pub fn set_message(&mut self, message: String, message_type: MessageType) {
        self.message = Some(message);
        self.message_type = message_type;
        self.message_time = Some(Instant::now());
    }

    pub fn clear_message(&mut self) {
        self.message = None;
        self.message_time = None;
    }

    /// Returns true if the current message has expired (older than 3 seconds).
    pub fn message_expired(&self) -> bool {
        self.message_time
            .is_some_and(|t| t.elapsed() >= Duration::from_secs(3))
    }

    pub fn clear_list_filter(&mut self) {
        self.list_filter_active = false;
        self.list_filter_query.clear();
        self.list_filter_indices = None;
    }

    /// Map a selection in the (possibly filtered) list view back to the
    /// index in the underlying list. `None` when the filter has no matches.
    pub fn selected_list_index(&self, selected: usize) -> Option<usize> {
        match &self.list_filter_indices {
            Some(indices) => indices.get(selected).copied(),
            None => Some(selected),
        }
    }

    /// Number of items visible in the current list view.
    pub fn filtered_list_len(&self, full_len: usize) -> usize {
        match &self.list_filter_indices {
            Some(indices) => indices.len(),
            None => full_len,
        }
    }

    /// Text shown at the bottom of a list window while a filter is set.
    pub fn list_filter_status(&self) -> Option<String> {
        if self.list_filter_active {
            Some(format!(" /{}█ ", self.list_filter_query))
        } else if self.list_filter_indices.is_some() {
            Some(format!(" /{} ", self.list_filter_query))
        } else {
            None
        }
    }

    pub fn open_window(&mut self, window_type: WindowType) {
        self.active_window = window_type.clone();
        // Any window change invalidates the list filter.
        self.clear_list_filter();
        match window_type {
            WindowType::Reader => {
                self.show_help = false;
                self.show_toc = false;
                self.show_bookmarks = false;
                self.show_library = false;
                self.show_search = false;
                self.show_links = false;
                self.show_images = false;
                self.show_metadata = false;
                self.show_statistics = false;
                self.show_dictionary = false;
                self.show_settings = false;
                self.show_highlights = false;
                self.visual_anchor = None;
                self.visual_cursor = None;
                self.pending_visual_find = None;
                self.pending_mark_command = None;
                self.link_preview = None;
            }
            WindowType::Help => {
                self.show_help = true;
                self.help_scroll_offset = 0;
            }
            WindowType::Toc => self.show_toc = true,
            WindowType::Bookmarks => self.show_bookmarks = true,
            WindowType::Library => self.show_library = true,
            WindowType::Search => self.show_search = true,
            WindowType::Links => self.show_links = true,
            WindowType::Images => self.show_images = true,
            WindowType::Metadata => self.show_metadata = true,
            WindowType::Statistics => self.show_statistics = true,
            WindowType::Dictionary => {
                self.show_dictionary = true;
                self.dictionary_scroll_offset = 0;
            }
            WindowType::Settings => self.show_settings = true,
            WindowType::Visual => {}
            WindowType::DictionaryCommandInput => {
                self.show_settings = false;
            }
            WindowType::Highlights => self.show_highlights = true,
            WindowType::HighlightCommentEditor => {
                self.show_highlights = false;
            }
            WindowType::ConfirmDeleteHighlight => {
                self.show_highlights = false;
            }
            WindowType::LinkPreview => {
                self.show_links = false;
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum MessageType {
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone)]
pub struct SearchResult {
    pub line: usize,
    pub ranges: Vec<(usize, usize)>,
    pub preview: String,
}

pub struct DictionaryResult {
    pub word: String,
    pub definition: Result<String, String>,
    pub client: String,
}

#[derive(Debug, Clone)]
struct WikipediaLookupResult {
    url: String,
    summary: String,
}

#[derive(Debug, Deserialize)]
struct WikipediaSummaryResponse {
    query: Option<WikipediaQueryData>,
}

#[derive(Debug, Deserialize)]
struct WikipediaQueryData {
    pages: Value,
}

#[derive(Debug, Deserialize)]
struct WikipediaSearchResponse {
    query: Option<WikipediaSearchQuery>,
}

#[derive(Debug, Deserialize)]
struct WikipediaSearchQuery {
    search: Vec<WikipediaSearchHit>,
}

#[derive(Debug, Deserialize)]
struct WikipediaSearchHit {
    title: String,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum SettingItem {
    ShowLineNumbers,
    MouseSupport,
    PageScrollAnimation,
    ShowProgressIndicator,
    SeamlessBetweenChapters,
    DictionaryClient,
    TtsEngine,
    Width,
    ShowTopBar,
    ColorTheme,
}

impl SettingItem {
    fn all() -> &'static [SettingItem] {
        &[
            SettingItem::ShowLineNumbers,
            SettingItem::MouseSupport,
            SettingItem::PageScrollAnimation,
            SettingItem::ShowProgressIndicator,
            SettingItem::SeamlessBetweenChapters,
            SettingItem::DictionaryClient,
            SettingItem::TtsEngine,
            SettingItem::Width,
            SettingItem::ShowTopBar,
            SettingItem::ColorTheme,
        ]
    }
}

/// Audio player backend for the pipelined edge-tts engine.
#[derive(Clone, Debug)]
enum EdgeTtsPlayer {
    Mpv,
    Ffplay,
}

const TTS_PREFETCH_WINDOW: usize = 4;

impl EdgeTtsPlayer {
    fn program(&self) -> &'static str {
        match self {
            Self::Mpv => "mpv",
            Self::Ffplay => "ffplay",
        }
    }
    fn args(&self, path: &std::path::Path) -> Vec<String> {
        let p = path.to_string_lossy().into_owned();
        match self {
            Self::Mpv => vec!["--really-quiet".into(), "--no-video".into(), p],
            Self::Ffplay => vec![
                "-nodisp".into(),
                "-autoexit".into(),
                "-loglevel".into(),
                "quiet".into(),
                p,
            ],
        }
    }
}

/// A single TTS chunk: the text to speak, the first display line it
/// touches (for scrolling), and the per-line underline column ranges.
struct TtsChunk {
    text: String,
    first_line: usize,
    /// line_num → (start_col, end_col_exclusive) in display characters
    underline: HashMap<usize, (usize, usize)>,
}

struct ActiveReadingSession {
    book_id: String,
    started_at: DateTime<Utc>,
    last_activity: Instant,
    last_activity_at: DateTime<Utc>,
    /// High-water mark: rows/words up to this row have already been counted,
    /// so re-reading (scrolling back up and down) is not double-counted.
    max_counted_row: usize,
    rows: usize,
    words: usize,
}

/// DB-derived reading statistics cached off the per-keypress path; refreshed
/// only when a session row is inserted, the book changes, or the Statistics
/// window opens.
struct CachedStatistics {
    book_id: Option<String>,
    stats: ReadingStatistics,
    /// Day assumed active for `streaks_with_day` (the running session's day).
    streak_day: NaiveDate,
    /// Streaks computed as if `streak_day` were a recorded reading day.
    streaks_with_day: (usize, usize),
}

enum TtsWorkerCommand {
    UpdatePlaybackIndex(usize),
    Stop,
}

enum TtsWorkerEvent {
    Ready {
        index: usize,
        path: std::path::PathBuf,
    },
    Failed {
        index: usize,
    },
}

/// Main reader application struct
pub struct Reader<B: Backend = CrosstermBackend<io::Stdout>> {
    state: Rc<RefCell<ApplicationState>>,
    terminal: Terminal<B>,
    db_state: State,
    board: Board,
    clipboard: Option<Clipboard>,
    ebook: Option<Epub>,
    content_start_rows: Vec<usize>,
    /// Per-chapter text structures for incremental rebuilds
    chapter_text_structures: Vec<TextStructure>,
    /// Text width used for the current chapter structures
    current_text_width: Option<usize>,
    dictionary_res_rx: Option<std::sync::mpsc::Receiver<DictionaryResult>>,
    /// Channel to receive notification when a TTS chunk finishes speaking
    tts_done_rx: Option<std::sync::mpsc::Receiver<()>>,
    /// Handle to the running TTS child process
    tts_child: Option<std::process::Child>,
    /// Precomputed TTS chunks with text and per-line underline ranges
    tts_chunks: Vec<TtsChunk>,
    /// Index into tts_chunks for the chunk currently being spoken
    tts_chunk_index: usize,
    /// PID of the running TTS process for killing (entire process group)
    tts_kill_pid: Option<u32>,
    /// Detected audio player for edge-tts pipeline (mpv or ffplay)
    tts_audio_player: Option<EdgeTtsPlayer>,
    /// Path of the temp audio file currently being played
    tts_current_audio_path: Option<std::path::PathBuf>,
    /// Converted audio chunks that are ready to play, keyed by chunk index.
    tts_ready_audio: HashMap<usize, std::path::PathBuf>,
    /// Background worker command channel for bounded chunk conversion.
    tts_worker_tx: Option<std::sync::mpsc::Sender<TtsWorkerCommand>>,
    /// Background worker event channel delivering ready/failed conversion results.
    tts_worker_rx: Option<std::sync::mpsc::Receiver<TtsWorkerEvent>>,
    /// The TTS engine in use for the current session (needed for prefetch after async play)
    tts_current_engine: String,
    /// Session-scoped temp dir for generated TTS audio files.
    tts_temp_dir: Option<std::path::PathBuf>,
    /// Active reading-statistics session, flushed on idle, book switch, or quit.
    reading_session: Option<ActiveReadingSession>,
    /// Cached DB-side reading statistics; see [`CachedStatistics`].
    cached_statistics: Option<CachedStatistics>,
}

impl Reader {
    /// Create a new Reader instance
    pub fn new(config: Config) -> eyre::Result<Self> {
        Self::with_backend(config, CrosstermBackend::new(io::stdout()), State::new()?)
    }
}

impl<B: Backend> Reader<B>
where
    B::Error: std::error::Error + Send + Sync + 'static,
{
    fn split_dictionary_command_template(template: &str) -> eyre::Result<Vec<(String, bool)>> {
        let mut args = Vec::new();
        let mut current = String::new();
        let mut chars = template.chars().peekable();
        let mut in_single = false;
        let mut in_double = false;
        let mut was_quoted = false;

        while let Some(ch) = chars.next() {
            match ch {
                '\\' if !in_single => {
                    if let Some(next) = chars.next() {
                        current.push(next);
                    } else {
                        return Err(eyre::eyre!(
                            "Invalid dictionary command template: trailing escape"
                        ));
                    }
                }
                '\'' if !in_double => {
                    in_single = !in_single;
                    was_quoted = true;
                }
                '"' if !in_single => {
                    in_double = !in_double;
                    was_quoted = true;
                }
                c if c.is_whitespace() && !in_single && !in_double => {
                    if !current.is_empty() {
                        args.push((std::mem::take(&mut current), was_quoted));
                        was_quoted = false;
                    }
                }
                _ => current.push(ch),
            }
        }

        if in_single || in_double {
            return Err(eyre::eyre!(
                "Invalid dictionary command template: unmatched quote"
            ));
        }
        if !current.is_empty() {
            args.push((current, was_quoted));
        }
        Ok(args)
    }

    fn build_dictionary_command(
        template: &str,
        query: &str,
    ) -> eyre::Result<(String, Vec<String>)> {
        let parts = Self::split_dictionary_command_template(template)?;
        if parts.is_empty() {
            return Err(eyre::eyre!("Dictionary command template is empty"));
        }

        let mut has_placeholder = false;
        let mut processed_parts = Vec::new();

        for (mut part, quoted) in parts {
            if part.contains("%q") {
                let substituted = if quoted {
                    // If it was quoted, we should escape internal quotes to be safe
                    query.replace('"', "\\\"")
                } else {
                    query.to_string()
                };
                part = part.replace("%q", &substituted);
                has_placeholder = true;
            }
            processed_parts.push(part);
        }

        if !has_placeholder {
            processed_parts.push(query.to_string());
        }

        let program = processed_parts.remove(0);
        Ok((program, processed_parts))
    }

    fn run_dictionary_client(
        client: &str,
        query: &str,
        timeout: Duration,
    ) -> eyre::Result<std::process::Output> {
        let mut child = match client {
            "sdcv" => std::process::Command::new("sdcv")
                .arg("-n")
                .arg(query)
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()?,
            "dict" => std::process::Command::new("dict")
                .arg(query)
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()?,
            "wkdict" => std::process::Command::new("wkdict")
                .arg(query)
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()?,
            template => {
                let (program, args) = Self::build_dictionary_command(template, query)?;
                std::process::Command::new(program)
                    .args(args)
                    .stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::piped())
                    .spawn()?
            }
        };

        let start = Instant::now();
        loop {
            match child.try_wait()? {
                Some(_) => return Ok(child.wait_with_output()?),
                None => {
                    if start.elapsed() >= timeout {
                        child.kill()?;
                        return Err(eyre::eyre!(
                            "Dictionary query timed out after {}s",
                            timeout.as_secs()
                        ));
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
            }
        }
    }

    /// Detect the Wikipedia language code based on the script of the query text.
    /// ASCII text is treated as English and uses Simple English Wikipedia.
    /// Non-ASCII text is mapped to the appropriate language Wikipedia.
    fn detect_wikipedia_language(query: &str) -> String {
        let trimmed = query.trim();
        if trimmed.is_ascii() {
            return "simple".to_string();
        }
        // Detect language from the dominant non-ASCII script
        for ch in trimmed.chars() {
            if ch.is_ascii() {
                continue;
            }
            return match ch {
                '\u{4E00}'..='\u{9FFF}' | '\u{3400}'..='\u{4DBF}' | '\u{F900}'..='\u{FAFF}' => {
                    "zh".to_string()
                }
                '\u{3040}'..='\u{309F}' | '\u{30A0}'..='\u{30FF}' => "ja".to_string(),
                '\u{AC00}'..='\u{D7AF}' | '\u{1100}'..='\u{11FF}' => "ko".to_string(),
                '\u{0400}'..='\u{04FF}' => "ru".to_string(),
                '\u{0600}'..='\u{06FF}' => "ar".to_string(),
                '\u{0E00}'..='\u{0E7F}' => "th".to_string(),
                '\u{0900}'..='\u{097F}' => "hi".to_string(),
                '\u{0980}'..='\u{09FF}' => "bn".to_string(),
                '\u{0A80}'..='\u{0AFF}' => "gu".to_string(),
                '\u{0B80}'..='\u{0BFF}' => "ta".to_string(),
                '\u{0370}'..='\u{03FF}' => "el".to_string(),
                '\u{0590}'..='\u{05FF}' => "he".to_string(),
                '\u{1000}'..='\u{109F}' => "my".to_string(),
                '\u{10A0}'..='\u{10FF}' => "ka".to_string(),
                '\u{0530}'..='\u{058F}' => "hy".to_string(),
                '\u{1780}'..='\u{17FF}' => "km".to_string(),
                '\u{0D00}'..='\u{0D7F}' => "ml".to_string(),
                '\u{0C80}'..='\u{0CFF}' => "kn".to_string(),
                '\u{0C00}'..='\u{0C7F}' => "te".to_string(),
                // Latin-extended characters (accented) — could be many European languages,
                // fall back to English Wikipedia which has the broadest coverage
                '\u{00C0}'..='\u{024F}' => "en".to_string(),
                _ => "en".to_string(),
            };
        }
        "simple".to_string()
    }

    fn build_wikipedia_page_url(language: &str, title: &str) -> eyre::Result<String> {
        let base = if language.starts_with("http://") || language.starts_with("https://") {
            language.trim_end_matches('/').to_string()
        } else {
            format!("https://{language}.wikipedia.org")
        };

        let mut page_url = reqwest::Url::parse(&format!("{base}/wiki/"))?;
        page_url
            .path_segments_mut()
            .map_err(|_| eyre::eyre!("Could not build Wikipedia page URL"))?
            .push(title);
        Ok(page_url.to_string())
    }

    fn wikipedia_api_url(language: &str) -> String {
        if language.starts_with("http://") || language.starts_with("https://") {
            format!("{}/w/api.php", language.trim_end_matches('/'))
        } else {
            format!("https://{language}.wikipedia.org/w/api.php")
        }
    }

    fn fetch_wikipedia_summary(
        client: &reqwest::blocking::Client,
        language: &str,
        title: &str,
    ) -> eyre::Result<Option<WikipediaLookupResult>> {
        let summary_url = Self::wikipedia_api_url(language);
        let response = client
            .get(summary_url)
            .query(&[
                ("action", "query"),
                ("format", "json"),
                ("redirects", "1"),
                ("prop", "extracts|info|pageprops"),
                ("inprop", "url"),
                ("explaintext", "1"),
                ("exintro", "1"),
                ("titles", title),
            ])
            .send()?
            .error_for_status()?;
        let parsed: WikipediaSummaryResponse = response.json()?;
        Ok(Self::parse_wikipedia_summary_response(
            &parsed, language, title,
        )?)
    }

    fn parse_wikipedia_summary_response(
        parsed: &WikipediaSummaryResponse,
        language: &str,
        title: &str,
    ) -> eyre::Result<Option<WikipediaLookupResult>> {
        let Some(query) = parsed.query.as_ref() else {
            return Ok(None);
        };
        let Some(pages_obj) = query.pages.as_object() else {
            return Ok(None);
        };

        for page in pages_obj.values() {
            let Some(page_obj) = page.as_object() else {
                continue;
            };

            if page_obj.contains_key("missing") {
                continue;
            }

            let is_disambiguation = page_obj
                .get("pageprops")
                .and_then(Value::as_object)
                .is_some_and(|pp| pp.contains_key("disambiguation"));
            if is_disambiguation {
                continue;
            }

            let summary = page_obj
                .get("extract")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_string();
            if summary.is_empty() {
                continue;
            }

            let title_for_url = page_obj
                .get("title")
                .and_then(Value::as_str)
                .unwrap_or(title);
            let url = page_obj
                .get("fullurl")
                .and_then(Value::as_str)
                .map(str::to_string)
                .filter(|u| !u.trim().is_empty())
                .unwrap_or(Self::build_wikipedia_page_url(language, title_for_url)?);

            return Ok(Some(WikipediaLookupResult { url, summary }));
        }

        Ok(None)
    }

    fn search_wikipedia_titles(
        client: &reqwest::blocking::Client,
        language: &str,
        query: &str,
        limit: usize,
    ) -> eyre::Result<Vec<String>> {
        let search_url = Self::wikipedia_api_url(language);
        let response = client
            .get(search_url)
            .query(&[
                ("action", "query"),
                ("list", "search"),
                ("format", "json"),
                ("srsearch", query),
                ("srlimit", &limit.to_string()),
            ])
            .send()?
            .error_for_status()?;

        let parsed: WikipediaSearchResponse = response.json()?;
        Ok(Self::extract_search_titles(parsed))
    }

    fn extract_search_titles(parsed: WikipediaSearchResponse) -> Vec<String> {
        parsed
            .query
            .map(|q| q.search.into_iter().map(|h| h.title).collect())
            .unwrap_or_default()
    }

    fn wikipedia_lookup_summary(
        query: &str,
        language: &str,
        timeout: Duration,
    ) -> eyre::Result<WikipediaLookupResult> {
        let mut builder = reqwest::blocking::Client::builder()
            .timeout(timeout)
            .user_agent("repy");
        if language.starts_with("http://127.0.0.1") || language.starts_with("http://localhost") {
            builder = builder.no_proxy();
        }
        let client = builder.build()?;

        if let Some(result) = Self::fetch_wikipedia_summary(&client, language, query)? {
            return Ok(result);
        }

        let candidates = Self::search_wikipedia_titles(&client, language, query, 3)?;
        for candidate in candidates {
            if let Some(result) = Self::fetch_wikipedia_summary(&client, language, &candidate)? {
                return Ok(result);
            }
        }

        Err(eyre::eyre!("No Wikipedia summary found for '{}'", query))
    }

    fn normalize_ebook_path(path: &str) -> String {
        if path.is_empty() {
            return path.to_string();
        }

        match std::fs::canonicalize(path) {
            Ok(canonical) => canonical.to_string_lossy().to_string(),
            Err(err) => {
                logging::debug(format!(
                    "Could not canonicalize ebook path {}: {}",
                    path, err
                ));
                path.to_string()
            }
        }
    }

    /// Build a Reader on top of an arbitrary backend, e.g. `TestBackend` in tests.
    fn with_backend(config: Config, backend: B, db_state: State) -> eyre::Result<Self> {
        let terminal = Terminal::new(backend)?;

        let app_state = ApplicationState::new(config);

        Ok(Self {
            state: Rc::new(RefCell::new(app_state)),
            terminal,
            db_state,
            board: Board::new(),
            clipboard: Clipboard::new().ok(),
            ebook: None,
            content_start_rows: Vec::new(),
            chapter_text_structures: Vec::new(),
            current_text_width: None,
            dictionary_res_rx: None,
            tts_done_rx: None,
            tts_child: None,
            tts_chunks: Vec::new(),
            tts_chunk_index: 0,
            tts_kill_pid: None,
            tts_audio_player: None,
            tts_current_audio_path: None,
            tts_ready_audio: HashMap::new(),
            tts_worker_tx: None,
            tts_worker_rx: None,
            tts_current_engine: String::new(),
            tts_temp_dir: None,
            reading_session: None,
            cached_statistics: None,
        })
    }

    /// Extract the current UI state into a single frame draw.
    fn draw(&mut self) -> eyre::Result<()> {
        let state = self.state.clone();
        self.terminal.draw(|f| {
            let state_ref = state.borrow();
            Self::render_static(f, &state_ref, &self.board, &self.content_start_rows);
        })?;
        Ok(())
    }

    /// Load the most recently read ebook, if any, using the database
    pub fn load_last_ebook_if_any(&mut self) -> eyre::Result<()> {
        if let Some(filepath) = self.db_state.get_last_read()?
            && std::path::Path::new(&filepath).exists()
        {
            self.load_ebook(&filepath)?;
        }
        Ok(())
    }

    pub fn load_ebook(&mut self, path: &str) -> eyre::Result<()> {
        self.finish_reading_session(Utc::now())?;

        let normalized_path = Self::normalize_ebook_path(path);
        if normalized_path != path {
            self.db_state.reconcile_filepath(path, &normalized_path)?;
        }

        let mut epub = Epub::new(&normalized_path);
        epub.initialize()?;
        let identity = annotations::derive_book_identity(&mut epub)?;
        let alias_conflict = self
            .db_state
            .alias_conflict(&normalized_path, &identity)?
            .is_some();
        if alias_conflict {
            self.db_state.upsert_book_record(&identity)?;
        } else {
            self.db_state
                .upsert_book_identity(&normalized_path, &identity)?;
        }

        // If this same book is already in the library under a different path
        // (e.g. opened from a new location), migrate the existing entry to the
        // current path instead of adding a duplicate. This preserves reading
        // progress, position, and bookmarks.
        if !alias_conflict {
            if let Some(existing_path) = self
                .db_state
                .find_other_library_path_for_book(&identity.book_id, &normalized_path)?
            {
                self.db_state
                    .reconcile_filepath(&existing_path, &normalized_path)?;
            }
        }

        // Load last reading state early to get preferred textwidth
        let db_state = self.db_state.get_last_reading_state(&epub).ok();

        // Determine textwidth: use DB value if available, otherwise use config default (70)
        let textwidth = if let Some(ref s) = db_state {
            s.textwidth
        } else {
            self.state.borrow().config.settings.width.unwrap_or(70)
        };

        // Calculate padding from textwidth for rendering
        let term_width = match crossterm::terminal::size() {
            Ok((w, _)) => w as usize,
            Err(_) => 100,
        };
        let padding = if term_width <= 20 {
            0 // Minimum width for very small windows
        } else {
            (term_width.saturating_sub(textwidth) / 2).max(5)
        };
        let text_width = term_width.saturating_sub(padding * 2).max(20);

        // Also update the state with the decided textwidth immediately so we are consistent
        if let Some(mut s) = db_state.clone() {
            s.textwidth = textwidth;
        }

        let page_height = self.chapter_break_page_height();
        let all_content = epub.get_all_parsed_content(text_width, page_height)?;

        // Store per-chapter structures for incremental rebuilds
        self.chapter_text_structures = all_content;
        self.current_text_width = Some(text_width);

        let mut combined_text_structure = TextStructure::default();
        let mut content_start_rows = Vec::with_capacity(self.chapter_text_structures.len());
        let mut row_offset = 0;
        for ts in &self.chapter_text_structures {
            content_start_rows.push(row_offset);
            row_offset += ts.text_lines.len();
            combined_text_structure
                .text_lines
                .extend(ts.text_lines.clone());
            combined_text_structure
                .image_maps
                .extend(ts.image_maps.clone());
            combined_text_structure
                .section_rows
                .extend(ts.section_rows.clone());
            combined_text_structure
                .formatting
                .extend(ts.formatting.clone());
            combined_text_structure.links.extend(ts.links.clone());
            combined_text_structure
                .pagebreak_map
                .extend(ts.pagebreak_map.clone());
        }

        self.board.update_text_structure(combined_text_structure);
        self.ebook = Some(epub);
        self.content_start_rows = content_start_rows;

        // Add the book to library immediately upon opening
        if let Some(epub) = self.ebook.as_ref() {
            // First, persist the reading state and update library
            let mut reading_state = if let Some(s) = db_state {
                s.clone()
            } else {
                ReadingState::default()
            };
            reading_state.textwidth = textwidth;

            let total_lines = self.board.total_lines();
            if total_lines > 0 && reading_state.row >= total_lines {
                reading_state.row = total_lines - 1;
            }

            // Persist the reading state first (required for foreign key constraint)
            self.db_state.set_last_reading_state(epub, &reading_state)?;
            let book_color_theme = self.db_state.get_book_theme(epub)?;
            let (jump_history, jump_history_index) = self.db_state.get_jump_history(epub)?;
            let marks: HashMap<char, ReadingState> =
                self.db_state.get_marks(epub)?.into_iter().collect();
            // Preserve any existing reading progress rather than resetting it to
            // 0% on open; only a brand-new book starts at 0.0.
            self.db_state
                .update_library(epub, reading_state.rel_pctg.or(Some(0.0)))?;

            // Now update the UI state
            let session_book_id = identity.book_id.clone();
            let mut state = self.state.borrow_mut();
            state.reading_state = reading_state;
            state.book_color_theme = book_color_theme;
            state.jump_history = jump_history;
            state.jump_history_index = jump_history_index.min(state.jump_history.len());
            state.marks = marks;
            state.ui_state.metadata = Some(epub.get_meta().clone());
            state.ui_state.book_identity = Some(identity);
            state.ui_state.toc_entries = epub.toc_entries().clone();
            state.ui_state.toc_selected_index = 0;
            if let Ok(bookmarks) = self.db_state.get_bookmarks(epub) {
                state.ui_state.bookmarks = bookmarks;
                state.ui_state.bookmarks_selected_index = 0;
            }
            let session_row = state.reading_state.row;
            drop(state);
            self.start_reading_session(session_book_id, session_row);
            self.refresh_statistics_snapshot()?;
            self.refresh_highlights()?;
            if alias_conflict {
                self.state.borrow_mut().ui_state.set_message(
                    "This path previously pointed to a different EPUB identity; highlights were kept separate."
                        .to_string(),
                    MessageType::Warning,
                );
            }
        }

        Ok(())
    }

    fn start_reading_session(&mut self, book_id: String, row: usize) {
        let now = Utc::now();
        self.reading_session = Some(ActiveReadingSession {
            book_id,
            started_at: now,
            last_activity: Instant::now(),
            last_activity_at: now,
            max_counted_row: row,
            rows: 0,
            words: 0,
        });
    }

    fn finish_reading_session(&mut self, ended_at: DateTime<Utc>) -> eyre::Result<()> {
        let Some(session) = self.reading_session.take() else {
            return Ok(());
        };
        self.db_state.insert_reading_session(
            &session.book_id,
            session.started_at,
            ended_at.max(session.started_at),
            session.rows,
            session.words,
        )?;
        // The inserted row changes the DB totals; force a re-query.
        self.cached_statistics = None;
        self.refresh_statistics_snapshot()?;
        Ok(())
    }

    fn close_idle_reading_session(&mut self) -> eyre::Result<()> {
        let Some(session) = self.reading_session.as_ref() else {
            return Ok(());
        };
        if session.last_activity.elapsed() < READING_IDLE_TIMEOUT {
            return Ok(());
        }
        // End at the last recorded activity; the idle gap itself was not
        // reading time.
        let ended_at = session.last_activity_at;
        self.finish_reading_session(ended_at)
    }

    fn record_reading_activity(&mut self, previous_row: usize) -> eyre::Result<()> {
        if self.ebook.is_none() || self.state.borrow().should_quit {
            return Ok(());
        }

        let (book_id, current_row) = {
            let state = self.state.borrow();
            let Some(identity) = state.ui_state.book_identity.as_ref() else {
                return Ok(());
            };
            (identity.book_id.clone(), state.reading_state.row)
        };

        if self
            .reading_session
            .as_ref()
            .is_none_or(|session| session.book_id != book_id)
        {
            self.start_reading_session(book_id, previous_row);
        }

        // Only count forward movement past the session's high-water mark, so
        // re-reading the same span is not double-counted.
        let jump_threshold = self.page_size().max(READING_JUMP_MIN_THRESHOLD_ROWS);
        let counted = self.reading_session.as_ref().and_then(|session| {
            if current_row <= previous_row || current_row <= session.max_counted_row {
                return None;
            }
            // Movement larger than one screen is a jump (G, ToC, search,
            // link, mark): the skipped span was not read, so advance the
            // mark without counting it.
            if current_row - previous_row > jump_threshold {
                return Some((0, 0));
            }
            let start = previous_row.max(session.max_counted_row);
            Some((
                current_row - start,
                self.count_words_in_range(start, current_row),
            ))
        });

        if let Some(session) = self.reading_session.as_mut() {
            if let Some((rows, words)) = counted {
                session.rows += rows;
                session.words += words;
                session.max_counted_row = current_row;
            }
            session.last_activity = Instant::now();
            session.last_activity_at = Utc::now();
        }
        self.refresh_statistics_snapshot()?;
        Ok(())
    }

    fn refresh_statistics_snapshot(&mut self) -> eyre::Result<()> {
        let book_id = self
            .state
            .borrow()
            .ui_state
            .book_identity
            .as_ref()
            .map(|identity| identity.book_id.clone());
        let session_day = self
            .reading_session
            .as_ref()
            .map(|session| session.started_at.with_timezone(&Local).date_naive());

        // Only hit the database when the cache does not apply (book changed,
        // session day rolled over, or the cache was explicitly invalidated);
        // the per-keypress path is pure in-memory overlay work.
        let cache_valid = self.cached_statistics.as_ref().is_some_and(|cache| {
            cache.book_id == book_id && session_day.is_none_or(|day| day == cache.streak_day)
        });
        if !cache_valid {
            let stats = self.db_state.get_reading_statistics(book_id.as_deref())?;
            let streak_day = session_day.unwrap_or_else(|| Local::now().date_naive());
            let streaks_with_day = self.db_state.reading_streaks_with_day(Some(streak_day))?;
            self.cached_statistics = Some(CachedStatistics {
                book_id: book_id.clone(),
                stats,
                streak_day,
                streaks_with_day,
            });
        }
        let cache = self
            .cached_statistics
            .as_ref()
            .expect("statistics cache populated above");

        let mut stats = cache.stats.clone();
        if let Some(session) = self.reading_session.as_ref() {
            let active_seconds = (Utc::now() - session.started_at).num_seconds().max(0);
            stats.global.seconds += active_seconds;
            stats.global.rows += session.rows as i64;
            stats.global.words += session.words as i64;
            stats.global.sessions += 1;
            (stats.current_streak_days, stats.longest_streak_days) = cache.streaks_with_day;
            if book_id.as_deref() == Some(session.book_id.as_str()) {
                stats.book.seconds += active_seconds;
                stats.book.rows += session.rows as i64;
                stats.book.words += session.words as i64;
                stats.book.sessions += 1;
            }
        }

        let wpm = stats
            .book
            .words_per_minute()
            .or_else(|| stats.global.words_per_minute())
            .filter(|wpm| *wpm >= 50.0)
            .unwrap_or(DEFAULT_READING_WPM);
        stats.estimated_chapter_minutes_left = self.estimated_minutes_left_for_range(
            self.current_row(),
            self.current_chapter_end(),
            wpm,
        );
        stats.estimated_book_minutes_left = self.estimated_minutes_left_for_range(
            self.current_row(),
            self.board.total_lines(),
            wpm,
        );

        self.state.borrow_mut().ui_state.statistics = stats;
        Ok(())
    }

    fn current_row(&self) -> usize {
        self.state.borrow().reading_state.row
    }

    fn current_chapter_end(&self) -> usize {
        let current_row = self.current_row();
        if let Some(index) = self.content_index_for_row(current_row)
            && let Some((_start, end)) = self.chapter_bounds_for_index(index)
        {
            return end.saturating_add(1);
        }
        self.board.total_lines()
    }

    fn estimated_minutes_left_for_range(
        &self,
        start_row: usize,
        end_row: usize,
        wpm: f64,
    ) -> Option<i64> {
        if end_row <= start_row || wpm <= 0.0 {
            return None;
        }
        let words = self.count_words_in_range(start_row, end_row);
        if words == 0 {
            return None;
        }
        Some((words as f64 / wpm).ceil() as i64)
    }

    fn count_words_in_range(&self, start_row: usize, end_row: usize) -> usize {
        self.board.words_in_range(start_row, end_row)
    }

    fn persist_state(&mut self) -> eyre::Result<()> {
        if let Some(epub) = self.ebook.as_ref() {
            let reading_state = {
                let state = self.state.borrow();
                state.reading_state.clone()
            };
            let total_lines = self.board.total_lines();
            let rel_pctg = if total_lines > 0 {
                Some(reading_state.row as f32 / total_lines as f32)
            } else {
                None
            };
            let mut to_save = reading_state.clone();
            to_save.rel_pctg = rel_pctg;
            self.db_state.set_last_reading_state(epub, &to_save)?;
            self.db_state.update_library(epub, rel_pctg)?;
            let (jump_history, jump_history_index) = {
                let state = self.state.borrow();
                (state.jump_history.clone(), state.jump_history_index)
            };
            self.db_state
                .set_jump_history(epub, &jump_history, jump_history_index)?;
        }
        Ok(())
    }
}

impl Reader {
    /// Run the main application loop
    pub fn run(&mut self) -> eyre::Result<()> {
        // Initialize terminal
        crossterm::terminal::enable_raw_mode()?;
        crossterm::execute!(
            io::stdout(),
            crossterm::terminal::EnterAlternateScreen,
            crossterm::event::EnableBracketedPaste
        )?;
        // Capture the mouse only when the setting is on, so that native
        // terminal selection/copy keeps working otherwise.
        if self.state.borrow().config.settings.mouse_support {
            crossterm::execute!(io::stdout(), crossterm::event::EnableMouseCapture)?;
        }

        self.terminal.clear()?;
        self.terminal.hide_cursor()?;

        // Main event loop
        loop {
            let state = self.state.borrow();
            if state.should_quit {
                break;
            }
            drop(state);

            // Auto-clear expired messages before rendering
            {
                let mut state = self.state.borrow_mut();
                if state.ui_state.message_expired() {
                    state.ui_state.clear_message();
                }
            }
            self.close_idle_reading_session()?;

            // Check for dictionary results
            if let Some(rx) = &self.dictionary_res_rx {
                if let Ok(res) = rx.try_recv() {
                    let mut state = self.state.borrow_mut();
                    state.ui_state.dictionary_word = res.word;
                    state.ui_state.dictionary_client_used = res.client;
                    state.ui_state.dictionary_definition = match res.definition {
                        Ok(def) => def,
                        Err(err) => err,
                    };
                    state.ui_state.dictionary_loading = false;
                    self.dictionary_res_rx = None;
                }
            }

            self.tts_poll_worker()?;

            // Check for TTS paragraph completion → advance to next paragraph
            if self.state.borrow().ui_state.tts_active {
                if let Some(rx) = &self.tts_done_rx {
                    if let Ok(()) = rx.try_recv() {
                        self.tts_child = None;
                        self.tts_done_rx = None;
                        let previous_row = self.state.borrow().reading_state.row;
                        self.tts_advance_paragraph()?;
                        self.record_reading_activity(previous_row)?;
                    }
                }
            }

            // Render UI
            self.draw()?;

            // Poll with timeout so we can re-render when messages expire or for animation
            let poll_timeout = {
                let state = self.state.borrow();
                if state.ui_state.tts_converting {
                    Duration::from_millis(80)
                } else if state.ui_state.tts_active {
                    Duration::from_millis(200)
                } else if state.ui_state.dictionary_loading && state.ui_state.show_dictionary {
                    Duration::from_millis(100)
                } else {
                    match state.ui_state.message_time {
                        Some(t) => {
                            let elapsed = t.elapsed();
                            let expiry = Duration::from_secs(3);
                            if elapsed < expiry {
                                expiry - elapsed
                            } else {
                                Duration::from_millis(100)
                            }
                        }
                        None => Duration::from_secs(60),
                    }
                }
            };

            if !crossterm::event::poll(poll_timeout)? {
                continue;
            }

            // Handle events
            if let Ok(event) = crossterm::event::read() {
                match event {
                    Event::Key(key) => {
                        if key.kind == KeyEventKind::Press {
                            self.close_idle_reading_session()?;
                            let previous_row = self.state.borrow().reading_state.row;
                            self.handle_key_event(key)?;
                            self.record_reading_activity(previous_row)?;
                        }
                    }
                    Event::Paste(text) => {
                        if self.state.borrow().ui_state.active_window
                            == WindowType::HighlightCommentEditor
                        {
                            self.highlight_comment_insert(&text);
                        }
                    }
                    Event::Mouse(mouse) => {
                        if self.state.borrow().config.settings.mouse_support {
                            self.close_idle_reading_session()?;
                            let previous_row = self.state.borrow().reading_state.row;
                            self.handle_mouse_event(mouse)?;
                            self.record_reading_activity(previous_row)?;
                        }
                    }
                    Event::Resize(_, _) => {
                        // Rebuild text structure on resize with current textwidth
                        let term_width = match crossterm::terminal::size() {
                            Ok((w, _)) => w as usize,
                            Err(_) => 100,
                        };
                        let textwidth = {
                            let state = self.state.borrow();
                            if state.config.settings.seamless_between_chapters {
                                None
                            } else {
                                Some(state.reading_state.textwidth)
                            }
                        };
                        if let Some(textwidth) = textwidth {
                            let padding = if term_width <= 20 {
                                0
                            } else {
                                (term_width.saturating_sub(textwidth) / 2).max(5)
                            };
                            self.rebuild_text_structure(padding)?;
                        }
                    }
                    _ => {}
                }
            }
        }

        self.finish_reading_session(Utc::now())?;

        // Stop TTS if it's still running
        self.stop_tts();

        // Persist current reading state to the database before cleaning up
        self.persist_state()?;

        // Cleanup terminal
        self.terminal.clear()?;
        self.terminal.show_cursor()?;
        crossterm::execute!(
            io::stdout(),
            crossterm::terminal::LeaveAlternateScreen,
            crossterm::event::DisableMouseCapture,
            crossterm::event::DisableBracketedPaste
        )?;
        crossterm::terminal::disable_raw_mode()?;

        Ok(())
    }
}

impl<B: Backend> Reader<B>
where
    B::Error: std::error::Error + Send + Sync + 'static,
{
    /// Handle keyboard input events
    fn handle_key_event(&mut self, key: KeyEvent) -> eyre::Result<()> {
        {
            let mut state = self.state.borrow_mut();
            if state.ui_state.message.is_some()
                && state.ui_state.active_window == WindowType::Reader
            {
                state.ui_state.clear_message();
            }
        }

        if self.handle_pending_mark_key(key)? {
            let mut state = self.state.borrow_mut();
            state.count_prefix.clear();
            return Ok(());
        }

        // Handle count prefix (number repetition)
        // Only capture digits if we are in a mode that supports it (Reader or Visual)
        let active_window = self.state.borrow().ui_state.active_window.clone();
        if matches!(active_window, WindowType::Reader | WindowType::Visual)
            && let KeyCode::Char(c) = key.code
            && c.is_ascii_digit()
        {
            let mut state = self.state.borrow_mut();
            if state.count_prefix.len() < 6 {
                state.count_prefix.push(c);
            }
            return Ok(());
        }

        // Determine repetition count
        let repeat_count = {
            let state = self.state.borrow();
            if state.count_prefix.is_empty() {
                1
            } else {
                state.count_prefix.parse().unwrap_or(1)
            }
        };

        // Handle key bindings based on current mode
        let active_window = {
            let state = self.state.borrow();
            state.ui_state.active_window.clone()
        };

        match active_window {
            WindowType::Search => self.handle_search_mode_keys(key, repeat_count)?,
            WindowType::Visual => self.handle_visual_mode_keys(key, repeat_count)?,
            WindowType::Toc => self.handle_toc_mode_keys(key, repeat_count)?,
            WindowType::Bookmarks => self.handle_bookmarks_mode_keys(key, repeat_count)?,
            WindowType::Highlights => self.handle_highlights_mode_keys(key, repeat_count)?,
            WindowType::HighlightCommentEditor => self.handle_highlight_comment_editor_keys(key)?,
            WindowType::ConfirmDeleteHighlight => self.handle_confirm_delete_highlight_keys(key)?,
            WindowType::Library => self.handle_library_mode_keys(key, repeat_count)?,
            WindowType::Settings => self.handle_settings_mode_keys(key, repeat_count)?,
            WindowType::Links => self.handle_links_mode_keys(key, repeat_count)?,
            WindowType::LinkPreview => self.handle_link_preview_mode_keys(key)?,
            WindowType::Images => self.handle_images_mode_keys(key, repeat_count)?,
            WindowType::Help => self.handle_help_mode_keys(key, repeat_count)?,
            WindowType::Metadata => self.handle_modal_close_keys(key)?,
            WindowType::Statistics => self.handle_modal_close_keys(key)?,
            WindowType::Dictionary => self.handle_dictionary_mode_keys(key, repeat_count)?,
            WindowType::DictionaryCommandInput => self.handle_dictionary_command_input_keys(key)?,
            _ => self.handle_normal_mode_keys(key, repeat_count)?,
        }

        // Clear count prefix after handling
        {
            let mut state = self.state.borrow_mut();
            state.count_prefix.clear();
        }

        Ok(())
    }

    /// Handle keys in normal reading mode
    fn handle_normal_mode_keys(&mut self, key: KeyEvent, repeat_count: u32) -> eyre::Result<()> {
        match key.code {
            // Jump History
            KeyCode::Char('o') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.jump_back();
            }
            KeyCode::Tab => {
                self.jump_forward();
            }
            KeyCode::Char('i') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.jump_forward();
            }

            // Navigation
            KeyCode::Char('j') | KeyCode::Down => {
                for _ in 0..repeat_count {
                    self.move_cursor(AppDirection::Down);
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                for _ in 0..repeat_count {
                    self.move_cursor(AppDirection::Up);
                }
            }
            KeyCode::Char('h') | KeyCode::Left => {
                for _ in 0..repeat_count {
                    self.move_cursor(AppDirection::PageUp);
                }
            }
            KeyCode::Char('l') | KeyCode::Right => {
                for _ in 0..repeat_count {
                    self.move_cursor(AppDirection::PageDown);
                }
            }

            // Page navigation
            KeyCode::PageDown | KeyCode::Char(' ') => {
                for _ in 0..repeat_count {
                    self.move_cursor(AppDirection::PageDown);
                }
            }
            KeyCode::PageUp => {
                for _ in 0..repeat_count {
                    self.move_cursor(AppDirection::PageUp);
                }
            }
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                for _ in 0..repeat_count {
                    self.move_cursor(AppDirection::HalfPageUp);
                }
            }
            KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                for _ in 0..repeat_count {
                    self.move_cursor(AppDirection::HalfPageDown);
                }
            }

            // Chapter navigation
            KeyCode::Char('L') => {
                self.next_chapter();
            }
            KeyCode::Char('H') => {
                self.previous_chapter();
            }
            KeyCode::Char('n') => {
                if key.modifiers.contains(KeyModifiers::CONTROL) {
                    self.next_chapter();
                } else {
                    self.search_next();
                }
            }
            KeyCode::Char('p') => {
                if key.modifiers.contains(KeyModifiers::CONTROL) {
                    self.previous_chapter();
                } else {
                    self.search_previous();
                }
            }
            KeyCode::Char('N') => {
                self.search_previous();
            }

            // Beginning/End
            KeyCode::Home => {
                self.goto_start();
            }
            KeyCode::End => {
                self.goto_end();
            }
            KeyCode::Char('g') => {
                self.goto_chapter_start();
            }
            KeyCode::Char('G') => {
                self.goto_chapter_end();
            }

            KeyCode::Char(_)
                if key_matches_binding(
                    &key,
                    &self
                        .state
                        .borrow()
                        .config
                        .keymap_user_dict()
                        .show_highlights,
                ) =>
            {
                self.open_highlights_window()?;
            }

            // Search
            KeyCode::Char('/') => {
                let history = self.db_state.get_search_history().unwrap_or_default();
                let mut state = self.state.borrow_mut();
                state.search_data = Some(SearchData::default());
                state.ui_state.search_query.clear();
                state.ui_state.search_results.clear();
                state.ui_state.search_matches.clear();
                state.ui_state.search_committed = false;
                state.ui_state.search_origin_row = state.reading_state.row;
                state.ui_state.search_history = history;
                state.ui_state.search_history_index = None;
                state.ui_state.search_history_draft.clear();
                state.ui_state.open_window(WindowType::Search);
            }

            // Two-phase flow: first v enters cursor mode, second v starts selection
            KeyCode::Char('v') => {
                let mut state = self.state.borrow_mut();
                // Place cursor at the first non-empty line on the current page
                let viewport_start = state.reading_state.row.saturating_sub(1);
                let total_lines = self.board.total_lines();
                let page = Self::page_size_for(state.config.settings.show_top_bar);
                let viewport_end = (viewport_start + page).min(total_lines);
                let mut start_row = viewport_start.min(total_lines.saturating_sub(1));
                for row in viewport_start..viewport_end {
                    if self.board.line_char_count(row) > 0 {
                        start_row = row;
                        break;
                    }
                }
                state.ui_state.visual_anchor = None;
                state.ui_state.visual_cursor = Some((start_row, 0));
                state.ui_state.open_window(WindowType::Visual);
            }

            // Windows
            KeyCode::Char('q') => {
                let mut state = self.state.borrow_mut();
                if state.ui_state.active_window != WindowType::Reader {
                    state.ui_state.open_window(WindowType::Reader);
                } else {
                    state.should_quit = true;
                }
            }
            KeyCode::Char('?') => {
                let mut state = self.state.borrow_mut();
                state.ui_state.open_window(WindowType::Help);
            }
            KeyCode::Char('t') => {
                self.open_toc_window()?;
            }
            KeyCode::Char('m') => {
                let mut state = self.state.borrow_mut();
                state.ui_state.pending_mark_command = Some(PendingMarkCommand::Set);
                state.ui_state.set_message(
                    "Mark position: press a mark key".to_string(),
                    MessageType::Info,
                );
            }
            KeyCode::Char('`') => {
                let mut state = self.state.borrow_mut();
                state.ui_state.pending_mark_command = Some(PendingMarkCommand::Jump);
                state.ui_state.set_message(
                    "Jump to mark: press a mark key".to_string(),
                    MessageType::Info,
                );
            }
            KeyCode::Char('B') => {
                self.open_bookmarks_window()?;
            }
            KeyCode::Char('u') => {
                self.open_links_window()?;
            }
            KeyCode::Char('o') => {
                if !key.modifiers.contains(KeyModifiers::CONTROL) {
                    self.open_images_window()?;
                }
            }
            KeyCode::Char('i') => {
                if key.modifiers.contains(KeyModifiers::CONTROL) {
                    self.jump_forward();
                } else {
                    self.open_metadata_window()?;
                }
            }
            KeyCode::Char('r') => {
                self.open_library_window()?;
            }
            KeyCode::Char('R') => {
                self.open_statistics_window()?;
            }
            KeyCode::Char('s') => {
                let mut state = self.state.borrow_mut();
                state.ui_state.settings_selected_index = 0;
                state.ui_state.open_window(WindowType::Settings);
            }
            KeyCode::Char('T') => {
                let mut state = self.state.borrow_mut();
                state.config.settings.show_top_bar = !state.config.settings.show_top_bar;
            }
            KeyCode::Char('+') => {
                self.change_textwidth(5)?;
            }
            KeyCode::Char('=') => {
                if key.modifiers.contains(KeyModifiers::SHIFT) {
                    self.change_textwidth(5)?;
                } else {
                    self.reset_width()?;
                }
            }
            KeyCode::Char('-') => {
                self.change_textwidth(-5)?;
            }

            // TTS toggle
            KeyCode::Char('!') => {
                self.toggle_tts()?;
            }

            // Color theme cycle
            KeyCode::Char('c') => {
                self.cycle_color_theme()?;
            }

            _ => {}
        }

        Ok(())
    }

    fn record_jump_position(&mut self) {
        let mut state = self.state.borrow_mut();
        state.record_jump();
    }

    fn jump_back(&mut self) {
        {
            let mut state = self.state.borrow_mut();
            state.jump_back();
        }
        self.sync_reading_content_index();
    }

    fn jump_forward(&mut self) {
        {
            let mut state = self.state.borrow_mut();
            state.jump_forward();
        }
        self.sync_reading_content_index();
    }

    fn sync_reading_content_index(&mut self) {
        let row = self.state.borrow().reading_state.row;
        if let Some(content_index) = self.content_index_for_row(row) {
            self.state.borrow_mut().reading_state.content_index = content_index;
        }
    }

    fn handle_pending_mark_key(&mut self, key: KeyEvent) -> eyre::Result<bool> {
        let pending = {
            let state = self.state.borrow();
            if state.ui_state.active_window != WindowType::Reader {
                return Ok(false);
            }
            state.ui_state.pending_mark_command
        };
        let Some(command) = pending else {
            return Ok(false);
        };

        if matches!(key.code, KeyCode::Esc) {
            let mut state = self.state.borrow_mut();
            state.ui_state.pending_mark_command = None;
            state
                .ui_state
                .set_message("Mark command cancelled".to_string(), MessageType::Info);
            return Ok(true);
        }

        let KeyCode::Char(name) = key.code else {
            return Ok(true);
        };
        self.state.borrow_mut().ui_state.pending_mark_command = None;
        if !name.is_ascii_alphanumeric() {
            self.state.borrow_mut().ui_state.set_message(
                "Invalid mark name (use a-z, A-Z, 0-9)".to_string(),
                MessageType::Warning,
            );
            return Ok(true);
        }

        match command {
            PendingMarkCommand::Set => {
                let Some(epub) = self.ebook.as_ref() else {
                    return Ok(true);
                };
                let reading_state = { self.state.borrow().reading_state.clone() };
                self.db_state.upsert_mark(epub, name, &reading_state)?;
                let mut state = self.state.borrow_mut();
                state.marks.insert(name, reading_state);
                state
                    .ui_state
                    .set_message(format!("Set mark '{name}'"), MessageType::Info);
            }
            PendingMarkCommand::Jump => {
                let target = { self.state.borrow().marks.get(&name).cloned() };
                if let Some(target) = target {
                    self.record_jump_position();
                    let mut state = self.state.borrow_mut();
                    state.reading_state.row = target.row;
                    state.reading_state.content_index = target.content_index;
                    state.ui_state.open_window(WindowType::Reader);
                } else {
                    self.state
                        .borrow_mut()
                        .ui_state
                        .set_message(format!("Mark '{name}' is not set"), MessageType::Warning);
                }
            }
        }
        Ok(true)
    }

    fn cycle_color_theme(&mut self) -> eyre::Result<()> {
        let next = {
            let state = self.state.borrow();
            state.effective_color_theme().next()
        };
        self.set_effective_color_theme(Some(next))?;
        self.state
            .borrow_mut()
            .ui_state
            .set_message(format!("Theme: {}", next.name()), MessageType::Info);
        Ok(())
    }

    fn set_effective_color_theme(&mut self, theme: Option<ColorTheme>) -> eyre::Result<()> {
        if let Some(epub) = self.ebook.as_ref() {
            self.db_state.set_book_theme(epub, theme)?;
            self.state.borrow_mut().book_color_theme = theme;
        } else {
            let mut state = self.state.borrow_mut();
            state.config.settings.color_theme = theme.unwrap_or(ColorTheme::Default);
            let _ = state.config.save();
        }
        Ok(())
    }

    fn set_clipboard_text(&mut self, text: String) -> eyre::Result<bool> {
        let Some(clipboard) = self.clipboard.as_mut() else {
            return Ok(false);
        };
        clipboard.set_text(text)?;
        Ok(true)
    }

    /// Handle keys in search mode.
    ///
    /// While the query is being typed (`search_committed == false`), matches
    /// update incrementally, Up/Down browse the persisted search history, and
    /// j/k are entered as text. After Enter commits the query, Up/Down and
    /// j/k navigate results and a second Enter jumps and closes the window.
    fn handle_search_mode_keys(&mut self, key: KeyEvent, _repeat_count: u32) -> eyre::Result<()> {
        let committed = self.state.borrow().ui_state.search_committed;
        match key.code {
            KeyCode::Enter => {
                if committed {
                    self.jump_to_selected_search_result();
                } else {
                    self.commit_search();
                }
            }
            KeyCode::Esc => {
                // Cancel search; while still typing, restore the original view.
                let mut state = self.state.borrow_mut();
                state.search_data = None;
                if !state.ui_state.search_committed {
                    state.reading_state.row = state.ui_state.search_origin_row;
                    state.ui_state.search_results.clear();
                    state.ui_state.search_matches.clear();
                }
                state.ui_state.open_window(WindowType::Reader);
            }
            KeyCode::Backspace => {
                {
                    let mut state = self.state.borrow_mut();
                    state.ui_state.search_query.pop();
                    state.ui_state.search_committed = false;
                    state.ui_state.search_history_index = None;
                }
                self.update_incremental_search();
            }
            KeyCode::Up if !committed => {
                self.search_history_older();
            }
            KeyCode::Down if !committed => {
                self.search_history_newer();
            }
            KeyCode::Down => {
                let mut state = self.state.borrow_mut();
                if !state.ui_state.search_results.is_empty() {
                    let next = (state.ui_state.selected_search_result + 1)
                        .min(state.ui_state.search_results.len() - 1);
                    state.ui_state.selected_search_result = next;
                    let line = state.ui_state.search_results.get(next).map(|r| r.line);
                    if let Some(line) = line {
                        state.reading_state.row = line;
                    }
                }
            }
            KeyCode::Up => {
                let mut state = self.state.borrow_mut();
                if !state.ui_state.search_results.is_empty() {
                    let current = state.ui_state.selected_search_result;
                    state.ui_state.selected_search_result = current.saturating_sub(1);
                    let idx = state.ui_state.selected_search_result;
                    let line = state.ui_state.search_results.get(idx).map(|r| r.line);
                    if let Some(line) = line {
                        state.reading_state.row = line;
                    }
                }
            }
            // `j`/`k` navigate results only once the query is committed; while
            // typing they are entered as text.
            KeyCode::Char('j') if committed => {
                let mut state = self.state.borrow_mut();
                if !state.ui_state.search_results.is_empty() {
                    let next = (state.ui_state.selected_search_result + 1)
                        .min(state.ui_state.search_results.len() - 1);
                    state.ui_state.selected_search_result = next;
                    let line = state.ui_state.search_results.get(next).map(|r| r.line);
                    if let Some(line) = line {
                        state.reading_state.row = line;
                    }
                }
            }
            KeyCode::Char('k') if committed => {
                let mut state = self.state.borrow_mut();
                if !state.ui_state.search_results.is_empty() {
                    let current = state.ui_state.selected_search_result;
                    state.ui_state.selected_search_result = current.saturating_sub(1);
                    let idx = state.ui_state.selected_search_result;
                    let line = state.ui_state.search_results.get(idx).map(|r| r.line);
                    if let Some(line) = line {
                        state.reading_state.row = line;
                    }
                }
            }
            KeyCode::Char(c) => {
                {
                    let mut state = self.state.borrow_mut();
                    state.ui_state.search_query.push(c);
                    state.ui_state.search_committed = false;
                    state.ui_state.search_history_index = None;
                }
                self.update_incremental_search();
            }
            _ => {}
        }

        Ok(())
    }

    /// Up in the search prompt: recall the next-older history entry.
    fn search_history_older(&mut self) {
        {
            let mut state = self.state.borrow_mut();
            if state.ui_state.search_history.is_empty() {
                return;
            }
            let next_index = match state.ui_state.search_history_index {
                None => {
                    state.ui_state.search_history_draft = state.ui_state.search_query.clone();
                    0
                }
                Some(index) if index + 1 < state.ui_state.search_history.len() => index + 1,
                Some(index) => index,
            };
            state.ui_state.search_history_index = Some(next_index);
            state.ui_state.search_query = state.ui_state.search_history[next_index].clone();
        }
        self.update_incremental_search();
    }

    /// Down in the search prompt: recall the next-newer entry, or restore the
    /// query that was being typed before history browsing started.
    fn search_history_newer(&mut self) {
        {
            let mut state = self.state.borrow_mut();
            match state.ui_state.search_history_index {
                None => return,
                Some(0) => {
                    state.ui_state.search_history_index = None;
                    state.ui_state.search_query = state.ui_state.search_history_draft.clone();
                }
                Some(index) => {
                    state.ui_state.search_history_index = Some(index - 1);
                    state.ui_state.search_query = state.ui_state.search_history[index - 1].clone();
                }
            }
        }
        self.update_incremental_search();
    }

    /// Returns the `Highlight` whose resolved range contains `visual_cursor`, if any.
    fn highlight_at_cursor(&self) -> Option<Highlight> {
        let state = self.state.borrow();
        let (row, col) = state.ui_state.visual_cursor?;
        let ranges = state.ui_state.highlight_ranges.get(&row)?;
        let range = ranges
            .iter()
            .find(|r| col >= r.start_col && col < r.end_col)?;
        state
            .ui_state
            .highlights
            .get(range.highlight_index)
            .cloned()
    }

    fn handle_confirm_delete_highlight_keys(&mut self, key: KeyEvent) -> eyre::Result<()> {
        match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') => {
                let pending = self
                    .state
                    .borrow_mut()
                    .ui_state
                    .pending_delete_highlight
                    .take();
                if let Some(highlight) = pending {
                    self.db_state.delete_highlight(&highlight.id)?;
                    self.refresh_highlights()?;
                    let mut state = self.state.borrow_mut();
                    state
                        .ui_state
                        .set_message("Highlight deleted".to_string(), MessageType::Info);
                    state.ui_state.open_window(WindowType::Visual);
                } else {
                    self.state
                        .borrow_mut()
                        .ui_state
                        .open_window(WindowType::Visual);
                }
            }
            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                let mut state = self.state.borrow_mut();
                state.ui_state.pending_delete_highlight = None;
                state.ui_state.open_window(WindowType::Visual);
            }
            _ => {}
        }
        Ok(())
    }

    /// Handle keys in two phases: cursor mode -> selection mode
    ///
    /// Phase 1 (cursor mode): visual_cursor is Some, visual_anchor is None.
    ///   - hjkl/wbe move the cursor. Press v to anchor and start selecting.
    ///   - Press d on a highlight to delete it.
    /// Phase 2 (selection mode): both visual_cursor and visual_anchor are Some.
    ///   - hjkl/wbe extend the selection. Press y to yank, d for dictionary,
    ///     p for Wikipedia, s to search the web.
    fn handle_visual_mode_keys(&mut self, key: KeyEvent, repeat_count: u32) -> eyre::Result<()> {
        let has_anchor = self.state.borrow().ui_state.visual_anchor.is_some();
        let search_input_active = self.state.borrow().ui_state.visual_search_input_active;

        // When the inline `/`-prompt is open, all keys go to the query editor.
        if search_input_active {
            match key.code {
                KeyCode::Esc => {
                    let mut state = self.state.borrow_mut();
                    state.ui_state.visual_search_input_active = false;
                    state.ui_state.visual_search_query.clear();
                }
                KeyCode::Enter => {
                    self.state.borrow_mut().ui_state.visual_search_input_active = false;
                    self.execute_visual_search();
                }
                KeyCode::Backspace => {
                    self.state.borrow_mut().ui_state.visual_search_query.pop();
                }
                KeyCode::Char(c) => {
                    self.state.borrow_mut().ui_state.visual_search_query.push(c);
                }
                _ => {}
            }
            return Ok(());
        }

        // Pending `f`/`F`/`t`/`T`: the next keypress is the find target.
        // The count typed before the motion key was stashed alongside the
        // direction, since `count_prefix` is cleared between key events.
        let pending_find = self.state.borrow().ui_state.pending_visual_find;
        if let Some((dir, pending_count)) = pending_find {
            self.state.borrow_mut().ui_state.pending_visual_find = None;
            if let KeyCode::Char(c) = key.code {
                self.move_visual_cursor_find_char(c, dir, pending_count);
            }
            return Ok(());
        }

        match key.code {
            KeyCode::Esc => {
                let mut state = self.state.borrow_mut();
                state.ui_state.visual_search_matches.clear();
                state.ui_state.visual_search_selected = 0;
                state.ui_state.pending_visual_find = None;
                if has_anchor {
                    // In selection mode: go back to cursor mode
                    state.ui_state.visual_anchor = None;
                } else {
                    // In cursor mode: exit to reader
                    state.ui_state.open_window(WindowType::Reader);
                }
            }
            KeyCode::Char('/') => {
                let mut state = self.state.borrow_mut();
                state.ui_state.visual_search_input_active = true;
                state.ui_state.visual_search_query.clear();
            }
            KeyCode::Char('n') => {
                for _ in 0..repeat_count {
                    self.visual_search_step(true);
                }
            }
            KeyCode::Char('N') => {
                for _ in 0..repeat_count {
                    self.visual_search_step(false);
                }
            }
            KeyCode::Char('v') => {
                let mut state = self.state.borrow_mut();
                if has_anchor {
                    // Already in selection mode: exit to reader
                    state.ui_state.open_window(WindowType::Reader);
                } else {
                    // In cursor mode: anchor here and start selection
                    state.ui_state.visual_anchor = state.ui_state.visual_cursor;
                }
            }
            KeyCode::Enter if !has_anchor => {
                if let Some(highlight) = self.highlight_at_cursor() {
                    self.clear_visual_search_state();
                    let mut state = self.state.borrow_mut();
                    state.ui_state.highlight_comment_buffer =
                        highlight.comment.clone().unwrap_or_default();
                    state.ui_state.highlight_comment_cursor =
                        state.ui_state.highlight_comment_buffer.len();
                    state.ui_state.highlight_comment_editing_id = Some(highlight.id);
                    state
                        .ui_state
                        .open_window(WindowType::HighlightCommentEditor);
                }
            }
            KeyCode::Char('d') if !has_anchor => {
                if let Some(highlight) = self.highlight_at_cursor() {
                    self.clear_visual_search_state();
                    let has_comment = highlight
                        .comment
                        .as_deref()
                        .is_some_and(|c| !c.trim().is_empty());
                    if has_comment {
                        let mut state = self.state.borrow_mut();
                        state.ui_state.pending_delete_highlight = Some(highlight);
                        state
                            .ui_state
                            .open_window(WindowType::ConfirmDeleteHighlight);
                    } else {
                        self.db_state.delete_highlight(&highlight.id)?;
                        self.refresh_highlights()?;
                        self.state
                            .borrow_mut()
                            .ui_state
                            .set_message("Highlight deleted".to_string(), MessageType::Info);
                    }
                }
            }
            KeyCode::Char('C') if !has_anchor => {
                if let Some(highlight) = self.highlight_at_cursor() {
                    let next_color = HighlightColor::from_name(&highlight.color).next();
                    self.db_state
                        .update_highlight_color(&highlight.id, next_color.name())?;
                    self.refresh_highlights()?;
                    let mut state = self.state.borrow_mut();
                    state.ui_state.next_highlight_color = next_color;
                    state.ui_state.set_message(
                        format!("Highlight color: {}", next_color.name()),
                        MessageType::Info,
                    );
                }
            }
            KeyCode::Char('y') if has_anchor => {
                self.yank_selection()?;
                self.clear_visual_search_state();
            }
            KeyCode::Char(_)
                if has_anchor
                    && key_matches_binding(
                        &key,
                        &self.state.borrow().config.keymap_user_dict().add_highlight,
                    ) =>
            {
                self.create_highlight_from_selection(false)?;
                self.clear_visual_search_state();
            }
            KeyCode::Char(_)
                if has_anchor
                    && key_matches_binding(
                        &key,
                        &self
                            .state
                            .borrow()
                            .config
                            .keymap_user_dict()
                            .add_highlight_comment,
                    ) =>
            {
                self.create_highlight_from_selection(true)?;
                self.clear_visual_search_state();
            }
            KeyCode::Char('d') if has_anchor => {
                self.dictionary_lookup()?;
                self.clear_visual_search_state();
            }
            KeyCode::Char('p') if has_anchor => {
                self.wikipedia_lookup()?;
                self.clear_visual_search_state();
            }
            KeyCode::Char('s') if has_anchor => {
                self.web_search_selection()?;
                self.clear_visual_search_state();
            }
            // Navigation — works in both cursor and selection mode
            KeyCode::Char('j') | KeyCode::Down => {
                for _ in 0..repeat_count {
                    self.move_visual_cursor(AppDirection::Down);
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                for _ in 0..repeat_count {
                    self.move_visual_cursor(AppDirection::Up);
                }
            }
            KeyCode::Char('h') | KeyCode::Left => {
                for _ in 0..repeat_count {
                    self.move_visual_cursor(AppDirection::Left);
                }
            }
            KeyCode::Char('l') | KeyCode::Right => {
                for _ in 0..repeat_count {
                    self.move_visual_cursor(AppDirection::Right);
                }
            }
            KeyCode::Char('w') => {
                for _ in 0..repeat_count {
                    self.move_visual_cursor_word_forward();
                }
            }
            KeyCode::Char('b') => {
                for _ in 0..repeat_count {
                    self.move_visual_cursor_word_backward();
                }
            }
            KeyCode::Char('e') => {
                for _ in 0..repeat_count {
                    self.move_visual_cursor_word_end();
                }
            }
            KeyCode::Char('$') => {
                self.move_visual_cursor_line_end();
            }
            KeyCode::Char('^') => {
                self.move_visual_cursor_line_first_non_blank();
            }
            KeyCode::Char(']') => {
                for _ in 0..repeat_count {
                    self.move_visual_cursor_paragraph_forward();
                }
            }
            KeyCode::Char('[') => {
                for _ in 0..repeat_count {
                    self.move_visual_cursor_paragraph_backward();
                }
            }
            KeyCode::Char('f') => {
                self.state.borrow_mut().ui_state.pending_visual_find =
                    Some((VisualFindDirection::Forward, repeat_count));
            }
            KeyCode::Char('F') => {
                self.state.borrow_mut().ui_state.pending_visual_find =
                    Some((VisualFindDirection::Backward, repeat_count));
            }
            KeyCode::Char('t') => {
                self.state.borrow_mut().ui_state.pending_visual_find =
                    Some((VisualFindDirection::TillForward, repeat_count));
            }
            KeyCode::Char('T') => {
                self.state.borrow_mut().ui_state.pending_visual_find =
                    Some((VisualFindDirection::TillBackward, repeat_count));
            }
            _ => {}
        }
        Ok(())
    }

    /// Handle common list navigation keys (Esc/q to close, j/k to move selection).
    /// Returns `true` if the key was consumed, `false` if it should be handled by the caller.
    /// Handle mouse input. Wheel ticks are translated into Up/Down key
    /// presses so every window reacts the same way it does to the keyboard
    /// (the reader scrolls, list windows move their selection, scrollable
    /// popups scroll). A left click in the reader follows the link on the
    /// clicked line, if any.
    fn handle_mouse_event(&mut self, mouse: MouseEvent) -> eyre::Result<()> {
        let active_window = self.state.borrow().ui_state.active_window.clone();
        let synthesize = |code: KeyCode| KeyEvent::new(code, KeyModifiers::NONE);
        match mouse.kind {
            MouseEventKind::ScrollDown | MouseEventKind::ScrollUp => {
                let code = if mouse.kind == MouseEventKind::ScrollDown {
                    KeyCode::Down
                } else {
                    KeyCode::Up
                };
                // Scroll several lines per tick in the reading view; one
                // selection step per tick inside windows.
                let steps = if active_window == WindowType::Reader {
                    3
                } else {
                    1
                };
                for _ in 0..steps {
                    self.handle_key_event(synthesize(code))?;
                }
            }
            MouseEventKind::Down(MouseButton::Left) if active_window == WindowType::Reader => {
                self.handle_reader_click(mouse.row)?;
            }
            _ => {}
        }
        Ok(())
    }

    /// Follow the link on the clicked reader line. With several links on
    /// the same line the links window opens instead, since wrapped text
    /// carries no per-column link information.
    fn handle_reader_click(&mut self, screen_row: u16) -> eyre::Result<()> {
        // Mirror render_reader_static's vertical layout: a 1-row top bar
        // plus a 2-row gap when the top bar is shown.
        let content_top: u16 = if self.state.borrow().config.settings.show_top_bar {
            3
        } else {
            0
        };
        if screen_row < content_top {
            return Ok(());
        }
        let line = self.state.borrow().reading_state.row + (screen_row - content_top) as usize;
        let (visible_start, visible_end) = self.visible_line_range();
        if line < visible_start || line >= visible_end {
            return Ok(());
        }
        let mut links = self.board.links_in_range(line, line + 1);
        match links.len() {
            0 => Ok(()),
            1 => self.follow_link_entry(links.remove(0)),
            _ => self.open_links_window(),
        }
    }

    fn handle_list_nav(
        &self,
        key: &KeyEvent,
        repeat_count: u32,
        list_len: usize,
        index: &mut usize,
    ) -> bool {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => {
                self.state
                    .borrow_mut()
                    .ui_state
                    .open_window(WindowType::Reader);
                true
            }
            KeyCode::Char('j') | KeyCode::Down => {
                if list_len > 0 {
                    *index = index
                        .saturating_add(repeat_count as usize)
                        .min(list_len - 1);
                }
                true
            }
            KeyCode::Char('k') | KeyCode::Up => {
                *index = index.saturating_sub(repeat_count as usize);
                true
            }
            _ => false,
        }
    }

    /// Handle `/`-fuzzy-filter keys for list windows. While the filter prompt
    /// is being typed every key except Enter is consumed; Enter commits the
    /// filter and falls through so the caller can act on the selected item.
    /// Returns true if the key was consumed.
    fn handle_list_filter_keys(
        &mut self,
        key: &KeyEvent,
        items: &[String],
        index: &mut usize,
    ) -> bool {
        let active = self.state.borrow().ui_state.list_filter_active;
        if active {
            let mut state = self.state.borrow_mut();
            let ui = &mut state.ui_state;
            match key.code {
                KeyCode::Esc => {
                    ui.clear_list_filter();
                    *index = 0;
                }
                KeyCode::Enter => {
                    ui.list_filter_active = false;
                    if ui.list_filter_query.is_empty() {
                        ui.clear_list_filter();
                    }
                    // Not consumed: the caller acts on the selected item.
                    return false;
                }
                KeyCode::Backspace => {
                    ui.list_filter_query.pop();
                    ui.list_filter_indices =
                        Some(fuzzy_filter_indices(&ui.list_filter_query, items));
                    *index = 0;
                }
                KeyCode::Down => {
                    let len = ui.filtered_list_len(items.len());
                    if len > 0 {
                        *index = index.saturating_add(1).min(len - 1);
                    }
                }
                KeyCode::Up => {
                    *index = index.saturating_sub(1);
                }
                KeyCode::Char(c)
                    if !key.modifiers.contains(KeyModifiers::CONTROL)
                        && !key.modifiers.contains(KeyModifiers::ALT) =>
                {
                    ui.list_filter_query.push(c);
                    ui.list_filter_indices =
                        Some(fuzzy_filter_indices(&ui.list_filter_query, items));
                    *index = 0;
                }
                _ => {}
            }
            return true;
        }
        match key.code {
            KeyCode::Char('/') if !items.is_empty() => {
                let mut state = self.state.borrow_mut();
                state.ui_state.list_filter_active = true;
                state.ui_state.list_filter_query.clear();
                state.ui_state.list_filter_indices = Some((0..items.len()).collect());
                *index = 0;
                true
            }
            // With a committed filter, the first Esc clears it; a second
            // Esc (handled by handle_list_nav) closes the window.
            KeyCode::Esc if self.state.borrow().ui_state.list_filter_indices.is_some() => {
                self.state.borrow_mut().ui_state.clear_list_filter();
                *index = 0;
                true
            }
            _ => false,
        }
    }

    fn handle_toc_mode_keys(&mut self, key: KeyEvent, repeat_count: u32) -> eyre::Result<()> {
        let (items, mut index) = {
            let s = self.state.borrow();
            let items: Vec<String> = s
                .ui_state
                .toc_entries
                .iter()
                .map(|entry| entry.label.clone())
                .collect();
            (items, s.ui_state.toc_selected_index)
        };
        if self.handle_list_filter_keys(&key, &items, &mut index) {
            self.state.borrow_mut().ui_state.toc_selected_index = index;
            return Ok(());
        }
        let list_len = self.state.borrow().ui_state.filtered_list_len(items.len());
        if !self.handle_list_nav(&key, repeat_count, list_len, &mut index) {
            match key.code {
                KeyCode::Enter => {
                    self.jump_to_toc_entry()?;
                }
                _ => {}
            }
        } else {
            self.state.borrow_mut().ui_state.toc_selected_index = index;
        }
        Ok(())
    }

    fn handle_bookmarks_mode_keys(&mut self, key: KeyEvent, repeat_count: u32) -> eyre::Result<()> {
        let (items, mut index) = {
            let s = self.state.borrow();
            let items: Vec<String> = s
                .ui_state
                .bookmarks
                .iter()
                .map(|(name, reading_state)| Self::format_bookmark_entry(name, reading_state))
                .collect();
            (items, s.ui_state.bookmarks_selected_index)
        };
        if self.handle_list_filter_keys(&key, &items, &mut index) {
            self.state.borrow_mut().ui_state.bookmarks_selected_index = index;
            return Ok(());
        }
        let list_len = self.state.borrow().ui_state.filtered_list_len(items.len());
        if !self.handle_list_nav(&key, repeat_count, list_len, &mut index) {
            match key.code {
                KeyCode::Char('a') => {
                    self.add_bookmark()?;
                    self.reset_list_filter_after_change();
                }
                KeyCode::Char('d') => {
                    self.delete_selected_bookmark()?;
                    self.reset_list_filter_after_change();
                }
                KeyCode::Enter => {
                    self.jump_to_selected_bookmark()?;
                }
                _ => {}
            }
        } else {
            self.state.borrow_mut().ui_state.bookmarks_selected_index = index;
        }
        Ok(())
    }

    /// Drop the list filter after the underlying list changed (add/delete),
    /// since the stored indices no longer line up with the new list.
    fn reset_list_filter_after_change(&mut self) {
        let mut state = self.state.borrow_mut();
        if state.ui_state.list_filter_indices.is_some() {
            state.ui_state.clear_list_filter();
            state.ui_state.bookmarks_selected_index = 0;
            state.ui_state.highlights_selected_index = 0;
            state.ui_state.library_selected_index = 0;
        }
    }

    fn handle_links_mode_keys(&mut self, key: KeyEvent, repeat_count: u32) -> eyre::Result<()> {
        let (list_len, mut index) = {
            let s = self.state.borrow();
            (s.ui_state.links.len(), s.ui_state.links_selected_index)
        };
        if !self.handle_list_nav(&key, repeat_count, list_len, &mut index) {
            match key.code {
                KeyCode::Enter => {
                    self.follow_selected_link()?;
                }
                KeyCode::Char('y') => {
                    self.copy_selected_link()?;
                }
                _ => {}
            }
        } else {
            self.state.borrow_mut().ui_state.links_selected_index = index;
        }
        Ok(())
    }

    fn handle_link_preview_mode_keys(&mut self, key: KeyEvent) -> eyre::Result<()> {
        match key.code {
            KeyCode::Enter => {
                self.confirm_link_preview_jump();
            }
            KeyCode::Esc | KeyCode::Char('q') => {
                let mut state = self.state.borrow_mut();
                state.ui_state.link_preview = None;
                state.ui_state.open_window(WindowType::Reader);
            }
            _ => {}
        }
        Ok(())
    }

    fn handle_images_mode_keys(&mut self, key: KeyEvent, repeat_count: u32) -> eyre::Result<()> {
        let (list_len, mut index) = {
            let s = self.state.borrow();
            (
                s.ui_state.images_list.len(),
                s.ui_state.images_selected_index,
            )
        };
        if !self.handle_list_nav(&key, repeat_count, list_len, &mut index) {
            match key.code {
                KeyCode::Enter => {
                    self.open_selected_image()?;
                }
                _ => {}
            }
        } else {
            self.state.borrow_mut().ui_state.images_selected_index = index;
        }
        Ok(())
    }

    fn handle_library_mode_keys(&mut self, key: KeyEvent, repeat_count: u32) -> eyre::Result<()> {
        let (items, mut index) = {
            let s = self.state.borrow();
            let items: Vec<String> = s
                .ui_state
                .library_items
                .iter()
                .map(Self::format_library_item)
                .collect();
            (items, s.ui_state.library_selected_index)
        };
        if self.handle_list_filter_keys(&key, &items, &mut index) {
            self.state.borrow_mut().ui_state.library_selected_index = index;
            return Ok(());
        }
        let list_len = self.state.borrow().ui_state.filtered_list_len(items.len());
        if !self.handle_list_nav(&key, repeat_count, list_len, &mut index) {
            match key.code {
                KeyCode::Char('d') => {
                    self.delete_selected_library_item()?;
                    self.reset_list_filter_after_change();
                }
                KeyCode::Enter => {
                    self.open_selected_library_item()?;
                }
                _ => {}
            }
        } else {
            self.state.borrow_mut().ui_state.library_selected_index = index;
        }
        Ok(())
    }

    fn handle_settings_mode_keys(&mut self, key: KeyEvent, repeat_count: u32) -> eyre::Result<()> {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => {
                let mut state = self.state.borrow_mut();
                state.ui_state.open_window(WindowType::Reader);
            }
            KeyCode::Char('j') | KeyCode::Down => {
                let mut state = self.state.borrow_mut();
                let max_index = SettingItem::all().len().saturating_sub(1);
                let next = state
                    .ui_state
                    .settings_selected_index
                    .saturating_add(repeat_count as usize);
                state.ui_state.settings_selected_index = next.min(max_index);
            }
            KeyCode::Char('k') | KeyCode::Up => {
                let mut state = self.state.borrow_mut();
                state.ui_state.settings_selected_index = state
                    .ui_state
                    .settings_selected_index
                    .saturating_sub(repeat_count as usize);
            }
            KeyCode::Enter | KeyCode::Char('l') => {
                let selected = {
                    let state = self.state.borrow();
                    SettingItem::all()
                        .get(state.ui_state.settings_selected_index)
                        .copied()
                };
                if selected == Some(SettingItem::DictionaryClient) {
                    let mut state = self.state.borrow_mut();
                    state.ui_state.dictionary_command_query =
                        state.config.settings.dictionary_client.clone();
                    state
                        .ui_state
                        .open_window(WindowType::DictionaryCommandInput);
                } else {
                    self.toggle_selected_setting()?;
                }
            }
            KeyCode::Char('+') | KeyCode::Char('=') | KeyCode::Right => {
                self.adjust_textwidth(5)?;
            }
            KeyCode::Char('-') | KeyCode::Left => {
                self.adjust_textwidth(-5)?;
            }
            KeyCode::Char('r') => {
                self.reset_selected_setting()?;
            }
            _ => {}
        }
        Ok(())
    }

    fn handle_help_mode_keys(&mut self, key: KeyEvent, repeat_count: u32) -> eyre::Result<()> {
        let (term_width, term_height) = crossterm::terminal::size().unwrap_or((80, 24));
        let max_offset = HelpWindow::max_scroll_offset(Rect::new(0, 0, term_width, term_height));

        match key.code {
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Enter => {
                let mut state = self.state.borrow_mut();
                state.ui_state.open_window(WindowType::Reader);
            }
            KeyCode::Char('j') | KeyCode::Down => {
                let mut state = self.state.borrow_mut();
                state.ui_state.help_scroll_offset = state
                    .ui_state
                    .help_scroll_offset
                    .saturating_add(repeat_count as u16)
                    .min(max_offset);
            }
            KeyCode::Char('k') | KeyCode::Up => {
                let mut state = self.state.borrow_mut();
                state.ui_state.help_scroll_offset = state
                    .ui_state
                    .help_scroll_offset
                    .saturating_sub(repeat_count as u16);
            }
            _ => {}
        }
        Ok(())
    }

    fn handle_modal_close_keys(&mut self, key: KeyEvent) -> eyre::Result<()> {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Enter => {
                let mut state = self.state.borrow_mut();
                state.ui_state.open_window(WindowType::Reader);
            }
            _ => {}
        }
        Ok(())
    }

    fn handle_dictionary_mode_keys(
        &mut self,
        key: KeyEvent,
        repeat_count: u32,
    ) -> eyre::Result<()> {
        let (term_width, term_height) = crossterm::terminal::size().unwrap_or((80, 24));
        let max_offset = {
            let state = self.state.borrow();
            DictionaryWindow::max_scroll_offset(
                Rect::new(0, 0, term_width, term_height),
                &state.ui_state.dictionary_definition,
                state.ui_state.dictionary_loading,
            )
        };

        match key.code {
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Enter => {
                let mut state = self.state.borrow_mut();
                state.ui_state.open_window(WindowType::Reader);
            }
            KeyCode::Char('j') | KeyCode::Down => {
                let mut state = self.state.borrow_mut();
                state.ui_state.dictionary_scroll_offset = state
                    .ui_state
                    .dictionary_scroll_offset
                    .saturating_add(repeat_count as u16)
                    .min(max_offset);
            }
            KeyCode::Char('k') | KeyCode::Up => {
                let mut state = self.state.borrow_mut();
                state.ui_state.dictionary_scroll_offset = state
                    .ui_state
                    .dictionary_scroll_offset
                    .saturating_sub(repeat_count as u16);
            }
            KeyCode::PageDown => {
                let mut state = self.state.borrow_mut();
                state.ui_state.dictionary_scroll_offset = state
                    .ui_state
                    .dictionary_scroll_offset
                    .saturating_add((repeat_count as u16).saturating_mul(10))
                    .min(max_offset);
            }
            KeyCode::PageUp => {
                let mut state = self.state.borrow_mut();
                state.ui_state.dictionary_scroll_offset = state
                    .ui_state
                    .dictionary_scroll_offset
                    .saturating_sub((repeat_count as u16).saturating_mul(10));
            }
            KeyCode::Home => {
                let mut state = self.state.borrow_mut();
                state.ui_state.dictionary_scroll_offset = 0;
            }
            _ => {}
        }
        Ok(())
    }

    fn handle_dictionary_command_input_keys(&mut self, key: KeyEvent) -> eyre::Result<()> {
        match key.code {
            KeyCode::Enter => {
                let query = {
                    let state = self.state.borrow();
                    state.ui_state.dictionary_command_query.trim().to_string()
                };
                {
                    let mut state = self.state.borrow_mut();
                    state.config.settings.dictionary_client = query;
                    let _ = state.config.save();
                    state.ui_state.open_window(WindowType::Settings);
                }
            }
            KeyCode::Esc => {
                let mut state = self.state.borrow_mut();
                state.ui_state.open_window(WindowType::Settings);
            }
            KeyCode::Backspace => {
                let mut state = self.state.borrow_mut();
                state.ui_state.dictionary_command_query.pop();
            }
            KeyCode::Char(c) => {
                let mut state = self.state.borrow_mut();
                state.ui_state.dictionary_command_query.push(c);
            }
            _ => {}
        }
        Ok(())
    }

    fn handle_highlight_comment_editor_keys(&mut self, key: KeyEvent) -> eyre::Result<()> {
        match key.code {
            KeyCode::Esc => {
                let mut state = self.state.borrow_mut();
                state.ui_state.highlight_comment_buffer.clear();
                state.ui_state.highlight_comment_cursor = 0;
                state.ui_state.highlight_comment_editing_id = None;
                state.ui_state.open_window(WindowType::Highlights);
            }
            KeyCode::Char('s') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.save_highlight_comment()?;
            }
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                let mut state = self.state.borrow_mut();
                let cursor = state.ui_state.highlight_comment_cursor;
                let start = state.ui_state.highlight_comment_buffer[..cursor]
                    .rfind('\n')
                    .map(|idx| idx + 1)
                    .unwrap_or(0);
                state
                    .ui_state
                    .highlight_comment_buffer
                    .replace_range(start..cursor, "");
                state.ui_state.highlight_comment_cursor = start;
            }
            KeyCode::Char('w') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.highlight_comment_delete_word();
            }
            KeyCode::Enter => self.highlight_comment_insert("\n"),
            KeyCode::Tab => self.highlight_comment_insert("\t"),
            KeyCode::Backspace => self.highlight_comment_backspace(),
            KeyCode::Delete => self.highlight_comment_delete(),
            KeyCode::Left => self.highlight_comment_move_left(),
            KeyCode::Right => self.highlight_comment_move_right(),
            KeyCode::Home => {
                let mut state = self.state.borrow_mut();
                let cursor = state.ui_state.highlight_comment_cursor;
                state.ui_state.highlight_comment_cursor = state.ui_state.highlight_comment_buffer
                    [..cursor]
                    .rfind('\n')
                    .map(|idx| idx + 1)
                    .unwrap_or(0);
            }
            KeyCode::End => {
                let mut state = self.state.borrow_mut();
                let cursor = state.ui_state.highlight_comment_cursor;
                let tail = &state.ui_state.highlight_comment_buffer[cursor..];
                state.ui_state.highlight_comment_cursor =
                    cursor + tail.find('\n').unwrap_or(tail.len());
            }
            KeyCode::Char(c) => {
                if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT {
                    let mut s = String::new();
                    s.push(c);
                    self.highlight_comment_insert(&s);
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn highlight_comment_insert(&mut self, text: &str) {
        let mut state = self.state.borrow_mut();
        let current_len = state.ui_state.highlight_comment_buffer.chars().count();
        let available = COMMENT_MAX_CHARS.saturating_sub(current_len);
        if available == 0 {
            state.ui_state.set_message(
                "Comment length limit reached".to_string(),
                MessageType::Warning,
            );
            return;
        }
        let insert: String = text.chars().take(available).collect();
        let cursor = state.ui_state.highlight_comment_cursor;
        state
            .ui_state
            .highlight_comment_buffer
            .insert_str(cursor, &insert);
        state.ui_state.highlight_comment_cursor = cursor + insert.len();
        if insert.chars().count() < text.chars().count() {
            state.ui_state.set_message(
                "Comment length limit reached".to_string(),
                MessageType::Warning,
            );
        }
    }

    fn highlight_comment_backspace(&mut self) {
        let mut state = self.state.borrow_mut();
        let cursor = state.ui_state.highlight_comment_cursor;
        if cursor == 0 {
            return;
        }
        let prev = previous_grapheme_boundary(&state.ui_state.highlight_comment_buffer, cursor);
        state
            .ui_state
            .highlight_comment_buffer
            .replace_range(prev..cursor, "");
        state.ui_state.highlight_comment_cursor = prev;
    }

    fn highlight_comment_delete(&mut self) {
        let mut state = self.state.borrow_mut();
        let cursor = state.ui_state.highlight_comment_cursor;
        if cursor >= state.ui_state.highlight_comment_buffer.len() {
            return;
        }
        let next = next_grapheme_boundary(&state.ui_state.highlight_comment_buffer, cursor);
        state
            .ui_state
            .highlight_comment_buffer
            .replace_range(cursor..next, "");
    }

    fn highlight_comment_move_left(&mut self) {
        let mut state = self.state.borrow_mut();
        let cursor = state.ui_state.highlight_comment_cursor;
        state.ui_state.highlight_comment_cursor =
            previous_grapheme_boundary(&state.ui_state.highlight_comment_buffer, cursor);
    }

    fn highlight_comment_move_right(&mut self) {
        let mut state = self.state.borrow_mut();
        let cursor = state.ui_state.highlight_comment_cursor;
        state.ui_state.highlight_comment_cursor =
            next_grapheme_boundary(&state.ui_state.highlight_comment_buffer, cursor);
    }

    fn highlight_comment_delete_word(&mut self) {
        let mut state = self.state.borrow_mut();
        let cursor = state.ui_state.highlight_comment_cursor;
        if cursor == 0 {
            return;
        }
        let text = &state.ui_state.highlight_comment_buffer;
        let mut start = cursor;
        while start > 0 {
            let prev = previous_grapheme_boundary(text, start);
            if text[prev..start].chars().any(|c| !c.is_whitespace()) {
                start = prev;
                break;
            }
            start = prev;
        }
        while start > 0 {
            let prev = previous_grapheme_boundary(text, start);
            if text[prev..start].chars().all(|c| c.is_whitespace()) {
                break;
            }
            start = prev;
        }
        state
            .ui_state
            .highlight_comment_buffer
            .replace_range(start..cursor, "");
        state.ui_state.highlight_comment_cursor = start;
    }

    fn save_highlight_comment(&mut self) -> eyre::Result<()> {
        let (id, comment) = {
            let state = self.state.borrow();
            let Some(id) = state.ui_state.highlight_comment_editing_id.clone() else {
                return Ok(());
            };
            let comment = state
                .ui_state
                .highlight_comment_buffer
                .trim_end()
                .to_string();
            (id, comment)
        };
        let comment_opt = (!comment.trim().is_empty()).then_some(comment.as_str());
        self.db_state.update_highlight_comment(&id, comment_opt)?;
        self.refresh_highlights()?;
        let mut state = self.state.borrow_mut();
        state.ui_state.highlight_comment_buffer.clear();
        state.ui_state.highlight_comment_cursor = 0;
        state.ui_state.highlight_comment_editing_id = None;
        state.ui_state.open_window(WindowType::Highlights);
        Ok(())
    }

    /// Static render method that can be called from a closure
    fn render_static(
        frame: &mut Frame,
        state: &ApplicationState,
        board: &Board,
        content_start_rows: &[usize],
    ) {
        let theme = state.theme();

        // Fill the terminal background for light/dark themes
        if let Some(bg) = theme.text_bg {
            let base_style = if let Some(fg) = theme.text_fg {
                Style::default().fg(fg).bg(bg)
            } else {
                Style::default().bg(bg)
            };
            let bg_block = Block::default().style(base_style);
            frame.render_widget(bg_block, frame.area());
        }

        // Main reader view
        Self::render_reader_static(frame, state, board, content_start_rows, &theme);

        // Render overlays/modals if active
        if state.ui_state.show_help {
            HelpWindow::render(
                frame,
                frame.area(),
                state.ui_state.help_scroll_offset,
                &theme,
            );
        } else if state.ui_state.show_toc {
            let filter = state.ui_state.list_filter_status();
            let filtered_toc: Vec<TocEntry>;
            let toc_entries: &[TocEntry] = match state.ui_state.list_filter_indices.as_ref() {
                Some(indices) => {
                    filtered_toc = indices
                        .iter()
                        .filter_map(|&i| state.ui_state.toc_entries.get(i).cloned())
                        .collect();
                    &filtered_toc
                }
                None => &state.ui_state.toc_entries,
            };
            TocWindow::render(
                frame,
                frame.area(),
                toc_entries,
                state.ui_state.toc_selected_index,
                state.ui_state.metadata.as_ref(),
                filter.as_deref(),
                &theme,
            );
        } else if state.ui_state.show_bookmarks {
            let entries: Vec<String> = state
                .ui_state
                .bookmarks
                .iter()
                .map(|(name, reading_state)| Self::format_bookmark_entry(name, reading_state))
                .collect();
            let filter = state.ui_state.list_filter_status();
            let entries = Self::apply_list_filter(entries, &state.ui_state);
            BookmarksWindow::render(
                frame,
                frame.area(),
                "Bookmarks",
                "No bookmarks yet",
                &entries,
                state.ui_state.bookmarks_selected_index,
                None,
                filter.as_deref(),
                &theme,
            );
        } else if state.ui_state.show_highlights {
            let entries: Vec<String> = state
                .ui_state
                .highlights
                .iter()
                .map(Self::format_highlight_entry)
                .collect();
            let filter = state.ui_state.list_filter_status();
            let entries = Self::apply_list_filter(entries, &state.ui_state);
            BookmarksWindow::render(
                frame,
                frame.area(),
                "Highlights",
                "No highlights yet",
                &entries,
                state.ui_state.highlights_selected_index,
                Some("Highlights (Enter jump, e edit, d delete)"),
                filter.as_deref(),
                &theme,
            );
        } else if state.ui_state.show_library {
            let entries: Vec<String> = state
                .ui_state
                .library_items
                .iter()
                .map(Self::format_library_item)
                .collect();
            let filter = state.ui_state.list_filter_status();
            let entries = Self::apply_list_filter(entries, &state.ui_state);
            LibraryWindow::render(
                frame,
                frame.area(),
                &entries,
                state.ui_state.library_selected_index,
                filter.as_deref(),
                &theme,
            );
        } else if state.ui_state.show_search {
            let entries: Vec<String> = state
                .ui_state
                .search_results
                .iter()
                .map(|result| format!("{}: {}", result.line + 1, result.preview))
                .collect();
            SearchWindow::render(
                frame,
                frame.area(),
                &state.ui_state.search_query,
                &entries,
                state.ui_state.selected_search_result,
                &theme,
            );
        } else if state.ui_state.active_window == WindowType::LinkPreview {
            Self::render_link_preview_static(frame, state, board, &theme);
        } else if state.ui_state.show_links {
            LinksWindow::render(
                frame,
                frame.area(),
                &state.ui_state.links,
                state.ui_state.links_selected_index,
                board,
                &theme,
            );
        } else if state.ui_state.show_images {
            ImagesWindow::render(
                frame,
                frame.area(),
                &state.ui_state.images_list,
                state.ui_state.images_selected_index,
                &theme,
            );
        } else if state.ui_state.show_dictionary {
            DictionaryWindow::render(
                frame,
                frame.area(),
                &state.ui_state.dictionary_word,
                &state.ui_state.dictionary_definition,
                &state.ui_state.dictionary_client_used,
                state.ui_state.dictionary_scroll_offset,
                state.ui_state.dictionary_loading,
                state.ui_state.dictionary_is_wikipedia,
                &theme,
            );
        } else if state.ui_state.show_metadata {
            MetadataWindow::render(
                frame,
                frame.area(),
                state.ui_state.metadata.as_ref(),
                &theme,
            );
        } else if state.ui_state.show_statistics {
            StatisticsWindow::render(frame, frame.area(), &state.ui_state.statistics, &theme);
        } else if state.ui_state.active_window == WindowType::DictionaryCommandInput {
            Self::render_dictionary_command_input_static(frame, state, &theme);
        } else if state.ui_state.active_window == WindowType::HighlightCommentEditor {
            Self::render_highlight_comment_editor_static(frame, state, &theme);
        } else if state.ui_state.active_window == WindowType::ConfirmDeleteHighlight {
            Self::render_confirm_delete_highlight_static(frame, state, &theme);
        } else if state.ui_state.show_settings {
            let entries = Self::settings_entries(state);
            SettingsWindow::render(
                frame,
                frame.area(),
                &entries,
                state.ui_state.settings_selected_index,
                &theme,
            );
        }

        // Render TTS synthesis waiting popup
        if state.ui_state.tts_converting {
            let spinner_frames = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
            let frame_char = spinner_frames[state.ui_state.tts_anim_frame % spinner_frames.len()];
            let label = format!("  {} Synthesizing…  ", frame_char);
            let popup_w = (label.chars().count() as u16) + 2;
            let popup_h = 3u16;
            let area = frame.area();
            let popup_area = Rect::new(
                area.x + area.width.saturating_sub(popup_w) / 2,
                area.y + area.height.saturating_sub(popup_h) / 2,
                popup_w.min(area.width),
                popup_h.min(area.height),
            );
            frame.render_widget(ratatui::widgets::Clear, popup_area);
            let block = ratatui::widgets::Block::default()
                .borders(ratatui::widgets::Borders::ALL)
                .style(theme.base_style());
            let inner = block.inner(popup_area);
            frame.render_widget(block, popup_area);
            frame.render_widget(
                ratatui::widgets::Paragraph::new(label).style(theme.base_style()),
                inner,
            );
        }

        // Render message if present
        if let Some(ref message) = state.ui_state.message {
            Self::render_message_static(frame, message, &state.ui_state.message_type, &theme);
        }

        // Visual-mode `/`-search prompt: a single-line bar at the bottom of the
        // screen showing what the user is typing. Drawn on top of everything
        // else so it is always visible while editing the query.
        if state.ui_state.visual_search_input_active {
            let frame_area = frame.area();
            if frame_area.height > 0 {
                let bar = Rect {
                    x: frame_area.x,
                    y: frame_area.y + frame_area.height - 1,
                    width: frame_area.width,
                    height: 1,
                };
                let text = format!("/{}", state.ui_state.visual_search_query);
                let bar_style = theme.base_style().fg(theme.info_fg);
                frame.render_widget(Clear, bar);
                frame.render_widget(Paragraph::new(text).style(bar_style), bar);
            }
        }
    }

    /// Keep only the entries selected by the active list filter, in
    /// filter (score) order. A no-op when no filter is applied.
    fn apply_list_filter(entries: Vec<String>, ui_state: &UiState) -> Vec<String> {
        match ui_state.list_filter_indices.as_ref() {
            Some(indices) => indices
                .iter()
                .filter_map(|&i| entries.get(i).cloned())
                .collect(),
            None => entries,
        }
    }

    fn format_bookmark_entry(name: &str, reading_state: &ReadingState) -> String {
        format!("{} (line {})", name, reading_state.row + 1)
    }

    fn format_highlight_entry(highlight: &Highlight) -> String {
        let status = match highlight.resolution_status.as_str() {
            "resolved" => String::new(),
            status => format!(" [{status}]"),
        };
        let comment = highlight
            .comment
            .as_deref()
            .filter(|text| !text.trim().is_empty())
            .map(|text| format!(" - {}", text.lines().next().unwrap_or("")))
            .unwrap_or_default();
        format!(
            "{}:{}{} {}{}",
            highlight.content_index + 1,
            highlight.approx_offset,
            status,
            highlight.exact,
            comment
        )
    }

    fn format_library_item(item: &LibraryItem) -> String {
        let reading_progress_str = match item.reading_progress {
            Some(p) => {
                let pct = (p * 100.0).round() as i32;
                let pct = pct.clamp(0, 100);
                format!("{:>4}", format!("{}%", pct))
            }
            None => format!("{:>4}", "N/A"),
        };

        let filename = {
            let path = &item.filepath;
            if let Ok(home) = std::env::var("HOME") {
                if path.starts_with(&home) {
                    path.replacen(&home, "~", 1)
                } else {
                    path.clone()
                }
            } else {
                path.clone()
            }
        };

        let book_name =
            if let (Some(title), Some(author)) = (item.title.as_ref(), item.author.as_ref()) {
                format!("{} - {} ({})", title, author, filename)
            } else if item.title.is_none() && item.author.is_some() {
                format!("{} - {}", filename, item.author.as_ref().unwrap())
            } else {
                filename
            };

        let last_read_local = item.last_read.with_timezone(&Local);
        let last_read_str = last_read_local.format("%I:%M%p %b %d").to_string();

        format!("{} {}: {}", reading_progress_str, last_read_str, book_name)
    }

    fn settings_entries(state: &ApplicationState) -> Vec<String> {
        let settings = &state.config.settings;
        SettingItem::all()
            .iter()
            .map(|item| match item {
                SettingItem::ShowLineNumbers => {
                    format!("Show line numbers: {}", settings.show_line_numbers)
                }
                SettingItem::MouseSupport => format!("Mouse support: {}", settings.mouse_support),
                SettingItem::PageScrollAnimation => {
                    format!("Page scroll animation: {}", settings.page_scroll_animation)
                }
                SettingItem::ShowProgressIndicator => {
                    format!(
                        "Show progress indicator: {}",
                        settings.show_progress_indicator
                    )
                }
                SettingItem::SeamlessBetweenChapters => {
                    format!(
                        "Seamless between chapters: {}",
                        settings.seamless_between_chapters
                    )
                }
                SettingItem::DictionaryClient => {
                    let client = if settings.dictionary_client.trim().is_empty() {
                        "auto"
                    } else {
                        settings.dictionary_client.trim()
                    };
                    if client == "auto" {
                        "Dictionary client: auto (default)".to_string()
                    } else {
                        format!("Dictionary client: {client}")
                    }
                }
                SettingItem::TtsEngine => {
                    let engine = settings.preferred_tts_engine.as_deref().unwrap_or("purr");
                    format!("TTS engine: {engine}")
                }
                SettingItem::Width => format!("Text width: {}", state.reading_state.textwidth),
                SettingItem::ShowTopBar => format!("Show top bar: {}", settings.show_top_bar),
                SettingItem::ColorTheme => {
                    let suffix = if state.book_color_theme.is_some() {
                        " (book)"
                    } else {
                        " (global)"
                    };
                    format!(
                        "Color theme: {}{}",
                        state.effective_color_theme().name(),
                        suffix
                    )
                }
            })
            .collect()
    }

    /// Static method to render the main reader view
    fn render_reader_static(
        frame: &mut Frame,
        state: &ApplicationState,
        board: &Board,
        content_start_rows: &[usize],
        theme: &Theme,
    ) {
        let frame_area = frame.area();
        let percent_text = if state.config.settings.show_progress_indicator {
            let total_lines = board.total_lines();
            if total_lines > 0 {
                let percent = (state.reading_state.row.saturating_mul(100)) / total_lines;
                Some(format!("{}%", percent.min(100)))
            } else {
                None
            }
        } else {
            None
        };
        let title = state
            .ui_state
            .metadata
            .as_ref()
            .and_then(|meta| meta.title.as_deref())
            .unwrap_or("repy");

        let show_top_bar = state.config.settings.show_top_bar;
        let top_bar_height = if show_top_bar { 1 } else { 0 };
        let top_gap_height = if show_top_bar { 2 } else { 0 };
        let bottom_gap_height = 2;

        // Reserve space for header and spacing even when the header is hidden.
        let chunks = Layout::default()
            .direction(ratatui::layout::Direction::Vertical)
            .constraints([
                Constraint::Length(top_bar_height),
                Constraint::Length(top_gap_height),
                Constraint::Min(0),
                Constraint::Length(bottom_gap_height),
            ])
            .split(frame_area);

        // Main content area with centered margins
        // Calculate padding from stored textwidth (minimum 5 on each side, unless window ≤ 20)
        // The line-number gutter ("9999 ") occupies 5 extra columns on top of
        // the wrapped text width, and the highlight margin indicator one
        // more, so reserve room for them to avoid visual wrap.
        let mut gutter_width = if state.config.settings.show_line_numbers {
            5
        } else {
            0
        };
        if !state.ui_state.highlights.is_empty() {
            gutter_width += 1;
        }
        let available_width = chunks[2].width as usize;
        let padding = if available_width <= 20 {
            0
        } else {
            (available_width.saturating_sub(state.reading_state.textwidth + gutter_width) / 2)
                .max(5)
        };
        let desired_width = available_width.saturating_sub(padding * 2).max(20) as u16;

        let content_width = desired_width.min(chunks[2].width);
        let left_pad = (chunks[2].width.saturating_sub(content_width)) / 2;
        let content_area = Rect {
            x: chunks[2].x + left_pad,
            y: chunks[2].y,
            width: content_width,
            height: chunks[2].height,
        };

        // Link handling: keep main text untouched; show a subtle header hint only when the page has
        // links. Pressing `u` opens a list; Enter jumps for internal anchors when possible.
        let visible_start = state.reading_state.row.saturating_sub(1);
        let visible_end = visible_start.saturating_add(content_area.height as usize);
        let link_count = board.link_count_in_range(visible_start, visible_end);
        let link_hint = if link_count > 0 {
            Some(format!("links:{} (u)", link_count))
        } else {
            None
        };
        let mode_hint = if state.ui_state.active_window == WindowType::Visual {
            if state.ui_state.visual_anchor.is_some() {
                Some("-- SELECTION MODE --".to_string())
            } else {
                Some("-- CURSOR MODE --".to_string())
            }
        } else {
            None
        };
        let page_text = board
            .current_page_label(state.reading_state.row)
            .map(|label| format!("p.{}", label));
        let progress_text = match (page_text, percent_text) {
            (Some(page), Some(pct)) => Some(format!("{} {}", page, pct)),
            (Some(page), None) => Some(page),
            (None, Some(pct)) => Some(pct),
            (None, None) => None,
        };
        let time_left_hint = state
            .ui_state
            .statistics
            .estimated_chapter_minutes_left
            .filter(|minutes| *minutes > 0)
            .map(|minutes| format!("~{} left", Self::format_minutes_compact(minutes)));
        let search_hint = if state.ui_state.search_results.is_empty() {
            None
        } else {
            Some(format!(
                "match {}/{}",
                state.ui_state.selected_search_result + 1,
                state.ui_state.search_results.len()
            ))
        };
        let right_parts: Vec<String> = [
            mode_hint,
            search_hint,
            link_hint,
            time_left_hint,
            progress_text,
        ]
        .into_iter()
        .flatten()
        .collect();
        let right_text = if right_parts.is_empty() {
            None
        } else {
            Some(right_parts.join(" "))
        };
        if show_top_bar {
            let header_line =
                Self::build_header_line(title, right_text.as_deref(), chunks[0].width);
            let header = Paragraph::new(Line::from(header_line));
            frame.render_widget(header, chunks[0]);
        }

        board.render(frame, content_area, state, Some(content_start_rows), theme);
    }

    fn build_header_line(title: &str, right_text: Option<&str>, width: u16) -> String {
        let width = width as usize;
        if width == 0 {
            return String::new();
        }

        let mut buffer = vec![' '; width];
        let right_len = right_text.map(|text| text.len()).unwrap_or(0);
        let content_width = if right_len > 0 {
            width.saturating_sub(right_len + 1)
        } else {
            width
        };

        let mut title_text = title.to_string();
        if title_text.len() > content_width {
            title_text.truncate(content_width);
        }
        let title_start = (content_width.saturating_sub(title_text.len())) / 2;
        for (i, ch) in title_text.chars().enumerate() {
            if title_start + i < buffer.len() {
                buffer[title_start + i] = ch;
            }
        }

        if let Some(right_text) = right_text {
            let start = width.saturating_sub(right_len);
            for (i, ch) in right_text.chars().enumerate() {
                if start + i < buffer.len() {
                    buffer[start + i] = ch;
                }
            }
        }

        buffer.into_iter().collect()
    }

    fn format_minutes_compact(minutes: i64) -> String {
        let minutes = minutes.max(0);
        if minutes >= 60 {
            format!("{}h{}m", minutes / 60, minutes % 60)
        } else {
            format!("{minutes}m")
        }
    }

    fn render_message_static(
        frame: &mut Frame,
        message: &str,
        message_type: &MessageType,
        theme: &Theme,
    ) {
        let color = match message_type {
            MessageType::Info => theme.info_fg,
            MessageType::Warning => theme.warning_fg,
            MessageType::Error => theme.error_fg,
        };

        let frame_area = frame.area();
        let max_width = frame_area.width.saturating_sub(4);

        // Simple line wrapping calculation to estimate height
        let mut lines = 1;
        let mut current_line_len = 0;
        for word in message.split_whitespace() {
            let word_len = word.chars().count();
            if current_line_len + word_len + 1 > max_width as usize {
                lines += 1;
                current_line_len = word_len;
            } else {
                current_line_len += word_len + 1;
            }
        }

        let height = (lines + 2).min(frame_area.height.saturating_sub(4)) as u16;
        let height = height.max(3);

        let message_paragraph = Paragraph::new(message)
            .style(Style::default().fg(color))
            .block(Block::default().borders(Borders::ALL))
            .wrap(Wrap { trim: true });

        let area = Rect {
            x: frame_area.x + 2,
            y: frame_area.y + 2,
            width: max_width,
            height,
        };

        frame.render_widget(Clear, area);
        frame.render_widget(message_paragraph, area);
    }

    fn render_dictionary_command_input_static(
        frame: &mut Frame,
        state: &ApplicationState,
        theme: &Theme,
    ) {
        let area = Rect::new(
            frame.area().x + frame.area().width / 4,
            frame.area().y + frame.area().height / 2 - 2,
            frame.area().width / 2,
            3,
        );

        let input = Paragraph::new(Line::from(state.ui_state.dictionary_command_query.as_str()))
            .block(
                Block::default()
                    .title("Dictionary Command Template (%q for query)")
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(theme.info_fg)),
            );

        frame.render_widget(Clear, area);
        frame.render_widget(input, area);

        // Set cursor position
        frame.set_cursor_position((
            area.x + state.ui_state.dictionary_command_query.len() as u16 + 1,
            area.y + 1,
        ));
    }

    fn render_highlight_comment_editor_static(
        frame: &mut Frame,
        state: &ApplicationState,
        theme: &Theme,
    ) {
        let area = Rect::new(
            frame.area().x + frame.area().width / 6,
            frame.area().y + frame.area().height / 6,
            frame.area().width * 2 / 3,
            frame.area().height * 2 / 3,
        );
        let block = Block::default()
            .title("Comment (Ctrl+s save, Esc cancel)")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.info_fg))
            .style(theme.base_style());
        let inner = block.inner(area);
        let text = state.ui_state.highlight_comment_buffer.as_str();
        let paragraph = Paragraph::new(text)
            .block(block)
            .wrap(Wrap { trim: false })
            .style(theme.base_style());
        frame.render_widget(Clear, area);
        frame.render_widget(paragraph, area);

        let (row, col) =
            wrapped_cursor_position(text, state.ui_state.highlight_comment_cursor, inner.width);
        if row < inner.height {
            frame.set_cursor_position((
                inner.x + col.min(inner.width.saturating_sub(1)),
                inner.y + row,
            ));
        }
    }

    fn render_confirm_delete_highlight_static(
        frame: &mut Frame,
        state: &ApplicationState,
        theme: &Theme,
    ) {
        let Some(highlight) = state.ui_state.pending_delete_highlight.as_ref() else {
            return;
        };
        let area = frame.area();
        let width = (area.width * 2 / 3).max(40).min(area.width);
        let height = 10u16.min(area.height);
        let x = area.x + (area.width.saturating_sub(width)) / 2;
        let y = area.y + (area.height.saturating_sub(height)) / 2;
        let popup_area = Rect::new(x, y, width, height);

        let truncate = |s: &str, n: usize| -> String {
            let collected: Vec<char> = s.chars().collect();
            if collected.len() > n {
                let head: String = collected.iter().take(n).collect();
                format!("{head}…")
            } else {
                s.to_string()
            }
        };
        let exact = truncate(&highlight.exact, 120);
        let comment = highlight
            .comment
            .as_deref()
            .map(|c| truncate(c, 200))
            .unwrap_or_default();

        let lines: Vec<Line> = vec![
            Line::from(""),
            Line::from(format!("  Highlight: {exact}")),
            Line::from(format!("  Comment:   {comment}")),
            Line::from(""),
            Line::from("  Delete this highlight and its comment? (y/N)"),
        ];

        let block = Block::default()
            .title("Confirm Delete")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.warning_fg))
            .style(theme.base_style());
        let paragraph = Paragraph::new(lines)
            .block(block)
            .wrap(Wrap { trim: false })
            .style(theme.base_style());
        frame.render_widget(Clear, popup_area);
        frame.render_widget(paragraph, popup_area);
    }

    fn render_link_preview_static(
        frame: &mut Frame,
        state: &ApplicationState,
        board: &Board,
        theme: &Theme,
    ) {
        let Some(entry) = state.ui_state.link_preview.as_ref() else {
            return;
        };
        let area = frame.area();
        let width = (area.width * 2 / 3).max(40).min(area.width);
        let height = 14u16.min(area.height);
        let x = area.x + (area.width.saturating_sub(width)) / 2;
        let y = area.y + (area.height.saturating_sub(height)) / 2;
        let popup_area = Rect::new(x, y, width, height);

        let preview_text = LinksWindow::build_preview_text_with_limit(entry, board, 10);
        let label = if entry.label.trim().is_empty() {
            entry.url.as_str()
        } else {
            entry.label.as_str()
        };
        let text = format!("{preview_text}\n\nEnter: jump  Esc/q: stay");
        let block = Block::default()
            .title(format!(" Link Preview: {label} "))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.info_fg))
            .style(theme.base_style());
        let paragraph = Paragraph::new(text)
            .block(block)
            .wrap(Wrap { trim: true })
            .style(theme.base_style());
        frame.render_widget(Clear, popup_area);
        frame.render_widget(paragraph, popup_area);
    }

    fn open_toc_window(&mut self) -> eyre::Result<()> {
        let toc_entries = if let Some(epub) = self.ebook.as_ref() {
            epub.toc_entries().clone()
        } else {
            Vec::new()
        };

        let current_row = self.state.borrow().reading_state.row;
        let mut selected_index = 0;

        for i in 0..toc_entries.len() {
            if let Some(row) = self.toc_activation_row(&toc_entries, i)
                && row <= current_row
            {
                selected_index = i;
            }
        }

        let mut state = self.state.borrow_mut();
        state.ui_state.toc_entries = toc_entries;
        state.ui_state.toc_selected_index = selected_index;
        state.ui_state.open_window(WindowType::Toc);
        Ok(())
    }

    fn open_bookmarks_window(&mut self) -> eyre::Result<()> {
        let bookmarks = if let Some(epub) = self.ebook.as_ref() {
            self.db_state.get_bookmarks(epub)?
        } else {
            Vec::new()
        };
        let mut state = self.state.borrow_mut();
        state.ui_state.bookmarks = bookmarks;
        state.ui_state.bookmarks_selected_index = 0;
        state.ui_state.open_window(WindowType::Bookmarks);
        Ok(())
    }

    fn open_highlights_window(&mut self) -> eyre::Result<()> {
        self.refresh_highlights()?;
        let mut state = self.state.borrow_mut();
        if state.ui_state.highlights_selected_index >= state.ui_state.highlights.len() {
            state.ui_state.highlights_selected_index =
                state.ui_state.highlights.len().saturating_sub(1);
        }
        state.ui_state.open_window(WindowType::Highlights);
        Ok(())
    }

    fn refresh_highlights(&mut self) -> eyre::Result<()> {
        let highlights = {
            let state = self.state.borrow();
            match state.ui_state.book_identity.as_ref() {
                Some(identity) => self.db_state.list_highlights(&identity.book_id)?,
                None => Vec::new(),
            }
        };
        self.state.borrow_mut().ui_state.highlights = highlights;
        self.refresh_highlight_ranges()
    }

    fn refresh_highlight_ranges(&mut self) -> eyre::Result<()> {
        let highlights = self.state.borrow().ui_state.highlights.clone();
        let mut all_ranges: HashMap<usize, Vec<HighlightRange>> = HashMap::new();
        let mut statuses = Vec::new();
        for content_index in 0..self.chapter_text_structures.len() {
            let Some(global_start_row) = self.content_start_rows.get(content_index).copied() else {
                continue;
            };
            for (idx, highlight) in highlights.iter().enumerate() {
                if highlight.content_index != content_index {
                    continue;
                }
                match annotations::resolve_highlight(
                    idx,
                    highlight,
                    &self.chapter_text_structures[content_index].text_lines,
                    global_start_row,
                ) {
                    annotations::Resolution::Resolved(ranges) => {
                        statuses.push((highlight.id.clone(), "resolved".to_string()));
                        for range in ranges {
                            all_ranges.entry(range.row).or_default().push(range);
                        }
                    }
                    annotations::Resolution::Ambiguous => {
                        statuses.push((highlight.id.clone(), "ambiguous".to_string()))
                    }
                    annotations::Resolution::Unresolved => {
                        statuses.push((highlight.id.clone(), "unresolved".to_string()))
                    }
                }
            }
        }
        {
            let mut state = self.state.borrow_mut();
            state.ui_state.highlight_ranges = all_ranges;
            for highlight in &mut state.ui_state.highlights {
                if let Some((_, status)) = statuses.iter().find(|(id, _)| id == &highlight.id) {
                    if highlight.resolution_status != *status {
                        highlight.resolution_status = status.clone();
                    }
                }
            }
        }
        for (id, status) in statuses {
            if highlights
                .iter()
                .find(|highlight| highlight.id == id)
                .is_some_and(|highlight| highlight.resolution_status != status)
            {
                self.db_state.update_highlight_status(&id, &status)?;
            }
        }
        Ok(())
    }

    fn handle_highlights_mode_keys(
        &mut self,
        key: KeyEvent,
        repeat_count: u32,
    ) -> eyre::Result<()> {
        let (items, mut index) = {
            let s = self.state.borrow();
            let items: Vec<String> = s
                .ui_state
                .highlights
                .iter()
                .map(Self::format_highlight_entry)
                .collect();
            (items, s.ui_state.highlights_selected_index)
        };
        if self.handle_list_filter_keys(&key, &items, &mut index) {
            self.state.borrow_mut().ui_state.highlights_selected_index = index;
            return Ok(());
        }
        let list_len = self.state.borrow().ui_state.filtered_list_len(items.len());
        if !self.handle_list_nav(&key, repeat_count, list_len, &mut index) {
            match key.code {
                KeyCode::Enter => self.jump_to_selected_highlight()?,
                KeyCode::Char('e') => self.edit_selected_highlight_comment()?,
                KeyCode::Char('d') => {
                    self.delete_selected_highlight()?;
                    self.reset_list_filter_after_change();
                }
                _ => {}
            }
        } else {
            self.state.borrow_mut().ui_state.highlights_selected_index = index;
        }
        Ok(())
    }

    fn jump_to_selected_highlight(&mut self) -> eyre::Result<()> {
        let highlight = {
            let state = self.state.borrow();
            state
                .ui_state
                .selected_list_index(state.ui_state.highlights_selected_index)
                .and_then(|i| state.ui_state.highlights.get(i))
                .cloned()
        };
        let Some(highlight) = highlight else {
            return Ok(());
        };
        let target_row = {
            let state = self.state.borrow();
            state
                .ui_state
                .highlight_ranges
                .values()
                .flat_map(|ranges| ranges.iter())
                .filter(|range| {
                    state
                        .ui_state
                        .highlights
                        .get(range.highlight_index)
                        .is_some_and(|h| h.id == highlight.id)
                })
                .min_by_key(|range| range.row)
                .map(|range| range.row)
        };
        if let Some(target_row) = target_row {
            self.record_jump_position();
            let mut state = self.state.borrow_mut();
            state.reading_state.row = target_row;
            state.ui_state.open_window(WindowType::Reader);
        } else {
            self.state.borrow_mut().ui_state.set_message(
                "Highlight is unresolved in this EPUB version".to_string(),
                MessageType::Warning,
            );
        }
        Ok(())
    }

    fn edit_selected_highlight_comment(&mut self) -> eyre::Result<()> {
        let highlight = {
            let state = self.state.borrow();
            state
                .ui_state
                .selected_list_index(state.ui_state.highlights_selected_index)
                .and_then(|i| state.ui_state.highlights.get(i))
                .cloned()
        };
        let Some(highlight) = highlight else {
            return Ok(());
        };
        let mut state = self.state.borrow_mut();
        state.ui_state.highlight_comment_buffer = highlight.comment.unwrap_or_default();
        state.ui_state.highlight_comment_cursor = state.ui_state.highlight_comment_buffer.len();
        state.ui_state.highlight_comment_editing_id = Some(highlight.id);
        state
            .ui_state
            .open_window(WindowType::HighlightCommentEditor);
        Ok(())
    }

    fn delete_selected_highlight(&mut self) -> eyre::Result<()> {
        let id = {
            let state = self.state.borrow();
            state
                .ui_state
                .selected_list_index(state.ui_state.highlights_selected_index)
                .and_then(|i| state.ui_state.highlights.get(i))
                .map(|highlight| highlight.id.clone())
        };
        if let Some(id) = id {
            self.db_state.delete_highlight(&id)?;
            self.refresh_highlights()?;
        }
        Ok(())
    }

    fn open_links_window(&mut self) -> eyre::Result<()> {
        let (start, end) = self.visible_line_range();
        let mut links = self.board.links_in_range(start, end);

        // Resolve target rows for internal links
        for link in &mut links {
            let base_content = self
                .content_index_for_row(link.row)
                .and_then(|index| self.ebook.as_ref()?.resource_path_for_content_index(index));

            if let Some(target_row) =
                self.resolve_internal_link_row(&link.url, base_content.as_deref())
            {
                link.target_row = Some(target_row);
            }
        }

        let mut state = self.state.borrow_mut();
        if links.is_empty() {
            state
                .ui_state
                .set_message("No links on this page".to_string(), MessageType::Info);
            return Ok(());
        }
        state.ui_state.links = links;
        state.ui_state.links_selected_index = 0;
        state.ui_state.open_window(WindowType::Links);
        Ok(())
    }

    fn open_images_window(&mut self) -> eyre::Result<()> {
        let (start, end) = self.visible_line_range();

        let mut images = Vec::new();
        if let Some(_lines) = self.board.lines() {
            for i in start..end {
                if let Some(src) = self.board.image_src(i) {
                    images.push((i, src));
                }
            }
        }

        let mut state = self.state.borrow_mut();
        if images.is_empty() {
            state
                .ui_state
                .set_message("No images on this page".to_string(), MessageType::Info);
            return Ok(());
        }
        state.ui_state.images_list = images;
        state.ui_state.images_selected_index = 0;
        state.ui_state.open_window(WindowType::Images);
        Ok(())
    }

    fn open_library_window(&mut self) -> eyre::Result<()> {
        let library_items = self.db_state.get_from_history()?;
        let mut state = self.state.borrow_mut();
        state.ui_state.library_items = library_items;
        state.ui_state.library_selected_index = 0;
        state.ui_state.open_window(WindowType::Library);
        Ok(())
    }

    fn open_metadata_window(&mut self) -> eyre::Result<()> {
        let metadata = self.ebook.as_ref().map(|epub| epub.get_meta().clone());
        let mut state = self.state.borrow_mut();
        state.ui_state.metadata = metadata;
        state.ui_state.open_window(WindowType::Metadata);
        Ok(())
    }

    fn open_statistics_window(&mut self) -> eyre::Result<()> {
        // Re-query the database so the window reflects the latest totals.
        self.cached_statistics = None;
        self.refresh_statistics_snapshot()?;
        self.state
            .borrow_mut()
            .ui_state
            .open_window(WindowType::Statistics);
        Ok(())
    }

    // Navigation methods
    fn move_cursor(&mut self, direction: AppDirection) {
        let (seamless, show_top_bar) = {
            let state = self.state.borrow();
            (
                state.config.settings.seamless_between_chapters,
                state.config.settings.show_top_bar,
            )
        };
        let mut state = self.state.borrow_mut();
        let total_lines = self.board.total_lines();
        let current_row = state.reading_state.row;
        let page = Self::page_size_for(show_top_bar);

        match direction {
            AppDirection::Up => {
                if current_row > 0 {
                    state.reading_state.row -= 1;
                }
            }
            AppDirection::Down => {
                if current_row < total_lines.saturating_sub(1) {
                    state.reading_state.row += 1;
                }
            }
            AppDirection::PageUp => {
                if !seamless
                    && let Some(index) = self.content_index_for_row(current_row)
                    && let Some((chapter_start, _chapter_end)) =
                        self.chapter_bounds_for_index(index)
                {
                    let current_start = current_row.saturating_sub(1);
                    if current_start <= chapter_start {
                        if index > 0
                            && let Some((prev_start, prev_end)) =
                                self.chapter_bounds_for_index(index - 1)
                        {
                            let last_start = prev_end
                                .saturating_sub(page.saturating_sub(1))
                                .max(prev_start);
                            state.reading_state.row = Self::row_from_start(last_start);
                            return;
                        }
                        state.reading_state.row = Self::row_from_start(chapter_start);
                        return;
                    }

                    let new_start = current_start.saturating_sub(page);
                    let clamped = if new_start < chapter_start {
                        chapter_start
                    } else {
                        new_start
                    };
                    state.reading_state.row = Self::row_from_start(clamped);
                    return;
                }
                state.reading_state.row = current_row.saturating_sub(page);
            }
            AppDirection::PageDown => {
                if !seamless
                    && let Some(index) = self.content_index_for_row(current_row)
                    && let Some((chapter_start, chapter_end)) = self.chapter_bounds_for_index(index)
                {
                    let current_start = current_row.saturating_sub(1);
                    let last_start = chapter_end
                        .saturating_sub(page.saturating_sub(1))
                        .max(chapter_start);
                    if current_start >= last_start {
                        if let Some(next_start) = self.content_start_rows.get(index + 1).copied() {
                            state.reading_state.row =
                                Self::row_from_start(next_start.min(total_lines.saturating_sub(1)));
                            return;
                        }
                        state.reading_state.row = Self::row_from_start(last_start);
                        return;
                    }

                    let new_start = current_start.saturating_add(page);
                    let clamped = if new_start > last_start {
                        last_start
                    } else {
                        new_start
                    };
                    state.reading_state.row = Self::row_from_start(clamped);
                    return;
                }
                let next = current_row.saturating_add(page);
                state.reading_state.row = next.min(total_lines.saturating_sub(1));
            }
            AppDirection::HalfPageUp => {
                let half_page = (page / 2).max(1);
                if !seamless
                    && let Some(index) = self.content_index_for_row(current_row)
                    && let Some((chapter_start, _chapter_end)) =
                        self.chapter_bounds_for_index(index)
                {
                    let current_start = current_row.saturating_sub(1);
                    if current_start <= chapter_start {
                        if index > 0
                            && let Some((prev_start, prev_end)) =
                                self.chapter_bounds_for_index(index - 1)
                        {
                            let last_start = prev_end
                                .saturating_sub(half_page.saturating_sub(1))
                                .max(prev_start);
                            state.reading_state.row = Self::row_from_start(last_start);
                            return;
                        }
                        state.reading_state.row = Self::row_from_start(chapter_start);
                        return;
                    }

                    let new_start = current_start.saturating_sub(half_page);
                    let clamped = if new_start < chapter_start {
                        chapter_start
                    } else {
                        new_start
                    };
                    state.reading_state.row = Self::row_from_start(clamped);
                    return;
                }
                state.reading_state.row = current_row.saturating_sub(half_page);
            }
            AppDirection::HalfPageDown => {
                let half_page = (page / 2).max(1);
                if !seamless
                    && let Some(index) = self.content_index_for_row(current_row)
                    && let Some((chapter_start, chapter_end)) = self.chapter_bounds_for_index(index)
                {
                    let current_start = current_row.saturating_sub(1);
                    let last_start = chapter_end
                        .saturating_sub(half_page.saturating_sub(1))
                        .max(chapter_start);
                    if current_start >= last_start {
                        if let Some(next_start) = self.content_start_rows.get(index + 1).copied() {
                            state.reading_state.row =
                                Self::row_from_start(next_start.min(total_lines.saturating_sub(1)));
                            return;
                        }
                        state.reading_state.row = Self::row_from_start(last_start);
                        return;
                    }

                    let new_start = current_start.saturating_add(half_page);
                    let clamped = if new_start > last_start {
                        last_start
                    } else {
                        new_start
                    };
                    state.reading_state.row = Self::row_from_start(clamped);
                    return;
                }
                let next = current_row.saturating_add(half_page);
                state.reading_state.row = next.min(total_lines.saturating_sub(1));
            }
            _ => {}
        }
    }

    fn move_visual_cursor(&mut self, direction: AppDirection) {
        let total_lines = self.board.total_lines();
        if total_lines == 0 {
            return;
        }

        let (row, col) = {
            let state = self.state.borrow();
            match state.ui_state.visual_cursor {
                Some(pos) => pos,
                None => return,
            }
        };

        let current_line_len = self.board.line_char_count(row);
        let (new_row, mut new_col) = match direction {
            AppDirection::Left => {
                if col > 0 {
                    (row, col - 1)
                } else if row > 0 {
                    let prev_row = row - 1;
                    let prev_len = self.board.line_char_count(prev_row);
                    (prev_row, prev_len.saturating_sub(1))
                } else {
                    (row, col)
                }
            }
            AppDirection::Right => {
                if current_line_len > 0 && col + 1 < current_line_len {
                    (row, col + 1)
                } else if row + 1 < total_lines {
                    (row + 1, 0)
                } else {
                    (row, col)
                }
            }
            AppDirection::Up => {
                if row > 0 {
                    let prev_row = row - 1;
                    let prev_len = self.board.line_char_count(prev_row);
                    (prev_row, col.min(prev_len.saturating_sub(1)))
                } else {
                    (row, col)
                }
            }
            AppDirection::Down => {
                if row + 1 < total_lines {
                    let next_row = row + 1;
                    let next_len = self.board.line_char_count(next_row);
                    (next_row, col.min(next_len.saturating_sub(1)))
                } else {
                    (row, col)
                }
            }
            _ => (row, col),
        };

        if self.board.line_char_count(new_row) == 0 {
            new_col = 0;
        }

        self.set_visual_cursor_and_scroll((new_row, new_col));
    }

    fn move_visual_cursor_word_forward(&mut self) {
        let Some(mut pos) = self.current_visual_cursor() else {
            return;
        };
        let Some(next) = self.next_visual_pos(pos) else {
            return;
        };
        pos = next;

        while let Some(ch) = self.char_at_visual_pos(pos) {
            if !Self::is_word_char(ch) {
                break;
            }
            let Some(next) = self.next_visual_pos(pos) else {
                self.set_visual_cursor_and_scroll(pos);
                return;
            };
            pos = next;
        }

        while let Some(ch) = self.char_at_visual_pos(pos) {
            if Self::is_word_char(ch) {
                self.set_visual_cursor_and_scroll(pos);
                return;
            }
            let Some(next) = self.next_visual_pos(pos) else {
                return;
            };
            pos = next;
        }
    }

    fn move_visual_cursor_word_backward(&mut self) {
        let Some(mut pos) = self.current_visual_cursor() else {
            return;
        };
        let Some(prev) = self.prev_visual_pos(pos) else {
            return;
        };
        pos = prev;

        while let Some(ch) = self.char_at_visual_pos(pos) {
            if Self::is_word_char(ch) {
                break;
            }
            let Some(prev) = self.prev_visual_pos(pos) else {
                return;
            };
            pos = prev;
        }

        while let Some(prev) = self.prev_visual_pos(pos) {
            let Some(ch) = self.char_at_visual_pos(prev) else {
                break;
            };
            if !Self::is_word_char(ch) {
                break;
            }
            pos = prev;
        }

        if self.char_at_visual_pos(pos).is_some_and(Self::is_word_char) {
            self.set_visual_cursor_and_scroll(pos);
        }
    }

    fn move_visual_cursor_word_end(&mut self) {
        let Some(mut pos) = self.current_visual_cursor() else {
            return;
        };

        // Step 1: advance at least one position
        let Some(next) = self.next_visual_pos(pos) else {
            return;
        };
        pos = next;

        // Step 2: skip non-word characters (whitespace, punctuation)
        while let Some(ch) = self.char_at_visual_pos(pos) {
            if Self::is_word_char(ch) {
                break;
            }
            let Some(next) = self.next_visual_pos(pos) else {
                return;
            };
            pos = next;
        }

        // Step 3: advance through word characters to the end
        while let Some(next) = self.next_visual_pos(pos) {
            let Some(ch) = self.char_at_visual_pos(next) else {
                break;
            };
            if !Self::is_word_char(ch) {
                break;
            }
            pos = next;
        }

        self.set_visual_cursor_and_scroll(pos);
    }

    fn move_visual_cursor_line_end(&mut self) {
        let Some((row, _)) = self.current_visual_cursor() else {
            return;
        };
        let len = self.board.line_char_count(row);
        let new_col = if len == 0 { 0 } else { len - 1 };
        self.set_visual_cursor_and_scroll((row, new_col));
    }

    fn move_visual_cursor_line_first_non_blank(&mut self) {
        let Some((row, _)) = self.current_visual_cursor() else {
            return;
        };
        let new_col = self
            .board
            .get_line(row)
            .and_then(|line| line.chars().position(|ch| !ch.is_whitespace()))
            .unwrap_or(0);
        self.set_visual_cursor_and_scroll((row, new_col));
    }

    fn is_blank_row(&self, row: usize) -> bool {
        match self.board.get_line(row) {
            Some(line) => line.chars().all(|ch| ch.is_whitespace()),
            None => true,
        }
    }

    fn move_visual_cursor_paragraph_forward(&mut self) {
        let Some((row, _)) = self.current_visual_cursor() else {
            return;
        };
        let total = self.board.total_lines();
        if total == 0 {
            return;
        }

        let mut r = row;
        // Advance past current non-blank run until we hit a blank line.
        while r < total && !self.is_blank_row(r) {
            r += 1;
        }
        // Advance past blank lines until we hit a non-blank line.
        while r < total && self.is_blank_row(r) {
            r += 1;
        }
        if r >= total {
            // No further paragraph: snap to last line.
            r = total - 1;
        }
        self.set_visual_cursor_and_scroll((r, 0));
    }

    fn move_visual_cursor_paragraph_backward(&mut self) {
        let Some((row, _)) = self.current_visual_cursor() else {
            return;
        };
        if row == 0 {
            self.set_visual_cursor_and_scroll((0, 0));
            return;
        }

        let mut r = row - 1;
        // Move up through blank lines above the current paragraph.
        while r > 0 && self.is_blank_row(r) {
            r -= 1;
        }
        // Move up through the previous paragraph's text.
        while r > 0 && !self.is_blank_row(r - 1) {
            r -= 1;
        }
        // r is now the first non-blank row of the previous paragraph (or 0).
        if self.is_blank_row(r) {
            // We landed on a blank line because the whole prefix was blank.
            r = 0;
        }
        self.set_visual_cursor_and_scroll((r, 0));
    }

    /// Vim-style `f<char>` / `F<char>` motion: move the visual cursor to the
    /// next or previous occurrence of `target` on the current line. Repeats
    /// `repeat_count` times; stops where it is if a step has no match.
    fn move_visual_cursor_find_char(
        &mut self,
        target: char,
        dir: VisualFindDirection,
        repeat_count: u32,
    ) {
        let Some((row, mut col)) = self.current_visual_cursor() else {
            return;
        };
        let Some(line) = self.board.get_line(row) else {
            return;
        };
        let chars: Vec<char> = line.chars().collect();
        let mut moved = false;

        for iter in 0..repeat_count.max(1) {
            // For repeated `t`/`T`, after the first hit the cursor sits one
            // position away from the previous target — skip past it so we
            // don't immediately re-find the same char. On the first iteration
            // we want a normal scan (col+1 / col), or `2tu` from just before a
            // `u` would miss the adjacent target.
            let repeating_till = iter > 0;
            let scan_from_forward = match dir {
                VisualFindDirection::TillForward if repeating_till => col + 2,
                _ => col + 1,
            };
            let scan_to_backward = match dir {
                VisualFindDirection::TillBackward if repeating_till => col.saturating_sub(1),
                _ => col,
            };
            let hit = match dir {
                VisualFindDirection::Forward | VisualFindDirection::TillForward => chars
                    .iter()
                    .enumerate()
                    .skip(scan_from_forward)
                    .find(|(_, c)| **c == target)
                    .map(|(i, _)| i),
                VisualFindDirection::Backward | VisualFindDirection::TillBackward => chars
                    .iter()
                    .enumerate()
                    .take(scan_to_backward)
                    .rev()
                    .find(|(_, c)| **c == target)
                    .map(|(i, _)| i),
            };
            let Some(i) = hit else { break };
            let new_col = match dir {
                VisualFindDirection::Forward | VisualFindDirection::Backward => i,
                VisualFindDirection::TillForward => i.saturating_sub(1),
                VisualFindDirection::TillBackward => i + 1,
            };
            col = new_col;
            moved = true;
        }

        if moved {
            self.set_visual_cursor_and_scroll((row, col));
        }
    }

    fn current_visual_cursor(&self) -> Option<(usize, usize)> {
        let state = self.state.borrow();
        state.ui_state.visual_cursor
    }

    fn set_visual_cursor_and_scroll(&mut self, pos: (usize, usize)) {
        let (row, col) = pos;
        let page_size = self.page_size();
        let mut state = self.state.borrow_mut();
        state.ui_state.visual_cursor = Some((row, col));

        let viewport_start = state.reading_state.row.saturating_sub(1);
        let viewport_end = viewport_start.saturating_add(page_size);
        if row < viewport_start {
            state.reading_state.row = Self::row_from_start(row);
        } else if row >= viewport_end {
            let new_start = row.saturating_sub(page_size.saturating_sub(1));
            state.reading_state.row = Self::row_from_start(new_start);
        }
    }

    fn next_visual_pos(&self, pos: (usize, usize)) -> Option<(usize, usize)> {
        let (row, col) = pos;
        let total_lines = self.board.total_lines();
        if row >= total_lines {
            return None;
        }

        let line_len = self.board.line_char_count(row);
        if line_len > 0 && col + 1 < line_len {
            return Some((row, col + 1));
        }
        if row + 1 < total_lines {
            return Some((row + 1, 0));
        }
        None
    }

    fn prev_visual_pos(&self, pos: (usize, usize)) -> Option<(usize, usize)> {
        let (row, col) = pos;
        let total_lines = self.board.total_lines();
        if row >= total_lines || (row == 0 && col == 0) {
            return None;
        }

        let line_len = self.board.line_char_count(row);
        if line_len > 0 && col > 0 {
            return Some((row, col - 1));
        }

        let prev_row = row.saturating_sub(1);
        let prev_len = self.board.line_char_count(prev_row);
        if prev_len == 0 {
            Some((prev_row, 0))
        } else {
            Some((prev_row, prev_len - 1))
        }
    }

    fn char_at_visual_pos(&self, pos: (usize, usize)) -> Option<char> {
        let (row, col) = pos;
        self.board.get_line(row)?.chars().nth(col)
    }

    fn is_word_char(ch: char) -> bool {
        ch.is_alphanumeric() || ch == '_'
    }

    fn next_chapter(&mut self) {
        let rows = self.chapter_rows();
        if rows.is_empty() {
            return;
        }
        let current_row = self.state.borrow().reading_state.row;
        let index = Self::current_chapter_index(&rows, current_row);
        if index + 1 < rows.len() {
            self.record_jump_position();
            let mut state = self.state.borrow_mut();
            state.reading_state.row = rows[index + 1];
        }
    }

    fn previous_chapter(&mut self) {
        let rows = self.chapter_rows();
        if rows.is_empty() {
            return;
        }
        let current_row = self.state.borrow().reading_state.row;
        let index = Self::current_chapter_index(&rows, current_row);
        if index > 0 {
            self.record_jump_position();
            let mut state = self.state.borrow_mut();
            state.reading_state.row = rows[index - 1];
        }
    }

    fn goto_start(&mut self) {
        self.record_jump_position();
        let mut state = self.state.borrow_mut();
        state.reading_state.row = 0;
    }

    fn goto_chapter_start(&mut self) {
        let rows = self.chapter_rows();
        if rows.is_empty() {
            self.goto_start();
            return;
        }
        let current_row = self.state.borrow().reading_state.row;
        let index = Self::current_chapter_index(&rows, current_row);
        self.record_jump_position();
        let mut state = self.state.borrow_mut();
        state.reading_state.row = rows[index];
    }

    fn goto_chapter_end(&mut self) {
        let rows = self.chapter_rows();
        if rows.is_empty() {
            self.goto_end();
            return;
        }
        let current_row = self.state.borrow().reading_state.row;
        let index = Self::current_chapter_index(&rows, current_row);
        let total_lines = self.board.total_lines();
        let page = self.page_size();

        // Find the actual last content line by skipping chapter break padding
        let next_chapter_start = if index + 1 < rows.len() {
            rows[index + 1]
        } else {
            total_lines
        };

        let chapter_end = self.find_chapter_end(rows[index], next_chapter_start);

        // Position like page-down: show last content line at bottom of screen
        let last_start = chapter_end
            .saturating_sub(page.saturating_sub(1))
            .max(rows[index]);

        self.record_jump_position();
        let mut state = self.state.borrow_mut();
        state.reading_state.row = Self::row_from_start(last_start);
    }

    fn goto_end(&mut self) {
        let total_lines = self.board.total_lines();
        self.record_jump_position();
        let mut state = self.state.borrow_mut();
        if total_lines > 0 {
            state.reading_state.row = total_lines - 1;
        }
    }

    /// Find the actual last content line of a chapter by searching backwards
    /// from the next chapter start, stopping at the chapter break marker.
    /// Includes empty padding lines to match the page-down behavior.
    fn find_chapter_end(&self, chapter_start: usize, next_chapter_start: usize) -> usize {
        use crate::models::CHAPTER_BREAK_MARKER;

        // If next chapter starts immediately after current one, there's no padding
        if next_chapter_start <= chapter_start {
            return chapter_start;
        }

        // Search backwards from the line before next chapter starts
        let mut row = next_chapter_start.saturating_sub(1);
        let mut last_content_row = None;

        while row > chapter_start {
            if let Some(line) = self.board.get_line(row) {
                // If we hit actual content, this is the end
                if !line.is_empty() && line != CHAPTER_BREAK_MARKER {
                    return row;
                }
                // Remember the last non-empty line (could be chapter break marker)
                if !line.is_empty() {
                    last_content_row = Some(row);
                }
            }
            row = row.saturating_sub(1);
        }

        // If we found a chapter break marker or other non-empty line, return it
        // Otherwise return the line before next chapter start (including padding)
        last_content_row.unwrap_or_else(|| next_chapter_start.saturating_sub(1))
    }

    /// Pure page-size calculation; callers that already hold a borrow on `state`
    /// should call this directly to avoid a RefCell double-borrow panic.
    fn page_size_for(show_top_bar: bool) -> usize {
        match crossterm::terminal::size() {
            Ok((_cols, rows)) => {
                let chrome: u16 = if show_top_bar {
                    1 + 2 + 2 // top_bar + top_gap + bottom_gap
                } else {
                    2 // bottom_gap only
                };
                rows.saturating_sub(chrome) as usize
            }
            Err(_) => 0,
        }
    }

    fn page_size(&self) -> usize {
        let show_top_bar = self.state.borrow().config.settings.show_top_bar;
        Self::page_size_for(show_top_bar)
    }

    fn chapter_break_page_height(&self) -> Option<usize> {
        let state = self.state.borrow();
        if state.config.settings.seamless_between_chapters {
            None
        } else {
            Some(self.page_size())
        }
    }

    fn visible_line_range(&self) -> (usize, usize) {
        let height = self.page_size();
        let start = self.state.borrow().reading_state.row.saturating_sub(1);
        let end = start.saturating_add(height).min(self.board.total_lines());
        (start, end)
    }

    fn content_index_for_row(&self, row: usize) -> Option<usize> {
        if self.content_start_rows.is_empty() {
            return None;
        }
        let mut index = 0;
        for (i, start) in self.content_start_rows.iter().enumerate() {
            if *start <= row {
                index = i;
            } else {
                break;
            }
        }
        Some(index)
    }

    fn chapter_bounds_for_index(&self, index: usize) -> Option<(usize, usize)> {
        let start = *self.content_start_rows.get(index)?;
        let end = if index + 1 < self.content_start_rows.len() {
            self.content_start_rows[index + 1].saturating_sub(1)
        } else {
            self.board.total_lines().saturating_sub(1)
        };
        Some((start, end))
    }

    fn row_from_start(start_line: usize) -> usize {
        if start_line == 0 { 0 } else { start_line + 1 }
    }

    fn chapter_index_for_start_row(content_start_rows: &[usize], row: usize) -> Option<usize> {
        if content_start_rows.is_empty() {
            return None;
        }

        let mut index = 0;
        for (i, start) in content_start_rows.iter().enumerate() {
            if *start <= row {
                index = i;
            } else {
                break;
            }
        }
        Some(index)
    }

    fn tts_target_row_for_chunk(
        current_row: usize,
        first_line: usize,
        last_line: usize,
        page_height: usize,
        seamless_between_chapters: bool,
        content_start_rows: &[usize],
    ) -> usize {
        let current_top = current_row.saturating_sub(1);

        if !seamless_between_chapters
            && let (Some(current_chapter), Some(chunk_chapter)) = (
                Self::chapter_index_for_start_row(content_start_rows, current_top),
                Self::chapter_index_for_start_row(content_start_rows, first_line),
            )
            && chunk_chapter != current_chapter
            && let Some(&chapter_start) = content_start_rows.get(chunk_chapter)
        {
            return Self::row_from_start(chapter_start);
        }

        let current_bottom = current_top.saturating_add(page_height);
        if first_line >= current_top && last_line < current_bottom {
            return current_row;
        }

        let top_to_show_bottom = (last_line + 2).saturating_sub(page_height);
        let new_top = top_to_show_bottom.max(current_top).min(first_line);
        Self::row_from_start(new_top)
    }

    /// Resolve the effective jump target row for a TOC entry.
    ///
    /// EPUBs produced by tools like Calibre often place a section anchor at the very
    /// END of the preceding chapter file (as a visual divider / forward pointer), while
    /// the real chapter content starts in the NEXT file.  We detect this by counting
    /// how many non-empty, non-break-marker lines remain from the resolved anchor row
    /// to the end of its chapter.  If ≤ 2 such lines remain the anchor is treated as a
    /// forward pointer and we return the start of the next chapter instead.
    ///
    /// Returns a raw 0-indexed row (suitable for passing to `row_from_start`), or
    /// `None` when no position can be determined.
    fn effective_toc_row(&self, content_index: usize, section_id: Option<&str>) -> Option<usize> {
        if let Some(section_id) = section_id
            && let Some(ts) = self.chapter_text_structures.get(content_index)
            && let Some(&section_row) = ts.section_rows.get(section_id)
        {
            let ch_start = self
                .content_start_rows
                .get(content_index)
                .copied()
                .unwrap_or(0);
            let local_row = section_row.saturating_sub(ch_start);
            let meaningful_remaining = ts
                .text_lines
                .get(local_row..)
                .map(|lines| {
                    lines
                        .iter()
                        .filter(|l| !l.is_empty() && l.as_str() != CHAPTER_BREAK_MARKER)
                        .count()
                })
                .unwrap_or(0);

            if meaningful_remaining <= 2 {
                // Forward anchor: the real chapter starts in the next file.
                if let Some(&next_start) = self.content_start_rows.get(content_index + 1) {
                    return Some(next_start);
                }
            }
            return Some(section_row);
        }
        // No section anchor (or lookup failed) – fall back to chapter start.
        self.content_start_rows.get(content_index).copied()
    }

    fn toc_activation_row(&self, toc_entries: &[TocEntry], index: usize) -> Option<usize> {
        let entry = toc_entries.get(index)?;
        let row = self.effective_toc_row(entry.content_index, entry.section.as_deref())?;
        if index == 0 {
            return Some(row);
        }

        let first_entry_for_content = toc_entries[..index]
            .iter()
            .all(|prev| prev.content_index != entry.content_index);

        if first_entry_for_content
            && let Some((content_start, content_end)) =
                self.chapter_bounds_for_index(entry.content_index)
            && (content_start..=content_end).contains(&row)
        {
            return Some(content_start);
        }

        Some(row)
    }

    fn chapter_rows(&self) -> Vec<usize> {
        let state = self.state.borrow();
        let mut rows = Vec::new();
        for entry in &state.ui_state.toc_entries {
            if let Some(row) = self.effective_toc_row(entry.content_index, entry.section.as_deref())
            {
                rows.push(row);
            }
        }
        rows.sort_unstable();
        rows.dedup();
        rows
    }

    fn current_chapter_index(rows: &[usize], current_row: usize) -> usize {
        let mut index = 0;
        for (i, row) in rows.iter().enumerate() {
            if *row <= current_row {
                index = i;
            } else {
                break;
            }
        }
        index
    }

    fn execute_search(&mut self) {
        let query = {
            let state = self.state.borrow();
            state.ui_state.search_query.trim().to_string()
        };

        if query.is_empty() {
            let mut state = self.state.borrow_mut();
            state
                .ui_state
                .set_message("Search query is empty".to_string(), MessageType::Warning);
            return;
        }

        let regex = match Regex::new(&query) {
            Ok(regex) => regex,
            Err(err) => {
                let mut state = self.state.borrow_mut();
                state
                    .ui_state
                    .set_message(format!("Invalid regex: {}", err), MessageType::Error);
                return;
            }
        };

        let (results, matches_map) = self.scan_search_matches(&regex);

        let mut state = self.state.borrow_mut();
        state.ui_state.search_results = results;
        state.ui_state.search_matches = matches_map;
        // Start from the first match at or after the pre-search position.
        let origin = state.ui_state.search_origin_row;
        let selected = state
            .ui_state
            .search_results
            .iter()
            .position(|result| result.line >= origin)
            .unwrap_or(0);
        state.ui_state.selected_search_result = selected;

        let line = state.ui_state.search_results.get(selected).map(|r| r.line);
        if let Some(line) = line {
            state.reading_state.row = line;
            let total = state.ui_state.search_results.len();
            state.ui_state.set_message(
                format!("Match {}/{}", selected + 1, total),
                MessageType::Info,
            );
        } else {
            state
                .ui_state
                .set_message("No matches found".to_string(), MessageType::Info);
        }
    }

    /// Scan all loaded lines for `regex`, returning results and per-line
    /// byte-range matches.
    fn scan_search_matches(
        &self,
        regex: &Regex,
    ) -> (Vec<SearchResult>, HashMap<usize, Vec<(usize, usize)>>) {
        let mut results = Vec::new();
        let mut matches_map: HashMap<usize, Vec<(usize, usize)>> = HashMap::new();
        if let Some(lines) = self.board.lines() {
            for (line_index, line) in lines.iter().enumerate() {
                let ranges: Vec<(usize, usize)> = regex
                    .find_iter(line)
                    .map(|mat| (mat.start(), mat.end()))
                    .collect();
                if !ranges.is_empty() {
                    results.push(SearchResult {
                        line: line_index,
                        ranges: ranges.clone(),
                        preview: line.trim().to_string(),
                    });
                    matches_map.insert(line_index, ranges);
                }
            }
        }
        (results, matches_map)
    }

    /// Re-run the search as the query is typed. Invalid (possibly partial)
    /// regexes clear the matches without an error message; an empty query
    /// restores the pre-search view.
    fn update_incremental_search(&mut self) {
        let query = {
            let state = self.state.borrow();
            state.ui_state.search_query.trim().to_string()
        };

        if query.is_empty() {
            let mut state = self.state.borrow_mut();
            state.ui_state.search_results.clear();
            state.ui_state.search_matches.clear();
            state.reading_state.row = state.ui_state.search_origin_row;
            return;
        }

        let Ok(regex) = Regex::new(&query) else {
            let mut state = self.state.borrow_mut();
            state.ui_state.search_results.clear();
            state.ui_state.search_matches.clear();
            return;
        };

        let (results, matches_map) = self.scan_search_matches(&regex);
        let mut state = self.state.borrow_mut();
        state.ui_state.search_results = results;
        state.ui_state.search_matches = matches_map;
        let origin = state.ui_state.search_origin_row;
        let selected = state
            .ui_state
            .search_results
            .iter()
            .position(|result| result.line >= origin)
            .unwrap_or(0);
        state.ui_state.selected_search_result = selected;
        // Preview the first match; Esc restores the original position.
        if let Some(result) = state.ui_state.search_results.get(selected) {
            state.reading_state.row = result.line;
        } else {
            state.reading_state.row = origin;
        }
    }

    /// First Enter in the search prompt: record the pre-search position in
    /// the jump list, run the full search, and persist the query to history.
    fn commit_search(&mut self) {
        {
            // Jump history should point back to where the search started,
            // not to the incrementally previewed match.
            let mut state = self.state.borrow_mut();
            state.reading_state.row = state.ui_state.search_origin_row;
        }
        self.record_jump_position();
        self.execute_search();

        let query = {
            let state = self.state.borrow();
            if state.ui_state.search_results.is_empty() {
                None
            } else {
                Some(state.ui_state.search_query.trim().to_string())
            }
        };
        if let Some(query) = query {
            if let Err(err) = self.db_state.add_search_history(&query) {
                logging::warn(format!("Could not save search history: {}", err));
            }
            self.state.borrow_mut().ui_state.search_committed = true;
        }
    }

    fn search_next(&mut self) {
        let mut state = self.state.borrow_mut();
        if state.ui_state.search_results.is_empty() {
            state
                .ui_state
                .set_message("No search results".to_string(), MessageType::Info);
            return;
        }
        let next =
            (state.ui_state.selected_search_result + 1) % state.ui_state.search_results.len();
        state.ui_state.selected_search_result = next;
        let total = state.ui_state.search_results.len();
        state
            .ui_state
            .set_message(format!("Match {}/{}", next + 1, total), MessageType::Info);
        let line = state.ui_state.search_results.get(next).map(|r| r.line);
        if let Some(line) = line {
            state.reading_state.row = line;
        }
    }

    fn search_previous(&mut self) {
        let mut state = self.state.borrow_mut();
        if state.ui_state.search_results.is_empty() {
            state
                .ui_state
                .set_message("No search results".to_string(), MessageType::Info);
            return;
        }
        let len = state.ui_state.search_results.len();
        let prev = if state.ui_state.selected_search_result == 0 {
            len - 1
        } else {
            state.ui_state.selected_search_result - 1
        };
        state.ui_state.selected_search_result = prev;
        state
            .ui_state
            .set_message(format!("Match {}/{}", prev + 1, len), MessageType::Info);
        let line = state.ui_state.search_results.get(prev).map(|r| r.line);
        if let Some(line) = line {
            state.reading_state.row = line;
        }
    }

    /// Build the regex used by visual-mode `/`-search. Spaces in the query are
    /// matched against any whitespace run (including the `\n` we insert between
    /// wrapped lines), and smartcase makes the match case-insensitive unless
    /// the query already contains an uppercase character.
    fn build_visual_search_regex(query: &str) -> Result<Regex, regex::Error> {
        // Build the pattern character by character so we can handle two things
        // that get inserted by the wrapper between query characters:
        //   * spaces — widened to `\s+` so they span newlines between wrapped
        //     lines;
        //   * soft hyphens — the wrapper writes `-\n` when it splits a word at
        //     the end of a line (see `parser.rs` around the
        //     "Handle hyphenation" block). Allowing an optional `(?:-\n)?` at
        //     every interior position means `/example` matches `exam-\nple`
        //     across the wrap without matching plain "auto-mate" on one line.
        let chars: Vec<char> = query.chars().collect();
        let mut pattern = String::new();
        for (idx, ch) in chars.iter().enumerate() {
            if *ch == ' ' {
                pattern.push_str(r"\s+");
            } else {
                pattern.push_str(&regex::escape(&ch.to_string()));
            }
            let is_last = idx + 1 == chars.len();
            let next_is_space = chars.get(idx + 1).map(|c| *c == ' ').unwrap_or(false);
            if !is_last && *ch != ' ' && !next_is_space {
                pattern.push_str(r"(?:-\n)?");
            }
        }

        let has_upper = query.chars().any(|c| c.is_uppercase());
        if has_upper {
            Regex::new(&pattern)
        } else {
            Regex::new(&format!("(?i){}", pattern))
        }
    }

    /// Search the visible viewport for `query`, populating
    /// `ui_state.visual_search_matches` and moving `visual_cursor` to the first
    /// match at or after the current cursor position. Matches are returned in
    /// absolute (line, char-col) coordinates.
    fn execute_visual_search(&mut self) {
        let query = self.state.borrow().ui_state.visual_search_query.clone();
        if query.is_empty() {
            let mut state = self.state.borrow_mut();
            state
                .ui_state
                .set_message("Search query is empty".to_string(), MessageType::Warning);
            return;
        }

        let re = match Self::build_visual_search_regex(&query) {
            Ok(re) => re,
            Err(err) => {
                let mut state = self.state.borrow_mut();
                state
                    .ui_state
                    .set_message(format!("Invalid pattern: {}", err), MessageType::Error);
                return;
            }
        };

        let page_size = self.page_size();
        let start_line = self.state.borrow().reading_state.row.saturating_sub(1);
        let total = self.board.total_lines();
        let end_line = (start_line + page_size).min(total);

        let Some(all_lines) = self.board.lines() else {
            return;
        };
        let visible = &all_lines[start_line..end_line];
        let haystack = visible.join("\n");

        // Walk the haystack once, mapping byte offsets -> (line, char_col).
        // We build a sorted list of (byte_offset, line_idx, char_col) snapshots
        // at every char boundary AND right after each '\n'.
        let mut snapshots: Vec<(usize, usize, usize)> = Vec::with_capacity(haystack.len() + 1);
        let mut line_idx = 0usize;
        let mut char_col = 0usize;
        for (byte, ch) in haystack.char_indices() {
            snapshots.push((byte, line_idx, char_col));
            if ch == '\n' {
                line_idx += 1;
                char_col = 0;
            } else {
                char_col += 1;
            }
        }
        snapshots.push((haystack.len(), line_idx, char_col));

        let pos_at = |byte_idx: usize| -> (usize, usize) {
            // snapshots is monotonically increasing in byte; binary_search by key
            let idx = snapshots
                .binary_search_by_key(&byte_idx, |(b, _, _)| *b)
                .unwrap_or_else(|i| i.saturating_sub(1));
            let (_, line, col) = snapshots[idx];
            (line, col)
        };

        let mut matches: Vec<(usize, usize, usize, usize)> = Vec::new();
        for mat in re.find_iter(&haystack) {
            if mat.start() == mat.end() {
                continue;
            }
            let (s_line, s_col) = pos_at(mat.start());
            // For the end position, we want the (line, col) of the byte *after*
            // the last matched char so that it represents an exclusive end.
            let (e_line, e_col) = pos_at(mat.end());
            matches.push((start_line + s_line, s_col, start_line + e_line, e_col));
        }

        if matches.is_empty() {
            let mut state = self.state.borrow_mut();
            state.ui_state.visual_search_matches.clear();
            state.ui_state.visual_search_selected = 0;
            state
                .ui_state
                .set_message("Pattern not found".to_string(), MessageType::Warning);
            return;
        }

        let cursor = self
            .state
            .borrow()
            .ui_state
            .visual_cursor
            .unwrap_or((start_line, 0));

        // Pick the first match starting at or after the current cursor; wrap to 0.
        let selected = matches
            .iter()
            .position(|(sl, sc, _, _)| (*sl, *sc) >= cursor)
            .unwrap_or(0);

        let (sl, sc, _, _) = matches[selected];
        {
            let mut state = self.state.borrow_mut();
            state.ui_state.visual_search_matches = matches;
            state.ui_state.visual_search_selected = selected;
        }
        self.set_visual_cursor_and_scroll((sl, sc));
    }

    /// Drop the current visual-mode `/`-search matches so the yellow highlights
    /// disappear after the user takes an action on the match (yank, dictionary,
    /// wikipedia, highlight, etc.). The cursor itself is left alone so the
    /// follow-up action keeps the position it just jumped to.
    fn clear_visual_search_state(&mut self) {
        let mut state = self.state.borrow_mut();
        state.ui_state.visual_search_matches.clear();
        state.ui_state.visual_search_selected = 0;
    }

    /// Move the visual cursor to the next or previous match in
    /// `visual_search_matches`, wrapping around.
    fn visual_search_step(&mut self, forward: bool) {
        let (target, message) = {
            let state = self.state.borrow();
            let matches = &state.ui_state.visual_search_matches;
            if matches.is_empty() {
                (None, Some("No matches".to_string()))
            } else {
                let len = matches.len();
                let cur = state.ui_state.visual_search_selected;
                let next = if forward {
                    (cur + 1) % len
                } else {
                    (cur + len - 1) % len
                };
                let (sl, sc, _, _) = matches[next];
                (Some((next, sl, sc)), None)
            }
        };
        if let Some(msg) = message {
            self.state
                .borrow_mut()
                .ui_state
                .set_message(msg, MessageType::Info);
            return;
        }
        if let Some((next, sl, sc)) = target {
            self.state.borrow_mut().ui_state.visual_search_selected = next;
            self.set_visual_cursor_and_scroll((sl, sc));
        }
    }

    fn jump_to_selected_search_result(&mut self) {
        let row = {
            let state = self.state.borrow();
            state
                .ui_state
                .search_results
                .get(state.ui_state.selected_search_result)
                .map(|result| result.line)
        };
        if let Some(row) = row {
            self.record_jump_position();
            let mut state = self.state.borrow_mut();
            state.reading_state.row = row;
            state.ui_state.open_window(WindowType::Reader);
        }
    }

    fn jump_to_toc_entry(&mut self) -> eyre::Result<()> {
        let (section, content_index) = {
            let state = self.state.borrow();
            if let Some(entry) = state
                .ui_state
                .selected_list_index(state.ui_state.toc_selected_index)
                .and_then(|i| state.ui_state.toc_entries.get(i))
            {
                (entry.section.clone(), entry.content_index)
            } else {
                return Ok(());
            }
        };

        let target_row = self
            .effective_toc_row(content_index, section.as_deref())
            .map(Self::row_from_start);

        if let Some(row) = target_row {
            self.record_jump_position();
            let mut state = self.state.borrow_mut();
            state.reading_state.row = row;
            if content_index < self.content_start_rows.len() {
                state.reading_state.content_index = content_index;
            }
            state.ui_state.open_window(WindowType::Reader);
        } else {
            let mut state = self.state.borrow_mut();
            state.ui_state.set_message(
                "TOC entry not mapped to text".to_string(),
                MessageType::Warning,
            );
        }
        Ok(())
    }

    fn add_bookmark(&mut self) -> eyre::Result<()> {
        let Some(epub) = self.ebook.as_ref() else {
            let mut state = self.state.borrow_mut();
            state
                .ui_state
                .set_message("No book loaded".to_string(), MessageType::Warning);
            return Ok(());
        };
        let bookmark_name = {
            let state = self.state.borrow();
            format!("Bookmark {}", state.ui_state.bookmarks.len() + 1)
        };
        let reading_state = { self.state.borrow().reading_state.clone() };
        self.db_state
            .insert_bookmark(epub, &bookmark_name, &reading_state)?;
        self.refresh_bookmarks()?;
        Ok(())
    }

    fn delete_selected_bookmark(&mut self) -> eyre::Result<()> {
        let Some(epub) = self.ebook.as_ref() else {
            return Ok(());
        };
        let bookmark_name = {
            let state = self.state.borrow();
            state
                .ui_state
                .selected_list_index(state.ui_state.bookmarks_selected_index)
                .and_then(|i| state.ui_state.bookmarks.get(i))
                .map(|(name, _)| name.clone())
        };
        if let Some(name) = bookmark_name {
            self.db_state.delete_bookmark(epub, &name)?;
            self.refresh_bookmarks()?;
        }
        Ok(())
    }

    fn refresh_bookmarks(&mut self) -> eyre::Result<()> {
        if let Some(epub) = self.ebook.as_ref() {
            let bookmarks = self.db_state.get_bookmarks(epub)?;
            let mut state = self.state.borrow_mut();
            state.ui_state.bookmarks = bookmarks;
            if state.ui_state.bookmarks_selected_index >= state.ui_state.bookmarks.len() {
                state.ui_state.bookmarks_selected_index =
                    state.ui_state.bookmarks.len().saturating_sub(1);
            }
        }
        Ok(())
    }

    fn jump_to_selected_bookmark(&mut self) -> eyre::Result<()> {
        let row = {
            let state = self.state.borrow();
            state
                .ui_state
                .selected_list_index(state.ui_state.bookmarks_selected_index)
                .and_then(|i| state.ui_state.bookmarks.get(i))
                .map(|(_, reading_state)| reading_state.row)
        };
        if let Some(row) = row {
            self.record_jump_position();
            let mut state = self.state.borrow_mut();
            state.reading_state.row = row;
            state.ui_state.open_window(WindowType::Reader);
        }
        Ok(())
    }

    fn delete_selected_library_item(&mut self) -> eyre::Result<()> {
        let filepath = {
            let state = self.state.borrow();
            state
                .ui_state
                .selected_list_index(state.ui_state.library_selected_index)
                .and_then(|i| state.ui_state.library_items.get(i))
                .map(|item| item.filepath.clone())
        };
        if let Some(path) = filepath {
            self.db_state.delete_from_library(&path)?;
            let library_items = self.db_state.get_from_history()?;
            let mut state = self.state.borrow_mut();
            state.ui_state.library_items = library_items;
            if state.ui_state.library_selected_index >= state.ui_state.library_items.len() {
                state.ui_state.library_selected_index =
                    state.ui_state.library_items.len().saturating_sub(1);
            }
        }
        Ok(())
    }

    fn open_selected_library_item(&mut self) -> eyre::Result<()> {
        let filepath = {
            let state = self.state.borrow();
            state
                .ui_state
                .selected_list_index(state.ui_state.library_selected_index)
                .and_then(|i| state.ui_state.library_items.get(i))
                .map(|item| item.filepath.clone())
        };
        if let Some(path) = filepath {
            if std::path::Path::new(&path).exists() {
                let already_open = self.ebook.as_ref().map_or(false, |e| e.path() == path);
                if already_open {
                    let mut state = self.state.borrow_mut();
                    state.ui_state.open_window(WindowType::Reader);
                    return Ok(());
                }
                self.load_ebook(&path)?;
                let mut state = self.state.borrow_mut();
                state.ui_state.open_window(WindowType::Reader);
            } else {
                let mut state = self.state.borrow_mut();
                state.ui_state.set_message(
                    "Selected file no longer exists".to_string(),
                    MessageType::Warning,
                );
            }
        }
        Ok(())
    }

    fn open_selected_image(&mut self) -> eyre::Result<()> {
        let image_src = {
            let state = self.state.borrow();
            state
                .ui_state
                .images_list
                .get(state.ui_state.images_selected_index)
                .map(|(_, src)| src.clone())
        };

        if let Some(src) = image_src
            && let Some(epub) = self.ebook.as_mut()
        {
            // Resolve relative path
            let current_index = self.state.borrow().reading_state.content_index;
            let base_path = epub.resource_path_for_content_index(current_index);
            let resolved_path = if let Some(base) = base_path {
                Self::resolve_relative_href(&src, Some(&base)).unwrap_or(src.clone())
            } else {
                src.clone()
            };

            match epub.get_img_bytestr(&resolved_path) {
                Ok((mime, bytes)) => {
                    // Create a temporary file with the correct extension
                    let extension = match mime.as_str() {
                        "image/jpeg" => "jpg",
                        "image/png" => "png",
                        "image/gif" => "gif",
                        "image/svg+xml" => "svg",
                        "image/webp" => "webp",
                        "image/bmp" => "bmp",
                        _ => "jpg", // Fallback
                    };

                    let temp_dir = std::env::temp_dir();
                    let filename = std::path::Path::new(&src)
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("image");
                    let temp_path =
                        temp_dir.join(format!("{}_{}.{}", "repy_img", filename, extension));

                    std::fs::write(&temp_path, bytes)?;

                    self.open_image_viewer(&temp_path.to_string_lossy())?;

                    let mut state = self.state.borrow_mut();
                    state
                        .ui_state
                        .set_message("Opened image".to_string(), MessageType::Info);
                    state.ui_state.open_window(WindowType::Reader);
                }
                Err(e) => {
                    let mut state = self.state.borrow_mut();
                    state
                        .ui_state
                        .set_message(format!("Failed to load image: {}", e), MessageType::Error);
                }
            }
        }
        Ok(())
    }

    fn open_image_viewer(&self, path: &str) -> eyre::Result<bool> {
        let config_viewer = {
            let state = self.state.borrow();
            state.config.settings.default_viewer.clone()
        };

        // Try feh first as requested, unless user configured something specific other than "auto"
        let viewers_to_try = if config_viewer == "auto" {
            vec!["feh", "xdg-open"]
        } else {
            vec![config_viewer.as_str(), "feh", "xdg-open"]
        };

        for viewer in viewers_to_try {
            let status = std::process::Command::new(viewer).arg(path).status();

            if let Ok(status) = status
                && status.success()
            {
                return Ok(true);
            }
        }

        Err(eyre::eyre!(
            "Failed to open image with any available viewer"
        ))
    }

    fn toggle_selected_setting(&mut self) -> eyre::Result<()> {
        let selected = {
            let state = self.state.borrow();
            SettingItem::all()
                .get(state.ui_state.settings_selected_index)
                .copied()
        };

        let Some(item) = selected else {
            return Ok(());
        };

        let mut state = self.state.borrow_mut();
        let mut rebuild_chapter_breaks = false;
        match item {
            SettingItem::ShowLineNumbers => {
                state.config.settings.show_line_numbers = !state.config.settings.show_line_numbers;
            }
            SettingItem::MouseSupport => {
                state.config.settings.mouse_support = !state.config.settings.mouse_support;
                // Apply immediately so the toggle works without a restart.
                if state.config.settings.mouse_support {
                    crossterm::execute!(io::stdout(), crossterm::event::EnableMouseCapture)?;
                } else {
                    crossterm::execute!(io::stdout(), crossterm::event::DisableMouseCapture)?;
                }
            }
            SettingItem::PageScrollAnimation => {
                state.config.settings.page_scroll_animation =
                    !state.config.settings.page_scroll_animation;
            }
            SettingItem::ShowProgressIndicator => {
                state.config.settings.show_progress_indicator =
                    !state.config.settings.show_progress_indicator;
            }
            SettingItem::SeamlessBetweenChapters => {
                state.config.settings.seamless_between_chapters =
                    !state.config.settings.seamless_between_chapters;
                rebuild_chapter_breaks = true;
            }
            SettingItem::DictionaryClient => {
                let current = if state.config.settings.dictionary_client.trim().is_empty() {
                    "auto"
                } else {
                    state.config.settings.dictionary_client.trim()
                };
                let options: Vec<&str> = std::iter::once("auto")
                    .chain(DICT_PRESET_LIST.iter().copied())
                    .collect();
                let current_index = options.iter().position(|v| *v == current).unwrap_or(0);
                let next_index = (current_index + 1) % options.len();
                state.config.settings.dictionary_client = options[next_index].to_string();
            }
            SettingItem::TtsEngine => {
                let current = state
                    .config
                    .settings
                    .preferred_tts_engine
                    .as_deref()
                    .unwrap_or("")
                    .to_string();
                let current_ref = if current.is_empty() { "purr" } else { &current };
                use crate::settings::TTS_PRESET_LIST;
                let options: Vec<&str> = TTS_PRESET_LIST.to_vec();
                let current_index = options.iter().position(|v| *v == current_ref).unwrap_or(0);
                let next_index = (current_index + 1) % options.len();
                state.config.settings.preferred_tts_engine = Some(options[next_index].to_string());
            }
            SettingItem::Width => {
                let textwidth = state.config.settings.width.unwrap_or(70);
                drop(state);
                self.rebuild_text_structure_with_textwidth(textwidth)?;
                self.persist_state()?;
                return Ok(());
            }
            SettingItem::ShowTopBar => {
                state.config.settings.show_top_bar = !state.config.settings.show_top_bar;
            }
            SettingItem::ColorTheme => {
                drop(state);
                self.cycle_color_theme()?;
                return Ok(());
            }
        }
        let _ = state.config.save();
        if rebuild_chapter_breaks {
            // Use current textwidth
            let textwidth = state.reading_state.textwidth;
            drop(state);
            self.rebuild_text_structure_with_textwidth(textwidth)?;
        }
        Ok(())
    }

    fn change_textwidth(&mut self, delta: i32) -> eyre::Result<()> {
        let current_textwidth = self.state.borrow().reading_state.textwidth as i32;
        let new_textwidth = (current_textwidth + delta).max(20); // Minimum 20 columns
        self.rebuild_text_structure_with_textwidth(new_textwidth as usize)?;
        self.persist_state()
    }

    fn reset_width(&mut self) -> eyre::Result<()> {
        // Reset to default textwidth of 70
        self.rebuild_text_structure_with_textwidth(70)?;
        self.persist_state()
    }

    fn adjust_textwidth(&mut self, delta: i32) -> eyre::Result<()> {
        let selected = {
            let state = self.state.borrow();
            SettingItem::all()
                .get(state.ui_state.settings_selected_index)
                .copied()
        };
        if selected != Some(SettingItem::Width) {
            return Ok(());
        }
        self.change_textwidth(delta)
    }

    fn reset_selected_setting(&mut self) -> eyre::Result<()> {
        let selected = {
            let state = self.state.borrow();
            SettingItem::all()
                .get(state.ui_state.settings_selected_index)
                .copied()
        };

        match selected {
            Some(SettingItem::DictionaryClient) => {
                let mut state = self.state.borrow_mut();
                state.config.settings.dictionary_client = "auto".to_string();
                let _ = state.config.save();
                state.ui_state.set_message(
                    "Dictionary client reset to auto".to_string(),
                    MessageType::Info,
                );
            }
            Some(SettingItem::Width) => {
                let textwidth = self.state.borrow().config.settings.width.unwrap_or(70);
                self.rebuild_text_structure_with_textwidth(textwidth)?;
                self.persist_state()?;
                self.state.borrow_mut().ui_state.set_message(
                    format!("Text width reset to {textwidth}"),
                    MessageType::Info,
                );
            }
            Some(SettingItem::ColorTheme) => {
                self.set_effective_color_theme(None)?;
                let theme_name = self.state.borrow().effective_color_theme().name();
                self.state
                    .borrow_mut()
                    .ui_state
                    .set_message(format!("Theme reset to {theme_name}"), MessageType::Info);
            }
            _ => {}
        }
        Ok(())
    }

    fn rebuild_text_structure(&mut self, padding: usize) -> eyre::Result<()> {
        // Calculate textwidth from padding (for backwards compatibility with resize handler)
        let term_width = match crossterm::terminal::size() {
            Ok((w, _)) => w as usize,
            Err(_) => 100,
        };
        let textwidth = term_width.saturating_sub(padding * 2).max(20);
        self.rebuild_text_structure_with_textwidth(textwidth)
    }

    fn rebuild_text_structure_with_textwidth(&mut self, textwidth: usize) -> eyre::Result<()> {
        // Capture current position semantically to restore it after rebuild
        let (current_chapter_idx, current_chapter_offset) = {
            let row = self.state.borrow().reading_state.row;
            if self.content_start_rows.is_empty() {
                (0, 0)
            } else {
                let idx = match self.content_start_rows.binary_search(&row) {
                    Ok(i) => i,
                    Err(i) => i.saturating_sub(1),
                };
                let start = self.content_start_rows[idx];
                (idx, row.saturating_sub(start))
            }
        };

        let term_width = match crossterm::terminal::size() {
            Ok((w, _)) => w as usize,
            Err(_) => 100,
        };

        // Calculate padding from textwidth (minimum 5 on each side, unless window ≤ 20)
        let padding = if term_width <= 20 {
            0
        } else {
            (term_width.saturating_sub(textwidth) / 2).max(5)
        };

        // Calculate actual text width for rendering
        let text_width = term_width
            .saturating_sub(padding * 2)
            .max(20)
            .min(term_width);

        // Collect page_height before any mutable borrows
        let page_height = self.chapter_break_page_height();

        let epub = match self.ebook.as_mut() {
            Some(epub) => epub,
            None => return Ok(()),
        };

        // Check if we need to rebuild or if width is the same
        let needs_rebuild = self.current_text_width != Some(text_width);

        if needs_rebuild {
            // Only re-parse the current chapter for performance
            let contents = epub.contents();
            let total_chapters = contents.len();

            if current_chapter_idx < self.chapter_text_structures.len()
                && current_chapter_idx < total_chapters
            {
                // Clone content_id to avoid holding immutable borrow across mutable call
                let content_id = contents[current_chapter_idx].clone();
                let starting_line = if current_chapter_idx > 0 {
                    self.content_start_rows[current_chapter_idx]
                } else {
                    0
                };

                // Parse only the current chapter with new width
                let mut parsed_chapter =
                    epub.get_parsed_content(&content_id, text_width, starting_line)?;

                // Add chapter break if needed
                if let Some(ph) = page_height
                    && current_chapter_idx + 1 < total_chapters
                {
                    let total_lines = starting_line + parsed_chapter.text_lines.len();
                    let break_lines = build_chapter_break(ph, total_lines);
                    parsed_chapter.text_lines.extend(break_lines);
                }

                // Update the cached structure for this chapter
                self.chapter_text_structures[current_chapter_idx] = parsed_chapter;
                self.current_text_width = Some(text_width);
            }
        }

        // Rebuild combined structure from cached chapter structures
        let mut combined_text_structure = TextStructure::default();
        let mut content_start_rows = Vec::with_capacity(self.chapter_text_structures.len());
        let mut row_offset = 0;
        for ts in &self.chapter_text_structures {
            content_start_rows.push(row_offset);
            row_offset += ts.text_lines.len();
            combined_text_structure
                .text_lines
                .extend(ts.text_lines.clone());
            combined_text_structure
                .image_maps
                .extend(ts.image_maps.clone());
            combined_text_structure
                .section_rows
                .extend(ts.section_rows.clone());
            combined_text_structure
                .formatting
                .extend(ts.formatting.clone());
            combined_text_structure.links.extend(ts.links.clone());
            combined_text_structure
                .pagebreak_map
                .extend(ts.pagebreak_map.clone());
        }
        self.board.update_text_structure(combined_text_structure);
        self.content_start_rows = content_start_rows;
        self.refresh_highlight_ranges()?;

        let mut state = self.state.borrow_mut();
        state.reading_state.textwidth = textwidth;

        // Restore position based on semantic location
        if !self.content_start_rows.is_empty() {
            let idx = current_chapter_idx.min(self.content_start_rows.len().saturating_sub(1));
            let start_row = self.content_start_rows[idx];

            // Calculate length of this chapter in new structure
            let chapter_len = if idx + 1 < self.content_start_rows.len() {
                self.content_start_rows[idx + 1] - start_row
            } else {
                self.board.total_lines() - start_row
            };

            let new_offset = current_chapter_offset.min(chapter_len.saturating_sub(1));
            state.reading_state.row = start_row + new_offset;
        }

        let total_lines = self.board.total_lines();
        if total_lines > 0 && state.reading_state.row >= total_lines {
            state.reading_state.row = total_lines - 1;
        }
        Ok(())
    }

    fn yank_selection(&mut self) -> eyre::Result<()> {
        let (anchor, cursor) = {
            let state = self.state.borrow();
            match (state.ui_state.visual_anchor, state.ui_state.visual_cursor) {
                (Some(anchor), Some(cursor)) => (anchor, cursor),
                _ => return Ok(()),
            }
        };

        let selected_text = self.board.get_selected_text_range(anchor, cursor);
        if !selected_text.is_empty() {
            let copied = self.set_clipboard_text(selected_text)?;
            let ui_state = &mut self.state.borrow_mut().ui_state;
            if copied {
                ui_state.set_message("Text copied to clipboard".to_string(), MessageType::Info);
            } else {
                ui_state.set_message("Clipboard unavailable".to_string(), MessageType::Warning);
            }
        }
        self.state
            .borrow_mut()
            .ui_state
            .open_window(WindowType::Reader);
        Ok(())
    }

    fn create_highlight_from_selection(&mut self, edit_comment: bool) -> eyre::Result<()> {
        let (anchor, cursor, book_identity) = {
            let state = self.state.borrow();
            let Some(book_identity) = state.ui_state.book_identity.clone() else {
                return Ok(());
            };
            match (state.ui_state.visual_anchor, state.ui_state.visual_cursor) {
                (Some(anchor), Some(cursor)) => (anchor, cursor, book_identity),
                _ => return Ok(()),
            }
        };
        let start_row = anchor.0.min(cursor.0);
        let end_row = anchor.0.max(cursor.0);
        let Some(start_index) = self.content_index_for_row(start_row) else {
            return Ok(());
        };
        let Some(end_index) = self.content_index_for_row(end_row) else {
            return Ok(());
        };
        if start_index != end_index {
            self.state.borrow_mut().ui_state.set_message(
                "Highlights cannot cross chapter boundaries yet".to_string(),
                MessageType::Warning,
            );
            return Ok(());
        }
        let Some(global_start_row) = self.content_start_rows.get(start_index).copied() else {
            return Ok(());
        };
        let Some((exact, prefix, suffix, approx_offset)) = annotations::anchor_from_selection(
            &self.chapter_text_structures[start_index].text_lines,
            global_start_row,
            anchor,
            cursor,
        ) else {
            return Ok(());
        };
        let now = chrono::Utc::now();
        let id = {
            use sha2::{Digest, Sha256};
            let mut hasher = Sha256::new();
            hasher.update(book_identity.book_id.as_bytes());
            hasher.update(start_index.to_string().as_bytes());
            hasher.update(approx_offset.to_string().as_bytes());
            hasher.update(exact.as_bytes());
            hasher.update(now.timestamp_micros().to_string().as_bytes());
            hex::encode(hasher.finalize())
        };
        let spine_href = self
            .ebook
            .as_ref()
            .and_then(|ebook| ebook.spine_href(start_index))
            .unwrap_or_else(|| start_index.to_string());
        let highlight = Highlight {
            id: id.clone(),
            book_id: book_identity.book_id,
            content_index: start_index,
            spine_href,
            exact,
            prefix,
            suffix,
            approx_offset,
            normalization_version: NORMALIZATION_VERSION,
            color: self
                .state
                .borrow()
                .ui_state
                .next_highlight_color
                .name()
                .to_string(),
            comment: None,
            comment_format: "plain".to_string(),
            created_at: now,
            updated_at: now,
            resolution_status: "resolved".to_string(),
        };
        self.db_state.insert_highlight(&highlight)?;
        self.refresh_highlights()?;
        {
            let mut state = self.state.borrow_mut();
            state.ui_state.visual_anchor = None;
            state.ui_state.visual_cursor = None;
            if edit_comment {
                state.ui_state.highlight_comment_buffer.clear();
                state.ui_state.highlight_comment_cursor = 0;
                state.ui_state.highlight_comment_editing_id = Some(id);
                state
                    .ui_state
                    .open_window(WindowType::HighlightCommentEditor);
            } else {
                state.ui_state.open_window(WindowType::Reader);
                state
                    .ui_state
                    .set_message("Highlight saved".to_string(), MessageType::Info);
            }
        }
        Ok(())
    }

    fn dictionary_lookup(&mut self) -> eyre::Result<()> {
        let (anchor, cursor) = {
            let state = self.state.borrow();
            match (state.ui_state.visual_anchor, state.ui_state.visual_cursor) {
                (Some(anchor), Some(cursor)) => (anchor, cursor),
                _ => return Ok(()),
            }
        };

        let selected_text = self.board.get_selected_text_range(anchor, cursor);
        let word = selected_text.trim().to_string();
        if word.is_empty() {
            self.state
                .borrow_mut()
                .ui_state
                .open_window(WindowType::Reader);
            return Ok(());
        }

        let dictionary_client = {
            let state = self.state.borrow();
            state.config.settings.dictionary_client.trim().to_string()
        };

        let (tx, rx) = std::sync::mpsc::channel();
        self.dictionary_res_rx = Some(rx);

        {
            let mut state = self.state.borrow_mut();
            state.ui_state.dictionary_word = word.clone();
            state.ui_state.dictionary_definition = String::new();
            state.ui_state.dictionary_loading = true;
            state.ui_state.dictionary_scroll_offset = 0;
            state.ui_state.dictionary_is_wikipedia = false;
            state.ui_state.visual_anchor = None;
            state.ui_state.visual_cursor = None;
            state.ui_state.open_window(WindowType::Dictionary);
        }

        let word_clone = word.clone();
        std::thread::spawn(move || {
            let start_total = Instant::now();
            let total_timeout = Duration::from_secs(10);

            let clients_to_try: Vec<String> =
                if dictionary_client.is_empty() || dictionary_client == "auto" {
                    DICT_PRESET_LIST.iter().map(|c| (*c).to_string()).collect()
                } else {
                    vec![dictionary_client]
                };

            let mut any_command_ran = false;
            let mut last_stderr: Option<String> = None;
            let mut definition: Option<String> = None;
            let mut successful_client: String = String::new();

            for client in clients_to_try {
                let remaining = total_timeout.saturating_sub(start_total.elapsed());
                if remaining.is_zero() {
                    break;
                }

                match Self::run_dictionary_client(&client, &word_clone, remaining) {
                    Ok(out) => {
                        any_command_ran = true;
                        let stdout_text = String::from_utf8_lossy(&out.stdout).trim().to_string();
                        let stderr_text = String::from_utf8_lossy(&out.stderr).trim().to_string();
                        if !stdout_text.is_empty() {
                            definition = Some(stdout_text);
                            successful_client = client;
                            break;
                        }
                        if !stderr_text.is_empty() {
                            last_stderr = Some(stderr_text);
                        }
                    }
                    Err(err) => {
                        last_stderr = Some(err.to_string());
                    }
                }
            }

            let result_definition = if let Some(text) = definition {
                Ok(text)
            } else if start_total.elapsed() >= total_timeout {
                Err(format!(
                    "Dictionary query timed out after {}s",
                    total_timeout.as_secs()
                ))
            } else if any_command_ran {
                Err(last_stderr
                    .unwrap_or_else(|| format!("No definition found for '{}'", word_clone)))
            } else {
                Err("No dictionary program found (install dict, sdcv, or wkdict)".to_string())
            };

            let _ = tx.send(DictionaryResult {
                word: word_clone,
                definition: result_definition,
                client: successful_client,
            });
        });

        Ok(())
    }

    fn wikipedia_lookup(&mut self) -> eyre::Result<()> {
        let (anchor, cursor) = {
            let state = self.state.borrow();
            match (state.ui_state.visual_anchor, state.ui_state.visual_cursor) {
                (Some(anchor), Some(cursor)) => (anchor, cursor),
                _ => return Ok(()),
            }
        };

        let selected_text = self.board.get_selected_text_range(anchor, cursor);
        let query = selected_text.trim().to_string();
        if query.is_empty() {
            self.state
                .borrow_mut()
                .ui_state
                .open_window(WindowType::Reader);
            return Ok(());
        }

        let (tx, rx) = std::sync::mpsc::channel();
        self.dictionary_res_rx = Some(rx);

        {
            let mut state = self.state.borrow_mut();
            state.ui_state.dictionary_word = query.clone();
            state.ui_state.dictionary_definition = String::new();
            state.ui_state.dictionary_loading = true;
            state.ui_state.dictionary_scroll_offset = 0;
            state.ui_state.dictionary_is_wikipedia = true;
            state.ui_state.visual_anchor = None;
            state.ui_state.visual_cursor = None;
            state.ui_state.open_window(WindowType::Dictionary);
        }

        std::thread::spawn(move || {
            let total_timeout = Duration::from_secs(10);
            let language = Self::detect_wikipedia_language(&query);
            let result_definition =
                match Self::wikipedia_lookup_summary(&query, &language, total_timeout) {
                    Ok(result) => Ok(format!("Wikipedia: {}\n\n{}", result.url, result.summary)),
                    Err(err) => {
                        let message = err.to_string();
                        if message.contains("timed out") {
                            Err(format!(
                                "Wikipedia query timed out after {}s",
                                total_timeout.as_secs()
                            ))
                        } else {
                            Err(format!("Wikipedia lookup failed.\n\n{}", message))
                        }
                    }
                };

            let _ = tx.send(DictionaryResult {
                word: query,
                definition: result_definition,
                client: "Wikipedia".to_string(),
            });
        });

        Ok(())
    }

    fn build_ecosia_search_url(query: &str) -> eyre::Result<String> {
        let normalized_query = query.split_whitespace().collect::<Vec<_>>().join(" ");
        let mut url = reqwest::Url::parse("https://www.ecosia.org/search")?;
        url.query_pairs_mut().append_pair("q", &normalized_query);
        Ok(url.to_string())
    }

    fn web_search_selection(&mut self) -> eyre::Result<()> {
        let (anchor, cursor) = {
            let state = self.state.borrow();
            match (state.ui_state.visual_anchor, state.ui_state.visual_cursor) {
                (Some(anchor), Some(cursor)) => (anchor, cursor),
                _ => return Ok(()),
            }
        };

        let selected_text = self.board.get_selected_text_range(anchor, cursor);
        let query = selected_text
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        if query.is_empty() {
            self.state
                .borrow_mut()
                .ui_state
                .open_window(WindowType::Reader);
            return Ok(());
        }

        let url = Self::build_ecosia_search_url(&query)?;
        match self.open_external_link(&url) {
            Ok(true) => {
                let mut state = self.state.borrow_mut();
                state.ui_state.visual_anchor = None;
                state.ui_state.visual_cursor = None;
                state.ui_state.open_window(WindowType::Reader);
                state
                    .ui_state
                    .set_message("Opened search in browser".to_string(), MessageType::Info);
            }
            Ok(false) | Err(_) => {
                let copied = self.set_clipboard_text(url)?;
                let mut state = self.state.borrow_mut();
                state.ui_state.visual_anchor = None;
                state.ui_state.visual_cursor = None;
                state.ui_state.open_window(WindowType::Reader);
                let message = if copied {
                    "Failed to open; search URL copied"
                } else {
                    "Failed to open; clipboard unavailable"
                };
                state
                    .ui_state
                    .set_message(message.to_string(), MessageType::Warning);
            }
        }

        Ok(())
    }

    fn copy_selected_link(&mut self) -> eyre::Result<()> {
        let url = {
            let state = self.state.borrow();
            state
                .ui_state
                .links
                .get(state.ui_state.links_selected_index)
                .map(|link| link.url.clone())
        };
        if let Some(url) = url {
            let copied = self.set_clipboard_text(url)?;
            let ui_state = &mut self.state.borrow_mut().ui_state;
            if copied {
                ui_state.set_message("Link copied to clipboard".to_string(), MessageType::Info);
            } else {
                ui_state.set_message("Clipboard unavailable".to_string(), MessageType::Warning);
            }
            ui_state.open_window(WindowType::Reader);
        }
        Ok(())
    }

    fn follow_selected_link(&mut self) -> eyre::Result<()> {
        let link = {
            let state = self.state.borrow();
            state
                .ui_state
                .links
                .get(state.ui_state.links_selected_index)
                .cloned()
        };

        let Some(link) = link else {
            return Ok(());
        };

        self.follow_link_entry(link)
    }

    fn follow_link_entry(&mut self, link: LinkEntry) -> eyre::Result<()> {
        let base_content = self
            .content_index_for_row(link.row)
            .and_then(|index| self.ebook.as_ref()?.resource_path_for_content_index(index));

        if let Some(target_row) = self.resolve_internal_link_row(&link.url, base_content.as_deref())
        {
            let mut link = link;
            link.target_row = Some(target_row);
            let mut state = self.state.borrow_mut();
            state.ui_state.link_preview = Some(link);
            state.ui_state.open_window(WindowType::LinkPreview);
            return Ok(());
        }

        if Self::is_external_link(&link.url) {
            match self.open_external_link(&link.url) {
                Ok(true) => {
                    let ui_state = &mut self.state.borrow_mut().ui_state;
                    ui_state.set_message("Opened link in browser".to_string(), MessageType::Info);
                    ui_state.open_window(WindowType::Reader);
                    return Ok(());
                }
                Ok(false) | Err(_) => {
                    let copied = self.set_clipboard_text(link.url)?;
                    let ui_state = &mut self.state.borrow_mut().ui_state;
                    let message = if copied {
                        "Failed to open; link copied"
                    } else {
                        "Failed to open; clipboard unavailable"
                    };
                    ui_state.set_message(message.to_string(), MessageType::Warning);
                    ui_state.open_window(WindowType::Reader);
                    return Ok(());
                }
            }
        }

        let copied = self.set_clipboard_text(link.url)?;
        let ui_state = &mut self.state.borrow_mut().ui_state;
        if copied {
            ui_state.set_message("Link copied to clipboard".to_string(), MessageType::Info);
        } else {
            ui_state.set_message("Clipboard unavailable".to_string(), MessageType::Warning);
        }
        ui_state.open_window(WindowType::Reader);
        Ok(())
    }

    fn confirm_link_preview_jump(&mut self) {
        let target_row = {
            let mut state = self.state.borrow_mut();
            state
                .ui_state
                .link_preview
                .take()
                .and_then(|entry| entry.target_row)
        };
        if let Some(target_row) = target_row {
            self.record_jump_position();
            let mut state = self.state.borrow_mut();
            state.reading_state.row = target_row;
            if let Some(content_index) = self.content_index_for_row(target_row) {
                state.reading_state.content_index = content_index;
            }
            state.ui_state.open_window(WindowType::Reader);
        } else {
            self.state
                .borrow_mut()
                .ui_state
                .open_window(WindowType::Reader);
        }
    }

    fn resolve_internal_link_row(&self, href: &str, base_content: Option<&str>) -> Option<usize> {
        let trimmed = href.trim();
        if trimmed.is_empty() || Self::is_external_link(trimmed) {
            return None;
        }

        if let Some(id) = trimmed.strip_prefix('#') {
            if !id.is_empty() {
                return self.resolve_anchor_row(id);
            }
            return None;
        }

        let (path, fragment) = match trimmed.split_once('#') {
            Some((path, fragment)) => (path, Some(fragment)),
            None => (trimmed, None),
        };

        let mut has_fragment = false;
        if let Some(fragment) = fragment
            && !fragment.is_empty()
        {
            has_fragment = true;
            if let Some(row) = self.resolve_anchor_row(fragment) {
                return Some(row);
            }
        }

        if let Some(epub) = self.ebook.as_ref() {
            if let Some(content_index) = epub.content_index_for_href(path) {
                if has_fragment {
                    let current_index = self.state.borrow().reading_state.content_index;
                    if content_index == current_index {
                        return None;
                    }
                }
                return self.content_start_rows.get(content_index).copied();
            }

            if let Some(resolved_path) = Self::resolve_relative_href(path, base_content)
                && let Some(content_index) = epub.content_index_for_href(&resolved_path)
            {
                if has_fragment {
                    let current_index = self.state.borrow().reading_state.content_index;
                    if content_index == current_index {
                        return None;
                    }
                }
                return self.content_start_rows.get(content_index).copied();
            }
        }

        None
    }

    fn resolve_relative_href(href: &str, base_content: Option<&str>) -> Option<String> {
        let href = href.trim();
        if href.is_empty() {
            return None;
        }

        if href.starts_with('/') {
            return Some(href.trim_start_matches('/').to_string());
        }

        let base_content = base_content?;
        let base_path = std::path::Path::new(base_content);
        let base_dir = base_path
            .parent()
            .unwrap_or_else(|| std::path::Path::new(""));
        let joined = base_dir.join(href);
        let mut normalized = std::path::PathBuf::new();
        for component in joined.components() {
            match component {
                std::path::Component::ParentDir => {
                    normalized.pop();
                }
                std::path::Component::CurDir => {}
                _ => normalized.push(component.as_os_str()),
            }
        }
        Some(normalized.to_string_lossy().to_string())
    }

    fn resolve_anchor_row(&self, fragment: &str) -> Option<usize> {
        if let Some(row) = self.board.section_row(fragment) {
            return Some(row);
        }

        let digits: String = fragment.chars().filter(|c| c.is_ascii_digit()).collect();
        if digits.is_empty() {
            return None;
        }

        let candidates = [
            format!("fn{}", digits),
            format!("fn{}fn", digits),
            format!("note{}", digits),
            format!("footnote{}", digits),
            format!("endnote{}", digits),
        ];
        for candidate in &candidates {
            if let Some(row) = self.board.section_row(candidate) {
                return Some(row);
            }
        }

        let section_rows = self.board.section_rows()?;
        let digits_lower = digits.to_ascii_lowercase();
        for (id, row) in section_rows {
            let id_lower = id.to_ascii_lowercase();
            if id_lower.contains(&digits_lower)
                && (id_lower.starts_with("fn")
                    || id_lower.starts_with("footnote")
                    || id_lower.starts_with("endnote")
                    || id_lower.starts_with("note"))
            {
                return Some(*row);
            }
        }

        None
    }

    fn is_external_link(href: &str) -> bool {
        let href = href.to_ascii_lowercase();
        href.starts_with("http://")
            || href.starts_with("https://")
            || href.starts_with("mailto:")
            || href.starts_with("tel:")
            || href.starts_with("ftp://")
    }

    fn open_external_link(&self, url: &str) -> eyre::Result<bool> {
        // Use a system opener to keep link handling out of the TUI.
        let status = std::process::Command::new("xdg-open").arg(url).status();
        match status {
            Ok(status) => Ok(status.success()),
            Err(err) => Err(err.into()),
        }
    }

    // ── TTS (Text-to-Speech) ───────────────────────────────────────

    /// Collect text chunks for TTS with precise per-line underline ranges.
    fn build_tts_chunks(&self) -> Vec<TtsChunk> {
        let Some(lines) = self.board.lines() else {
            return Vec::new();
        };

        // First pass: collect raw paragraphs as (start, end) line ranges.
        let mut raw_paragraphs: Vec<(usize, usize)> = Vec::new();
        let mut start: Option<usize> = None;
        for (i, line) in lines.iter().enumerate() {
            let is_content =
                !line.is_empty() && line != CHAPTER_BREAK_MARKER && !line.starts_with("[Image:");
            if is_content {
                if start.is_none() {
                    start = Some(i);
                }
            } else if let Some(s) = start.take() {
                raw_paragraphs.push((s, i));
            }
        }
        if let Some(s) = start {
            raw_paragraphs.push((s, lines.len()));
        }

        // Second pass: split each paragraph into sentence-boundary chunks
        // and compute per-line underline character ranges.
        let mut chunks = Vec::new();
        for (para_start, para_end) in raw_paragraphs {
            let para_lines: Vec<&str> = (para_start..para_end)
                .filter_map(|i| lines.get(i).map(String::as_str))
                .collect();
            let full_text = para_lines.join(" ");
            if full_text.trim().is_empty() {
                continue;
            }

            // Build cumulative byte offsets for each line boundary in the
            // joined string.  offsets[i] = byte position where line i starts.
            let mut offsets = Vec::with_capacity(para_lines.len() + 1);
            let mut pos = 0usize;
            for (i, line) in para_lines.iter().enumerate() {
                offsets.push(pos);
                pos += line.len();
                if i + 1 < para_lines.len() {
                    pos += 1; // the " " separator
                }
            }
            offsets.push(pos); // end sentinel

            let (min_chunk, max_chunk) = (50, 100);
            let sentence_chunks =
                Self::split_into_sentence_chunks(&full_text, min_chunk, max_chunk);

            let mut byte_cursor = 0usize;
            for chunk_text in sentence_chunks {
                let suffix = &full_text[byte_cursor..];
                let Some(rel_start) = suffix.find(chunk_text.as_str()) else {
                    continue;
                };
                let chunk_byte_start = byte_cursor + rel_start;
                let chunk_byte_end = chunk_byte_start + chunk_text.len();
                byte_cursor = chunk_byte_end;

                // Compute per-line underline ranges
                let mut underline = HashMap::new();
                let mut first_line = para_start;
                let mut found_first = false;

                for (li, line_text) in para_lines.iter().enumerate() {
                    let line_byte_start = offsets[li];
                    let line_byte_end = line_byte_start + line_text.len();

                    // Check if this line overlaps with the chunk
                    if line_byte_end <= chunk_byte_start || line_byte_start >= chunk_byte_end {
                        continue;
                    }

                    if !found_first {
                        first_line = para_start + li;
                        found_first = true;
                    }

                    // Compute column range within this line (in characters)
                    let overlap_byte_start =
                        chunk_byte_start.max(line_byte_start) - line_byte_start;
                    let overlap_byte_end = chunk_byte_end.min(line_byte_end) - line_byte_start;

                    // Convert byte offsets to character offsets
                    let col_start = line_text[..overlap_byte_start].chars().count();
                    let col_end = line_text[..overlap_byte_end].chars().count();

                    if col_start < col_end {
                        underline.insert(para_start + li, (col_start, col_end));
                    }
                }

                let tts_text = RE_TTS_HYPHEN.replace_all(&chunk_text, "$1$2").into_owned();
                chunks.push(TtsChunk {
                    text: tts_text,
                    first_line,
                    underline,
                });
            }
        }
        chunks
    }

    /// Skip trailing closers and inline footnote markers after terminal punctuation.
    fn skip_sentence_trailers(chars: &[char], mut i: usize) -> usize {
        while i < chars.len() {
            match chars[i] {
                '"' | '\'' | ')' | ']' | '}' | '»' | '”' | '’' => i += 1,
                '[' => {
                    let mut j = i + 1;
                    while j < chars.len() && chars[j].is_ascii_alphanumeric() {
                        j += 1;
                    }
                    if j > i + 1 && j < chars.len() && chars[j] == ']' {
                        i = j + 1;
                    } else {
                        break;
                    }
                }
                _ => break,
            }
        }
        i
    }

    /// Return the exclusive end position of a sentence boundary, including
    /// any trailing quote/bracket/footnote markers, if `chars[i]` ends a sentence.
    fn sentence_end_after(chars: &[char], i: usize) -> Option<usize> {
        let ch = chars[i];
        // ? ! ; are almost always sentence endings
        if matches!(ch, '?' | '!' | ';') {
            let next = Self::skip_sentence_trailers(chars, i + 1);
            return (next >= chars.len() || chars[next].is_whitespace()).then_some(next);
        }
        if ch != '.' {
            return None;
        }
        // Must be followed only by closers / footnotes, then whitespace or end of text.
        let next = Self::skip_sentence_trailers(chars, i + 1);
        if next < chars.len() && !chars[next].is_whitespace() {
            return None;
        }
        // Walk back to find the word before the period
        let mut j = i;
        while j > 0 && chars[j - 1].is_alphabetic() {
            j -= 1;
        }
        let word_len = i - j;
        // Single letter before period → likely an initial (L. , M. , etc.)
        if word_len <= 1 {
            return None;
        }
        // Check for common abbreviations (case-insensitive)
        let word: String = chars[j..i].iter().collect::<String>().to_lowercase();
        let abbrevs = [
            "mr", "mrs", "ms", "dr", "st", "sr", "jr", "prof", "gen", "gov", "sgt", "cpl", "pvt",
            "lt", "col", "maj", "capt", "cmdr", "adm", "rev", "hon", "pres", "vs", "etc", "approx",
            "dept", "est", "vol", "fig", "inc", "corp", "ltd", "no",
        ];
        if abbrevs.contains(&word.as_str()) {
            return None;
        }
        Some(next)
    }

    /// Split `text` into chunks of approximately `min_len`..`max_len` characters,
    /// breaking at sentence boundaries. Uses `is_sentence_end` for robust detection.
    fn split_into_sentence_chunks(text: &str, min_len: usize, max_len: usize) -> Vec<String> {
        let text = text.trim();
        if text.is_empty() {
            return Vec::new();
        }
        if text.len() <= max_len {
            return vec![text.to_string()];
        }

        let chars: Vec<char> = text.chars().collect();
        let mut chunks = Vec::new();
        let mut chunk_start = 0;

        while chunk_start < chars.len() {
            if chars.len() - chunk_start <= max_len {
                let s: String = chars[chunk_start..]
                    .iter()
                    .collect::<String>()
                    .trim()
                    .to_string();
                if !s.is_empty() {
                    chunks.push(s);
                }
                break;
            }

            let search_end = (chunk_start + max_len).min(chars.len());
            let search_start = chunk_start + min_len;
            let mut split_at = None;

            // Find the last sentence end in [min_len, max_len]
            for i in search_start..search_end {
                if let Some(end) = Self::sentence_end_after(&chars, i) {
                    split_at = Some(end);
                }
            }

            // If none found, scan forward past max_len
            if split_at.is_none() {
                for i in search_end..chars.len() {
                    if let Some(end) = Self::sentence_end_after(&chars, i) {
                        split_at = Some(end);
                        break;
                    }
                }
            }

            let end = split_at.unwrap_or(chars.len());
            let chunk: String = chars[chunk_start..end]
                .iter()
                .collect::<String>()
                .trim()
                .to_string();
            if !chunk.is_empty() {
                chunks.push(chunk);
            }
            chunk_start = end;
            while chunk_start < chars.len() && chars[chunk_start].is_whitespace() {
                chunk_start += 1;
            }
        }

        chunks
    }

    /// Find the chunk index whose underline range contains `row`,
    /// or the first chunk starting at or after `row`.
    fn find_chunk_at(&self, row: usize) -> Option<usize> {
        for (i, chunk) in self.tts_chunks.iter().enumerate() {
            if chunk.underline.contains_key(&row) {
                return Some(i);
            }
        }
        for (i, chunk) in self.tts_chunks.iter().enumerate() {
            if chunk.first_line >= row {
                return Some(i);
            }
        }
        None
    }

    /// Returns a temp path for edge-tts audio output at the given index.
    fn tts_temp_path(dir: &std::path::Path, index: usize) -> std::path::PathBuf {
        let mut p = dir.to_path_buf();
        p.push(format!("repy_tts_{}.mp3", index));
        p
    }

    fn tts_create_temp_dir() -> eyre::Result<std::path::PathBuf> {
        let mut dir = std::env::temp_dir();
        dir.push(format!(
            "repy_tts_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir)?;
        Ok(dir)
    }

    /// Detect and cache an audio player (mpv or ffplay) for the edge-tts pipeline.
    fn tts_detect_player(&mut self) -> Option<EdgeTtsPlayer> {
        if let Some(ref p) = self.tts_audio_player {
            return Some(p.clone());
        }
        if self.check_program_exists("mpv") {
            self.tts_audio_player = Some(EdgeTtsPlayer::Mpv);
        } else if self.check_program_exists("ffplay") {
            self.tts_audio_player = Some(EdgeTtsPlayer::Ffplay);
        }
        self.tts_audio_player.clone()
    }

    /// Returns true for engines that write audio to a file for playback via mpv/ffplay.
    fn is_file_based_engine(engine: &str) -> bool {
        matches!(engine, "edge-tts" | "purr" | "trans")
            || (engine.contains("{}") && engine.contains("{output}"))
    }

    /// Synchronously convert `text` to an audio file at `path`.
    /// Handles both edge-tts and custom templates containing `{output}`.
    fn tts_convert_with_engine(
        engine: &str,
        text: &str,
        path: &std::path::Path,
    ) -> eyre::Result<()> {
        if engine == "edge-tts" {
            let status = std::process::Command::new("edge-tts")
                .args(["--text", text, "--write-media", &path.to_string_lossy()])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()?;
            if !status.success() {
                return Err(eyre::eyre!("edge-tts exited with non-zero status"));
            }
            return Ok(());
        }
        if engine == "purr" {
            let path_str = path.to_string_lossy();
            let status = std::process::Command::new("purr")
                .args(["speak", "--output", &path_str, "--quiet", text])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()?;
            if !status.success() {
                return Err(eyre::eyre!("purr exited with non-zero status"));
            }
            return Ok(());
        }
        if engine == "trans" {
            let path_str = path.to_string_lossy();
            let status = std::process::Command::new("trans")
                .args([
                    "-brief",
                    "-no-translate",
                    "-download-audio-as",
                    &path_str,
                    "en:",
                    text,
                ])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()?;
            if !status.success() {
                return Err(eyre::eyre!("trans exited with non-zero status"));
            }
            return Ok(());
        }
        // Custom template: substitute {} (text) and {output} (file path)
        let expanded = engine
            .replace("{output}", &path.to_string_lossy())
            .replace("{}", text);
        let parts: Vec<&str> = expanded.split_whitespace().collect();
        if parts.is_empty() {
            return Err(eyre::eyre!("empty TTS command"));
        }
        let status = std::process::Command::new(parts[0])
            .args(&parts[1..])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()?;
        if !status.success() {
            return Err(eyre::eyre!("TTS command exited with non-zero status"));
        }
        Ok(())
    }

    fn tts_set_converting(&mut self, converting: bool) {
        let mut state = self.state.borrow_mut();
        let was_converting = state.ui_state.tts_converting;
        state.ui_state.tts_converting = converting;
        if converting && !was_converting {
            state.ui_state.tts_anim_frame = 0;
        }
    }

    fn tts_prefetch_limit(playback_index: usize, total_chunks: usize) -> Option<usize> {
        if total_chunks == 0 || playback_index >= total_chunks {
            return None;
        }
        Some(
            playback_index
                .saturating_add(TTS_PREFETCH_WINDOW.saturating_sub(1))
                .min(total_chunks - 1),
        )
    }

    fn tts_spawn_worker(
        &mut self,
        engine: String,
        temp_dir: std::path::PathBuf,
        start_index: usize,
    ) {
        let texts: Vec<String> = self
            .tts_chunks
            .iter()
            .map(|chunk| chunk.text.clone())
            .collect();
        let total_chunks = texts.len();
        let (cmd_tx, cmd_rx) = std::sync::mpsc::channel::<TtsWorkerCommand>();
        let (event_tx, event_rx) = std::sync::mpsc::channel::<TtsWorkerEvent>();

        self.tts_worker_tx = Some(cmd_tx.clone());
        self.tts_worker_rx = Some(event_rx);

        std::thread::spawn(move || {
            Self::tts_worker_loop(
                engine,
                temp_dir,
                texts,
                start_index,
                total_chunks,
                cmd_rx,
                event_tx,
            );
        });

        let _ = cmd_tx.send(TtsWorkerCommand::UpdatePlaybackIndex(start_index));
    }

    fn tts_worker_loop(
        engine: String,
        temp_dir: std::path::PathBuf,
        texts: Vec<String>,
        start_index: usize,
        total_chunks: usize,
        cmd_rx: std::sync::mpsc::Receiver<TtsWorkerCommand>,
        event_tx: std::sync::mpsc::Sender<TtsWorkerEvent>,
    ) {
        let mut playback_index = start_index;
        let mut next_to_convert = start_index;

        loop {
            while let Ok(cmd) = cmd_rx.try_recv() {
                match cmd {
                    TtsWorkerCommand::UpdatePlaybackIndex(index) => {
                        playback_index = index.min(total_chunks);
                        next_to_convert = next_to_convert.max(playback_index);
                    }
                    TtsWorkerCommand::Stop => return,
                }
            }

            let Some(limit) = Self::tts_prefetch_limit(playback_index, total_chunks) else {
                return;
            };

            if next_to_convert <= limit {
                let Some(text) = texts.get(next_to_convert) else {
                    return;
                };
                let path = Self::tts_temp_path(&temp_dir, next_to_convert);
                let event = match Self::tts_convert_with_engine(&engine, text, &path) {
                    Ok(()) => TtsWorkerEvent::Ready {
                        index: next_to_convert,
                        path,
                    },
                    Err(_) => TtsWorkerEvent::Failed {
                        index: next_to_convert,
                    },
                };

                if event_tx.send(event).is_err() {
                    return;
                }
                next_to_convert += 1;
                continue;
            }

            match cmd_rx.recv_timeout(Duration::from_millis(80)) {
                Ok(TtsWorkerCommand::UpdatePlaybackIndex(index)) => {
                    playback_index = index.min(total_chunks);
                    next_to_convert = next_to_convert.max(playback_index);
                }
                Ok(TtsWorkerCommand::Stop) => return,
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return,
            }
        }
    }

    fn tts_notify_worker(&self) {
        if let Some(tx) = &self.tts_worker_tx {
            let _ = tx.send(TtsWorkerCommand::UpdatePlaybackIndex(self.tts_chunk_index));
        }
    }

    fn tts_try_play_ready_chunk(&mut self) -> eyre::Result<bool> {
        if let Some(path) = self.tts_ready_audio.remove(&self.tts_chunk_index) {
            self.tts_set_converting(false);
            self.tts_play_file(path)?;
            return Ok(true);
        }

        self.tts_set_converting(true);
        Ok(false)
    }

    fn tts_poll_worker(&mut self) -> eyre::Result<()> {
        let mut events = Vec::new();
        if let Some(rx) = &self.tts_worker_rx {
            loop {
                match rx.try_recv() {
                    Ok(event) => events.push(event),
                    Err(std::sync::mpsc::TryRecvError::Empty) => break,
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        self.tts_worker_rx = None;
                        self.tts_worker_tx = None;
                        break;
                    }
                }
            }
        }

        for event in events {
            match event {
                TtsWorkerEvent::Ready { index, path } => {
                    self.tts_ready_audio.insert(index, path);
                }
                TtsWorkerEvent::Failed { index } => {
                    if index == self.tts_chunk_index && self.state.borrow().ui_state.tts_active {
                        self.stop_tts();
                        self.state
                            .borrow_mut()
                            .ui_state
                            .set_message("TTS conversion failed".to_string(), MessageType::Error);
                        return Ok(());
                    }
                }
            }
        }

        let waiting_for_current = self.state.borrow().ui_state.tts_active
            && self.tts_done_rx.is_none()
            && self.tts_current_audio_path.is_none();

        if waiting_for_current {
            if self.tts_try_play_ready_chunk()? {
                return Ok(());
            }

            let next = self.state.borrow().ui_state.tts_anim_frame.wrapping_add(1);
            self.state.borrow_mut().ui_state.tts_anim_frame = next;
        }

        Ok(())
    }

    /// Toggle TTS: start if not active, stop if active.
    fn toggle_tts(&mut self) -> eyre::Result<()> {
        if self.state.borrow().ui_state.tts_active {
            self.stop_tts();
            return Ok(());
        }

        // Check if the TTS engine is available
        let engine = {
            let state = self.state.borrow();
            state
                .config
                .settings
                .preferred_tts_engine
                .clone()
                .unwrap_or_default()
        };
        let program = if engine == "edge-tts" {
            "edge-tts"
        } else if engine == "purr" {
            "purr"
        } else if engine == "trans" {
            "trans"
        } else if engine.contains("{}") {
            engine.split_whitespace().next().unwrap_or_default()
        } else {
            &engine
        };

        if !program.is_empty() {
            if !self.check_program_exists(program) {
                let mut state = self.state.borrow_mut();
                let msg = if program == "edge-tts" {
                    "TTS failed: 'edge-tts' not found. Install edge-tts: https://github.com/rany2/edge-tts".to_string()
                } else {
                    format!("TTS failed: command '{}' not found", program)
                };
                state.ui_state.set_message(msg, MessageType::Error);
                return Ok(());
            }
        }

        // For file-based engines, verify that an audio player is available
        if Self::is_file_based_engine(&engine) && self.tts_detect_player().is_none() {
            let mut state = self.state.borrow_mut();
            state.ui_state.set_message(
                "TTS: no audio player found; install mpv or ffplay".to_string(),
                MessageType::Error,
            );
            return Ok(());
        }

        self.tts_chunks = self.build_tts_chunks();
        self.tts_ready_audio.clear();
        self.tts_current_engine = engine.clone();
        self.tts_temp_dir = None;
        let current_row = self.state.borrow().reading_state.row.saturating_sub(1);
        let idx = match self.find_chunk_at(current_row) {
            Some(i) => i,
            None => {
                let mut state = self.state.borrow_mut();
                state
                    .ui_state
                    .set_message("No text found to read".to_string(), MessageType::Error);
                return Ok(());
            }
        };
        self.tts_chunk_index = idx;
        if Self::is_file_based_engine(&engine) {
            self.tts_temp_dir = Some(Self::tts_create_temp_dir()?);
            let temp_dir = self
                .tts_temp_dir
                .clone()
                .ok_or_else(|| eyre::eyre!("missing TTS temp dir"))?;
            self.tts_spawn_worker(engine, temp_dir, idx);
        }
        self.tts_speak_current()?;
        Ok(())
    }

    /// Speak the current chunk.
    fn tts_speak_current(&mut self) -> eyre::Result<()> {
        let chunk = match self.tts_chunks.get(self.tts_chunk_index) {
            Some(c) => c,
            None => {
                self.stop_tts();
                return Ok(());
            }
        };

        let text = chunk.text.clone();
        let first_line = chunk.first_line;
        let last_line = chunk.underline.keys().max().copied().unwrap_or(first_line);
        let underline = chunk.underline.clone();

        // Update UI state: mark active, set underline ranges, scroll
        {
            let mut state = self.state.borrow_mut();
            state.ui_state.tts_active = true;
            state.ui_state.tts_underline_ranges = underline;

            let term_rows = match crossterm::terminal::size() {
                Ok((_, rows)) => rows as usize,
                Err(_) => 24,
            };
            let chrome = if state.config.settings.show_top_bar {
                1 + 2 + 2
            } else {
                2
            };
            let page_height = term_rows.saturating_sub(chrome).max(1);
            state.reading_state.row = Self::tts_target_row_for_chunk(
                state.reading_state.row,
                first_line,
                last_line,
                page_height,
                state.config.settings.seamless_between_chapters,
                &self.content_start_rows,
            );
        }

        // Redraw before starting synthesis
        self.draw()?;

        let engine = self.tts_current_engine.clone();

        // --- File-based engines: background conversion queue → play via mpv/ffplay ---
        if Self::is_file_based_engine(&engine) {
            self.tts_notify_worker();
            let _ = text;
            self.tts_try_play_ready_chunk()?;
            return Ok(());
        }

        // --- Inline engines (custom {}-only command) ---
        let (program, args) = if engine.contains("{}") {
            let expanded = engine.replace("{}", &text);
            let parts: Vec<&str> = expanded.split_whitespace().collect();
            if parts.is_empty() {
                self.stop_tts();
                return Ok(());
            }
            (
                parts[0].to_string(),
                parts[1..].iter().map(|s| s.to_string()).collect(),
            )
        } else {
            (engine, vec![text])
        };

        // Spawn TTS process in its own process group so we can kill all its children.
        let (tx, rx) = std::sync::mpsc::channel();
        self.tts_done_rx = Some(rx);

        let mut cmd = std::process::Command::new(&program);
        cmd.args(&args)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());

        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            unsafe {
                cmd.pre_exec(|| {
                    libc::setsid();
                    Ok(())
                });
            }
        }

        match cmd.spawn() {
            Ok(child) => {
                let pid = child.id();
                let mut child_for_thread = child;
                std::thread::spawn(move || {
                    let _ = child_for_thread.wait();
                    let _ = tx.send(());
                });
                self.tts_child = None;
                self.tts_kill_pid = Some(pid);
            }
            Err(err) => {
                self.stop_tts();
                let mut state = self.state.borrow_mut();
                state
                    .ui_state
                    .set_message(format!("TTS failed: {err}"), MessageType::Error);
            }
        }

        Ok(())
    }

    /// Spawn the audio player for `audio_path`.
    fn tts_play_file(&mut self, audio_path: std::path::PathBuf) -> eyre::Result<()> {
        let player = match self.tts_detect_player() {
            Some(p) => p,
            None => {
                self.stop_tts();
                let mut state = self.state.borrow_mut();
                state.ui_state.set_message(
                    "TTS: no audio player found; install mpv or ffplay".to_string(),
                    MessageType::Error,
                );
                return Ok(());
            }
        };

        self.tts_current_audio_path = Some(audio_path.clone());
        self.tts_set_converting(false);

        let (tx, rx) = std::sync::mpsc::channel::<()>();
        self.tts_done_rx = Some(rx);

        let mut cmd = std::process::Command::new(player.program());
        cmd.args(player.args(&audio_path))
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());

        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            unsafe {
                cmd.pre_exec(|| {
                    libc::setsid();
                    Ok(())
                });
            }
        }

        match cmd.spawn() {
            Ok(child) => {
                let pid = child.id();
                let mut child_for_thread = child;
                std::thread::spawn(move || {
                    let _ = child_for_thread.wait();
                    let _ = tx.send(());
                });
                self.tts_child = None;
                self.tts_kill_pid = Some(pid);
            }
            Err(err) => {
                self.stop_tts();
                let mut state = self.state.borrow_mut();
                state
                    .ui_state
                    .set_message(format!("TTS player failed: {err}"), MessageType::Error);
                return Ok(());
            }
        }
        Ok(())
    }

    /// Advance to the next chunk after the current one finishes.
    fn tts_advance_paragraph(&mut self) -> eyre::Result<()> {
        // Clean up the temp file for the chunk that just finished playing.
        if let Some(path) = self.tts_current_audio_path.take() {
            let _ = std::fs::remove_file(&path);
        }
        self.tts_chunk_index += 1;
        if self.tts_chunk_index >= self.tts_chunks.len() {
            self.stop_tts();
            let mut state = self.state.borrow_mut();
            state
                .ui_state
                .set_message("TTS finished".to_string(), MessageType::Info);
            return Ok(());
        }
        self.tts_notify_worker();
        self.tts_speak_current()
    }

    /// Stop TTS playback — kill the entire process group.
    fn stop_tts(&mut self) {
        #[allow(unused_variables)]
        if let Some(pid) = self.tts_kill_pid.take() {
            #[cfg(unix)]
            unsafe {
                libc::kill(-(pid as i32), libc::SIGKILL);
            }
            #[cfg(not(unix))]
            {
                if let Some(mut child) = self.tts_child.take() {
                    let _ = child.kill();
                }
            }
        }
        if let Some(mut child) = self.tts_child.take() {
            let _ = child.kill();
        }
        self.tts_done_rx = None;

        if let Some(tx) = self.tts_worker_tx.take() {
            let _ = tx.send(TtsWorkerCommand::Stop);
        }
        self.tts_worker_rx = None;

        // Delete temp audio files.
        if let Some(path) = self.tts_current_audio_path.take() {
            let _ = std::fs::remove_file(&path);
        }
        for (_, path) in self.tts_ready_audio.drain() {
            let _ = std::fs::remove_file(&path);
        }
        if let Some(dir) = self.tts_temp_dir.take() {
            let _ = std::fs::remove_dir_all(dir);
        }

        self.tts_chunks.clear();
        self.tts_chunk_index = 0;
        self.tts_current_engine.clear();
        let mut state = self.state.borrow_mut();
        state.ui_state.tts_active = false;
        state.ui_state.tts_converting = false;
        state.ui_state.tts_underline_ranges.clear();
    }

    /// Check if a program exists in the PATH.
    fn check_program_exists(&self, program: &str) -> bool {
        // Try 'which' command first
        let status = std::process::Command::new("which")
            .arg(program)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();

        match status {
            Ok(s) => s.success(),
            Err(_) => {
                // 'which' failed for any reason (missing, etc.), fallback to direct spawn check
                match std::process::Command::new(program)
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .spawn()
                {
                    Ok(mut child) => {
                        let _ = child.kill();
                        true
                    }
                    Err(e) => e.kind() != std::io::ErrorKind::NotFound,
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Reader, TtsChunk, WikipediaSearchResponse, WikipediaSummaryResponse};
    use crate::config::Config;
    use crate::models::{TextStructure, TocEntry};
    use crate::settings::{CfgDefaultKeymaps, Settings};
    use crate::state::State;
    use crate::ui::board::Board;
    use crate::ui::reader::{ApplicationState, MessageType};
    use arboard::Clipboard;
    use ratatui::Terminal;
    use ratatui::backend::CrosstermBackend;
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::io::{BufRead, BufReader, Write};
    use std::net::{TcpListener, TcpStream};
    use std::rc::Rc;
    use std::thread;
    use std::time::Duration;

    /// Alias pinning the default backend so associated-function calls like
    /// `Reader::foo()` don't need turbofish (the default type param isn't
    /// inferred for those).
    type TestReader = Reader<CrosstermBackend<std::io::Stdout>>;

    fn make_test_reader(text_lines: Vec<String>) -> Reader {
        let config =
            Config::with_settings(Settings::default(), CfgDefaultKeymaps::default()).unwrap();
        let state = State::new_for_test();
        let app_state = Rc::new(RefCell::new(ApplicationState::new(config)));

        let mut board = Board::new();
        let ts = TextStructure {
            text_lines,
            ..Default::default()
        };
        board = board.with_text_structure(ts);

        Reader {
            state: app_state,
            terminal: Terminal::new(CrosstermBackend::new(std::io::stdout())).unwrap(),
            db_state: state,
            board,
            clipboard: Clipboard::new().ok(),
            ebook: None,
            content_start_rows: Vec::new(),
            chapter_text_structures: Vec::new(),
            current_text_width: None,
            dictionary_res_rx: None,
            tts_done_rx: None,
            tts_child: None,
            tts_chunks: Vec::new(),
            tts_chunk_index: 0,
            tts_kill_pid: None,
            tts_audio_player: None,
            tts_current_audio_path: None,
            tts_ready_audio: HashMap::new(),
            tts_worker_tx: None,
            tts_worker_rx: None,
            tts_current_engine: String::new(),
            tts_temp_dir: None,
            reading_session: None,
            cached_statistics: None,
        }
    }

    fn read_request_line(stream: TcpStream) -> (TcpStream, String) {
        let mut reader = BufReader::new(stream);
        let mut request_line = String::new();
        reader.read_line(&mut request_line).unwrap();
        (reader.into_inner(), request_line)
    }

    fn write_json_response(stream: &mut TcpStream, status: &str, body: &str) {
        let response = format!(
            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        stream.write_all(response.as_bytes()).unwrap();
        stream.flush().unwrap();
    }

    #[test]
    fn toc_activation_starts_first_entry_at_content_start() {
        let mut reader = make_test_reader(vec![
            "Saturday evening".to_string(),
            "Entering the retreat".to_string(),
            "Last line".to_string(),
            "[[Image: Mu_by_Kusan.png]]".to_string(),
            "Sunday morning".to_string(),
            "The basis of meditation".to_string(),
        ]);
        reader.content_start_rows = vec![0, 3];
        reader.chapter_text_structures = vec![
            TextStructure {
                text_lines: vec![
                    "Saturday evening".to_string(),
                    "Entering the retreat".to_string(),
                    "Last line".to_string(),
                ],
                section_rows: HashMap::from([("sat".to_string(), 0)]),
                ..Default::default()
            },
            TextStructure {
                text_lines: vec![
                    "[[Image: Mu_by_Kusan.png]]".to_string(),
                    "Sunday morning".to_string(),
                    "The basis of meditation".to_string(),
                ],
                section_rows: HashMap::from([("sun".to_string(), 4)]),
                ..Default::default()
            },
        ];

        let toc_entries = vec![
            TocEntry {
                label: "Sat p.m. Entering the retreat".to_string(),
                content_index: 0,
                section: Some("sat".to_string()),
            },
            TocEntry {
                label: "Sun a.m. The basis of meditation".to_string(),
                content_index: 1,
                section: Some("sun".to_string()),
            },
        ];

        assert_eq!(reader.effective_toc_row(1, Some("sun")), Some(4));
        assert_eq!(reader.toc_activation_row(&toc_entries, 1), Some(3));

        let current_row = 3;
        let mut selected_index = 0;
        for i in 0..toc_entries.len() {
            if let Some(row) = reader.toc_activation_row(&toc_entries, i)
                && row <= current_row
            {
                selected_index = i;
            }
        }

        assert_eq!(selected_index, 1);
    }

    #[test]
    fn toc_activation_does_not_shift_first_entry_in_single_content_file() {
        let mut reader = make_test_reader(vec![
            "Front matter".to_string(),
            "Chapter one".to_string(),
            "Chapter two".to_string(),
        ]);
        reader.content_start_rows = vec![0];
        reader.chapter_text_structures = vec![TextStructure {
            text_lines: vec![
                "Front matter".to_string(),
                "Chapter one".to_string(),
                "Chapter two".to_string(),
            ],
            section_rows: HashMap::from([("chapter-one".to_string(), 1)]),
            ..Default::default()
        }];

        let toc_entries = vec![TocEntry {
            label: "Chapter one".to_string(),
            content_index: 0,
            section: Some("chapter-one".to_string()),
        }];

        assert_eq!(reader.toc_activation_row(&toc_entries, 0), Some(1));
    }

    #[test]
    fn resolve_relative_href_joins_base_dir() {
        let resolved = TestReader::resolve_relative_href(
            "chapter007.xhtml",
            Some("OEBPS/Text/chapter001.xhtml"),
        );
        assert_eq!(resolved, Some("OEBPS/Text/chapter007.xhtml".to_string()));
    }

    #[test]
    fn resolve_relative_href_handles_parent_dirs() {
        let resolved = TestReader::resolve_relative_href(
            "../Images/cover.jpg",
            Some("OEBPS/Text/chapter001.xhtml"),
        );
        assert_eq!(resolved, Some("OEBPS/Images/cover.jpg".to_string()));
    }

    #[test]
    fn resolve_relative_href_strips_leading_slash() {
        let resolved = TestReader::resolve_relative_href("/Text/chapter007.xhtml", None);
        assert_eq!(resolved, Some("Text/chapter007.xhtml".to_string()));
    }

    #[test]
    fn build_dictionary_command_replaces_placeholder() {
        let (program, args) =
            TestReader::build_dictionary_command("dict -wn \"%q\"", "apple").unwrap();
        assert_eq!(program, "dict");
        assert_eq!(args, vec!["-wn".to_string(), "apple".to_string()]);
    }

    #[test]
    fn build_dictionary_command_appends_query_without_placeholder() {
        let (program, args) = TestReader::build_dictionary_command("dict -wn", "apple").unwrap();
        assert_eq!(program, "dict");
        assert_eq!(args, vec!["-wn".to_string(), "apple".to_string()]);
    }

    #[test]
    fn build_dictionary_command_handles_internal_quotes_in_query() {
        // Current behavior: if query contains quotes, they are passed as part of the argument.
        // This is safe because we don't use shell=True.
        let (program, args) =
            TestReader::build_dictionary_command("tool --arg=%q", "word \"with\" quotes").unwrap();
        assert_eq!(program, "tool");
        assert_eq!(args, vec!["--arg=word \"with\" quotes".to_string()]);
    }

    #[test]
    fn build_dictionary_command_escapes_quotes_if_wrapped_in_template() {
        let (program, args) =
            TestReader::build_dictionary_command("sh -c \"dict %q\"", "a\"b").unwrap();
        assert_eq!(program, "sh");
        assert_eq!(args, vec!["-c".to_string(), "dict a\\\"b".to_string()]);
    }

    #[test]
    fn parse_wikipedia_summary_response_extracts_result() {
        let body = r#"{
          "query": {
            "pages": {
              "123": {
                "title": "Rust",
                "extract": "Rust is a systems programming language.",
                "fullurl": "https://simple.wikipedia.org/wiki/Rust"
              }
            }
          }
        }"#;
        let parsed: WikipediaSummaryResponse = serde_json::from_str(body).unwrap();
        let result = TestReader::parse_wikipedia_summary_response(&parsed, "simple", "Rust")
            .unwrap()
            .unwrap();
        assert_eq!(result.url, "https://simple.wikipedia.org/wiki/Rust");
        assert!(result.summary.contains("systems programming language"));
    }

    #[test]
    fn parse_wikipedia_summary_response_skips_missing_and_disambiguation() {
        let body = r#"{
          "query": {
            "pages": {
              "-1": {
                "title": "NoSuchTerm",
                "missing": ""
              },
              "99": {
                "title": "Mercury",
                "extract": "Mercury may refer to ...",
                "pageprops": {
                  "disambiguation": ""
                }
              }
            }
          }
        }"#;
        let parsed: WikipediaSummaryResponse = serde_json::from_str(body).unwrap();
        let result =
            TestReader::parse_wikipedia_summary_response(&parsed, "simple", "NoSuchTerm").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn extract_search_titles_reads_candidates() {
        let body = r#"{
          "query": {
            "search": [
              { "title": "Rust_(programming_language)" },
              { "title": "Rust_(fungus)" }
            ]
          }
        }"#;
        let parsed: WikipediaSearchResponse = serde_json::from_str(body).unwrap();
        let titles = TestReader::extract_search_titles(parsed);
        assert_eq!(
            titles,
            vec![
                "Rust_(programming_language)".to_string(),
                "Rust_(fungus)".to_string()
            ]
        );
    }

    #[test]
    fn build_ecosia_search_url_encodes_normalized_query() {
        let url = TestReader::build_ecosia_search_url("rust\nterminal  ui").unwrap();
        assert_eq!(url, "https://www.ecosia.org/search?q=rust+terminal+ui");
    }

    #[test]
    fn wikipedia_lookup_summary_mock_http_direct_hit() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());

        let server = thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let (mut stream, request_line) = read_request_line(stream);
            assert!(request_line.contains("/w/api.php?"));
            assert!(request_line.contains("prop=extracts%7Cinfo%7Cpageprops"));
            assert!(request_line.contains("titles=Rust"));

            let body = r#"{
              "query": {
                "pages": {
                  "123": {
                    "title": "Rust",
                    "extract": "Rust is a systems programming language.",
                    "fullurl": "https://simple.wikipedia.org/wiki/Rust"
                  }
                }
              }
            }"#;
            write_json_response(&mut stream, "200 OK", body);
        });

        let result = TestReader::wikipedia_lookup_summary("Rust", &base, Duration::from_secs(2))
            .expect("direct lookup should succeed");
        server.join().unwrap();

        assert_eq!(result.url, "https://simple.wikipedia.org/wiki/Rust");
        assert!(result.summary.contains("systems programming language"));
    }

    #[test]
    fn wikipedia_lookup_summary_mock_http_search_fallback() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());

        let server = thread::spawn(move || {
            // 1) initial summary request (miss)
            let (stream1, _) = listener.accept().unwrap();
            let (mut stream1, request_line1) = read_request_line(stream1);
            assert!(request_line1.contains("/w/api.php?"));
            assert!(request_line1.contains("titles=NoSuchTerm"));
            let miss = r#"{
              "query": {
                "pages": {
                  "-1": {
                    "title": "NoSuchTerm",
                    "missing": ""
                  }
                }
              }
            }"#;
            write_json_response(&mut stream1, "200 OK", miss);

            // 2) search request
            let (stream2, _) = listener.accept().unwrap();
            let (mut stream2, request_line2) = read_request_line(stream2);
            assert!(request_line2.contains("list=search"));
            assert!(request_line2.contains("srsearch=NoSuchTerm"));
            let search = r#"{
              "query": {
                "search": [
                  { "title": "Rust_(programming_language)" }
                ]
              }
            }"#;
            write_json_response(&mut stream2, "200 OK", search);

            // 3) candidate summary request (hit)
            let (stream3, _) = listener.accept().unwrap();
            let (mut stream3, request_line3) = read_request_line(stream3);
            assert!(request_line3.contains("titles=Rust_%28programming_language%29"));
            let hit = r#"{
              "query": {
                "pages": {
                  "456": {
                    "title": "Rust (programming language)",
                    "extract": "Rust is a multi-paradigm language focused on safety.",
                    "fullurl": "https://simple.wikipedia.org/wiki/Rust_(programming_language)"
                  }
                }
              }
            }"#;
            write_json_response(&mut stream3, "200 OK", hit);
        });

        let result =
            TestReader::wikipedia_lookup_summary("NoSuchTerm", &base, Duration::from_secs(2))
                .expect("fallback lookup should succeed");
        server.join().unwrap();

        assert_eq!(
            result.url,
            "https://simple.wikipedia.org/wiki/Rust_(programming_language)"
        );
        assert!(result.summary.contains("focused on safety"));
    }

    #[test]
    fn tts_detection_hint_on_missing_program() {
        let mut reader = make_test_reader(vec!["Some text to read for TTS test.".to_string()]);
        let app_state = reader.state.clone();

        // Ensure tts engine is set to edge-tts (default)
        {
            let mut s = app_state.borrow_mut();
            s.config.settings.preferred_tts_engine = Some("edge-tts".to_string());
        }

        // Set to a definitely missing program
        {
            let mut s = app_state.borrow_mut();
            s.config.settings.preferred_tts_engine =
                Some("definitely-not-a-real-program-12345".to_string());
        }

        reader.toggle_tts().unwrap();

        let s = app_state.borrow();
        assert!(s.ui_state.message.is_some());
        let msg = s.ui_state.message.as_ref().unwrap();
        let msg_type = &s.ui_state.message_type;
        assert_eq!(*msg_type, MessageType::Error);
        assert!(msg.contains("command 'definitely-not-a-real-program-12345' not found"));
    }

    #[test]
    fn test_dehyphenate_tts_text() {
        use super::RE_TTS_HYPHEN;
        let input = "This is an ex- ample of hyphen- ation artifacts.";
        let result = RE_TTS_HYPHEN.replace_all(input, "$1$2").into_owned();
        assert_eq!(result, "This is an example of hyphenation artifacts.");
    }

    #[test]
    fn tts_chunk_matching_handles_unicode_boundaries() {
        let text = "“Well said, friend,” the delighted bhikkhus spoke, and asked, “Is there yet another teaching on how a disciple practices Right View?\u{a0}.\u{a0}.\u{a0}.”";
        let chunks = TestReader::split_into_sentence_chunks(text, 50, 100);

        assert!(!chunks.is_empty());

        let mut byte_cursor = 0usize;
        for chunk_text in chunks {
            let suffix = &text[byte_cursor..];
            let rel_start = suffix
                .find(chunk_text.as_str())
                .expect("chunk should be found at a character boundary");
            let chunk_byte_start = byte_cursor + rel_start;
            let chunk_byte_end = chunk_byte_start + chunk_text.len();

            assert!(text.is_char_boundary(chunk_byte_start));
            assert!(text.is_char_boundary(chunk_byte_end));
            assert_eq!(&text[chunk_byte_start..chunk_byte_end], chunk_text);

            byte_cursor = chunk_byte_end;
        }
    }

    #[test]
    fn tts_chunk_split_respects_quote_and_footnote_sentence_boundaries() {
        let text = "Subhadda asked, “World-Honored One, are the other religious teachers in Magadha and Koshala fully enlightened?” The Buddha knew he had only a short time to live and that answering such a question would be a waste of precious moments. When you have the opportunity to ask a teacher about the Dharma, ask a question that can change your life. The Buddha replied, “Subhadda, it is not important whether they are fully enlightened. The question is whether you want to liberate yourself. If you do, practice the Noble Eightfold Path. Wherever the Noble Eightfold Path is practiced, joy, peace, and insight are there.”[1] The Buddha offered the Eightfold Path in his first Dharma talk, he continued to teach the Eightfold Path for forty-five years, and in his last Dharma talk, spoken to Subhadda, he offered the Noble Eightfold Path. Right View, Right Thinking, Right Speech, Right Action, Right Livelihood, Right Diligence, Right Mindfulness, and Right Concentration.[2]";
        let chunks = TestReader::split_into_sentence_chunks(text, 50, 100);

        assert!(
            chunks.iter().any(|chunk| chunk.ends_with("there.”[1]")),
            "expected a chunk to end at the quoted footnote boundary, got {chunks:?}"
        );
        assert!(
            chunks
                .iter()
                .any(|chunk| chunk.starts_with("The Buddha offered the Eightfold Path")),
            "expected a new chunk after the footnote boundary, got {chunks:?}"
        );
    }

    #[test]
    fn tts_target_row_turns_page_when_next_chunk_starts_new_chapter() {
        let target_row = TestReader::tts_target_row_for_chunk(1, 10, 12, 20, false, &[0, 10]);

        assert_eq!(target_row, 11);
    }

    #[test]
    fn tts_target_row_keeps_viewport_when_chunk_is_visible_in_same_chapter() {
        let target_row = TestReader::tts_target_row_for_chunk(11, 12, 13, 20, false, &[0, 10]);

        assert_eq!(target_row, 11);
    }

    #[test]
    fn find_chunk_at_uses_visible_top_line_without_skipping_footnotes() {
        let mut reader = make_test_reader(vec![
            "[1] Mahaparinibbana Sutta, Digha Nikaya 16.".to_string(),
            String::new(),
            "[2] See chap. 3, n. 1, on [here], regarding the use of the word \"Right.\""
                .to_string(),
        ]);
        reader.tts_chunks = vec![
            TtsChunk {
                text: "[1] Mahaparinibbana Sutta, Digha Nikaya 16.".to_string(),
                first_line: 0,
                underline: HashMap::from([(0, (0, 43))]),
            },
            TtsChunk {
                text: "[2] See chap. 3, n. 1, on [here], regarding the use of the word \"Right.\""
                    .to_string(),
                first_line: 2,
                underline: HashMap::from([(2, (0, 73))]),
            },
        ];

        let current_row = 1usize.saturating_sub(1);

        assert_eq!(reader.find_chunk_at(current_row), Some(0));
    }

    #[test]
    fn visual_search_regex_smartcase_and_escape() {
        // All-lowercase query is case-insensitive (smartcase off).
        let re = TestReader::build_visual_search_regex("foo").unwrap();
        assert!(re.is_match("FOO"));
        assert!(re.is_match("foo"));

        // Mixed/upper case forces case-sensitive (smartcase on).
        let re = TestReader::build_visual_search_regex("Foo").unwrap();
        assert!(re.is_match("Foo"));
        assert!(!re.is_match("foo"));

        // Regex specials are treated literally.
        let re = TestReader::build_visual_search_regex("a.b").unwrap();
        assert!(re.is_match("a.b"));
        assert!(!re.is_match("axb"));

        // Spaces match across newlines so wrapped-line queries work.
        let re = TestReader::build_visual_search_regex("foo bar").unwrap();
        assert!(re.is_match("foo\nbar"));
        assert!(re.is_match("foo  bar"));

        // Soft hyphen wraps inserted by the line-wrapper match.
        let re = TestReader::build_visual_search_regex("example").unwrap();
        assert!(re.is_match("example"));
        assert!(re.is_match("exam-\nple"));
        // But a plain in-line hyphen is not silently matched.
        assert!(!re.is_match("exam-ple"));
    }
}

#[cfg(test)]
mod snapshot_tests;
