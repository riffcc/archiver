use anyhow::{anyhow, Context, Result};
use chrono::{DateTime, Utc}; // Import chrono types
use reqwest::Client;
use serde::Deserialize;
use std::collections::HashMap; // For handling arbitrary metadata fields

const ADVANCED_SEARCH_URL: &str = "https://archive.org/advancedsearch.php";
const METADATA_URL_BASE: &str = "https://archive.org/metadata/";

#[derive(Deserialize, Debug)]
struct ArchiveSearchResponse {
    response: ResponseContent,
}

#[derive(Deserialize, Debug)]
#[allow(dead_code)] // Allow unused fields for now (num_found, start)
struct ResponseContent {
    #[serde(rename = "numFound")] // Map JSON field to snake_case
    num_found: usize,
    start: usize,
    docs: Vec<ArchiveDoc>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct ArchiveDoc {
    pub identifier: String,
    // Add other fields you might need, e.g., title, description
    // pub title: Option<String>,
    // Consider adding other useful fields like 'title' if needed for the list view
}

// --- Structs for Item Metadata Endpoint (metadata/{identifier}) ---

/// Represents the overall structure of the response from the metadata endpoint.
#[derive(Deserialize, Debug, Clone)]
pub struct ItemMetadataResponse {
    pub metadata: Option<MetadataDetails>,
    // Changed from Vec<FileDetails> to HashMap<String, FileDetailsInternal>
    pub files: Option<HashMap<String, FileDetailsInternal>>,
    pub server: Option<String>, // Server hosting the files
    pub dir: Option<String>,    // Directory path on the server
    // Add other top-level fields if needed (e.g., reviews, related)
}

/// Represents the 'metadata' object within the response.
#[derive(Deserialize, Debug, Clone)]
pub struct MetadataDetails {
    pub identifier: String,
    // Use Value to handle string-or-array cases for common fields
    pub title: Option<serde_json::Value>,
    pub creator: Option<serde_json::Value>,
    pub description: Option<serde_json::Value>,
    pub date: Option<String>, // Date can be in various formats, parse later
    pub publicdate: Option<DateTime<Utc>>, // Already parsed if in standard format
    pub uploader: Option<String>,
    pub collection: Option<Vec<String>>, // Collection can be an array
    // Use HashMap for other potential metadata fields we don't explicitly define
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
    // Removed duplicate extra field below
}

/// Represents the value part of an entry in the 'files' map from the API response.
/// Note: The filename itself is the key in the map.
#[derive(Deserialize, Debug, Clone)]
struct FileDetailsInternal {
    pub source: Option<String>, // Usually "original" or "derivative"
    pub format: Option<String>, // e.g., "JPEG", "MP3", "JSON"
    pub size: Option<String>,   // Size is often a string, parse later if needed
    pub md5: Option<String>,
    // Add other file fields if needed (e.g., length, height, width)
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

/// Final structure representing a file, used within ItemDetails.
/// This is constructed manually, not directly deserialized.
#[derive(Debug, Clone, Default)]
pub struct FileDetails {
    pub name: String, // The actual filename
    pub source: Option<String>,
    pub format: Option<String>,
    pub size: Option<String>,
    pub md5: Option<String>,
}


/// A processed structure holding the relevant details for display.
#[derive(Debug, Clone, Default)]
pub struct ItemDetails {
    pub identifier: String,
    pub title: Option<String>,
    pub creator: Option<String>,
    pub description: Option<String>,
    pub date: Option<String>, // Keep as string for now due to format variety
    pub uploader: Option<String>,
    pub collections: Vec<String>,
    pub files: Vec<FileDetails>, // Store the list of files
    pub download_base_url: Option<String>, // Constructed base URL for downloads
}


// --- API Fetch Functions ---

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
    // let start = (page - 1) * rows; // API uses 0-based start index

    let response = client
        .get(ADVANCED_SEARCH_URL)
        .query(&[
            ("q", query.as_str()),
            ("fl[]", "identifier"), // Request only the identifier field for the list
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

/// Fetches detailed metadata and file list for a given item identifier.
pub async fn fetch_item_details(client: &Client, identifier: &str) -> Result<ItemDetails> {
    let url = format!("{}{}", METADATA_URL_BASE, identifier);

    let response = client.get(&url).send().await?;

    if !response.status().is_success() {
        return Err(anyhow!(
            "Metadata API request failed for '{}' with status: {}",
            identifier,
            response.status()
        ));
    }

    let raw_details = response
        .json::<ItemMetadataResponse>()
        .await
        .context(format!("Failed to parse JSON for item '{}'", identifier))?;

    // Process into our ItemDetails struct
    let metadata = raw_details.metadata.unwrap_or_default(); // Provide default if metadata is missing

    let download_base_url = match (raw_details.server, raw_details.dir) {
        (Some(server), Some(dir)) => Some(format!("https://{}/{}", server, dir)),
        _ => None,
    };

    // Helper function to extract the first string from a Value (string or array)
    let get_first_string = |v: &Option<serde_json::Value>| -> Option<String> {
        match v {
            Some(serde_json::Value::String(s)) => Some(s.clone()),
            Some(serde_json::Value::Array(arr)) => arr
                .get(0)
                .and_then(|first| first.as_str())
                .map(String::from),
            _ => None,
        }
    };

    // Ensure the identifier in the returned struct matches the one requested,
    // regardless of what the metadata field in the response contains.
    let details = ItemDetails {
        identifier: identifier.to_string(), // Use the function argument identifier
        title: get_first_string(&metadata.title),
        creator: get_first_string(&metadata.creator),
        description: get_first_string(&metadata.description),
        date: metadata.date, // Keep raw date string for now
        uploader: metadata.uploader,
        collections: metadata.collection.unwrap_or_default(),
        files: raw_details
            .files
            .map(|files_map| {
                files_map
                    .into_iter()
                    .map(|(name, internal_details)| FileDetails {
                        // Remove leading '/' from filename if present (common in API)
                        name: name.strip_prefix('/').unwrap_or(&name).to_string(),
                        source: internal_details.source,
                        format: internal_details.format,
                        size: internal_details.size,
                        md5: internal_details.md5,
                    })
                    .collect()
            })
            .unwrap_or_default(), // Use empty vec if files field is missing or None
        download_base_url,
    };

    Ok(details)
}


#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::Client;
    use tokio;

    // --- Integration Tests (require network access to archive.org) ---

    // --- fetch_collection_items tests ---
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

    // --- fetch_item_details tests ---

    #[tokio::test]
    #[ignore]
    async fn test_fetch_item_details_integration_success() {
        // Arrange
        let client = Client::new();
        let identifier = "IsaacAsimov-TheFunTheyHad"; // A known item

        // Act
        let result = fetch_item_details(&client, identifier).await;

        // Assert
        assert!(result.is_ok(), "API call should succeed: {:?}", result.err());
        let details = result.unwrap();

        assert_eq!(details.identifier, identifier);
        // Use assert_eq! with a more informative message if title is None
        assert_eq!(details.title.is_some(), true, "Title should be present for item '{}'. Details: {:?}", identifier, details);
        assert!(details.creator.is_some(), "Should have a creator");
        assert!(details.date.is_some(), "Should have a date");
        assert!(!details.files.is_empty(), "Should have files");
        assert!(details.download_base_url.is_some(), "Should have download base URL");

        // Check a specific file format (example)
        assert!(details.files.iter().any(|f| f.format == Some("MP3".to_string())), "Should contain an MP3 file");
        assert!(details.files.iter().any(|f| f.name.ends_with(".mp3")), "Should contain a file ending with .mp3");
    }

     #[tokio::test]
    #[ignore]
    async fn test_fetch_item_details_integration_not_found() {
        // Arrange
        let client = Client::new();
        let identifier = "this_item_definitely_does_not_exist_98765";

        // Act
        let result = fetch_item_details(&client, identifier).await;

        // Assert
        // The metadata API often returns 200 OK with empty/null data for non-existent items.
        // So, we expect Ok, but the details should reflect that it wasn't really found.
        assert!(result.is_ok(), "API call should succeed even for non-existent item, returning empty data");
        let details = result.unwrap();
        assert_eq!(details.identifier, identifier, "Identifier should match the request");
        // A key indicator of a non-found item is often a missing title or empty files list.
        assert!(details.title.is_none(), "Non-existent item should not have a title");
        assert!(details.files.is_empty(), "Non-existent item should have no files");
    }

     #[tokio::test]
    #[ignore]
    async fn test_fetch_item_details_integration_minimal_metadata() {
         // Arrange
        let client = Client::new();
         // Find an item known to have minimal metadata if possible, or use a test item
         // For now, using a known good item and checking defaults isn't ideal but demonstrates structure handling
        let identifier = "gd1967-xx-xx.sbd.studio.81178.flac16"; // Example item

        // Act
        let result = fetch_item_details(&client, identifier).await;

        // Assert
        assert!(result.is_ok(), "API call should succeed: {:?}", result.err());
        let details = result.unwrap();
        assert_eq!(details.identifier, identifier);
        // Check that even if some fields were None in JSON, the call succeeds.
        // We don't strictly need to assert title.is_some() for this specific item in a "minimal" test.
        // The main point is that parsing didn't fail.
        // We can still check that files were parsed if they exist for this item.
        assert!(!details.files.is_empty(), "File list should be parsed for item '{}'. Details: {:?}", identifier, details);
        // Other fields might be None, which is okay if the Option reflects that
    }
}

// Add default implementation for MetadataDetails for cleaner error handling
impl Default for MetadataDetails {
    fn default() -> Self {
        Self {
            identifier: String::new(), // Default identifier is empty
            title: None,
            creator: None,
            description: None,
            date: None,
            publicdate: None,
            uploader: None,
            collection: None,
            extra: HashMap::new(),
        }
    }
}
