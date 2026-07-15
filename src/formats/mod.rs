//! Ebook format backends.
//!
//! A format backend implements [`Ebook`]: it exposes metadata, the table of
//! contents, per-chapter raw payloads ([`ChapterContent`]), and resource
//! (image) access. Turning a chapter payload into wrapped, styled text lines
//! is the renderer's job (`crate::renderer`), so backends stay layout-free.

pub mod cbz;
pub mod epub;
pub mod fb2;
pub mod mobi;
pub mod text;

pub use cbz::Cbz;
pub use epub::Epub;
pub use fb2::Fb2;
pub use mobi::MobiBook;
pub use text::{TextBook, TextKind};

use crate::css::StyledClasses;
use crate::models::{BookMetadata, TocEntry};
use eyre::Result;
use std::sync::LazyLock;

static EMPTY_STYLED_CLASSES: LazyLock<StyledClasses> = LazyLock::new(StyledClasses::default);

/// Raw chapter payload as stored in the book file. The renderer converts
/// every variant into HTML and feeds it through the shared parse pipeline.
#[derive(Debug, Clone, PartialEq)]
pub enum ChapterContent {
    Html(String),
    PlainText(String),
    Markdown(String),
    /// Resource path (inside the book) of a full-page image chapter,
    /// e.g. a comic-book page.
    ImagePage(String),
}

impl ChapterContent {
    /// The raw payload used for content fingerprinting (book identity).
    /// Must be stable across releases: highlight anchoring and reading
    /// state are keyed by hashes of this text.
    pub fn fingerprint_text(&self) -> &str {
        match self {
            ChapterContent::Html(text)
            | ChapterContent::PlainText(text)
            | ChapterContent::Markdown(text)
            | ChapterContent::ImagePage(text) => text,
        }
    }
}

pub trait Ebook {
    fn path(&self) -> &str;
    /// Chapter identifiers in reading order (format-specific, e.g. EPUB
    /// spine idrefs). Their count defines the valid chapter index range.
    fn contents(&self) -> &Vec<String>;
    fn toc_entries(&self) -> &Vec<TocEntry>;
    fn get_meta(&self) -> &BookMetadata;
    /// Stable per-chapter identifier (resource path inside the book).
    /// Highlight anchoring and book identity depend on it staying stable
    /// for a given file across releases.
    fn spine_href(&self, index: usize) -> Option<String>;

    fn initialize(&mut self) -> Result<()>;
    /// Raw payload of the chapter at `index` (see [`Self::contents`]).
    fn get_chapter(&mut self, index: usize) -> Result<ChapterContent>;
    /// MIME type and bytes of a resource (image) inside the book.
    fn get_resource(&mut self, path: &str) -> Result<(String, Vec<u8>)>;
    fn get_cover(&mut self) -> Option<(String, Vec<u8>)> {
        None
    }
    /// Chapter index for an internal link target. Only meaningful for
    /// formats with intra-book links (HTML-based ones).
    fn content_index_for_href(&self, _href: &str) -> Option<usize> {
        None
    }
    /// Classes that CSS marks italic/bold, recovered during parsing.
    /// Only HTML-based formats with stylesheets have any.
    fn styled_classes(&self) -> &StyledClasses {
        &EMPTY_STYLED_CLASSES
    }
    fn cleanup(&mut self) -> Result<()>;
}

/// Open and initialize the right format backend for `path`, picked by file
/// extension with a magic-bytes fallback for misnamed files.
pub fn open(path: &str) -> Result<Box<dyn Ebook>> {
    let extension = std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default();

    let mut book: Box<dyn Ebook> = match extension.as_str() {
        "epub" => Box::new(Epub::new(path)),
        "txt" | "text" => Box::new(TextBook::new(path, TextKind::Plain)),
        "md" | "markdown" => Box::new(TextBook::new(path, TextKind::Markdown)),
        "cbz" => Box::new(Cbz::new(path)),
        "fb2" => Box::new(Fb2::new(path)),
        "zip" if path.to_ascii_lowercase().ends_with(".fb2.zip") => Box::new(Fb2::new(path)),
        "mobi" | "azw" | "azw3" => Box::new(MobiBook::new(path)),
        _ if has_zip_magic(path) => Box::new(Epub::new(path)),
        _ => eyre::bail!("Unsupported ebook format: {}", path),
    };
    book.initialize()?;
    Ok(book)
}

fn has_zip_magic(path: &str) -> bool {
    use std::io::Read;
    let Ok(mut file) = std::fs::File::open(path) else {
        return false;
    };
    let mut magic = [0u8; 4];
    file.read_exact(&mut magic).is_ok() && &magic == b"PK\x03\x04"
}

/// Resolve an href/src relative to a content document path inside the book,
/// normalizing `.` and `..` components. Leading `/` means book-root-relative.
pub fn resolve_relative_resource(href: &str, base_content: Option<&str>) -> Option<String> {
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

pub(crate) fn escape_html(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(ch),
        }
    }
    out
}

pub(crate) fn mime_from_extension(path: &str) -> String {
    let ext = std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "gif" => "image/gif",
        "svg" => "image/svg+xml",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        _ => "application/octet-stream",
    }
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_open_epub_by_extension() -> Result<()> {
        let book = open("tests/fixtures/small.epub")?;
        assert!(!book.contents().is_empty());
        assert!(book.get_meta().title.is_some());
        Ok(())
    }

    #[test]
    fn test_open_unsupported_format() {
        let error = open("Cargo.toml").err().expect("open should fail");
        assert!(error.to_string().contains("Unsupported ebook format"));
    }

    #[test]
    fn test_open_nonexistent_file() {
        assert!(open("tests/fixtures/nonexistent.epub").is_err());
    }

    #[test]
    fn test_open_text_and_markdown_by_extension() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let txt = dir.path().join("story.txt");
        std::fs::write(&txt, "Hello.")?;
        let md = dir.path().join("notes.md");
        std::fs::write(&md, "# Notes\n\nHello.")?;

        let txt_book = open(&txt.to_string_lossy())?;
        assert_eq!(txt_book.get_meta().title.as_deref(), Some("story"));

        let md_book = open(&md.to_string_lossy())?;
        assert_eq!(md_book.get_meta().title.as_deref(), Some("Notes"));
        Ok(())
    }

    #[test]
    fn test_open_epub_by_magic_bytes() -> Result<()> {
        // A zip-magic file without an .epub extension still opens as EPUB.
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("misnamed-book");
        std::fs::copy("tests/fixtures/small.epub", &path)?;
        let book = open(&path.to_string_lossy())?;
        assert!(!book.contents().is_empty());
        Ok(())
    }

    #[test]
    fn test_fingerprint_text_all_variants() {
        for content in [
            ChapterContent::Html("x".into()),
            ChapterContent::PlainText("x".into()),
            ChapterContent::Markdown("x".into()),
            ChapterContent::ImagePage("x".into()),
        ] {
            assert_eq!(content.fingerprint_text(), "x");
        }
    }

    #[test]
    fn test_mime_from_extension() {
        assert_eq!(mime_from_extension("a/b.JPG"), "image/jpeg");
        assert_eq!(mime_from_extension("cover.png"), "image/png");
        assert_eq!(mime_from_extension("pic.svg"), "image/svg+xml");
        assert_eq!(mime_from_extension("noext"), "application/octet-stream");
    }
}
