use super::{ChapterContent, Ebook, mime_from_extension};
use crate::models::{BookMetadata, TocEntry};
use eyre::Result;

/// Whether a [`TextBook`] file holds plain text or Markdown.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TextKind {
    Plain,
    Markdown,
}

/// Single-file plain-text or Markdown book. The whole file is one chapter;
/// the renderer reflows it through the shared HTML pipeline. Markdown books
/// resolve relative image links against the file's directory.
pub struct TextBook {
    path: String,
    kind: TextKind,
    contents: Vec<String>,
    toc: Vec<TocEntry>,
    metadata: BookMetadata,
    text: Option<String>,
}

impl TextBook {
    pub fn new(path: &str, kind: TextKind) -> Self {
        Self {
            path: path.to_string(),
            kind,
            contents: Vec::new(),
            toc: Vec::new(),
            metadata: BookMetadata::default(),
            text: None,
        }
    }

    fn file_name(&self) -> String {
        std::path::Path::new(&self.path)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| self.path.clone())
    }

    fn file_stem(&self) -> String {
        std::path::Path::new(&self.path)
            .file_stem()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| self.path.clone())
    }

    /// First ATX `# heading` of a Markdown file, if any.
    fn markdown_title(text: &str) -> Option<String> {
        text.lines().find_map(|line| {
            let line = line.trim_start();
            line.strip_prefix("# ")
                .map(str::trim)
                .filter(|t| !t.is_empty())
                .map(str::to_string)
        })
    }
}

impl Ebook for TextBook {
    fn path(&self) -> &str {
        &self.path
    }

    fn contents(&self) -> &Vec<String> {
        &self.contents
    }

    fn toc_entries(&self) -> &Vec<TocEntry> {
        &self.toc
    }

    fn get_meta(&self) -> &BookMetadata {
        &self.metadata
    }

    fn spine_href(&self, index: usize) -> Option<String> {
        self.contents.get(index).cloned()
    }

    fn initialize(&mut self) -> Result<()> {
        let bytes = std::fs::read(&self.path)?;
        let text = String::from_utf8_lossy(&bytes).into_owned();

        let title = match self.kind {
            TextKind::Markdown => Self::markdown_title(&text).unwrap_or_else(|| self.file_stem()),
            TextKind::Plain => self.file_stem(),
        };
        self.metadata = BookMetadata {
            title: Some(title),
            ..BookMetadata::default()
        };

        self.contents = vec![self.file_name()];
        self.text = Some(text);
        Ok(())
    }

    fn get_chapter(&mut self, index: usize) -> Result<ChapterContent> {
        if index >= self.contents.len() {
            return Err(eyre::eyre!("Content not found"));
        }
        let text = self
            .text
            .clone()
            .ok_or_else(|| eyre::eyre!("Book not initialized"))?;
        Ok(match self.kind {
            TextKind::Plain => ChapterContent::PlainText(text),
            TextKind::Markdown => ChapterContent::Markdown(text),
        })
    }

    /// Resources (image links in Markdown) resolve against the book file's
    /// directory; absolute paths and paths escaping that directory are
    /// rejected so a book cannot read arbitrary files.
    fn get_resource(&mut self, path: &str) -> Result<(String, Vec<u8>)> {
        if self.kind != TextKind::Markdown {
            return Err(eyre::eyre!("Image not found"));
        }
        let relative = std::path::Path::new(path);
        if relative.is_absolute()
            || relative
                .components()
                .any(|c| matches!(c, std::path::Component::ParentDir))
        {
            return Err(eyre::eyre!("Image not found"));
        }
        let base_dir = std::path::Path::new(&self.path)
            .parent()
            .unwrap_or_else(|| std::path::Path::new(""));
        let full = base_dir.join(relative);
        match std::fs::read(&full) {
            Ok(bytes) => Ok((mime_from_extension(path), bytes)),
            Err(_) => Err(eyre::eyre!("Image not found")),
        }
    }

    fn cleanup(&mut self) -> Result<()> {
        self.text = None;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_temp(dir: &tempfile::TempDir, name: &str, content: &str) -> String {
        let path = dir.path().join(name);
        std::fs::write(&path, content).unwrap();
        path.to_string_lossy().to_string()
    }

    #[test]
    fn test_plain_text_book() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let path = write_temp(&dir, "my-story.txt", "First.\n\nSecond.");
        let mut book = TextBook::new(&path, TextKind::Plain);
        book.initialize()?;

        assert_eq!(book.get_meta().title.as_deref(), Some("my-story"));
        assert_eq!(book.contents().len(), 1);
        assert_eq!(book.spine_href(0).as_deref(), Some("my-story.txt"));
        assert!(matches!(
            book.get_chapter(0)?,
            ChapterContent::PlainText(text) if text == "First.\n\nSecond."
        ));
        Ok(())
    }

    #[test]
    fn test_markdown_book_title_from_heading() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let path = write_temp(&dir, "notes.md", "intro\n\n# The Real Title\n\nBody.");
        let mut book = TextBook::new(&path, TextKind::Markdown);
        book.initialize()?;

        assert_eq!(book.get_meta().title.as_deref(), Some("The Real Title"));
        assert!(matches!(book.get_chapter(0)?, ChapterContent::Markdown(_)));
        Ok(())
    }

    #[test]
    fn test_markdown_book_title_fallback_to_stem() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let path = write_temp(&dir, "plain-notes.md", "no heading here");
        let mut book = TextBook::new(&path, TextKind::Markdown);
        book.initialize()?;

        assert_eq!(book.get_meta().title.as_deref(), Some("plain-notes"));
        Ok(())
    }

    #[test]
    fn test_get_chapter_out_of_range() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let path = write_temp(&dir, "a.txt", "x");
        let mut book = TextBook::new(&path, TextKind::Plain);
        book.initialize()?;

        assert!(book.get_chapter(1).is_err());
        Ok(())
    }

    #[test]
    fn test_markdown_resource_relative_to_file() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let path = write_temp(&dir, "book.md", "![cover](img/cover.png)");
        std::fs::create_dir(dir.path().join("img"))?;
        std::fs::write(dir.path().join("img/cover.png"), b"\x89PNG")?;

        let mut book = TextBook::new(&path, TextKind::Markdown);
        book.initialize()?;

        let (mime, bytes) = book.get_resource("img/cover.png")?;
        assert_eq!(mime, "image/png");
        assert_eq!(bytes, b"\x89PNG");
        Ok(())
    }

    #[test]
    fn test_resource_escaping_base_dir_rejected() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let path = write_temp(&dir, "book.md", "x");
        let mut book = TextBook::new(&path, TextKind::Markdown);
        book.initialize()?;

        assert!(book.get_resource("../secret.png").is_err());
        assert!(book.get_resource("/etc/passwd").is_err());
        Ok(())
    }

    #[test]
    fn test_initialize_nonexistent() {
        let mut book = TextBook::new("tests/fixtures/nonexistent.txt", TextKind::Plain);
        assert!(book.initialize().is_err());
    }
}
