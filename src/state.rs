use crate::models::{LibraryItem, ReadingState};
use eyre::Result;
use rusqlite::{Connection, params};
use std::path::PathBuf;

// Re-use the get_app_data_prefix from config.rs
use crate::config::get_app_data_prefix;

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

                Err(rusqlite::Error::QueryReturnedNoRows) => Ok(ReadingState::default()),

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
