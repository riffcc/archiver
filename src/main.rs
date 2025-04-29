use anyhow::Result;
use rust_tui_app::{
    app::{App, AppState}, // Import AppState
    archive_api::{self, ArchiveDoc, ItemDetails}, // Import ItemDetails
    event::{Event, EventHandler},
    settings,
    tui::Tui,
    update::update,
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io;
use tokio::sync::mpsc;

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

    // Create a channel for download status updates (placeholder for now)
    // let (download_status_tx, mut download_status_rx) = mpsc::channel::<String>(1);

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
                        let should_fetch_collection = update(&mut app, key_event);

                        if should_fetch_collection {
                            // Trigger collection search API call
                            app.is_loading = true; // Set loading state for collection search
                            app.items.clear(); // Clear previous items
                            app.error_message = None; // Clear previous error

                            let client = app.client.clone();
                            let collection_name = app.collection_input.clone();
                            let tx = collection_api_tx.clone();

                            tokio::spawn(async move {
                                // Fetch items (e.g., first 50 on page 1)
                                let result = archive_api::fetch_collection_items(&client, &collection_name, 50, 1).await;
                                // Send the result back to the main loop, ignore error if receiver dropped
                                let _ = tx.send(result).await;
                            });
                        } else if app.is_loading_details {
                             // Trigger item details API call if flag is set by update()
                             if let Some(identifier) = app.viewing_item_id.clone() {
                                let client = app.client.clone();
                                let tx = item_details_tx.clone();
                                tokio::spawn(async move {
                                    let result = archive_api::fetch_item_details(&client, &identifier).await;
                                    let _ = tx.send(result).await;
                                });
                             } else {
                                 // Should not happen if is_loading_details is true, but handle defensively
                                 app.is_loading_details = false;
                                 app.error_message = Some("Error: Tried to load details without an item ID.".to_string());
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
        }
    }

    // Exit the user interface.
    tui.exit()?;
    Ok(())
}
