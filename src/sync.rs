//! Minimal client for KOReader's progress-sync (kosync) protocol.

use eyre::{Result, WrapErr, eyre};
use md5::{Digest, Md5};
use reqwest::blocking::Client;
use serde::Deserialize;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::time::Duration;

const MEDIA_TYPE: &str = "application/vnd.koreader.v1+json";

#[derive(Debug, Clone, PartialEq)]
pub struct KosyncConfig {
    pub server: String,
    pub username: String,
    pub user_key: String,
}

impl KosyncConfig {
    pub fn new(server: &str, username: &str, user_key: &str) -> Option<Self> {
        let server = server.trim().trim_end_matches('/');
        let username = username.trim();
        let user_key = user_key.trim();
        if server.is_empty() || username.is_empty() || user_key.is_empty() {
            return None;
        }
        Some(Self {
            server: server.to_string(),
            username: username.to_string(),
            user_key: user_key.to_ascii_lowercase(),
        })
    }

    pub fn from_password(server: &str, username: &str, password: &str) -> Option<Self> {
        Self::new(server, username, &password_key(password))
    }
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct RemoteProgress {
    pub document: String,
    pub progress: String,
    pub percentage: f64,
    pub device: String,
    #[serde(default)]
    pub device_id: String,
    #[serde(default)]
    pub timestamp: i64,
}

pub fn password_key(password: &str) -> String {
    hex::encode(Md5::digest(password.as_bytes()))
}

/// KOReader's non-even partial-MD5 sampling algorithm. It hashes 1 KiB at
/// the file head, then at 1 KiB and successive powers of four up to 1 GiB.
pub fn document_id(path: impl AsRef<Path>) -> Result<String> {
    let mut file = File::open(path.as_ref())
        .wrap_err_with(|| format!("cannot open {} for kosync", path.as_ref().display()))?;
    let mut md5 = Md5::new();
    let mut buf = [0u8; 1024];
    let mut offset = 0u64;
    for sample_index in 0..12 {
        file.seek(SeekFrom::Start(offset))?;
        let read = file.read(&mut buf)?;
        if read == 0 {
            break;
        }
        md5.update(&buf[..read]);
        offset = if sample_index == 0 {
            1024
        } else {
            offset.saturating_mul(4)
        };
    }
    Ok(hex::encode(md5.finalize()))
}

fn client() -> Result<Client> {
    Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(20))
        .build()
        .wrap_err("cannot build kosync HTTP client")
}

fn request(
    client: &Client,
    config: &KosyncConfig,
    method: reqwest::Method,
    url: String,
) -> reqwest::blocking::RequestBuilder {
    client
        .request(method, url)
        .header(reqwest::header::ACCEPT, MEDIA_TYPE)
        .header(reqwest::header::CONTENT_TYPE, MEDIA_TYPE)
        .header("x-auth-user", &config.username)
        .header("x-auth-key", &config.user_key)
}

pub fn pull(config: &KosyncConfig, document: &str) -> Result<Option<RemoteProgress>> {
    let client = client()?;
    let url = format!("{}/syncs/progress/{}", config.server, document);
    let response = request(&client, config, reqwest::Method::GET, url).send()?;
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }
    if !response.status().is_success() {
        return Err(eyre!("kosync pull failed: HTTP {}", response.status()));
    }
    parse_pull_body(
        &response
            .text()
            .wrap_err("invalid kosync progress response")?,
    )
}

/// Parse a kosync progress body. The common server implementations answer
/// HTTP 200 with an empty (or document-only) JSON object when they have no
/// progress stored for the document, so that case is `None`, not an error.
fn parse_pull_body(body: &str) -> Result<Option<RemoteProgress>> {
    let value: serde_json::Value =
        serde_json::from_str(body).wrap_err("invalid kosync progress response")?;
    if value.get("progress").is_none() || value.get("percentage").is_none() {
        return Ok(None);
    }
    Ok(Some(
        serde_json::from_value(value).wrap_err("invalid kosync progress response")?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn empty_progress_response_is_none_not_an_error() {
        // Sync servers answer 200 with an empty or document-only object
        // for books they have never seen.
        assert_eq!(parse_pull_body("{}").unwrap(), None);
        assert_eq!(parse_pull_body(r#"{"document": "abc123"}"#).unwrap(), None);
        let full = r#"{"document":"abc","progress":"0.42","percentage":0.42,"device":"KOReader"}"#;
        let parsed = parse_pull_body(full).unwrap().unwrap();
        assert_eq!(parsed.percentage, 0.42);
        assert_eq!(parsed.device, "KOReader");
        assert!(parse_pull_body("not json").is_err());
    }

    #[test]
    fn derives_protocol_password_key() {
        assert_eq!(password_key("password"), "5f4dcc3b5aa765d61d8327deb882cf99");
    }

    #[test]
    fn partial_md5_uses_koreader_offsets() {
        let mut temp = tempfile::NamedTempFile::new().unwrap();
        let bytes: Vec<u8> = (0..20_000).map(|i| (i % 251) as u8).collect();
        temp.write_all(&bytes).unwrap();
        let mut expected = Md5::new();
        for offset in [0usize, 1024, 4096, 16384] {
            expected.update(&bytes[offset..(offset + 1024).min(bytes.len())]);
        }
        assert_eq!(
            document_id(temp.path()).unwrap(),
            hex::encode(expected.finalize())
        );
    }
}
