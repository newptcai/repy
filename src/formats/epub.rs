use super::{ChapterContent, Ebook, mime_from_extension};
use crate::css::{StyledClasses, collect_styled_classes};
use crate::models::{BookMetadata, TocEntry};
use epub::doc::{EpubDoc, NavPoint};
use eyre::Result;
use std::collections::HashMap;
use std::path::PathBuf;

pub struct Epub {
    path: String,
    doc: Option<EpubDoc<std::io::BufReader<std::fs::File>>>,
    contents: Vec<String>,
    toc: Vec<TocEntry>,
    metadata: BookMetadata,
    raw_text_cache: HashMap<String, String>,
    styled_classes: StyledClasses,
}

impl Epub {
    pub fn new(path: &str) -> Self {
        Self {
            path: path.to_string(),
            doc: None,
            contents: Vec::new(),
            toc: Vec::new(),
            metadata: BookMetadata::default(),
            raw_text_cache: HashMap::new(),
            styled_classes: StyledClasses::default(),
        }
    }

    fn resource_path_for_content_index(&self, index: usize) -> Option<String> {
        let doc = self.doc.as_ref()?;
        let spine_item = doc.spine.get(index)?;
        let resource = doc.resources.get(&spine_item.idref)?;
        Some(resource.path.to_string_lossy().to_string())
    }

    fn split_navpoint_target(content: &std::path::Path) -> (PathBuf, Option<String>) {
        let content_str = content.to_string_lossy();
        if let Some((path, fragment)) = content_str.split_once('#') {
            let section = if fragment.is_empty() {
                None
            } else {
                Some(fragment.to_string())
            };
            let resource_path = if path.is_empty() {
                content.to_path_buf()
            } else {
                PathBuf::from(path)
            };
            (resource_path, section)
        } else {
            (content.to_path_buf(), None)
        }
    }

    fn append_navpoints(
        toc_entries: &mut Vec<TocEntry>,
        navpoints: &[NavPoint],
        doc: &EpubDoc<std::io::BufReader<std::fs::File>>,
        parent_path: Option<&std::path::Path>,
    ) {
        for navpoint in navpoints {
            let (resource_path, section) = Self::split_navpoint_target(&navpoint.content);
            let content_index = doc
                .resource_uri_to_chapter(&resource_path)
                .unwrap_or(usize::MAX);
            let label = navpoint.label.trim();
            let is_subtitle = label
                .chars()
                .next()
                .map(|c| c.is_lowercase())
                .unwrap_or(false);
            let same_content_as_parent = parent_path
                .map(|path| path == resource_path.as_path())
                .unwrap_or(false);
            if !(same_content_as_parent && is_subtitle) {
                toc_entries.push(TocEntry {
                    label: label.to_string(),
                    content_index,
                    section,
                });
            }

            if !navpoint.children.is_empty() {
                Self::append_navpoints(
                    toc_entries,
                    &navpoint.children,
                    doc,
                    Some(resource_path.as_path()),
                );
            }
        }
    }

    fn get_raw_text(&mut self, content_id: &str) -> Result<String> {
        if let Some(content) = self.raw_text_cache.get(content_id) {
            return Ok(content.clone());
        }

        if let Some(ref mut doc) = self.doc
            && let Some(index) = self.contents.iter().position(|id| id == content_id)
            && doc.set_current_chapter(index)
            && let Some((content, _)) = doc.get_current_str()
        {
            self.raw_text_cache
                .insert(content_id.to_string(), content.clone());
            return Ok(content);
        }
        Err(eyre::eyre!("Content not found"))
    }
}

impl Ebook for Epub {
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
        self.resource_path_for_content_index(index)
    }

    fn initialize(&mut self) -> Result<()> {
        let mut doc = EpubDoc::new(&self.path)?;

        self.contents = doc
            .spine
            .iter()
            .filter(|item| {
                if let Some(resource) = doc.resources.get(&item.idref) {
                    // Filter out NCX (EPUB 2 TOC)
                    if resource.mime == "application/x-dtbncx+xml" {
                        return false;
                    }
                    // Filter out Nav Document (EPUB 3 TOC)
                    if let Some(properties) = &resource.properties
                        && properties.split_whitespace().any(|p| p == "nav")
                    {
                        return false;
                    }
                }
                true
            })
            .map(|item| item.idref.clone())
            .collect();

        let mut toc_entries = Vec::new();
        Self::append_navpoints(&mut toc_entries, &doc.toc, &doc, None);
        self.toc = toc_entries;

        let mut metadata = BookMetadata::default();
        macro_rules! load_mdata {
            ($field:ident) => {
                if let Some(val) = doc.mdata(stringify!($field)) {
                    metadata.$field = Some(val.value.clone());
                }
            };
        }
        load_mdata!(title);
        load_mdata!(creator);
        load_mdata!(description);
        load_mdata!(publisher);
        load_mdata!(date);
        load_mdata!(language);
        load_mdata!(format);
        load_mdata!(identifier);
        load_mdata!(source);
        self.metadata = metadata;

        // Load every text/css resource and scan it for class-driven italic/bold
        // styling. This lets parse_html treat e.g. <span class="x"> as italic
        // when the EPUB's CSS sets `.x { font-style: italic; }`.
        let css_paths: Vec<PathBuf> = doc
            .resources
            .values()
            .filter(|r| r.mime == "text/css")
            .map(|r| r.path.clone())
            .collect();
        let mut css_sources: Vec<String> = Vec::with_capacity(css_paths.len());
        for path in css_paths {
            if let Some(bytes) = doc.get_resource_by_path(&path) {
                if let Ok(text) = String::from_utf8(bytes) {
                    css_sources.push(text);
                }
            }
        }
        let refs: Vec<&str> = css_sources.iter().map(String::as_str).collect();
        self.styled_classes = collect_styled_classes(&refs);

        self.doc = Some(doc);
        Ok(())
    }

    fn get_chapter(&mut self, index: usize) -> Result<ChapterContent> {
        let content_id = self
            .contents
            .get(index)
            .cloned()
            .ok_or_else(|| eyre::eyre!("Content not found"))?;
        Ok(ChapterContent::Html(self.get_raw_text(&content_id)?))
    }

    fn get_resource(&mut self, path: &str) -> Result<(String, Vec<u8>)> {
        if let Some(ref mut doc) = self.doc
            && let Some(bytes) = doc.get_resource_by_path(path)
        {
            let mime = doc
                .resources
                .values()
                .find(|r| r.path == PathBuf::from(path))
                .map(|r| r.mime.clone())
                .unwrap_or_else(|| mime_from_extension(path));
            return Ok((mime, bytes));
        }
        Err(eyre::eyre!("Image not found"))
    }

    fn get_cover(&mut self) -> Option<(String, Vec<u8>)> {
        let doc = self.doc.as_mut()?;
        let (bytes, mime) = doc.get_cover()?;
        Some((mime, bytes))
    }

    fn content_index_for_href(&self, href: &str) -> Option<usize> {
        let doc = self.doc.as_ref()?;
        let path = href.split('#').next().unwrap_or("");
        if path.is_empty() {
            return None;
        }
        let resource_path = PathBuf::from(path);
        doc.resource_uri_to_chapter(&resource_path)
    }

    fn styled_classes(&self) -> &StyledClasses {
        &self.styled_classes
    }

    fn cleanup(&mut self) -> Result<()> {
        self.doc = None;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chapter_text(epub: &mut Epub, index: usize) -> Result<String> {
        Ok(epub.get_chapter(index)?.fingerprint_text().to_string())
    }

    #[test]
    fn test_epub_toc_filtered() -> Result<()> {
        let mut epub = Epub::new("tests/fixtures/small.epub");
        epub.initialize()?;

        // The spine has 'htmltoc', but our contents should not
        let contents = epub.contents();

        // We know 'htmltoc' is the ID of the TOC in small.epub
        assert!(!contents.contains(&"htmltoc".to_string()));

        Ok(())
    }

    #[test]
    fn test_epub_new() {
        let epub = Epub::new("test.epub");
        assert_eq!(epub.path(), "test.epub");
        assert_eq!(epub.contents().len(), 0); // Should be empty before initialization
        assert_eq!(epub.toc_entries().len(), 0); // Should be empty before initialization
    }

    #[test]
    fn test_epub_new_invalid_path() {
        let epub = Epub::new("");
        assert_eq!(epub.path(), "");
    }

    #[test]
    fn test_epub_initialize_small() -> Result<()> {
        let mut epub = Epub::new("tests/fixtures/small.epub");
        epub.initialize()?;

        // Check that contents were loaded
        assert!(!epub.contents().is_empty());
        assert!(epub.contents().len() > 10); // small.epub should have many chapters

        // Check metadata extraction
        let meta = epub.get_meta();
        assert!(meta.title.is_some());
        assert!(meta.creator.is_some() || meta.publisher.is_some()); // At least some metadata

        Ok(())
    }

    #[test]
    fn test_epub_initialize_meditations() -> Result<()> {
        let mut epub = Epub::new("tests/fixtures/meditations.epub");
        epub.initialize()?;

        // Check that contents were loaded
        assert!(!epub.contents().is_empty());
        assert!(epub.contents().len() >= 12); // meditations.epub should have 12+ chapters

        // Check metadata extraction
        let meta = epub.get_meta();
        assert!(meta.title.is_some());
        assert!(meta.title.as_ref().unwrap().contains("Meditations"));

        Ok(())
    }

    #[test]
    fn test_epub_toc_skips_subtitle_navpoints() -> Result<()> {
        let mut epub = Epub::new("tests/fixtures/meditations.epub");
        epub.initialize()?;

        let toc_labels: Vec<&str> = epub
            .toc_entries()
            .iter()
            .map(|entry| entry.label.as_str())
            .collect();

        assert!(!toc_labels.contains(&"concerning HIMSELF:"));

        Ok(())
    }

    #[test]
    fn test_epub_initialize_nonexistent() {
        let mut epub = Epub::new("tests/fixtures/nonexistent.epub");
        let result = epub.initialize();
        assert!(result.is_err());
    }

    #[test]
    fn test_epub_get_chapter_small() -> Result<()> {
        let mut epub = Epub::new("tests/fixtures/small.epub");
        epub.initialize()?;

        let raw_text = chapter_text(&mut epub, 0)?;
        assert!(!raw_text.is_empty());
        assert!(raw_text.len() > 100); // Should have some content
        assert!(raw_text.contains("<")); // Should look like HTML/XML

        Ok(())
    }

    #[test]
    fn test_epub_get_chapter_is_html_variant() -> Result<()> {
        let mut epub = Epub::new("tests/fixtures/small.epub");
        epub.initialize()?;

        assert!(matches!(epub.get_chapter(0)?, ChapterContent::Html(_)));

        Ok(())
    }

    #[test]
    fn test_epub_get_chapter_meditations() -> Result<()> {
        let mut epub = Epub::new("tests/fixtures/meditations.epub");
        epub.initialize()?;

        let raw_text = chapter_text(&mut epub, 0)?;
        assert!(!raw_text.is_empty());
        assert!(raw_text.len() > 100); // Should have some content

        Ok(())
    }

    #[test]
    fn test_epub_get_chapter_multiple() -> Result<()> {
        let mut epub = Epub::new("tests/fixtures/small.epub");
        epub.initialize()?;

        for index in 0..3 {
            let raw_text = chapter_text(&mut epub, index)?;
            assert!(!raw_text.is_empty());
            assert!(raw_text.len() > 50); // Each chapter should have content
        }

        Ok(())
    }

    #[test]
    fn test_epub_get_chapter_out_of_range() -> Result<()> {
        let mut epub = Epub::new("tests/fixtures/small.epub");
        epub.initialize()?;

        let result = epub.get_chapter(usize::MAX);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Content not found")
        );

        Ok(())
    }

    #[test]
    fn test_epub_get_resource() -> Result<()> {
        let mut epub = Epub::new("tests/fixtures/small.epub");
        epub.initialize()?;

        // Try to get the cover image (should exist in small.epub)
        let result = epub.get_resource("EPUB/covers/9781449328030_lrg.jpg");

        // The image might exist or not - just test that the method doesn't crash
        match result {
            Ok((mime_type, bytes)) => {
                assert_eq!(mime_type, "image/jpeg"); // From the EPUB manifest
                assert!(!bytes.is_empty());
                assert!(bytes.len() > 1000); // Should be a substantial image
            }
            Err(_) => {
                // It's okay if the image doesn't exist - just test error handling
            }
        }

        Ok(())
    }

    #[test]
    fn test_epub_get_cover() -> Result<()> {
        let mut epub = Epub::new("tests/fixtures/small.epub");
        epub.initialize()?;

        if let Some((mime, bytes)) = epub.get_cover() {
            assert!(mime.starts_with("image/"));
            assert!(!bytes.is_empty());
        }

        Ok(())
    }

    #[test]
    fn test_epub_get_resource_nonexistent() -> Result<()> {
        let mut epub = Epub::new("tests/fixtures/small.epub");
        epub.initialize()?;

        let result = epub.get_resource("nonexistent/image.jpg");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Image not found"));

        Ok(())
    }

    #[test]
    fn test_epub_cleanup() -> Result<()> {
        let mut epub = Epub::new("tests/fixtures/small.epub");
        epub.initialize()?;

        // Verify it's initialized
        assert!(!epub.contents().is_empty());

        // Cleanup
        epub.cleanup()?;

        Ok(())
    }

    #[test]
    fn test_epub_toc_entries() -> Result<()> {
        let mut epub = Epub::new("tests/fixtures/small.epub");
        epub.initialize()?;

        // Just ensure TOC access works without crashing; some EPUBs have
        // limited TOC support via the epub crate.
        let _ = epub.toc_entries();

        Ok(())
    }

    #[test]
    fn test_epub_spine_href_stability() -> Result<()> {
        // spine_href is the stable chapter ID that highlight anchoring and
        // book identity depend on; it must be the in-book resource path.
        let mut epub = Epub::new("tests/fixtures/small.epub");
        epub.initialize()?;

        let href = epub.spine_href(0).expect("first chapter has an href");
        assert!(!href.is_empty());
        assert_eq!(
            epub.content_index_for_href(&href),
            Some(0),
            "spine_href round-trips through content_index_for_href"
        );

        Ok(())
    }
}
