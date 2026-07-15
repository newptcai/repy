use super::{ChapterContent, Ebook};
use crate::models::{BookMetadata, TocEntry};
use eyre::Result;
use mobi::headers::ExthRecord;
use regex::Regex;
use std::sync::LazyLock;

/// Legacy MOBI6/PalmDOC backend. The `mobi` crate also opens some AZW/AZW3
/// containers, but KF8-only markup is intentionally best-effort.
pub struct MobiBook {
    path: String,
    contents: Vec<String>,
    toc: Vec<TocEntry>,
    metadata: BookMetadata,
    html: String,
    images: Vec<(String, Vec<u8>)>,
    cover_index: Option<usize>,
}

impl MobiBook {
    pub fn new(path: &str) -> Self {
        Self {
            path: path.to_string(),
            contents: Vec::new(),
            toc: Vec::new(),
            metadata: BookMetadata::default(),
            html: String::new(),
            images: Vec::new(),
            cover_index: None,
        }
    }
}

static IMG_TAG: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?is)<img\b([^>]*)>"#).expect("valid image regex"));
static RECINDEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)\brecindex\s*=\s*[\"']?([0-9]+)[\"']?"#).expect("valid recindex regex")
});

/// MOBI6 images are referenced as one-based `recindex` attributes. Convert
/// them to ordinary src attributes consumed by the shared image pipeline.
fn normalize_image_references(html: &str) -> String {
    IMG_TAG
        .replace_all(html, |caps: &regex::Captures| {
            let attrs = caps.get(1).map_or("", |m| m.as_str());
            let Some(index) = RECINDEX
                .captures(attrs)
                .and_then(|c| c.get(1))
                .map(|m| m.as_str())
            else {
                return caps[0].to_string();
            };
            let cleaned = RECINDEX.replace(attrs, "");
            format!(r#"<img{} src="mobi-image-{}">"#, cleaned, index)
        })
        .into_owned()
}

fn image_mime(bytes: &[u8]) -> String {
    match image::guess_format(bytes) {
        Ok(image::ImageFormat::Png) => "image/png",
        Ok(image::ImageFormat::Jpeg) => "image/jpeg",
        Ok(image::ImageFormat::Gif) => "image/gif",
        Ok(image::ImageFormat::WebP) => "image/webp",
        Ok(image::ImageFormat::Bmp) => "image/bmp",
        Ok(image::ImageFormat::Tiff) => "image/tiff",
        _ => "application/octet-stream",
    }
    .to_string()
}

fn exth_u32(book: &mobi::Mobi, record: ExthRecord) -> Option<u32> {
    let bytes = book.metadata.exth.get_record(record)?.first()?;
    let array: [u8; 4] = bytes.get(..4)?.try_into().ok()?;
    Some(u32::from_be_bytes(array))
}

impl Ebook for MobiBook {
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
        let book = mobi::Mobi::from_path(&self.path)
            .map_err(|e| eyre::eyre!("Unable to read MOBI file {}: {}", self.path, e))?;
        let title = book.title();
        self.metadata.title = Some(if title.trim().is_empty() {
            std::path::Path::new(&self.path)
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| self.path.clone())
        } else {
            title
        });
        self.metadata.creator = book.author();
        self.metadata.publisher = book.publisher();
        self.metadata.description = book.description();
        self.metadata.identifier = book.isbn();
        self.metadata.date = book.publish_date();

        self.html = normalize_image_references(&book.content_as_string_lossy());
        if self.html.trim().is_empty() {
            eyre::bail!("MOBI contains no readable content: {}", self.path);
        }
        self.images = book
            .image_records()
            .into_iter()
            .map(|record| (image_mime(record.content), record.content.to_vec()))
            .collect();
        self.cover_index = exth_u32(&book, ExthRecord::CoverOffset).map(|n| n as usize);
        self.contents = vec!["mobi-content".to_string()];
        self.toc = vec![TocEntry {
            label: self.metadata.title.clone().unwrap_or_default(),
            content_index: 0,
            section: None,
        }];
        Ok(())
    }

    fn get_chapter(&mut self, index: usize) -> Result<ChapterContent> {
        if index != 0 {
            eyre::bail!("Content not found");
        }
        Ok(ChapterContent::Html(self.html.clone()))
    }

    fn get_resource(&mut self, path: &str) -> Result<(String, Vec<u8>)> {
        let index = path
            .trim_start_matches("mobi-image-")
            .parse::<usize>()
            .ok()
            .and_then(|n| n.checked_sub(1))
            .ok_or_else(|| eyre::eyre!("Invalid MOBI image reference: {}", path))?;
        self.images
            .get(index)
            .cloned()
            .ok_or_else(|| eyre::eyre!("Image not found"))
    }

    fn get_cover(&mut self) -> Option<(String, Vec<u8>)> {
        self.images.get(self.cover_index?).cloned()
    }

    fn cleanup(&mut self) -> Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_mobi_recindex_images() {
        let html = r#"<p>Before<img align="middle" recindex="00002">after</p>"#;
        let normalized = normalize_image_references(html);
        assert!(normalized.contains(r#"src="mobi-image-00002""#));
        assert!(!normalized.contains("recindex"));
        assert!(normalized.contains(r#"align="middle""#));
    }

    #[test]
    fn test_preserve_normal_img_tags() {
        let html = r#"<img src="cover.jpg">"#;
        assert_eq!(normalize_image_references(html), html);
    }

    #[test]
    fn test_invalid_mobi_reports_context() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("broken.mobi");
        std::fs::write(&path, b"not a mobi").unwrap();
        let error = super::super::open(&path.to_string_lossy()).err().unwrap();
        assert!(error.to_string().contains("Unable to read MOBI file"));
    }
}
