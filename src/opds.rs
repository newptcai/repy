use eyre::{Result, eyre};
use std::io::Write;
use std::path::Path;

pub const EPUB_MIME: &str = "application/epub+zip";

/// OPDS acquisition link relation prefixes
const ACQ_PREFIX: &str = "http://opds-spec.org/acquisition";
const ACQ_OPEN: &str = "http://opds-spec.org/acquisition/open-access";
const ACQ_SAMPLE: &str = "http://opds-spec.org/acquisition/sample";

/// Navigation-type link relations
const REL_SUBSECTION: &str = "subsection";
const REL_START: &str = "start";
const REL_NEXT: &str = "next";
const REL_PREV: &str = "previous";
const REL_SEARCH: &str = "search";

#[derive(Debug, Clone, PartialEq)]
pub struct OpdsLink {
    pub href: String,
    pub rel: String,
    pub type_: String,
    pub title: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct OpdsEntry {
    pub id: String,
    pub title: String,
    pub summary: Option<String>,
    pub authors: Vec<String>,
    pub links: Vec<OpdsLink>,
    pub updated: String,
    pub language: Option<String>,
    pub publisher: Option<String>,
    pub issued: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct OpdsFeed {
    pub title: String,
    pub id: String,
    pub links: Vec<OpdsLink>,
    pub entries: Vec<OpdsEntry>,
}

impl OpdsFeed {
    pub fn next_link(&self) -> Option<&OpdsLink> {
        self.links.iter().find(|l| l.rel == REL_NEXT)
    }

    pub fn prev_link(&self) -> Option<&OpdsLink> {
        self.links.iter().find(|l| l.rel == REL_PREV)
    }

    pub fn search_link(&self) -> Option<&OpdsLink> {
        self.links.iter().find(|l| l.rel == REL_SEARCH)
    }
}

/// Per-catalog authentication credentials.
#[derive(Debug, Clone, PartialEq)]
pub struct OpdsAuth {
    pub username: String,
    pub password: String,
}

/// Returns true if this link relation is a navigation entry (leads to another feed).
pub fn is_nav_rel(rel: &str) -> bool {
    rel == REL_SUBSECTION
        || rel == REL_START
        || rel.starts_with("http://opds-spec.org/sort/")
        || rel == "http://opds-spec.org/featured"
        || rel == "http://opds-spec.org/recommended"
        || rel == "related"
        || rel == "collection"
}

/// Returns true if this link relation is an acquisition link.
pub fn is_acq_rel(rel: &str) -> bool {
    rel == ACQ_PREFIX || rel == ACQ_OPEN || rel == ACQ_SAMPLE || rel.starts_with(ACQ_PREFIX)
}

/// Find the best EPUB acquisition link for an entry (prefers open-access, then generic).
pub fn find_epub_link(entry: &OpdsEntry) -> Option<&OpdsLink> {
    // Prefer open-access
    if let Some(l) = entry
        .links
        .iter()
        .find(|l| l.rel == ACQ_OPEN && l.type_ == EPUB_MIME)
    {
        return Some(l);
    }
    // Then generic acquisition
    entry
        .links
        .iter()
        .find(|l| is_acq_rel(&l.rel) && l.type_ == EPUB_MIME)
}

/// Find the navigation link in an entry (to browse into a sub-catalog).
pub fn find_nav_link(entry: &OpdsEntry) -> Option<&OpdsLink> {
    entry.links.iter().find(|l| {
        is_nav_rel(&l.rel)
            && (l.type_.contains("application/atom+xml")
                || l.type_.is_empty()
                || l.type_ == "application/atom+xml;profile=opds-catalog")
    })
}

/// Resolve a potentially relative URL against a base URL.
pub fn resolve_url(base: &str, href: &str) -> String {
    if href.starts_with("http://") || href.starts_with("https://") {
        return href.to_string();
    }
    if href.starts_with('/') {
        // Absolute path — attach to base origin
        if let Some(origin_end) = base
            .find("://")
            .and_then(|i| base[i + 3..].find('/').map(|j| i + 3 + j))
        {
            return format!("{}{}", &base[..origin_end], href);
        }
        return href.to_string();
    }
    // Relative path — resolve against base directory
    let base_dir = base.rfind('/').map(|i| &base[..i]).unwrap_or(base);
    format!("{}/{}", base_dir, href)
}

/// Build a search URL by substituting {searchTerms} in an OpenSearch template.
pub fn build_search_url(template: &str, query: &str) -> String {
    let encoded: String = query
        .chars()
        .flat_map(|c| {
            if c.is_alphanumeric() || "-_.~".contains(c) {
                vec![c]
            } else if c == ' ' {
                vec!['+']
            } else {
                // percent-encode
                let s = format!("%{:02X}", c as u32);
                s.chars().collect::<Vec<_>>()
            }
        })
        .collect();
    template.replace("{searchTerms}", &encoded)
}

/// Parse an Atom/OPDS feed XML string into an OpdsFeed.
pub fn parse_feed(xml: &str) -> Result<OpdsFeed> {
    let doc = roxmltree::Document::parse(xml)
        .map_err(|e| eyre!("XML parse error: {e}"))?;

    let root = doc.root_element();

    // The root should be atom:feed or atom:entry
    let title = find_text_child(&root, "title").unwrap_or_default();
    let id = find_text_child(&root, "id").unwrap_or_default();
    let links = parse_links(&root);
    let entries = root
        .children()
        .filter(|n| n.is_element() && n.tag_name().name() == "entry")
        .map(parse_entry)
        .collect();

    Ok(OpdsFeed {
        title,
        id,
        links,
        entries,
    })
}

fn find_text_child(node: &roxmltree::Node, local_name: &str) -> Option<String> {
    node.children()
        .find(|n| n.is_element() && n.tag_name().name() == local_name)
        .and_then(|n| n.text().map(str::trim).filter(|s| !s.is_empty()).map(String::from))
}

fn parse_links(node: &roxmltree::Node) -> Vec<OpdsLink> {
    node.children()
        .filter(|n| n.is_element() && n.tag_name().name() == "link")
        .map(|n| OpdsLink {
            href: n.attribute("href").unwrap_or("").to_string(),
            rel: n.attribute("rel").unwrap_or("").to_string(),
            type_: n.attribute("type").unwrap_or("").to_string(),
            title: n.attribute("title").map(String::from),
        })
        .collect()
}

fn parse_entry(node: roxmltree::Node) -> OpdsEntry {
    let id = find_text_child(&node, "id").unwrap_or_default();
    let title = find_text_child(&node, "title").unwrap_or_else(|| "(no title)".to_string());
    let updated = find_text_child(&node, "updated").unwrap_or_default();

    // Summary: prefer <summary>, fall back to <content>
    let summary = find_text_child(&node, "summary")
        .or_else(|| find_text_child(&node, "content"));

    // Authors
    let authors: Vec<String> = node
        .children()
        .filter(|n| n.is_element() && n.tag_name().name() == "author")
        .filter_map(|n| find_text_child(&n, "name"))
        .collect();

    // Dublin Core metadata
    let language = node
        .children()
        .find(|n| n.is_element() && n.tag_name().name() == "language")
        .and_then(|n| n.text().map(str::trim).filter(|s| !s.is_empty()).map(String::from));
    let publisher = node
        .children()
        .find(|n| n.is_element() && n.tag_name().name() == "publisher")
        .and_then(|n| n.text().map(str::trim).filter(|s| !s.is_empty()).map(String::from));
    let issued = node
        .children()
        .find(|n| n.is_element() && n.tag_name().name() == "issued")
        .and_then(|n| n.text().map(str::trim).filter(|s| !s.is_empty()).map(String::from));

    let links = parse_links(&node);

    OpdsEntry {
        id,
        title,
        summary,
        authors,
        links,
        updated,
        language,
        publisher,
        issued,
    }
}

/// Fetch and parse an OPDS feed from a URL.
pub fn fetch_feed(url: &str, auth: Option<&OpdsAuth>) -> Result<OpdsFeed> {
    let client = reqwest::blocking::Client::builder()
        .user_agent("repy/1.0 (OPDS client)")
        .build()?;

    let mut req = client.get(url);
    if let Some(a) = auth {
        req = req.basic_auth(&a.username, Some(&a.password));
    }

    let resp = req.send()?;
    if !resp.status().is_success() {
        return Err(eyre!("HTTP {} for {url}", resp.status()));
    }

    let text = resp.text()?;
    parse_feed(&text)
}

/// Fetch an OpenSearch description and extract the URL template for OPDS results.
pub fn fetch_search_template(
    search_link: &OpdsLink,
    base_url: &str,
    auth: Option<&OpdsAuth>,
) -> Result<String> {
    let url = resolve_url(base_url, &search_link.href);

    let client = reqwest::blocking::Client::builder()
        .user_agent("repy/1.0 (OPDS client)")
        .build()?;
    let mut req = client.get(&url);
    if let Some(a) = auth {
        req = req.basic_auth(&a.username, Some(&a.password));
    }
    let resp = req.send()?;
    let text = resp.text()?;

    // Parse OpenSearch description document — look for <Url template="...">
    // that targets an OPDS acquisition feed.
    let doc = roxmltree::Document::parse(&text)
        .map_err(|e| eyre!("OpenSearch XML parse error: {e}"))?;

    for node in doc.descendants() {
        if node.is_element() && node.tag_name().name() == "Url" {
            let type_ = node.attribute("type").unwrap_or("");
            let template = node.attribute("template").unwrap_or("");
            if type_.contains("application/atom+xml") && template.contains("{searchTerms}") {
                return Ok(template.to_string());
            }
        }
    }

    // Fallback: if we couldn't find an OpenSearch doc, treat the href itself as a template
    // (some servers put the template directly in the link href)
    if search_link.href.contains("{searchTerms}") {
        return Ok(resolve_url(base_url, &search_link.href));
    }

    Err(eyre!("No OpenSearch template found at {url}"))
}

/// Download a book from a URL to a destination file.
pub fn download_book(url: &str, auth: Option<&OpdsAuth>, dest: &Path) -> Result<()> {
    let client = reqwest::blocking::Client::builder()
        .user_agent("repy/1.0 (OPDS client)")
        .build()?;

    let mut req = client.get(url);
    if let Some(a) = auth {
        req = req.basic_auth(&a.username, Some(&a.password));
    }

    let resp = req.send()?;
    if !resp.status().is_success() {
        return Err(eyre!("HTTP {} downloading {url}", resp.status()));
    }

    let bytes = resp.bytes()?;
    let mut file = std::fs::File::create(dest)?;
    file.write_all(&bytes)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const NAV_FEED_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<feed xmlns="http://www.w3.org/2005/Atom"
      xmlns:opds="http://opds-spec.org/2010/catalog">
  <id>urn:test:nav</id>
  <title>Test Catalog</title>
  <updated>2024-01-01T00:00:00Z</updated>
  <link rel="self" href="/opds" type="application/atom+xml;profile=opds-catalog;kind=navigation"/>
  <link rel="search" href="/opds/search" type="application/opensearchdescription+xml"/>
  <entry>
    <id>urn:test:entry:1</id>
    <title>Science Fiction</title>
    <updated>2024-01-01T00:00:00Z</updated>
    <link rel="subsection" href="/opds/sci-fi"
          type="application/atom+xml;profile=opds-catalog;kind=acquisition"/>
  </entry>
  <entry>
    <id>urn:test:entry:2</id>
    <title>Mystery</title>
    <updated>2024-01-01T00:00:00Z</updated>
    <link rel="subsection" href="/opds/mystery"
          type="application/atom+xml;profile=opds-catalog;kind=acquisition"/>
  </entry>
</feed>"#;

    const ACQ_FEED_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<feed xmlns="http://www.w3.org/2005/Atom"
      xmlns:dc="http://purl.org/dc/terms/"
      xmlns:opds="http://opds-spec.org/2010/catalog">
  <id>urn:test:acq</id>
  <title>Science Fiction Books</title>
  <updated>2024-01-01T00:00:00Z</updated>
  <link rel="next" href="/opds/sci-fi?page=2"
        type="application/atom+xml;profile=opds-catalog;kind=acquisition"/>
  <entry>
    <id>urn:test:book:1</id>
    <title>The War of the Worlds</title>
    <updated>2024-01-01T00:00:00Z</updated>
    <author><name>H.G. Wells</name></author>
    <dc:language>en</dc:language>
    <dc:issued>1898</dc:issued>
    <summary>A Martian invasion of Earth.</summary>
    <link rel="http://opds-spec.org/acquisition/open-access"
          href="/books/war-of-worlds.epub"
          type="application/epub+zip"/>
    <link rel="http://opds-spec.org/acquisition"
          href="/books/war-of-worlds.pdf"
          type="application/pdf"/>
  </entry>
  <entry>
    <id>urn:test:book:2</id>
    <title>Twenty Thousand Leagues</title>
    <updated>2024-01-01T00:00:00Z</updated>
    <author><name>Jules Verne</name></author>
    <link rel="http://opds-spec.org/acquisition"
          href="/books/twenty-thousand-leagues.epub"
          type="application/epub+zip"/>
  </entry>
</feed>"#;

    #[test]
    fn test_parse_navigation_feed() {
        let feed = parse_feed(NAV_FEED_XML).unwrap();
        assert_eq!(feed.title, "Test Catalog");
        assert_eq!(feed.id, "urn:test:nav");
        assert_eq!(feed.entries.len(), 2);
        assert_eq!(feed.entries[0].title, "Science Fiction");
        assert_eq!(feed.entries[1].title, "Mystery");
    }

    #[test]
    fn test_parse_acquisition_feed() {
        let feed = parse_feed(ACQ_FEED_XML).unwrap();
        assert_eq!(feed.title, "Science Fiction Books");
        assert_eq!(feed.entries.len(), 2);

        let entry = &feed.entries[0];
        assert_eq!(entry.title, "The War of the Worlds");
        assert_eq!(entry.authors, vec!["H.G. Wells"]);
        assert_eq!(entry.summary.as_deref(), Some("A Martian invasion of Earth."));
        assert_eq!(entry.language.as_deref(), Some("en"));
        assert_eq!(entry.issued.as_deref(), Some("1898"));
    }

    #[test]
    fn test_parse_pagination_links() {
        let feed = parse_feed(ACQ_FEED_XML).unwrap();
        let next = feed.next_link().expect("should have next link");
        assert_eq!(next.href, "/opds/sci-fi?page=2");
        assert!(feed.prev_link().is_none());
    }

    #[test]
    fn test_parse_search_link() {
        let feed = parse_feed(NAV_FEED_XML).unwrap();
        let search = feed.search_link().expect("should have search link");
        assert_eq!(search.href, "/opds/search");
    }

    #[test]
    fn test_find_epub_link_prefers_open_access() {
        let feed = parse_feed(ACQ_FEED_XML).unwrap();
        let entry = &feed.entries[0];
        let link = find_epub_link(entry).expect("should find epub link");
        assert_eq!(link.rel, "http://opds-spec.org/acquisition/open-access");
        assert_eq!(link.type_, EPUB_MIME);
    }

    #[test]
    fn test_find_epub_link_generic_acq() {
        let feed = parse_feed(ACQ_FEED_XML).unwrap();
        let entry = &feed.entries[1]; // only generic acquisition
        let link = find_epub_link(entry).expect("should find epub link");
        assert_eq!(link.type_, EPUB_MIME);
    }

    #[test]
    fn test_find_nav_link() {
        let feed = parse_feed(NAV_FEED_XML).unwrap();
        let entry = &feed.entries[0];
        let link = find_nav_link(entry).expect("should find nav link");
        assert_eq!(link.rel, "subsection");
        assert_eq!(link.href, "/opds/sci-fi");
    }

    #[test]
    fn test_build_search_url() {
        let url = build_search_url("https://example.com/search?q={searchTerms}", "H.G. Wells");
        assert_eq!(url, "https://example.com/search?q=H.G.+Wells");
    }

    #[test]
    fn test_build_search_url_special_chars() {
        let url = build_search_url("https://example.com/search?q={searchTerms}", "war & peace");
        assert!(url.contains("war"));
        assert!(url.contains("peace"));
    }

    #[test]
    fn test_resolve_url_absolute() {
        assert_eq!(
            resolve_url("https://example.com/opds", "https://other.com/feed"),
            "https://other.com/feed"
        );
    }

    #[test]
    fn test_resolve_url_absolute_path() {
        assert_eq!(
            resolve_url("https://example.com/opds/root", "/opds/sci-fi"),
            "https://example.com/opds/sci-fi"
        );
    }

    #[test]
    fn test_resolve_url_relative() {
        assert_eq!(
            resolve_url("https://example.com/opds/root", "sci-fi"),
            "https://example.com/opds/sci-fi"
        );
    }

    #[test]
    fn test_is_nav_rel() {
        assert!(is_nav_rel("subsection"));
        assert!(is_nav_rel("http://opds-spec.org/sort/new"));
        assert!(is_nav_rel("http://opds-spec.org/featured"));
        assert!(!is_nav_rel("http://opds-spec.org/acquisition"));
        assert!(!is_nav_rel("http://opds-spec.org/acquisition/open-access"));
    }

    #[test]
    fn test_is_acq_rel() {
        assert!(is_acq_rel("http://opds-spec.org/acquisition"));
        assert!(is_acq_rel("http://opds-spec.org/acquisition/open-access"));
        assert!(is_acq_rel("http://opds-spec.org/acquisition/buy"));
        assert!(!is_acq_rel("subsection"));
    }
}
