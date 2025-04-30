use anyhow::{anyhow, Context, Result};
use rust_tui_app::{
    app::{App, DownloadAction, DownloadProgress, UpdateAction}, // Add DownloadProgress
    archive_api::{self, ArchiveDoc, ItemDetails},
    event::{Event, EventHandler},
    settings,
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
                        if let Some(action) = update(&mut app, key_event) {
                            match action {
                                UpdateAction::FetchCollection => { // Use direct name
                                    app.is_loading = true; // Set loading state for collection search
                                    app.items.clear(); // Clear previous items
                                    app.error_message = None; // Clear previous error
                                    app.download_status = None; // Clear download status

                                    let client = app.client.clone();
                                    // Use current_collection_name if set, otherwise input
                                    let collection_name = app.current_collection_name.clone().unwrap_or_else(|| app.collection_input.clone());
                                    let tx = collection_api_tx.clone();

                                    tokio::spawn(async move {
                                        let result = archive_api::fetch_collection_items(&client, &collection_name, 50, 1).await;
                                        let _ = tx.send(result).await;
                                    });
                                }
                                UpdateAction::FetchItemDetails => { // Use direct name
                                    // is_loading_details should already be true from update()
                                    if let Some(identifier) = app.viewing_item_id.clone() {
                                        let client = app.client.clone();
                                        let tx = item_details_tx.clone();
                                        app.error_message = None; // Clear previous error
                                        app.download_status = None; // Clear download status
                                        tokio::spawn(async move {
                                            let result = archive_api::fetch_item_details(&client, &identifier).await;
                                            let _ = tx.send(result).await;
                                        });
                                    } else {
                                        app.is_loading_details = false;
                                        app.error_message = Some("Error: Tried to load details without an item ID.".to_string());
                                    }
                                }
                                UpdateAction::StartDownload(download_action) => { // Use direct name
                                    if app.is_downloading {
                                        app.download_status = Some("Another download is already in progress.".to_string());
                                    } else if let Some(base_dir) = app.settings.download_directory.clone() {
                                        app.is_downloading = true; // Set downloading flag
                                        app.error_message = None; // Clear previous error
                                        // Reset progress counters for new download operation
                                        // total_items_to_download is set in update() for bulk downloads
                                        // app.total_items_to_download = None;
                                        app.items_downloaded_count = 0;
                                        app.total_files_to_download = None;
                                        app.files_downloaded_count = 0;
                                        app.total_bytes_downloaded = 0; // Reset bytes
                                        app.download_start_time = Some(Instant::now()); // Record start time

                                        // Clone necessary data for the task
                                        let client = app.client.clone();
                                        // Clone data needed *before* the permit acquisition task
                                        let client_clone = client.clone();
                                        let base_dir_clone = base_dir.clone();
                                        let collection_clone = app.current_collection_name.clone().unwrap_or_default();
                                        let progress_tx_clone = download_progress_tx.clone(); // Use progress channel
                                        let semaphore_clone = Arc::clone(&semaphore); // Clone Arc pointer

                                        tokio::spawn(async move {
                                            // Acquire semaphore permit before starting download logic
                                            let permit = match semaphore_clone.acquire().await {
                                                 Ok(p) => p,
                                                 Err(_) => {
                                                     // Semaphore closed, unlikely but handle
                                                     let _ = progress_tx_clone.send(DownloadProgress::Error("Download semaphore closed.".to_string())).await;
                                                     return;
                                                 }
                                            };

                                            // Now we have a permit, proceed with the download
                                            let result = match download_action {
                                                DownloadAction::ItemAllFiles(id) => {
                                                    download_item(&client_clone, &base_dir_clone, &collection_clone, &id, progress_tx_clone.clone()).await
                                                }
                                                DownloadAction::File(id, file) => {
                                                    download_single_file(&client_clone, &base_dir_clone, &collection_clone, &id, &file, progress_tx_clone.clone()).await
                                                }
                                                DownloadAction::Collection => {
                                                     download_collection(&client_clone, &base_dir_clone, &collection_clone, progress_tx_clone.clone(), Arc::clone(&semaphore_clone)).await
                                                }
                                            };

                                            // Send error if the top-level task function itself failed (e.g., fetching identifiers)
                                            if let Err(e) = result {
                                                 let _ = progress_tx_clone.send(DownloadProgress::Error(format!("Download Task Error: {}", e))).await;
                                            }

                                            // Permit is automatically dropped here when the task finishes, releasing the semaphore slot
                                            drop(permit);
                                        });
                                    } else {
                                         // Should be caught by update, but handle defensively
                                         app.error_message = Some("Error: Download directory not set.".to_string());
                                    }
                                }
                            }
                        }
                    },
                    Event::Mouse(_) => {}
                    Event::Resize(_, _) => {}
                }
            }
            // Handle collection search API results
            Some(result) = collection_api_rx.recv() => {
                app.is_loading = false; // Reset collection loading state
                match result {
                    Ok((items, total_found)) => { // Destructure the tuple
                        app.items = items;
                        app.total_items_found = Some(total_found); // Store total found
                        if !app.items.is_empty() {
                             app.list_state.select(Some(0)); // Select first item if list is not empty
                        } else {
                             app.list_state.select(None); // Deselect if list is empty
                        }
                        app.error_message = None; // Clear error on success
                    }
                    Err(e) => {
                        app.items.clear(); // Clear items on error
                        app.total_items_found = None; // Clear total found on error
                        app.list_state.select(None); // Deselect on error
                        app.error_message = Some(format!("Error fetching data: {}", e));
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
async fn download_single_file(
    client: &Client,
    base_dir: &str,
    collection: &str,
    item_id: &str,
    file_details: &archive_api::FileDetails,
    progress_tx: mpsc::Sender<DownloadProgress>, // Changed type
) -> Result<()> {
    let file_path = Path::new(base_dir).join(collection).join(item_id).join(&file_details.name);
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

    // --- Idempotency Check ---
    let expected_size_str = file_details.size.as_deref();
    let expected_size: Option<u64> = expected_size_str.and_then(|s| s.parse().ok());

    if let Some(expected) = expected_size {
        if let Ok(metadata) = fs::metadata(&file_path).await {
            if metadata.is_file() && metadata.len() == expected {
                // Send FileCompleted immediately if skipped
                let _ = progress_tx.send(DownloadProgress::FileCompleted(file_details.name.clone())).await;
                // Also send a status message for clarity
                let _ = progress_tx.send(DownloadProgress::Status(format!("Skipping (exists): {}", file_details.name))).await;
                return Ok(()); // File exists and size matches, skip download
            }
        }
        // If metadata check fails or size mismatch, continue to download
    } else {
         // If expected size is unknown, download anyway (or log a warning?)
         let _ = progress_tx.send(DownloadProgress::Status(format!("Warning: Unknown size for {}, downloading anyway", file_details.name))).await;
    }
    // --- End Idempotency Check ---

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
    Ok(())
}

/// Downloads all files for a given item.
async fn download_item(
    client: &Client,
    base_dir: &str,
    collection: &str,
    item_id: &str,
    progress_tx: mpsc::Sender<DownloadProgress>, // Changed type
) -> Result<()> {
     let _ = progress_tx.send(DownloadProgress::ItemStarted(item_id.to_string())).await;
     // Fetch item details first to get the file list
     let details = archive_api::fetch_item_details(client, item_id).await?;

     let total_files = details.files.len();
     let _ = progress_tx.send(DownloadProgress::ItemFileCount(total_files)).await;


     if details.files.is_empty() {
         let _ = progress_tx.send(DownloadProgress::Status(format!("No files found for item: {}", item_id))).await;
         let _ = progress_tx.send(DownloadProgress::ItemCompleted(item_id.to_string(), true)).await; // Mark as completed (successfully, with 0 files)
         return Ok(()); // Not an error, just nothing to do
     }

     let _ = progress_tx.send(DownloadProgress::Status(format!("Starting download for {} files in item: {}", total_files, item_id))).await;

     let item_dir = Path::new(base_dir).join(collection).join(item_id);
     fs::create_dir_all(&item_dir).await.context("Failed to create item directory")?;

     // Removed unused success_count
     let mut item_failed = false;

     for file in details.files.iter() {
         // download_single_file now sends FileCompleted or Error messages
         if let Err(e) = download_single_file(client, base_dir, collection, item_id, file, progress_tx.clone()).await {
              item_failed = true;
              // Error message already sent by download_single_file
              let _ = progress_tx.send(DownloadProgress::Status(format!("Continuing item {} after error on file {}: {}", item_id, file.name, e))).await;
         }
     }

     // Send item completion status
     let _ = progress_tx.send(DownloadProgress::ItemCompleted(item_id.to_string(), !item_failed)).await;

     Ok(()) // Return Ok even if some files failed, ItemCompleted indicates success/failure
}

/// Downloads all items listed for the current collection.
async fn download_collection(
    client: &Client,
    base_dir: &str,
    collection: &str,
    progress_tx: mpsc::Sender<DownloadProgress>, // Changed type
    semaphore: Arc<Semaphore>, // Pass semaphore for sub-tasks
) -> Result<()> {
    let _ = progress_tx.send(DownloadProgress::Status(format!("Fetching identifiers for: {}", collection))).await;

    // --- Fetch ALL identifiers ---
    // This requires a modified fetch function or pagination loop.
    // Placeholder: Assume we have a function that returns Vec<String>
    let all_identifiers = archive_api::fetch_all_collection_identifiers(client, collection).await
        .context("Failed to fetch collection identifiers")?;
    // --- End Placeholder ---

    if all_identifiers.is_empty() {
        let _ = progress_tx.send(DownloadProgress::Status(format!("No items found for: {}", collection))).await;
        let _ = progress_tx.send(DownloadProgress::CollectionCompleted(0, 0)).await; // Send completion for 0 items
        return Ok(());
    }

    let total_items = all_identifiers.len();
    // Remove sending CollectionInfo - total is set in app state by update()
    // let _ = progress_tx.send(DownloadProgress::CollectionInfo(total_items)).await;
    let _ = progress_tx.send(DownloadProgress::Status(format!("Queueing {} items for: {}", total_items, collection))).await;

    let mut join_handles = vec![];
    let mut total_failed_items = 0; // Track failed items

    for (_index, item_id) in all_identifiers.into_iter().enumerate() { // Prefix index with _
        // Clone necessary data for the item download task
        let client_clone = client.clone();
        let base_dir_clone = base_dir.to_string();
        let collection_clone = collection.to_string();
        let progress_tx_clone = progress_tx.clone(); // Clone progress sender
        let semaphore_clone = Arc::clone(&semaphore);
        let item_id_clone = item_id.clone();

        let handle = tokio::spawn(async move {
            // Acquire permit for this specific item download task
            let permit = match semaphore_clone.acquire().await {
                 Ok(p) => p,
                 Err(_) => {
                     let _ = progress_tx_clone.send(DownloadProgress::Error(format!("Semaphore closed for item {}", item_id_clone))).await;
                     return Err(anyhow!("Semaphore closed")); // Return error from task
                 }
            };

            // download_item now sends its own progress, including ItemCompleted which indicates success/failure
            let item_result = download_item(&client_clone, &base_dir_clone, &collection_clone, &item_id_clone, progress_tx_clone.clone()).await;

             // Permit dropped automatically when task scope ends
             drop(permit);
             item_result // Return the result of the item download
        });
        join_handles.push(handle);
    }

    // Wait for all spawned item download tasks to complete and count failures
    for handle in join_handles {
        match handle.await {
            Ok(Ok(_)) => { /* Item download task succeeded */ }
            Ok(Err(_)) => { total_failed_items += 1; } // Item download function returned error
            Err(_) => { total_failed_items += 1; } // Task itself panicked or was cancelled
        }
    }

    // Send final collection completion status
    let _ = progress_tx.send(DownloadProgress::CollectionCompleted(total_items, total_failed_items)).await;

    Ok(())
}
