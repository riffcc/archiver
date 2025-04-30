use anyhow::{anyhow, Context, Result};
use rust_tui_app::{
    app::{App, DownloadAction, DownloadProgress, UpdateAction}, // Removed AppState
    archive_api::{self, ArchiveDoc, ItemDetails}, // Removed FileDetails
    event::{Event, EventHandler},
    settings, // Removed self, Settings
    tui::Tui,
    update::update,
};
use ratatui::{backend::CrosstermBackend, Terminal};
use reqwest::Client;
use std::{io, sync::Arc, time::Instant}; // Add Instant
use tokio::sync::{mpsc, Semaphore};

#[tokio::main]
async fn main() -> Result<()> {
    // Load settings first.
    let settings = settings::load_settings()?;

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
    tui.init()?;

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
                                         app.error_message = Some(format!("Failed to save settings: {}", e));
                                     } else {
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
                        app.items.clear();
                        app.total_items_found = None;
                        app.item_list_state.select(None);
                        app.error_message = Some(format!("Error fetching items: {}", e));
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
                        app.current_item_details = None; // Clear details on error
                        app.file_list_state.select(None); // Reset file selection
                        app.error_message = Some(format!("Error fetching item details: {}", e));
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
    tui.exit()?;
    Ok(())
}


// --- Download Helper Functions ---

use std::path::Path;
use tokio::fs::{self, File};
use tokio::io::AsyncWriteExt;
use futures_util::StreamExt;


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
    // --- Idempotency Check ---
    // Construct path based on whether collection_id is present
    let file_path = match collection_id {
        Some(c) => Path::new(base_dir).join(c).join(item_id).join(&file_details.name),
        None => Path::new(base_dir).join(item_id).join(&file_details.name),
    };
    let expected_size_str = file_details.size.as_deref();
    let expected_size: Option<u64> = expected_size_str.and_then(|s| s.parse().ok());

    if let Some(expected) = expected_size {
        if let Ok(metadata) = fs::metadata(&file_path).await {
            if metadata.is_file() && metadata.len() == expected {
                // Send FileCompleted immediately if skipped
                let _ = progress_tx.send(DownloadProgress::FileCompleted(file_details.name.clone())).await;
                // Also send a status message for clarity
                let _ = progress_tx.send(DownloadProgress::Status(format!("Skipping (exists): {}", file_details.name))).await;
                return Ok(()); // File exists and size matches, skip download - NO PERMIT USED
            }
        }
        // If metadata check fails or size mismatch, continue to acquire permit and download
    } else {
         // If expected size is unknown, we still need to acquire permit before checking/downloading
         // Log warning later if needed after acquiring permit
    }
    // --- End Idempotency Check ---

    // --- Acquire Semaphore Permit ---
    // Acquire permit *before* making network request or creating file.
    // The permit is stored in `_permit` and will be dropped automatically
    // when this function returns (success or error).
    let _permit = semaphore.acquire_owned().await.context("Failed to acquire download semaphore permit")?;
    // --- Permit Acquired ---


    // Log unknown size warning if necessary
    if expected_size.is_none() {
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
        fs::create_dir_all(parent_dir).await.context("Failed to create download directory")?;
    }

    // Removed duplicate idempotency check block below

     let _ = progress_tx.send(DownloadProgress::Status(format!("Downloading: {}", file_details.name))).await;

    // Make the request
    let response = client.get(&download_url).send().await.context("Failed to send download request")?;

    if !response.status().is_success() {
        let err_msg = format!("Download failed for {}: Status {}", file_details.name, response.status());
         let _ = progress_tx.send(DownloadProgress::Error(err_msg.clone())).await; // Send error via progress channel
        return Err(anyhow!(err_msg));
    }

    // Stream the response body to the file
    let mut dest = File::create(&file_path).await.context("Failed to create target file")?;
    let mut stream = response.bytes_stream();

    while let Some(chunk_result) = stream.next().await {
        let chunk = chunk_result.context("Failed to read download chunk")?;
        dest.write_all(&chunk).await.context("Failed to write chunk to file")?;
        // Send byte count update
        let _ = progress_tx.send(DownloadProgress::BytesDownloaded(chunk.len() as u64)).await;
    }

    // Send completion via progress channel
    let _ = progress_tx.send(DownloadProgress::FileCompleted(file_details.name.clone())).await;

    // Permit is dropped automatically when _permit goes out of scope here.
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
    let _ = progress_tx.send(DownloadProgress::ItemStarted(item_id.to_string())).await;
    // Fetch item details first to get the file list
     let details = archive_api::fetch_item_details(client, item_id).await?;

     let total_files = details.files.len();
     let _ = progress_tx.send(DownloadProgress::ItemFileCount(total_files)).await;


     if details.files.is_empty() {
         let _ = progress_tx.send(DownloadProgress::Status(format!("No files found for item: {}", item_id))).await;
         let _ = progress_tx.send(DownloadProgress::ItemCompleted(item_id.to_string(), true)).await; // Mark as completed (successfully, with 0 files)
         return Ok(());
     }

     let _ = progress_tx.send(DownloadProgress::Status(format!("Queueing {} files for item: {}", total_files, item_id))).await;

     // Create directory: base_dir / [collection_id] / item_id
     let item_dir = match collection_id {
        Some(c) => Path::new(base_dir).join(c).join(item_id),
        None => Path::new(base_dir).join(item_id),
     };
     fs::create_dir_all(&item_dir).await.context("Failed to create item directory")?;

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
             Ok(Ok(_)) => { /* File download task succeeded */ }
             Ok(Err(e)) => {
                 item_failed = true;
                 // Error message already sent by download_single_file, but maybe log context here?
                 let _ = progress_tx.send(DownloadProgress::Status(format!("File download failed within item {}: {}", item_id, e))).await;
             }
             Err(e) => { // Task panicked or was cancelled
                 item_failed = true;
                 let _ = progress_tx.send(DownloadProgress::Error(format!("File download task panicked for item {}: {}", item_id, e))).await;
             }
         }
     }

     // Send item completion status based on whether any file task failed
     let _ = progress_tx.send(DownloadProgress::ItemCompleted(item_id.to_string(), !item_failed)).await;

     Ok(()) // Return Ok even if some files failed, ItemCompleted indicates success/failure
}

/// Downloads all items for a specific collection identifier.
async fn download_collection(
    client: &Client,
    base_dir: &str,
    collection_id: &str, // Now takes specific collection ID
    progress_tx: mpsc::Sender<DownloadProgress>,
    semaphore: Arc<Semaphore>, // File download semaphore
) -> Result<()> {
    let _ = progress_tx.send(DownloadProgress::Status(format!("Fetching identifiers for: {}", collection_id))).await;

    // --- Fetch ALL identifiers for the specified collection ---
    let all_identifiers = archive_api::fetch_all_collection_identifiers(client, collection_id).await
        .context(format!("Failed to fetch identifiers for collection '{}'", collection_id))?;
    // ---

    if all_identifiers.is_empty() {
        let _ = progress_tx.send(DownloadProgress::Status(format!("No items found in collection: {}", collection_id))).await;
        let _ = progress_tx.send(DownloadProgress::CollectionCompleted(0, 0)).await;
        return Ok(());
    }

    let total_items = all_identifiers.len();
    // Send total item count for this collection download
    let _ = progress_tx.send(DownloadProgress::CollectionInfo(total_items)).await;
    let _ = progress_tx.send(DownloadProgress::Status(format!("Queueing {} items for collection: {}", total_items, collection_id))).await;

    let mut join_handles = vec![];
    let mut total_failed_items = 0;

    // Iterate through identifiers and spawn item download tasks
    for item_id in all_identifiers.into_iter() {
        // Clone data needed for the item download task
        let client_clone = client.clone();
        let base_dir_clone = base_dir.to_string();
        let progress_tx_clone = progress_tx.clone();
        let semaphore_clone = Arc::clone(&semaphore); // Pass file semaphore down
        let item_id_clone = item_id.clone(); // Keep clone for task

        let handle = tokio::spawn(async move {
            // download_item handles fetching details and spawning file downloads
            // It uses the semaphore passed down for individual file permits
            let item_result = download_item(
                &client_clone,
                &base_dir_clone,
                Some(&collection_id_clone), // Pass collection ID context
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
    for handle in join_handles {
        match handle.await {
            Ok(Ok(_)) => { /* Item processing (spawning files) succeeded */ }
            Ok(Err(_)) => { total_failed_items += 1; } // download_item returned an error (e.g., failed to fetch details)
            Err(_) => { total_failed_items += 1; } // Task panicked
        }
        // Note: Individual file errors within an item are handled by download_item
        // and reflected in the ItemCompleted message's success flag.
        // total_failed_items here counts items where the top-level processing failed.
    }

    // Send final completion status for this specific collection download
    let _ = progress_tx.send(DownloadProgress::CollectionCompleted(total_items, total_failed_items)).await;

    Ok(())
}

// TODO: Implement multi-collection download logic using max_concurrent_collections semaphore.
// This would likely involve another layer of task spawning in main.rs or a dedicated function.
