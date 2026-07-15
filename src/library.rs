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
use walkdir::WalkDir;

use crate::models::ScannedBook;
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

/// Scan the given directories (following symlinks) and return every ebook
/// found, refreshing the metadata cache and pruning entries for files that
/// disappeared. Metadata comes from the cache when the file is unchanged,
/// from a sibling Calibre `metadata.opf` when present, or from the EPUB
/// itself otherwise.
pub fn scan_library_directories(dirs: &[String], state: &State) -> Result<Vec<ScannedBook>> {
    let mut books = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for dir in dirs {
        let root = expand_tilde(dir);
        for entry in WalkDir::new(&root)
            .follow_links(true)
            .into_iter()
            .filter_map(|e| e.ok())
        {
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
            // Canonicalize so entries merge with reading history, which
            // stores canonical paths (see Reader::normalize_ebook_path).
            let Ok(filepath) = std::fs::canonicalize(entry.path()) else {
                continue;
            };
            let filepath_str = filepath.to_string_lossy().to_string();
            if !seen.insert(filepath_str.clone()) {
                continue;
            }
            let mtime = file_mtime(&filepath);
            let (title, author) = match state.cached_library_file(&filepath_str, mtime)? {
                Some(cached) => cached,
                None => {
                    let (title, author) = extract_metadata(&filepath);
                    state.upsert_library_file(
                        &filepath_str,
                        mtime,
                        title.as_deref(),
                        author.as_deref(),
                    )?;
                    (title, author)
                }
            };
            books.push(ScannedBook {
                filepath: filepath_str,
                title,
                author,
            });
        }
    }
    let seen_paths: Vec<String> = books.iter().map(|b| b.filepath.clone()).collect();
    state.prune_library_files(&seen_paths)?;
    Ok(books)
}

fn file_mtime(path: &Path) -> i64 {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Title and author for an ebook file.
fn extract_metadata(path: &Path) -> (Option<String>, Option<String>) {
    if let Some(parent) = path.parent()
        && let Ok(xml) = std::fs::read_to_string(parent.join("metadata.opf"))
    {
        let (title, author) = parse_opf_metadata(&xml);
        if title.is_some() || author.is_some() {
            return (title, author);
        }
    }
    match crate::formats::open(&path.to_string_lossy()) {
        Ok(book) => {
            let meta = book.get_meta();
            (meta.title.clone(), meta.creator.clone())
        }
        Err(_) => (None, None),
    }
}

/// Extract `dc:title` and `dc:creator` from an OPF package document.
/// Multiple creators are joined with ", ".
fn parse_opf_metadata(xml: &str) -> (Option<String>, Option<String>) {
    let title = element_texts(xml, "dc:title").into_iter().next();
    let creators = element_texts(xml, "dc:creator");
    let author = if creators.is_empty() {
        None
    } else {
        Some(creators.join(", "))
    };
    (title, author)
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
        let (title, author) = parse_opf_metadata(SAMPLE_OPF);
        assert_eq!(title.as_deref(), Some("War & Peace"));
        assert_eq!(author.as_deref(), Some("Leo Tolstoy"));
    }

    #[test]
    fn test_parse_opf_metadata_multiple_creators() {
        let xml = r#"<metadata>
            <dc:title>What is this?</dc:title>
            <dc:creator opf:role="aut">Martine Batchelor</dc:creator>
            <dc:creator opf:role="aut">Stephen Batchelor</dc:creator>
        </metadata>"#;
        let (title, author) = parse_opf_metadata(xml);
        assert_eq!(title.as_deref(), Some("What is this?"));
        assert_eq!(
            author.as_deref(),
            Some("Martine Batchelor, Stephen Batchelor")
        );
    }

    #[test]
    fn test_parse_opf_metadata_missing_fields() {
        assert_eq!(parse_opf_metadata("<package></package>"), (None, None));
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
        let state = State::new_for_test();
        let books =
            scan_library_directories(&[dir.path().to_string_lossy().to_string()], &state).unwrap();
        assert_eq!(books.len(), 1);
        assert_eq!(books[0].title.as_deref(), Some("War & Peace"));
        assert_eq!(books[0].author.as_deref(), Some("Leo Tolstoy"));
        assert!(books[0].filepath.ends_with(".epub"));

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
}
