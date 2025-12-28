use crate::models::{LibraryItem, ReadingState};
use eyre::Result;
use rusqlite::{Connection, params};

// Re-use the get_app_data_prefix from config.rs
use crate::config::get_app_data_prefix;

pub struct State {
    conn: Connection,
}

impl State {
    pub fn new() -> Result<Self> {
        let prefix = get_app_data_prefix()?;
        let filepath = prefix.join("states.db");

        // Ensure the parent directory exists
        if let Some(parent) = filepath.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let conn = Connection::open(&filepath)?;

        // Always ensure the schema exists. Tables are created only if missing,
        // so this is safe to run on an existing database and also fixes
        // previously-created empty databases.
        Self::init_db(&conn)?;

        Ok(Self { conn })
    }

    fn init_db(conn: &Connection) -> Result<()> {
        conn.execute_batch(
            "
            PRAGMA foreign_keys = ON;

            CREATE TABLE IF NOT EXISTS reading_states (
                filepath TEXT PRIMARY KEY,
                content_index INTEGER,
                textwidth INTEGER DEFAULT 80,
                row INTEGER,
                rel_pctg REAL
            );

            CREATE TABLE IF NOT EXISTS library (
                last_read DATETIME DEFAULT (datetime('now')),
                filepath TEXT PRIMARY KEY,
                title TEXT,
                author TEXT,
                reading_progress REAL,
                FOREIGN KEY (filepath) REFERENCES reading_states(filepath)
                ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS bookmarks (
                id TEXT PRIMARY KEY,
                filepath TEXT,
                name TEXT,
                content_index INTEGER,
                textwidth INTEGER DEFAULT 80,
                row INTEGER,
                rel_pctg REAL,
                FOREIGN KEY (filepath) REFERENCES reading_states(filepath)
                ON DELETE CASCADE
            );
            ",
        )?;

        // Migration: Attempt to add textwidth column if it doesn't exist
        // We ignore errors here which would happen if the column already exists
        let _ = conn.execute("ALTER TABLE reading_states ADD COLUMN textwidth INTEGER DEFAULT 80", []);
        let _ = conn.execute("ALTER TABLE bookmarks ADD COLUMN textwidth INTEGER DEFAULT 80", []);

        Ok(())
    }

    // Other methods will go here

    pub fn get_from_history(&self) -> Result<Vec<LibraryItem>> {
        let mut stmt = self.conn.prepare(
            "SELECT last_read, filepath, title, author, reading_progress FROM library ORDER BY last_read DESC",
        )?;

        let library_items_iter = stmt.query_map([], |row| {
            Ok(LibraryItem {
                last_read: row.get(0)?,
                filepath: row.get(1)?,
                title: row.get(2)?,
                author: row.get(3)?,
                reading_progress: row.get(4)?,
            })
        })?;

        let mut library_items = Vec::new();
        for item_result in library_items_iter {
            library_items.push(item_result?);
        }

        Ok(library_items)
    }

    pub fn delete_from_library(&self, filepath: &str) -> Result<()> {
        self.conn.execute("PRAGMA foreign_keys = ON", [])?;
        self.conn.execute("DELETE FROM reading_states WHERE filepath=?", params![filepath])?;
        Ok(())
    }

    pub fn get_last_read(&self) -> Result<Option<String>> {
        let library = self.get_from_history()?;
        Ok(library.into_iter().next().map(|item| item.filepath))
    }

    pub fn get_last_reading_state(&self, ebook: &dyn crate::ebook::Ebook) -> Result<ReadingState> {
        let mut stmt = self.conn.prepare("SELECT content_index, textwidth, row, rel_pctg FROM reading_states WHERE filepath=?")?;
        let result = stmt.query_row(params![ebook.path()], |row| {
            Ok(ReadingState {
                content_index: row.get(0)?,
                textwidth: row.get(1)?,
                row: row.get(2)?,
                rel_pctg: row.get(3)?,
                section: None,
            })
        });

        match result {
            Ok(reading_state) => Ok(reading_state),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(ReadingState {
                content_index: 0,
                textwidth: 80,
                row: 0,
                rel_pctg: None,
                section: None,
            }),
            Err(e) => Err(e.into()),
        }
    }

    pub fn set_last_reading_state(&self, ebook: &dyn crate::ebook::Ebook, reading_state: &ReadingState) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO reading_states (filepath, content_index, textwidth, row, rel_pctg) VALUES (?, ?, ?, ?, ?)",
            params![
                ebook.path(),
                reading_state.content_index,
                reading_state.textwidth,
                reading_state.row,
                reading_state.rel_pctg,
            ],
        )?;
        Ok(())
    }

    pub fn insert_bookmark(&self, ebook: &dyn crate::ebook::Ebook, name: &str, reading_state: &ReadingState) -> Result<()> {
        use sha1::{Sha1, Digest};
        let mut hasher = Sha1::new();
        hasher.update(format!("{}{}", ebook.path(), name).as_bytes());
        let hash = hasher.finalize();
        let id = &hex::encode(hash)[..10];

        self.conn.execute(
            "INSERT INTO bookmarks (id, filepath, name, content_index, textwidth, row, rel_pctg) VALUES (?, ?, ?, ?, ?, ?, ?)",
            params![
                id,
                ebook.path(),
                name,
                reading_state.content_index,
                reading_state.textwidth,
                reading_state.row,
                reading_state.rel_pctg,
            ],
        )?;
        Ok(())
    }

    pub fn delete_bookmark(&self, ebook: &dyn crate::ebook::Ebook, name: &str) -> Result<()> {
        self.conn.execute(
            "DELETE FROM bookmarks WHERE filepath=? AND name=?",
            params![ebook.path(), name],
        )?;
        Ok(())
    }

    pub fn get_bookmarks(&self, ebook: &dyn crate::ebook::Ebook) -> Result<Vec<(String, ReadingState)>> {
        let mut stmt = self.conn.prepare("SELECT name, content_index, textwidth, row, rel_pctg FROM bookmarks WHERE filepath=?")?;
        let bookmarks_iter = stmt.query_map(params![ebook.path()], |row| {
            Ok((
                row.get(0)?,
                ReadingState {
                    content_index: row.get(1)?,
                    textwidth: row.get(2)?,
                    row: row.get(3)?,
                    rel_pctg: row.get(4)?,
                    section: None,
                },
            ))
        })?;

        let mut bookmarks = Vec::new();
        for bookmark_result in bookmarks_iter {
            bookmarks.push(bookmark_result?);
        }

        Ok(bookmarks)
    }

    pub fn update_library(&self, ebook: &dyn crate::ebook::Ebook, reading_progress: Option<f32>) -> Result<()> {
        let metadata = &ebook.get_meta();
        self.conn.execute(
            "INSERT OR REPLACE INTO library (filepath, title, author, reading_progress) VALUES (?, ?, ?, ?)",
            params![ebook.path(), metadata.title, metadata.creator, reading_progress],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ebook::Ebook;
    use crate::models::{BookMetadata, TocEntry, TextStructure};
    use tempfile::TempDir;

    // Mock Ebook implementation for testing
    #[derive(Debug)]
    struct MockEbook {
        path_str: String,
        metadata: BookMetadata,
        contents: Vec<String>,
        toc_entries: Vec<TocEntry>,
    }

    impl MockEbook {
        fn new(path: &str, title: &str, author: &str) -> Self {
            Self {
                path_str: path.to_string(),
                metadata: BookMetadata {
                    title: Some(title.to_string()),
                    creator: Some(author.to_string()),
                    language: Some("en".to_string()),
                    publisher: Some("Test Publisher".to_string()),
                    identifier: Some("test-id".to_string()),
                    ..Default::default()
                },
                contents: vec!["chapter1".to_string(), "chapter2".to_string()],
                toc_entries: vec![
                    TocEntry {
                        label: "Chapter 1".to_string(),
                        content_index: 0,
                        section: Some("chapter1".to_string()),
                    },
                    TocEntry {
                        label: "Chapter 2".to_string(),
                        content_index: 1,
                        section: Some("chapter2".to_string()),
                    },
                ],
            }
        }
    }

    impl Ebook for MockEbook {
        fn path(&self) -> &str {
            &self.path_str
        }

        fn contents(&self) -> &Vec<String> {
            &self.contents
        }

        fn toc_entries(&self) -> &Vec<TocEntry> {
            &self.toc_entries
        }

        fn get_meta(&self) -> &BookMetadata {
            &self.metadata
        }

        fn initialize(&mut self) -> Result<()> {
            // No initialization needed for mock
            Ok(())
        }

        fn get_raw_text(&mut self, _content_id: &str) -> Result<String> {
            Ok("Mock text content for testing purposes.".to_string())
        }

        fn get_img_bytestr(&mut self, _path: &str) -> Result<(String, Vec<u8>)> {
            Ok(("image/jpeg".to_string(), vec![0xFF, 0xD8, 0xFF])) // Mock JPEG header
        }

        fn cleanup(&mut self) -> Result<()> {
            // No cleanup needed for mock
            Ok(())
        }

        fn get_parsed_content(&mut self, _content_id: &str, _text_width: usize, _starting_line: usize) -> Result<TextStructure> {
            Ok(TextStructure::default())
        }

        fn get_all_parsed_content(&mut self, _text_width: usize, _page_height: Option<usize>) -> Result<Vec<TextStructure>> {
            Ok(vec![TextStructure::default()])
        }
    }

    fn setup_test_state() -> (State, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test_states.db");

        // Create a test state by manually opening a connection
        let conn = Connection::open(&db_path).unwrap();

        // Initialize the database with textwidth schema
        conn.execute_batch(
            "
            PRAGMA foreign_keys = ON;

            CREATE TABLE IF NOT EXISTS reading_states (
                filepath TEXT PRIMARY KEY,
                content_index INTEGER,
                textwidth INTEGER DEFAULT 80,
                row INTEGER,
                rel_pctg REAL
            );

            CREATE TABLE IF NOT EXISTS library (
                last_read DATETIME DEFAULT (datetime('now')),
                filepath TEXT PRIMARY KEY,
                title TEXT,
                author TEXT,
                reading_progress REAL,
                FOREIGN KEY (filepath) REFERENCES reading_states(filepath)
                ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS bookmarks (
                id TEXT PRIMARY KEY,
                filepath TEXT,
                name TEXT,
                content_index INTEGER,
                textwidth INTEGER DEFAULT 80,
                row INTEGER,
                rel_pctg REAL,
                FOREIGN KEY (filepath) REFERENCES reading_states(filepath)
                ON DELETE CASCADE
            );
            ",
        ).unwrap();

        let state = State {
            conn,
        };

        (state, temp_dir)
    }

    #[test]
    fn test_state_database_initialization() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test_init.db");
        assert!(!db_path.exists());
        let conn = Connection::open(&db_path).unwrap();
        State::init_db(&conn).unwrap();
        assert!(db_path.exists());
        
        // Verify textwidth column exists
        let mut stmt = conn.prepare("PRAGMA table_info(reading_states)").unwrap();
        let columns: Vec<String> = stmt.query_map([], |row| row.get(1)).unwrap()
            .map(|r| r.unwrap())
            .collect();
        assert!(columns.contains(&"textwidth".to_string()));
    }

    #[test]
    fn test_get_from_history_empty() {
        let (state, _temp_dir) = setup_test_state();
        let history = state.get_from_history().unwrap();
        assert!(history.is_empty());
    }

    #[test]
    fn test_library_management() {
        let (state, _temp_dir) = setup_test_state();

        let ebook1 = MockEbook::new("/path/to/book1.epub", "Book One", "Author One");
        let ebook2 = MockEbook::new("/path/to/book2.epub", "Book Two", "Author Two");

        let default_state = ReadingState {
            content_index: 0,
            textwidth: 80,
            row: 0,
            rel_pctg: None,
            section: None,
        };
        state.set_last_reading_state(&ebook1, &default_state).unwrap();
        state.set_last_reading_state(&ebook2, &default_state).unwrap();
        state.update_library(&ebook1, Some(0.25)).unwrap();

        std::thread::sleep(std::time::Duration::from_millis(10));

        state.update_library(&ebook2, Some(0.75)).unwrap();

        let history = state.get_from_history().unwrap();
        assert_eq!(history.len(), 2);

        let book2_found = history.iter().any(|item|
            item.filepath == "/path/to/book2.epub" &&
            item.title == Some("Book Two".to_string()) &&
            item.author == Some("Author Two".to_string()) &&
            item.reading_progress == Some(0.75)
        );
        let book1_found = history.iter().any(|item|
            item.filepath == "/path/to/book1.epub" &&
            item.title == Some("Book One".to_string()) &&
            item.author == Some("Author One".to_string()) &&
            item.reading_progress == Some(0.25)
        );

        assert!(book2_found, "Book 2 should be found in history");
        assert!(book1_found, "Book 1 should be found in history");

        let last_read = state.get_last_read().unwrap();
        assert!(last_read.is_some(), "Should have a last read book");
        assert!(last_read.unwrap().contains("book"), "Should be one of our test books");

        state.delete_from_library("/path/to/book1.epub").unwrap();
        let history = state.get_from_history().unwrap();
        assert_eq!(history.len(), 1);
        assert!(history[0].filepath.contains("book2"), "Should be book 2 remaining");
    }

    #[test]
    fn test_reading_state_management() {
        let (state, _temp_dir) = setup_test_state();
        let ebook = MockEbook::new("/path/to/test.epub", "Test Book", "Test Author");

        let reading_state = state.get_last_reading_state(&ebook).unwrap();
        assert_eq!(reading_state.content_index, 0);
        assert_eq!(reading_state.textwidth, 80);
        assert_eq!(reading_state.row, 0);
        assert_eq!(reading_state.rel_pctg, None);

        let new_state = ReadingState {
            content_index: 5,
            textwidth: 80,
            row: 42,
            rel_pctg: Some(0.678),
            section: None,
        };
        state.set_last_reading_state(&ebook, &new_state).unwrap();

        let retrieved_state = state.get_last_reading_state(&ebook).unwrap();
        assert_eq!(retrieved_state.content_index, 5);
        assert_eq!(retrieved_state.textwidth, 80);
        assert_eq!(retrieved_state.row, 42);
        assert_eq!(retrieved_state.rel_pctg, Some(0.678));
        assert_eq!(retrieved_state.section, None);

        let updated_state = ReadingState {
            content_index: 10,
            textwidth: 80,
            row: 100,
            rel_pctg: Some(0.890),
            section: None,
        };
        state.set_last_reading_state(&ebook, &updated_state).unwrap();

        let final_state = state.get_last_reading_state(&ebook).unwrap();
        assert_eq!(final_state.content_index, 10);
        assert_eq!(final_state.textwidth, 80);
        assert_eq!(final_state.row, 100);
        assert_eq!(final_state.rel_pctg, Some(0.890));
    }

    #[test]
    fn test_bookmark_management() {
        let (state, _temp_dir) = setup_test_state();
        let ebook = MockEbook::new("/path/to/test.epub", "Test Book", "Test Author");

        let bookmarks = state.get_bookmarks(&ebook).unwrap();
        assert!(bookmarks.is_empty());

        let initial_state = ReadingState {
            content_index: 0,
            textwidth: 80,
            row: 0,
            rel_pctg: None,
            section: None,
        };
        state.set_last_reading_state(&ebook, &initial_state).unwrap();

        let state1 = ReadingState {
            content_index: 2,
            textwidth: 80,
            row: 15,
            rel_pctg: Some(0.2),
            section: None,
        };
        let state2 = ReadingState {
            content_index: 5,
            textwidth: 80,
            row: 42,
            rel_pctg: Some(0.5),
            section: None,
        };

        state.insert_bookmark(&ebook, "Chapter 1", &state1).unwrap();
        state.insert_bookmark(&ebook, "Chapter 2", &state2).unwrap();

        let bookmarks = state.get_bookmarks(&ebook).unwrap();
        assert_eq!(bookmarks.len(), 2);

        let chapter1_bookmark = bookmarks.iter().find(|(name, _)| name == "Chapter 1");
        let chapter2_bookmark = bookmarks.iter().find(|(name, _)| name == "Chapter 2");

        assert!(chapter1_bookmark.is_some());
        assert!(chapter2_bookmark.is_some());

        let (_, state1_retrieved) = chapter1_bookmark.unwrap();
        assert_eq!(state1_retrieved.content_index, 2);
        assert_eq!(state1_retrieved.textwidth, 80);
        assert_eq!(state1_retrieved.row, 15);
        assert_eq!(state1_retrieved.rel_pctg, Some(0.2));

        let (_, state2_retrieved) = chapter2_bookmark.unwrap();
        assert_eq!(state2_retrieved.content_index, 5);
        assert_eq!(state2_retrieved.textwidth, 80);
        assert_eq!(state2_retrieved.row, 42);
        assert_eq!(state2_retrieved.rel_pctg, Some(0.5));

        state.delete_bookmark(&ebook, "Chapter 1").unwrap();
        let bookmarks = state.get_bookmarks(&ebook).unwrap();
        assert_eq!(bookmarks.len(), 1);
        assert_eq!(bookmarks[0].0, "Chapter 2");
    }

    #[test]
    fn test_bookmark_id_generation() {
        let (state, _temp_dir) = setup_test_state();
        let ebook1 = MockEbook::new("/path/to/test1.epub", "Test Book 1", "Test Author");
        let ebook2 = MockEbook::new("/path/to/test2.epub", "Test Book 2", "Test Author");

        let reading_state = ReadingState {
            content_index: 1,
            textwidth: 80,
            row: 10,
            rel_pctg: None,
            section: None,
        };

        let default_state = ReadingState {
            content_index: 0,
            textwidth: 80,
            row: 0,
            rel_pctg: None,
            section: None,
        };
        state.set_last_reading_state(&ebook1, &default_state).unwrap();
        state.set_last_reading_state(&ebook2, &default_state).unwrap();
        state.insert_bookmark(&ebook1, "Important", &reading_state).unwrap();
        state.insert_bookmark(&ebook2, "Important", &reading_state).unwrap();

        let bookmarks1 = state.get_bookmarks(&ebook1).unwrap();
        let bookmarks2 = state.get_bookmarks(&ebook2).unwrap();

        assert_eq!(bookmarks1.len(), 1);
        assert_eq!(bookmarks2.len(), 1);

        assert_eq!(bookmarks1[0].0, "Important");
        assert_eq!(bookmarks2[0].0, "Important");

        let state1 = &bookmarks1[0].1;
        let state2 = &bookmarks2[0].1;
        assert_eq!(state1.content_index, state2.content_index);
    }

    #[test]
    fn test_foreign_key_constraints() {
        let (state, _temp_dir) = setup_test_state();
        let ebook = MockEbook::new("/path/to/test.epub", "Test Book", "Test Author");

        let reading_state = ReadingState {
            content_index: 1,
            textwidth: 80,
            row: 10,
            rel_pctg: Some(0.1),
            section: None,
        };
        state.set_last_reading_state(&ebook, &reading_state).unwrap();

        state.update_library(&ebook, Some(0.1)).unwrap();

        state.insert_bookmark(&ebook, "Test Bookmark", &reading_state).unwrap();

        let history = state.get_from_history().unwrap();
        assert_eq!(history.len(), 1);

        let bookmarks = state.get_bookmarks(&ebook).unwrap();
        assert_eq!(bookmarks.len(), 1);

        state.conn.execute("DELETE FROM reading_states WHERE filepath=?", params![ebook.path()]).unwrap();

        let history = state.get_from_history().unwrap();
        assert_eq!(history.len(), 0);

        let bookmarks = state.get_bookmarks(&ebook).unwrap();
        assert_eq!(bookmarks.len(), 0);
    }

    #[test]
    fn test_error_handling() {
        let (state, _temp_dir) = setup_test_state();
        let fake_ebook = MockEbook::new("/nonexistent/path.epub", "Fake Book", "Fake Author");

        let reading_state = state.get_last_reading_state(&fake_ebook).unwrap();
        assert_eq!(reading_state.content_index, 0);

        let bookmarks = state.get_bookmarks(&fake_ebook).unwrap();
        assert!(bookmarks.is_empty());

        let result = state.delete_bookmark(&fake_ebook, "Non-existent bookmark");
        assert!(result.is_ok());
    }

    #[test]
    fn test_update_library_replace() {
        let (state, _temp_dir) = setup_test_state();
        let ebook = MockEbook::new("/path/to/test.epub", "Test Book", "Test Author");

        let default_state = ReadingState {
            content_index: 0,
            textwidth: 80,
            row: 0,
            rel_pctg: None,
            section: None,
        };
        state.set_last_reading_state(&ebook, &default_state).unwrap();

        state.update_library(&ebook, Some(0.25)).unwrap();
        state.update_library(&ebook, Some(0.75)).unwrap();

        let history = state.get_from_history().unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].reading_progress, Some(0.75));

        state.update_library(&ebook, None).unwrap();

        let history = state.get_from_history().unwrap();
        assert_eq!(history[0].reading_progress, None);
    }

    #[test]
    fn test_reading_state_replace() {
        let (state, _temp_dir) = setup_test_state();
        let ebook = MockEbook::new("/path/to/test.epub", "Test Book", "Test Author");

        let state1 = ReadingState {
            content_index: 1,
            textwidth: 80,
            row: 10,
            rel_pctg: Some(0.1),
            section: None,
        };
        state.set_last_reading_state(&ebook, &state1).unwrap();

        let state2 = ReadingState {
            content_index: 5,
            textwidth: 80,
            row: 50,
            rel_pctg: Some(0.5),
            section: None,
        };
        state.set_last_reading_state(&ebook, &state2).unwrap();

        let final_state = state.get_last_reading_state(&ebook).unwrap();
        assert_eq!(final_state.content_index, 5);
        assert_eq!(final_state.textwidth, 80);
        assert_eq!(final_state.row, 50);
        assert_eq!(final_state.rel_pctg, Some(0.5));
    }

    #[test]
    fn test_multiple_ebooks_isolation() {
        let (state, _temp_dir) = setup_test_state();

        let ebook1 = MockEbook::new("/path/to/book1.epub", "Book 1", "Author 1");
        let ebook2 = MockEbook::new("/path/to/book2.epub", "Book 2", "Author 2");
        let ebook3 = MockEbook::new("/path/to/book3.epub", "Book 3", "Author 3");

        let state1 = ReadingState { content_index: 1, textwidth: 80, row: 10, rel_pctg: Some(0.1), section: None };
        let state2 = ReadingState { content_index: 2, textwidth: 80, row: 20, rel_pctg: Some(0.2), section: None };
        let state3 = ReadingState { content_index: 3, textwidth: 80, row: 30, rel_pctg: Some(0.3), section: None };

        state.set_last_reading_state(&ebook1, &state1).unwrap();
        state.set_last_reading_state(&ebook2, &state2).unwrap();
        state.set_last_reading_state(&ebook3, &state3).unwrap();

        let retrieved1 = state.get_last_reading_state(&ebook1).unwrap();
        let retrieved2 = state.get_last_reading_state(&ebook2).unwrap();
        let retrieved3 = state.get_last_reading_state(&ebook3).unwrap();

        assert_eq!(retrieved1.content_index, 1);
        assert_eq!(retrieved2.content_index, 2);
        assert_eq!(retrieved3.content_index, 3);

        state.insert_bookmark(&ebook1, "Bookmark 1", &state1).unwrap();
        state.insert_bookmark(&ebook2, "Bookmark 2", &state2).unwrap();
        state.insert_bookmark(&ebook3, "Bookmark 3", &state3).unwrap();

        let bookmarks1 = state.get_bookmarks(&ebook1).unwrap();
        let bookmarks2 = state.get_bookmarks(&ebook2).unwrap();
        let bookmarks3 = state.get_bookmarks(&ebook3).unwrap();

        assert_eq!(bookmarks1.len(), 1);
        assert_eq!(bookmarks2.len(), 1);
        assert_eq!(bookmarks3.len(), 1);

        assert_eq!(bookmarks1[0].0, "Bookmark 1");
        assert_eq!(bookmarks2[0].0, "Bookmark 2");
        assert_eq!(bookmarks3[0].0, "Bookmark 3");
    }
}
