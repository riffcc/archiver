use anyhow::{anyhow, Result};
use reqwest::Client;
use serde::Deserialize;

const ARCHIVE_API_URL: &str = "https://archive.org/advancedsearch.php";

#[derive(Deserialize, Debug)]
struct ArchiveSearchResponse {
    response: ResponseContent,
}

#[derive(Deserialize, Debug)]
struct ResponseContent {
    numFound: usize,
    start: usize,
    docs: Vec<ArchiveDoc>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct ArchiveDoc {
    pub identifier: String,
    // Add other fields you might need, e.g., title, description
    // pub title: Option<String>,
}

/// Fetches item identifiers for a given collection name from Archive.org.
///
/// Uses the advanced search API.
pub async fn fetch_collection_items(
    client: &Client,
    collection_name: &str,
    rows: usize, // Number of results per page
    page: usize, // Page number (1-based)
) -> Result<Vec<ArchiveDoc>> {
    let query = format!("collection:{}", collection_name);
    // let start = (page - 1) * rows; // API uses 0-based start index - This is unused as we use 'page' param

    let response = client
        .get(ARCHIVE_API_URL)
        .query(&[
            ("q", query.as_str()),
            ("fl[]", "identifier"), // Request only the identifier field
            // Add other fields to fl[] if needed later, e.g., "title"
            ("rows", &rows.to_string()),
            ("page", &page.to_string()), // Note: API might use 'start' instead of 'page' depending on endpoint version/preference
            ("output", "json"),
        ])
        .send()
        .await?;

    if !response.status().is_success() {
        return Err(anyhow!(
            "API request failed with status: {}",
            response.status()
        ));
    }

    let search_result = response.json::<ArchiveSearchResponse>().await?;

    Ok(search_result.response.docs)
}
