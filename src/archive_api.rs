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


#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::Client;
    use tokio;

    // --- Integration Tests (require network access to archive.org) ---

    #[tokio::test]
    #[ignore] // Ignored by default, run with `cargo test -- --ignored`
    async fn test_fetch_collection_items_integration_success() {
        // Arrange
        let client = Client::new();
        let collection_name = "nasa"; // A known, large collection
        let rows = 5; // Fetch a small number of rows
        let page = 1;

        // Act
        let result = fetch_collection_items(&client, collection_name, rows, page).await;

        // Assert
        assert!(result.is_ok(), "API call should succeed");
        let items = result.unwrap();
        assert!(!items.is_empty(), "Should return some items for 'nasa'");
        assert!(items.len() <= rows, "Should return at most 'rows' items"); // API might return fewer than requested if less available
        assert!(items.iter().all(|doc| !doc.identifier.is_empty()), "All items should have an identifier");

        // Fetch page 2 and check if identifiers are different (basic pagination check)
        let page2_result = fetch_collection_items(&client, collection_name, rows, page + 1).await;
        assert!(page2_result.is_ok(), "API call for page 2 should succeed");
        let page2_items = page2_result.unwrap();
        assert!(!page2_items.is_empty(), "Page 2 should also return items for 'nasa'");
        if !items.is_empty() && !page2_items.is_empty() {
             assert_ne!(items[0].identifier, page2_items[0].identifier, "First item of page 1 and page 2 should differ");
        }
    }

    #[tokio::test]
    #[ignore] // Ignored by default, run with `cargo test -- --ignored`
    async fn test_fetch_collection_items_integration_not_found() {
        // Arrange
        let client = Client::new();
        // Use a collection name highly unlikely to exist
        let collection_name = "this_collection_should_really_not_exist_12345";
        let rows = 10;
        let page = 1;

        // Act
        let result = fetch_collection_items(&client, collection_name, rows, page).await;

        // Assert
        assert!(result.is_ok(), "API call should still succeed even if no items are found");
        let items = result.unwrap();
        assert!(items.is_empty(), "Should return no items for a non-existent collection");
    }

     #[tokio::test]
    #[ignore] // Ignored by default, run with `cargo test -- --ignored`
    async fn test_fetch_collection_items_integration_invalid_chars() {
        // Arrange
        let client = Client::new();
        // Archive.org might handle this gracefully, but good to test
        let collection_name = "invalid collection name with spaces";
        let rows = 10;
        let page = 1;

        // Act
        let result = fetch_collection_items(&client, collection_name, rows, page).await;

        // Assert
        // We expect the API call to succeed but return no results for such a name
        assert!(result.is_ok(), "API call should succeed");
        let items = result.unwrap();
        assert!(items.is_empty(), "Should return no items for an invalid collection name format");
    }
}
