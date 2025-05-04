use anyhow::{anyhow, Result}; // Removed unused Context
use log::{debug, error, info, warn}; // Import log macros
use reqwest::{Client, StatusCode}; // Import StatusCode
use serde::{Deserialize, Serialize}; // Added Serialize
use std::collections::HashMap; // Removed unused sync::Arc
// Removed mpsc import as FetchAllResult is removed
use tokio::time::{sleep, Duration as TokioDuration}; // Import sleep and Tokio Duration for retries
use crate::app::AppRateLimiter; // Use the type alias from app.rs

const ADVANCED_SEARCH_URL: &str = "https://archive.org/advancedsearch.php";
const METADATA_URL_BASE: &str = "https://archive.org/metadata/";

// --- Structs for Bulk Search API (JSONP response) ---

/// Outer structure for the JSONP response (trimmed).
#[derive(Deserialize, Debug)]
struct JsonpResponseWrapper {
    // responseHeader isn't strictly needed but good for completeness
    // #[serde(rename = "responseHeader")]
    // response_header: serde_json::Value,
    response: JsonpResponseContent,
}

/// Inner 'response' object within the JSONP structure.
#[derive(Deserialize, Debug)]
struct JsonpResponseContent {
    #[serde(rename = "numFound")]
    num_found: usize,
    #[allow(dead_code)] // Allow dead code for this field specifically
    start: usize, // Keep original name for deserialization, allow dead code
    docs: Vec<ArchiveDoc>,
}


// --- Structs for Item List and Details ---

#[derive(Deserialize, Serialize, Debug, Clone)] // Added Serialize
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
    pub mediatype: Option<String>, // Added mediatype field
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
    pub mediatype: Option<String>, // Added mediatype field
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
// Removed ROWS_PER_PAGE
const BULK_ROWS: usize = 1_000_000; // Fetch up to 1 million rows in one go
const MAX_FETCH_RETRIES: u32 = 3; // Max retries for network/server errors

// --- API Fetch Functions ---

/// Fetches ALL item identifiers for a given collection name from Archive.org in a single bulk request.
///
/// Uses the advanced search API with JSONP output format and trims the wrapper.
pub async fn fetch_collection_items_bulk(
    client: &Client,
    collection_name: &str,
    rate_limiter: AppRateLimiter, // Added rate limiter parameter
) -> Result<(Vec<ArchiveDoc>, usize)> {
    info!("Fetching collection items BULK for '{}', rows {}", collection_name, BULK_ROWS);
    let query = format!("collection:\"{}\"", collection_name); // Ensure collection name is quoted
    let mut last_error: Option<anyhow::Error> = None;

    for attempt in 1..=MAX_FETCH_RETRIES {
        debug!("Attempting bulk fetch for '{}', attempt {}/{}", collection_name, attempt, MAX_FETCH_RETRIES);

        // --- Wait for Rate Limiter (inside retry loop) ---
        debug!("Waiting for rate limit permit for bulk collection items: {}", collection_name);
        rate_limiter.until_ready().await;
        debug!("Acquired rate limit permit for bulk collection items: {}", collection_name);
        // --- Rate Limit Permit Acquired ---

        // Construct request builder inside the loop for retries
        let request_builder = client
            .get(ADVANCED_SEARCH_URL)
            .query(&[
                ("q", query.as_str()),
                ("fl[]", "identifier"), // Request only the identifier field
                ("rows", &BULK_ROWS.to_string()),
                ("output", "json"),
                ("callback", "callback"), // Use the JSONP callback parameter
                // ("page", "1"), // Page/start usually not needed with huge rows, but API might require it? Test without first.
            ]);

        debug!("Sending bulk collection items request: {:?}", request_builder);

        match request_builder.try_clone() {
            Some(cloned_builder) => {
                match cloned_builder.send().await {
                    Ok(response) => {
                        let status = response.status();
                        if status.is_success() {
                            // Read the body as text first to handle JSONP wrapper
                            match response.text().await {
                                Ok(body_text) => {
                                    // Trim the "callback(" prefix and ")" suffix
                                    let trimmed_body = body_text
                                        .strip_prefix("callback(")
                                        .and_then(|s| s.strip_suffix(')'))
                                        .unwrap_or(&body_text); // Fallback to original text if trimming fails

                                    // Parse the trimmed JSON
                                    match serde_json::from_str::<JsonpResponseWrapper>(trimmed_body) {
                                        Ok(parsed_jsonp) => {
                                            let docs = parsed_jsonp.response.docs;
                                            let total_found = parsed_jsonp.response.num_found;
                                            info!("Successfully fetched BULK {} items (total reported: {}) for collection '{}'",
                                                  docs.len(), total_found, collection_name);
                                            // Basic sanity check
                                            if docs.len() > total_found {
                                                warn!("Fetched more items ({}) than reported total ({}) for collection '{}'. Using fetched count.", docs.len(), total_found, collection_name);
                                                // Optionally return docs.len() as the total? Or stick with reported total?
                                                // Let's return the actual docs and the reported total for now.
                                            }
                                            return Ok((docs, total_found));
                                        }
                                        Err(e) => {
                                            let parse_err = anyhow!(e).context(format!(
                                                "Failed to parse trimmed JSONP response for bulk collection items '{}' (Attempt {}/{})",
                                                collection_name, attempt, MAX_FETCH_RETRIES
                                            ));
                                            error!("Trimmed Body: '{}'", trimmed_body); // Log the body that failed parsing
                                            error!("{}", parse_err);
                                            last_error = Some(parse_err);
                                            // Don't retry on parse errors
                                            break;
                                        }
                                    }
                                }
                                Err(e) => {
                                    let body_err = anyhow!(e).context(format!(
                                        "Failed to read response body for bulk collection items '{}' (Attempt {}/{})",
                                        collection_name, attempt, MAX_FETCH_RETRIES
                                    ));
                                    error!("{}", body_err);
                                    last_error = Some(body_err);
                                    // Don't retry if reading body fails
                                    break;
                                }
                            }
                        } else {
                            // Handle non-success HTTP status
                            let err_msg = format!(
                                "Bulk collection items API request failed for '{}' with status: {} (Attempt {}/{})",
                                collection_name, status, attempt, MAX_FETCH_RETRIES
                            );
                            error!("{}", err_msg);
                            last_error = Some(anyhow!(err_msg));

                            // Retry only on server errors (5xx) or specific transient errors if needed
                            if status.is_server_error() && attempt < MAX_FETCH_RETRIES {
                                let delay_secs = 1 << (attempt - 1); // Exponential backoff: 1s, 2s
                                warn!("Retrying bulk collection items fetch in {} seconds...", delay_secs);
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
                            "Failed to send bulk collection items request for '{}' (Attempt {}/{})",
                            collection_name, attempt, MAX_FETCH_RETRIES
                        ));
                        error!("{}", current_err);
                        last_error = Some(current_err);

                        if attempt < MAX_FETCH_RETRIES {
                            let delay_secs = 1 << (attempt - 1); // Exponential backoff: 1s, 2s
                            warn!("Retrying bulk collection items fetch in {} seconds...", delay_secs);
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
                let build_err = anyhow!("Failed to clone request builder for bulk collection items '{}'", collection_name);
                error!("{}", build_err);
                last_error = Some(build_err);
                break; // Cannot retry if builder cannot be cloned
            }
        }
    } // End retry loop

    // If loop finished without returning Ok, return the last error
    Err(last_error.unwrap_or_else(|| anyhow!("Bulk collection items request failed after {} attempts for '{}'", MAX_FETCH_RETRIES, collection_name)))
}


/// Fetches detailed metadata and file list for a given item identifier.
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
                    // --- Check if essential data is missing even on 200 OK ---
                    if raw_details.metadata.is_none() && raw_details.files.is_none() {
                        warn!("Received 200 OK but metadata and files are missing for identifier '{}'. Treating as NotFound.", identifier);
                        return Err(FetchDetailsError {
                            kind: FetchDetailsErrorKind::NotFound,
                            source: anyhow!("Metadata and files missing in successful response"),
                            identifier: identifier.to_string(),
                        });
                    }

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
                let (title, creator, description, date, uploader, collections, mediatype) = // Added mediatype
                    if let Some(metadata) = &raw_details.metadata {
                         (
                            get_first_string(&metadata.title),
                            get_first_string(&metadata.creator),
                            get_first_string(&metadata.description),
                            metadata.date.clone(), // Clone the Option<String>
                            metadata.uploader.clone(), // Clone the Option<String>
                            get_string_array(&metadata.collection), // Use helper for collection
                            metadata.mediatype.clone(), // Clone the Option<String> for mediatype
                        )
                    } else {
                        // If metadata object is missing entirely, return None/empty values
                        (None, None, None, None, None, Vec::new(), None) // Added None for mediatype
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
                    mediatype,                          // Use processed value
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
} // <-- Add missing closing brace for fetch_item_details function

// Removed FetchAllResult enum and fetch_all_collection_items_incremental function


#[cfg(test)]
mod tests {
    // Note: Tests for fetch_collection_items and fetch_all_collection_items_incremental
    // need to be removed or adapted for fetch_collection_items_bulk.
    use super::*;
    use crate::app::AppRateLimiter; // Use the type alias
    use governor::{Quota, RateLimiter, clock::SystemClock};
    use reqwest::Client;
    use std::{error::Error, sync::Arc, time::Duration, num::NonZeroU32}; // Import std::error::Error
    use tokio;

    // Helper function to create a client with timeouts for tests
    fn test_client() -> Client {
        Client::builder()
            .timeout(Duration::from_secs(1800)) // Increased timeout to 1800s (30 minutes) for potentially large test fetches
            .connect_timeout(Duration::from_secs(60)) // Keep connect timeout reasonable
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

    // --- fetch_collection_items_bulk tests ---
    #[tokio::test]
    #[ignore] // Ignored by default, run with `cargo test -- --ignored`
    async fn test_fetch_collection_items_bulk_integration_success() {
        // Arrange
        let client = test_client();
        let collection_name = "enough_records"; // Use a smaller collection for testing
        let limiter = test_limiter();

        // Act
        let result = fetch_collection_items_bulk(&client, collection_name, Arc::clone(&limiter)).await;

        // Assert
        assert!(result.is_ok(), "Bulk API call should succeed. Error: {:?}", result.err());
        let (items, total_found) = result.unwrap();
        // Adjust assertion for 'enough_records' - check for a reasonable number > 0
        assert!(total_found > 100, "Total found should be > 100 for 'enough_records' (found {})", total_found);
        assert!(!items.is_empty(), "Should return items for 'enough_records'");
        // Check if the number of items fetched is close to the total reported
        // Allow some difference as the total might fluctuate slightly or BULK_ROWS might be smaller
        let diff = (total_found as isize - items.len() as isize).abs();
        // Allow a slightly larger difference percentage for smaller collections if needed, or keep absolute diff
        assert!(diff < 100 || items.len() >= BULK_ROWS,
                "Fetched items ({}) should be close to total ({}) or limited by BULK_ROWS ({}) for '{}'",
                items.len(), total_found, BULK_ROWS, collection_name);
        assert!(items.iter().all(|doc| !doc.identifier.is_empty()), "All items should have an identifier");
    }

    #[tokio::test]
    #[ignore] // Ignored by default, run with `cargo test -- --ignored`
    async fn test_fetch_collection_items_bulk_integration_not_found() {
        // Arrange
        let client = test_client();
        let collection_name = "this_collection_should_really_not_exist_12345";
        let limiter = test_limiter();

        // Act
        let result = fetch_collection_items_bulk(&client, collection_name, limiter).await;

        // Assert
        // The API call itself might succeed but return 0 results.
        assert!(result.is_ok(), "Bulk API call should succeed even for non-existent collection. Error: {:?}", result.err());
        let (items, total_found) = result.unwrap();
        assert_eq!(total_found, 0, "Total found should be 0 for non-existent collection");
        assert!(items.is_empty(), "Should return no items for a non-existent collection");
    }

    // --- fetch_item_details tests (remain unchanged) ---
    #[tokio::test]
    #[ignore]
    async fn test_fetch_item_details_integration_success() {
        // Arrange
        let client = test_client(); // Use helper function
        // Using the item provided by the user
        let identifier = "enrmp270_litmus_-_perception_of_light";
        let limiter = test_limiter(); // Create dummy limiter

        // Act
        let result = fetch_item_details(&client, identifier, limiter).await; // Use the declared limiter

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

    // Removed leftover tests calling the old fetch_collection_items function
}
