use crate::models::{BookIdentity, Highlight, LibraryItem, ReadingState};
use crate::theme::ColorTheme;
use eyre::Result;
use rusqlite::{Connection, OptionalExtension, params};

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
        conn.pragma_update(None, "journal_mode", "WAL")?;

        // Always ensure the schema exists. Tables are created only if missing,
        // so this is safe to run on an existing database and also fixes
        // previously-created empty databases.
        Self::init_db(&conn)?;

        Ok(Self { conn })
    }

    /// Create a new in-memory state for testing.
    #[cfg(test)]
    pub fn new_for_test() -> Self {
        let conn = Connection::open_in_memory().unwrap();
        Self::init_db(&conn).unwrap();
        Self { conn }
    }

    fn init_db(conn: &Connection) -> Result<()> {
        conn.execute_batch("PRAGMA foreign_keys = ON;")?;
        let current_version: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
        if current_version < 1 {
            conn.execute_batch("BEGIN IMMEDIATE TRANSACTION;")?;
            if let Err(err) = Self::migrate_v1(conn).and_then(|_| {
                conn.pragma_update(None, "user_version", 1)
                    .map_err(Into::into)
            }) {
                let _ = conn.execute_batch("ROLLBACK;");
                return Err(err);
            }
            conn.execute_batch("COMMIT;")?;
        }
        if current_version < 2 {
            conn.execute_batch("BEGIN IMMEDIATE TRANSACTION;")?;
            if let Err(err) = Self::migrate_v2(conn).and_then(|_| {
                conn.pragma_update(None, "user_version", 2)
                    .map_err(Into::into)
            }) {
                let _ = conn.execute_batch("ROLLBACK;");
                return Err(err);
            }
            conn.execute_batch("COMMIT;")?;
        }
        if current_version < 3 {
            conn.execute_batch("BEGIN IMMEDIATE TRANSACTION;")?;
            if let Err(err) = Self::migrate_v3(conn).and_then(|_| {
                conn.pragma_update(None, "user_version", 3)
                    .map_err(Into::into)
            }) {
                let _ = conn.execute_batch("ROLLBACK;");
                return Err(err);
            }
            conn.execute_batch("COMMIT;")?;
        }
        if current_version < 4 {
            conn.execute_batch("BEGIN IMMEDIATE TRANSACTION;")?;
            if let Err(err) = Self::migrate_v4(conn).and_then(|_| {
                conn.pragma_update(None, "user_version", 4)
                    .map_err(Into::into)
            }) {
                let _ = conn.execute_batch("ROLLBACK;");
                return Err(err);
            }
            conn.execute_batch("COMMIT;")?;
        }
        Ok(())
    }

    fn migrate_v1(conn: &Connection) -> Result<()> {
        conn.execute_batch(
            "
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
        let _ = conn.execute(
            "ALTER TABLE reading_states ADD COLUMN textwidth INTEGER DEFAULT 80",
            [],
        );
        let _ = conn.execute(
            "ALTER TABLE bookmarks ADD COLUMN textwidth INTEGER DEFAULT 80",
            [],
        );
        Ok(())
    }

    fn migrate_v2(conn: &Connection) -> Result<()> {
        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS books (
                book_id TEXT PRIMARY KEY,
                identifier TEXT,
                title TEXT,
                creator TEXT,
                spine_hrefs_hash TEXT NOT NULL,
                content_fingerprints_hash TEXT NOT NULL,
                created_at DATETIME DEFAULT (datetime('now')),
                updated_at DATETIME DEFAULT (datetime('now'))
            );

            CREATE TABLE IF NOT EXISTS book_aliases (
                filepath TEXT PRIMARY KEY,
                book_id TEXT NOT NULL,
                spine_hrefs_hash TEXT NOT NULL,
                content_fingerprints_hash TEXT NOT NULL,
                updated_at DATETIME DEFAULT (datetime('now')),
                FOREIGN KEY (book_id) REFERENCES books(book_id)
                ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS highlights (
                id TEXT PRIMARY KEY,
                book_id TEXT NOT NULL,
                content_index INTEGER NOT NULL,
                spine_href TEXT NOT NULL,
                exact TEXT NOT NULL,
                prefix TEXT NOT NULL,
                suffix TEXT NOT NULL,
                approx_offset INTEGER NOT NULL,
                normalization_version INTEGER NOT NULL,
                color TEXT NOT NULL,
                comment TEXT,
                comment_format TEXT NOT NULL DEFAULT 'plain',
                created_at DATETIME NOT NULL,
                updated_at DATETIME NOT NULL,
                resolution_status TEXT NOT NULL DEFAULT 'unresolved',
                FOREIGN KEY (book_id) REFERENCES books(book_id)
                ON DELETE CASCADE
            );

            CREATE INDEX IF NOT EXISTS idx_highlights_book_content
            ON highlights(book_id, content_index, created_at);
            ",
        )?;
        Ok(())
    }

    fn migrate_v3(conn: &Connection) -> Result<()> {
        // `seq` is a monotonically increasing recency counter; timestamps
        // only have second resolution, which makes ordering ambiguous.
        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS search_history (
                query TEXT PRIMARY KEY,
                seq INTEGER NOT NULL
            );
            ",
        )?;
        Ok(())
    }

    fn migrate_v4(conn: &Connection) -> Result<()> {
        let _ = conn.execute("ALTER TABLE reading_states ADD COLUMN color_theme TEXT", []);
        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS jump_history (
                filepath TEXT PRIMARY KEY,
                entries_json TEXT NOT NULL,
                current_index INTEGER NOT NULL,
                updated_at DATETIME DEFAULT (datetime('now')),
                FOREIGN KEY (filepath) REFERENCES reading_states(filepath)
                ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS marks (
                filepath TEXT NOT NULL,
                name TEXT NOT NULL,
                content_index INTEGER,
                textwidth INTEGER DEFAULT 80,
                row INTEGER,
                rel_pctg REAL,
                updated_at DATETIME DEFAULT (datetime('now')),
                PRIMARY KEY (filepath, name),
                FOREIGN KEY (filepath) REFERENCES reading_states(filepath)
                ON DELETE CASCADE
            );
            ",
        )?;
        Ok(())
    }

    /// Record a search query, refreshing its recency. History is capped at
    /// the 100 most recently used queries.
    pub fn add_search_history(&self, query: &str) -> Result<()> {
        let query = query.trim();
        if query.is_empty() {
            return Ok(());
        }
        self.conn.execute(
            "INSERT INTO search_history (query, seq)
             VALUES (?, (SELECT COALESCE(MAX(seq), 0) + 1 FROM search_history))
             ON CONFLICT(query) DO UPDATE
             SET seq=(SELECT COALESCE(MAX(seq), 0) + 1 FROM search_history)",
            params![query],
        )?;
        self.conn.execute(
            "DELETE FROM search_history WHERE query NOT IN
             (SELECT query FROM search_history ORDER BY seq DESC LIMIT 100)",
            [],
        )?;
        Ok(())
    }

    /// Return search queries, most recently used first.
    pub fn get_search_history(&self) -> Result<Vec<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT query FROM search_history ORDER BY seq DESC LIMIT 100")?;
        let rows = stmt.query_map([], |row| row.get(0))?;
        let mut queries = Vec::new();
        for row in rows {
            queries.push(row?);
        }
        Ok(queries)
    }

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
        self.conn.execute(
            "DELETE FROM reading_states WHERE filepath=?",
            params![filepath],
        )?;
        Ok(())
    }

    pub fn reconcile_filepath(&mut self, old_path: &str, new_path: &str) -> Result<()> {
        if old_path == new_path {
            return Ok(());
        }

        let tx = self.conn.transaction()?;
        tx.execute("PRAGMA foreign_keys = ON", [])?;

        let old_exists = tx
            .query_row(
                "SELECT 1 FROM reading_states WHERE filepath=? LIMIT 1",
                params![old_path],
                |_| Ok(()),
            )
            .optional()?
            .is_some();
        if !old_exists {
            tx.commit()?;
            return Ok(());
        }

        let new_exists = tx
            .query_row(
                "SELECT 1 FROM reading_states WHERE filepath=? LIMIT 1",
                params![new_path],
                |_| Ok(()),
            )
            .optional()?
            .is_some();

        if !new_exists {
            tx.execute(
                "INSERT INTO reading_states (filepath, content_index, textwidth, row, rel_pctg, color_theme)
                 SELECT ?, content_index, textwidth, row, rel_pctg, color_theme FROM reading_states WHERE filepath=?",
                params![new_path, old_path],
            )?;
        }

        let old_library: Option<(String, Option<String>, Option<String>, Option<f32>)> = tx
            .query_row(
                "SELECT last_read, title, author, reading_progress FROM library WHERE filepath=?",
                params![old_path],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .optional()?;
        let new_library: Option<(String, Option<String>, Option<String>, Option<f32>)> = tx
            .query_row(
                "SELECT last_read, title, author, reading_progress FROM library WHERE filepath=?",
                params![new_path],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .optional()?;

        let should_promote_old = match (&old_library, &new_library) {
            (Some((old_last, ..)), Some((new_last, ..))) => old_last > new_last,
            (Some(_), None) => true,
            _ => false,
        };

        if let Some((last_read, title, author, reading_progress)) = old_library {
            if should_promote_old {
                if new_library.is_some() {
                    tx.execute(
                        "UPDATE library SET last_read=?, title=?, author=?, reading_progress=? WHERE filepath=?",
                        params![last_read, title, author, reading_progress, new_path],
                    )?;
                } else {
                    tx.execute(
                        "INSERT INTO library (last_read, filepath, title, author, reading_progress) VALUES (?, ?, ?, ?, ?)",
                        params![last_read, new_path, title, author, reading_progress],
                    )?;
                }
                tx.execute(
                    "INSERT INTO reading_states (filepath, content_index, textwidth, row, rel_pctg, color_theme)
                     SELECT ?, content_index, textwidth, row, rel_pctg, color_theme FROM reading_states WHERE filepath=?
                     ON CONFLICT(filepath) DO UPDATE SET
                        content_index=excluded.content_index,
                        textwidth=excluded.textwidth,
                        row=excluded.row,
                        rel_pctg=excluded.rel_pctg,
                        color_theme=excluded.color_theme",
                    params![new_path, old_path],
                )?;
            }
        }

        tx.execute(
            "UPDATE bookmarks SET filepath=? WHERE filepath=?",
            params![new_path, old_path],
        )?;
        tx.execute(
            "DELETE FROM jump_history WHERE filepath=? AND EXISTS
             (SELECT 1 FROM jump_history WHERE filepath=?)",
            params![new_path, old_path],
        )?;
        tx.execute(
            "UPDATE jump_history SET filepath=? WHERE filepath=?",
            params![new_path, old_path],
        )?;
        tx.execute(
            "DELETE FROM marks
             WHERE filepath=? AND name IN
             (SELECT name FROM marks WHERE filepath=?)",
            params![new_path, old_path],
        )?;
        tx.execute(
            "UPDATE marks SET filepath=? WHERE filepath=?",
            params![new_path, old_path],
        )?;

        tx.execute("DELETE FROM library WHERE filepath=?", params![old_path])?;
        tx.execute(
            "DELETE FROM reading_states WHERE filepath=?",
            params![old_path],
        )?;
        tx.execute("DELETE FROM bookmarks WHERE filepath=?", params![old_path])?;
        tx.execute(
            "DELETE FROM jump_history WHERE filepath=?",
            params![old_path],
        )?;
        tx.execute("DELETE FROM marks WHERE filepath=?", params![old_path])?;

        tx.commit()?;
        Ok(())
    }

    pub fn get_last_read(&self) -> Result<Option<String>> {
        let library = self.get_from_history()?;
        Ok(library.into_iter().next().map(|item| item.filepath))
    }

    pub fn get_last_reading_state(&self, ebook: &dyn crate::ebook::Ebook) -> Result<ReadingState> {
        let mut stmt = self.conn.prepare(
            "SELECT content_index, textwidth, row, rel_pctg FROM reading_states WHERE filepath=?",
        )?;
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

    pub fn set_last_reading_state(
        &self,
        ebook: &dyn crate::ebook::Ebook,
        reading_state: &ReadingState,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO reading_states (filepath, content_index, textwidth, row, rel_pctg)
             VALUES (?, ?, ?, ?, ?)
             ON CONFLICT(filepath) DO UPDATE SET
                content_index=excluded.content_index,
                textwidth=excluded.textwidth,
                row=excluded.row,
                rel_pctg=excluded.rel_pctg",
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

    pub fn get_book_theme(&self, ebook: &dyn crate::ebook::Ebook) -> Result<Option<ColorTheme>> {
        let stored: Option<String> = self
            .conn
            .query_row(
                "SELECT color_theme FROM reading_states WHERE filepath=?",
                params![ebook.path()],
                |row| row.get(0),
            )
            .optional()?
            .flatten();
        Ok(stored.and_then(|name| ColorTheme::from_storage_name(&name)))
    }

    pub fn set_book_theme(
        &self,
        ebook: &dyn crate::ebook::Ebook,
        theme: Option<ColorTheme>,
    ) -> Result<()> {
        self.conn.execute(
            "UPDATE reading_states SET color_theme=? WHERE filepath=?",
            params![theme.map(|theme| theme.storage_name()), ebook.path()],
        )?;
        Ok(())
    }

    pub fn insert_bookmark(
        &self,
        ebook: &dyn crate::ebook::Ebook,
        name: &str,
        reading_state: &ReadingState,
    ) -> Result<()> {
        use sha1::{Digest, Sha1};
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

    pub fn get_bookmarks(
        &self,
        ebook: &dyn crate::ebook::Ebook,
    ) -> Result<Vec<(String, ReadingState)>> {
        let mut stmt = self.conn.prepare(
            "SELECT name, content_index, textwidth, row, rel_pctg FROM bookmarks WHERE filepath=?",
        )?;
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

    pub fn set_jump_history(
        &self,
        ebook: &dyn crate::ebook::Ebook,
        entries: &[usize],
        current_index: usize,
    ) -> Result<()> {
        let entries_json = serde_json::to_string(entries)?;
        let current_index = current_index.min(entries.len());
        self.conn.execute(
            "INSERT INTO jump_history (filepath, entries_json, current_index, updated_at)
             VALUES (?, ?, ?, datetime('now'))
             ON CONFLICT(filepath) DO UPDATE SET
                entries_json=excluded.entries_json,
                current_index=excluded.current_index,
                updated_at=datetime('now')",
            params![ebook.path(), entries_json, current_index],
        )?;
        Ok(())
    }

    pub fn get_jump_history(&self, ebook: &dyn crate::ebook::Ebook) -> Result<(Vec<usize>, usize)> {
        let stored: Option<(String, usize)> = self
            .conn
            .query_row(
                "SELECT entries_json, current_index FROM jump_history WHERE filepath=?",
                params![ebook.path()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let Some((entries_json, current_index)) = stored else {
            return Ok((Vec::new(), 0));
        };
        let entries: Vec<usize> = serde_json::from_str(&entries_json).unwrap_or_default();
        let current_index = current_index.min(entries.len());
        Ok((entries, current_index))
    }

    pub fn upsert_mark(
        &self,
        ebook: &dyn crate::ebook::Ebook,
        name: char,
        reading_state: &ReadingState,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO marks
             (filepath, name, content_index, textwidth, row, rel_pctg, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, datetime('now'))
             ON CONFLICT(filepath, name) DO UPDATE SET
                content_index=excluded.content_index,
                textwidth=excluded.textwidth,
                row=excluded.row,
                rel_pctg=excluded.rel_pctg,
                updated_at=datetime('now')",
            params![
                ebook.path(),
                name.to_string(),
                reading_state.content_index,
                reading_state.textwidth,
                reading_state.row,
                reading_state.rel_pctg,
            ],
        )?;
        Ok(())
    }

    pub fn get_marks(&self, ebook: &dyn crate::ebook::Ebook) -> Result<Vec<(char, ReadingState)>> {
        let mut stmt = self.conn.prepare(
            "SELECT name, content_index, textwidth, row, rel_pctg
             FROM marks WHERE filepath=? ORDER BY name",
        )?;
        let rows = stmt.query_map(params![ebook.path()], |row| {
            let name: String = row.get(0)?;
            Ok((
                name.chars().next().unwrap_or('\0'),
                ReadingState {
                    content_index: row.get(1)?,
                    textwidth: row.get(2)?,
                    row: row.get(3)?,
                    rel_pctg: row.get(4)?,
                    section: None,
                },
            ))
        })?;

        let mut marks = Vec::new();
        for row in rows {
            let (name, reading_state) = row?;
            if name != '\0' {
                marks.push((name, reading_state));
            }
        }
        Ok(marks)
    }

    pub fn update_library(
        &self,
        ebook: &dyn crate::ebook::Ebook,
        reading_progress: Option<f32>,
    ) -> Result<()> {
        let metadata = &ebook.get_meta();
        self.conn.execute(
            "INSERT OR REPLACE INTO library (filepath, title, author, reading_progress) VALUES (?, ?, ?, ?)",
            params![ebook.path(), metadata.title, metadata.creator, reading_progress],
        )?;
        Ok(())
    }

    /// Find the most-recently-read library filepath that holds the same book
    /// (by `book_id` via `book_aliases`) but is stored under a path different
    /// from `current_path`. Used to recognise that an ebook opened from a new
    /// location is already in the library, so we can reconcile the existing
    /// entry instead of adding a duplicate.
    pub fn find_other_library_path_for_book(
        &self,
        book_id: &str,
        current_path: &str,
    ) -> Result<Option<String>> {
        let result = self
            .conn
            .query_row(
                "SELECT l.filepath FROM library l
                 JOIN book_aliases a ON a.filepath = l.filepath
                 WHERE a.book_id = ? AND l.filepath != ?
                 ORDER BY l.last_read DESC
                 LIMIT 1",
                params![book_id, current_path],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        Ok(result)
    }

    pub fn upsert_book_record(&self, identity: &BookIdentity) -> Result<()> {
        self.conn.execute(
            "INSERT INTO books
             (book_id, identifier, title, creator, spine_hrefs_hash, content_fingerprints_hash, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, datetime('now'))
             ON CONFLICT(book_id) DO UPDATE SET
                identifier=excluded.identifier,
                title=excluded.title,
                creator=excluded.creator,
                spine_hrefs_hash=excluded.spine_hrefs_hash,
                content_fingerprints_hash=excluded.content_fingerprints_hash,
                updated_at=datetime('now')",
            params![
                identity.book_id,
                identity.identifier,
                identity.title,
                identity.creator,
                identity.spine_hrefs_hash,
                identity.content_fingerprints_hash,
            ],
        )?;
        Ok(())
    }

    pub fn upsert_book_identity(&self, filepath: &str, identity: &BookIdentity) -> Result<()> {
        self.upsert_book_record(identity)?;
        self.conn.execute(
            "INSERT INTO book_aliases
             (filepath, book_id, spine_hrefs_hash, content_fingerprints_hash, updated_at)
             VALUES (?, ?, ?, ?, datetime('now'))
             ON CONFLICT(filepath) DO UPDATE SET
                book_id=excluded.book_id,
                spine_hrefs_hash=excluded.spine_hrefs_hash,
                content_fingerprints_hash=excluded.content_fingerprints_hash,
                updated_at=datetime('now')",
            params![
                filepath,
                identity.book_id,
                identity.spine_hrefs_hash,
                identity.content_fingerprints_hash,
            ],
        )?;
        Ok(())
    }

    pub fn alias_conflict(
        &self,
        filepath: &str,
        identity: &BookIdentity,
    ) -> Result<Option<String>> {
        let existing: Option<(String, String, String)> = self
            .conn
            .query_row(
                "SELECT book_id, spine_hrefs_hash, content_fingerprints_hash
                 FROM book_aliases WHERE filepath=?",
                params![filepath],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?;
        Ok(existing.and_then(|(book_id, spine_hash, content_hash)| {
            (book_id != identity.book_id
                && (spine_hash != identity.spine_hrefs_hash
                    || content_hash != identity.content_fingerprints_hash))
                .then_some(book_id)
        }))
    }

    pub fn insert_highlight(&self, highlight: &Highlight) -> Result<()> {
        self.conn.execute(
            "INSERT INTO highlights
             (id, book_id, content_index, spine_href, exact, prefix, suffix, approx_offset,
              normalization_version, color, comment, comment_format, created_at, updated_at,
              resolution_status)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(id) DO UPDATE SET
                exact=excluded.exact,
                prefix=excluded.prefix,
                suffix=excluded.suffix,
                approx_offset=excluded.approx_offset,
                color=excluded.color,
                comment=excluded.comment,
                comment_format=excluded.comment_format,
                updated_at=excluded.updated_at,
                resolution_status=excluded.resolution_status",
            params![
                highlight.id,
                highlight.book_id,
                highlight.content_index,
                highlight.spine_href,
                highlight.exact,
                highlight.prefix,
                highlight.suffix,
                highlight.approx_offset,
                highlight.normalization_version,
                highlight.color,
                highlight.comment,
                highlight.comment_format,
                highlight.created_at,
                highlight.updated_at,
                highlight.resolution_status,
            ],
        )?;
        Ok(())
    }

    pub fn list_highlights(&self, book_id: &str) -> Result<Vec<Highlight>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, book_id, content_index, spine_href, exact, prefix, suffix, approx_offset,
                    normalization_version, color, comment, comment_format, created_at, updated_at,
                    resolution_status
             FROM highlights WHERE book_id=? ORDER BY content_index ASC, approx_offset ASC, created_at ASC",
        )?;
        let rows = stmt.query_map(params![book_id], |row| {
            Ok(Highlight {
                id: row.get(0)?,
                book_id: row.get(1)?,
                content_index: row.get(2)?,
                spine_href: row.get(3)?,
                exact: row.get(4)?,
                prefix: row.get(5)?,
                suffix: row.get(6)?,
                approx_offset: row.get(7)?,
                normalization_version: row.get(8)?,
                color: row.get(9)?,
                comment: row.get(10)?,
                comment_format: row.get(11)?,
                created_at: row.get(12)?,
                updated_at: row.get(13)?,
                resolution_status: row.get(14)?,
            })
        })?;
        let mut highlights = Vec::new();
        for row in rows {
            highlights.push(row?);
        }
        Ok(highlights)
    }

    pub fn update_highlight_comment(&self, id: &str, comment: Option<&str>) -> Result<()> {
        self.conn.execute(
            "UPDATE highlights
             SET comment=?, comment_format='plain', updated_at=datetime('now')
             WHERE id=?",
            params![comment, id],
        )?;
        Ok(())
    }

    pub fn update_highlight_color(&self, id: &str, color: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE highlights
             SET color=?, updated_at=datetime('now')
             WHERE id=?",
            params![color, id],
        )?;
        Ok(())
    }

    pub fn update_highlight_status(&self, id: &str, status: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE highlights SET resolution_status=?, updated_at=datetime('now') WHERE id=?",
            params![status, id],
        )?;
        Ok(())
    }

    pub fn delete_highlight(&self, id: &str) -> Result<()> {
        self.conn
            .execute("DELETE FROM highlights WHERE id=?", params![id])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ebook::Ebook;
    use crate::models::{BookMetadata, TextStructure, TocEntry};
    use tempfile::TempDir;

    #[test]
    fn test_search_history_recency_and_dedup() {
        let state = State::new_for_test();
        state.add_search_history("foo").unwrap();
        state.add_search_history("bar").unwrap();
        // Re-adding an existing query moves it to the front.
        state.add_search_history("foo").unwrap();
        let history = state.get_search_history().unwrap();
        assert_eq!(history, vec!["foo".to_string(), "bar".to_string()]);
        // Blank queries are ignored.
        state.add_search_history("   ").unwrap();
        assert_eq!(state.get_search_history().unwrap().len(), 2);
    }

    #[test]
    fn test_search_history_capped_at_100() {
        let state = State::new_for_test();
        for i in 0..120 {
            state.add_search_history(&format!("query-{}", i)).unwrap();
        }
        let history = state.get_search_history().unwrap();
        assert_eq!(history.len(), 100);
        assert_eq!(history[0], "query-119");
        assert_eq!(history[99], "query-20");
    }

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

        fn spine_href(&self, index: usize) -> Option<String> {
            self.contents.get(index).cloned()
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

        fn get_parsed_content(
            &mut self,
            _content_id: &str,
            _text_width: usize,
            _starting_line: usize,
        ) -> Result<TextStructure> {
            Ok(TextStructure::default())
        }

        fn get_all_parsed_content(
            &mut self,
            _text_width: usize,
            _page_height: Option<usize>,
        ) -> Result<Vec<TextStructure>> {
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
        )
        .unwrap();

        State::init_db(&conn).unwrap();

        let state = State { conn };

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
        let columns: Vec<String> = stmt
            .query_map([], |row| row.get(1))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        assert!(columns.contains(&"textwidth".to_string()));
        assert!(columns.contains(&"color_theme".to_string()));

        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, 4);
    }

    fn sample_identity(book_id: &str) -> BookIdentity {
        BookIdentity {
            book_id: book_id.to_string(),
            identifier: Some("id".to_string()),
            title: Some("Title".to_string()),
            creator: Some("Author".to_string()),
            spine_hrefs_hash: "spine".to_string(),
            content_fingerprints_hash: "content".to_string(),
        }
    }

    fn sample_highlight(id: &str, book_id: &str) -> Highlight {
        let now = chrono::Utc::now();
        Highlight {
            id: id.to_string(),
            book_id: book_id.to_string(),
            content_index: 0,
            spine_href: "chapter.xhtml".to_string(),
            exact: "selected text".to_string(),
            prefix: "before ".to_string(),
            suffix: " after".to_string(),
            approx_offset: 10,
            normalization_version: crate::annotations::NORMALIZATION_VERSION,
            color: "yellow".to_string(),
            comment: None,
            comment_format: "plain".to_string(),
            created_at: now,
            updated_at: now,
            resolution_status: "resolved".to_string(),
        }
    }

    #[test]
    fn test_book_identity_alias_reuse_and_conflict() {
        let state = State::new_for_test();
        let identity = sample_identity("book-a");
        state
            .upsert_book_identity("/tmp/book.epub", &identity)
            .unwrap();
        assert_eq!(
            state.alias_conflict("/tmp/book.epub", &identity).unwrap(),
            None
        );

        let mut changed = sample_identity("book-b");
        changed.content_fingerprints_hash = "different".to_string();
        assert_eq!(
            state.alias_conflict("/tmp/book.epub", &changed).unwrap(),
            Some("book-a".to_string())
        );
    }

    #[test]
    fn test_find_other_library_path_for_book_dedup() {
        let mut state = State::new_for_test();
        let identity = sample_identity("book-x");

        // Book is already in the library under an old path.
        let old_ebook = MockEbook::new("/old/path.epub", "Title", "Author");
        state
            .set_last_reading_state(&old_ebook, &ReadingState::default())
            .unwrap();
        state
            .upsert_book_identity("/old/path.epub", &identity)
            .unwrap();
        state.update_library(&old_ebook, Some(0.42)).unwrap();

        // Opening the same book from a new location registers the alias.
        state
            .upsert_book_identity("/new/path.epub", &identity)
            .unwrap();

        // The new path is recognised as the same book stored elsewhere.
        assert_eq!(
            state
                .find_other_library_path_for_book(&identity.book_id, "/new/path.epub")
                .unwrap(),
            Some("/old/path.epub".to_string())
        );
        // The current path itself is never returned.
        assert_eq!(
            state
                .find_other_library_path_for_book(&identity.book_id, "/old/path.epub")
                .unwrap(),
            None
        );

        // Reconciling migrates the entry instead of leaving a duplicate, and
        // preserves the reading progress.
        state
            .reconcile_filepath("/old/path.epub", "/new/path.epub")
            .unwrap();
        let history = state.get_from_history().unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].filepath, "/new/path.epub");
        assert_eq!(history[0].reading_progress, Some(0.42));
    }

    #[test]
    fn test_highlight_crud() {
        let state = State::new_for_test();
        let identity = sample_identity("book-a");
        state
            .upsert_book_identity("/tmp/book.epub", &identity)
            .unwrap();

        let mut highlight = sample_highlight("h1", &identity.book_id);
        state.insert_highlight(&highlight).unwrap();
        let highlights = state.list_highlights(&identity.book_id).unwrap();
        assert_eq!(highlights.len(), 1);
        assert_eq!(highlights[0].exact, "selected text");

        state
            .update_highlight_comment("h1", Some("plain comment"))
            .unwrap();
        state.update_highlight_status("h1", "ambiguous").unwrap();
        let updated = state.list_highlights(&identity.book_id).unwrap();
        assert_eq!(updated[0].comment.as_deref(), Some("plain comment"));
        assert_eq!(updated[0].resolution_status, "ambiguous");

        highlight.exact = "changed text".to_string();
        highlight.comment = Some("new comment".to_string());
        state.insert_highlight(&highlight).unwrap();
        let replaced = state.list_highlights(&identity.book_id).unwrap();
        assert_eq!(replaced.len(), 1);
        assert_eq!(replaced[0].exact, "changed text");

        state.delete_highlight("h1").unwrap();
        assert!(state.list_highlights(&identity.book_id).unwrap().is_empty());
    }

    #[test]
    fn test_migration_from_v1_preserves_existing_state() {
        let conn = Connection::open_in_memory().unwrap();
        State::migrate_v1(&conn).unwrap();
        conn.pragma_update(None, "user_version", 1).unwrap();
        conn.execute(
            "INSERT INTO reading_states (filepath, content_index, textwidth, row, rel_pctg)
             VALUES (?, ?, ?, ?, ?)",
            params!["/legacy.epub", 0, 80, 5, 0.5],
        )
        .unwrap();

        State::init_db(&conn).unwrap();

        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, 4);

        let row: i64 = conn
            .query_row(
                "SELECT row FROM reading_states WHERE filepath=?",
                params!["/legacy.epub"],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(row, 5);

        let highlight_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM highlights", [], |row| row.get(0))
            .unwrap();
        assert_eq!(highlight_count, 0);
        let jump_table_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM jump_history", [], |row| row.get(0))
            .unwrap();
        assert_eq!(jump_table_count, 0);
        let marks_table_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM marks", [], |row| row.get(0))
            .unwrap();
        assert_eq!(marks_table_count, 0);

        let state = State { conn };
        let identity = sample_identity("legacy-book");
        state
            .upsert_book_identity("/legacy.epub", &identity)
            .unwrap();
        state
            .insert_highlight(&sample_highlight("h-after-migrate", &identity.book_id))
            .unwrap();
        assert_eq!(state.list_highlights(&identity.book_id).unwrap().len(), 1);
    }

    #[test]
    fn test_alias_path_change_reuses_book_id() {
        let state = State::new_for_test();
        let identity = sample_identity("book-stable");
        state
            .upsert_book_identity("/old/path.epub", &identity)
            .unwrap();
        state
            .insert_highlight(&sample_highlight("h-keep", &identity.book_id))
            .unwrap();

        // Same identity, new path — should not create a new book and highlights survive
        state
            .upsert_book_identity("/new/path.epub", &identity)
            .unwrap();
        assert_eq!(
            state.alias_conflict("/new/path.epub", &identity).unwrap(),
            None
        );
        let highlights = state.list_highlights(&identity.book_id).unwrap();
        assert_eq!(highlights.len(), 1);
        assert_eq!(highlights[0].id, "h-keep");
    }

    #[test]
    fn test_book_theme_jump_history_and_marks_persist() {
        let state = State::new_for_test();
        let ebook = MockEbook::new("/tmp/book.epub", "Title", "Author");
        let reading_state = ReadingState {
            content_index: 2,
            textwidth: 86,
            row: 42,
            rel_pctg: Some(0.4),
            section: None,
        };
        state
            .set_last_reading_state(&ebook, &reading_state)
            .unwrap();

        assert_eq!(state.get_book_theme(&ebook).unwrap(), None);
        state
            .set_book_theme(&ebook, Some(ColorTheme::Sepia))
            .unwrap();
        assert_eq!(
            state.get_book_theme(&ebook).unwrap(),
            Some(ColorTheme::Sepia)
        );

        state.set_jump_history(&ebook, &[3, 9, 42], 2).unwrap();
        assert_eq!(state.get_jump_history(&ebook).unwrap(), (vec![3, 9, 42], 2));

        state.upsert_mark(&ebook, 'a', &reading_state).unwrap();
        let marks = state.get_marks(&ebook).unwrap();
        assert_eq!(marks.len(), 1);
        assert_eq!(marks[0].0, 'a');
        assert_eq!(marks[0].1.row, 42);

        let updated_state = ReadingState {
            row: 100,
            ..reading_state
        };
        state
            .set_last_reading_state(&ebook, &updated_state)
            .unwrap();
        assert_eq!(
            state.get_book_theme(&ebook).unwrap(),
            Some(ColorTheme::Sepia)
        );
        assert_eq!(state.get_jump_history(&ebook).unwrap(), (vec![3, 9, 42], 2));
        assert_eq!(state.get_marks(&ebook).unwrap()[0].1.row, 42);
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
        state
            .set_last_reading_state(&ebook1, &default_state)
            .unwrap();
        state
            .set_last_reading_state(&ebook2, &default_state)
            .unwrap();
        state.update_library(&ebook1, Some(0.25)).unwrap();

        std::thread::sleep(std::time::Duration::from_millis(10));

        state.update_library(&ebook2, Some(0.75)).unwrap();

        let history = state.get_from_history().unwrap();
        assert_eq!(history.len(), 2);

        let book2_found = history.iter().any(|item| {
            item.filepath == "/path/to/book2.epub"
                && item.title == Some("Book Two".to_string())
                && item.author == Some("Author Two".to_string())
                && item.reading_progress == Some(0.75)
        });
        let book1_found = history.iter().any(|item| {
            item.filepath == "/path/to/book1.epub"
                && item.title == Some("Book One".to_string())
                && item.author == Some("Author One".to_string())
                && item.reading_progress == Some(0.25)
        });

        assert!(book2_found, "Book 2 should be found in history");
        assert!(book1_found, "Book 1 should be found in history");

        let last_read = state.get_last_read().unwrap();
        assert!(last_read.is_some(), "Should have a last read book");
        assert!(
            last_read.unwrap().contains("book"),
            "Should be one of our test books"
        );

        state.delete_from_library("/path/to/book1.epub").unwrap();
        let history = state.get_from_history().unwrap();
        assert_eq!(history.len(), 1);
        assert!(
            history[0].filepath.contains("book2"),
            "Should be book 2 remaining"
        );
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
        state
            .set_last_reading_state(&ebook, &updated_state)
            .unwrap();

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
        state
            .set_last_reading_state(&ebook, &initial_state)
            .unwrap();

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
        state
            .set_last_reading_state(&ebook1, &default_state)
            .unwrap();
        state
            .set_last_reading_state(&ebook2, &default_state)
            .unwrap();
        state
            .insert_bookmark(&ebook1, "Important", &reading_state)
            .unwrap();
        state
            .insert_bookmark(&ebook2, "Important", &reading_state)
            .unwrap();

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
        state
            .set_last_reading_state(&ebook, &reading_state)
            .unwrap();

        state.update_library(&ebook, Some(0.1)).unwrap();

        state
            .insert_bookmark(&ebook, "Test Bookmark", &reading_state)
            .unwrap();

        let history = state.get_from_history().unwrap();
        assert_eq!(history.len(), 1);

        let bookmarks = state.get_bookmarks(&ebook).unwrap();
        assert_eq!(bookmarks.len(), 1);

        state
            .conn
            .execute(
                "DELETE FROM reading_states WHERE filepath=?",
                params![ebook.path()],
            )
            .unwrap();

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
        state
            .set_last_reading_state(&ebook, &default_state)
            .unwrap();

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
    fn test_reconcile_filepath_moves_entries() {
        let (mut state, _temp_dir) = setup_test_state();
        let old_ebook = MockEbook::new("/path/to/old.epub", "Old Book", "Old Author");
        let new_ebook = MockEbook::new("/path/to/new.epub", "Old Book", "Old Author");

        let reading_state = ReadingState {
            content_index: 2,
            textwidth: 80,
            row: 5,
            rel_pctg: Some(0.2),
            section: None,
        };
        state
            .set_last_reading_state(&old_ebook, &reading_state)
            .unwrap();
        state.update_library(&old_ebook, Some(0.2)).unwrap();
        state
            .insert_bookmark(&old_ebook, "Bookmark", &reading_state)
            .unwrap();

        state
            .reconcile_filepath(old_ebook.path(), new_ebook.path())
            .unwrap();

        let history = state.get_from_history().unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].filepath, "/path/to/new.epub");

        let migrated_state = state.get_last_reading_state(&new_ebook).unwrap();
        assert_eq!(migrated_state.content_index, 2);

        let bookmarks = state.get_bookmarks(&new_ebook).unwrap();
        assert_eq!(bookmarks.len(), 1);
        assert_eq!(bookmarks[0].0, "Bookmark");
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

        let state1 = ReadingState {
            content_index: 1,
            textwidth: 80,
            row: 10,
            rel_pctg: Some(0.1),
            section: None,
        };
        let state2 = ReadingState {
            content_index: 2,
            textwidth: 80,
            row: 20,
            rel_pctg: Some(0.2),
            section: None,
        };
        let state3 = ReadingState {
            content_index: 3,
            textwidth: 80,
            row: 30,
            rel_pctg: Some(0.3),
            section: None,
        };

        state.set_last_reading_state(&ebook1, &state1).unwrap();
        state.set_last_reading_state(&ebook2, &state2).unwrap();
        state.set_last_reading_state(&ebook3, &state3).unwrap();

        let retrieved1 = state.get_last_reading_state(&ebook1).unwrap();
        let retrieved2 = state.get_last_reading_state(&ebook2).unwrap();
        let retrieved3 = state.get_last_reading_state(&ebook3).unwrap();

        assert_eq!(retrieved1.content_index, 1);
        assert_eq!(retrieved2.content_index, 2);
        assert_eq!(retrieved3.content_index, 3);

        state
            .insert_bookmark(&ebook1, "Bookmark 1", &state1)
            .unwrap();
        state
            .insert_bookmark(&ebook2, "Bookmark 2", &state2)
            .unwrap();
        state
            .insert_bookmark(&ebook3, "Bookmark 3", &state3)
            .unwrap();

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
