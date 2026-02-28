use arboard::Clipboard;
use regex::Regex;
use serde::Deserialize;
use serde_json::Value;
use std::cell::RefCell;
use std::collections::HashMap;
use std::io;
use std::rc::Rc;
use std::time::{Duration, Instant};

use chrono::Local;
use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::{
    Frame, Terminal,
    backend::CrosstermBackend,
    layout::{Constraint, Layout, Rect},
    style::{Color, Style},
    text::Line,
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};

use crate::config::Config;
use crate::ebook::{Ebook, Epub, build_chapter_break};
use crate::logging;
use crate::models::{
    BookMetadata, CHAPTER_BREAK_MARKER, Direction as AppDirection, LibraryItem, LinkEntry,
    ReadingState, SearchData, TextStructure, TocEntry, WindowType,
};
use crate::settings::DICT_PRESET_LIST;
use crate::state::State;
use crate::ui::board::Board;
use crate::ui::windows::{
    bookmarks::BookmarksWindow, dictionary::DictionaryWindow, help::HelpWindow,
    images::ImagesWindow, library::LibraryWindow, links::LinksWindow, metadata::MetadataWindow,
    search::SearchWindow, settings::SettingsWindow, toc::TocWindow,
};

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
        }
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
    pub show_dictionary: bool,
    pub show_settings: bool,
    pub search_query: String,
    pub search_results: Vec<SearchResult>,
    pub search_matches: HashMap<usize, Vec<(usize, usize)>>,
    pub selected_search_result: usize,
    pub toc_entries: Vec<TocEntry>,
    pub toc_selected_index: usize,
    pub bookmarks: Vec<(String, ReadingState)>,
    pub bookmarks_selected_index: usize,
    pub links: Vec<LinkEntry>,
    pub links_selected_index: usize,
    pub images_list: Vec<(usize, String)>,
    pub images_selected_index: usize,
    pub library_items: Vec<LibraryItem>,
    pub library_selected_index: usize,
    pub metadata: Option<BookMetadata>,
    pub dictionary_word: String,
    pub dictionary_definition: String,
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
            show_dictionary: false,
            show_settings: false,
            search_query: String::new(),
            search_results: Vec::new(),
            search_matches: HashMap::new(),
            selected_search_result: 0,
            toc_entries: Vec::new(),
            toc_selected_index: 0,
            bookmarks: Vec::new(),
            bookmarks_selected_index: 0,
            links: Vec::new(),
            links_selected_index: 0,
            images_list: Vec::new(),
            images_selected_index: 0,
            library_items: Vec::new(),
            library_selected_index: 0,
            metadata: None,
            dictionary_word: String::new(),
            dictionary_definition: String::new(),
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

    pub fn open_window(&mut self, window_type: WindowType) {
        self.active_window = window_type.clone();
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
                self.show_dictionary = false;
                self.show_settings = false;
                self.visual_anchor = None;
                self.visual_cursor = None;
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
            WindowType::Dictionary => {
                self.show_dictionary = true;
                self.dictionary_scroll_offset = 0;
            }
            WindowType::Settings => self.show_settings = true,
            WindowType::Visual => {}
            WindowType::DictionaryCommandInput => {
                self.show_settings = false;
            }
        }
    }
}

#[derive(Debug, Clone)]
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
    StartWithDoubleSpread,
    SeamlessBetweenChapters,
    DictionaryClient,
    TtsEngine,
    Width,
    ShowTopBar,
}

impl SettingItem {
    fn all() -> &'static [SettingItem] {
        &[
            SettingItem::ShowLineNumbers,
            SettingItem::MouseSupport,
            SettingItem::PageScrollAnimation,
            SettingItem::ShowProgressIndicator,
            SettingItem::StartWithDoubleSpread,
            SettingItem::SeamlessBetweenChapters,
            SettingItem::DictionaryClient,
            SettingItem::TtsEngine,
            SettingItem::Width,
            SettingItem::ShowTopBar,
        ]
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

/// Main reader application struct
pub struct Reader {
    state: Rc<RefCell<ApplicationState>>,
    terminal: Terminal<CrosstermBackend<io::Stdout>>,
    db_state: State,
    board: Board,
    clipboard: Clipboard,
    ebook: Option<Epub>,
    content_start_rows: Vec<usize>,
    /// Per-chapter text structures for incremental rebuilds
    chapter_text_structures: Vec<TextStructure>,
    /// Text width used for the current chapter structures
    current_text_width: Option<usize>,
    dictionary_res_rx: Option<std::sync::mpsc::Receiver<DictionaryResult>>,
    /// Channel to receive notification when a TTS chunk finishes speaking
    tts_done_rx: Option<std::sync::mpsc::Receiver<()>>,
    /// Handle to the running TTS child process so we can kill it on Esc
    tts_child: Option<std::process::Child>,
    /// Precomputed TTS chunks with text and per-line underline ranges
    tts_chunks: Vec<TtsChunk>,
    /// Index into tts_chunks for the chunk currently being spoken
    tts_chunk_index: usize,
    /// PID of the running TTS process for killing (entire process group)
    tts_kill_pid: Option<u32>,
}

impl Reader {
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

    /// Create a new Reader instance
    pub fn new(config: Config) -> eyre::Result<Self> {
        let backend = CrosstermBackend::new(io::stdout());
        let terminal = Terminal::new(backend)?;

        // Initialize database state
        let db_state = State::new()?;

        let app_state = ApplicationState::new(config);

        Ok(Self {
            state: Rc::new(RefCell::new(app_state)),
            terminal,
            db_state,
            board: Board::new(),
            clipboard: Clipboard::new()?,
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
        })
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
        let normalized_path = Self::normalize_ebook_path(path);
        if normalized_path != path {
            self.db_state.reconcile_filepath(path, &normalized_path)?;
        }

        let mut epub = Epub::new(&normalized_path);
        epub.initialize()?;

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

        if let Some(epub) = self.ebook.as_ref() {
            let mut state = self.state.borrow_mut();

            // Load last reading state from the database (or default if none)
            if let Some(s) = db_state {
                state.reading_state = s;
            }

            // Ensure textwidth matches what we decided
            state.reading_state.textwidth = textwidth;

            let total_lines = self.board.total_lines();
            if total_lines > 0 && state.reading_state.row >= total_lines {
                state.reading_state.row = total_lines - 1;
            }

            state.ui_state.metadata = Some(epub.get_meta().clone());
            state.ui_state.toc_entries = epub.toc_entries().clone();
            state.ui_state.toc_selected_index = 0;
            if let Ok(bookmarks) = self.db_state.get_bookmarks(epub) {
                state.ui_state.bookmarks = bookmarks;
                state.ui_state.bookmarks_selected_index = 0;
            }
        }

        Ok(())
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
        }
        Ok(())
    }

    /// Run the main application loop
    pub fn run(&mut self) -> eyre::Result<()> {
        // Initialize terminal
        crossterm::terminal::enable_raw_mode()?;
        crossterm::execute!(
            io::stdout(),
            crossterm::terminal::EnterAlternateScreen,
            crossterm::event::EnableMouseCapture
        )?;

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

            // Check for dictionary results
            if let Some(rx) = &self.dictionary_res_rx {
                if let Ok(res) = rx.try_recv() {
                    let mut state = self.state.borrow_mut();
                    state.ui_state.dictionary_word = res.word;
                    state.ui_state.dictionary_definition = match res.definition {
                        Ok(def) => def,
                        Err(err) => err,
                    };
                    state.ui_state.dictionary_loading = false;
                    self.dictionary_res_rx = None;
                }
            }

            // Check for TTS paragraph completion → advance to next paragraph
            if self.state.borrow().ui_state.tts_active {
                if let Some(rx) = &self.tts_done_rx {
                    if let Ok(()) = rx.try_recv() {
                        self.tts_child = None;
                        self.tts_done_rx = None;
                        self.tts_advance_paragraph()?;
                    }
                }
            }

            // Render UI
            {
                let state = self.state.clone();
                self.terminal.draw(|f| {
                    let state_ref = state.borrow();
                    Self::render_static(f, &state_ref, &self.board, &self.content_start_rows);
                })?;
            }

            // Poll with timeout so we can re-render when messages expire or for animation
            let poll_timeout = {
                let state = self.state.borrow();
                if state.ui_state.tts_active {
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
                            self.handle_key_event(key)?;
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
            crossterm::event::DisableMouseCapture
        )?;
        crossterm::terminal::disable_raw_mode()?;

        Ok(())
    }

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
            WindowType::Library => self.handle_library_mode_keys(key, repeat_count)?,
            WindowType::Settings => self.handle_settings_mode_keys(key, repeat_count)?,
            WindowType::Links => self.handle_links_mode_keys(key, repeat_count)?,
            WindowType::Images => self.handle_images_mode_keys(key, repeat_count)?,
            WindowType::Help => self.handle_help_mode_keys(key, repeat_count)?,
            WindowType::Metadata => self.handle_modal_close_keys(key)?,
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

            // Search
            KeyCode::Char('/') => {
                let mut state = self.state.borrow_mut();
                state.search_data = Some(SearchData::default());
                state.ui_state.search_query.clear();
                state.ui_state.search_results.clear();
                state.ui_state.search_matches.clear();
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
            // Esc stops TTS if active
            KeyCode::Esc => {
                if self.state.borrow().ui_state.tts_active {
                    self.stop_tts();
                }
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
        let mut state = self.state.borrow_mut();
        state.jump_back();
    }

    fn jump_forward(&mut self) {
        let mut state = self.state.borrow_mut();
        state.jump_forward();
    }

    /// Handle keys in search mode
    fn handle_search_mode_keys(&mut self, key: KeyEvent, _repeat_count: u32) -> eyre::Result<()> {
        match key.code {
            KeyCode::Enter => {
                let has_results = {
                    let state = self.state.borrow();
                    !state.ui_state.search_results.is_empty()
                };
                if has_results {
                    self.jump_to_selected_search_result();
                } else {
                    self.execute_search();
                }
            }
            KeyCode::Esc => {
                // Cancel search
                {
                    let mut state = self.state.borrow_mut();
                    state.search_data = None;
                    state.ui_state.open_window(WindowType::Reader);
                }
            }
            KeyCode::Backspace => {
                // Remove last character from search query
                let mut state = self.state.borrow_mut();
                state.ui_state.search_query.pop();
                state.ui_state.search_results.clear();
                state.ui_state.search_matches.clear();
            }
            KeyCode::Char('j') | KeyCode::Down => {
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
            KeyCode::Char('k') | KeyCode::Up => {
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
                // Add character to search query
                let mut state = self.state.borrow_mut();
                state.ui_state.search_query.push(c);
                state.ui_state.search_results.clear();
                state.ui_state.search_matches.clear();
            }
            _ => {}
        }

        Ok(())
    }

    /// Handle keys in two phases: cursor mode -> selection mode
    ///
    /// Phase 1 (cursor mode): visual_cursor is Some, visual_anchor is None.
    ///   - hjkl/wbe move the cursor. Press v to anchor and start selecting.
    /// Phase 2 (selection mode): both visual_cursor and visual_anchor are Some.
    ///   - hjkl/wbe extend the selection. Press y to yank, d for dictionary, p for Wikipedia.
    fn handle_visual_mode_keys(&mut self, key: KeyEvent, repeat_count: u32) -> eyre::Result<()> {
        let has_anchor = self.state.borrow().ui_state.visual_anchor.is_some();

        match key.code {
            KeyCode::Esc => {
                let mut state = self.state.borrow_mut();
                if has_anchor {
                    // In selection mode: go back to cursor mode
                    state.ui_state.visual_anchor = None;
                } else {
                    // In cursor mode: exit to reader
                    state.ui_state.open_window(WindowType::Reader);
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
            KeyCode::Char('y') if has_anchor => {
                self.yank_selection()?;
            }
            KeyCode::Char('d') if has_anchor => {
                self.dictionary_lookup()?;
            }
            KeyCode::Char('p') if has_anchor => {
                self.wikipedia_lookup()?;
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
            _ => {}
        }
        Ok(())
    }

    /// Handle common list navigation keys (Esc/q to close, j/k to move selection).
    /// Returns `true` if the key was consumed, `false` if it should be handled by the caller.
    fn handle_list_nav(
        &self,
        key: &KeyEvent,
        repeat_count: u32,
        list_len: usize,
        index: &mut usize,
    ) -> bool {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => {
                self.state.borrow_mut().ui_state.open_window(WindowType::Reader);
                true
            }
            KeyCode::Char('j') | KeyCode::Down => {
                if list_len > 0 {
                    *index = index.saturating_add(repeat_count as usize).min(list_len - 1);
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

    fn handle_toc_mode_keys(&mut self, key: KeyEvent, repeat_count: u32) -> eyre::Result<()> {
        let (list_len, mut index) = {
            let s = self.state.borrow();
            (s.ui_state.toc_entries.len(), s.ui_state.toc_selected_index)
        };
        if !self.handle_list_nav(&key, repeat_count, list_len, &mut index) {
            match key.code {
                KeyCode::Enter => { self.jump_to_toc_entry()?; }
                _ => {}
            }
        } else {
            self.state.borrow_mut().ui_state.toc_selected_index = index;
        }
        Ok(())
    }

    fn handle_bookmarks_mode_keys(&mut self, key: KeyEvent, repeat_count: u32) -> eyre::Result<()> {
        let (list_len, mut index) = {
            let s = self.state.borrow();
            (s.ui_state.bookmarks.len(), s.ui_state.bookmarks_selected_index)
        };
        if !self.handle_list_nav(&key, repeat_count, list_len, &mut index) {
            match key.code {
                KeyCode::Char('a') => { self.add_bookmark()?; }
                KeyCode::Char('d') => { self.delete_selected_bookmark()?; }
                KeyCode::Enter => { self.jump_to_selected_bookmark()?; }
                _ => {}
            }
        } else {
            self.state.borrow_mut().ui_state.bookmarks_selected_index = index;
        }
        Ok(())
    }

    fn handle_links_mode_keys(&mut self, key: KeyEvent, repeat_count: u32) -> eyre::Result<()> {
        let (list_len, mut index) = {
            let s = self.state.borrow();
            (s.ui_state.links.len(), s.ui_state.links_selected_index)
        };
        if !self.handle_list_nav(&key, repeat_count, list_len, &mut index) {
            match key.code {
                KeyCode::Enter => { self.follow_selected_link()?; }
                KeyCode::Char('y') => { self.copy_selected_link()?; }
                _ => {}
            }
        } else {
            self.state.borrow_mut().ui_state.links_selected_index = index;
        }
        Ok(())
    }

    fn handle_images_mode_keys(&mut self, key: KeyEvent, repeat_count: u32) -> eyre::Result<()> {
        let (list_len, mut index) = {
            let s = self.state.borrow();
            (s.ui_state.images_list.len(), s.ui_state.images_selected_index)
        };
        if !self.handle_list_nav(&key, repeat_count, list_len, &mut index) {
            match key.code {
                KeyCode::Enter => { self.open_selected_image()?; }
                _ => {}
            }
        } else {
            self.state.borrow_mut().ui_state.images_selected_index = index;
        }
        Ok(())
    }

    fn handle_library_mode_keys(&mut self, key: KeyEvent, repeat_count: u32) -> eyre::Result<()> {
        let (list_len, mut index) = {
            let s = self.state.borrow();
            (s.ui_state.library_items.len(), s.ui_state.library_selected_index)
        };
        if !self.handle_list_nav(&key, repeat_count, list_len, &mut index) {
            match key.code {
                KeyCode::Char('d') => { self.delete_selected_library_item()?; }
                KeyCode::Enter => { self.open_selected_library_item()?; }
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
                self.reset_selected_setting();
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

    /// Static render method that can be called from a closure
    fn render_static(
        frame: &mut Frame,
        state: &ApplicationState,
        board: &Board,
        content_start_rows: &[usize],
    ) {
        // Main reader view
        Self::render_reader_static(frame, state, board, content_start_rows);

        // Render overlays/modals if active
        if state.ui_state.show_help {
            HelpWindow::render(frame, frame.area(), state.ui_state.help_scroll_offset);
        } else if state.ui_state.show_toc {
            TocWindow::render(
                frame,
                frame.area(),
                &state.ui_state.toc_entries,
                state.ui_state.toc_selected_index,
                state.ui_state.metadata.as_ref(),
            );
        } else if state.ui_state.show_bookmarks {
            let entries: Vec<String> = state
                .ui_state
                .bookmarks
                .iter()
                .map(|(name, reading_state)| format!("{} (line {})", name, reading_state.row + 1))
                .collect();
            BookmarksWindow::render(
                frame,
                frame.area(),
                &entries,
                state.ui_state.bookmarks_selected_index,
                None,
            );
        } else if state.ui_state.show_library {
            let entries: Vec<String> = state
                .ui_state
                .library_items
                .iter()
                .map(Self::format_library_item)
                .collect();
            LibraryWindow::render(
                frame,
                frame.area(),
                &entries,
                state.ui_state.library_selected_index,
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
            );
        } else if state.ui_state.show_links {
            LinksWindow::render(
                frame,
                frame.area(),
                &state.ui_state.links,
                state.ui_state.links_selected_index,
                board,
            );
        } else if state.ui_state.show_images {
            ImagesWindow::render(
                frame,
                frame.area(),
                &state.ui_state.images_list,
                state.ui_state.images_selected_index,
            );
        } else if state.ui_state.show_dictionary {
            DictionaryWindow::render(
                frame,
                frame.area(),
                &state.ui_state.dictionary_word,
                &state.ui_state.dictionary_definition,
                state.ui_state.dictionary_scroll_offset,
                state.ui_state.dictionary_loading,
                state.ui_state.dictionary_is_wikipedia,
            );
        } else if state.ui_state.show_metadata {
            MetadataWindow::render(frame, frame.area(), state.ui_state.metadata.as_ref());
        } else if state.ui_state.active_window == WindowType::DictionaryCommandInput {
            Self::render_dictionary_command_input_static(frame, state);
        } else if state.ui_state.show_settings {
            let entries = Self::settings_entries(state);
            SettingsWindow::render(
                frame,
                frame.area(),
                &entries,
                state.ui_state.settings_selected_index,
            );
        }

        // Render message if present
        if let Some(ref message) = state.ui_state.message {
            Self::render_message_static(frame, message, &state.ui_state.message_type);
        }
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
                SettingItem::StartWithDoubleSpread => {
                    format!(
                        "Start with double spread: {}",
                        settings.start_with_double_spread
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
                    let engine = settings
                        .preferred_tts_engine
                        .as_deref()
                        .unwrap_or("");
                    if engine.is_empty() {
                        "TTS engine: edge-playback (default)".to_string()
                    } else {
                        format!("TTS engine: {engine}")
                    }
                }
                SettingItem::Width => format!("Text width: {}", state.reading_state.textwidth),
                SettingItem::ShowTopBar => format!("Show top bar: {}", settings.show_top_bar),
            })
            .collect()
    }

    /// Static method to render the main reader view
    fn render_reader_static(
        frame: &mut Frame,
        state: &ApplicationState,
        board: &Board,
        content_start_rows: &[usize],
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
        let available_width = chunks[2].width as usize;
        let padding = if available_width <= 20 {
            0
        } else {
            (available_width.saturating_sub(state.reading_state.textwidth) / 2).max(5)
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
        let right_text = match (mode_hint, link_hint, progress_text) {
            (Some(mode), Some(link_hint), Some(progress_text)) => {
                Some(format!("{} {} {}", mode, link_hint, progress_text))
            }
            (Some(mode), Some(link_hint), None) => Some(format!("{} {}", mode, link_hint)),
            (Some(mode), None, Some(progress_text)) => {
                Some(format!("{} {}", mode, progress_text))
            }
            (Some(mode), None, None) => Some(mode),
            (None, Some(link_hint), Some(progress_text)) => {
                Some(format!("{} {}", link_hint, progress_text))
            }
            (None, Some(link_hint), None) => Some(link_hint),
            (None, None, Some(progress_text)) => Some(progress_text),
            (None, None, None) => None,
        };
        if show_top_bar {
            let header_line =
                Self::build_header_line(title, right_text.as_deref(), chunks[0].width);
            let header = Paragraph::new(Line::from(header_line));
            frame.render_widget(header, chunks[0]);
        }

        board.render(frame, content_area, state, Some(content_start_rows));
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

    fn render_message_static(frame: &mut Frame, message: &str, message_type: &MessageType) {
        let color = match message_type {
            MessageType::Info => Color::Blue,
            MessageType::Warning => Color::Yellow,
            MessageType::Error => Color::Red,
        };

        let message_paragraph = Paragraph::new(message)
            .style(Style::default().fg(color))
            .block(Block::default().borders(Borders::ALL))
            .wrap(Wrap { trim: true });

        let frame_area = frame.area();
        let area = Rect {
            x: frame_area.x + 2,
            y: frame_area.y + 2,
            width: frame_area.width - 4,
            height: 3,
        };

        frame.render_widget(Clear, area);
        frame.render_widget(message_paragraph, area);
    }

    fn render_dictionary_command_input_static(frame: &mut Frame, state: &ApplicationState) {
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
                    .border_style(Style::default().fg(Color::Blue)),
            );

        frame.render_widget(Clear, area);
        frame.render_widget(input, area);

        // Set cursor position
        frame.set_cursor_position((
            area.x + state.ui_state.dictionary_command_query.len() as u16 + 1,
            area.y + 1,
        ));
    }

    fn open_toc_window(&mut self) -> eyre::Result<()> {
        let toc_entries = if let Some(epub) = self.ebook.as_ref() {
            epub.toc_entries().clone()
        } else {
            Vec::new()
        };

        let current_row = self.state.borrow().reading_state.row;
        let mut selected_index = 0;

        for (i, entry) in toc_entries.iter().enumerate() {
            let mut target_row = None;

            // Try to resolve row from section ID
            if let Some(section_id) = &entry.section
                && let Some(section_rows) = self.board.section_rows()
                && let Some(row) = section_rows.get(section_id)
            {
                target_row = Some(*row);
            }

            // Fallback to content index
            if target_row.is_none()
                && let Some(row) = self.content_start_rows.get(entry.content_index)
            {
                target_row = Some(*row);
            }

            if let Some(row) = target_row
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

    fn chapter_rows(&self) -> Vec<usize> {
        let section_rows = self.board.section_rows();
        let state = self.state.borrow();
        let mut rows = Vec::new();
        for entry in &state.ui_state.toc_entries {
            // First try to use section ID if available
            if let Some(section) = entry.section.as_ref() {
                if let Some(section_rows) = section_rows
                    && let Some(row) = section_rows.get(section)
                {
                    rows.push(*row);
                }
            } else if entry.content_index < self.content_start_rows.len() {
                // Fall back to using content file index
                rows.push(self.content_start_rows[entry.content_index]);
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

        let mut results = Vec::new();
        let mut matches_map: HashMap<usize, Vec<(usize, usize)>> = HashMap::new();

        if let Some(lines) = self.board.lines() {
            for (line_index, line) in lines.iter().enumerate() {
                let mut ranges = Vec::new();
                for mat in regex.find_iter(line) {
                    ranges.push((mat.start(), mat.end()));
                }
                if !ranges.is_empty() {
                    let preview = line.trim().to_string();
                    results.push(SearchResult {
                        line: line_index,
                        ranges: ranges.clone(),
                        preview,
                    });
                    matches_map.insert(line_index, ranges);
                }
            }
        }

        let mut state = self.state.borrow_mut();
        state.ui_state.search_results = results;
        state.ui_state.search_matches = matches_map;
        state.ui_state.selected_search_result = 0;

        let first_line = state.ui_state.search_results.first().map(|r| r.line);
        if let Some(line) = first_line {
            state.reading_state.row = line;
        } else {
            state
                .ui_state
                .set_message("No matches found".to_string(), MessageType::Info);
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
        let line = state.ui_state.search_results.get(prev).map(|r| r.line);
        if let Some(line) = line {
            state.reading_state.row = line;
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
                .toc_entries
                .get(state.ui_state.toc_selected_index)
            {
                (entry.section.clone(), entry.content_index)
            } else {
                return Ok(());
            }
        };

        let mut target_row = None;
        if let Some(section_id) = section.as_ref()
            && let Some(section_rows) = self.board.section_rows()
            && let Some(row) = section_rows.get(section_id)
        {
            target_row = Some(*row);
        }

        if target_row.is_none()
            && let Some(row) = self.content_start_rows.get(content_index)
        {
            target_row = Some(*row);
        }

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
                .bookmarks
                .get(state.ui_state.bookmarks_selected_index)
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
                .bookmarks
                .get(state.ui_state.bookmarks_selected_index)
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
                .library_items
                .get(state.ui_state.library_selected_index)
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
                .library_items
                .get(state.ui_state.library_selected_index)
                .map(|item| item.filepath.clone())
        };
        if let Some(path) = filepath {
            if std::path::Path::new(&path).exists() {
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
            }
            SettingItem::PageScrollAnimation => {
                state.config.settings.page_scroll_animation =
                    !state.config.settings.page_scroll_animation;
            }
            SettingItem::ShowProgressIndicator => {
                state.config.settings.show_progress_indicator =
                    !state.config.settings.show_progress_indicator;
            }
            SettingItem::StartWithDoubleSpread => {
                state.config.settings.start_with_double_spread =
                    !state.config.settings.start_with_double_spread;
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
                let current_ref = if current.is_empty() {
                    "edge-playback"
                } else {
                    &current
                };
                use crate::settings::TTS_PRESET_LIST;
                let options: Vec<&str> = TTS_PRESET_LIST.to_vec();
                let current_index = options.iter().position(|v| *v == current_ref).unwrap_or(0);
                let next_index = (current_index + 1) % options.len();
                state.config.settings.preferred_tts_engine =
                    Some(options[next_index].to_string());
            }
            SettingItem::Width => {
                state.config.settings.width = if state.config.settings.width.is_some() {
                    None
                } else {
                    Some(70)
                };
                // Set textwidth to the configured value
                let textwidth = state.config.settings.width.unwrap_or(70);
                let _ = state.config.save();
                drop(state);
                self.rebuild_text_structure_with_textwidth(textwidth)?;
                return Ok(());
            }
            SettingItem::ShowTopBar => {
                state.config.settings.show_top_bar = !state.config.settings.show_top_bar;
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

    fn reset_selected_setting(&mut self) {
        let selected = {
            let state = self.state.borrow();
            SettingItem::all()
                .get(state.ui_state.settings_selected_index)
                .copied()
        };

        if selected != Some(SettingItem::DictionaryClient) {
            return;
        }

        let mut state = self.state.borrow_mut();
        state.config.settings.dictionary_client = "auto".to_string();
        let _ = state.config.save();
        state.ui_state.set_message(
            "Dictionary client reset to auto".to_string(),
            MessageType::Info,
        );
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
            self.clipboard.set_text(selected_text)?;
            let ui_state = &mut self.state.borrow_mut().ui_state;
            ui_state.set_message("Text copied to clipboard".to_string(), MessageType::Info);
        }
        self.state
            .borrow_mut()
            .ui_state
            .open_window(WindowType::Reader);
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
                Err("No dictionary program found (install sdcv, dict, or wkdict)".to_string())
            };

            let _ = tx.send(DictionaryResult {
                word: word_clone,
                definition: result_definition,
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
            });
        });

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
            self.clipboard.set_text(url)?;
            let ui_state = &mut self.state.borrow_mut().ui_state;
            ui_state.set_message("Link copied to clipboard".to_string(), MessageType::Info);
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
            self.record_jump_position();
            let mut state = self.state.borrow_mut();
            state.reading_state.row = target_row;
            if let Some(content_index) = self.content_index_for_row(target_row) {
                state.reading_state.content_index = content_index;
            }
            state.ui_state.open_window(WindowType::Reader);
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
                    self.clipboard.set_text(link.url)?;
                    let ui_state = &mut self.state.borrow_mut().ui_state;
                    ui_state.set_message(
                        "Failed to open; link copied".to_string(),
                        MessageType::Warning,
                    );
                    ui_state.open_window(WindowType::Reader);
                    return Ok(());
                }
            }
        }

        self.clipboard.set_text(link.url)?;
        let ui_state = &mut self.state.borrow_mut().ui_state;
        ui_state.set_message("Link copied to clipboard".to_string(), MessageType::Info);
        ui_state.open_window(WindowType::Reader);
        Ok(())
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

            let sentence_chunks = Self::split_into_sentence_chunks(&full_text, 300, 400);

            let mut byte_cursor = 0usize;
            for chunk_text in sentence_chunks {
                // Advance cursor past inter-chunk whitespace
                while byte_cursor < full_text.len() {
                    if full_text[byte_cursor..].starts_with(chunk_text.as_str()) {
                        break;
                    }
                    byte_cursor += 1;
                }
                let chunk_byte_start = byte_cursor;
                let chunk_byte_end = byte_cursor + chunk_text.len();
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
                    let overlap_byte_start = chunk_byte_start.max(line_byte_start) - line_byte_start;
                    let overlap_byte_end = chunk_byte_end.min(line_byte_end) - line_byte_start;

                    // Convert byte offsets to character offsets
                    let col_start = line_text[..overlap_byte_start].chars().count();
                    let col_end = line_text[..overlap_byte_end].chars().count();

                    if col_start < col_end {
                        underline.insert(para_start + li, (col_start, col_end));
                    }
                }

                chunks.push(TtsChunk {
                    text: chunk_text,
                    first_line,
                    underline,
                });
            }
        }
        chunks
    }

    /// Check if a period at position `i` in `chars` is a real sentence end.
    /// Filters out abbreviations like "L.", "Mr.", "Dr.", "St.", "e.g.", etc.
    fn is_sentence_end(chars: &[char], i: usize) -> bool {
        let ch = chars[i];
        // ? ! ; are almost always sentence endings
        if matches!(ch, '?' | '!' | ';') {
            return i + 1 >= chars.len() || chars[i + 1].is_whitespace();
        }
        if ch != '.' {
            return false;
        }
        // Must be followed by whitespace or end of text
        if i + 1 < chars.len() && !chars[i + 1].is_whitespace() {
            return false;
        }
        // Walk back to find the word before the period
        let mut j = i;
        while j > 0 && chars[j - 1].is_alphabetic() {
            j -= 1;
        }
        let word_len = i - j;
        // Single letter before period → likely an initial (L. , M. , etc.)
        if word_len <= 1 {
            return false;
        }
        // Check for common abbreviations (case-insensitive)
        let word: String = chars[j..i].iter().collect::<String>().to_lowercase();
        let abbrevs = [
            "mr", "mrs", "ms", "dr", "st", "sr", "jr", "prof", "gen", "gov",
            "sgt", "cpl", "pvt", "lt", "col", "maj", "capt", "cmdr", "adm",
            "rev", "hon", "pres", "vs", "etc", "approx", "dept", "est",
            "vol", "fig", "inc", "corp", "ltd", "no",
        ];
        if abbrevs.contains(&word.as_str()) {
            return false;
        }
        true
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
                let s: String = chars[chunk_start..].iter().collect::<String>().trim().to_string();
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
                if Self::is_sentence_end(&chars, i) {
                    split_at = Some(i + 1);
                }
            }

            // If none found, scan forward past max_len
            if split_at.is_none() {
                for i in search_end..chars.len() {
                    if Self::is_sentence_end(&chars, i) {
                        split_at = Some(i + 1);
                        break;
                    }
                }
            }

            let end = split_at.unwrap_or(chars.len());
            let chunk: String = chars[chunk_start..end].iter().collect::<String>().trim().to_string();
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

    /// Toggle TTS: start if not active, stop if active.
    fn toggle_tts(&mut self) -> eyre::Result<()> {
        if self.state.borrow().ui_state.tts_active {
            self.stop_tts();
            return Ok(());
        }
        self.tts_chunks = self.build_tts_chunks();
        let current_row = self.state.borrow().reading_state.row;
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
        let last_line = chunk
            .underline
            .keys()
            .max()
            .copied()
            .unwrap_or(first_line);
        let underline = chunk.underline.clone();

        // Update UI state: mark active, set underline ranges, scroll
        {
            let mut state = self.state.borrow_mut();
            state.ui_state.tts_active = true;
            state.ui_state.tts_underline_ranges = underline;

            // Smart scroll: only scroll if the chunk isn't fully visible.
            // Scroll just enough so the chunk fits, or until its first line
            // hits the top — whichever comes first.
            let current_top = state.reading_state.row.saturating_sub(1);
            // Compute the real content height: terminal rows minus the
            // layout chrome (top bar + gaps).
            let term_rows = match crossterm::terminal::size() {
                Ok((_, rows)) => rows as usize,
                Err(_) => 24,
            };
            let chrome = if state.config.settings.show_top_bar {
                1 + 2 + 2 // top_bar + top_gap + bottom_gap
            } else {
                2 // bottom_gap only
            };
            let page_height = term_rows.saturating_sub(chrome).max(1);
            let current_bottom = current_top + page_height;

            if first_line >= current_top && last_line < current_bottom {
                // Chunk is entirely visible — don't scroll
            } else {
                // Need to scroll.  Ideal: put last_line at the bottom.
                // new_top = last_line - page_height + 2  (so last_line is the
                // last visible line).  But never scroll past first_line to top.
                let top_to_show_bottom = (last_line + 2).saturating_sub(page_height);
                let new_top = top_to_show_bottom.max(current_top).min(first_line);
                state.reading_state.row = new_top.saturating_add(1);
            }
        }

        // Redraw the screen so the scroll + underline are visible
        // before the TTS process starts speaking.
        {
            let state = self.state.clone();
            self.terminal.draw(|f| {
                let state_ref = state.borrow();
                Self::render_static(f, &state_ref, &self.board, &self.content_start_rows);
            })?;
        }

        // Build command
        let engine = {
            let state = self.state.borrow();
            state
                .config
                .settings
                .preferred_tts_engine
                .clone()
                .unwrap_or_default()
        };

        let (program, args) = if engine.is_empty() || engine == "edge-playback" {
            (
                "edge-playback".to_string(),
                vec!["--text".to_string(), text],
            )
        } else if engine == "espeak" {
            ("espeak".to_string(), vec![text])
        } else if engine == "say" {
            ("say".to_string(), vec![text])
        } else if engine.contains("{}") {
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

        // Spawn TTS process in its own process group so we can kill all
        // its children (e.g. mpv spawned by edge-playback).
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
                state.ui_state.set_message(
                    format!("TTS failed: {err}"),
                    MessageType::Error,
                );
            }
        }

        Ok(())
    }

    /// Advance to the next chunk after the current one finishes.
    fn tts_advance_paragraph(&mut self) -> eyre::Result<()> {
        self.tts_chunk_index += 1;
        if self.tts_chunk_index >= self.tts_chunks.len() {
            self.stop_tts();
            let mut state = self.state.borrow_mut();
            state
                .ui_state
                .set_message("TTS finished".to_string(), MessageType::Info);
            return Ok(());
        }
        self.tts_speak_current()
    }

    /// Stop TTS playback — kill the entire process group.
    fn stop_tts(&mut self) {
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
        self.tts_chunks.clear();
        self.tts_chunk_index = 0;
        let mut state = self.state.borrow_mut();
        state.ui_state.tts_active = false;
        state.ui_state.tts_underline_ranges.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::{Reader, WikipediaSearchResponse, WikipediaSummaryResponse};
    use std::io::{BufRead, BufReader, Write};
    use std::net::{TcpListener, TcpStream};
    use std::thread;
    use std::time::Duration;

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
    fn resolve_relative_href_joins_base_dir() {
        let resolved =
            Reader::resolve_relative_href("chapter007.xhtml", Some("OEBPS/Text/chapter001.xhtml"));
        assert_eq!(resolved, Some("OEBPS/Text/chapter007.xhtml".to_string()));
    }

    #[test]
    fn resolve_relative_href_handles_parent_dirs() {
        let resolved = Reader::resolve_relative_href(
            "../Images/cover.jpg",
            Some("OEBPS/Text/chapter001.xhtml"),
        );
        assert_eq!(resolved, Some("OEBPS/Images/cover.jpg".to_string()));
    }

    #[test]
    fn resolve_relative_href_strips_leading_slash() {
        let resolved = Reader::resolve_relative_href("/Text/chapter007.xhtml", None);
        assert_eq!(resolved, Some("Text/chapter007.xhtml".to_string()));
    }

    #[test]
    fn build_dictionary_command_replaces_placeholder() {
        let (program, args) = Reader::build_dictionary_command("dict -wn \"%q\"", "apple").unwrap();
        assert_eq!(program, "dict");
        assert_eq!(args, vec!["-wn".to_string(), "apple".to_string()]);
    }

    #[test]
    fn build_dictionary_command_appends_query_without_placeholder() {
        let (program, args) = Reader::build_dictionary_command("dict -wn", "apple").unwrap();
        assert_eq!(program, "dict");
        assert_eq!(args, vec!["-wn".to_string(), "apple".to_string()]);
    }

    #[test]
    fn build_dictionary_command_handles_internal_quotes_in_query() {
        // Current behavior: if query contains quotes, they are passed as part of the argument.
        // This is safe because we don't use shell=True.
        let (program, args) =
            Reader::build_dictionary_command("tool --arg=%q", "word \"with\" quotes").unwrap();
        assert_eq!(program, "tool");
        assert_eq!(args, vec!["--arg=word \"with\" quotes".to_string()]);
    }

    #[test]
    fn build_dictionary_command_escapes_quotes_if_wrapped_in_template() {
        let (program, args) =
            Reader::build_dictionary_command("sh -c \"dict %q\"", "a\"b").unwrap();
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
        let result = Reader::parse_wikipedia_summary_response(&parsed, "simple", "Rust")
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
            Reader::parse_wikipedia_summary_response(&parsed, "simple", "NoSuchTerm").unwrap();
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
        let titles = Reader::extract_search_titles(parsed);
        assert_eq!(
            titles,
            vec![
                "Rust_(programming_language)".to_string(),
                "Rust_(fungus)".to_string()
            ]
        );
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

        let result = Reader::wikipedia_lookup_summary("Rust", &base, Duration::from_secs(2))
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

        let result = Reader::wikipedia_lookup_summary("NoSuchTerm", &base, Duration::from_secs(2))
            .expect("fallback lookup should succeed");
        server.join().unwrap();

        assert_eq!(
            result.url,
            "https://simple.wikipedia.org/wiki/Rust_(programming_language)"
        );
        assert!(result.summary.contains("focused on safety"));
    }
}
