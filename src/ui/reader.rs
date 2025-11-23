use std::cell::RefCell;
use std::io;
use std::rc::Rc;

use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
    Frame, Terminal,
};

use crate::config::Config;
use crate::models::{
    Direction as AppDirection, ReadingState, SearchData, WindowType,
};
use crate::state::State;

/// Application state that encompasses all UI and reading state
#[derive(Debug, Clone)]
pub struct ApplicationState {
    pub reading_state: ReadingState,
    pub config: Config,
    pub search_data: Option<SearchData>,
    pub ui_state: UiState,
    pub should_quit: bool,
    pub count_prefix: String, // For command repetition (e.g., "5j")
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
    pub show_metadata: bool,
    pub show_settings: bool,
    pub search_query: String,
    pub search_results: Vec<String>,
    pub selected_search_result: usize,
    pub message: Option<String>,
    pub message_type: MessageType,
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
            show_metadata: false,
            show_settings: false,
            search_query: String::new(),
            search_results: Vec::new(),
            selected_search_result: 0,
            message: None,
            message_type: MessageType::Info,
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
                self.show_metadata = false;
                self.show_settings = false;
            }
            WindowType::Help => self.show_help = true,
            WindowType::Toc => self.show_toc = true,
            WindowType::Bookmarks => self.show_bookmarks = true,
            WindowType::Library => self.show_library = true,
            WindowType::Search => self.show_search = true,
            WindowType::Metadata => self.show_metadata = true,
            WindowType::Settings => self.show_settings = true,
        }
    }
}

#[derive(Debug, Clone)]
pub enum MessageType {
    Info,
    Warning,
    Error,
}

/// Main reader application struct
pub struct Reader {
    state: Rc<RefCell<ApplicationState>>,
    terminal: Terminal<CrosstermBackend<io::Stdout>>,
    db_state: State,
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
        })
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
                    Self::render_static(f, &state_ref);
                })?;
            }

            // Handle events
            if let Ok(event) = crossterm::event::read() {
                if let Event::Key(key) = event {
                    if key.kind == KeyEventKind::Press {
                        self.handle_key_event(key)?;
                    }
                }
            }
        }

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
        let has_search_data = {
            let state = self.state.borrow();
            state.search_data.is_some()
        };

        if has_search_data {
            self.handle_search_mode_keys(key, repeat_count)?;
        } else {
            self.handle_normal_mode_keys(key, repeat_count)?;
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
            KeyCode::Home | KeyCode::Char('g') => {
                if key.modifiers.contains(KeyModifiers::SHIFT) || key.modifiers.contains(KeyModifiers::CONTROL) {
                    self.goto_end();
                } else {
                    self.goto_start();
                }
            }
            KeyCode::End | KeyCode::Char('G') => {
                self.goto_end();
            }

            // Search
            KeyCode::Char('/') => {
                {
                    let mut state = self.state.borrow_mut();
                    state.search_data = Some(SearchData::default());
                    state.ui_state.open_window(WindowType::Search);
                }
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
                let mut state = self.state.borrow_mut();
                state.ui_state.open_window(WindowType::Toc);
            }
            KeyCode::Char('m') => {
                let mut state = self.state.borrow_mut();
                state.ui_state.open_window(WindowType::Bookmarks);
            }
            KeyCode::Char('i') => {
                let mut state = self.state.borrow_mut();
                state.ui_state.open_window(WindowType::Metadata);
            }

            _ => {}
        }

        Ok(())
    }

    /// Handle keys in search mode
    fn handle_search_mode_keys(&mut self, key: KeyEvent, _repeat_count: u32) -> eyre::Result<()> {
        match key.code {
            KeyCode::Enter => {
                // Execute search
                self.execute_search();
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
            }
            KeyCode::Char(c) => {
                // Add character to search query
                let mut state = self.state.borrow_mut();
                state.ui_state.search_query.push(c);
            }
            _ => {}
        }

        Ok(())
    }

    /// Render the UI
    fn render(&self, frame: &mut Frame) {
        let state = self.state.borrow();
        Self::render_static(frame, &state);
    }

    /// Static render method that can be called from a closure
    fn render_static(frame: &mut Frame, state: &ApplicationState) {
        // Main reader view
        Self::render_reader_static(frame, &state);

        // Render overlays/modals if active
        if state.ui_state.show_help {
            // TODO: Implement help overlay
        } else if state.ui_state.show_toc {
            // TODO: Implement TOC overlay
        } else if state.ui_state.show_bookmarks {
            // TODO: Implement bookmarks overlay
        } else if state.ui_state.show_search {
            // TODO: Implement search overlay
        } else if state.ui_state.show_metadata {
            // TODO: Implement metadata overlay
        }

        // Render message if present
        if let Some(ref message) = state.ui_state.message {
            Self::render_message_static(frame, message, &state.ui_state.message_type);
        }
    }

    /// Render the main reader view
    fn render_reader(&self, frame: &mut Frame, state: &ApplicationState) {
        Self::render_reader_static(frame, state);
    }

    /// Static method to render the main reader view
    fn render_reader_static(frame: &mut Frame, state: &ApplicationState) {
        let chunks = Layout::default()
            .direction(ratatui::layout::Direction::Vertical)
            .constraints([
                Constraint::Min(0), // Main content
                Constraint::Length(3), // Status bar
            ])
            .split(frame.area());

        // Main content area (will be implemented in board.rs)
        let content_block = Block::default()
            .borders(Borders::ALL)
            .title("Reader");

        frame.render_widget(content_block, chunks[0]);

        // Status bar
        let status_text = vec![
            Line::from(vec![
                Span::styled("Position: ", Style::default()),
                Span::styled(
                    format!("{}/{}", state.reading_state.row, 0), // TODO: Get total lines
                    Style::default().add_modifier(Modifier::BOLD),
                ),
            ]),
        ];

        let status_paragraph = Paragraph::new(status_text)
            .block(Block::default().borders(Borders::ALL))
            .wrap(Wrap { trim: true });

        frame.render_widget(status_paragraph, chunks[1]);
    }

    // Placeholder methods for rendering overlays
    fn render_help_overlay(&self, _frame: &mut Frame) {
        // TODO: Implement help overlay
    }

    fn render_toc_overlay(&self, _frame: &mut Frame) {
        // TODO: Implement TOC overlay
    }

    fn render_bookmarks_overlay(&self, _frame: &mut Frame) {
        // TODO: Implement bookmarks overlay
    }

    fn render_search_overlay(&self, _frame: &mut Frame) {
        // TODO: Implement search overlay
    }

    fn render_metadata_overlay(&self, _frame: &mut Frame) {
        // TODO: Implement metadata overlay
    }

    fn render_message(&self, frame: &mut Frame, message: &str, message_type: &MessageType) {
        Self::render_message_static(frame, message, message_type);
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

    // Navigation methods (placeholders)
    fn move_cursor(&mut self, _direction: AppDirection) {
        // TODO: Implement cursor movement
    }

    fn next_chapter(&mut self) {
        // TODO: Implement next chapter navigation
    }

    fn previous_chapter(&mut self) {
        // TODO: Implement previous chapter navigation
    }

    fn goto_start(&mut self) {
        // TODO: Implement go to start
    }

    fn goto_end(&mut self) {
        // TODO: Implement go to end
    }

    fn execute_search(&mut self) {
        // TODO: Implement search execution
    }

    fn search_next(&mut self) {
        // TODO: Implement search next
    }

    fn search_previous(&mut self) {
        // TODO: Implement search previous
    }
}