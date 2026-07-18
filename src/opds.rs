//! OPDS transport-independent catalog model and OPDS 1.2 Atom implementation.
//! A future OPDS 2.0 parser should populate the same [`Feed`] model.

use eyre::{Context, Result, bail};
use quick_xml::{Reader, events::Event};
use reqwest::blocking::{Client, Response};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    time::Duration,
};
use url::Url;

pub const OPDS_ACCEPT: &str =
    "application/atom+xml;profile=opds-catalog, application/atom+xml;q=0.9";
const ACQUISITION_PREFIX: &str = "http://opds-spec.org/acquisition";

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Feed {
    pub title: String,
    pub navigation: Vec<NavigationEntry>,
    pub publications: Vec<Publication>,
    pub pagination: Pagination,
    pub search: Option<SearchDescription>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NavigationEntry {
    pub title: String,
    pub href: String,
    pub summary: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Publication {
    pub title: String,
    pub authors: Vec<String>,
    pub summary: Option<String>,
    pub cover: Option<String>,
    pub acquisitions: Vec<AcquisitionLink>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcquisitionLink {
    pub href: String,
    pub media_type: Option<String>,
    pub relation: String,
    pub availability: Availability,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Availability {
    Readable,
    Unsupported,
    Restricted,
}
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Pagination {
    pub next: Option<String>,
    pub previous: Option<String>,
    /// OpenSearch `totalResults`: entries across all pages, when advertised.
    pub total_results: Option<u64>,
    /// OpenSearch `startIndex`: 1-based index of this page's first entry.
    pub start_index: Option<u64>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchDescription {
    pub template: String,
}

impl Publication {
    pub fn readable_acquisitions(&self) -> Vec<&AcquisitionLink> {
        let mut links: Vec<_> = self
            .acquisitions
            .iter()
            .filter(|a| a.availability == Availability::Readable)
            .collect();
        links.sort_by_key(|a| if a.extension() == Some("epub") { 0 } else { 1 });
        links
    }
}

impl AcquisitionLink {
    pub fn extension(&self) -> Option<&'static str> {
        supported_extension(self.media_type.as_deref(), &self.href)
    }
}

fn local_name(name: &[u8]) -> &[u8] {
    name.rsplit(|b| *b == b':').next().unwrap_or(name)
}
fn text_to_plain(s: &str) -> String {
    let mut out = String::new();
    let mut tag = false;
    for c in s.chars() {
        match c {
            '<' => {
                if out.chars().last().is_some_and(|c| !c.is_whitespace()) {
                    out.push(' ');
                }
                tag = true;
            }
            '>' => tag = false,
            _ if !tag => out.push(c),
            _ => {}
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Parse an OPDS 1.2 Atom document, resolving all usable links against `base`.
pub fn parse_opds1(xml: &str, base: &Url) -> Result<Feed> {
    let mut r = Reader::from_str(xml);
    r.config_mut().trim_text(true);
    let mut feed = Feed::default();
    let mut entry: Option<EntryBuilder> = None;
    let mut stack: Vec<Vec<u8>> = Vec::new();
    let mut content = String::new();
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) => {
                let name = local_name(e.name().as_ref()).to_vec();
                stack.push(name.clone());
                content.clear();
                if name == b"entry" {
                    entry = Some(EntryBuilder::default());
                }
                if name == b"link" {
                    parse_link(&e, base, entry.as_mut(), &mut feed)?;
                }
            }
            Ok(Event::Empty(e)) => {
                if local_name(e.name().as_ref()) == b"link" {
                    parse_link(&e, base, entry.as_mut(), &mut feed)?;
                }
            }
            Ok(Event::Text(e)) => content.push_str(&e.decode()?.into_owned()),
            Ok(Event::CData(e)) => content.push_str(&e.decode()?.into_owned()),
            Ok(Event::GeneralRef(e)) => {
                let entity: &[u8] = e.as_ref();
                match entity {
                    b"lt" => content.push('<'),
                    b"gt" => content.push('>'),
                    b"amp" => content.push('&'),
                    b"quot" => content.push('"'),
                    b"apos" => content.push('\''),
                    _ => {}
                }
            }
            Ok(Event::End(e)) => {
                let qualified_name = e.name();
                let name = local_name(qualified_name.as_ref());
                let value = text_to_plain(&content);
                if name == b"entry" {
                    if let Some(b) = entry.take() {
                        b.finish(&mut feed);
                    }
                } else if let Some(b) = entry.as_mut() {
                    match name {
                        b"title" => b.title = value,
                        b"name" if stack.iter().any(|x| x == b"author") => {
                            if !value.is_empty() {
                                b.authors.push(value)
                            }
                        }
                        b"summary" | b"content" => {
                            if !value.is_empty() {
                                b.summary = Some(value)
                            }
                        }
                        _ => {}
                    }
                } else if name == b"title" && feed.title.is_empty() {
                    feed.title = value;
                } else if name == b"totalResults" {
                    feed.pagination.total_results = value.parse().ok();
                } else if name == b"startIndex" {
                    feed.pagination.start_index = value.parse().ok();
                }
                stack.pop();
                content.clear();
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(e).wrap_err("invalid OPDS XML"),
            _ => {}
        }
    }
    Ok(feed)
}

#[derive(Default)]
struct EntryBuilder {
    title: String,
    authors: Vec<String>,
    summary: Option<String>,
    navigation: Option<String>,
    cover: Option<String>,
    acquisitions: Vec<AcquisitionLink>,
}
impl EntryBuilder {
    fn finish(self, feed: &mut Feed) {
        if !self.acquisitions.is_empty() {
            feed.publications.push(Publication {
                title: self.title,
                authors: self.authors,
                summary: self.summary,
                cover: self.cover,
                acquisitions: self.acquisitions,
            });
        } else if let Some(href) = self.navigation {
            feed.navigation.push(NavigationEntry {
                title: self.title,
                href,
                summary: self.summary,
            });
        }
    }
}

fn parse_link(
    e: &quick_xml::events::BytesStart<'_>,
    base: &Url,
    entry: Option<&mut EntryBuilder>,
    feed: &mut Feed,
) -> Result<()> {
    let mut rel = String::new();
    let mut href = None;
    let mut typ = None;
    for a in e.attributes().with_checks(false) {
        let a = a?;
        let v = a
            .decoded_and_normalized_value(quick_xml::XmlVersion::Implicit1_0, e.decoder())?
            .into_owned();
        match local_name(a.key.as_ref()) {
            b"rel" => rel = v,
            b"href" => href = Some(v),
            b"type" => typ = Some(v),
            _ => {}
        }
    }
    let Some(raw) = href else { return Ok(()) };
    let Ok(url) = resolve_http(base, &raw) else {
        return Ok(());
    };
    let href = url.to_string();
    if let Some(b) = entry {
        if rel.starts_with(ACQUISITION_PREFIX) {
            let restricted =
                rel != ACQUISITION_PREFIX && rel != format!("{ACQUISITION_PREFIX}/open-access");
            let supported = supported_extension(typ.as_deref(), &href).is_some();
            b.acquisitions.push(AcquisitionLink {
                href,
                media_type: typ,
                relation: rel,
                availability: if restricted {
                    Availability::Restricted
                } else if supported {
                    Availability::Readable
                } else {
                    Availability::Unsupported
                },
            });
        } else if rel == "http://opds-spec.org/image"
            || rel == "http://opds-spec.org/image/thumbnail"
        {
            if b.cover.is_none() {
                b.cover = Some(href);
            }
        } else if rel == "subsection" || rel == "alternate" || rel.is_empty() {
            b.navigation = Some(href);
        }
    } else {
        match rel.as_str() {
            "next" => feed.pagination.next = Some(href),
            "previous" | "prev" => feed.pagination.previous = Some(href),
            "search" => {
                if let Some(t) = typ {
                    if t.contains("opensearchdescription") || raw.contains("{searchTerms}") {
                        feed.search = Some(SearchDescription { template: href });
                    }
                }
            }
            _ => {}
        }
    }
    Ok(())
}

pub fn resolve_http(base: &Url, href: &str) -> Result<Url> {
    let url = base.join(href)?;
    if matches!(url.scheme(), "http" | "https") {
        Ok(url)
    } else {
        bail!("OPDS only permits HTTP(S) URLs")
    }
}
pub fn same_origin(a: &Url, b: &Url) -> bool {
    a.scheme() == b.scheme()
        && a.host_str() == b.host_str()
        && a.port_or_known_default() == b.port_or_known_default()
}
pub fn expand_search(template: &str, query: &str) -> Result<Url> {
    let encoded = url::form_urlencoded::byte_serialize(query.as_bytes()).collect::<String>();
    Url::parse(
        &template
            .replace("{searchTerms}", &encoded)
            .replace("%7BsearchTerms%7D", &encoded)
            .replace("%7bsearchTerms%7d", &encoded),
    )
    .wrap_err("invalid OPDS search template")
}

/// Extract the OPDS/Atom result template from an OpenSearch description.
pub fn parse_search_description(xml: &str, base: &Url) -> Result<String> {
    let mut reader = Reader::from_str(xml);
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) | Ok(Event::Empty(e))
                if local_name(e.name().as_ref()) == b"Url" =>
            {
                let mut template = None;
                let mut media_type = None;
                for attr in e.attributes().with_checks(false) {
                    let attr = attr?;
                    let value = attr
                        .decoded_and_normalized_value(
                            quick_xml::XmlVersion::Implicit1_0,
                            e.decoder(),
                        )?
                        .into_owned();
                    match local_name(attr.key.as_ref()) {
                        b"template" => template = Some(value),
                        b"type" => media_type = Some(value),
                        _ => {}
                    }
                }
                if media_type.as_deref().is_some_and(|t| {
                    t.contains("application/atom+xml") || t.contains("opds-catalog")
                }) && let Some(template) = template
                {
                    return Ok(resolve_http(base, &template)?.to_string());
                }
            }
            Ok(Event::Eof) => bail!("OpenSearch description has no OPDS result template"),
            Err(error) => return Err(error).wrap_err("invalid OpenSearch description"),
            _ => {}
        }
    }
}

pub fn client(download: bool) -> Result<Client> {
    Ok(Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(if download { 300 } else { 30 }))
        .user_agent(format!(
            "repy/{} (+https://github.com/newptcai/repy)",
            env!("CARGO_PKG_VERSION")
        ))
        .build()?)
}
pub fn get_feed(
    client: &Client,
    url: &Url,
    origin: &Url,
    credentials: Option<(&str, Option<&str>)>,
) -> Result<Feed> {
    let mut req = client
        .get(url.clone())
        .header(reqwest::header::ACCEPT, OPDS_ACCEPT);
    if same_origin(url, origin) {
        if let Some((u, p)) = credentials {
            req = req.basic_auth(u, p);
        }
    }
    let response = req.send()?.error_for_status()?;
    let final_url = response.url().clone();
    parse_opds1(&response.text()?, &final_url)
}

pub fn search_feed(
    client: &Client,
    description: &SearchDescription,
    query: &str,
    origin: &Url,
    credentials: Option<(&str, Option<&str>)>,
) -> Result<Feed> {
    let template = if description.template.contains("{searchTerms}")
        || description.template.contains("%7BsearchTerms%7D")
    {
        description.template.clone()
    } else {
        let description_url = Url::parse(&description.template)?;
        let mut request = client.get(description_url.clone());
        if same_origin(&description_url, origin)
            && let Some((username, password)) = credentials
        {
            request = request.basic_auth(username, password);
        }
        let response = request.send()?.error_for_status()?;
        let final_url = response.url().clone();
        parse_search_description(&response.text()?, &final_url)?
    };
    let search_url = expand_search(&template, query)?;
    get_feed(client, &search_url, origin, credentials)
}

pub fn supported_extension(media: Option<&str>, href: &str) -> Option<&'static str> {
    let m = media
        .unwrap_or("")
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    match m.as_str() {
        "application/epub+zip" => Some("epub"),
        "application/x-fictionbook+xml" => Some("fb2"),
        "application/x-mobipocket-ebook" => Some("mobi"),
        "application/vnd.amazon.ebook" => Some("azw3"),
        "application/vnd.comicbook+zip" | "application/x-cbz" => Some("cbz"),
        "text/markdown" => Some("md"),
        "text/plain" => Some("txt"),
        _ => {
            let p = href.split('?').next().unwrap_or(href).to_ascii_lowercase();
            [
                "fb2.zip", "epub", "fb2", "mobi", "azw3", "azw", "cbz", "markdown", "md", "text",
                "txt",
            ]
            .into_iter()
            .find(|e| p.ends_with(&format!(".{e}")))
        }
    }
}

pub fn default_download_directory(configured: Option<&str>) -> Result<PathBuf> {
    if let Some(p) = configured {
        return Ok(PathBuf::from(p));
    }
    if let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) {
        return Ok(PathBuf::from(home).join("Downloads").join("repy"));
    }
    Ok(crate::config::get_app_data_prefix()?.join("downloads"))
}
fn sanitized_name(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | ' ') {
                c
            } else {
                '_'
            }
        })
        .collect();
    let s = s.trim_matches(|c: char| c == '.' || c == ' ').trim();
    if s.is_empty() {
        "book".into()
    } else {
        s.chars().take(180).collect()
    }
}
fn response_filename(response: &Response, url: &Url, title: &str, ext: &str) -> String {
    let cd = response
        .headers()
        .get(reqwest::header::CONTENT_DISPOSITION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| {
            v.split(';').find_map(|p| {
                p.trim()
                    .strip_prefix("filename=")
                    .map(|x| x.trim_matches('"'))
            })
        });
    let raw = cd
        .or_else(|| {
            url.path_segments()
                .and_then(|mut s| s.next_back())
                .filter(|s| !s.is_empty())
        })
        .unwrap_or(title);
    let mut n = sanitized_name(raw);
    if !n.to_ascii_lowercase().ends_with(&format!(".{ext}")) {
        n.push('.');
        n.push_str(ext);
    }
    n
}
fn collision_path(dir: &Path, filename: &str) -> PathBuf {
    let first = dir.join(filename);
    if !first.exists() {
        return first;
    }
    let p = Path::new(filename);
    let stem = p.file_stem().unwrap_or_default().to_string_lossy();
    let ext = p.extension().map(|x| x.to_string_lossy());
    for i in 2.. {
        let n = match &ext {
            Some(e) => format!("{stem}-{i}.{e}"),
            None => format!("{stem}-{i}"),
        };
        let c = dir.join(n);
        if !c.exists() {
            return c;
        }
    }
    unreachable!()
}
pub fn download<F>(
    client: &Client,
    link: &AcquisitionLink,
    title: &str,
    dir: &Path,
    origin: &Url,
    credentials: Option<(&str, Option<&str>)>,
    mut progress: F,
) -> Result<PathBuf>
where
    F: FnMut(u64, Option<u64>),
{
    let url = Url::parse(&link.href)?;
    let ext = link
        .extension()
        .ok_or_else(|| eyre::eyre!("unsupported acquisition format"))?;
    let mut req = client.get(url.clone());
    if same_origin(&url, origin) {
        if let Some((u, p)) = credentials {
            req = req.basic_auth(u, p);
        }
    }
    let mut response = req.send()?.error_for_status()?;
    let total = response.content_length();
    fs::create_dir_all(dir)?;
    let target = collision_path(dir, &response_filename(&response, &url, title, ext));
    // Keep the real extension last so the existing format factory can
    // validate the incomplete file before it becomes visible at `target`.
    let part = target.with_file_name(format!(
        ".{}.part.{ext}",
        target.file_stem().unwrap_or_default().to_string_lossy()
    ));
    let result = (|| -> Result<()> {
        let mut f = fs::File::create(&part)?;
        let mut downloaded = 0u64;
        let mut buffer = [0u8; 64 * 1024];
        progress(0, total);
        loop {
            let count = std::io::Read::read(&mut response, &mut buffer)?;
            if count == 0 {
                break;
            }
            f.write_all(&buffer[..count])?;
            downloaded += count as u64;
            progress(downloaded, total);
        }
        f.flush()?;
        crate::formats::open(part.to_str().unwrap_or_default())
            .map(|_| ())
            .wrap_err("downloaded file is not a valid ebook")?;
        fs::rename(&part, &target)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&part);
    }
    result.map(|_| target)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_navigation_publications_and_relative_links() {
        let base = Url::parse("https://example.test/catalog/").unwrap();
        let xml = r#"<feed xmlns="http://www.w3.org/2005/Atom"><title>Books</title><link rel="next" href="?page=2"/><link rel="search" type="application/opensearchdescription+xml" href="search?q={searchTerms}"/><entry><title>Fiction</title><link rel="subsection" href="fiction"/></entry><entry><title>Book</title><author><name>A One</name></author><author><name>B Two</name></author><summary type="html">A &lt;b&gt;story&lt;/b&gt;</summary><link rel="http://opds-spec.org/acquisition/open-access" type="application/epub+zip" href="book.epub"/><link rel="http://opds-spec.org/acquisition/buy" type="application/pdf" href="book.pdf"/></entry></feed>"#;
        let f = parse_opds1(xml, &base).unwrap();
        assert_eq!(f.navigation[0].href, "https://example.test/catalog/fiction");
        assert_eq!(f.publications[0].authors, vec!["A One", "B Two"]);
        assert_eq!(f.publications[0].summary.as_deref(), Some("A story"));
        assert_eq!(f.publications[0].readable_acquisitions().len(), 1);
        assert_eq!(
            f.pagination.next.as_deref(),
            Some("https://example.test/catalog/?page=2")
        );
        assert!(f.search.is_some());
    }
    #[test]
    fn parses_opensearch_pagination_totals() {
        let base = Url::parse("https://example.test/catalog/").unwrap();
        let xml = r#"<feed xmlns="http://www.w3.org/2005/Atom" xmlns:opensearch="http://a9.com/-/spec/opensearch/1.1/"><title>All Books</title><opensearch:totalResults>1234</opensearch:totalResults><opensearch:startIndex>26</opensearch:startIndex><opensearch:itemsPerPage>25</opensearch:itemsPerPage></feed>"#;
        let f = parse_opds1(xml, &base).unwrap();
        assert_eq!(f.pagination.total_results, Some(1234));
        assert_eq!(f.pagination.start_index, Some(26));
    }
    #[test]
    fn rejects_non_http_and_encodes_search() {
        assert!(resolve_http(&Url::parse("https://x/").unwrap(), "file:///tmp/a").is_err());
        assert_eq!(
            expand_search("https://x/?q={searchTerms}", "a b+c")
                .unwrap()
                .as_str(),
            "https://x/?q=a+b%2Bc"
        );
    }
    #[test]
    fn origin_includes_port() {
        let a = Url::parse("https://x/a").unwrap();
        assert!(same_origin(&a, &Url::parse("https://x:443/b").unwrap()));
        assert!(!same_origin(&a, &Url::parse("https://x:444/b").unwrap()));
    }

    #[test]
    fn parses_external_opensearch_description() {
        let xml = r#"<OpenSearchDescription xmlns="http://a9.com/-/spec/opensearch/1.1/"><Url type="text/html" template="/html?q={searchTerms}"/><Url type="application/atom+xml;profile=opds-catalog" template="/ebooks/search.opds/?query={searchTerms}"/></OpenSearchDescription>"#;
        assert_eq!(
            parse_search_description(
                xml,
                &Url::parse("https://example.test/catalog/osd.xml").unwrap()
            )
            .unwrap(),
            "https://example.test/ebooks/search.opds/?query={searchTerms}"
        );
    }
}
