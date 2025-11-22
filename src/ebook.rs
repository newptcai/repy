use crate::models::{BookMetadata, TextStructure, TocEntry};
use crate::parser::parse_html;
use eyre::Result;
use epub::doc::EpubDoc;
use std::collections::HashSet;

pub trait Ebook {
    fn path(&self) -> &str;
    fn contents(&self) -> &Vec<String>; // Using String for content identifiers for now
    fn toc_entries(&self) -> &Vec<TocEntry>;
    fn get_meta(&self) -> &BookMetadata;

    fn initialize(&mut self) -> Result<()>;
    fn get_raw_text(&mut self, content_id: &str) -> Result<String>;
    fn get_img_bytestr(&mut self, path: &str) -> Result<(String, Vec<u8>)>;
    fn cleanup(&mut self) -> Result<()>;

    fn get_parsed_content(&mut self, content_id: &str, text_width: usize, starting_line: usize) -> Result<TextStructure>;
    fn get_all_parsed_content(&mut self, text_width: usize) -> Result<Vec<TextStructure>>;
}

pub struct Epub {
    path: String,
    doc: Option<EpubDoc<std::io::BufReader<std::fs::File>>>,
    contents: Vec<String>,
    toc: Vec<TocEntry>,
    metadata: BookMetadata,
}

impl Epub {
    pub fn new(path: &str) -> Self {
        Self {
            path: path.to_string(),
            doc: None,
            contents: Vec::new(),
            toc: Vec::new(),
            metadata: BookMetadata::default(),
        }
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

    fn initialize(&mut self) -> Result<()> {
        let doc = EpubDoc::new(&self.path)?;
        self.contents = doc.spine.iter().map(|item| item.idref.clone()).collect();
        
        let mut toc_entries = Vec::new();
        for entry in &doc.toc {
            toc_entries.push(TocEntry {
                label: entry.label.clone(),
                content_index: 0, 
                section: None,
            });
        }
        self.toc = toc_entries;

        let mut metadata = BookMetadata::default();
        if let Some(title) = doc.mdata("title") {
            metadata.title = Some(title.value.clone());
        }
        if let Some(creator) = doc.mdata("creator") {
            metadata.creator = Some(creator.value.clone());
        }
        if let Some(description) = doc.mdata("description") {
            metadata.description = Some(description.value.clone());
        }
        if let Some(publisher) = doc.mdata("publisher") {
            metadata.publisher = Some(publisher.value.clone());
        }
        if let Some(date) = doc.mdata("date") {
            metadata.date = Some(date.value.clone());
        }
        if let Some(language) = doc.mdata("language") {
            metadata.language = Some(language.value.clone());
        }
        if let Some(format) = doc.mdata("format") {
            metadata.format = Some(format.value.clone());
        }
        if let Some(identifier) = doc.mdata("identifier") {
            metadata.identifier = Some(identifier.value.clone());
        }
        if let Some(source) = doc.mdata("source") {
            metadata.source = Some(source.value.clone());
        }
        self.metadata = metadata;
        self.doc = Some(doc);
        Ok(())
    }

    fn get_raw_text(&mut self, content_id: &str) -> Result<String> {
        if let Some(ref mut doc) = self.doc {
            if let Some(index) = self.contents.iter().position(|id| id == content_id) {
                if doc.set_current_chapter(index) {
                    if let Some((content, _)) = doc.get_current_str() {
                        return Ok(content);
                    }
                }
            }
        }
        Err(eyre::eyre!("Content not found"))
    }

    fn get_img_bytestr(&mut self, path: &str) -> Result<(String, Vec<u8>)> {
        if let Some(ref mut doc) = self.doc {
            if let Some(bytes) = doc.get_resource_by_path(path) {
                // For now, assume it's an image and use a generic MIME type
                // In a real implementation, we'd determine the MIME type from the file extension
                let mime = "image/jpeg".to_string(); // Default assumption
                return Ok((mime, bytes));
            }
        }
        Err(eyre::eyre!("Image not found"))
    }

    fn cleanup(&mut self) -> Result<()> {
        self.doc = None;
        Ok(())
    }

    fn get_parsed_content(&mut self, content_id: &str, text_width: usize, starting_line: usize) -> Result<TextStructure> {
        let raw_html = self.get_raw_text(content_id)?;

        // Collect section IDs from table of contents
        let section_ids: HashSet<String> = self.toc_entries()
            .iter()
            .filter_map(|entry| entry.section.clone())
            .collect();

        parse_html(&raw_html, Some(text_width), Some(section_ids), starting_line)
    }

    fn get_all_parsed_content(&mut self, text_width: usize) -> Result<Vec<TextStructure>> {
        let mut all_content = Vec::new();
        let mut starting_line = 0;

        // Collect all content IDs first to avoid borrowing issues
        let content_ids: Vec<String> = self.contents().clone();

        for content_id in content_ids {
            let parsed_content = self.get_parsed_content(&content_id, text_width, starting_line)?;
            starting_line += parsed_content.text_lines.len();
            all_content.push(parsed_content);
        }

        Ok(all_content)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_epub_new() {
        let epub = Epub::new("test.epub");
        assert_eq!(epub.path(), "test.epub");
    }

    #[test]
    fn test_epub_initialize() -> Result<()> {
        let mut epub = Epub::new("tests/fixtures/small.epub");
        epub.initialize()?;
        assert!(!epub.contents().is_empty());
        assert!(!epub.toc_entries().is_empty());
        assert!(epub.get_meta().title.is_some());
        Ok(())
    }

    #[test]
    fn test_get_raw_text() -> Result<()> {
        let mut epub = Epub::new("tests/fixtures/small.epub");
        epub.initialize()?;
        let content_id = epub.contents()[0].clone();
        let raw_text = epub.get_raw_text(&content_id)?;
        assert!(!raw_text.is_empty());
        Ok(())
    }

    #[test]
    fn test_get_parsed_content() -> Result<()> {
        let mut epub = Epub::new("tests/fixtures/small.epub");
        epub.initialize()?;
        let content_id = epub.contents()[0].clone();
        let parsed_content = epub.get_parsed_content(&content_id, 80, 0)?;
        assert!(!parsed_content.text_lines.is_empty());
        Ok(())
    }

    #[test]
    fn test_get_all_parsed_content() -> Result<()> {
        let mut epub = Epub::new("tests/fixtures/small.epub");
        epub.initialize()?;
        let all_content = epub.get_all_parsed_content(80)?;
        assert!(!all_content.is_empty());
        Ok(())
    }
}