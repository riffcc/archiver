use serde::{Deserialize, Serialize};
use std::error::Error;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchiveOrgItem {
    pub identifier: String,
    pub title: Option<String>,
    pub creator: Option<String>,
    pub date: Option<String>,
    pub mediatype: Option<String>,
    pub downloads: Option<i32>,
    pub item_size: Option<i64>,
    pub publicdate: Option<String>,
    pub description: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct SearchResponse {
    pub response: SearchResponseInner,
}

#[derive(Debug, Deserialize)]
pub struct SearchResponseInner {
    pub docs: Vec<ArchiveOrgItem>,
    pub numFound: i64,
    pub start: i64,
}

pub struct ArchiveOrgClient {
    client: reqwest::Client,
    base_url: String,
}

impl ArchiveOrgClient {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
            base_url: "https://archive.org".to_string(),
        }
    }

    pub async fn search_audio_collections(&self, page: usize, per_page: usize) -> Result<SearchResponse, Box<dyn Error>> {
        let url = format!(
            "{}/advancedsearch.php?q=mediatype:audio+AND+collection:*&fl=identifier,title,creator,date,mediatype,downloads,item_size,publicdate,description&output=json&rows={}&page={}",
            self.base_url, per_page, page
        );
        
        let response = self.client.get(&url).send().await?;
        let search_response: SearchResponse = response.json().await?;
        
        Ok(search_response)
    }

    pub async fn get_item_metadata(&self, identifier: &str) -> Result<serde_json::Value, Box<dyn Error>> {
        let url = format!("{}/metadata/{}", self.base_url, identifier);
        let response = self.client.get(&url).send().await?;
        let metadata = response.json().await?;
        Ok(metadata)
    }

    pub fn get_thumbnail_url(&self, identifier: &str) -> String {
        format!("{}/services/img/{}", self.base_url, identifier)
    }

    pub async fn download_thumbnail(&self, identifier: &str) -> Result<Vec<u8>, Box<dyn Error>> {
        let url = self.get_thumbnail_url(identifier);
        let response = self.client.get(&url).send().await?;
        let bytes = response.bytes().await?;
        Ok(bytes.to_vec())
    }
}