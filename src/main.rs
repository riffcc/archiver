use anyhow::{anyhow, Context, Result};
use log::{error, info, warn, LevelFilter}; // Import log macros and LevelFilter
use rust_tui_app::{
    app::{App, DownloadAction, DownloadProgress, UpdateAction},
    archive_api::{self, ArchiveDoc, ItemDetails}, // Removed FileDetails
    event::{Event, EventHandler},
    settings, // Removed self, Settings
    tui::Tui,
    update::update,
};
use ratatui::{backend::CrosstermBackend, Terminal};
use reqwest::Client;
use simplelog::{CombinedLogger, Config, TermLogger, WriteLogger, TerminalMode, ColorChoice}; // Import simplelog items
use std::{fs::File, io, path::Path, sync::Arc, time::Instant}; // Add File, Path
use tokio::sync::{mpsc, Semaphore};
/// Initializes the logger. Logs to `/var/log/riffarchiver.log`.
/// Falls back to terminal logging if file logging fails.
fn initialize_logging() -> Result<()> {
    let log_path = Path::new("/var/log/riffarchiver.log");

    // Attempt to create/open the log file
    let log_file_result = File::create(log_path);

    match log_file_result {
        Ok(log_file) => {
            CombinedLogger::init(vec![
                // Log INFO level and above to the file
                WriteLogger::new(LevelFilter::Info, Config::default(), log_file),
                // Log WARN level and above to the terminal
                TermLogger::new(LevelFilter::Warn, Config::default(), TerminalMode::Mixed, ColorChoice::Auto),
            ])?;
            info!("File logging initialized successfully to: {}", log_path.display());
        }
        Err(e) => {
            // If file logging fails, fall back to terminal-only logging
            TermLogger::init(LevelFilter::Info, Config::default(), TerminalMode::Mixed, ColorChoice::Auto)?;
            warn!(
                "Failed to create/open log file at '{}': {}. Falling back to terminal logging.",
                log_path.display(),
                e
            );
            warn!("Ensure the directory exists and the application has write permissions.");
        }
    }
    Ok(())
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

    // Create an application and load settings into it.
    let mut app = App::new();
    app.load_settings(settings);

    // Create a channel for collection search API results (now returns tuple)
    let (collection_api_tx, mut collection_api_rx) = mpsc::channel::<Result<(Vec<ArchiveDoc>, usize)>>(1);
    // Create a channel for item details API results
    let (item_details_tx, mut item_details_rx) = mpsc::channel::<Result<ItemDetails>>(1);
    // Create a channel for download progress updates
    let (download_progress_tx, mut download_progress_rx) = mpsc::channel::<DownloadProgress>(50); // Increased buffer

    // --- Concurrency Limiter ---
    // Use Arc for shared ownership across tasks
    let max_downloads = app.settings.max_concurrent_downloads.unwrap_or(usize::MAX); // Use MAX if None (effectively unlimited)
    let semaphore = Arc::new(Semaphore::new(max_downloads.max(1))); // Ensure at least 1 permit

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
                        if let Some(action) = update(&mut app, key_event) { // update now returns Option<UpdateAction>
                            match action {
                                UpdateAction::FetchCollectionItems(collection_name) => {
                                    // This action is now triggered when selecting a collection
                                    app.is_loading = true; // Set loading state for item list
                                    app.items.clear(); // Clear previous items
                                    app.error_message = None;
                                    app.download_status = None;
                                    // current_collection_name should already be set by update()
                                    // assert_eq!(app.current_collection_name.as_ref(), Some(&collection_name));

                                    let client = app.client.clone();
                                    let tx = collection_api_tx.clone();
                                    // Spawn task to fetch items for the specific collection
                                    tokio::spawn(async move {
                                        // TODO: Add pagination support later (rows=50, page=1 for now)
                                        let result = archive_api::fetch_collection_items(&client, &collection_name, 50, 1).await;
                                        let _ = tx.send(result).await;
                                    });
                                }
                                UpdateAction::FetchItemDetails => {
                                    // Triggered when selecting an item in the item list
                                    // is_loading_details should already be true from update()
                                    if let Some(identifier) = app.viewing_item_id.clone() {
                                        let client = app.client.clone();
                                        let tx = item_details_tx.clone();
                                        app.error_message = None;
                                        app.download_status = None;
                                        tokio::spawn(async move {
                                            let result = archive_api::fetch_item_details(&client, &identifier).await;
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
                                        let semaphore_clone = Arc::clone(&semaphore); // File download semaphore

                                        // Spawn the download task
                                        tokio::spawn(async move {
                                            let result = match download_action {
                                                DownloadAction::ItemAllFiles(item_id) => {
                                                    // Pass semaphore down, no collection context for single item download
                                                    download_item(&client_clone, &base_dir_clone, None, &item_id, progress_tx_clone.clone(), semaphore_clone).await
                                                }
                                                DownloadAction::File(item_id, file) => {
                                                    // Pass semaphore down, no collection context for single file download
                                                    download_single_file(&client_clone, &base_dir_clone, None, &item_id, &file, progress_tx_clone.clone(), semaphore_clone).await
                                                }
                                                DownloadAction::Collection(collection_id) => {
                                                     // Pass semaphore down, collection context is provided by download_collection itself
                                                     download_collection(&client_clone, &base_dir_clone, &collection_id, progress_tx_clone.clone(), semaphore_clone).await
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
            // Handle collection search API results
            Some(result) = collection_api_rx.recv() => {
                app.is_loading = false; // Reset collection loading state
                match result {
                    Ok((items, total_found)) => {
                        app.items = items;
                        app.total_items_found = Some(total_found);
                        if !app.items.is_empty() {
                            app.item_list_state.select(Some(0)); // Select first item in item list
                        } else {
                            app.item_list_state.select(None); // Deselect if list is empty
                        }
                        // Don't clear error message here, might be unrelated save error etc.
                        // update() clears errors at the start of handling event.
                    }
                    Err(e) => {
                        // Error fetching items for the selected collection
                        let err_msg = format!("Error fetching items: {}", e);
                        error!("{}", err_msg); // Log the error
                        app.items.clear();
                        app.total_items_found = None;
                        app.item_list_state.select(None);
                        app.error_message = Some(err_msg);
                    }
                }
            }
            // Handle item details API results
            Some(result) = item_details_rx.recv() => {
                app.is_loading_details = false; // Reset details loading state
                match result {
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
                    Err(e) => {
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
use tokio::fs;
use tokio::io::AsyncWriteExt;
use futures_util::StreamExt;
use log::{debug, error, info, warn}; // Import log macros here too


/// Downloads a single file.
/// Path: base_dir / [collection_id] / item_id / filename
async fn download_single_file(
    client: &Client,
    base_dir: &str,
    collection_id: Option<&str>, // Added: Optional collection context
    item_id: &str,
    file_details: &archive_api::FileDetails,
    progress_tx: mpsc::Sender<DownloadProgress>,
    semaphore: Arc<Semaphore>,
) -> Result<()> {
    let collection_str = collection_id.unwrap_or("<none>");
    info!("Starting download_single_file: collection='{}', item='{}', file='{}'",
          collection_str, item_id, file_details.name);

    // --- Idempotency Check ---
    // Construct path based on whether collection_id is present
    let file_path = match collection_id {
        Some(c) => Path::new(base_dir).join(c).join(item_id).join(&file_details.name),
        None => Path::new(base_dir).join(item_id).join(&file_details.name),
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
    debug!("Attempting to acquire download permit for file: {}", file_details.name);
    let _permit = semaphore.acquire_owned().await.context("Failed to acquire download semaphore permit")?;
    debug!("Acquired download permit for file: {}", file_details.name);
    // --- Permit Acquired ---


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
    progress_tx: mpsc::Sender<DownloadProgress>,
    semaphore: Arc<Semaphore>,
) -> Result<()> {
    let collection_str = collection_id.unwrap_or("<none>");
    info!("Starting download_item: collection='{}', item='{}'", collection_str, item_id);
    let _ = progress_tx.send(DownloadProgress::ItemStarted(item_id.to_string())).await;

    // Fetch item details first to get the file list
    let details = match archive_api::fetch_item_details(client, item_id).await {
        Ok(d) => d,
        Err(e) => {
            error!("Failed to fetch item details for '{}': {}. Skipping item download.", item_id, e);
            let _ = progress_tx.send(DownloadProgress::Error(format!("Failed to get details for {}: {}", item_id, e))).await;
            let _ = progress_tx.send(DownloadProgress::ItemCompleted(item_id.to_string(), false)).await; // Mark as failed
            return Err(e).context(format!("Failed fetching details for item '{}'", item_id));
        }
    };

     let total_files = details.files.len();
     info!("Found {} files for item '{}'", total_files, item_id);
     let _ = progress_tx.send(DownloadProgress::ItemFileCount(total_files)).await;


     if details.files.is_empty() {
         info!("No files found for item: {}. Marking as complete.", item_id);
         let _ = progress_tx.send(DownloadProgress::Status(format!("No files found for item: {}", item_id))).await;
         let _ = progress_tx.send(DownloadProgress::ItemCompleted(item_id.to_string(), true)).await; // Mark as completed (successfully, with 0 files)
         return Ok(());
     }

     info!("Queueing {} files for item: {}", total_files, item_id);
     let _ = progress_tx.send(DownloadProgress::Status(format!("Queueing {} files for item: {}", total_files, item_id))).await;

     // Create directory: base_dir / [collection_id] / item_id
     let item_dir = match collection_id {
        Some(c) => Path::new(base_dir).join(c).join(item_id),
        None => Path::new(base_dir).join(item_id),
     };
     debug!("Ensuring item directory exists: {}", item_dir.display());
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
         let semaphore_clone = Arc::clone(&semaphore);
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
                 semaphore_clone,
             )
             .await
         });
         file_join_handles.push(handle);
     }

     // Wait for all file download tasks for this item to complete
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

     // Send item completion status based on whether any file task failed
     let success_status = !item_failed;
     info!("Finished processing item '{}'. Success: {}", item_id, success_status);
     let _ = progress_tx.send(DownloadProgress::ItemCompleted(item_id.to_string(), success_status)).await;

     // Return Ok even if some files failed, ItemCompleted indicates success/failure of the item overall
     Ok(())
}

/// Downloads all items for a specific collection identifier.
async fn download_collection(
    client: &Client,
    base_dir: &str,
    collection_id: &str, // Now takes specific collection ID
    progress_tx: mpsc::Sender<DownloadProgress>,
    semaphore: Arc<Semaphore>, // File download semaphore
) -> Result<()> {
    info!("Starting download_collection for '{}'", collection_id);
    let _ = progress_tx.send(DownloadProgress::Status(format!("Fetching identifiers for: {}", collection_id))).await;

    // --- Fetch ALL identifiers for the specified collection ---
    let all_identifiers = match archive_api::fetch_all_collection_identifiers(client, collection_id).await {
        Ok(ids) => ids,
        Err(e) => {
            error!("Failed to fetch identifiers for collection '{}': {}", collection_id, e);
            let _ = progress_tx.send(DownloadProgress::Error(format!("Failed to get identifiers for {}: {}", collection_id, e))).await;
            // Send CollectionCompleted with 0/0 since we couldn't even start
            let _ = progress_tx.send(DownloadProgress::CollectionCompleted(0, 0)).await;
            return Err(e).context(format!("Failed fetching identifiers for collection '{}'", collection_id));
        }
    };
    // ---

    if all_identifiers.is_empty() {
        info!("No items found in collection: {}. Download complete.", collection_id);
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
        // Clone data needed for the item download task
        let client_clone = client.clone();
        let base_dir_clone = base_dir.to_string();
        let progress_tx_clone = progress_tx.clone();
        let semaphore_clone = Arc::clone(&semaphore); // Pass file semaphore down
        let item_id_clone = item_id.clone(); // Keep clone for task
        let collection_id_clone = collection_id.to_string(); // Clone collection ID for task

        let handle = tokio::spawn(async move {
            // download_item handles fetching details and spawning file downloads
            // It uses the semaphore passed down for individual file permits
            let item_result = download_item(
                &client_clone,
                &base_dir_clone,
                Some(&collection_id_clone), // Pass collection ID context (now cloned)
                &item_id_clone,
                progress_tx_clone.clone(),
                semaphore_clone, // Pass file semaphore
            )
            .await;
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
}

// TODO: Implement multi-collection download logic using max_concurrent_collections semaphore.
// This would likely involve another layer of task spawning in main.rs or a dedicated function.
