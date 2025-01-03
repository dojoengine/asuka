use regex::Regex;
use reqwest::Client;
use rig::{completion::CompletionModel, extractor::ExtractorBuilder};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf};
use thiserror::Error;
use tracing::debug;
use url::Url;

#[derive(Error, Debug)]
pub enum SiteLoaderError {
    #[error("Request error: {0}")]
    RequestError(String),

    #[error("URL parse error: {0}")]
    UrlError(#[from] url::ParseError),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
}

impl From<reqwest::Error> for SiteLoaderError {
    fn from(err: reqwest::Error) -> Self {
        Self::RequestError(err.to_string())
    }
}

#[derive(Debug, Deserialize, JsonSchema, Serialize)]
/// A record containing extracted topics
pub struct Content {
    /// The content extracted from the text
    pub content: String,
}

pub struct SiteLoader<M: CompletionModel> {
    url: Url,
    client: Client,
    model: M,
    base_path: PathBuf,
}

impl<M: CompletionModel> SiteLoader<M> {
    pub fn new(url: String, model: M) -> Result<Self, SiteLoaderError> {
        let url = Url::parse(&url)?;
        let base_path = PathBuf::from(".sources/sites");
        Ok(Self {
            url,
            client: Client::new(),
            model,
            base_path,
        })
    }

    fn get_site_dir(&self) -> PathBuf {
        let host = self.url.host_str().unwrap_or("unknown");
        let path = self.url.path().trim_matches('/');
        self.base_path.join(host).join(path)
    }

    pub async fn extract_content(&self) -> Result<String, SiteLoaderError> {
        let site_dir = self.get_site_dir();
        let html_path = site_dir.join("index.html");
        let content_path = site_dir.join("content.txt");

        // If content already exists, return it
        // if content_path.exists() {
        //     info!(path = ?content_path, "Content file exists, using cached version");
        //     return Ok(fs::read_to_string(content_path)?);
        // }

        debug!(url = %self.url, "Fetching and extracting site content");

        // Create the directory structure
        fs::create_dir_all(&site_dir)?;

        // Fetch and save HTML
        let response = self.client.get(self.url.clone()).send().await?;
        let html = response.text().await?;

        // Extract just the body content first
        let body_content = if let Some(start) = html.find("<body") {
            if let Some(end) = html[start..].find("</body>") {
                &html[start..start + end + 7]
            } else {
                &html
            }
        } else {
            &html
        };

        // Basic preprocessing to remove common non-content elements from body
        let script_re = Regex::new(r"<script[^>]*>[\s\S]*?</script>").unwrap();
        let style_re = Regex::new(r"<style[^>]*>[\s\S]*?</style>").unwrap();
        let tag_re = Regex::new(r"<[^>]+>").unwrap();

        let html = script_re.replace_all(body_content, "");
        let html = style_re.replace_all(&html, "");
        let html = tag_re.replace_all(&html, " ");

        let html = html
            .replace("&nbsp;", " ")
            .replace("&amp;", "&")
            .replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace(r#"\s{2,}"#, " ")
            .trim()
            .to_string();

        fs::write(&html_path, &html)?;

        let extractor = ExtractorBuilder::<Content, _>::new(self.model.clone())
            .preamble("Cleanup the content in the given text to only have the main content. Return a json data structure with a 'content' attribute set only.")
            .build();

        let content = extractor
            .extract(&html)
            .await
            .map_err(|e| SiteLoaderError::RequestError(format!("Extraction failed: {}", e)))?;

        // Save the extracted content
        fs::write(&content_path, &content.content)?;

        Ok(content.content)
    }
}
