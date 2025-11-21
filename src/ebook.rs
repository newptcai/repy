use crate::models::{BookMetadata, TocEntry, TextStructure};
use eyre::Result;
use epub::doc::EpubDoc;
use std::collections::HashMap;

pub trait Ebook {
    fn path(&self) -> &str;
    fn contents(&self) -> &Vec<String>; // Using String for content identifiers for now
    fn toc_entries(&self) -> &Vec<TocEntry>;
    fn get_meta(&self) -> &BookMetadata;

    fn initialize(&mut self) -> Result<()>;
    fn get_raw_text(&mut self, content_id: &str) -> Result<String>;
    fn get_img_bytestr(&mut self, path: &str) -> Result<(String, Vec<u8>)>;
    fn cleanup(&mut self) -> Result<()>;
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
        let mut doc = EpubDoc::new(&self.path)?;
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
                    if let Some((content, _)) = doc.get_current_str().ok() {
                        return Ok(content);
                    }
                }
            }
        }
        Err(eyre::eyre!("Content not found"))
    }

    fn get_img_bytestr(&mut self, path: &str) -> Result<(String, Vec<u8>)> {
        if let Some(ref mut doc) = self.doc {
            if let Ok((bytes, mime)) = doc.get_resource_by_path(path) {
                return Ok((mime, bytes));
            }
        }
        Err(eyre::eyre!("Image not found"))
    }

    fn cleanup(&mut self) -> Result<()> {
        self.doc = None;
        Ok(())
    }
}