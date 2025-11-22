# Comprehensive Rust Porting Plan: epy to repy

This document outlines a detailed plan for porting the Python-based epub reader `epy` to a Rust-based equivalent, `repy`, based on thorough analysis of the epy codebase architecture and implementation.

## Executive Summary

`epy` is a sophisticated TUI ebook reader (1600+ lines of core reader logic) with advanced features including multi-format support, text-to-speech integration, search with highlighting, bookmarks, library management, and configurable keybindings. The Rust port must maintain this rich feature set while leveraging Rust's performance and safety benefits.

## Codebase Architecture Analysis

### Core Epy Components
- **Reader (`reader.py`)**: 1600+ line main TUI application with state management, event handling, search, TTS, and navigation
- **Models (`models.py`)**: Frozen dataclasses for ReadingState, TextStructure, BookMetadata with complex type relationships
- **Ebook Parsers (`ebooks/`)**: Abstract base class with specialized implementations for EPUB, MOBI, AZW, FB2, and URL content
- **HTML Processing (`parser.py`)**: Custom HTML parser converting to formatted text lines with style preservation
- **Text Rendering (`board.py`)**: InfiniBoard lazy rendering system with double-spread support and animations
- **State Management (`state.py`)**: SQLite-based persistence for reading positions, library, and bookmarks
- **Configuration (`config.py`, `settings.py`)**: JSON-based settings with customizable keymaps and platform support
- **TTS Integration (`speakers/`)**: Multiple text-to-speech engines via subprocess calls
- **Utilities (`utils.py`, `lib.py`)**: Helper functions for file handling, text processing, and external tool integration

### Key Design Patterns
- **Multiprocessing**: Letter counting in separate process for performance
- **Modal Windows**: Help, TOC, metadata, bookmarks dialogs
- **Seamless Chapters**: Continuous reading across chapter boundaries
- **Count Prefixes**: Numeric command repetition (5j for 5 lines down)
- **External Tool Integration**: TTS, dictionary, image viewers via subprocess

## Detailed Technical Challenges & Solutions

### 1. State Management & Ownership Architecture

**Challenge**: Epy uses shared mutable state across multiple objects (Reader, Board, Ebook parsers) with complex interdependencies.

**Rust Solution**:
- **Primary Pattern**: `Rc<RefCell<ApplicationState>>` for shared mutable access
- **Alternative**: Message-passing architecture using `std::sync::mpsc` channels
- **State Components**:
  ```rust
  struct ApplicationState {
      reading_state: RefCell<ReadingState>,
      config: RefCell<Config>,
      ebook: RefCell<Box<dyn Ebook>>,
      search_data: RefCell<Option<SearchData>>,
      ui_state: RefCell<UiState>, // Current modal, active windows, etc.
  }
  ```

### 2. Complex Text Processing Pipeline

**Epy Implementation**: Custom HTML parser with style preservation, section anchors, image handling, and word wrapping.

**Rust Implementation Strategy**:
```rust
// Step 1: HTML to structured representation
struct HtmlDocument {
    elements: Vec<HtmlElement>,
    sections: HashMap<String, usize>, // Section ID -> line index
    images: HashMap<usize, String>,   // Line index -> image path
}

// Step 2: Text conversion with formatting
impl HtmlProcessor {
    fn to_text_structure(&self, doc: &HtmlDocument, width: usize) -> TextStructure {
        // - Process HTML elements in order
        // - Apply formatting spans
        // - Handle image placeholders
        // - Extract section anchors
        // - Generate wrapped text with preserved formatting
    }
}
```

### 3. Multiprocessing to Async Concurrency

**Epy Pattern**: Separate process for letter counting to avoid UI blocking.

**Rust Solution**:
- **Primary**: `tokio` async runtime for non-blocking operations
- **Alternative**: Worker threads using `std::thread` with channels
```rust
async fn calculate_reading_progress(ebook: &dyn Ebook) -> Result<LettersCount, Error> {
    tokio::task::spawn_blocking(move || {
        // CPU-intensive letter counting
    }).await?
}
```

### 4. Event-Driven TUI Architecture

**Epy Pattern**: Curses-based imperative UI with direct screen manipulation.

**Ratatui Pattern**: Declarative widgets with immediate mode rendering:
```rust
struct ReaderApp {
    state: Rc<RefCell<ApplicationState>>,
    focused_widget: WidgetId,
}

impl Widget for ReaderApp {
    fn render(self, area: Rect, buf: &mut Buffer) {
        // Render based on current state
        match self.state.borrow().ui_state.active_window {
            WindowType::Reader => self.render_reader(area, buf),
            WindowType::Help => self.render_help(area, buf),
            WindowType::Toc => self.render_toc(area, buf),
            // ...
        }
    }
}
```

### 5. Advanced Search with Highlighting

**Epy Implementation**: Multi-chapter regex search with temporary highlighting overlays.

**Rust Enhancement**:
```rust
struct SearchEngine {
    pattern: Regex,
    results: Vec<SearchResult>,
    current_index: usize,
}

struct SearchResult {
    content_index: usize,
    line_range: Range<usize>,
    char_range: Range<usize>,
    context: String,
}

impl SearchEngine {
    fn search_across_chapters(&mut self, ebook: &dyn Ebook) -> Result<(), Error> {
        // Iterate through all chapters
        // Find matches in TextStructure
        // Track character positions across wrapped lines
        // Store results for navigation
    }
}
```

### 6. External Tool Integration

**Epy Pattern**: Subprocess calls with error handling for TTS, dictionary, image viewers.

**Rust Strategy**:
```rust
struct ExternalTools {
    tts_engine: Box<dyn TtsEngine>,
    dictionary: Box<dyn DictionaryProvider>,
    image_viewer: Box<dyn ImageViewer>,
}

trait TtsEngine {
    fn speak(&self, text: &str) -> Result<(), Error>;
}

struct GttsMpvEngine {
    // Configuration and state
}

impl TtsEngine for GttsMpvEngine {
    fn speak(&self, text: &str) -> Result<(), Error> {
        // Use tokio::process::Command for non-blocking execution
    }
}
```

### 7. Configuration and Keymap System

**Epy Implementation**: JSON configuration with dynamic keymap customization.

**Rust Enhanced Version**:
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Config {
    display: DisplayConfig,
    reading: ReadingConfig,
    tools: ToolsConfig,
    keymap: KeyMap,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct KeyMap {
    // Use enum for strong typing
    quit: Vec<KeyBinding>,
    page_down: Vec<KeyBinding>,
    // ... custom key combinations
}

#[derive(Debug, Clone, Serialize, Deserialize)]
enum KeyBinding {
    Char(char),
    Ctrl(char),
    Alt(char),
    Function(u8),
    // ... complex combinations
}
```

### 8. Multi-Format Ebook Support

**Epy Architecture**: Abstract base class with format-specific implementations.

**Rust Trait System**:
```rust
#[async_trait]
trait Ebook: Send + Sync {
    async fn get_metadata(&self) -> Result<BookMetadata, Error>;
    async fn initialize(&mut self) -> Result<(), Error>;
    async fn get_text_structure(&self, content_index: usize, width: usize) -> Result<TextStructure, Error>;
    async fn get_image_data(&self, path: &str) -> Result<(String, Vec<u8>), Error>;
    fn cleanup(&mut self);
}

struct EpubParser {
    // EPUB-specific state
}

#[async_trait]
impl Ebook for EpubParser {
    // EPUB-specific implementations
}
```

## Dependency Ecosystem Mapping

### Core Dependencies
| Python | Rust | Purpose |
|--------|------|---------|
| `curses` | `ratatui` + `crossterm` | Terminal UI framework |
| `multiprocessing` | `tokio` | Async/concurrent processing |
| `sqlite3` | `rusqlite` | Database persistence |
| `zipfile` | `zip` | ZIP file handling |
| `xml.etree.ElementTree` | `xml-rs` | XML parsing |
| `html.parser` | `html2text` + `scraper` | HTML processing |
| `textwrap` | `textwrap` | Text wrapping |
| `urllib.parse` | `url` | URL handling |

### Enhanced Dependencies for Rust
- **Async Runtime**: `tokio` for non-blocking operations
- **Serialization**: `serde` + `serde_toml` for configuration
- **Error Handling**: `eyre` + `color-eyre` for rich errors
- **Logging**: `tracing` + `tracing-subscriber` for debugging
- **Clap**: `clap` with derive macros for CLI
- **Testing**: `tempfile` + `assert_cmd` for comprehensive tests

### External Tool Integration
- **TTS**: Continue subprocess approach for external engines
- **Image Viewers**: Cross-platform detection and subprocess execution
- **Dictionary**: Multiple dictionary engine support with fallbacks

## Comprehensive Implementation Roadmap

### Phase 1: Core Infrastructure (COMPLETED ✅)

1.  **Project Setup:** ✅
    *   [x] Initialize a new Rust project.
    *   [x] Add core dependencies to `Cargo.toml`: `ratatui`, `crossterm`, `eyre`.
    *   [x] Research and add dependencies for epub parsing (`epub` crate seems promising) and HTML parsing (`scraper` or `html5ever`).

2.  **Basic Structure and Error Handling:** ✅
    *   [x] Set up the main application entry point in `src/main.rs`.
    *   [x] Implement a global error handling solution using `eyre`.

3.  **Data Models (`src/models.rs`):** ✅
    *   [x] Port all data classes from `epy/src/epy_reader/models.py` to Rust structs. This includes:
        *   [x] `Direction` (as an enum)
        *   [x] `InlineStyle`
        *   [x] `Key` (functionality to be handled by `crossterm`'s native key handling)
        *   [x] `LettersCount`
        *   [x] `NoUpdate` (might be replaced by `Option` or `Result`)
        *   [x] `ReadingState`
        *   [x] `SearchData`
        *   [x] `TextStructure`
        *   [x] `TocEntry`
    *   [x] Ensure all model tests pass (8 tests passing successfully)

4.  **Configuration (`src/config.rs`):** ✅
    *   [x] Port the `Config` class from `epy/src/epy_reader/config.py`.
    *   [x] Port the settings from `epy/src/epy_reader/settings.py`.
    *   [x] Implement loading/saving of configuration from/to a file (JSON format).
    *   [x] Add comprehensive accessor methods and error handling.
    *   [x] Implement platform-specific configuration directory detection.
    *   [x] Add robust test coverage for all edge cases and error conditions.
    *   [x] Ensure all config tests pass (9 tests passing successfully).
    *   [x] Ensure all settings tests pass (20 tests passing successfully).

5.  **Application State (`src/state.rs`):** ✅
    *   [x] Port the `State` class from `epy/src/epy_reader/state.py`.
    *   [x] Implement a simple database using `rusqlite` to store bookmarks and reading history.
    *   [x] Add comprehensive tests for all State methods (13 tests passing):
        *   [x] Database initialization and schema validation
        *   [x] Library management (add, get, delete books)
        *   [x] Reading state persistence and retrieval
        *   [x] Bookmark management (insert, get, delete)
        *   [x] Foreign key constraint handling
        *   [x] Error handling for edge cases
        *   [x] SHA1 bookmark ID generation
        *   [x] Integration tests with mock ebook objects
    *   [x] Verified foreign key constraints and cascade deletions work correctly
    *   [x] Confirmed datetime handling and default values match Python implementation
    *   [x] All 13 state tests passing, covering 100% of functionality

6.  **Ebook Parsing (`src/ebook.rs`, `src/parser.rs`):** ✅
    *   [x] Create an `Ebook` trait to handle different ebook formats.
    *   [x] Implement an `Epub` struct that implements the `Ebook` trait, using the `epub` crate.
    *   [x] Implement HTML parsing using the `html2text` library for robust text conversion and `scraper` for structure extraction. Successfully tested with Marcus Aurelius' "Meditations" EPUB (7,953 lines of text parsed correctly).
    *   [x] Ensure all parser tests pass (5 tests passing successfully)
    *   [x] Ensure all ebook tests pass (5 tests passing successfully) - fixed test_epub_initialize to handle EPUBs without NCX-based TOC

### Phase 2: Terminal UI Infrastructure (IN PROGRESS 🔄)

7.  **Terminal UI (`src/ui/`):**
    *   [ ] Create a `ui` module to hold all TUI-related code.
    *   [ ] **Main Reader (`src/ui/reader.rs`):**
        *   [ ] Create a `Reader` struct to manage the application's main state and logic.
        *   [ ] Implement the main event loop, handling user input from `crossterm`.
        *   [ ] Design state management architecture using `Rc<RefCell<ApplicationState>>`
        *   [ ] Implement count prefix handling for command repetition
        *   [ ] Add seamless chapter navigation support
    *   [ ] **Content View (`src/ui/board.rs`):**
        *   [ ] Implement a `Board` widget (or similar) that is responsible for rendering the book's text content using `ratatui`.
        *   [ ] Add support for double-spread layout with configurable padding
        *   [ ] Implement lazy rendering for performance with large books
        *   [ ] Add text selection and copy functionality
        *   [ ] Support different color schemes and themes
    *   [ ] **Dialogs/Windows (`src/ui/windows/`):**
        *   [ ] Create separate modules for each dialog/window:
        *   [ ] **Table of Contents** (`toc.rs`): Navigation with section anchors and search
        *   [ ] **Metadata display** (`metadata.rs`): Book information display
        *   [ ] **Help window** (`help.rs`): Keybinding reference and usage instructions
        *   [ ] **Bookmarks management** (`bookmarks.rs`): Add, remove, navigate bookmarks
        *   [ ] **Library view** (`library.rs`): Recent books and reading history
        *   [ ] **Search input and results** (`search.rs`): Regex search with highlighting
        *   [ ] **Settings dialog** (`settings.rs`): Runtime configuration changes

8.  **Command-Line Interface (`src/cli.rs`):**
    *   [x] Port the argument parsing logic from `epy/src/epy_reader/cli.py` using the `clap` crate.
    *   [ ] Handle starting the TUI or dumping book content based on arguments.
    *   [ ] Add support for configuration file specification
    *   [ ] Implement verbose logging and debug modes

### Phase 3: Advanced Features (PENDING ⏳)

9.  **Text-to-Speech Integration (`src/tts/`):**
    *   [ ] Create TTS trait system for multiple engine support
    *   [ ] **GTTS + MPV Engine**: Async TTS with progress tracking
    *   [ ] **Mimic Engine**: Local TTS synthesis integration
    *   [ ] **Pico Engine**: Cross-platform voice synthesis
    *   [ ] Add voice selection, speed control, and pronunciation dictionaries
    *   [ ] Implement reading position synchronization with TTS

10. **Advanced Search (`src/search.rs`):**
    *   [ ] Multi-chapter regex search with performance optimization
    *   [ ] Search result highlighting with configurable colors
    *   [ ] Search history and saved searches
    *   [ ] Fuzzy search and typo tolerance
    *   [ ] Incremental search with real-time results

11. **External Tool Integration (`src/tools/`):**
    *   [ ] **Dictionary Integration**: Multiple dictionary engines (sdcv, dict, etc.)
    *   [ ] **Image Viewer Integration**: Cross-platform image display
    *   [ ] **Export Functionality**: Text and highlighted content export
    *   [ ] **Sync Integration**: Cloud storage for reading progress

12. **Utilities (`src/utils.rs`):**
    *   [ ] Port the helper functions from `epy/src/epy_reader/utils.py` and `epy/src/epy_reader/lib.py` to a `utils` module.
    *   [ ] Add platform-specific utilities (Windows/Linux/macOS)
    *   [ ] Implement file format detection and validation
    *   [ ] Add logging and debugging utilities

### Phase 4: Performance & Polish (PENDING ⏳)

13. **Performance Optimization:**
    *   [ ] Implement async book loading and caching
    *   [ ] Optimize large book handling with lazy loading
    *   [ ] Add memory management for massive texts
    *   [ ] Implement progressive loading for network books
    *   [ ] Performance profiling and benchmarking

14. **Advanced Features:**
    *   [ ] **Multiple Format Support**: MOBI, AZW, FB2 parsers using trait system
    *   [ ] **Plugin System**: Extensible architecture for custom parsers and tools
    *   [ ] **Reading Statistics**: Track reading speed, habits, and progress
    *   [ ] **Annotation System**: Marginal notes and highlighting
    *   [ ] **Custom Themes**: User-defined color schemes and layouts

15. **Quality Assurance:**
    *   [ ] Comprehensive test suite with integration tests
    *   [ ] Property-based testing for critical components
    *   [ ] Performance testing and memory profiling
    *   [ ] Cross-platform compatibility testing
    *   [ ] Accessibility features and usability testing

### Phase 5: Integration & Deployment (PENDING ⏳)

16. **Integration (`src/main.rs`):**
    *   [ ] Tie all the modules together in the `main` function.
    *   [ ] Initialize the configuration and state.
    *   [ ] Parse command-line arguments.
    *   [ ] Set up the terminal for `ratatui`.
    *   [ ] Create and run the main `Reader` application.
    *   [ ] Ensure graceful shutdown and terminal restoration.

17. **Distribution & Documentation:**
    *   [ ] Create build scripts and CI/CD pipeline
    *   [ ] Package for multiple platforms (cargo-deb, cargo-wix, etc.)
    *   [ ] Comprehensive user documentation and README
    *   [ ] Developer documentation and contribution guidelines
    *   [ ] Migration guide from epy to repy

## Rust File Structure Documentation

### Core Source Files (`src/`)

**`src/main.rs`** - Application entry point
- CLI argument parsing and configuration loading
- Terminal initialization and graceful shutdown
- Main application lifecycle management

**`src/models.rs`** - Data structures and domain models (27 tests passing)
- `Direction`, `BookMetadata`, `LibraryItem`, `ReadingState`
- `SearchData`, `LettersCount`, `CharPos`, `TextMark`, `TextSpan`
- `InlineStyle`, `TocEntry`, `TextStructure`, `NoUpdate`
- All models include comprehensive validation and edge case handling

**`src/config.rs`** - Configuration management system (9 tests passing)
- `Config` struct for application settings with comprehensive accessor methods
- `Settings` for user preferences and keymaps with merge functionality
- JSON configuration file loading/saving with automatic directory creation
- Platform-specific configuration directory handling (XDG_CONFIG_HOME, HOME, USERPROFILE)
- Robust error handling and fallbacks for invalid or partial configurations
- Enhanced test coverage covering edge cases, serialization, and merge operations

**`src/settings.rs`** - Settings and keymap management (20 tests passing)
- Comprehensive settings structures with serialization support
- Multiple keymap types: `CfgDefaultKeymaps`, `CfgBuiltinKeymaps`, `Keymap`
- Settings merge functionality for configuration overrides
- Double-spread padding configuration
- Complete validation and edge case handling for all settings types

**`src/state.rs`** - Application state persistence (13 tests passing)
- `State` struct for database operations using `rusqlite`
- SQLite integration for bookmarks and reading history with foreign key constraints
- State serialization and recovery with comprehensive error handling
- Library management, reading state persistence, and bookmark operations
- SHA1-based bookmark ID generation matching Python implementation
- Full foreign key constraint support with cascade deletions

**`src/ebook.rs`** - Ebook format abstraction (5 tests passing)
- `Ebook` trait for format-agnostic ebook handling
- `Epub` implementation for EPUB/EPUB3 format support
- Async ebook loading and metadata extraction

**`src/parser.rs`** - HTML and text processing (5 tests passing)
- HTML to text conversion with style preservation
- Text structure analysis and formatting
- Section anchor extraction and image handling

**`src/cli.rs`** - Command-line interface
- Argument parsing using `clap` with derive macros
- Configuration file specification and verbose modes
- Content dumping and TUI mode selection

### Test Files (`tests/`)

**Test Fixtures:**
- `tests/fixtures/small.epub` - Small EPUB file for basic ebook parsing tests
- `tests/fixtures/meditations.epub` - Marcus Aurelius' "Meditations" (7,953 lines) for testing large file handling
- Used by ebook.rs tests to verify EPUB parsing, text extraction, and content processing

### Phase 2: UI Infrastructure (In Progress)

**Planned UI Files (`src/ui/`):**
- `src/ui/reader.rs` - Main TUI application and event loop
- `src/ui/board.rs` - Text rendering widget with lazy loading
- `src/ui/windows/` - Modal dialogs (TOC, help, bookmarks, etc.)

### Dependencies and External Tools

**Core Dependencies:**
- `ratatui` + `crossterm` - Terminal UI framework
- `tokio` - Async runtime for non-blocking operations
- `rusqlite` - Database persistence
- `epub` - EPUB file parsing
- `scraper` + `html2text` - HTML processing
- `serde` - Configuration serialization
- `eyre` - Rich error handling

**External Tool Integration:**
- Text-to-speech engines (GTTS+MPV, Mimic, Pico)
- Dictionary tools (sdcv, dict, wkdict)
- Image viewers (cross-platform detection)

## Development Guidelines

**IMPORTANT**:
- **Commit frequently!** Make small, focused commits as you complete each task or fix each issue. This helps track progress and makes it easier to identify and revert problematic changes.
- **Test-driven development**: Write tests before implementation for complex components
- **Performance first**: Profile and optimize critical paths early
- **Cross-platform mindset**: Ensure compatibility across Linux, macOS, and Windows
- **User experience preservation**: Maintain all epy features while improving performance

## Success Metrics

- **Feature Parity**: 100% of epy functionality available in repy
- **Performance Improvement**: 2x faster book loading and rendering
- **Memory Efficiency**: 50% lower memory usage for large books
- **Reliability**: Zero crashes in production use with comprehensive error handling
- **Maintainability**: Clear code architecture with comprehensive test coverage
