use anyhow::{anyhow, Context, Result};
use log::{debug, error, info, warn}; // Import log macros
use reqwest::{Client, StatusCode}; // Import StatusCode
use serde::Deserialize;
use std::{collections::HashMap, sync::Arc};
use tokio::time::{sleep, Duration as TokioDuration}; // Import sleep and Tokio Duration for retries
use crate::app::AppRateLimiter; // Use the type alias from app.rs

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
    // Changed to Value to handle potential empty array `[]` from API instead of map `{}`
    pub files: Option<serde_json::Value>,
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
    pub publicdate: Option<String>, // Changed back to String to avoid parsing errors
    pub uploader: Option<String>,
    pub collection: Option<serde_json::Value>, // Changed to Value for flexibility
    // Use HashMap for other potential metadata fields we don't explicitly define
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
    // Removed duplicate extra field below
}

/// Represents the value part of an entry in the 'files' map from the API response.
/// Represents the file details as returned within the API response (either in a map value or array element).
#[derive(Deserialize, Debug, Clone)]
pub struct FileDetailsInternal {
    // Note: 'name' is handled separately depending on whether files is Array or Map
    pub source: Option<String>, // Usually "original" or "derivative"
    pub format: Option<String>, // e.g., "JPEG", "MP3", "JSON"
    pub size: Option<String>,   // Size is often a string, parse later if needed
    pub md5: Option<String>,
    // Add other file fields if needed (e.g., length, height, width)
    #[serde(flatten)]
    pub _extra: HashMap<String, serde_json::Value>, // Prefixed with _
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

/// Specific errors that can occur during `fetch_item_details`.
#[derive(Debug)]
pub enum FetchDetailsErrorKind {
    /// Item not found (e.g., HTTP 404). Considered permanent.
    NotFound,
    /// Failed to parse the JSON response. Considered permanent.
    ParseError,
    /// Network-related error during the request (e.g., timeout, DNS). Potentially transient.
    NetworkError,
    /// Server-side error (e.g., HTTP 5xx). Potentially transient.
    ServerError(StatusCode),
    /// Client-side error other than 404 (e.g., 400, 403). Considered permanent.
    ClientError(StatusCode),
    /// Should not happen if rate limiter is working, but included for completeness. Potentially transient.
    RateLimitExceeded, // Typically HTTP 429
    /// Any other unexpected error. Potentially transient.
    Other,
}

/// Error type returned by `fetch_item_details`.
#[derive(Debug)]
pub struct FetchDetailsError {
    pub kind: FetchDetailsErrorKind,
    pub source: anyhow::Error,
    pub identifier: String, // Include identifier for context
}

impl std::fmt::Display for FetchDetailsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Failed to fetch details for '{}': {:?} - {}", self.identifier, self.kind, self.source)
    }
}

impl std::error::Error for FetchDetailsError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.source.source()
    }
}


// --- Constants ---
const ROWS_PER_PAGE: usize = 100; // Number of results to fetch per API call during pagination

// --- API Fetch Functions ---

/// Fetches item identifiers for a given collection name from Archive.org.
///
/// Uses the advanced search API.
pub async fn fetch_collection_items(
    client: &Client,
    collection_name: &str,
    rows: usize, // Number of results per page
    page: usize, // Page number (1-based)
    rate_limiter: AppRateLimiter, // Added rate limiter parameter
) -> Result<(Vec<ArchiveDoc>, usize)> {
    info!("Fetching collection items for '{}', page {}, rows {}", collection_name, page, rows);
    let query = format!("collection:{}", collection_name);
    let max_retries = 3;
    let mut last_error: Option<anyhow::Error> = None;

    for attempt in 1..=max_retries {
        debug!("Attempting to fetch collection items for '{}', page {}, attempt {}/{}", collection_name, page, attempt, max_retries);

        // --- Wait for Rate Limiter (inside retry loop) ---
        debug!("Waiting for rate limit permit for collection items: {} (page {})", collection_name, page);
        rate_limiter.until_ready().await;
        debug!("Acquired rate limit permit for collection items: {} (page {})", collection_name, page);
        // --- Rate Limit Permit Acquired ---

        // Clone the request builder inside the loop for retries
        let request_builder = client
            .get(ADVANCED_SEARCH_URL)
            .query(&[
                ("q", query.as_str()),
                ("fl[]", "identifier"), // Request only the identifier field for the list
                // Add other fields to fl[] if needed later, e.g., "title"
                ("rows", &rows.to_string()),
                ("page", &page.to_string()), // Note: API might use 'start' instead of 'page' depending on endpoint version/preference
                ("output", "json"),
            ]);

        debug!("Sending collection items request: {:?}", request_builder);

        match request_builder.try_clone() { // Need to clone the builder for potential retries
            Some(cloned_builder) => {
                match cloned_builder.send().await {
                    Ok(response) => {
                        if response.status().is_success() {
                            // Success path
                            match response.json::<ArchiveSearchResponse>().await {
                                Ok(search_result) => {
                                    info!("Successfully fetched {} items (total found: {}) for collection '{}', page {}",
                                          search_result.response.docs.len(), search_result.response.num_found, collection_name, page);
                                    return Ok((search_result.response.docs, search_result.response.num_found));
                                }
                                Err(e) => {
                                    let parse_err = anyhow!(e).context(format!(
                                        "Failed to parse JSON for collection items '{}', page {} (Attempt {}/{})",
                                        collection_name, page, attempt, max_retries
                                    ));
                                    error!("{}", parse_err);
                                    last_error = Some(parse_err);
                                    // Don't retry on parse errors
                                    break;
                                }
                            }
                        } else {
                            // Handle non-success HTTP status
                            let status = response.status();
                            let err_msg = format!(
                                "Collection items API request failed for '{}', page {} with status: {} (Attempt {}/{})",
                                collection_name, page, status, attempt, max_retries
                            );
                            error!("{}", err_msg);
                            last_error = Some(anyhow!(err_msg));

                            // Retry only on server errors (5xx) or specific transient errors if needed
                            if status.is_server_error() && attempt < max_retries {
                                let delay_secs = 1 << (attempt - 1); // 1s, 2s
                                warn!("Retrying collection items fetch in {} seconds...", delay_secs);
                                sleep(TokioDuration::from_secs(delay_secs)).await;
                                continue; // Go to next attempt
                            } else {
                                // Don't retry for client errors (4xx) or after max retries
                                break;
                            }
                        }
                    }
                    Err(e) => {
                        // Handle request sending errors (network, timeout, etc.)
                        let current_err = anyhow!(e).context(format!(
                            "Failed to send collection items request for '{}', page {} (Attempt {}/{})",
                            collection_name, page, attempt, max_retries
                        ));
                        error!("{}", current_err);
                        last_error = Some(current_err);

                        if attempt < max_retries {
                            let delay_secs = 1 << (attempt - 1); // 1s, 2s
                            warn!("Retrying collection items fetch in {} seconds...", delay_secs);
                            sleep(TokioDuration::from_secs(delay_secs)).await;
                            continue; // Go to next attempt
                        } else {
                            break; // Max retries reached
                        }
                    }
                }
            }
            None => {
                // Should not happen with standard reqwest builders
                let build_err = anyhow!("Failed to clone request builder for collection items '{}', page {}", collection_name, page);
                error!("{}", build_err);
                last_error = Some(build_err);
                break; // Cannot retry if builder cannot be cloned
            }
        }
    } // End retry loop

    // If loop finished without returning Ok, return the last error
    Err(last_error.unwrap_or_else(|| anyhow!("Collection items request failed after {} attempts for '{}', page {}", max_retries, collection_name, page)))
}


/// Fetches detailed metadata and file list for a given item identifier, with retries.
/// Fetches detailed metadata and file list for a given item identifier.
/// Returns `FetchDetailsError` on failure, classifying the error type.
pub async fn fetch_item_details(
    client: &Client,
    identifier: &str,
    rate_limiter: AppRateLimiter, // Added rate limiter parameter
) -> Result<ItemDetails, FetchDetailsError> { // Changed return type
    info!("Fetching item details for identifier: {}", identifier);
    let url = format!("{}{}", METADATA_URL_BASE, identifier);

    // --- Wait for Rate Limiter ---
    debug!("Waiting for rate limit permit for item details: {}", identifier);
    rate_limiter.until_ready().await;
    debug!("Acquired rate limit permit for item details: {}", identifier);
    // --- Rate Limit Permit Acquired ---

    debug!("Requesting item details from URL: {}", url);
    let response_result = client.get(&url).send().await;

    match response_result {
        Ok(response) => {
            let status = response.status();
            if !status.is_success() {
                // Classify HTTP errors
                let kind = match status {
                    StatusCode::NOT_FOUND => FetchDetailsErrorKind::NotFound,
                    StatusCode::TOO_MANY_REQUESTS => FetchDetailsErrorKind::RateLimitExceeded,
                    s if s.is_client_error() => FetchDetailsErrorKind::ClientError(s),
                    s if s.is_server_error() => FetchDetailsErrorKind::ServerError(s),
                    _ => FetchDetailsErrorKind::Other, // Should not happen often
                };
                let err = anyhow!("Metadata API request failed with status: {}", status);
                warn!("{} for identifier '{}'", err, identifier); // Log warning for non-success
                // Return specific error, even if we attempt parsing later for some cases (like 404)
                // For 404, we might still get an empty JSON, but we treat 404 itself as the primary error.
                 return Err(FetchDetailsError {
                    kind,
                    source: err,
                    identifier: identifier.to_string(),
                });
            }

            // Attempt to parse the successful response
            match response.json::<ItemMetadataResponse>().await {
                Ok(raw_details) => {
                    // --- Start of existing processing logic ---
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

                // Helper function to extract a string array from a Value (string or array)
                let get_string_array = |v: &Option<serde_json::Value>| -> Vec<String> {
                    match v {
                        Some(serde_json::Value::String(s)) => vec![s.clone()], // Single string becomes a vec
                        Some(serde_json::Value::Array(arr)) => arr
                            .iter()
                            .filter_map(|val| val.as_str().map(String::from))
                            .collect(),
                        _ => Vec::new(), // Otherwise, return empty vec
                    }
                };


                // Process into our ItemDetails struct
                // Handle Option<MetadataDetails> explicitly instead of unwrap_or_default
                let (title, creator, description, date, uploader, collections) =
                    if let Some(metadata) = &raw_details.metadata {
                         (
                            get_first_string(&metadata.title),
                            get_first_string(&metadata.creator),
                            get_first_string(&metadata.description),
                            metadata.date.clone(), // Clone the Option<String>
                            metadata.uploader.clone(), // Clone the Option<String>
                            get_string_array(&metadata.collection), // Use helper for collection
                        )
                    } else {
                        // If metadata object is missing entirely, return None/empty values
                        (None, None, None, None, None, Vec::new())
                    };

                let download_base_url = match (raw_details.server, raw_details.dir) {
                    (Some(server), Some(dir)) => Some(format!("https://{}/{}", server, dir)),
                    _ => None, // Add default case
                }; // Add closing semicolon

                // Ensure the identifier in the returned struct matches the one requested.
                // Use the variables extracted earlier.
                let details = ItemDetails {
                    identifier: identifier.to_string(), // Use the function argument identifier
                    title,                              // Use processed value
                    creator,                            // Use processed value
                    description,                        // Use processed value
                    date,                               // Use processed value
                    uploader,                           // Use processed value
                    collections,                        // Use processed value
                    files: match raw_details.files {
                        // Handle the case where 'files' is a JSON Array
                        Some(serde_json::Value::Array(files_array)) => {
                            files_array
                                .into_iter()
                                .filter_map(|value| {
                                    // Attempt to deserialize each element in the array into FileDetailsInternal
                                    // We also need the 'name' field from within the object now.
                                    #[derive(Deserialize)]
                                    struct FileWithName {
                                        name: String,
                                        #[serde(flatten)]
                                        details: FileDetailsInternal,
                                    }

                                    match serde_json::from_value::<FileWithName>(value) {
                                        Ok(file_with_name) => Some(FileDetails {
                                            name: file_with_name.name, // Get name from the parsed struct
                                            source: file_with_name.details.source,
                                            format: file_with_name.details.format,
                                            size: file_with_name.details.size,
                                            md5: file_with_name.details.md5,
                                        }),
                                        Err(_) => None, // Skip files that don't match the expected structure
                                    }
                                })
                                .collect()
                        }
                        // Handle the (less likely?) case where 'files' is a JSON object (Map)
                        Some(serde_json::Value::Object(files_map)) => {
                             files_map
                                .into_iter()
                                .filter_map(|(name, value)| {
                                    // Attempt to deserialize each value in the map into FileDetailsInternal
                                    match serde_json::from_value::<FileDetailsInternal>(value) {
                                        Ok(internal_details) => Some(FileDetails {
                                            // Use the map key as the name
                                            name: name.strip_prefix('/').unwrap_or(&name).to_string(),
                                            source: internal_details.source,
                                            format: internal_details.format,
                                            size: internal_details.size,
                                            md5: internal_details.md5,
                                        }),
                                        Err(_) => None, // Skip files that don't match the expected structure
                                    }
                                })
                                .collect()
                        }
                        // If 'files' is None, Null, or some other unexpected type, return empty vec
                        _ => Vec::new(),
                    },
                    download_base_url,
                };

                info!("Successfully processed item details for identifier: {}", identifier);
                    info!("Successfully processed item details for identifier: {}", identifier);
                    Ok(details) // Success, return the processed details
                    // --- End of existing processing logic ---
                }
                Err(e) => {
                    // Failed to parse JSON even from a successful HTTP response
                    let err = anyhow!(e).context("Failed to parse JSON response for item details");
                    error!("{} for identifier '{}'", err, identifier);
                    Err(FetchDetailsError {
                        kind: FetchDetailsErrorKind::ParseError,
                        source: err,
                        identifier: identifier.to_string(),
                    })
                }
            }
        }
        Err(e) => {
            // Error sending the request (network issue, timeout, etc.)
            // Extract info from 'e' *before* moving it into anyhow!
            let is_timeout = e.is_timeout();
            let is_connect_or_request = e.is_connect() || e.is_request();

            // Move 'e' into anyhow! now
            let err = anyhow!(e).context("Failed to send item details request");
            error!("{} for identifier '{}'", err, identifier); // Log the error created from 'e'

            // Classify network errors using the extracted info
            let kind = if is_timeout {
                FetchDetailsErrorKind::NetworkError // Specifically timeout
            } else if is_connect_or_request {
                 FetchDetailsErrorKind::NetworkError // Other connection/request errors
            } else {
                 FetchDetailsErrorKind::Other // Other reqwest errors
            };

             Err(FetchDetailsError {
                kind,
                source: err, // Use the anyhow error created above
                identifier: identifier.to_string(),
            })
        }
    }
}


/// Fetches ALL item identifiers for a given collection using pagination.
pub async fn fetch_all_collection_identifiers(
    client: &Client,
    collection_name: &str,
    rate_limiter: AppRateLimiter, // Added rate limiter parameter
) -> Result<Vec<String>> {
    info!("Fetching all identifiers for collection: {}", collection_name);
    let mut all_identifiers = Vec::new();
    let mut current_page = 1;
    let mut total_found = 0;
    let mut fetched_count = 0;

   loop {
       // Clone limiter for each page fetch
       let limiter_clone = Arc::clone(&rate_limiter);
       let (docs, page_total_found) = fetch_collection_items(
           client,
           collection_name,
           ROWS_PER_PAGE,
           current_page,
           limiter_clone, // Pass limiter
       )
       .await
       .context(format!(
            "Failed to fetch page {} for collection '{}'",
            current_page, collection_name
        ))?;

        debug!("Fetched page {} for collection '{}'. Found {} docs on page, total reported: {}",
               current_page, collection_name, docs.len(), page_total_found);

        // Set total_found on the first successful page fetch
        if current_page == 1 {
            total_found = page_total_found;
            info!("Total items reported for collection '{}': {}", collection_name, total_found);
        } else if total_found != page_total_found {
            warn!(
                "Total items found changed between page 1 ({}) and page {} ({}) for collection '{}'. Using first page total.",
                total_found, current_page, page_total_found, collection_name
            );
            // Continue using the total_found from the first page.
        }

        let num_docs_on_page = docs.len();
        fetched_count += num_docs_on_page;

        // Add identifiers from the current page
        all_identifiers.extend(docs.into_iter().map(|doc| doc.identifier));

        // Check termination conditions:
        // 1. If total_found is 0, stop immediately.
        // 2. If the number of docs received on this page is less than requested, it must be the last page.
        // 3. If we have fetched at least as many items as the total reported, we are done.
        if total_found == 0 || num_docs_on_page < ROWS_PER_PAGE || fetched_count >= total_found {
            break;
        }

        // Prepare for the next page
        current_page += 1;

        // Optional: Add a small delay between requests to be polite to the API
        // tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }

    info!("Finished fetching identifiers for collection '{}'. Found {} identifiers.", collection_name, fetched_count);
    if total_found > 0 && fetched_count != total_found {
        warn!("Fetched identifier count ({}) does not match reported total ({}) for collection '{}'.", fetched_count, total_found, collection_name);
    }

    Ok(all_identifiers)
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::AppRateLimiter; // Use the type alias
    use governor::{Quota, RateLimiter, clock::SystemClock};
    use reqwest::Client;
    use std::{error::Error, sync::Arc, time::Duration, num::NonZeroU32}; // Import std::error::Error
    use tokio;

    // Helper function to create a client with timeouts for tests
    fn test_client() -> Client {
        Client::builder()
            .timeout(Duration::from_secs(60)) // Use a longer timeout for tests (e.g., 60s)
            .connect_timeout(Duration::from_secs(60))
            .build()
            .expect("Failed to build test client")
    }

    // Helper function to create a rate limiter for integration tests (respects API limits)
    fn test_limiter() -> AppRateLimiter {
        // Use the actual 15 requests per minute quota for integration tests
        let quota = Quota::per_minute(NonZeroU32::new(15).unwrap());
        // Use direct_with_clock and SystemClock to match the AppRateLimiter type alias
        Arc::new(RateLimiter::direct_with_clock(quota, &SystemClock::default()))
    }

    // --- Integration Tests (require network access to archive.org) ---

    // --- fetch_collection_items tests ---
    #[tokio::test]
    #[ignore] // Ignored by default, run with `cargo test -- --ignored`
    async fn test_fetch_collection_items_integration_success() {
        // Arrange
        let client = test_client(); // Use helper function
        let collection_name = "nasa"; // A known, large collection
        let rows = 5; // Fetch a small number of rows
        let page = 1;
        let limiter = test_limiter(); // Create dummy limiter

        // Act
        let result = fetch_collection_items(&client, collection_name, rows, page, Arc::clone(&limiter)).await;

        // Assert
        assert!(result.is_ok(), "API call should succeed");
        let (items, total_found) = result.unwrap(); // Destructure the tuple
        assert!(total_found > 0, "Total found should be greater than 0 for 'nasa'");
        assert!(!items.is_empty(), "Should return some items for 'nasa'");
        assert!(items.len() <= rows, "Should return at most 'rows' items"); // API might return fewer than requested if less available
        assert!(items.iter().all(|doc| !doc.identifier.is_empty()), "All items should have an identifier");

        // Fetch page 2 and check if identifiers are different (basic pagination check)
        let page2_result = fetch_collection_items(&client, collection_name, rows, page + 1, Arc::clone(&limiter)).await;
        assert!(page2_result.is_ok(), "API call for page 2 should succeed");
        let (page2_items, page2_total_found) = page2_result.unwrap(); // Destructure page 2 result
        assert_eq!(total_found, page2_total_found, "Total found should be consistent across pages");
        assert!(!page2_items.is_empty(), "Page 2 should also return items for 'nasa'");
        if !items.is_empty() && !page2_items.is_empty() {
             assert_ne!(items[0].identifier, page2_items[0].identifier, "First item of page 1 and page 2 should differ");
        }
    }

    #[tokio::test]
    #[ignore] // Ignored by default, run with `cargo test -- --ignored`
    async fn test_fetch_collection_items_integration_not_found() {
        // Arrange
        let client = test_client(); // Use helper function
        // Use a collection name highly unlikely to exist
        let collection_name = "this_collection_should_really_not_exist_12345";
        let rows = 10;
        let page = 1;
        let limiter = test_limiter(); // Create dummy limiter

        // Act
        let result = fetch_collection_items(&client, collection_name, rows, page, limiter).await;

        // Assert
        assert!(result.is_ok(), "API call should still succeed even if no items are found");
        let (items, total_found) = result.unwrap(); // Destructure
        assert_eq!(total_found, 0, "Total found should be 0 for non-existent collection");
        assert!(items.is_empty(), "Should return no items for a non-existent collection");
    }

     #[tokio::test]
    #[ignore] // Ignored by default, run with `cargo test -- --ignored`
    async fn test_fetch_collection_items_integration_invalid_chars() {
        // Arrange
        let client = test_client(); // Use helper function
        // Archive.org might handle this gracefully, but good to test
        let collection_name = "invalid collection name with spaces";
        let rows = 10;
        let page = 1;
        let limiter = test_limiter(); // Create dummy limiter

        // Act
        let result = fetch_collection_items(&client, collection_name, rows, page, limiter).await;

        // Assert
        // We expect the API call to succeed but return no results for such a name
        assert!(result.is_ok(), "API call should succeed");
        let (items, total_found) = result.unwrap(); // Destructure
        assert_eq!(total_found, 0, "Total found should be 0 for invalid collection name");
        assert!(items.is_empty(), "Should return no items for an invalid collection name format");
    }

    // --- fetch_item_details tests ---

    #[tokio::test]
    #[ignore]
    async fn test_fetch_item_details_integration_success() {
        // Arrange
        let client = test_client(); // Use helper function
        // Using the item provided by the user
        let identifier = "enrmp270_litmus_-_perception_of_light";
        let limiter = test_limiter(); // Create dummy limiter

        let limiter = test_limiter(); // Create dummy limiter

        // Act
        let result = fetch_item_details(&client, identifier, limiter).await;

        // Assert
        if let Err(ref e) = result {
             eprintln!("Fetch details error: {}", e); // Print error details if it fails
             // Call source() directly on 'e' (which is &FetchDetailsError)
             // Requires std::error::Error trait to be in scope
             if let Some(source) = e.source() {
                 eprintln!("Source: {}", source);
             }
        }
        assert!(result.is_ok(), "API call should succeed");
        let details = result.unwrap();

        assert_eq!(details.identifier, identifier);
        assert_eq!(details.title.is_some(), true, "Title should be present for item '{}'. Details: {:?}", identifier, details);
        assert_eq!(details.title.as_deref(), Some("Litmus - Perception Of Light [enrmp270]"), "Title mismatch");
        assert!(details.creator.is_some(), "Should have a creator: {:?}", details.creator);
        assert_eq!(details.creator.as_deref(), Some("Litmus"), "Creator mismatch");
        assert!(details.date.is_some(), "Should have a date: {:?}", details.date);
        assert!(!details.collections.is_empty(), "Should belong to collections: {:?}", details.collections);
        assert!(details.collections.contains(&"enough_records".to_string()), "Should be in 'enough_records' collection");
        assert!(!details.files.is_empty(), "Should have files");
        assert!(details.download_base_url.is_some(), "Should have download base URL");

        // Check a specific file format (example)
        assert!(details.files.iter().any(|f| f.format == Some("VBR MP3".to_string())), "Should contain a VBR MP3 file format");
        assert!(details.files.iter().any(|f| f.name.ends_with(".mp3")), "Should contain a file ending with .mp3");
    }

     #[tokio::test]
    #[ignore]
    async fn test_fetch_item_details_integration_not_found() {
        // Arrange
        let client = test_client(); // Use helper function
        let identifier = "this_item_definitely_does_not_exist_98765";
        let limiter = test_limiter(); // Create dummy limiter

        // Act
        let result = fetch_item_details(&client, identifier, limiter).await;

        // Assert
        // The metadata API should now return a specific error for 404.
        assert!(result.is_err(), "API call should fail for non-existent item");
        let err = result.unwrap_err();
        assert!(matches!(err.kind, FetchDetailsErrorKind::NotFound), "Error kind should be NotFound");
        assert_eq!(err.identifier, identifier, "Error should contain the correct identifier");
    }

    // Removed test_fetch_item_details_integration_minimal_metadata as it used an invalid identifier

    #[tokio::test]
    #[ignore]
    async fn test_fetch_collection_items_total_found_nasa() {
        // Arrange
        let client = test_client(); // Use helper function
        let collection_name = "nasa";
        let rows = 1; // Only need 1 row to get the total count
        let page = 1;
        let limiter = test_limiter(); // Create dummy limiter

        // Act
        let result = fetch_collection_items(&client, collection_name, rows, page, limiter).await;

        // Assert
        assert!(result.is_ok(), "API call should succeed");
        let (_items, total_found) = result.unwrap();
        assert!(total_found > 1000, "NASA collection should have many items (found {})", total_found); // Check for a reasonably large number
    }

    #[tokio::test]
    #[ignore]
    async fn test_fetch_collection_items_total_found_nonexistent() {
        // Arrange
        let client = test_client(); // Use helper function
        let collection_name = "this_collection_definitely_does_not_exist_1234567890";
        let rows = 1;
        let page = 1;
        let limiter = test_limiter(); // Create dummy limiter

        // Act
        let result = fetch_collection_items(&client, collection_name, rows, page, limiter).await;

        // Assert
        assert!(result.is_ok(), "API call should succeed even for non-existent collection");
        let (_items, total_found) = result.unwrap();
        assert_eq!(total_found, 0, "Total found should be 0 for non-existent collection");
    }
}

// Removed default implementation for MetadataDetails
