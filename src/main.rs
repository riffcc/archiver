use anyhow::Result;
use rust_tui_app::{
    app::{App, AppState}, // Import AppState
    archive_api::{self, ArchiveDoc},
    event::{Event, EventHandler},
    settings::{self, Settings}, // Import settings module
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

    // Create a channel for API results
    let (api_result_tx, mut api_result_rx) = mpsc::channel::<Result<Vec<ArchiveDoc>>>(1);

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
                        if update(&mut app, key_event) {
                            // Trigger API call if update returns true
                            app.is_loading = true; // Set loading state
                            app.items.clear(); // Clear previous items
                            app.error_message = None; // Clear previous error

                            let client = app.client.clone();
                            let collection_name = app.collection_input.clone();
                            let tx = api_result_tx.clone();

                            tokio::spawn(async move {
                                // Fetch items (e.g., first 50 on page 1)
                                let result = archive_api::fetch_collection_items(&client, &collection_name, 50, 1).await;
                                // Send the result back to the main loop, ignore error if receiver dropped
                                let _ = tx.send(result).await;
                            });
                        }
                    },
                    Event::Mouse(_) => {}
                    Event::Resize(_, _) => {}
                }
            }
            // Handle API results
            Some(result) = api_result_rx.recv() => {
                app.is_loading = false; // Reset loading state
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
        }
    }

    // Exit the user interface.
    tui.exit()?;
    Ok(())
}
