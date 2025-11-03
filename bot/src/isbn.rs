use std::fmt;

use anyhow::{anyhow, Result};
use reqwest::Client;
use serde::Deserialize;

#[derive(Debug, Clone)]
pub struct NormalizedIsbn {
    pub isbn_13: String,
    pub isbn_10: Option<String>,
}

#[derive(Debug, Clone)]
pub struct IsbnMetadata {
    pub isbn_13: String,
    pub isbn_10: Option<String>,
    pub title: String,
    pub subtitle: Option<String>,
    pub authors: Vec<String>,
    pub source: MetadataSource,
}

impl IsbnMetadata {
    pub fn display_title(&self) -> String {
        match &self.subtitle {
            Some(subtitle) if !subtitle.is_empty() => format!("{}: {}", self.title, subtitle),
            _ => self.title.clone(),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum MetadataSource {
    OpenLibrary,
    GoogleBooks,
    Manual,
}

impl MetadataSource {
    pub fn as_str(&self) -> &'static str {
        match self {
            MetadataSource::OpenLibrary => "open_library",
            MetadataSource::GoogleBooks => "google_books",
            MetadataSource::Manual => "manual",
        }
    }
}

impl fmt::Display for MetadataSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

pub fn normalize(code: &str) -> Result<NormalizedIsbn> {
    let digits: String = code.chars().filter(|c| c.is_ascii_digit()).collect();
    if digits.len() == 10 {
        let isbn_10 = digits;
        let isbn_13 = isbn10_to_13(&isbn_10)?;
        Ok(NormalizedIsbn {
            isbn_13,
            isbn_10: Some(isbn_10),
        })
    } else if digits.len() == 13 {
        if !is_valid_isbn13(&digits) {
            return Err(anyhow!("invalid ISBN-13"));
        }
        Ok(NormalizedIsbn {
            isbn_13: digits,
            isbn_10: None,
        })
    } else {
        Err(anyhow!("ISBN must be 10 or 13 digits"))
    }
}

fn isbn10_to_13(isbn_10: &str) -> Result<String> {
    if isbn_10.len() != 10 {
        return Err(anyhow!("ISBN-10 must have 10 digits"));
    }
    let mut prefix = String::from("978");
    prefix.push_str(&isbn_10[..9]);
    let check = compute_isbn13_check_digit(prefix.as_bytes());
    prefix.push(char::from(b'0' + check as u8));
    Ok(prefix)
}

fn compute_isbn13_check_digit(bytes: &[u8]) -> u32 {
    let mut sum = 0u32;
    for (idx, byte) in bytes.iter().enumerate() {
        let digit = (byte - b'0') as u32;
        if idx % 2 == 0 {
            sum += digit;
        } else {
            sum += digit * 3;
        }
    }
    let rem = sum % 10;
    if rem == 0 {
        0
    } else {
        10 - rem
    }
}

fn is_valid_isbn13(isbn: &str) -> bool {
    if isbn.len() != 13 || !isbn.chars().all(|c| c.is_ascii_digit()) {
        return false;
    }
    let check = compute_isbn13_check_digit(isbn[..12].as_bytes());
    match isbn.chars().last().and_then(|c| c.to_digit(10)) {
        Some(last) => check == last,
        None => false,
    }
}

pub async fn lookup_metadata(
    client: &Client,
    normalized: &NormalizedIsbn,
    title_override: Option<&str>,
) -> Result<IsbnMetadata> {
    if let Some(meta) = fetch_open_library(client, normalized).await? {
        return Ok(meta);
    }

    if let Some(meta) = fetch_google_books(client, normalized).await? {
        return Ok(meta);
    }

    if let Some(title) = title_override {
        return Ok(IsbnMetadata {
            isbn_13: normalized.isbn_13.clone(),
            isbn_10: normalized.isbn_10.clone(),
            title: title.to_string(),
            subtitle: None,
            authors: Vec::new(),
            source: MetadataSource::Manual,
        });
    }

    Err(anyhow!(
        "Unable to resolve metadata for ISBN {}",
        normalized.isbn_13
    ))
}

async fn fetch_open_library(
    client: &Client,
    normalized: &NormalizedIsbn,
) -> Result<Option<IsbnMetadata>> {
    let url = format!("https://openlibrary.org/isbn/{}.json", normalized.isbn_13);
    let response = client.get(&url).send().await?;
    if response.status().is_success() {
        let data: OpenLibraryResponse = response.json().await?;
        if let Some(title) = data.title {
            let authors = data
                .authors
                .into_iter()
                .filter_map(|author| author.name)
                .collect();
            return Ok(Some(IsbnMetadata {
                isbn_13: normalized.isbn_13.clone(),
                isbn_10: normalized.isbn_10.clone(),
                title,
                subtitle: data.subtitle,
                authors,
                source: MetadataSource::OpenLibrary,
            }));
        }
    } else if response.status().as_u16() == 404 {
        return Ok(None);
    }
    Ok(None)
}

async fn fetch_google_books(
    client: &Client,
    normalized: &NormalizedIsbn,
) -> Result<Option<IsbnMetadata>> {
    let url = format!(
        "https://www.googleapis.com/books/v1/volumes?q=isbn:{}",
        normalized.isbn_13
    );
    let response = client.get(&url).send().await?;
    if !response.status().is_success() {
        return Ok(None);
    }

    let data: GoogleBooksResponse = response.json().await?;
    let item = data.items.unwrap_or_default().into_iter().next();
    if let Some(item) = item {
        if let Some(title) = item.volume_info.title {
            return Ok(Some(IsbnMetadata {
                isbn_13: normalized.isbn_13.clone(),
                isbn_10: normalized.isbn_10.clone(),
                title,
                subtitle: item.volume_info.subtitle,
                authors: item.volume_info.authors.unwrap_or_default(),
                source: MetadataSource::GoogleBooks,
            }));
        }
    }

    Ok(None)
}

#[derive(Debug, Deserialize)]
struct OpenLibraryResponse {
    title: Option<String>,
    subtitle: Option<String>,
    #[serde(default)]
    authors: Vec<OpenLibraryAuthor>,
}

#[derive(Debug, Deserialize)]
struct OpenLibraryAuthor {
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GoogleBooksResponse {
    #[serde(default)]
    items: Option<Vec<GoogleBooksItem>>,
}

#[derive(Debug, Deserialize)]
struct GoogleBooksItem {
    #[serde(rename = "volumeInfo")]
    volume_info: GoogleVolumeInfo,
}

#[derive(Debug, Deserialize)]
struct GoogleVolumeInfo {
    title: Option<String>,
    subtitle: Option<String>,
    #[serde(default)]
    authors: Option<Vec<String>>,
}
