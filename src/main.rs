use anyhow::{anyhow, Context, Result}; // Add anyhow macro and Context trait
use rust_tui_app::{
    app::{App, DownloadAction, UpdateAction}, // Remove AppState, Add actions
    archive_api::{self, ArchiveDoc, ItemDetails},
    event::{Event, EventHandler},
    settings,
    tui::Tui,
    update::update,
};
use ratatui::{backend::CrosstermBackend, Terminal};
use reqwest::Client;
use std::{io, sync::Arc}; // Add Arc
use tokio::sync::{mpsc, Semaphore}; // Add Semaphore

#[tokio::main]
async fn main() -> Result<()> {
    // Load settings first.
    let settings = settings::load_settings()?;

    // Create an application and load settings into it.
    let mut app = App::new();
    app.load_settings(settings);

    // Create a channel for collection search API results
    let (collection_api_tx, mut collection_api_rx) = mpsc::channel::<Result<Vec<ArchiveDoc>>>(1);
    // Create a channel for item details API results
    let (item_details_tx, mut item_details_rx) = mpsc::channel::<Result<ItemDetails>>(1);
    // Create a channel for download status updates
    let (download_status_tx, mut download_status_rx) = mpsc::channel::<String>(10);

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
                                        app.is_downloading = true;
                                        app.error_message = None; // Clear previous error
                                        // Clone necessary data for the task
                                        let client = app.client.clone();
                                        // Clone data needed *before* the permit acquisition task
                                        let client_clone = client.clone();
                                        let base_dir_clone = base_dir.clone();
                                        let collection_clone = app.current_collection_name.clone().unwrap_or_default();
                                        let status_tx_clone = download_status_tx.clone();
                                        let semaphore_clone = Arc::clone(&semaphore); // Clone Arc pointer

                                        tokio::spawn(async move {
                                            // Acquire semaphore permit before starting download logic
                                            let permit = match semaphore_clone.acquire().await {
                                                 Ok(p) => p,
                                                 Err(_) => {
                                                     // Semaphore closed, unlikely but handle
                                                     let _ = status_tx_clone.send("Error: Download semaphore closed.".to_string()).await;
                                                     return;
                                                 }
                                            };

                                            // Now we have a permit, proceed with the download
                                            let result = match download_action {
                                                DownloadAction::Item(id) => {
                                                    download_item(&client_clone, &base_dir_clone, &collection_clone, &id, status_tx_clone.clone()).await
                                                }
                                                DownloadAction::File(id, file) => {
                                                    download_single_file(&client_clone, &base_dir_clone, &collection_clone, &id, &file, status_tx_clone.clone()).await
                                                }
                                            };

                                            // Handle potential errors from the download functions themselves
                                            if let Err(e) = result {
                                                 let _ = status_tx_clone.send(format!("Download Task Error: {}", e)).await;
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
                    Ok(items) => {
                        app.items = items;
                        if !app.items.is_empty() {
                             app.list_state.select(Some(0)); // Select first item if list is not empty
                        } else {
                             app.list_state.select(None); // Deselect if list is empty
                        }
                        app.error_message = None; // Clear error on success
                    }
                    Err(e) => {
                        app.items.clear(); // Clear items on error
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
            // Handle download status updates
            Some(status) = download_status_rx.recv() => {
                 // Check for a final status message to reset the flag
                 if status.starts_with("Completed download") || status.starts_with("Download Error:") || status.starts_with("No files found") {
                     app.is_downloading = false;
                 }
                 app.download_status = Some(status);
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
    status_tx: mpsc::Sender<String>,
) -> Result<()> {
    let file_path = Path::new(base_dir).join(collection).join(item_id).join(&file_details.name);
    let download_url = format!(
        "https://archive.org/download/{}/{}",
        item_id,
        // URL encode the filename part? Archive.org seems tolerant but might be safer.
        // Using raw name for now.
        file_details.name
    );

    let _ = status_tx.send(format!("Checking: {}", file_details.name)).await;

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
                let _ = status_tx.send(format!("Skipping (exists): {}", file_details.name)).await;
                return Ok(()); // File exists and size matches, skip download
            }
        }
        // If metadata check fails or size mismatch, continue to download
    } else {
         // If expected size is unknown, download anyway (or log a warning?)
         let _ = status_tx.send(format!("Warning: Unknown size for {}, downloading anyway", file_details.name)).await;
    }
    // --- End Idempotency Check ---

     let _ = status_tx.send(format!("Downloading: {}", file_details.name)).await;

    // Make the request
    let response = client.get(&download_url).send().await.context("Failed to send download request")?;

    if !response.status().is_success() {
        let err_msg = format!("Download failed for {}: Status {}", file_details.name, response.status());
         let _ = status_tx.send(err_msg.clone()).await;
        return Err(anyhow!(err_msg));
    }

    // Stream the response body to the file
    let mut dest = File::create(&file_path).await.context("Failed to create target file")?;
    let mut stream = response.bytes_stream();

    while let Some(chunk_result) = stream.next().await {
        let chunk = chunk_result.context("Failed to read download chunk")?;
        dest.write_all(&chunk).await.context("Failed to write chunk to file")?;
    }

    let _ = status_tx.send(format!("Completed download: {}", file_details.name)).await;
    Ok(())
}

/// Downloads all files for a given item.
async fn download_item(
    client: &Client,
    base_dir: &str,
    collection: &str,
    item_id: &str,
    status_tx: mpsc::Sender<String>,
) -> Result<()> {
     let _ = status_tx.send(format!("Fetching details for item: {}", item_id)).await;
     // Fetch item details first to get the file list
     let details = archive_api::fetch_item_details(client, item_id).await?;

     if details.files.is_empty() {
         let _ = status_tx.send(format!("No files found to download for item: {}", item_id)).await;
         return Ok(()); // Not an error, just nothing to do
     }

     let total_files = details.files.len();
     let _ = status_tx.send(format!("Starting download for {} files in item: {}", total_files, item_id)).await;

     let item_dir = Path::new(base_dir).join(collection).join(item_id);
     fs::create_dir_all(&item_dir).await.context("Failed to create item directory")?;

     let mut success_count = 0;
     let mut fail_count = 0;

     for (index, file) in details.files.iter().enumerate() {
         let _ = status_tx.send(format!("[{}/{}] Downloading: {}", index + 1, total_files, file.name)).await;
         // Reuse single file download logic
         match download_single_file(client, base_dir, collection, item_id, file, status_tx.clone()).await {
              Ok(_) => success_count += 1,
              Err(e) => {
                  fail_count += 1;
                  // Send specific file error, but continue with others
                  let _ = status_tx.send(format!("Error downloading {}: {}", file.name, e)).await;
              }
         }
     }

     let final_status = format!(
         "Completed download for item: {}. Success: {}, Failed: {}",
         item_id, success_count, fail_count
     );
     let _ = status_tx.send(final_status).await;

     Ok(())
}
