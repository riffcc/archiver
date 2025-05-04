use anyhow::{anyhow, Context, Result};
use log::{debug, error, info, warn}; // Import log macros (removed LevelFilter)
use rust_tui_app::{
    app::{App, AppRateLimiter, DownloadAction, DownloadProgress, UpdateAction}, // Import AppRateLimiter
    archive_api::{self, ArchiveDoc, ItemDetails}, // Removed FetchAllResult
    event::{Event, EventHandler},
    settings::{self, DownloadMode},
    tui::Tui,
    update::update,
}; // Removed extra closing brace
use ratatui::{backend::CrosstermBackend, Terminal};
// Use SystemClock here to match the AppRateLimiter definition
use governor::{Quota, RateLimiter, clock::SystemClock}; // Removed unused NotKeyed
// Removed unused NoOpMiddleware import
// Removed unused nonzero_ext import
use reqwest::Client;
use simplelog::{Config, WriteLogger, LevelFilter}; // Import necessary simplelog items
use std::{fs::File, io, num::NonZeroU32, path::Path, sync::Arc, time::Instant}; // Add NonZeroU32, File, Path
use tokio::sync::{mpsc, Semaphore};
use tokio::time::Duration; // Import tokio Duration
/// Fails if the log file cannot be created or written to.
fn initialize_logging() -> Result<()> {
    let log_path = Path::new("/var/log/riffarchiver.log");

    // Attempt to create/open the log file
    match File::create(log_path) {
        Ok(log_file) => {
            // Initialize ONLY the file logger. Use LevelFilter::Info or adjust as needed.
            WriteLogger::init(LevelFilter::Info, Config::default(), log_file)
                .context(format!("Failed to initialize file logger at {}", log_path.display()))?;
            // Log initialization success *after* successful initialization
            info!("File logging initialized successfully to: {}", log_path.display());
            Ok(())
        }
        Err(e) => {
            // If file creation fails, return an error immediately.
            // No logging is possible here via simplelog if the logger isn't initialized.
            // The error will be propagated back to main and printed there before TUI starts.
            Err(anyhow!(
                "Failed to create/open log file at '{}': {}. Ensure the directory exists and the application has write permissions.",
                log_path.display(),
                e
            ))
        }
    }
}


#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging first.
    initialize_logging().context("Failed to initialize logging")?;
    info!("Application starting up.");


    // Load settings first.
    let settings = match settings::load_settings() {
        Ok(s) => s,
        Err(e) => {
            error!("Failed to load settings: {}", e);
            // Use default settings if loading fails
            warn!("Using default settings due to loading error.");
            settings::Settings::default()
        }
    };

    // --- Rate Limiter Setup ---
    // Allow 15 requests per minute. Use Arc for sharing.
    // Using SystemClock to match AppRateLimiter type alias.
    let quota = Quota::per_minute(NonZeroU32::new(15).unwrap());
    // Explicitly type with AppRateLimiter alias and use SystemClock
    let rate_limiter: AppRateLimiter = Arc::new(RateLimiter::direct_with_clock(quota, &SystemClock::default()));


    // Create an application, load settings, and pass the rate limiter.
    let mut app = App::new(Arc::clone(&rate_limiter));
    app.load_settings(settings);

    // Create a channel for bulk item fetch results (replaces incremental channel)
    let (bulk_fetch_tx, mut bulk_fetch_rx) = mpsc::channel::<Result<(Vec<ArchiveDoc>, usize)>>(1); // Buffer size 1
    // Create a channel for item details API results
    let (item_details_tx, mut item_details_rx) = mpsc::channel::<Result<ItemDetails, archive_api::FetchDetailsError>>(1);
    // Create a channel for download progress updates
    let (download_progress_tx, mut download_progress_rx) = mpsc::channel::<DownloadProgress>(50); // Increased buffer

    // --- Concurrency Limiter ---
    // --- Concurrency Limiters ---
    // Semaphore for limiting concurrent *file* downloads within items/collections
    let max_file_downloads = app.settings.max_concurrent_downloads.unwrap_or(4).max(1); // Default 4, min 1
    let file_semaphore = Arc::new(Semaphore::new(max_file_downloads));
    info!("File download concurrency limit: {}", max_file_downloads);

    // Semaphore for limiting concurrent *item processing* tasks within a collection download
    // (controls concurrent metadata fetches primarily)
    let max_item_tasks = app.settings.max_concurrent_collections.unwrap_or(2).max(1); // Default 2, min 1
    let collection_item_semaphore = Arc::new(Semaphore::new(max_item_tasks));
     info!("Collection item processing concurrency limit: {}", max_item_tasks);


    // Initialize the terminal user interface.
    let backend = CrosstermBackend::new(io::stderr());
    let terminal = Terminal::new(backend)?;
    let events = EventHandler::new(250); // Tick rate 250ms
    let mut tui = Tui::new(terminal, events);
    if let Err(e) = tui.init() {
        error!("Failed to initialize TUI: {}", e);
        // Attempt to restore terminal before exiting
        let _ = tui.exit(); // Ignore error during exit attempt
        return Err(e.context("TUI initialization failed"));
    }
    info!("TUI initialized successfully.");

    // Start the main loop.
    while app.running {
        // Render the user interface.
        tui.draw(&mut app)?;
        // Handle events using tokio::select!
        tokio::select! {
            // Handle terminal events
            event = tui.events.next() => {
                match event? {
                    Event::Tick => app.tick(),
                    Event::Key(key_event) => {
                        // Handle input and check if an action is requested
                        if let Some(action) = update(&mut app, key_event) {
                            match action {
                                UpdateAction::StartBulkItemFetch(collection_name) => { // Renamed action
                                    // Triggered when selecting a collection in update() and cache misses
                                    // State (is_loading, items cleared, etc.) should be set by update()
                                    app.error_message = None; // Clear previous errors
                                    app.download_status = None; // Clear status

                                    // Ensure collection name matches the one set in app state by update()
                                    if app.current_collection_name.as_ref() != Some(&collection_name) {
                                        error!("Mismatch between action collection name '{}' and app state '{}'",
                                               collection_name, app.current_collection_name.as_deref().unwrap_or("<None>"));
                                        app.is_loading = false; // Reset loading state on error
                                        app.error_message = Some("Internal error: Collection name mismatch.".to_string());
                                        continue; // Skip spawning task
                                    }

                                    let client = app.client.clone();
                                    let tx = bulk_fetch_tx.clone(); // Use the bulk channel sender
                                    let limiter_clone = Arc::clone(&rate_limiter);
                                    // Spawn the bulk fetch task
                                    tokio::spawn(async move {
                                        let result = archive_api::fetch_collection_items_bulk(
                                            &client,
                                            &collection_name,
                                            limiter_clone,
                                        )
                                        .await;
                                        // Send the single result (Ok or Err) back
                                        if tx.send(result).await.is_err() {
                                            warn!("Bulk fetch receiver dropped for collection '{}'.", collection_name);
                                        }
                                    });
                                }
                                UpdateAction::FetchItemDetails => {
                                    // Triggered when selecting an item in the item list
                                    // is_loading_details should already be true from update()
                                    if let Some(identifier) = app.viewing_item_id.clone() {
                                        let client = app.client.clone();
                                        let tx = item_details_tx.clone();
                                        let limiter_clone = Arc::clone(&rate_limiter); // Clone limiter for task
                                        app.error_message = None;
                                        app.download_status = None;
                                        tokio::spawn(async move {
                                            let result = archive_api::fetch_item_details(&client, &identifier, limiter_clone).await;
                                            let _ = tx.send(result).await;
                                        });
                                    } else {
                                        // Should not happen if triggered correctly from update()
                                        app.is_loading_details = false;
                                        app.error_message = Some("Error: No item ID available for details fetch.".to_string());
                                    }
                                }
                                UpdateAction::StartDownload(download_action) => {
                                    // Triggered by 'd' or 'b' in various contexts
                                    // Removed check: if app.is_downloading { ... }
                                    if let Some(base_dir) = app.settings.download_directory.clone() {
                                        // Set downloading flag and reset progress
                                        // Note: is_downloading is now slightly less accurate, as it's true
                                        // if *any* download task is running. We might need more granular tracking later.
                                        app.is_downloading = true;
                                        app.error_message = None;
                                        app.items_downloaded_count = 0;
                                        app.total_files_to_download = None; // Reset, will be updated by tasks
                                        app.files_downloaded_count = 0;
                                        app.total_bytes_downloaded = 0;
                                        app.download_start_time = Some(Instant::now());
                                        app.total_items_to_download = None; // Reset, set by Collection task if needed

                                        // Clone data needed for the download task
                                        let client_clone = app.client.clone();
                                        let base_dir_clone = base_dir.clone();
                                        let progress_tx_clone = download_progress_tx.clone();
                                        let file_semaphore_clone = Arc::clone(&file_semaphore); // Use renamed semaphore
                                        let collection_item_semaphore_clone = Arc::clone(&collection_item_semaphore); // Clone new semaphore
                                        let limiter_clone = Arc::clone(&rate_limiter); // Clone rate limiter
                                        let download_mode = app.settings.download_mode; // Get current download mode
                                        // Clone the current collection name *before* spawning the task
                                        let current_collection_name_clone = app.current_collection_name.clone();

                                        // Spawn the download task
                                        tokio::spawn(async move {
                                            let result = match download_action {
                                                DownloadAction::ItemAllFiles(item_id) => {
                                                    // Pass file_semaphore, mode, AND limiter down
                                                    // Pass the captured collection name
                                                    download_item(&client_clone, &base_dir_clone, current_collection_name_clone.as_deref(), &item_id, download_mode, progress_tx_clone.clone(), file_semaphore_clone, limiter_clone).await
                                                }
                                                DownloadAction::File(item_id, file) => {
                                                    // Pass file_semaphore AND limiter down
                                                    // Mode doesn't apply here, always download the specific file
                                                    // Pass the captured collection name
                                                    download_single_file(&client_clone, &base_dir_clone, current_collection_name_clone.as_deref(), &item_id, &file, progress_tx_clone.clone(), file_semaphore_clone, limiter_clone).await
                                                }
                                                DownloadAction::Collection(collection_id) => {
                                                     // Pass both semaphores, mode, AND limiter down
                                                     download_collection(&client_clone, &base_dir_clone, &collection_id, download_mode, progress_tx_clone.clone(), file_semaphore_clone, collection_item_semaphore_clone, limiter_clone).await
                                                }
                                            };

                                            // Report top-level task errors (e.g., failed to get identifiers)
                                            if let Err(e) = result {
                                                let _ = progress_tx_clone.send(DownloadProgress::Error(format!("Download Task Error: {}", e))).await;
                                            }
                                            // Note: is_downloading flag is reset when CollectionCompleted or Error is received
                                        });
                                    } else {
                                        // This case should be handled by update() sending to AskingDownloadDir state
                                        app.error_message = Some("Error: Download directory not set.".to_string());
                                    }
                                }
                                UpdateAction::SaveSettings => {
                                    // Triggered after adding/removing collection or exiting settings
                                    if let Err(e) = settings::save_settings(&app.settings) {
                                        let err_msg = format!("Failed to save settings: {}", e);
                                        error!("{}", err_msg); // Log the error
                                        app.error_message = Some(err_msg);
                                    } else {
                                        info!("Settings saved successfully.");
                                        // Optional: Show confirmation? Status bar might be enough.
                                        // app.download_status = Some("Settings saved.".to_string());
                                    }
                                }
                            }
                        }
                    },
                    Event::Mouse(_) => {} // Ignore mouse events
                    Event::Resize(_, _) => {} // Terminal handles resize redraw automatically
                }
            }
            // Handle bulk item fetch results
            Some(result) = bulk_fetch_rx.recv() => {
                app.is_loading = false; // Fetch finished (successfully or not)
                match result {
                    Ok((items, total_found)) => {
                        info!("Received {} items (total reported: {}) from bulk fetch.", items.len(), total_found);
                        app.total_items_found = Some(total_found); // Store reported total
                        let was_empty = app.items.is_empty(); // Check before setting new items

                        // Set items and save to cache
                        if let Err(e) = app.set_and_save_items(items) {
                             let err_msg = format!("Error saving item cache after bulk fetch: {}", e);
                             error!("{}", err_msg);
                             app.error_message = Some(err_msg);
                        } else {
                            // Select first item only if the list *was* empty before this fetch
                            // and the new list is not empty.
                            if was_empty && !app.items.is_empty() {
                                app.item_list_state.select(Some(0));
                            } else if app.items.is_empty() {
                                // Ensure selection is cleared if fetch returned no items
                                app.item_list_state.select(None);
                            }
                            // Clear error on successful fetch and save
                            app.error_message = None;
                        }
                    }
                    Err(e) => {
                        let err_msg = format!("Bulk item fetch failed: {}", e);
                        error!("{}", err_msg);
                        app.error_message = Some(err_msg);
                        app.items.clear(); // Clear items on fetch error
                        app.item_list_state.select(None);
                        app.total_items_found = None;
                    }
                }
            }
            // Handle item details API results
            Some(result) = item_details_rx.recv() => {
                app.is_loading_details = false; // Reset details loading state
                match result {
                    // Update match arm to handle FetchDetailsError
                    Ok(details) => {
                        app.current_item_details = Some(details);
                        // Select first file if available
                        if app.current_item_details.as_ref().map_or(false, |d| !d.files.is_empty()) {
                            app.file_list_state.select(Some(0));
                        } else {
                            app.file_list_state.select(None);
                        }
                        app.error_message = None; // Clear error on success
                    }
                    // Update match arm to handle FetchDetailsError
                    Err(e) => {
                        // Use the Display impl of FetchDetailsError directly
                        let err_msg = format!("Error fetching item details: {}", e);
                        error!("{}", err_msg); // Log the error
                        app.current_item_details = None; // Clear details on error
                        app.file_list_state.select(None); // Reset file selection
                        app.error_message = Some(err_msg);
                    }
                }
            }
            // Handle download progress updates
            Some(status) = download_progress_rx.recv() => { // Use correct receiver name
                 // Check for a final status message to reset the flag
                 // Note: CollectionCompleted is now the primary signal for bulk completion
                 // Individual ItemCompleted or Error messages might not reset is_downloading
                 // if it's part of a larger bulk download. Resetting only on CollectionCompleted or Error.

                 // Update App state based on progress message
                 match status {
                     DownloadProgress::CollectionInfo(total) => { // Add handler for CollectionInfo
                        app.total_items_to_download = Some(total);
                        // Keep existing status message or update if desired
                    }
                     DownloadProgress::ItemStarted(id) => {
                         app.download_status = Some(format!("Starting: {}", id));
                     }
                     DownloadProgress::ItemFileCount(count) => {
                         app.total_files_to_download = Some(app.total_files_to_download.unwrap_or(0) + count);
                         app.download_status = Some(format!("Found {} files...", count));
                     }
                     DownloadProgress::BytesDownloaded(bytes) => {
                         app.total_bytes_downloaded += bytes;
                         // Don't update status string for every chunk, too noisy
                     }
                     DownloadProgress::FileCompleted(filename) => {
                         app.files_downloaded_count += 1;
                         app.download_status = Some(format!("Done: {}", filename));
                     }
                     DownloadProgress::ItemCompleted(id, success) => {
                         app.items_downloaded_count += 1;
                         let status_prefix = if success { "Completed item" } else { "Finished item (with errors)" };
                         app.download_status = Some(format!("{}: {}", status_prefix, id));
                     }
                     DownloadProgress::ItemSkippedWasCollection(id) => {
                        // Increment downloaded count as we 'processed' it by skipping
                        app.items_downloaded_count += 1;
                        app.download_status = Some(format!("Skipped (is collection): {}", id));
                        // Optional: Add logic here to queue a download for the discovered collection 'id'
                        // E.g., app.pending_action = Some(UpdateAction::StartDownload(DownloadAction::Collection(id)));
                        // For now, just log and update status.
                        info!("Identified '{}' as a collection during TorrentOnly download attempt.", id);
                     }
                     DownloadProgress::CollectionCompleted(total, failed) => {
                         app.is_downloading = false; // Collection finished
                         app.download_start_time = None; // Clear start time
                         app.download_status = Some(format!("Collection download finished. Items: {} attempted, {} failed.", total, failed));
                     }
                     DownloadProgress::Error(msg) => {
                         error!("Download Progress Error: {}", msg); // Log the error
                         app.is_downloading = false; // Stop on major error
                         app.download_start_time = None; // Clear start time
                         app.error_message = Some(msg.clone()); // Show as main error
                         app.download_status = Some(format!("Error: {}", msg));
                     }
                     DownloadProgress::Status(msg) => {
                         // General status update
                         app.download_status = Some(msg);
                     }
                 }
            }
        }
    }

    // Exit the user interface.
    if let Err(e) = tui.exit() {
        error!("Failed to exit TUI cleanly: {}", e);
        // Continue shutdown despite TUI exit error
    } else {
        info!("TUI exited successfully.");
    }

    info!("Application shutting down.");
    Ok(())
}


// --- Download Helper Functions ---

// Removed redundant imports: use std::path::Path; and use tokio::fs::{self, File};
// The necessary items (std::path::Path, tokio::fs::File) are imported at the top.
// We still need `tokio::fs` itself for functions like `metadata` and `create_dir_all`.
use tokio::fs::{self, File as TokioFile}; // Alias tokio::fs::File to avoid clash with std::fs::File
use tokio::io::{AsyncReadExt, AsyncWriteExt}; // Add AsyncReadExt for reading cache file
use futures_util::StreamExt;
use serde_json; // Add serde_json for caching
// Removed redundant log macro import: use log::{debug, error, info, warn};
// Macros are already imported at the top of the file.


/// Downloads a single file.
/// Path: base_dir / [collection_id] / item_id / filename
async fn download_single_file(
    client: &Client,
    base_dir: &str,
    collection_id: Option<&str>, // Added: Optional collection context
    item_id: &str,
    file_details: &archive_api::FileDetails,
    progress_tx: mpsc::Sender<DownloadProgress>,
    file_semaphore: Arc<Semaphore>, // Renamed
    rate_limiter: AppRateLimiter, // Use the type alias
) -> Result<()> {
    let collection_str = collection_id.unwrap_or("<none>");
    info!("Starting download_single_file: collection='{}', item='{}', file='{}'",
          collection_str, item_id, file_details.name);

    // --- Idempotency Check & Path Construction ---
    // Construct path based on whether collection_id is present and if it's a torrent file
    let file_path = if file_details.name.ends_with("_archive.torrent") { // Check for the specific suffix
        // Special path for torrent files: base_dir / collection_id / item_id_archive.torrent
        match collection_id {
            Some(c) => Path::new(base_dir).join(c).join(format!("{}_archive.torrent", item_id)), // Use _archive.torrent format
            // If no collection context, maybe place in base_dir? Or error?
            // For now, assume torrent downloads happen within a collection context.
            // If collection_id is None, this will likely cause issues later if triggered.
            None => {
                warn!("Attempting to download torrent file '{}' for item '{}' without collection context. Placing in base directory.", file_details.name, item_id);
                Path::new(base_dir).join(format!("{}_archive.torrent", item_id)) // Use _archive.torrent format
                // Alternatively, return an error:
                // return Err(anyhow!("Cannot determine torrent download path without collection context for item '{}'", item_id));
            }
        }
    } else {
        // Standard path for other files: base_dir / [collection_id] / item_id / filename
        match collection_id {
            Some(c) => Path::new(base_dir).join(c).join(item_id).join(&file_details.name),
            None => Path::new(base_dir).join(item_id).join(&file_details.name),
        }
    };

    let expected_size_str = file_details.size.as_deref();
    let expected_size: Option<u64> = expected_size_str.and_then(|s| s.parse().ok());

    if let Some(expected) = expected_size {
        // Use tokio::fs::metadata here
        match fs::metadata(&file_path).await {
            Ok(metadata) => {
                if metadata.is_file() && metadata.len() == expected {
                    info!("Skipping existing file with matching size: '{}'", file_path.display());
                    // Send FileCompleted immediately if skipped
                    let _ = progress_tx.send(DownloadProgress::FileCompleted(file_details.name.clone())).await;
                    // Also send a status message for clarity
                    let _ = progress_tx.send(DownloadProgress::Status(format!("Skipping (exists): {}", file_details.name))).await;
                    return Ok(()); // File exists and size matches, skip download - NO PERMIT USED
                } else {
                     debug!("Existing file found but size mismatch or not a file: '{}'. Proceeding with download.", file_path.display());
                }
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                 debug!("File not found: '{}'. Proceeding with download.", file_path.display());
            }
            Err(e) => {
                 warn!("Failed to get metadata for '{}': {}. Proceeding with download.", file_path.display(), e);
            }
        }
        // If metadata check fails or size mismatch, continue to acquire permit and download
    } else {
         // If expected size is unknown, we still need to acquire permit before checking/downloading
         // Log warning later if needed after acquiring permit
         debug!("File size unknown for '{}'. Will acquire permit and download.", file_details.name);
    }
    // --- End Idempotency Check ---

    // --- Acquire Semaphore Permit ---
    // Acquire permit *before* making network request or creating file.
    // The permit is stored in `_permit` and will be dropped automatically
    // when this function returns (success or error).
    debug!("Attempting to acquire file download permit for file: {}", file_details.name);
    let _permit = file_semaphore.acquire_owned().await.context("Failed to acquire file download semaphore permit")?;
    debug!("Acquired file download permit for file: {}", file_details.name);
    // --- File Permit Acquired ---


    // --- Wait for Rate Limiter ---
    debug!("Waiting for rate limit permit for file: {}", file_details.name);
    rate_limiter.until_ready().await;
    debug!("Acquired rate limit permit for file: {}", file_details.name);
    // --- Rate Limit Permit Acquired ---


    // Log unknown size warning if necessary
    if expected_size.is_none() {
        warn!("File size is unknown for '{}'. Downloading anyway.", file_details.name);
        let _ = progress_tx.send(DownloadProgress::Status(format!("Warning: Unknown size for {}, downloading anyway", file_details.name))).await;
    }


    let download_url = format!(
        "https://archive.org/download/{}/{}",
        item_id,
        // URL encode the filename part? Archive.org seems tolerant but might be safer.
        // Using raw name for now.
        file_details.name
    );

    // Send status via progress channel
    // let _ = progress_tx.send(DownloadProgress::Status(format!("Checking: {}", file_details.name))).await;

    // Ensure target directory exists
    if let Some(parent_dir) = file_path.parent() {
        debug!("Ensuring download directory exists: {}", parent_dir.display());
        fs::create_dir_all(parent_dir).await.context(format!("Failed to create download directory '{}'", parent_dir.display()))?;
    } else {
        error!("Could not determine parent directory for path: {}", file_path.display());
        return Err(anyhow!("Invalid download file path: {}", file_path.display()));
    }

    info!("Downloading '{}' from {}", file_details.name, download_url);
    let _ = progress_tx.send(DownloadProgress::Status(format!("Downloading: {}", file_details.name))).await;

    // Make the request
    let response = client.get(&download_url).send().await.context(format!("Failed to send download request for {}", file_details.name))?;

    if !response.status().is_success() {
        let status = response.status();
        let err_msg = format!("Download request failed for '{}': Status {}", file_details.name, status);
        error!("{}", err_msg);
        let _ = progress_tx.send(DownloadProgress::Error(err_msg.clone())).await; // Send error via progress channel
        return Err(anyhow!(err_msg));
    }

    // Stream the response body to the file
    // Explicitly use tokio::fs::File::create for async operation
    debug!("Creating target file: {}", file_path.display());
    let mut dest = tokio::fs::File::create(&file_path).await.context(format!("Failed to create target file '{}'", file_path.display()))?;
    let mut stream = response.bytes_stream();
    let mut bytes_written: u64 = 0;

    while let Some(chunk_result) = stream.next().await {
        match chunk_result {
            Ok(chunk) => {
                let chunk_len = chunk.len() as u64;
                if let Err(e) = dest.write_all(&chunk).await {
                    error!("Failed to write chunk to file '{}': {}", file_path.display(), e);
                    return Err(e).context(format!("Failed to write chunk to file '{}'", file_path.display()));
                }
                bytes_written += chunk_len;
                // Send byte count update
                let _ = progress_tx.send(DownloadProgress::BytesDownloaded(chunk_len)).await;
            }
            Err(e) => {
                 error!("Failed to read download chunk for '{}': {}", file_details.name, e);
                 return Err(e).context(format!("Failed to read download chunk for '{}'", file_details.name));
            }
        }
    }

    info!("Successfully downloaded file '{}' ({} bytes)", file_details.name, bytes_written);
    // Send completion via progress channel
    let _ = progress_tx.send(DownloadProgress::FileCompleted(file_details.name.clone())).await;

    debug!("Releasing download permit for file: {}", file_details.name); // Log before permit is dropped
    Ok(())
}

/// Downloads all files for a given item.
/// Path: base_dir / [collection_id] / item_id / ...
async fn download_item(
    client: &Client,
    base_dir: &str,
    collection_id: Option<&str>, // Added: Optional collection context
    item_id: &str,
    mode: DownloadMode, // Added: Download mode
    progress_tx: mpsc::Sender<DownloadProgress>,
    file_semaphore: Arc<Semaphore>, // Renamed
    rate_limiter: AppRateLimiter, // Use the type alias
) -> Result<()> {
    let collection_str = collection_id.unwrap_or("<none>");
    info!("Starting download_item: collection='{}', item='{}', mode='{:?}'", collection_str, item_id, mode);
    let _ = progress_tx.send(DownloadProgress::ItemStarted(item_id.to_string())).await;

    // --- Mode-Specific Logic ---
    if mode == DownloadMode::TorrentOnly {
        info!("TorrentOnly mode: Attempting direct download of {}.torrent", item_id);
        let _ = progress_tx.send(DownloadProgress::Status(format!("Queueing torrent file for item: {}", item_id))).await;
        let _ = progress_tx.send(DownloadProgress::ItemFileCount(1)).await; // Only 1 file to download

        // Construct the expected torrent file details using the new format
        let torrent_file_details = archive_api::FileDetails {
            name: format!("{}_archive.torrent", item_id), // Use _archive.torrent format
            source: None,
            format: Some("Torrent".to_string()), // Indicate format if known
            size: None, // Size is unknown without fetching metadata
            md5: None,
        };

        // Ensure the parent directory for the torrent file exists
        // Path: base_dir / collection_id / item_id.torrent -> Parent: base_dir / collection_id
        let torrent_parent_dir = match collection_id {
            Some(c) => Path::new(base_dir).join(c),
            None => Path::new(base_dir).to_path_buf(), // Place directly in base if no collection?
        };
        debug!("Ensuring torrent parent directory exists: {}", torrent_parent_dir.display());
        fs::create_dir_all(&torrent_parent_dir).await.context(format!("Failed to create torrent parent directory '{}'", torrent_parent_dir.display()))?;

        // --- Idempotency Check for Torrent File ---
        let torrent_file_path = torrent_parent_dir.join(&torrent_file_details.name);
        match fs::metadata(&torrent_file_path).await {
            Ok(metadata) if metadata.is_file() => {
                info!("Skipping existing torrent file: '{}'", torrent_file_path.display());
                let _ = progress_tx.send(DownloadProgress::Status(format!("Skipping (exists): {}", torrent_file_details.name))).await;
                // Send ItemCompleted as we successfully handled this item (by skipping)
                let _ = progress_tx.send(DownloadProgress::ItemCompleted(item_id.to_string(), true)).await;
                return Ok(()); // File exists, skip download attempt
            }
            Ok(_) => {
                // Path exists but is not a file (e.g., a directory) - proceed to download/overwrite?
                // Or log a warning? For now, proceed.
                warn!("Path exists but is not a file: '{}'. Proceeding with download.", torrent_file_path.display());
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                // File does not exist, proceed with download
                debug!("Torrent file not found: '{}'. Proceeding with download.", torrent_file_path.display());
            }
            Err(e) => {
                // Other error checking metadata, log warning and proceed
                warn!("Failed to get metadata for torrent file '{}': {}. Proceeding with download.", torrent_file_path.display(), e);
            }
        }
        // --- End Idempotency Check ---

        // Spawn a single task to download the assumed torrent file
        let client_clone = client.clone();
        let base_dir_clone = base_dir.to_string();
        let item_id_clone = item_id.to_string();
        let progress_tx_clone = progress_tx.clone();
        let file_semaphore_clone = Arc::clone(&file_semaphore);
        let limiter_clone_torrent = Arc::clone(&rate_limiter);
        let collection_id_task_clone = collection_id.map(|s| s.to_string());

        let handle = tokio::spawn(async move {
            download_single_file(
                &client_clone,
                &base_dir_clone,
                collection_id_task_clone.as_deref(),
                &item_id_clone,
                &torrent_file_details, // Pass the constructed details
                progress_tx_clone,
                file_semaphore_clone,
                limiter_clone_torrent,
            )
            .await
        });

        // Wait for the single torrent download task
        let torrent_result = handle.await;
        let item_success = match torrent_result {
            Ok(Ok(_)) => {
                debug!("Assumed torrent download task completed successfully for item '{}'.", item_id);
                true
            }
            Ok(Err(e)) => { // Torrent download task completed but reported an error (e.g., 404)
                warn!("Assumed torrent download failed for item '{}': {}. Fetching metadata to check if it's a collection.", item_id, e);
                let _ = progress_tx.send(DownloadProgress::Status(format!("Torrent download failed for {}, checking metadata...", item_id))).await;

                // Fetch metadata to check if it's a collection
                let limiter_clone_details = Arc::clone(&rate_limiter);
                match archive_api::fetch_item_details(client, item_id, limiter_clone_details).await {
                    Ok(details) => {
                        // Check mediatype directly on the processed ItemDetails struct
                        if details.mediatype == Some("collection".to_string()) {
                             info!("Item '{}' is actually a collection. Skipping torrent download.", item_id);
                             let _ = progress_tx.send(DownloadProgress::ItemSkippedWasCollection(item_id.to_string())).await;
                             // Mark item processing as 'successful' in the sense that we handled it (by skipping)
                                // The CollectionCompleted count won't increment 'failed' for this.
                                return Ok(()); // Exit download_item successfully after skipping
                        } else {
                            // If mediatype is None or not "collection", proceed to mark as failed download
                            warn!("Item '{}' is not a collection (mediatype: {:?}). Torrent download failed.", item_id, details.mediatype);
                        }
                    }
                    Err(fetch_err) => {
                        // Metadata fetch failed after torrent download failed
                        error!("Failed to fetch metadata for item '{}' after torrent download failed: {}", item_id, fetch_err);
                        // Proceed to mark item as failed below
                    }
                }
                // If we reach here, it means torrent download failed AND (metadata fetch failed OR it wasn't a collection)
                // Mark the item as failed.
                let _ = progress_tx.send(DownloadProgress::Error(format!("Torrent download failed for {}: {}", item_id, e))).await;
                false // item_success = false
            }
            Err(e) => { // Torrent download task panicked
                error!("Assumed torrent download task panicked for item {}: {}", item_id, e);
                let _ = progress_tx.send(DownloadProgress::Error(format!("Torrent download task panicked for item {}: {}", item_id, e))).await;
                false
            }
        };

        // Only send ItemCompleted if we didn't already send ItemSkippedWasCollection
        if item_success {
            info!("Finished processing item '{}' (TorrentOnly mode - direct attempt). Success: {}", item_id, item_success);
            let _ = progress_tx.send(DownloadProgress::ItemCompleted(item_id.to_string(), item_success)).await;
        } else {
             // Failure case (torrent download failed and it wasn't identified as a collection, or task panicked)
             info!("Finished processing item '{}' (TorrentOnly mode - direct attempt). Success: false", item_id);
             let _ = progress_tx.send(DownloadProgress::ItemCompleted(item_id.to_string(), false)).await;
        }
        return Ok(()); // Finished processing this item in TorrentOnly mode

    } else { // Direct Mode
        // --- Fetch item details with retry logic (Only for Direct mode) ---
        let details: ItemDetails; // Declare details, assign on success
        let mut attempt = 0;
        let mut backoff_secs = 1; // Initial backoff delay
        const MAX_BACKOFF_SECS: u64 = 60 * 10; // Cap backoff at 10 minutes

        loop {
            attempt += 1;
            let limiter_clone_details = Arc::clone(&rate_limiter);
            let details_result = archive_api::fetch_item_details(client, item_id, limiter_clone_details).await;

            match details_result {
                Ok(fetched_details) => {
                    info!("Successfully fetched details for item '{}' on attempt {}", item_id, attempt);
                    details = fetched_details; // Assign directly on success
                    break; // Exit loop on success
                }
                Err(e) => {
                    // Check if the error is permanent
                    match e.kind {
                        archive_api::FetchDetailsErrorKind::NotFound |
                        archive_api::FetchDetailsErrorKind::ParseError |
                        archive_api::FetchDetailsErrorKind::ClientError(_) => {
                            error!("Permanent error fetching details for item '{}': {}. Skipping item.", item_id, e);
                            let _ = progress_tx.send(DownloadProgress::Error(format!("Permanent error for {}: {:?}", item_id, e.kind))).await;
                            let _ = progress_tx.send(DownloadProgress::ItemCompleted(item_id.to_string(), false)).await; // Mark as failed
                            return Ok(()); // Handled permanent item error
                        }
                        // Otherwise, it's a transient error, proceed with retry logic
                        _ => {
                            warn!("Transient error fetching details for item '{}' (Attempt {}): {}. Retrying in {}s...", item_id, attempt, e, backoff_secs);
                            let _ = progress_tx.send(DownloadProgress::Status(format!("Retrying {} (Attempt {}, Wait {}s): {:?}", item_id, attempt, backoff_secs, e.kind))).await;
                            tokio::time::sleep(Duration::from_secs(backoff_secs)).await;
                            backoff_secs = (backoff_secs * 2).min(MAX_BACKOFF_SECS);
                        }
                    }
                }
            }
        } // --- End fetch details retry loop ---

        let total_files = details.files.len();
        info!("Direct mode: Found {} files for item '{}'", total_files, item_id);
        let _ = progress_tx.send(DownloadProgress::ItemFileCount(total_files)).await;

        if details.files.is_empty() {
            info!("No files found for item: {}. Marking as complete.", item_id);
            let _ = progress_tx.send(DownloadProgress::Status(format!("No files found for item: {}", item_id))).await;
            let _ = progress_tx.send(DownloadProgress::ItemCompleted(item_id.to_string(), true)).await;
            return Ok(());
        }

        info!("Direct mode: Queueing {} files for item: {}", total_files, item_id);
        let _ = progress_tx.send(DownloadProgress::Status(format!("Queueing {} files for item: {}", total_files, item_id))).await;

        // For Direct mode, ensure the item-specific directory exists, as files (other than torrents) go there.
        // Path: base_dir / [collection_id] / item_id
        let item_dir = match collection_id {
            Some(c) => Path::new(base_dir).join(c).join(item_id),
            None => Path::new(base_dir).join(item_id),
        };
        debug!("Ensuring item directory exists for non-torrent files: {}", item_dir.display());
        fs::create_dir_all(&item_dir).await.context(format!("Failed to create item directory '{}'", item_dir.display()))?;


        let mut file_join_handles = vec![];
        let mut item_failed = false; // Track if any file task fails

        // Spawn a download task for each file concurrently
        for file in details.files { // Iterate by value to move into tasks
            // Clone necessary data for the file download task
            let client_clone = client.clone();
            let base_dir_clone = base_dir.to_string();
         let item_id_clone = item_id.to_string();
         let progress_tx_clone = progress_tx.clone();
         let file_semaphore_clone = Arc::clone(&file_semaphore); // Use renamed semaphore
         let limiter_clone_file = Arc::clone(&rate_limiter); // Clone limiter for file download
         let file_clone = file.clone();
         // Clone collection_id for the task (as Option<String>)
         let collection_id_task_clone = collection_id.map(|s| s.to_string());


         let handle = tokio::spawn(async move {
             // Call download_single_file, passing the optional collection ID
             download_single_file(
                 &client_clone,
                 &base_dir_clone,
                 collection_id_task_clone.as_deref(), // Pass optional collection ID as &str
                 &item_id_clone,
                 &file_clone,
                 progress_tx_clone,
                 file_semaphore_clone, // Pass renamed semaphore
                 limiter_clone_file, // Pass limiter
             )
             .await
         });
         file_join_handles.push(handle);
     }

     // Wait for all file download tasks for this item to complete (Direct Mode)
     for handle in file_join_handles {
         match handle.await {
             Ok(Ok(_)) => {
                 debug!("File download task completed successfully for item '{}'.", item_id);
             }
             Ok(Err(e)) => {
                 item_failed = true;
                 // Error already logged and sent by download_single_file, just log context here.
                 error!("File download task failed within item {}: {}", item_id, e);
                 // Optionally send another status update if needed, but Error should have been sent.
                 // let _ = progress_tx.send(DownloadProgress::Status(format!("File download failed within item {}: {}", item_id, e))).await;
             }
             Err(e) => { // Task panicked or was cancelled
                 item_failed = true;
                 error!("File download task panicked or was cancelled for item {}: {}", item_id, e);
                 let _ = progress_tx.send(DownloadProgress::Error(format!("File download task panicked for item {}: {}", item_id, e))).await;
             }
         }
     }

     // Send item completion status based on whether any file task failed (Direct Mode)
     let success_status = !item_failed;
     info!("Finished processing item '{}' (Direct mode). Success: {}", item_id, success_status);
     let _ = progress_tx.send(DownloadProgress::ItemCompleted(item_id.to_string(), success_status)).await;

     // Return Ok even if some files failed, ItemCompleted indicates success/failure of the item overall
     Ok(())
    } // End else block for Direct Mode
} // End download_item function

/// Downloads all items for a specific collection identifier.
async fn download_collection(
    client: &Client,
    base_dir: &str,
    collection_id: &str, // Now takes specific collection ID
    mode: DownloadMode, // Added: Download mode
    progress_tx: mpsc::Sender<DownloadProgress>,
    file_semaphore: Arc<Semaphore>, // Renamed file download semaphore
    collection_item_semaphore: Arc<Semaphore>, // Added item processing semaphore
    rate_limiter: AppRateLimiter, // Use the type alias
) -> Result<()> {
    info!("Starting download_collection for '{}', mode: {:?}", collection_id, mode);

    // --- Identifier Caching Logic ---
    let cache_file_name = format!("{}.identifiers.json", collection_id);
    let cache_path = Path::new(base_dir).join(&cache_file_name);
    let mut all_identifiers: Vec<String> = Vec::new();
    let mut use_cache = false;

    // 1. Check if cache file exists
    if cache_path.exists() {
        info!("Found identifier cache file: {}", cache_path.display());
        let _ = progress_tx.send(DownloadProgress::Status(format!("Loading identifiers from cache: {}", cache_file_name))).await;
        match TokioFile::open(&cache_path).await {
            Ok(mut file) => {
                let mut contents = String::new();
                if file.read_to_string(&mut contents).await.is_ok() {
                    match serde_json::from_str::<Vec<String>>(&contents) {
                        Ok(cached_ids) => {
                            if !cached_ids.is_empty() {
                                info!("Successfully loaded {} identifiers from cache: {}", cached_ids.len(), cache_path.display());
                                all_identifiers = cached_ids;
                                use_cache = true;
                            } else {
                                warn!("Cache file is empty or invalid: {}. Re-fetching.", cache_path.display());
                            }
                        }
                        Err(e) => {
                            warn!("Failed to parse cache file JSON '{}': {}. Re-fetching.", cache_path.display(), e);
                            // Attempt to delete the invalid cache file? Or just overwrite later.
                            let _ = fs::remove_file(&cache_path).await; // Try removing invalid cache
                        }
                    }
                } else {
                    warn!("Failed to read cache file '{}'. Re-fetching.", cache_path.display());
                }
            }
            Err(e) => {
                warn!("Failed to open cache file '{}': {}. Re-fetching.", cache_path.display(), e);
            }
        }
    }

    // 2. Fetch from API if cache wasn't used
    if !use_cache {
        info!("Fetching identifiers from API for collection: {}", collection_id);
        let _ = progress_tx.send(DownloadProgress::Status(format!("Fetching identifiers from API: {}", collection_id))).await;

        // --- Use bulk fetch to get identifiers ---
        // No temporary channel needed here, call directly
        let client_clone_ids = client.clone();
        let collection_id_clone_ids = collection_id.to_string(); // Keep clone for error messages
        let limiter_clone_ids = Arc::clone(&rate_limiter);

        // Call the bulk fetch function directly
        match archive_api::fetch_collection_items_bulk(&client_clone_ids, collection_id, limiter_clone_ids).await {
            Ok((fetched_items, _total_found)) => {
                 // Extract identifiers from fetched items
                 all_identifiers = fetched_items.into_iter().map(|doc| doc.identifier).collect();
                 info!("Fetched {} identifiers via bulk API for collection '{}'", all_identifiers.len(), collection_id);
            }
            Err(e) => {
                 // Propagate error if fetch failed
                 let err_msg = format!("Failed to get identifiers for {}: {}", collection_id_clone_ids, e);
                 error!("{}", err_msg);
                 let _ = progress_tx.send(DownloadProgress::Error(err_msg.clone())).await;
                 let _ = progress_tx.send(DownloadProgress::CollectionCompleted(0, 0)).await;
                 return Err(anyhow!(err_msg)); // Return the error
            }
        }
        // --- End bulk fetch ---

        // 3. Save fetched identifiers to cache (only if fetch was successful)
        if !all_identifiers.is_empty() {
            if !all_identifiers.is_empty() {
                    match serde_json::to_string_pretty(&all_identifiers) {
                        Ok(json_data) => {
                            // Ensure parent directory exists (should already from download setup, but good practice)
                            if let Some(parent) = cache_path.parent() {
                                if let Err(e) = fs::create_dir_all(parent).await {
                                     warn!("Failed to ensure cache directory exists '{}': {}", parent.display(), e);
                                     // Proceed without saving cache if dir creation fails
                                } else {
                                    // Write to cache file
                                    match TokioFile::create(&cache_path).await {
                                        Ok(mut file) => {
                                            if let Err(e) = file.write_all(json_data.as_bytes()).await {
                                                warn!("Failed to write to cache file '{}': {}", cache_path.display(), e);
                                            } else {
                                                info!("Successfully saved {} identifiers to cache: {}", all_identifiers.len(), cache_path.display());
                                            }
                                        }
                                        Err(e) => {
                                            warn!("Failed to create cache file '{}': {}", cache_path.display(), e);
                                        }
                                    }
                                }
                            } else {
                                warn!("Could not determine parent directory for cache file: {}", cache_path.display());
                            }
                        }
                        Err(e) => {
                            warn!("Failed to serialize identifiers to JSON for caching: {}", e);
                        }
                    }
                } // End of if !all_identifiers.is_empty()
                else { // Add the else block here
                    info!("No identifiers fetched from API, cache file not created/updated.");
                }
            } // End of else block (fetch_error is None)
        } // End of if !use_cache block
    // --- End Identifier Caching Logic --- // Removed extra brace before this comment


    if all_identifiers.is_empty() {
        info!("No items found in collection (or cache): {}. Download complete.", collection_id);
        let _ = progress_tx.send(DownloadProgress::Status(format!("No items found in collection: {}", collection_id))).await;
        let _ = progress_tx.send(DownloadProgress::CollectionCompleted(0, 0)).await;
        return Ok(());
    }

    let total_items = all_identifiers.len();
    info!("Found {} items to download for collection '{}'", total_items, collection_id);
    // Send total item count for this collection download
    let _ = progress_tx.send(DownloadProgress::CollectionInfo(total_items)).await;
    let _ = progress_tx.send(DownloadProgress::Status(format!("Queueing {} items for collection: {}", total_items, collection_id))).await;

    let mut join_handles = vec![];
    let mut total_failed_items = 0; // Count items where download_item itself returned Err or panicked

    // Iterate through identifiers and spawn item download tasks
    for item_id in all_identifiers.into_iter() {
        // Acquire item processing permit *before* spawning
        debug!("Attempting to acquire item processing permit for item: {}", item_id);
        let item_permit = match collection_item_semaphore.clone().acquire_owned().await {
            Ok(permit) => {
                debug!("Acquired item processing permit for item: {}", item_id);
                permit
            },
            Err(e) => {
                error!("Failed to acquire item processing permit for item {}: {}", item_id, e);
                // Skip this item if permit acquisition fails
                total_failed_items += 1;
                continue;
            }
        };
        debug!("Acquired item processing permit for item: {}", item_id);

        // Clone data needed for the item download task
        let client_clone = client.clone();
        let base_dir_clone = base_dir.to_string();
        let progress_tx_clone = progress_tx.clone();
        let file_semaphore_clone = Arc::clone(&file_semaphore); // Pass file semaphore down
        let limiter_clone_item = Arc::clone(&rate_limiter); // Clone limiter for item download
        let item_id_clone = item_id.clone(); // Keep clone for task
        let collection_id_clone = collection_id.to_string(); // Clone collection ID for task

        let handle = tokio::spawn(async move {
            // download_item handles fetching details and spawning file downloads based on mode
            // It uses the file_semaphore passed down for individual file permits
            let item_result = download_item(
                &client_clone,
                &base_dir_clone,
                Some(&collection_id_clone), // Pass collection ID context (now cloned)
                &item_id_clone,
                mode, // Pass the download mode down
                progress_tx_clone.clone(),
                file_semaphore_clone, // Pass file semaphore
                limiter_clone_item, // Pass limiter
            )
            .await;
            // Drop the item permit when the task finishes
            drop(item_permit);
            debug!("Released item processing permit for item: {}", item_id_clone);
            item_result // Return result (Ok or Err)
        });
        join_handles.push(handle);
    }

    // Wait for all item download tasks for this collection to complete
    info!("Waiting for {} item download tasks for collection '{}'...", join_handles.len(), collection_id);
    for handle in join_handles {
        match handle.await {
            Ok(Ok(_)) => {
                debug!("Item download task completed successfully for collection '{}'.", collection_id);
            }
            Ok(Err(e)) => {
                // Error should have been logged within download_item (e.g., failed details fetch)
                error!("Item download task failed for collection '{}': {}", collection_id, e);
                total_failed_items += 1;
            }
            Err(e) => { // Task panicked or was cancelled
                error!("Item download task panicked or was cancelled for collection '{}': {}", collection_id, e);
                total_failed_items += 1;
            }
        }
        // Note: Individual file errors within an item are handled by download_item
        // and reflected in the ItemCompleted message's success flag.
        // total_failed_items here counts items where the top-level download_item task failed.
    }

    info!("Finished collection download for '{}'. Total items: {}, Failed items: {}",
          collection_id, total_items, total_failed_items);
    // Send final completion status for this specific collection download
    let _ = progress_tx.send(DownloadProgress::CollectionCompleted(total_items, total_failed_items)).await;

    Ok(())
} // Re-adding the closing brace for download_collection

// TODO: Implement multi-collection download logic using max_concurrent_collections semaphore.
// This would likely involve another layer of task spawning in main.rs or a dedicated function.
