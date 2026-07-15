use super::{ChapterContent, Ebook, mime_from_extension};
use crate::models::{BookMetadata, TocEntry};
use eyre::Result;
use std::io::Read;

const IMAGE_EXTENSIONS: [&str; 6] = ["jpg", "jpeg", "png", "gif", "webp", "bmp"];

/// Comic-book archive (`.cbz`): a zip of image pages. Every image entry is
/// one [`ChapterContent::ImagePage`] chapter, in natural (numeric-aware)
/// name order. Pages render through the inline-image machinery, so reading
/// one needs `inline_images: shown` and a graphics-capable terminal.
pub struct Cbz {
    path: String,
    archive: Option<zip::ZipArchive<std::fs::File>>,
    contents: Vec<String>,
    toc: Vec<TocEntry>,
    metadata: BookMetadata,
}

impl Cbz {
    pub fn new(path: &str) -> Self {
        Self {
            path: path.to_string(),
            archive: None,
            contents: Vec::new(),
            toc: Vec::new(),
            metadata: BookMetadata::default(),
        }
    }

    fn read_entry(&mut self, name: &str) -> Result<Vec<u8>> {
        let archive = self
            .archive
            .as_mut()
            .ok_or_else(|| eyre::eyre!("Book not initialized"))?;
        let mut entry = archive
            .by_name(name)
            .map_err(|_| eyre::eyre!("Image not found"))?;
        let mut bytes = Vec::with_capacity(entry.size() as usize);
        entry.read_to_end(&mut bytes)?;
        Ok(bytes)
    }

    fn is_image_page(name: &str) -> bool {
        if name.ends_with('/') {
            return false;
        }
        let path = std::path::Path::new(name);
        if path.components().any(|c| {
            let part = c.as_os_str().to_string_lossy();
            part.starts_with('.') || part == "__MACOSX"
        }) {
            return false;
        }
        path.extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase())
            .is_some_and(|ext| IMAGE_EXTENSIONS.contains(&ext.as_str()))
    }

    /// `<Tag>text</Tag>` from a ComicInfo.xml document, unescaped.
    fn comic_info_field(xml: &str, tag: &str) -> Option<String> {
        let pattern = format!(r"(?s)<{tag}(?:\s[^>]*)?>(.*?)</{tag}>");
        let re = regex::Regex::new(&pattern).ok()?;
        let text = re.captures(xml)?.get(1)?.as_str().trim();
        if text.is_empty() {
            return None;
        }
        Some(
            text.replace("&lt;", "<")
                .replace("&gt;", ">")
                .replace("&quot;", "\"")
                .replace("&apos;", "'")
                .replace("&#39;", "'")
                .replace("&amp;", "&"),
        )
    }

    fn file_stem(&self) -> String {
        std::path::Path::new(&self.path)
            .file_stem()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| self.path.clone())
    }
}

/// Compare path names naturally: digit runs compare as numbers, everything
/// else case-insensitively, so `page-2` sorts before `page-10`.
fn natural_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    let mut a_chars = a.chars().peekable();
    let mut b_chars = b.chars().peekable();
    loop {
        match (a_chars.peek().copied(), b_chars.peek().copied()) {
            (None, None) => return std::cmp::Ordering::Equal,
            (None, Some(_)) => return std::cmp::Ordering::Less,
            (Some(_), None) => return std::cmp::Ordering::Greater,
            (Some(ca), Some(cb)) => {
                if ca.is_ascii_digit() && cb.is_ascii_digit() {
                    let mut num_a = String::new();
                    while let Some(&c) = a_chars.peek().filter(|c| c.is_ascii_digit()) {
                        num_a.push(c);
                        a_chars.next();
                    }
                    let mut num_b = String::new();
                    while let Some(&c) = b_chars.peek().filter(|c| c.is_ascii_digit()) {
                        num_b.push(c);
                        b_chars.next();
                    }
                    // Compare as numbers: longer trimmed run is larger.
                    let trim_a = num_a.trim_start_matches('0');
                    let trim_b = num_b.trim_start_matches('0');
                    let ord = trim_a
                        .len()
                        .cmp(&trim_b.len())
                        .then_with(|| trim_a.cmp(trim_b))
                        .then_with(|| num_a.len().cmp(&num_b.len()));
                    if ord != std::cmp::Ordering::Equal {
                        return ord;
                    }
                } else {
                    let ord = ca
                        .to_lowercase()
                        .cmp(cb.to_lowercase())
                        .then_with(|| ca.cmp(&cb));
                    if ord != std::cmp::Ordering::Equal {
                        return ord;
                    }
                    a_chars.next();
                    b_chars.next();
                }
            }
        }
    }
}

impl Ebook for Cbz {
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
        let file = std::fs::File::open(&self.path)?;
        let mut archive = zip::ZipArchive::new(file)?;

        let mut pages: Vec<String> = archive
            .file_names()
            .filter(|name| Self::is_image_page(name))
            .map(str::to_string)
            .collect();
        pages.sort_by(|a, b| natural_cmp(a, b));
        if pages.is_empty() {
            eyre::bail!("CBZ contains no image pages: {}", self.path);
        }

        let comic_info_name = archive
            .file_names()
            .find(|name| name.eq_ignore_ascii_case("ComicInfo.xml"))
            .map(str::to_string);
        let comic_info = comic_info_name.and_then(|name| {
            let mut entry = archive.by_name(&name).ok()?;
            let mut xml = String::new();
            entry.read_to_string(&mut xml).ok()?;
            Some(xml)
        });

        let mut metadata = BookMetadata::default();
        if let Some(xml) = &comic_info {
            let title = Self::comic_info_field(xml, "Title");
            let series = Self::comic_info_field(xml, "Series");
            let number = Self::comic_info_field(xml, "Number");
            metadata.title = match (series, number, title) {
                (Some(series), Some(number), _) => Some(format!("{} #{}", series, number)),
                (Some(series), None, None) => Some(series),
                (_, _, title) => title,
            };
            metadata.creator = Self::comic_info_field(xml, "Writer");
        }
        if metadata.title.is_none() {
            metadata.title = Some(self.file_stem());
        }

        self.contents = pages;
        self.metadata = metadata;
        self.archive = Some(archive);
        Ok(())
    }

    fn get_chapter(&mut self, index: usize) -> Result<ChapterContent> {
        let name = self
            .contents
            .get(index)
            .cloned()
            .ok_or_else(|| eyre::eyre!("Content not found"))?;
        Ok(ChapterContent::ImagePage(name))
    }

    fn get_resource(&mut self, path: &str) -> Result<(String, Vec<u8>)> {
        let bytes = self.read_entry(path)?;
        Ok((mime_from_extension(path), bytes))
    }

    fn get_cover(&mut self) -> Option<(String, Vec<u8>)> {
        let first = self.contents.first().cloned()?;
        let bytes = self.read_entry(&first).ok()?;
        Some((mime_from_extension(&first), bytes))
    }

    fn cleanup(&mut self) -> Result<()> {
        self.archive = None;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    // Smallest valid 1x1 PNG (transparent pixel).
    const TINY_PNG: &[u8] = &[
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F,
        0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x00,
        0x01, 0x00, 0x00, 0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49,
        0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ];

    fn build_cbz(dir: &tempfile::TempDir, name: &str, entries: &[(&str, &[u8])]) -> String {
        let path = dir.path().join(name);
        let file = std::fs::File::create(&path).unwrap();
        let mut writer = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default();
        for (entry_name, bytes) in entries {
            writer.start_file(*entry_name, options).unwrap();
            writer.write_all(bytes).unwrap();
        }
        writer.finish().unwrap();
        path.to_string_lossy().to_string()
    }

    #[test]
    fn test_cbz_pages_natural_order() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let path = build_cbz(
            &dir,
            "comic.cbz",
            &[
                ("page-10.png", TINY_PNG),
                ("page-2.png", TINY_PNG),
                ("page-1.png", TINY_PNG),
                ("notes.txt", b"not a page"),
                ("__MACOSX/page-1.png", TINY_PNG),
                (".hidden.png", TINY_PNG),
            ],
        );
        let mut cbz = Cbz::new(&path);
        cbz.initialize()?;

        assert_eq!(
            cbz.contents(),
            &vec![
                "page-1.png".to_string(),
                "page-2.png".to_string(),
                "page-10.png".to_string(),
            ]
        );
        assert!(matches!(
            cbz.get_chapter(0)?,
            ChapterContent::ImagePage(name) if name == "page-1.png"
        ));
        Ok(())
    }

    #[test]
    fn test_cbz_metadata_from_comic_info() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let xml = r#"<?xml version="1.0"?>
<ComicInfo>
  <Series>Little Nemo</Series>
  <Number>3</Number>
  <Title>In Slumberland</Title>
  <Writer>Winsor McCay</Writer>
</ComicInfo>"#;
        let path = build_cbz(
            &dir,
            "comic.cbz",
            &[("01.png", TINY_PNG), ("ComicInfo.xml", xml.as_bytes())],
        );
        let mut cbz = Cbz::new(&path);
        cbz.initialize()?;

        assert_eq!(cbz.get_meta().title.as_deref(), Some("Little Nemo #3"));
        assert_eq!(cbz.get_meta().creator.as_deref(), Some("Winsor McCay"));
        Ok(())
    }

    #[test]
    fn test_cbz_title_falls_back_to_stem() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let path = build_cbz(&dir, "great-comic.cbz", &[("01.png", TINY_PNG)]);
        let mut cbz = Cbz::new(&path);
        cbz.initialize()?;

        assert_eq!(cbz.get_meta().title.as_deref(), Some("great-comic"));
        Ok(())
    }

    #[test]
    fn test_cbz_resource_and_cover() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let path = build_cbz(
            &dir,
            "comic.cbz",
            &[("b.png", TINY_PNG), ("a.jpg", b"\xFF\xD8\xFFjpeg")],
        );
        let mut cbz = Cbz::new(&path);
        cbz.initialize()?;

        let (mime, bytes) = cbz.get_resource("b.png")?;
        assert_eq!(mime, "image/png");
        assert_eq!(bytes, TINY_PNG);
        assert!(cbz.get_resource("missing.png").is_err());

        let (cover_mime, cover_bytes) = cbz.get_cover().expect("first page is the cover");
        assert_eq!(cover_mime, "image/jpeg");
        assert_eq!(cover_bytes, b"\xFF\xD8\xFFjpeg");
        Ok(())
    }

    #[test]
    fn test_cbz_without_images_fails() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let path = build_cbz(&dir, "empty.cbz", &[("readme.txt", b"nothing")]);
        let mut cbz = Cbz::new(&path);
        assert!(cbz.initialize().is_err());
        Ok(())
    }

    #[test]
    fn test_natural_cmp() {
        use std::cmp::Ordering;
        assert_eq!(natural_cmp("page-2", "page-10"), Ordering::Less);
        assert_eq!(natural_cmp("page-10", "page-2"), Ordering::Greater);
        assert_eq!(natural_cmp("a/02.png", "a/2.png"), Ordering::Greater);
        assert_eq!(natural_cmp("A1", "a2"), Ordering::Less);
        assert_eq!(natural_cmp("same", "same"), Ordering::Equal);
    }
}
