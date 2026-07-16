//! Library directory scanning.
//!
//! Walks the user-configured `library_directories` for ebook files and
//! resolves their title/author. Metadata is cached in SQLite keyed by
//! (canonical path, mtime) so rescans only touch new or changed files.
//! Calibre libraries are supported as plain directories: the per-book
//! `metadata.opf` that Calibre writes next to each ebook is preferred as a
//! metadata source because it avoids opening the EPUB archive.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use eyre::Result;
use rusqlite::{Connection, OpenFlags, OptionalExtension};
use walkdir::WalkDir;

use crate::models::{LibraryCacheEntry, ScannedBook};
use crate::state::State;

/// Expand a leading `~` or `~/` to the user's home directory.
pub fn expand_tilde(dir: &str) -> PathBuf {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .ok();
    match home {
        Some(home) if dir == "~" => PathBuf::from(home),
        Some(home) => match dir.strip_prefix("~/") {
            Some(rest) => Path::new(&home).join(rest),
            None => PathBuf::from(dir),
        },
        None => PathBuf::from(dir),
    }
}

/// Scan the given directories (without following directory symlinks) and return every ebook
/// found, refreshing the metadata cache and pruning entries for files that
/// disappeared. Metadata comes from the cache when the file is unchanged,
/// from a sibling Calibre `metadata.opf` when present, or from the EPUB
/// itself otherwise.
pub fn scan_library_directories(dirs: &[String], state: &State) -> Result<Vec<ScannedBook>> {
    let mut globally_seen: HashSet<String> = HashSet::new();
    for dir in dirs {
        let root = expand_tilde(dir);
        let Ok(root) = std::fs::canonicalize(&root) else {
            continue;
        };
        let root_str = root.to_string_lossy().to_string();
        let db_catalog = scan_calibre_database(&root).ok();
        let mut candidates = Vec::new();
        let mut walk_succeeded = true;
        // Do not follow directory symlinks: this avoids loops and duplicate
        // records. Canonical file paths still deduplicate overlapping roots.
        for result in WalkDir::new(&root).follow_links(false) {
            if db_catalog.is_some() {
                break;
            }
            let entry = match result {
                Ok(e) => e,
                Err(_) => {
                    walk_succeeded = false;
                    continue;
                }
            };
            if !entry.file_type().is_file() {
                continue;
            }
            let is_fb2_zip = entry.path().file_name().is_some_and(|name| {
                name.to_string_lossy()
                    .to_ascii_lowercase()
                    .ends_with(".fb2.zip")
            });
            let is_book = is_fb2_zip
                || entry.path().extension().is_some_and(|ext| {
                    ext.eq_ignore_ascii_case("epub")
                        || ext.eq_ignore_ascii_case("cbz")
                        || ext.eq_ignore_ascii_case("fb2")
                        || ext.eq_ignore_ascii_case("mobi")
                        || ext.eq_ignore_ascii_case("azw")
                        || ext.eq_ignore_ascii_case("azw3")
                });
            if !is_book {
                continue;
            }
            if let Ok(path) = std::fs::canonicalize(entry.path()) {
                candidates.push(path);
            }
        }
        let catalog: Vec<(PathBuf, Option<BookMetadata>, String)> = match db_catalog {
            Some(entries) => entries,
            None => candidates
                .into_iter()
                .map(|path| {
                    let parent = path.parent().unwrap_or(&root);
                    let record = if parent.join("metadata.opf").is_file() {
                        parent
                    } else {
                        path.as_path()
                    };
                    let key = std::fs::canonicalize(record)
                        .unwrap_or_else(|_| record.to_path_buf())
                        .to_string_lossy()
                        .to_string();
                    (path, None, key)
                })
                .collect(),
        };
        let mut seen_in_root = Vec::new();
        for (filepath, database_metadata, book_key) in catalog {
            let canonical = std::fs::canonicalize(&filepath).unwrap_or(filepath);
            if !globally_seen.insert(canonical.to_string_lossy().to_string()) {
                continue;
            }
            let filepath = canonical;
            let filepath_str = filepath.to_string_lossy().to_string();
            let parent = filepath.parent().unwrap_or(&root);
            let opf = parent.join("metadata.opf");
            let cover = parent.join("cover.jpg");
            let metadata_mtime = file_mtime(&opf).max(file_mtime(&root.join("metadata.db")));
            let cover_mtime = file_mtime(&cover);
            let mtime = file_mtime(&filepath);
            let entry = match state.cached_library_entry(
                &filepath_str,
                mtime,
                metadata_mtime,
                cover_mtime,
            )? {
                Some(cached) if cached.library_root == root_str => cached,
                _ => {
                    let metadata = database_metadata.unwrap_or_else(|| extract_metadata(&filepath));
                    LibraryCacheEntry {
                        filepath: filepath_str.clone(),
                        library_root: root_str.clone(),
                        book_key,
                        mtime,
                        metadata_mtime,
                        cover_mtime,
                        title: metadata.title,
                        author: metadata.author,
                        series: metadata.series,
                        series_index: metadata.series_index,
                        tags: metadata.tags,
                        language: metadata.language,
                        publisher: metadata.publisher,
                        description: metadata.description,
                        cover_path: cover.is_file().then(|| cover.to_string_lossy().to_string()),
                    }
                }
            };
            state.upsert_library_entry(&entry)?;
            seen_in_root.push(filepath_str);
        }
        if walk_succeeded {
            state.prune_library_root(&root_str, &seen_in_root)?;
        }
    }
    state.get_scanned_library_files()
}

/// Read a Calibre catalog without taking locks or ever making the database
/// writable. Any schema/query error rejects the catalog so callers can fall
/// back to the directory/OPF scanner.
fn scan_calibre_database(
    root: &Path,
) -> rusqlite::Result<Vec<(PathBuf, Option<BookMetadata>, String)>> {
    let db = root.join("metadata.db");
    if !db.is_file() {
        return Err(rusqlite::Error::InvalidPath(db));
    }
    let flags = OpenFlags::SQLITE_OPEN_READ_ONLY
        | OpenFlags::SQLITE_OPEN_URI
        | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    let uri = format!("file:{}?immutable=1", db.to_string_lossy());
    let conn = Connection::open_with_flags(uri, flags)
        .or_else(|_| Connection::open_with_flags(&db, OpenFlags::SQLITE_OPEN_READ_ONLY))?;

    // Requiring these core Calibre tables also makes an incompatible schema
    // fail atomically rather than returning a misleading partial catalog.
    let mut books =
        conn.prepare("SELECT id, title, path, series_index, has_cover FROM books ORDER BY id")?;
    let rows = books.query_map([], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, Option<String>>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, Option<f32>>(3)?,
            row.get::<_, Option<bool>>(4)?,
        ))
    })?;
    let mut catalog = Vec::new();
    for row in rows {
        let (id, title, relative_dir, series_index, _has_cover) = row?;
        let book_dir = root.join(&relative_dir);
        let mut formats = conn.prepare("SELECT format, name FROM data WHERE book = ?1")?;
        let files = formats
            .query_map([id], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let author = joined_relation(&conn, "authors", "books_authors_link", "name", "author", id)?;
        let series = single_relation(&conn, "series", "books_series_link", "name", "series", id)?;
        let tags = relation_values(&conn, "tags", "books_tags_link", "name", "tag", id)?;
        let language = single_relation(
            &conn,
            "languages",
            "books_languages_link",
            "lang_code",
            "lang_code",
            id,
        )?;
        let publisher = single_relation(
            &conn,
            "publishers",
            "books_publishers_link",
            "name",
            "publisher",
            id,
        )?;
        let description = conn
            .query_row("SELECT text FROM comments WHERE book = ?1", [id], |r| {
                r.get(0)
            })
            .optional()?
            .and_then(|text: String| plain_text_description(&text));
        let metadata = BookMetadata {
            title,
            author,
            series,
            series_index,
            tags,
            language,
            publisher,
            description,
        };
        let key = book_dir.to_string_lossy().to_string();
        for (format, name) in files {
            let ext = format.to_ascii_lowercase();
            if !matches!(
                ext.as_str(),
                "epub" | "cbz" | "fb2" | "mobi" | "azw" | "azw3"
            ) {
                continue;
            }
            let path = book_dir.join(format!("{name}.{ext}"));
            if path.is_file() {
                catalog.push((path, Some(metadata.clone()), key.clone()));
            }
        }
    }
    Ok(catalog)
}

fn relation_values(
    conn: &Connection,
    table: &str,
    link: &str,
    value_column: &str,
    foreign_key: &str,
    book: i64,
) -> rusqlite::Result<Vec<String>> {
    let sql = format!(
        "SELECT x.{value_column} FROM {table} x JOIN {link} l ON l.{foreign_key} = x.id WHERE l.book = ?1 ORDER BY x.{value_column}"
    );
    let mut stmt = conn.prepare(&sql)?;
    stmt.query_map([book], |r| r.get(0))?
        .collect::<rusqlite::Result<Vec<_>>>()
}

fn single_relation(
    conn: &Connection,
    table: &str,
    link: &str,
    value_column: &str,
    key: &str,
    book: i64,
) -> rusqlite::Result<Option<String>> {
    Ok(relation_values(conn, table, link, value_column, key, book)?
        .into_iter()
        .next())
}

fn joined_relation(
    conn: &Connection,
    table: &str,
    link: &str,
    value_column: &str,
    key: &str,
    book: i64,
) -> rusqlite::Result<Option<String>> {
    let values = relation_values(conn, table, link, value_column, key, book)?;
    Ok((!values.is_empty()).then(|| values.join(", ")))
}

fn file_mtime(path: &Path) -> i64 {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_nanos() as i64)
        .unwrap_or(0)
}

/// Title and author for an ebook file.
#[derive(Clone, Default)]
struct BookMetadata {
    title: Option<String>,
    author: Option<String>,
    series: Option<String>,
    series_index: Option<f32>,
    tags: Vec<String>,
    language: Option<String>,
    publisher: Option<String>,
    description: Option<String>,
}

fn extract_metadata(path: &Path) -> BookMetadata {
    if let Some(parent) = path.parent()
        && let Ok(xml) = std::fs::read_to_string(parent.join("metadata.opf"))
    {
        let metadata = parse_opf_metadata(&xml);
        if metadata.title.is_some() || metadata.author.is_some() {
            return metadata;
        }
    }
    match crate::formats::open(&path.to_string_lossy()) {
        Ok(book) => {
            let meta = book.get_meta();
            BookMetadata {
                title: meta.title.clone(),
                author: meta.creator.clone(),
                ..Default::default()
            }
        }
        Err(_) => BookMetadata::default(),
    }
}

/// Extract `dc:title` and `dc:creator` from an OPF package document.
/// Multiple creators are joined with ", ".
fn parse_opf_metadata(xml: &str) -> BookMetadata {
    let title = element_texts(xml, "dc:title").into_iter().next();
    let creators = element_texts(xml, "dc:creator");
    let author = if creators.is_empty() {
        None
    } else {
        Some(creators.join(", "))
    };
    let meta = |name: &str| {
        let pattern = format!(
            r#"(?s)<meta\s+[^>]*name=["']{}["'][^>]*content=["'](.*?)["'][^>]*/?>"#,
            regex::escape(name)
        );
        regex::Regex::new(&pattern)
            .ok()?
            .captures(xml)
            .map(|c| unescape_xml(c[1].trim()))
    };
    BookMetadata {
        title,
        author,
        series: meta("calibre:series"),
        series_index: meta("calibre:series_index").and_then(|s| s.parse().ok()),
        tags: element_texts(xml, "dc:subject"),
        language: element_texts(xml, "dc:language").into_iter().next(),
        publisher: element_texts(xml, "dc:publisher").into_iter().next(),
        description: element_texts(xml, "dc:description")
            .into_iter()
            .next()
            .and_then(|text| plain_text_description(&text)),
    }
}

/// Calibre stores comments as an HTML fragment, and OPF descriptions often
/// contain the same escaped fragment. Convert either form to clean text before
/// it reaches the cache or UI. A large wrap width leaves final wrapping to the
/// responsive details panel.
pub(crate) fn plain_text_description(raw: &str) -> Option<String> {
    let text = html2text::config::plain_no_decorate()
        .link_footnotes(false)
        .string_from_read(raw.as_bytes(), 10_000)
        .ok()?;
    let mut lines = Vec::new();
    let mut previous_blank = true;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            if !previous_blank {
                lines.push(String::new());
            }
            previous_blank = true;
        } else {
            lines.push(line.to_string());
            previous_blank = false;
        }
    }
    while lines.last().is_some_and(String::is_empty) {
        lines.pop();
    }
    let text = lines.join("\n");
    (!text.is_empty()).then_some(text)
}

fn element_texts(xml: &str, tag: &str) -> Vec<String> {
    let pattern = format!(
        r"(?s)<{tag}(?:\s[^>]*)?>(.*?)</{tag}>",
        tag = regex::escape(tag)
    );
    let re = regex::Regex::new(&pattern).expect("static element pattern");
    re.captures_iter(xml)
        .filter_map(|c| {
            let text = unescape_xml(c[1].trim());
            if text.is_empty() { None } else { Some(text) }
        })
        .collect()
}

/// Resolve the predefined XML entities. OPF metadata text does not nest
/// markup, so this is sufficient for titles and creator names.
fn unescape_xml(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&#39;", "'")
        .replace("&amp;", "&")
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_OPF: &str = r#"<?xml version='1.0' encoding='utf-8'?>
<package xmlns="http://www.idpf.org/2007/opf" unique-identifier="uuid_id" version="2.0">
    <metadata xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:opf="http://www.idpf.org/2007/opf">
        <dc:identifier opf:scheme="calibre" id="calibre_id">246</dc:identifier>
        <dc:title>War &amp; Peace</dc:title>
        <dc:creator opf:file-as="Tolstoy, Leo" opf:role="aut">Leo Tolstoy</dc:creator>
        <dc:language>eng</dc:language>
    </metadata>
</package>"#;

    #[test]
    fn test_parse_opf_metadata() {
        let metadata = parse_opf_metadata(SAMPLE_OPF);
        assert_eq!(metadata.title.as_deref(), Some("War & Peace"));
        assert_eq!(metadata.author.as_deref(), Some("Leo Tolstoy"));
    }

    #[test]
    fn test_parse_opf_metadata_multiple_creators() {
        let xml = r#"<metadata>
            <dc:title>What is this?</dc:title>
            <dc:creator opf:role="aut">Martine Batchelor</dc:creator>
            <dc:creator opf:role="aut">Stephen Batchelor</dc:creator>
        </metadata>"#;
        let metadata = parse_opf_metadata(xml);
        assert_eq!(metadata.title.as_deref(), Some("What is this?"));
        assert_eq!(
            metadata.author.as_deref(),
            Some("Martine Batchelor, Stephen Batchelor")
        );
    }

    #[test]
    fn test_parse_opf_metadata_missing_fields() {
        let metadata = parse_opf_metadata("<package></package>");
        assert!(metadata.title.is_none() && metadata.author.is_none());
    }

    #[test]
    fn test_plain_text_description_removes_calibre_html() {
        let html = "<p>Written with <b>clarity</b> &amp; care.</p><p>Second<br>line.</p>";
        assert_eq!(
            plain_text_description(html).as_deref(),
            Some("Written with clarity & care.\n\nSecond\nline.")
        );
        assert_eq!(
            plain_text_description("  Plain text.  ").as_deref(),
            Some("Plain text.")
        );
        assert_eq!(plain_text_description("<p> </p>"), None);
    }

    #[test]
    fn test_expand_tilde() {
        let home = std::env::var("HOME").unwrap();
        assert_eq!(expand_tilde("~"), PathBuf::from(&home));
        assert_eq!(expand_tilde("~/Calibre"), Path::new(&home).join("Calibre"));
        assert_eq!(expand_tilde("/abs/path"), PathBuf::from("/abs/path"));
    }

    /// Build a Calibre-style library: Author/Title (id)/book.epub + metadata.opf.
    fn make_calibre_dir() -> tempfile::TempDir {
        let dir = tempfile::TempDir::new().unwrap();
        let book_dir = dir.path().join("Leo Tolstoy").join("War & Peace (246)");
        std::fs::create_dir_all(&book_dir).unwrap();
        std::fs::copy(
            "tests/fixtures/small.epub",
            book_dir.join("War & Peace - Leo Tolstoy.epub"),
        )
        .unwrap();
        std::fs::write(book_dir.join("metadata.opf"), SAMPLE_OPF).unwrap();
        dir
    }

    #[test]
    fn test_scan_calibre_directory_uses_opf() {
        let dir = make_calibre_dir();
        let book_dir = dir.path().join("Leo Tolstoy").join("War & Peace (246)");
        std::fs::write(book_dir.join("War & Peace - Leo Tolstoy.mobi"), b"mobi").unwrap();
        std::fs::write(book_dir.join("War & Peace - Leo Tolstoy.fb2"), b"fb2").unwrap();
        let state = State::new_for_test();
        let books =
            scan_library_directories(&[dir.path().to_string_lossy().to_string()], &state).unwrap();
        assert_eq!(books.len(), 1);
        assert_eq!(books[0].title.as_deref(), Some("War & Peace"));
        assert_eq!(books[0].author.as_deref(), Some("Leo Tolstoy"));
        assert!(books[0].filepath.ends_with(".epub"));
        assert_eq!(books[0].formats.len(), 3);

        // The scan populated the cache.
        let cached = state.get_scanned_library_files().unwrap();
        assert_eq!(cached.len(), 1);
        assert_eq!(cached[0].title.as_deref(), Some("War & Peace"));
    }

    #[test]
    fn test_scan_falls_back_to_epub_metadata() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::copy("tests/fixtures/small.epub", dir.path().join("small.epub")).unwrap();
        let state = State::new_for_test();
        let books =
            scan_library_directories(&[dir.path().to_string_lossy().to_string()], &state).unwrap();
        assert_eq!(books.len(), 1);
        // No metadata.opf, so metadata must come from inside the EPUB.
        assert!(books[0].title.is_some());
    }

    #[test]
    fn test_scan_uses_cache_when_mtime_matches() {
        let dir = make_calibre_dir();
        let state = State::new_for_test();
        let dirs = [dir.path().to_string_lossy().to_string()];

        let books = scan_library_directories(&dirs, &state).unwrap();
        let filepath = books[0].filepath.clone();
        let mtime = file_mtime(Path::new(&filepath));

        // Overwrite the cached title; the rescan must return the cached
        // value untouched (proving the file was not re-parsed).
        state
            .upsert_library_file(&filepath, mtime, Some("Cached Title"), None)
            .unwrap();
        let books = scan_library_directories(&dirs, &state).unwrap();
        assert_eq!(books[0].title.as_deref(), Some("Cached Title"));
    }

    #[test]
    fn test_scan_prunes_deleted_files() {
        let dir = make_calibre_dir();
        let extra = dir.path().join("loose.epub");
        std::fs::copy("tests/fixtures/small.epub", &extra).unwrap();
        let state = State::new_for_test();
        let dirs = [dir.path().to_string_lossy().to_string()];

        let books = scan_library_directories(&dirs, &state).unwrap();
        assert_eq!(books.len(), 2);

        std::fs::remove_file(&extra).unwrap();
        let books = scan_library_directories(&dirs, &state).unwrap();
        assert_eq!(books.len(), 1);
        assert_eq!(state.get_scanned_library_files().unwrap().len(), 1);
    }

    #[test]
    fn test_parse_rich_calibre_metadata() {
        let xml = r#"<metadata>
          <dc:title>Guards! Guards!</dc:title><dc:creator>Terry Pratchett</dc:creator>
          <dc:subject>Fantasy</dc:subject><dc:subject>City Watch</dc:subject>
          <dc:language>eng</dc:language><dc:publisher>Corgi</dc:publisher>
          <dc:description>A dragon appears.</dc:description>
          <meta name="calibre:series" content="Discworld"/>
          <meta name="calibre:series_index" content="8"/>
        </metadata>"#;
        let metadata = parse_opf_metadata(xml);
        assert_eq!(metadata.series.as_deref(), Some("Discworld"));
        assert_eq!(metadata.series_index, Some(8.0));
        assert_eq!(metadata.tags, ["Fantasy", "City Watch"]);
        assert_eq!(metadata.publisher.as_deref(), Some("Corgi"));
        assert_eq!(metadata.description.as_deref(), Some("A dragon appears."));
    }

    #[test]
    fn test_opf_change_invalidates_cache_without_ebook_change() {
        let dir = make_calibre_dir();
        let state = State::new_for_test();
        let dirs = [dir.path().to_string_lossy().to_string()];
        let first = scan_library_directories(&dirs, &state).unwrap();
        assert_eq!(first[0].title.as_deref(), Some("War & Peace"));
        let opf = dir
            .path()
            .join("Leo Tolstoy/War & Peace (246)/metadata.opf");
        std::fs::write(&opf, SAMPLE_OPF.replace("War &amp; Peace", "Anna Karenina")).unwrap();
        let second = scan_library_directories(&dirs, &state).unwrap();
        assert_eq!(second[0].title.as_deref(), Some("Anna Karenina"));
    }

    #[test]
    fn test_unavailable_root_keeps_cache_while_other_root_prunes() {
        let unavailable = make_calibre_dir();
        let available = tempfile::TempDir::new().unwrap();
        let extra = available.path().join("extra.epub");
        std::fs::copy("tests/fixtures/small.epub", &extra).unwrap();
        let state = State::new_for_test();
        let dirs = [
            unavailable.path().to_string_lossy().to_string(),
            available.path().to_string_lossy().to_string(),
        ];
        assert_eq!(scan_library_directories(&dirs, &state).unwrap().len(), 2);
        std::fs::remove_dir_all(unavailable.path()).unwrap();
        std::fs::remove_file(extra).unwrap();
        let books = scan_library_directories(&dirs, &state).unwrap();
        assert_eq!(books.len(), 1);
        assert_eq!(books[0].title.as_deref(), Some("War & Peace"));
    }

    fn make_calibre_database(root: &Path) {
        let db = Connection::open(root.join("metadata.db")).unwrap();
        db.execute_batch(
            "CREATE TABLE books (id INTEGER PRIMARY KEY, title TEXT, path TEXT, series_index REAL, has_cover BOOL);
             CREATE TABLE data (id INTEGER PRIMARY KEY, book INTEGER, format TEXT, name TEXT);
             CREATE TABLE authors (id INTEGER PRIMARY KEY, name TEXT);
             CREATE TABLE books_authors_link (book INTEGER, author INTEGER);
             CREATE TABLE series (id INTEGER PRIMARY KEY, name TEXT);
             CREATE TABLE books_series_link (book INTEGER, series INTEGER);
             CREATE TABLE tags (id INTEGER PRIMARY KEY, name TEXT);
             CREATE TABLE books_tags_link (book INTEGER, tag INTEGER);
             CREATE TABLE languages (id INTEGER PRIMARY KEY, lang_code TEXT);
             CREATE TABLE books_languages_link (book INTEGER, lang_code INTEGER);
             CREATE TABLE publishers (id INTEGER PRIMARY KEY, name TEXT);
             CREATE TABLE books_publishers_link (book INTEGER, publisher INTEGER);
             CREATE TABLE comments (book INTEGER PRIMARY KEY, text TEXT);
             INSERT INTO books VALUES (7, 'Guards! Guards!', 'Terry Pratchett/Guards! Guards! (7)', 8, 1);
             INSERT INTO data VALUES (1, 7, 'EPUB', 'Guards! Guards! - Terry Pratchett');
             INSERT INTO data VALUES (2, 7, 'MOBI', 'Guards! Guards! - Terry Pratchett');
             INSERT INTO authors VALUES (1, 'Terry Pratchett');
             INSERT INTO books_authors_link VALUES (7, 1);
             INSERT INTO series VALUES (1, 'Discworld');
             INSERT INTO books_series_link VALUES (7, 1);
             INSERT INTO tags VALUES (1, 'Fantasy');
             INSERT INTO tags VALUES (2, 'City Watch');
             INSERT INTO books_tags_link VALUES (7, 1);
             INSERT INTO books_tags_link VALUES (7, 2);
             INSERT INTO languages VALUES (1, 'eng');
             INSERT INTO books_languages_link VALUES (7, 1);
             INSERT INTO publishers VALUES (1, 'Corgi');
             INSERT INTO books_publishers_link VALUES (7, 1);
             INSERT INTO comments VALUES (7, 'A dragon appears.');",
        )
        .unwrap();
    }

    #[test]
    fn test_scan_calibre_metadata_database_catalog() {
        let dir = tempfile::TempDir::new().unwrap();
        let book_dir = dir.path().join("Terry Pratchett/Guards! Guards! (7)");
        std::fs::create_dir_all(&book_dir).unwrap();
        std::fs::copy(
            "tests/fixtures/small.epub",
            book_dir.join("Guards! Guards! - Terry Pratchett.epub"),
        )
        .unwrap();
        std::fs::write(
            book_dir.join("Guards! Guards! - Terry Pratchett.mobi"),
            b"mobi",
        )
        .unwrap();
        make_calibre_database(dir.path());

        let state = State::new_for_test();
        let books =
            scan_library_directories(&[dir.path().to_string_lossy().to_string()], &state).unwrap();
        assert_eq!(books.len(), 1);
        assert_eq!(books[0].title.as_deref(), Some("Guards! Guards!"));
        assert_eq!(books[0].author.as_deref(), Some("Terry Pratchett"));
        assert_eq!(books[0].series.as_deref(), Some("Discworld"));
        assert_eq!(books[0].series_index, Some(8.0));
        assert_eq!(books[0].tags, ["City Watch", "Fantasy"]);
        assert_eq!(books[0].language.as_deref(), Some("eng"));
        assert_eq!(books[0].publisher.as_deref(), Some("Corgi"));
        assert_eq!(books[0].description.as_deref(), Some("A dragon appears."));
        assert_eq!(books[0].formats.len(), 2);
    }

    #[test]
    fn test_incompatible_calibre_database_falls_back_to_opf_scan() {
        let dir = make_calibre_dir();
        Connection::open(dir.path().join("metadata.db"))
            .unwrap()
            .execute("CREATE TABLE unrelated (id INTEGER)", [])
            .unwrap();
        let state = State::new_for_test();
        let books =
            scan_library_directories(&[dir.path().to_string_lossy().to_string()], &state).unwrap();
        assert_eq!(books.len(), 1);
        assert_eq!(books[0].title.as_deref(), Some("War & Peace"));
    }
}
