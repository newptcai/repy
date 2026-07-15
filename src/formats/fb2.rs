use super::{ChapterContent, Ebook, escape_html};
use crate::models::{BookMetadata, TocEntry};
use base64::Engine;
use eyre::Result;
use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};
use std::collections::HashMap;

/// FictionBook 2 (`.fb2`, `.fb2.zip`): a single XML document. Each top-level
/// `<section>` of a `<body>` becomes one chapter, converted to HTML for the
/// shared pipeline; `<binary>` elements carry base64 images served through
/// `get_resource` by their id.
pub struct Fb2 {
    path: String,
    contents: Vec<String>,
    toc: Vec<TocEntry>,
    metadata: BookMetadata,
    chapters: Vec<String>,
    binaries: HashMap<String, (String, Vec<u8>)>,
    cover_id: Option<String>,
}

impl Fb2 {
    pub fn new(path: &str) -> Self {
        Self {
            path: path.to_string(),
            contents: Vec::new(),
            toc: Vec::new(),
            metadata: BookMetadata::default(),
            chapters: Vec::new(),
            binaries: HashMap::new(),
            cover_id: None,
        }
    }

    fn file_stem(&self) -> String {
        let name = std::path::Path::new(&self.path)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| self.path.clone());
        name.trim_end_matches(".zip")
            .trim_end_matches(".fb2")
            .to_string()
    }

    /// Raw XML bytes: the file itself, or the first `.fb2` entry of a
    /// `.fb2.zip` wrapper.
    fn read_document(&self) -> Result<Vec<u8>> {
        let bytes = std::fs::read(&self.path)?;
        if !bytes.starts_with(b"PK\x03\x04") {
            return Ok(bytes);
        }
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes))?;
        let entry_name = archive
            .file_names()
            .find(|name| name.to_ascii_lowercase().ends_with(".fb2"))
            .map(str::to_string)
            .ok_or_else(|| eyre::eyre!("Zip archive contains no .fb2 document"))?;
        let mut entry = archive.by_name(&entry_name)?;
        let mut xml = Vec::with_capacity(entry.size() as usize);
        std::io::Read::read_to_end(&mut entry, &mut xml)?;
        Ok(xml)
    }
}

/// HTML tag for an FB2 body element, `None` when the element has bespoke
/// handling (image, empty-line, v) or is passed through as transparent.
fn map_tag(local: &str, section_depth: usize) -> Option<&'static str> {
    Some(match local {
        "p" => "p",
        "emphasis" => "em",
        "strong" => "strong",
        "strikethrough" => "del",
        "sub" => "sub",
        "sup" => "sup",
        "code" => "code",
        "style" => "span",
        "a" => "a",
        "cite" | "epigraph" => "blockquote",
        "poem" => "div",
        "stanza" => "p",
        "text-author" => "p",
        "subtitle" => "h3",
        "title" => {
            if section_depth <= 1 {
                "h2"
            } else {
                "h3"
            }
        }
        "table" => "table",
        "tr" => "tr",
        "th" => "th",
        "td" => "td",
        _ => return None,
    })
}

/// `l:href` / `xlink:href` / `href` attribute value.
fn href_attribute(element: &BytesStart, reader: &Reader<&[u8]>) -> Option<String> {
    element.attributes().flatten().find_map(|attr| {
        if attr.key.local_name().as_ref() != b"href" {
            return None;
        }
        attr.decoded_and_normalized_value(quick_xml::XmlVersion::Implicit1_0, reader.decoder())
            .ok()
            .map(|v| v.to_string())
    })
}

fn plain_attribute(element: &BytesStart, reader: &Reader<&[u8]>, name: &[u8]) -> Option<String> {
    element.attributes().flatten().find_map(|attr| {
        if attr.key.local_name().as_ref() != name {
            return None;
        }
        attr.decoded_and_normalized_value(quick_xml::XmlVersion::Implicit1_0, reader.decoder())
            .ok()
            .map(|v| v.to_string())
    })
}

#[derive(Default)]
struct WalkState {
    // Metadata
    element_path: Vec<String>,
    author_parts: Vec<String>,
    annotation: String,
    // Body conversion
    in_body: bool,
    body_named: bool,
    top_level_section_seen: bool,
    section_depth: usize,
    chapter_html: String,
    capturing_toc_label: bool,
    toc_label: String,
    toc_label_pending: bool,
    // Binaries
    binary_id: Option<(String, String)>,
    binary_data: String,
}

impl WalkState {
    fn path_is(&self, suffix: &[&str]) -> bool {
        self.element_path.len() >= suffix.len()
            && self.element_path[self.element_path.len() - suffix.len()..]
                .iter()
                .zip(suffix)
                .all(|(a, b)| a == b)
    }
}

impl Fb2 {
    fn parse_document(&mut self, xml: &[u8]) -> Result<()> {
        let mut reader = Reader::from_reader(xml);
        let mut state = WalkState::default();
        let mut buffer = Vec::new();

        loop {
            let event = reader
                .read_event_into(&mut buffer)
                .map_err(|e| eyre::eyre!("FB2 parse error: {}", e))?;
            match event {
                Event::Start(element) => {
                    let local = String::from_utf8_lossy(element.local_name().as_ref()).to_string();
                    self.handle_start(&local, &element, &reader, &mut state);
                    state.element_path.push(local);
                }
                Event::Empty(element) => {
                    let local = String::from_utf8_lossy(element.local_name().as_ref()).to_string();
                    self.handle_empty(&local, &element, &reader, &mut state);
                }
                Event::End(element) => {
                    let local = String::from_utf8_lossy(element.local_name().as_ref()).to_string();
                    state.element_path.pop();
                    self.handle_end(&local, &mut state);
                }
                Event::Text(text) => {
                    let decoded = reader
                        .decoder()
                        .decode(text.as_ref())
                        .unwrap_or_default()
                        .to_string();
                    let unescaped = quick_xml::escape::unescape(&decoded)
                        .map(|c| c.to_string())
                        .unwrap_or(decoded);
                    self.handle_text(&unescaped, &mut state);
                }
                Event::GeneralRef(reference) => {
                    let name = reader
                        .decoder()
                        .decode(reference.as_ref())
                        .unwrap_or_default();
                    let encoded = format!("&{};", name);
                    let decoded = quick_xml::escape::unescape(&encoded)
                        .map(|value| value.into_owned())
                        .unwrap_or(encoded);
                    self.handle_text(&decoded, &mut state);
                }
                Event::Eof => break,
                _ => {}
            }
            buffer.clear();
        }

        self.flush_chapter(&mut state);
        if self.chapters.is_empty() {
            eyre::bail!("FB2 contains no readable content: {}", self.path);
        }
        if self.metadata.title.is_none() {
            self.metadata.title = Some(self.file_stem());
        }
        if !state.annotation.trim().is_empty() {
            self.metadata.description = Some(state.annotation.trim().to_string());
        }
        self.contents = (1..=self.chapters.len())
            .map(|i| format!("section-{:04}", i))
            .collect();
        Ok(())
    }

    fn handle_start(
        &mut self,
        local: &str,
        element: &BytesStart,
        reader: &Reader<&[u8]>,
        state: &mut WalkState,
    ) {
        match local {
            "body" => {
                state.in_body = true;
                state.section_depth = 0;
                state.top_level_section_seen = false;
                self.flush_chapter(state);
                // A named body (usually "notes") gets one TOC entry for the
                // whole body; its sections don't get per-footnote entries.
                state.body_named = false;
                if let Some(name) = plain_attribute(element, reader, b"name") {
                    state.body_named = true;
                    self.toc.push(TocEntry {
                        label: name,
                        content_index: self.chapters.len(),
                        section: None,
                    });
                }
            }
            "section" if state.in_body => {
                if state.section_depth == 0 {
                    // Keep a body-level title with the first section. Later
                    // top-level sections still start new chapters.
                    if state.top_level_section_seen || !html_has_content(&state.chapter_html) {
                        self.flush_chapter(state);
                    }
                    state.top_level_section_seen = true;
                    state.toc_label_pending = !state.body_named;
                    state.toc_label.clear();
                }
                state.section_depth += 1;
                if let Some(id) = plain_attribute(element, reader, b"id") {
                    state
                        .chapter_html
                        .push_str(&format!("<div id=\"{}\">", escape_html(&id)));
                } else {
                    state.chapter_html.push_str("<div>");
                }
            }
            "binary" => {
                let id = plain_attribute(element, reader, b"id").unwrap_or_default();
                let mime = plain_attribute(element, reader, b"content-type")
                    .unwrap_or_else(|| "application/octet-stream".to_string());
                state.binary_id = Some((id, mime));
                state.binary_data.clear();
            }
            "title" if state.in_body && state.toc_label_pending => {
                state.capturing_toc_label = true;
                self.emit_start(local, element, reader, state);
            }
            _ if state.in_body => self.emit_start(local, element, reader, state),
            _ => {}
        }
    }

    fn emit_start(
        &mut self,
        local: &str,
        element: &BytesStart,
        reader: &Reader<&[u8]>,
        state: &mut WalkState,
    ) {
        let Some(tag) = map_tag(local, state.section_depth) else {
            return;
        };
        if tag == "a" {
            let href = href_attribute(element, reader).unwrap_or_default();
            state
                .chapter_html
                .push_str(&format!("<a href=\"{}\">", escape_html(&href)));
        } else if local == "text-author" {
            state.chapter_html.push_str("<p><em>");
        } else {
            state.chapter_html.push_str(&format!("<{}>", tag));
        }
    }

    fn handle_empty(
        &mut self,
        local: &str,
        element: &BytesStart,
        reader: &Reader<&[u8]>,
        state: &mut WalkState,
    ) {
        match local {
            "image" => {
                let href = href_attribute(element, reader).unwrap_or_default();
                let id = href.trim_start_matches('#').to_string();
                if state.path_is(&["coverpage"]) {
                    self.cover_id = Some(id);
                } else if state.in_body && !id.is_empty() {
                    state
                        .chapter_html
                        .push_str(&format!("<img src=\"{}\"/>", escape_html(&id)));
                }
            }
            "empty-line" if state.in_body => state.chapter_html.push_str("<br/>"),
            _ => {}
        }
    }

    fn handle_end(&mut self, local: &str, state: &mut WalkState) {
        match local {
            "body" => {
                self.flush_chapter(state);
                state.in_body = false;
            }
            "section" if state.in_body => {
                state.chapter_html.push_str("</div>");
                state.section_depth = state.section_depth.saturating_sub(1);
            }
            "binary" => {
                if let Some((id, mime)) = state.binary_id.take() {
                    let cleaned: String = state
                        .binary_data
                        .chars()
                        .filter(|c| !c.is_whitespace())
                        .collect();
                    if let Ok(bytes) =
                        base64::engine::general_purpose::STANDARD.decode(cleaned.as_bytes())
                        && !id.is_empty()
                    {
                        self.binaries.insert(id, (mime, bytes));
                    }
                }
            }
            "author" if state.path_is(&["title-info"]) => {
                if self.metadata.creator.is_none() && !state.author_parts.is_empty() {
                    self.metadata.creator = Some(state.author_parts.join(" "));
                }
                state.author_parts.clear();
            }
            "title" if state.capturing_toc_label => {
                state.capturing_toc_label = false;
                state.toc_label_pending = false;
                let label = state
                    .toc_label
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ");
                if !label.is_empty() {
                    self.toc.push(TocEntry {
                        label,
                        content_index: self.chapters.len(),
                        section: None,
                    });
                }
                self.emit_end(local, state);
            }
            _ if state.in_body => self.emit_end(local, state),
            _ => {}
        }
    }

    fn emit_end(&mut self, local: &str, state: &mut WalkState) {
        let Some(tag) = map_tag(local, state.section_depth) else {
            if local == "v" && state.in_body {
                state.chapter_html.push_str("<br/>");
            }
            return;
        };
        if local == "text-author" {
            state.chapter_html.push_str("</em></p>");
        } else {
            state.chapter_html.push_str(&format!("</{}>", tag));
        }
    }

    fn handle_text(&mut self, text: &str, state: &mut WalkState) {
        if state.binary_id.is_some() {
            state.binary_data.push_str(text);
            return;
        }
        if state.in_body {
            if state.capturing_toc_label {
                state.toc_label.push_str(text);
                state.toc_label.push(' ');
            }
            state.chapter_html.push_str(&escape_html(text));
            return;
        }

        let trimmed = text.trim();
        if trimmed.is_empty() {
            return;
        }
        if state.path_is(&["title-info", "book-title"]) {
            self.metadata.title = Some(trimmed.to_string());
        } else if state.path_is(&["title-info", "author", "first-name"])
            || state.path_is(&["title-info", "author", "middle-name"])
            || state.path_is(&["title-info", "author", "last-name"])
        {
            state.author_parts.push(trimmed.to_string());
        } else if state.path_is(&["title-info", "lang"]) {
            self.metadata.language = Some(trimmed.to_string());
        } else if state.path_is(&["title-info", "date"]) {
            self.metadata.date = Some(trimmed.to_string());
        } else if state.path_is(&["publish-info", "publisher"]) {
            self.metadata.publisher = Some(trimmed.to_string());
        } else if state.path_is(&["document-info", "id"]) {
            self.metadata.identifier = Some(trimmed.to_string());
        } else if state.element_path.iter().any(|p| p == "annotation")
            && state.element_path.iter().any(|p| p == "title-info")
        {
            state.annotation.push_str(trimmed);
            state.annotation.push(' ');
        }
    }

    /// Close out the chapter being built, skipping whitespace-only buffers.
    fn flush_chapter(&mut self, state: &mut WalkState) {
        let html = std::mem::take(&mut state.chapter_html);
        if html_has_content(&html) {
            self.chapters.push(html);
        } else if state.toc_label_pending {
            // The pending label belongs to the next chapter; keep it.
        }
    }
}

/// True when the HTML has any text beyond tags and whitespace, or an image.
fn html_has_content(html: &str) -> bool {
    if html.contains("<img ") {
        return true;
    }
    let mut in_tag = false;
    html.chars().any(|c| {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag && !c.is_whitespace() => return true,
            _ => {}
        }
        false
    })
}

impl Ebook for Fb2 {
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
        let xml = self.read_document()?;
        self.parse_document(&xml)
    }

    fn get_chapter(&mut self, index: usize) -> Result<ChapterContent> {
        let html = self
            .chapters
            .get(index)
            .cloned()
            .ok_or_else(|| eyre::eyre!("Content not found"))?;
        Ok(ChapterContent::Html(html))
    }

    fn get_resource(&mut self, path: &str) -> Result<(String, Vec<u8>)> {
        let id = path.trim_start_matches('#');
        self.binaries
            .get(id)
            .cloned()
            .ok_or_else(|| eyre::eyre!("Image not found"))
    }

    fn get_cover(&mut self) -> Option<(String, Vec<u8>)> {
        let id = self.cover_id.clone()?;
        self.binaries.get(&id).cloned()
    }

    fn cleanup(&mut self) -> Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> Result<Fb2> {
        let mut fb2 = Fb2::new("tests/fixtures/sample.fb2");
        fb2.initialize()?;
        Ok(fb2)
    }

    #[test]
    fn test_fb2_metadata() -> Result<()> {
        let fb2 = fixture()?;
        let meta = fb2.get_meta();
        assert_eq!(meta.title.as_deref(), Some("The Test Book"));
        assert_eq!(meta.creator.as_deref(), Some("Alexei Testov"));
        assert_eq!(meta.language.as_deref(), Some("en"));
        assert_eq!(meta.publisher.as_deref(), Some("Sample House"));
        Ok(())
    }

    #[test]
    fn test_fb2_chapters_and_toc() -> Result<()> {
        let fb2 = fixture()?;
        assert_eq!(fb2.contents().len(), 3, "two sections plus the notes body");
        assert_eq!(fb2.spine_href(0).as_deref(), Some("section-0001"));

        let labels: Vec<&str> = fb2
            .toc_entries()
            .iter()
            .map(|entry| entry.label.as_str())
            .collect();
        assert!(labels.contains(&"Chapter One"));
        assert!(labels.contains(&"Chapter Two"));
        Ok(())
    }

    #[test]
    fn test_fb2_html_conversion() -> Result<()> {
        let mut fb2 = fixture()?;
        let ChapterContent::Html(html) = fb2.get_chapter(0)? else {
            panic!("FB2 chapters are HTML");
        };
        assert!(html.contains("<h2>"));
        assert!(html.contains("<em>emphasised</em>"));
        assert!(html.contains("<strong>strong</strong>"));
        assert!(html.contains("<img src=\"pic1\"/>"));
        assert!(html.contains("&amp;"), "escaped ampersand missing: {html}");
        Ok(())
    }

    #[test]
    fn test_fb2_binary_resource_and_cover() -> Result<()> {
        let mut fb2 = fixture()?;
        let (mime, bytes) = fb2.get_resource("pic1")?;
        assert_eq!(mime, "image/png");
        assert!(bytes.starts_with(b"\x89PNG"));
        // hrefs may keep their leading '#'
        assert!(fb2.get_resource("#pic1").is_ok());
        assert!(fb2.get_resource("missing").is_err());

        let (cover_mime, cover_bytes) = fb2.get_cover().expect("coverpage is declared");
        assert_eq!(cover_mime, "image/png");
        assert!(!cover_bytes.is_empty());
        Ok(())
    }

    #[test]
    fn test_fb2_renders_through_pipeline() -> Result<()> {
        let mut fb2 = fixture()?;
        let structures = crate::renderer::parse_book(&mut fb2, 60, None, None)?;
        assert_eq!(structures.len(), 3);
        let text: String = structures
            .iter()
            .flat_map(|s| s.text_lines.iter())
            .cloned()
            .collect::<Vec<_>>()
            .join("\n");
        assert!(text.contains("Chapter One"));
        assert!(text.contains("emphasised"));
        assert!(text.contains("A verse line"));
        Ok(())
    }

    #[test]
    fn test_fb2_windows_1251_encoding() -> Result<()> {
        // Build a windows-1251 document in memory: "Тест" in 1251 bytes.
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("ru.fb2");
        let mut doc: Vec<u8> = Vec::new();
        doc.extend_from_slice(b"<?xml version=\"1.0\" encoding=\"windows-1251\"?>\n");
        doc.extend_from_slice(b"<FictionBook><description><title-info><book-title>");
        doc.extend_from_slice(&[0xD2, 0xE5, 0xF1, 0xF2]); // "\u{422}\u{435}\u{441}\u{442}"
        doc.extend_from_slice(b"</book-title></title-info></description>");
        doc.extend_from_slice(b"<body><section><p>");
        doc.extend_from_slice(&[0xD2, 0xE5, 0xF1, 0xF2]);
        doc.extend_from_slice(b"</p></section></body></FictionBook>");
        std::fs::write(&path, doc)?;

        let mut fb2 = Fb2::new(&path.to_string_lossy());
        fb2.initialize()?;
        assert_eq!(fb2.get_meta().title.as_deref(), Some("Тест"));
        let ChapterContent::Html(html) = fb2.get_chapter(0)? else {
            panic!("HTML expected");
        };
        assert!(html.contains("Тест"));
        Ok(())
    }

    #[test]
    fn test_fb2_zip_wrapper() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let inner = std::fs::read("tests/fixtures/sample.fb2")?;
        let path = dir.path().join("sample.fb2.zip");
        let file = std::fs::File::create(&path)?;
        let mut writer = zip::ZipWriter::new(file);
        writer.start_file("sample.fb2", zip::write::SimpleFileOptions::default())?;
        std::io::Write::write_all(&mut writer, &inner)?;
        writer.finish()?;

        let mut fb2 = Fb2::new(&path.to_string_lossy());
        fb2.initialize()?;
        assert_eq!(fb2.get_meta().title.as_deref(), Some("The Test Book"));
        Ok(())
    }

    #[test]
    fn test_fb2_invalid_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("broken.fb2");
        std::fs::write(&path, "not xml at all").unwrap();
        let mut fb2 = Fb2::new(&path.to_string_lossy());
        assert!(fb2.initialize().is_err());
    }
}
