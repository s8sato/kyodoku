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
    let mut cleaned = String::new();
    for ch in code.chars() {
        if ch.is_ascii_digit() {
            cleaned.push(ch);
        } else if matches!(ch, 'x' | 'X') {
            cleaned.push('X');
        }
    }

    match cleaned.len() {
        10 => {
            if !is_valid_isbn10(&cleaned) {
                return Err(anyhow!("invalid ISBN-10"));
            }
            let isbn_13 = isbn10_to_13(&cleaned)?;
            Ok(NormalizedIsbn {
                isbn_13,
                isbn_10: Some(cleaned),
            })
        }
        13 => {
            if !is_valid_isbn13(&cleaned) {
                return Err(anyhow!("invalid ISBN-13"));
            }
            let isbn_10 = isbn13_to_10(&cleaned);
            Ok(NormalizedIsbn {
                isbn_13: cleaned,
                isbn_10,
            })
        }
        _ => Err(anyhow!("ISBN must be 10 or 13 characters")),
    }
}

fn isbn10_to_13(isbn_10: &str) -> Result<String> {
    if isbn_10.len() != 10 {
        return Err(anyhow!("ISBN-10 must have 10 characters"));
    }
    let body = &isbn_10[..9];
    let mut prefix = String::from("978");
    prefix.push_str(body);
    let check = compute_isbn13_check_digit(prefix.as_bytes());
    prefix.push(char::from(b'0' + check as u8));
    Ok(prefix)
}

fn isbn13_to_10(isbn_13: &str) -> Option<String> {
    if !isbn_13.starts_with("978") {
        return None;
    }
    let body = &isbn_13[3..12];
    if !body.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let check = compute_isbn10_check_digit(body)?;
    let check_char = match check {
        10 => 'X',
        value => char::from(b'0' + value as u8),
    };
    Some(format!("{}{}", body, check_char))
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

fn compute_isbn10_check_digit(body: &str) -> Option<u32> {
    if body.len() != 9 || !body.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let mut sum = 0u32;
    for (idx, ch) in body.chars().enumerate() {
        let digit = ch.to_digit(10).unwrap();
        sum += digit * (10 - idx as u32);
    }
    let remainder = sum % 11;
    let check = if remainder == 0 { 0 } else { 11 - remainder };
    Some(check)
}

fn is_valid_isbn10(isbn: &str) -> bool {
    if isbn.len() != 10 {
        return false;
    }
    let mut sum = 0u32;
    for (idx, ch) in isbn.chars().enumerate() {
        let weight = 10 - idx as u32;
        let value = match ch {
            'X' if idx == 9 => 10,
            c if c.is_ascii_digit() => c.to_digit(10).unwrap(),
            _ => return false,
        };
        sum += value * weight;
    }
    sum % 11 == 0
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_isbn10_with_x_check_digit() {
        let normalized = normalize("0-8044-2957-X").expect("normalize succeeds");
        assert_eq!(normalized.isbn_13, "9780804429573");
        assert_eq!(normalized.isbn_10.as_deref(), Some("080442957X"));
    }

    #[test]
    fn normalizes_isbn13_and_derives_isbn10() {
        let normalized = normalize("9780306406157").expect("normalize succeeds");
        assert_eq!(normalized.isbn_13, "9780306406157");
        assert_eq!(normalized.isbn_10.as_deref(), Some("0306406152"));
    }

    #[test]
    fn rejects_invalid_codes() {
        assert!(normalize("12345").is_err());
        assert!(normalize("9780306406158").is_err());
        assert!(normalize("0306406153").is_err());
    }
}
