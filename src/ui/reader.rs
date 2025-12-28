use arboard::Clipboard;
use regex::Regex;
use std::cell::RefCell;
use std::collections::HashMap;
use std::io;
use std::rc::Rc;

use chrono::Local;
use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Layout, Rect},
    style::{Color, Style},
    text::Line,
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
    Frame, Terminal,
};

use crate::config::Config;
use crate::ebook::{build_chapter_break, Ebook, Epub};
use crate::models::{
    BookMetadata, Direction as AppDirection, LibraryItem, LinkEntry, ReadingState, SearchData,
    TextStructure, TocEntry, WindowType,
};
use crate::state::State;
use crate::ui::board::Board;
use crate::ui::windows::{
    bookmarks::BookmarksWindow, help::HelpWindow, images::ImagesWindow, library::LibraryWindow,
    links::LinksWindow, metadata::MetadataWindow, search::SearchWindow, settings::SettingsWindow,
    toc::TocWindow,
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
    pub settings_selected_index: usize,
    pub message: Option<String>,
    pub message_type: MessageType,
    pub selection_start: Option<usize>,
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
            settings_selected_index: 0,
            message: None,
            message_type: MessageType::Info,
            selection_start: None,
        }
    }

    pub fn set_message(&mut self, message: String, message_type: MessageType) {
        self.message = Some(message);
        self.message_type = message_type;
    }

    pub fn clear_message(&mut self) {
        self.message = None;
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
                self.show_settings = false;
                self.selection_start = None;
            }
            WindowType::Help => self.show_help = true,
            WindowType::Toc => self.show_toc = true,
            WindowType::Bookmarks => self.show_bookmarks = true,
            WindowType::Library => self.show_library = true,
            WindowType::Search => self.show_search = true,
            WindowType::Links => self.show_links = true,
            WindowType::Images => self.show_images = true,
            WindowType::Metadata => self.show_metadata = true,
            WindowType::Settings => self.show_settings = true,
            WindowType::Visual => {
                let current_row = self.selection_start.unwrap_or(0);
                self.selection_start = Some(current_row);
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

#[derive(Debug, Clone, Copy, PartialEq)]
enum SettingItem {
    ShowLineNumbers,
    MouseSupport,
    PageScrollAnimation,
    ShowProgressIndicator,
    StartWithDoubleSpread,
    SeamlessBetweenChapters,
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
            SettingItem::Width,
            SettingItem::ShowTopBar,
        ]
    }
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
}

impl Reader {
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
        })
    }

    /// Load the most recently read ebook, if any, using the database
    pub fn load_last_ebook_if_any(&mut self) -> eyre::Result<()> {
        if let Some(filepath) = self.db_state.get_last_read()? {
            if std::path::Path::new(&filepath).exists() {
                self.load_ebook(&filepath)?;
            }
        }
        Ok(())
    }

    pub fn load_ebook(&mut self, path: &str) -> eyre::Result<()> {
        let mut epub = Epub::new(path);
        epub.initialize()?;

        // Load last reading state early to get preferred padding
        let db_state = self.db_state.get_last_reading_state(&epub).ok();
        
        // Determine padding: use DB value if available, otherwise calculate from config width
        let term_width = match crossterm::terminal::size() {
            Ok((w, _)) => w as usize,
            Err(_) => 100,
        };

        let padding = if let Some(ref s) = db_state {
             s.padding
        } else {
             let preferred_width = self.state.borrow().config.settings.width.unwrap_or(80);
             (term_width.saturating_sub(preferred_width) / 2).max(5)
        };

        let text_width = term_width.saturating_sub(padding * 2).max(20).min(term_width);
        
        // Also update the state with the decided padding immediately so we are consistent
        if let Some(mut s) = db_state.clone() {
             s.padding = padding;
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
            combined_text_structure.text_lines.extend(ts.text_lines.clone());
            combined_text_structure.image_maps.extend(ts.image_maps.clone());
            combined_text_structure.section_rows.extend(ts.section_rows.clone());
            combined_text_structure.formatting.extend(ts.formatting.clone());
            combined_text_structure.links.extend(ts.links.clone());
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

            // Ensure padding matches what we used
            state.reading_state.padding = padding;
            
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

            // Render UI
            {
                let state = self.state.clone();
                self.terminal.draw(|f| {
                    let state_ref = state.borrow();
                    Self::render_static(f, &state_ref, &self.board, &self.content_start_rows);
                })?;
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
                        let padding = {
                            let state = self.state.borrow();
                            if state.config.settings.seamless_between_chapters {
                                None
                            } else {
                                Some(state.reading_state.padding)
                            }
                        };
                        if let Some(padding) = padding {
                            self.rebuild_text_structure(padding)?;
                        }
                    }
                    _ => {}
                }
            }
        }

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
            if state.ui_state.message.is_some() && state.ui_state.active_window == WindowType::Reader {
                state.ui_state.clear_message();
            }
        }

        // Handle count prefix (number repetition)
        if let KeyCode::Char(c) = key.code {
            if c.is_ascii_digit() {
                let mut state = self.state.borrow_mut();
                if state.count_prefix.len() < 6 {
                    state.count_prefix.push(c);
                }
                return Ok(());
            }
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
            WindowType::Help | WindowType::Metadata => self.handle_modal_close_keys(key)?,
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
            KeyCode::PageDown | KeyCode::Char(' ') | KeyCode::Char('f') => {
                for _ in 0..repeat_count {
                    self.move_cursor(AppDirection::PageDown);
                }
            }
            KeyCode::PageUp | KeyCode::Char('b') => {
                for _ in 0..repeat_count {
                    self.move_cursor(AppDirection::PageUp);
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
                {
                    let mut state = self.state.borrow_mut();
                    state.search_data = Some(SearchData::default());
                    state.ui_state.search_query.clear();
                    state.ui_state.search_results.clear();
                    state.ui_state.search_matches.clear();
                    state.ui_state.open_window(WindowType::Search);
                }
            }

            // Visual Mode
            KeyCode::Char('v') => {
                let mut state = self.state.borrow_mut();
                state.ui_state.open_window(WindowType::Visual);
            }

            // Windows
            KeyCode::Char('q') => {
                {
                    let mut state = self.state.borrow_mut();
                    if state.ui_state.active_window != WindowType::Reader {
                        state.ui_state.open_window(WindowType::Reader);
                    } else {
                        state.should_quit = true;
                    }
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
                self.change_padding(-5)?;
            }
            KeyCode::Char('=') => {
                if key.modifiers.contains(KeyModifiers::SHIFT) {
                    self.change_padding(-5)?;
                } else {
                    self.reset_width()?;
                }
            }
            KeyCode::Char('-') => {
                self.change_padding(5)?;
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
                    if let Some(result) = state.ui_state.search_results.get(next) {
                        state.reading_state.row = result.line;
                    }
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                let mut state = self.state.borrow_mut();
                if !state.ui_state.search_results.is_empty() {
                    let current = state.ui_state.selected_search_result;
                    state.ui_state.selected_search_result = current.saturating_sub(1);
                    if let Some(result) = state.ui_state.search_results.get(state.ui_state.selected_search_result) {
                        state.reading_state.row = result.line;
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

    /// Handle keys in visual mode
    fn handle_visual_mode_keys(&mut self, key: KeyEvent, repeat_count: u32) -> eyre::Result<()> {
        match key.code {
            KeyCode::Esc => {
                let mut state = self.state.borrow_mut();
                state.ui_state.open_window(WindowType::Reader);
            }
            KeyCode::Char('y') => {
                self.yank_selection()?;
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
                    self.move_cursor(AppDirection::Left);
                }
            }
            KeyCode::Char('l') | KeyCode::Right => {
                for _ in 0..repeat_count {
                    self.move_cursor(AppDirection::Right);
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn handle_toc_mode_keys(&mut self, key: KeyEvent, repeat_count: u32) -> eyre::Result<()> {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => {
                let mut state = self.state.borrow_mut();
                state.ui_state.open_window(WindowType::Reader);
            }
            KeyCode::Char('j') | KeyCode::Down => {
                let mut state = self.state.borrow_mut();
                if !state.ui_state.toc_entries.is_empty() {
                    let next = state.ui_state.toc_selected_index.saturating_add(repeat_count as usize);
                    state.ui_state.toc_selected_index = next.min(state.ui_state.toc_entries.len() - 1);
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                let mut state = self.state.borrow_mut();
                let current = state.ui_state.toc_selected_index;
                state.ui_state.toc_selected_index = current.saturating_sub(repeat_count as usize);
            }
            KeyCode::Enter => {
                self.jump_to_toc_entry()?;
            }
            _ => {}
        }
        Ok(())
    }

    fn handle_bookmarks_mode_keys(&mut self, key: KeyEvent, repeat_count: u32) -> eyre::Result<()> {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => {
                let mut state = self.state.borrow_mut();
                state.ui_state.open_window(WindowType::Reader);
            }
            KeyCode::Char('j') | KeyCode::Down => {
                let mut state = self.state.borrow_mut();
                if !state.ui_state.bookmarks.is_empty() {
                    let next = state.ui_state.bookmarks_selected_index.saturating_add(repeat_count as usize);
                    state.ui_state.bookmarks_selected_index = next.min(state.ui_state.bookmarks.len() - 1);
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                let mut state = self.state.borrow_mut();
                let current = state.ui_state.bookmarks_selected_index;
                state.ui_state.bookmarks_selected_index = current.saturating_sub(repeat_count as usize);
            }
            KeyCode::Char('a') => {
                self.add_bookmark()?;
            }
            KeyCode::Char('d') => {
                self.delete_selected_bookmark()?;
            }
            KeyCode::Enter => {
                self.jump_to_selected_bookmark()?;
            }
            _ => {}
        }
        Ok(())
    }

    fn handle_links_mode_keys(&mut self, key: KeyEvent, repeat_count: u32) -> eyre::Result<()> {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => {
                let mut state = self.state.borrow_mut();
                state.ui_state.open_window(WindowType::Reader);
            }
            KeyCode::Char('j') | KeyCode::Down => {
                let mut state = self.state.borrow_mut();
                if !state.ui_state.links.is_empty() {
                    let next = state.ui_state.links_selected_index.saturating_add(repeat_count as usize);
                    state.ui_state.links_selected_index = next.min(state.ui_state.links.len() - 1);
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                let mut state = self.state.borrow_mut();
                let current = state.ui_state.links_selected_index;
                state.ui_state.links_selected_index = current.saturating_sub(repeat_count as usize);
            }
            KeyCode::Enter => {
                self.follow_selected_link()?;
            }
            KeyCode::Char('y') => {
                self.copy_selected_link()?;
            }
            _ => {}
        }
        Ok(())
    }

    fn handle_images_mode_keys(&mut self, key: KeyEvent, repeat_count: u32) -> eyre::Result<()> {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => {
                let mut state = self.state.borrow_mut();
                state.ui_state.open_window(WindowType::Reader);
            }
            KeyCode::Char('j') | KeyCode::Down => {
                let mut state = self.state.borrow_mut();
                if !state.ui_state.images_list.is_empty() {
                    let next = state.ui_state.images_selected_index.saturating_add(repeat_count as usize);
                    state.ui_state.images_selected_index = next.min(state.ui_state.images_list.len() - 1);
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                let mut state = self.state.borrow_mut();
                let current = state.ui_state.images_selected_index;
                state.ui_state.images_selected_index = current.saturating_sub(repeat_count as usize);
            }
            KeyCode::Enter => {
                self.open_selected_image()?;
            }
            _ => {}
        }
        Ok(())
    }


    fn handle_library_mode_keys(&mut self, key: KeyEvent, repeat_count: u32) -> eyre::Result<()> {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => {
                let mut state = self.state.borrow_mut();
                state.ui_state.open_window(WindowType::Reader);
            }
            KeyCode::Char('j') | KeyCode::Down => {
                let mut state = self.state.borrow_mut();
                if !state.ui_state.library_items.is_empty() {
                    let next = state.ui_state.library_selected_index.saturating_add(repeat_count as usize);
                    state.ui_state.library_selected_index = next.min(state.ui_state.library_items.len() - 1);
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                let mut state = self.state.borrow_mut();
                let current = state.ui_state.library_selected_index;
                state.ui_state.library_selected_index = current.saturating_sub(repeat_count as usize);
            }
            KeyCode::Char('d') => {
                self.delete_selected_library_item()?;
            }
            KeyCode::Enter => {
                self.open_selected_library_item()?;
            }
            _ => {}
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
                let next = state.ui_state.settings_selected_index.saturating_add(repeat_count as usize);
                state.ui_state.settings_selected_index = next.min(max_index);
            }
            KeyCode::Char('k') | KeyCode::Up => {
                let mut state = self.state.borrow_mut();
                state.ui_state.settings_selected_index =
                    state.ui_state.settings_selected_index.saturating_sub(repeat_count as usize);
            }
            KeyCode::Enter => {
                self.toggle_selected_setting()?;
            }
            KeyCode::Char('+') | KeyCode::Char('=') | KeyCode::Right => {
                self.adjust_padding(-5)?;
            }
            KeyCode::Char('-') | KeyCode::Left => {
                self.adjust_padding(5)?;
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

    /// Static render method that can be called from a closure
    fn render_static(
        frame: &mut Frame,
        state: &ApplicationState,
        board: &Board,
        content_start_rows: &[usize],
    ) {
        // Main reader view
        Self::render_reader_static(frame, &state, board, content_start_rows);

        // Render overlays/modals if active
        if state.ui_state.show_help {
            HelpWindow::render(frame, frame.area());
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
                .map(|(name, reading_state)| {
                    format!("{} (line {})", name, reading_state.row + 1)
                })
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
        } else if state.ui_state.show_metadata {
            MetadataWindow::render(frame, frame.area(), state.ui_state.metadata.as_ref());
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

        let book_name = if let (Some(title), Some(author)) = (item.title.as_ref(), item.author.as_ref()) {
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
                SettingItem::ShowLineNumbers => format!("Show line numbers: {}", settings.show_line_numbers),
                SettingItem::MouseSupport => format!("Mouse support: {}", settings.mouse_support),
                SettingItem::PageScrollAnimation => {
                    format!("Page scroll animation: {}", settings.page_scroll_animation)
                }
                SettingItem::ShowProgressIndicator => {
                    format!("Show progress indicator: {}", settings.show_progress_indicator)
                }
                SettingItem::StartWithDoubleSpread => {
                    format!("Start with double spread: {}", settings.start_with_double_spread)
                }
                SettingItem::SeamlessBetweenChapters => {
                    format!("Seamless between chapters: {}", settings.seamless_between_chapters)
                }
                SettingItem::Width => format!(
                    "Padding: {}",
                    settings.width.map(|_| state.reading_state.padding.to_string()).unwrap_or_else(|| "auto".to_string())
                ),
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

        let (chunks, header_height) = if state.config.settings.show_top_bar {
            let chunks = Layout::default()
                .direction(ratatui::layout::Direction::Vertical)
                .constraints([
                    Constraint::Length(1), // Header
                    Constraint::Min(0),    // Main content
                ])
                .split(frame_area);
            (chunks, 1)
        } else {
            let chunks = Layout::default()
                .direction(ratatui::layout::Direction::Vertical)
                .constraints([
                    Constraint::Min(0),    // Main content (no header)
                ])
                .split(frame_area);
            (chunks, 0)
        };

        // Main content area with centered margins
        // Calculate desired width from padding
        let padding = state.reading_state.padding;
        let content_chunk_index = if header_height > 0 { 1 } else { 0 };
        let available_width = chunks[content_chunk_index].width as usize;
        let desired_width = available_width.saturating_sub(padding * 2).max(20) as u16;
        
        let content_width = desired_width.min(chunks[content_chunk_index].width);
        let left_pad = (chunks[content_chunk_index].width.saturating_sub(content_width)) / 2;
        let content_area = Rect {
            x: chunks[content_chunk_index].x + left_pad,
            y: chunks[content_chunk_index].y,
            width: content_width,
            height: chunks[content_chunk_index].height,
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
        let right_text = match (link_hint, percent_text) {
            (Some(link_hint), Some(percent_text)) => Some(format!("{} {}", link_hint, percent_text)),
            (Some(link_hint), None) => Some(link_hint),
            (None, Some(percent_text)) => Some(percent_text),
            (None, None) => None,
        };
        if state.config.settings.show_top_bar {
            let header_line = Self::build_header_line(title, right_text.as_deref(), chunks[0].width);
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

    fn open_toc_window(&mut self) -> eyre::Result<()> {
        let toc_entries = if let Some(epub) = self.ebook.as_ref() {
            epub.toc_entries().clone()
        } else {
            Vec::new()
        };
        let mut state = self.state.borrow_mut();
        state.ui_state.toc_entries = toc_entries;
        state.ui_state.toc_selected_index = 0;
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

            if let Some(target_row) = self.resolve_internal_link_row(&link.url, base_content.as_deref()) {
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

    // Navigation methods (placeholders)
    fn move_cursor(&mut self, direction: AppDirection) {
        let seamless = {
            let state = self.state.borrow();
            state.config.settings.seamless_between_chapters
        };
        let mut state = self.state.borrow_mut();
        let total_lines = self.board.total_lines();
        let current_row = state.reading_state.row;
        let page = self.page_size();

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
                if !seamless {
                    if let Some(index) = self.content_index_for_row(current_row) {
                        if let Some((chapter_start, _chapter_end)) = self.chapter_bounds_for_index(index) {
                            let current_start = current_row.saturating_sub(1);
                            if current_start <= chapter_start {
                                if index > 0 {
                                    if let Some((prev_start, prev_end)) = self.chapter_bounds_for_index(index - 1) {
                                        let last_start = prev_end
                                            .saturating_sub(page.saturating_sub(1))
                                            .max(prev_start);
                                        state.reading_state.row = Self::row_from_start(last_start);
                                        return;
                                    }
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
                    }
                }
                state.reading_state.row = current_row.saturating_sub(page);
            }
            AppDirection::PageDown => {
                if !seamless {
                    if let Some(index) = self.content_index_for_row(current_row) {
                        if let Some((chapter_start, chapter_end)) = self.chapter_bounds_for_index(index) {
                            let current_start = current_row.saturating_sub(1);
                            let last_start = chapter_end
                                .saturating_sub(page.saturating_sub(1))
                                .max(chapter_start);
                            if current_start >= last_start {
                                if let Some(next_start) = self.content_start_rows.get(index + 1).copied() {
                                    state.reading_state.row = Self::row_from_start(
                                        next_start.min(total_lines.saturating_sub(1)),
                                    );
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
                    }
                }
                let next = current_row.saturating_add(page);
                state.reading_state.row = next.min(total_lines.saturating_sub(1));
            }
            _ => {}
        }
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

    fn page_size(&self) -> usize {
        match crossterm::terminal::size() {
            Ok((_cols, rows)) => rows.saturating_sub(1) as usize,
            Err(_) => 0,
        }
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
        let end = start
            .saturating_add(height)
            .min(self.board.total_lines());
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
        if start_line == 0 {
            0
        } else {
            start_line + 1
        }
    }

    fn chapter_rows(&self) -> Vec<usize> {
        let section_rows = self.board.section_rows();
        let state = self.state.borrow();
        let mut rows = Vec::new();
        for entry in &state.ui_state.toc_entries {
            // First try to use section ID if available
            if let Some(section) = entry.section.as_ref() {
                if let Some(section_rows) = section_rows {
                    if let Some(row) = section_rows.get(section) {
                        rows.push(*row);
                    }
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
            state.ui_state.set_message("Search query is empty".to_string(), MessageType::Warning);
            return;
        }

        let regex = match Regex::new(&query) {
            Ok(regex) => regex,
            Err(err) => {
                let mut state = self.state.borrow_mut();
                state.ui_state.set_message(format!("Invalid regex: {}", err), MessageType::Error);
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

        if let Some(first) = state.ui_state.search_results.first() {
            state.reading_state.row = first.line;
        } else {
            state.ui_state.set_message("No matches found".to_string(), MessageType::Info);
        }
    }

    fn search_next(&mut self) {
        let mut state = self.state.borrow_mut();
        if state.ui_state.search_results.is_empty() {
            state.ui_state.set_message("No search results".to_string(), MessageType::Info);
            return;
        }
        let next = (state.ui_state.selected_search_result + 1) % state.ui_state.search_results.len();
        state.ui_state.selected_search_result = next;
        if let Some(result) = state.ui_state.search_results.get(next) {
            state.reading_state.row = result.line;
        }
    }

    fn search_previous(&mut self) {
        let mut state = self.state.borrow_mut();
        if state.ui_state.search_results.is_empty() {
            state.ui_state.set_message("No search results".to_string(), MessageType::Info);
            return;
        }
        let len = state.ui_state.search_results.len();
        let prev = if state.ui_state.selected_search_result == 0 {
            len - 1
        } else {
            state.ui_state.selected_search_result - 1
        };
        state.ui_state.selected_search_result = prev;
        if let Some(result) = state.ui_state.search_results.get(prev) {
            state.reading_state.row = result.line;
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
            if let Some(entry) = state.ui_state.toc_entries.get(state.ui_state.toc_selected_index) {
                (entry.section.clone(), entry.content_index)
            } else {
                return Ok(());
            }
        };

        let mut target_row = None;
        if let Some(section_id) = section.as_ref() {
            if let Some(section_rows) = self.board.section_rows() {
                if let Some(row) = section_rows.get(section_id) {
                    target_row = Some(*row);
                }
            }
        }

        if target_row.is_none() {
            if let Some(row) = self.content_start_rows.get(content_index) {
                target_row = Some(*row);
            }
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
            state.ui_state.set_message("TOC entry not mapped to text".to_string(), MessageType::Warning);
        }
        Ok(())
    }

    fn add_bookmark(&mut self) -> eyre::Result<()> {
        let Some(epub) = self.ebook.as_ref() else {
            let mut state = self.state.borrow_mut();
            state.ui_state.set_message("No book loaded".to_string(), MessageType::Warning);
            return Ok(());
        };
        let bookmark_name = {
            let state = self.state.borrow();
            format!("Bookmark {}", state.ui_state.bookmarks.len() + 1)
        };
        let reading_state = { self.state.borrow().reading_state.clone() };
        self.db_state.insert_bookmark(epub, &bookmark_name, &reading_state)?;
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
                state
                    .ui_state
                    .set_message("Selected file no longer exists".to_string(), MessageType::Warning);
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

        if let Some(src) = image_src {
            if let Some(epub) = self.ebook.as_mut() {
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
                        let temp_path = temp_dir.join(format!("{}_{}.{}", "repy_img", filename, extension));
                        
                        std::fs::write(&temp_path, bytes)?;
                        
                        self.open_image_viewer(&temp_path.to_string_lossy())?;
                        
                        let mut state = self.state.borrow_mut();
                        state.ui_state.set_message("Opened image".to_string(), MessageType::Info);
                        state.ui_state.open_window(WindowType::Reader);
                    }
                    Err(e) => {
                        let mut state = self.state.borrow_mut();
                        state.ui_state.set_message(format!("Failed to load image: {}", e), MessageType::Error);
                    }
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
            let status = std::process::Command::new(viewer)
                .arg(path)
                .status();
            
            if let Ok(status) = status {
                if status.success() {
                    return Ok(true);
                }
            }
        }

        Err(eyre::eyre!("Failed to open image with any available viewer"))
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
                state.config.settings.page_scroll_animation = !state.config.settings.page_scroll_animation;
            }
            SettingItem::ShowProgressIndicator => {
                state.config.settings.show_progress_indicator = !state.config.settings.show_progress_indicator;
            }
            SettingItem::StartWithDoubleSpread => {
                state.config.settings.start_with_double_spread = !state.config.settings.start_with_double_spread;
            }
            SettingItem::SeamlessBetweenChapters => {
                state.config.settings.seamless_between_chapters = !state.config.settings.seamless_between_chapters;
                rebuild_chapter_breaks = true;
            }
            SettingItem::Width => {
                state.config.settings.width = if state.config.settings.width.is_some() {
                    None
                } else {
                    Some(80)
                };
                // Calculate proper padding for the new setting
                let preferred_width = state.config.settings.width.unwrap_or(80);
                drop(state);
                
                let term_width = match crossterm::terminal::size() {
                    Ok((w, _)) => w as usize,
                    Err(_) => 100,
                };
                let padding = (term_width.saturating_sub(preferred_width) / 2).max(5);
                self.rebuild_text_structure(padding)?;
                return Ok(());
            }
            SettingItem::ShowTopBar => {
                state.config.settings.show_top_bar = !state.config.settings.show_top_bar;
            }
        }
        if rebuild_chapter_breaks {
            // Use current padding
            let padding = state.reading_state.padding;
            drop(state);
            self.rebuild_text_structure(padding)?;
        }
        Ok(())
    }

    fn change_padding(&mut self, delta: i32) -> eyre::Result<()> {
        let current_padding = self.state.borrow().reading_state.padding as i32;
        let new_padding = (current_padding + delta).max(0);
        self.rebuild_text_structure(new_padding as usize)?;
        self.persist_state()
    }

    fn reset_width(&mut self) -> eyre::Result<()> {
        let preferred_width = self.state.borrow().config.settings.width.unwrap_or(80);
        let term_width = match crossterm::terminal::size() {
            Ok((w, _)) => w as usize,
            Err(_) => 100,
        };
        // Calculate padding to achieve preferred width
        let padding = (term_width.saturating_sub(preferred_width) / 2).max(5);
        self.rebuild_text_structure(padding)?;
        self.persist_state()
    }

    fn adjust_padding(&mut self, delta_padding: i32) -> eyre::Result<()> {
        let selected = {
            let state = self.state.borrow();
            SettingItem::all()
                .get(state.ui_state.settings_selected_index)
                .copied()
        };
        if selected != Some(SettingItem::Width) {
            return Ok(());
        }
        self.change_padding(delta_padding)
    }

    fn rebuild_text_structure(&mut self, padding: usize) -> eyre::Result<()> {
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

        // Calculate text width from padding
        let text_width = term_width.saturating_sub(padding * 2).max(20).min(term_width);

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

            if current_chapter_idx < self.chapter_text_structures.len() && current_chapter_idx < total_chapters {
                // Clone content_id to avoid holding immutable borrow across mutable call
                let content_id = contents[current_chapter_idx].clone();
                let starting_line = if current_chapter_idx > 0 {
                    self.content_start_rows[current_chapter_idx]
                } else {
                    0
                };

                // Parse only the current chapter with new width
                let mut parsed_chapter = epub.get_parsed_content(&content_id, text_width, starting_line)?;

                // Add chapter break if needed
                if let Some(ph) = page_height {
                    if current_chapter_idx + 1 < total_chapters {
                        let total_lines = starting_line + parsed_chapter.text_lines.len();
                        let break_lines = build_chapter_break(ph, total_lines);
                        parsed_chapter.text_lines.extend(break_lines);
                    }
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
            combined_text_structure.text_lines.extend(ts.text_lines.clone());
            combined_text_structure.image_maps.extend(ts.image_maps.clone());
            combined_text_structure.section_rows.extend(ts.section_rows.clone());
            combined_text_structure.formatting.extend(ts.formatting.clone());
            combined_text_structure.links.extend(ts.links.clone());
        }
        self.board.update_text_structure(combined_text_structure);
        self.content_start_rows = content_start_rows;

        let mut state = self.state.borrow_mut();
        state.reading_state.padding = padding;

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
        let state = self.state.borrow();
        if let Some(selection_start) = state.ui_state.selection_start {
            let selection_end = state.reading_state.row;
            let selected_text = self.board.get_selected_text(selection_start, selection_end);
            self.clipboard.set_text(selected_text)?;
            let ui_state = &mut self.state.borrow_mut().ui_state;
            ui_state.set_message("Text copied to clipboard".to_string(), MessageType::Info);
            ui_state.open_window(WindowType::Reader);
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

        if let Some(target_row) = self.resolve_internal_link_row(&link.url, base_content.as_deref()) {
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
                    ui_state.set_message("Failed to open; link copied".to_string(), MessageType::Warning);
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
        if let Some(fragment) = fragment {
            if !fragment.is_empty() {
                has_fragment = true;
                if let Some(row) = self.resolve_anchor_row(fragment) {
                    return Some(row);
                }
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

            if let Some(resolved_path) = Self::resolve_relative_href(path, base_content) {
                if let Some(content_index) = epub.content_index_for_href(&resolved_path) {
                    if has_fragment {
                        let current_index = self.state.borrow().reading_state.content_index;
                        if content_index == current_index {
                            return None;
                        }
                    }
                    return self.content_start_rows.get(content_index).copied();
                }
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
        let base_dir = base_path.parent().unwrap_or_else(|| std::path::Path::new(""));
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
        let status = std::process::Command::new("xdg-open")
            .arg(url)
            .status();
        match status {
            Ok(status) => Ok(status.success()),
            Err(err) => Err(err.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Reader;

    #[test]
    fn resolve_relative_href_joins_base_dir() {
        let resolved = Reader::resolve_relative_href(
            "chapter007.xhtml",
            Some("OEBPS/Text/chapter001.xhtml"),
        );
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
}
