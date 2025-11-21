use eyre::Result;
use rusqlite::{Connection, params};
use std::path::PathBuf;

// Re-use the get_app_data_prefix from config.rs
use crate::config::get_app_data_prefix;
use crate::models::{LibraryItem, ReadingState};

pub struct State {
    conn: Connection,
    filepath: PathBuf,
}

impl State {
    pub fn new() -> Result<Self> {
        let prefix = get_app_data_prefix()?;
        let filepath = prefix.join("states.db");

        let conn = Connection::open(&filepath)?;

        // Check if the database needs initialization
        // Note: rusqlite::Connection::open creates the file if it doesn't exist
        // So, we check if the file existed *before* opening the connection.
        let db_exists = filepath.exists();

        if !db_exists {
            Self::init_db(&conn)?;
        }

        Ok(Self { conn, filepath })
    }

    fn init_db(conn: &Connection) -> Result<()> {
        conn.execute_batch(
            "
            PRAGMA foreign_keys = ON;

            CREATE TABLE IF NOT EXISTS reading_states (
                filepath TEXT PRIMARY KEY,
                content_index INTEGER,
                textwidth INTEGER,
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
                textwidth INTEGER,
                row INTEGER,
                rel_pctg REAL,
                FOREIGN KEY (filepath) REFERENCES reading_states(filepath)
                ON DELETE CASCADE
            );
            ",
        )?;
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
}
