//! Datasheet retrieval, caching, and PDF-to-Markdown conversion.

use regex::Regex;
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use thiserror::Error;

use crate::cache;
use crate::graph::Component;

#[derive(Debug, Error)]
pub enum DatasheetError {
    #[error("component {reference} has no datasheet URL; exact MPN: {mpn}")]
    MissingUrl { reference: String, mpn: String },
    #[error("unsupported datasheet URL for {reference}: {url}")]
    UnsupportedUrl { reference: String, url: String },
    #[error("missing required environment variable {variable} for Digi-Key resolution")]
    MissingCredential { variable: &'static str },
    #[error("component {reference} has no MPN for Digi-Key resolution")]
    MissingMpn { reference: String },
    #[error("Digi-Key API request failed: {source}")]
    ApiRequest { source: reqwest::Error },
    #[error("Digi-Key API returned HTTP status {status}: {body}")]
    ApiResponse { status: u16, body: String },
    #[error("Digi-Key API response could not be decoded: {source}")]
    ApiDecode { source: reqwest::Error },
    #[error("invalid Digi-Key API configuration: {message}")]
    ApiConfiguration { message: String },
    #[error("could not determine the Trace cache directory")]
    CacheDirectoryUnavailable,
    #[error("failed to create datasheet cache directory {path}: {source}")]
    CreateDirectory {
        path: String,
        source: std::io::Error,
    },
    #[error("failed to download datasheet {url}: {source}")]
    Download { url: String, source: reqwest::Error },
    #[error("datasheet download returned HTTP status {status} for {url}")]
    HttpStatus { url: String, status: u16 },
    #[error("datasheet URL did not return a PDF: {url}")]
    NotPdf { url: String },
    #[error("failed to write datasheet file {path}: {source}")]
    Write {
        path: String,
        source: std::io::Error,
    },
    #[error("failed to read datasheet file {path}: {source}")]
    Read {
        path: String,
        source: std::io::Error,
    },
    #[error("invalid datasheet search pattern {pattern}: {message}")]
    InvalidSearchPattern { pattern: String, message: String },
    #[error("pdf-inspector could not parse {path}: {message}")]
    Parse { path: String, message: String },
    #[error("failed to encode datasheet metadata: {0}")]
    MetadataEncode(#[from] serde_json::Error),
}

#[derive(Debug, Clone)]
pub struct DatasheetCandidate {
    pub provider: String,
    pub manufacturer: Option<String>,
    pub mpn: Option<String>,
    pub datasheet_url: Option<String>,
    pub product_url: Option<String>,
    pub exact_match: bool,
    pub match_kind: String,
}

pub trait DatasheetResolver {
    fn resolve(&self, component: &Component) -> Result<Vec<DatasheetCandidate>, DatasheetError>;
}

#[derive(Debug, Clone)]
pub struct DigiKeyResolver {
    client: Client,
    client_id: String,
    client_secret: String,
    api_base: String,
    site: String,
    language: String,
    currency: String,
    account_id: Option<String>,
}

impl DigiKeyResolver {
    pub fn from_env() -> Result<Self, DatasheetError> {
        Ok(Self {
            client: Client::builder()
                .user_agent("Trace/0.1 datasheet resolver")
                .build()
                .map_err(|source| DatasheetError::ApiRequest { source })?,
            client_id: required_env("TRACE_DIGIKEY_CLIENT_ID")?,
            client_secret: required_env("TRACE_DIGIKEY_CLIENT_SECRET")?,
            api_base: std::env::var("TRACE_DIGIKEY_API_BASE")
                .unwrap_or_else(|_| "https://api.digikey.com".to_string())
                .trim_end_matches('/')
                .to_string(),
            site: std::env::var("TRACE_DIGIKEY_SITE").unwrap_or_else(|_| "US".to_string()),
            language: std::env::var("TRACE_DIGIKEY_LANGUAGE").unwrap_or_else(|_| "en".to_string()),
            currency: std::env::var("TRACE_DIGIKEY_CURRENCY").unwrap_or_else(|_| "USD".to_string()),
            account_id: std::env::var("TRACE_DIGIKEY_ACCOUNT_ID").ok(),
        })
    }

    fn access_token(&self) -> Result<String, DatasheetError> {
        let response = self
            .client
            .post(format!("{}/v1/oauth2/token", self.api_base))
            .form(&[
                ("client_id", self.client_id.as_str()),
                ("client_secret", self.client_secret.as_str()),
                ("grant_type", "client_credentials"),
            ])
            .send()
            .map_err(|source| DatasheetError::ApiRequest { source })?;

        let status = response.status();
        if !status.is_success() {
            return Err(DatasheetError::ApiResponse {
                status: status.as_u16(),
                body: response.text().unwrap_or_default(),
            });
        }

        let token = response
            .json::<DigiKeyToken>()
            .map_err(|source| DatasheetError::ApiDecode { source })?;
        Ok(token.access_token)
    }
}

impl DatasheetResolver for DigiKeyResolver {
    fn resolve(&self, component: &Component) -> Result<Vec<DatasheetCandidate>, DatasheetError> {
        let mpn = component
            .mpn
            .as_deref()
            .filter(|mpn| !mpn.trim().is_empty())
            .or_else(|| (!component.value.trim().is_empty()).then_some(component.value.as_str()))
            .ok_or_else(|| DatasheetError::MissingMpn {
                reference: component.reference.clone(),
            })?;
        let token = self.access_token()?;
        let mut endpoint = reqwest::Url::parse(&format!("{}/products/v4/search", self.api_base))
            .map_err(|error| DatasheetError::ApiConfiguration {
                message: error.to_string(),
            })?;
        endpoint
            .path_segments_mut()
            .map_err(|_| DatasheetError::ApiConfiguration {
                message: "Digi-Key API base URL cannot accept path segments".to_string(),
            })?
            .push(mpn)
            .push("productdetails");

        let mut request = self
            .client
            .get(endpoint)
            .header("X-DIGIKEY-Client-Id", &self.client_id)
            .header("X-DIGIKEY-Locale-Site", &self.site)
            .header("X-DIGIKEY-Locale-Language", &self.language)
            .header("X-DIGIKEY-Locale-Currency", &self.currency)
            .bearer_auth(token);
        if let Some(account_id) = &self.account_id {
            request = request.header("X-DIGIKEY-Account-Id", account_id);
        }

        let response = request
            .send()
            .map_err(|source| DatasheetError::ApiRequest { source })?;
        let status = response.status();
        if !status.is_success() {
            return Err(DatasheetError::ApiResponse {
                status: status.as_u16(),
                body: response.text().unwrap_or_default(),
            });
        }

        let payload = response
            .json::<DigiKeyProductResponse>()
            .map_err(|source| DatasheetError::ApiDecode { source })?;
        let Some(product) = payload.product else {
            return Ok(Vec::new());
        };

        let returned_mpn = product.manufacturer_part_number;
        let returned_manufacturer = product
            .manufacturer
            .and_then(|manufacturer| manufacturer.name);
        let exact_mpn = returned_mpn
            .as_deref()
            .is_some_and(|returned| returned.trim().eq_ignore_ascii_case(mpn.trim()));
        let exact_manufacturer = match (&component.manufacturer, &returned_manufacturer) {
            (Some(expected), Some(returned)) => {
                expected.trim().eq_ignore_ascii_case(returned.trim())
            }
            (None, _) => true,
            _ => false,
        };
        let exact_match = exact_mpn && exact_manufacturer;
        let match_kind = if exact_match {
            "exact-mpn-and-manufacturer"
        } else if exact_mpn {
            "exact-mpn-manufacturer-unverified"
        } else {
            "non-exact-candidate"
        };

        Ok(vec![DatasheetCandidate {
            provider: "digikey".to_string(),
            manufacturer: returned_manufacturer,
            mpn: returned_mpn,
            datasheet_url: product.datasheet_url,
            product_url: product.product_url,
            exact_match,
            match_kind: match_kind.to_string(),
        }])
    }
}

#[derive(Debug, Deserialize)]
struct DigiKeyToken {
    access_token: String,
}

#[derive(Debug, Deserialize)]
struct DigiKeyProductResponse {
    #[serde(rename = "Product")]
    product: Option<DigiKeyProduct>,
}

#[derive(Debug, Deserialize)]
struct DigiKeyProduct {
    #[serde(rename = "ManufacturerProductNumber")]
    manufacturer_part_number: Option<String>,
    #[serde(rename = "Manufacturer")]
    manufacturer: Option<DigiKeyManufacturer>,
    #[serde(rename = "DatasheetUrl")]
    datasheet_url: Option<String>,
    #[serde(rename = "ProductUrl")]
    product_url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct DigiKeyManufacturer {
    #[serde(rename = "Name")]
    name: Option<String>,
}

fn required_env(variable: &'static str) -> Result<String, DatasheetError> {
    std::env::var(variable).map_err(|_| DatasheetError::MissingCredential { variable })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatasheetMetadata {
    pub schema: u32,
    pub component_reference: String,
    pub component_value: String,
    pub manufacturer: Option<String>,
    pub mpn: Option<String>,
    pub source_url: String,
    pub pdf_type: String,
    pub confidence: f32,
    pub page_count: u32,
    #[serde(default)]
    pub resolution_source: String,
    #[serde(default)]
    pub match_kind: String,
    #[serde(default = "default_document_kind")]
    pub document_kind: String,
}

#[derive(Debug, Clone)]
pub struct DatasheetDocument {
    pub source_pdf: PathBuf,
    pub markdown: PathBuf,
    pub metadata: PathBuf,
    pub information: DatasheetMetadata,
    pub cache_hit: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelatedDocumentCandidate {
    pub kind: String,
    pub url: String,
    pub discovery_source: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DatasheetSearchMatch {
    pub line_number: usize,
    pub line: String,
    pub context_before: Vec<String>,
    pub context_after: Vec<String>,
}

pub fn retrieve_and_parse(
    component: &Component,
    refresh: bool,
) -> Result<DatasheetDocument, DatasheetError> {
    retrieve_and_parse_with_provenance(
        component,
        refresh,
        "kicad-datasheet-field",
        "exact-url",
        "primary-datasheet",
    )
}

pub fn retrieve_and_parse_candidate(
    component: &Component,
    candidate: &DatasheetCandidate,
    refresh: bool,
) -> Result<DatasheetDocument, DatasheetError> {
    let mut resolved_component = component.clone();
    resolved_component.datasheet_url = candidate.datasheet_url.clone();
    retrieve_and_parse_with_provenance(
        &resolved_component,
        refresh,
        &candidate.provider,
        &candidate.match_kind,
        "resolved-datasheet",
    )
}

pub fn related_document_candidates(
    component: &Component,
    primary: &DatasheetDocument,
) -> Result<Vec<RelatedDocumentCandidate>, DatasheetError> {
    let mut candidates = known_related_documents(component);
    let markdown =
        std::fs::read_to_string(&primary.markdown).map_err(|source| DatasheetError::Read {
            path: primary.markdown.display().to_string(),
            source,
        })?;

    for url in extract_pdf_urls(&markdown) {
        if url == primary.information.source_url
            || candidates.iter().any(|candidate| candidate.url == url)
            || !is_allowed_related_url(&url, &primary.information.source_url)
        {
            continue;
        }
        candidates.push(RelatedDocumentCandidate {
            kind: "linked-pdf".to_string(),
            url,
            discovery_source: "primary-datasheet-markdown-link".to_string(),
        });
    }

    Ok(candidates)
}

pub fn retrieve_related_documents(
    component: &Component,
    primary: &DatasheetDocument,
    refresh: bool,
) -> Result<Vec<(RelatedDocumentCandidate, DatasheetDocument)>, DatasheetError> {
    related_document_candidates(component, primary)?
        .into_iter()
        .map(|candidate| {
            let mut related_component = component.clone();
            related_component.datasheet_url = Some(candidate.url.clone());
            let document = retrieve_and_parse_with_provenance(
                &related_component,
                refresh,
                &candidate.discovery_source,
                "related-document",
                &candidate.kind,
            )?;
            Ok((candidate, document))
        })
        .collect()
}

pub fn search_markdown(
    markdown: &str,
    pattern: &str,
    context_lines: usize,
) -> Result<Vec<DatasheetSearchMatch>, DatasheetError> {
    let regex = Regex::new(pattern).map_err(|error| DatasheetError::InvalidSearchPattern {
        pattern: pattern.to_string(),
        message: error.to_string(),
    })?;
    let lines = markdown.lines().map(str::to_string).collect::<Vec<_>>();
    let mut matches = Vec::new();

    for (index, line) in lines.iter().enumerate() {
        if !regex.is_match(line) {
            continue;
        }

        let before_start = index.saturating_sub(context_lines);
        let after_end = (index + context_lines + 1).min(lines.len());
        matches.push(DatasheetSearchMatch {
            line_number: index + 1,
            line: line.clone(),
            context_before: lines[before_start..index].to_vec(),
            context_after: lines[index + 1..after_end].to_vec(),
        });
    }

    Ok(matches)
}

pub fn search_document(
    document: &DatasheetDocument,
    pattern: &str,
    context_lines: usize,
) -> Result<Vec<DatasheetSearchMatch>, DatasheetError> {
    let markdown =
        std::fs::read_to_string(&document.markdown).map_err(|source| DatasheetError::Read {
            path: document.markdown.display().to_string(),
            source,
        })?;
    search_markdown(&markdown, pattern, context_lines)
}

fn retrieve_and_parse_with_provenance(
    component: &Component,
    refresh: bool,
    resolution_source: &str,
    match_kind: &str,
    document_kind: &str,
) -> Result<DatasheetDocument, DatasheetError> {
    let url = component
        .datasheet_url
        .as_deref()
        .ok_or_else(|| DatasheetError::MissingUrl {
            reference: component.reference.clone(),
            mpn: component
                .mpn
                .clone()
                .unwrap_or_else(|| component.value.clone()),
        })?;

    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err(DatasheetError::UnsupportedUrl {
            reference: component.reference.clone(),
            url: url.to_string(),
        });
    }

    let root = cache::cache_directory().ok_or(DatasheetError::CacheDirectoryUnavailable)?;
    let directory = root.join("datasheets").join(identity_key(component, url));
    std::fs::create_dir_all(&directory).map_err(|source| DatasheetError::CreateDirectory {
        path: directory.display().to_string(),
        source,
    })?;

    let source_pdf = directory.join("source.pdf");
    let markdown = directory.join("document.md");
    let metadata = directory.join("metadata.json");
    let complete_cache = source_pdf.is_file() && markdown.is_file() && metadata.is_file();

    if refresh || !source_pdf.is_file() {
        download_pdf(url, &source_pdf)?;
    }

    if !refresh && complete_cache {
        let information = read_metadata(&metadata)?;
        return Ok(DatasheetDocument {
            source_pdf,
            markdown,
            metadata,
            information,
            cache_hit: true,
        });
    }

    let result =
        pdf_inspector::process_pdf(&source_pdf).map_err(|error| DatasheetError::Parse {
            path: source_pdf.display().to_string(),
            message: error.to_string(),
        })?;
    let markdown_text = result.markdown.ok_or_else(|| DatasheetError::Parse {
        path: source_pdf.display().to_string(),
        message: format!(
            "pdf-inspector returned no Markdown for {:?}",
            result.pdf_type
        ),
    })?;

    std::fs::write(&markdown, markdown_text).map_err(|source| DatasheetError::Write {
        path: markdown.display().to_string(),
        source,
    })?;

    let information = DatasheetMetadata {
        schema: 1,
        component_reference: component.reference.clone(),
        component_value: component.value.clone(),
        manufacturer: component.manufacturer.clone(),
        mpn: component.mpn.clone(),
        source_url: url.to_string(),
        pdf_type: format!("{:?}", result.pdf_type),
        confidence: result.confidence,
        page_count: result.page_count,
        resolution_source: resolution_source.to_string(),
        match_kind: match_kind.to_string(),
        document_kind: document_kind.to_string(),
    };
    std::fs::write(&metadata, serde_json::to_vec_pretty(&information)?).map_err(|source| {
        DatasheetError::Write {
            path: metadata.display().to_string(),
            source,
        }
    })?;

    Ok(DatasheetDocument {
        source_pdf,
        markdown,
        metadata,
        information,
        cache_hit: false,
    })
}

fn download_pdf(url: &str, destination: &Path) -> Result<(), DatasheetError> {
    let client = Client::builder()
        .user_agent("Trace/0.1 datasheet retrieval")
        .build()
        .map_err(|source| DatasheetError::Download {
            url: url.to_string(),
            source,
        })?;
    let response = client
        .get(url)
        .send()
        .map_err(|source| DatasheetError::Download {
            url: url.to_string(),
            source,
        })?;

    if !response.status().is_success() {
        return Err(DatasheetError::HttpStatus {
            url: url.to_string(),
            status: response.status().as_u16(),
        });
    }

    let bytes = response
        .bytes()
        .map_err(|source| DatasheetError::Download {
            url: url.to_string(),
            source,
        })?;
    if bytes.len() < 5 || &bytes[..5] != b"%PDF-" {
        return Err(DatasheetError::NotPdf {
            url: url.to_string(),
        });
    }

    std::fs::write(destination, bytes).map_err(|source| DatasheetError::Write {
        path: destination.display().to_string(),
        source,
    })
}

fn read_metadata(path: &Path) -> Result<DatasheetMetadata, DatasheetError> {
    let bytes = std::fs::read(path).map_err(|source| DatasheetError::Read {
        path: path.display().to_string(),
        source,
    })?;
    serde_json::from_slice(&bytes).map_err(DatasheetError::MetadataEncode)
}

fn identity_key(component: &Component, url: &str) -> String {
    let identity = format!(
        "{}\0{}\0{}\0{}",
        component.manufacturer.as_deref().unwrap_or_default(),
        component.mpn.as_deref().unwrap_or_default(),
        component.value,
        url
    );
    let digest = Sha256::digest(identity.as_bytes());
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn default_document_kind() -> String {
    "datasheet".to_string()
}

fn known_related_documents(component: &Component) -> Vec<RelatedDocumentCandidate> {
    let value = component.value.to_ascii_lowercase();
    let manufacturer_matches = component
        .manufacturer
        .as_deref()
        .is_none_or(|manufacturer| manufacturer.to_ascii_lowercase().contains("microchip"));
    if !manufacturer_matches || !(value.contains("atmega4808") || value.contains("atmega4809")) {
        return Vec::new();
    }

    vec![
        RelatedDocumentCandidate {
            kind: "family-datasheet".to_string(),
            url: "https://ww1.microchip.com/downloads/en/DeviceDoc/ATmega4808-4809-Data-Sheet-DS40002173A.pdf".to_string(),
            discovery_source: "microchip-family-document-set".to_string(),
        },
        RelatedDocumentCandidate {
            kind: "errata".to_string(),
            url: "https://ww1.microchip.com/downloads/en/DeviceDoc/ATmega4808-09-SilConErrataClarif-DS80000867B.pdf".to_string(),
            discovery_source: "microchip-family-document-set".to_string(),
        },
        RelatedDocumentCandidate {
            kind: "avr-instruction-set".to_string(),
            url: "https://ww1.microchip.com/downloads/en/DeviceDoc/AVR-InstructionSet-Manual-DS40002198.pdf".to_string(),
            discovery_source: "microchip-avr-document-set".to_string(),
        },
    ]
}

fn extract_pdf_urls(markdown: &str) -> Vec<String> {
    let url_pattern = Regex::new(r"https?://[^\s)\]>]+(?i:\.pdf)(?:\?[^\s)\]>]+)?")
        .expect("static PDF URL pattern must be valid");
    url_pattern
        .find_iter(markdown)
        .map(|matched| {
            matched
                .as_str()
                .trim_end_matches(&['.', ',', ';'][..])
                .to_string()
        })
        .collect()
}

fn is_allowed_related_url(url: &str, primary_url: &str) -> bool {
    let Ok(candidate) = reqwest::Url::parse(url) else {
        return false;
    };
    let Ok(primary) = reqwest::Url::parse(primary_url) else {
        return false;
    };
    let Some(candidate_host) = candidate.host_str() else {
        return false;
    };
    let Some(primary_host) = primary.host_str() else {
        return false;
    };

    candidate_host.eq_ignore_ascii_case(primary_host)
        || candidate_host
            .to_ascii_lowercase()
            .ends_with(".microchip.com")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_key_is_stable_and_path_safe() {
        let component = Component {
            reference: "U1".to_string(),
            value: "ATmega4808-AU".to_string(),
            footprint: None,
            manufacturer: Some("Microchip".to_string()),
            mpn: Some("ATmega4808-AU".to_string()),
            datasheet_url: Some("https://example.test/mcu.pdf".to_string()),
        };
        let first = identity_key(&component, "https://example.test/mcu.pdf");
        let second = identity_key(&component, "https://example.test/mcu.pdf");
        assert_eq!(first, second);
        assert_eq!(first.len(), 64);
        assert!(first.chars().all(|character| character.is_ascii_hexdigit()));
    }

    #[test]
    fn searches_markdown_with_context() {
        let markdown = "# Pin Functions\nPA2 is TCA0 WO2\nPA5 is TCA0 WO5\n";
        let matches = search_markdown(markdown, r"(?i)pa2|wo2", 1).unwrap();

        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].line_number, 2);
        assert_eq!(matches[0].line, "PA2 is TCA0 WO2");
        assert_eq!(matches[0].context_before, vec!["# Pin Functions"]);
        assert_eq!(matches[0].context_after, vec!["PA5 is TCA0 WO5"]);
    }

    #[test]
    fn rejects_invalid_search_pattern() {
        let error = search_markdown("text", "(", 0).unwrap_err();
        assert!(matches!(error, DatasheetError::InvalidSearchPattern { .. }));
    }
}
