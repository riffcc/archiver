use crate::app::{ActivePane, App, AppState, DownloadAction, UpdateAction};
// Removed unused settings import
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::widgets::ListState;

/// Handle key events based on the current application state.
/// Returns an optional `UpdateAction` to be performed by the main loop.
pub fn update(app: &mut App, key_event: KeyEvent) -> Option<UpdateAction> {
    // Clear pending action and non-sticky messages at the start
    app.pending_action = None;
    if !app.is_downloading {
        app.download_status = None; // Clear download status if not downloading
    }
    // Clear general error messages unless in a state that displays specific errors
    match app.current_state {
        AppState::AskingDownloadDir | AppState::EditingSetting | AppState::AddingCollection => {} // Keep errors in input modes
        _ => app.error_message = None, // Clear errors in other states
    }

    // --- Global Keys ---
    match key_event.code {
        KeyCode::Char('q') => {
            app.quit();
            return None;
        }
        KeyCode::Char('c') | KeyCode::Char('C') if key_event.modifiers == KeyModifiers::CONTROL => {
            app.quit();
            return None;
        }
        // Global 's' to enter settings (unless in an input mode)
        KeyCode::Char('s') => {
             match app.current_state {
                 AppState::Browsing | AppState::ViewingItem => {
                     app.current_state = AppState::SettingsView;
                     app.settings_list_state.select(Some(app.selected_setting_index));
                     return None;
                 }
                 _ => {} // Ignore 's' in other states like input modes
             }
        }
        // Global Esc handling (exit input modes or quit)
        KeyCode::Esc => {
            match app.current_state {
                AppState::AskingDownloadDir | AppState::EditingSetting | AppState::AddingCollection => {
                    // Handled within the specific state handlers to revert to previous state
                }
                AppState::ViewingItem | AppState::SettingsView => {
                    // Handled within the specific state handlers to revert to Browsing
                }
                AppState::Browsing => {
                    // Esc in Browsing mode quits the app
                    app.quit();
                    return None;
                }
                AppState::Downloading => {} // Ignore Esc during download
            }
        }
        _ => {} // Other keys are handled by state
    }

    // --- State-Specific Handling ---
    match app.current_state {
        AppState::Browsing => handle_browsing_input(app, key_event),
        AppState::AskingDownloadDir => handle_asking_download_dir_input(app, key_event),
        AppState::ViewingItem => handle_viewing_item_input(app, key_event),
        AppState::SettingsView => handle_settings_view_input(app, key_event),
        AppState::EditingSetting => handle_editing_setting_input(app, key_event),
        AppState::AddingCollection => handle_adding_collection_input(app, key_event),
        AppState::Downloading => {} // Ignore most input during download
    }

    // Return the pending action, if any was set by the handlers
    app.pending_action.clone()
}

/// Handles input when in the main browsing state (`AppState::Browsing`).
/// Dispatches to specific handlers based on the active pane.
fn handle_browsing_input(app: &mut App, key_event: KeyEvent) {
    // Handle Tab first to switch panes
    if key_event.code == KeyCode::Tab {
        app.active_pane = match app.active_pane {
            ActivePane::Collections => ActivePane::Items,
            ActivePane::Items => ActivePane::Collections,
        };
        return; // Pane switched, no further action needed for this event
    }

    // Delegate to pane-specific handlers
    match app.active_pane {
        ActivePane::Collections => handle_collections_pane_input(app, key_event),
        ActivePane::Items => handle_items_pane_input(app, key_event),
    }
}

/// Handles key events when the Collections pane is active.
fn handle_collections_pane_input(app: &mut App, key_event: KeyEvent) {
    match key_event.code {
        // Navigation
        KeyCode::Down => app.select_next_collection(),
        KeyCode::Up => app.select_previous_collection(),

        // Actions
        KeyCode::Enter => {
            if let Some(collection_name) = app.get_selected_collection().cloned() {
                app.current_collection_name = Some(collection_name.clone());
                app.items.clear(); // Clear previous items
                app.item_list_state.select(None); // Reset item selection
                app.is_loading = true; // Set loading flag for items
                app.pending_action = Some(UpdateAction::FetchCollectionItems(collection_name));
                app.active_pane = ActivePane::Items; // Switch focus to items pane after loading
            }
        }
        KeyCode::Char('a') => {
            // Enter Add Collection mode
            app.current_state = AppState::AddingCollection;
            app.add_collection_input.clear();
            app.add_collection_cursor_pos = 0;
        }
        KeyCode::Delete | KeyCode::Backspace => { // Use Delete or Backspace to remove
            if let Some(selected_collection) = app.get_selected_collection().cloned() {
                if app.remove_selected_collection() {
                    // If a collection was removed, trigger save
                    app.pending_action = Some(UpdateAction::SaveSettings);
                    // Clear items list if the removed collection was the one being viewed
                    if app.current_collection_name.as_ref() == Some(&selected_collection) {
                         app.items.clear();
                         app.item_list_state.select(None);
                         app.current_collection_name = None; // No collection selected anymore
                         app.total_items_found = None;
                    }
                }
            }
        }
        KeyCode::Char('d') | KeyCode::Char('b') => { // 'd' or 'b' to download selected collection
            if let Some(collection_name) = app.get_selected_collection().cloned() {
                if app.settings.download_directory.is_none() {
                    app.current_state = AppState::AskingDownloadDir;
                    // Use editing_setting_input for the path temporarily
                    app.editing_setting_input.clear();
                    app.cursor_position = 0;
                } else {
                    // Trigger download for the selected collection
                    app.pending_action = Some(UpdateAction::StartDownload(DownloadAction::Collection(collection_name.clone())));
                    app.download_status = Some(format!("Queueing download for collection: {}", collection_name));
                }
            } else {
                app.error_message = Some("Select a collection to download.".to_string());
            }
        }

        _ => {} // Ignore other keys
    }
}

/// Handles key events when the Items pane is active.
fn handle_items_pane_input(app: &mut App, key_event: KeyEvent) {
    match key_event.code {
        // Navigation
        KeyCode::Down => app.select_next_item(),
        KeyCode::Up => app.select_previous_item(),

        // Actions
        KeyCode::Enter => {
            // View item details
            if let Some(selected_index) = app.item_list_state.selected() {
                if let Some(item) = app.items.get(selected_index) {
                    app.viewing_item_id = Some(item.identifier.clone());
                    app.current_state = AppState::ViewingItem;
                    app.current_item_details = None; // Clear previous details
                    app.file_list_state = ListState::default(); // Reset file list selection
                    app.is_loading_details = true; // Set flag
                    app.pending_action = Some(UpdateAction::FetchItemDetails);
                }
            }
        }
        KeyCode::Char('d') => { // Download selected item
            if let Some(selected_index) = app.item_list_state.selected() {
                if let Some(item) = app.items.get(selected_index) {
                    if app.settings.download_directory.is_none() {
                        app.current_state = AppState::AskingDownloadDir;
                        app.editing_setting_input.clear();
                        app.cursor_position = 0;
                    } else {
                        app.pending_action = Some(UpdateAction::StartDownload(DownloadAction::ItemAllFiles(item.identifier.clone())));
                        app.download_status = Some(format!("Queueing download for item: {}", item.identifier));
                    }
                }
            } else {
                app.error_message = Some("Select an item to download.".to_string());
            }
        }
        KeyCode::Char('b') => { // Bulk download all items in the *current view*
            if let Some(collection_name) = app.current_collection_name.clone() {
                 if app.settings.download_directory.is_none() {
                     app.current_state = AppState::AskingDownloadDir;
                     app.editing_setting_input.clear();
                     app.cursor_position = 0;
                 } else if !app.items.is_empty() {
                     // Trigger download for the currently loaded collection
                     // Note: This re-uses the Collection action, which might re-fetch identifiers.
                     // A future optimization could pass the already loaded identifiers.
                     app.pending_action = Some(UpdateAction::StartDownload(DownloadAction::Collection(collection_name.clone())));
                     app.download_status = Some(format!("Queueing bulk download for collection: {}", collection_name));
                 } else {
                     app.error_message = Some("No items listed to download.".to_string());
                 }
            } else {
                 app.error_message = Some("No collection selected to download items from.".to_string());
            }
        }

        _ => {} // Ignore other keys
    }
}


/// Handles input when prompting for the download directory.
/// Uses the `editing_setting_input` buffer and `cursor_position`.
fn handle_asking_download_dir_input(app: &mut App, key_event: KeyEvent) {
    match key_event.code {
        KeyCode::Esc => {
            // Cancel entering download dir and return to browsing
            app.current_state = AppState::Browsing;
            app.editing_setting_input.clear(); // Clear the temp input
            app.error_message = None;
        }
        KeyCode::Char(to_insert) => {
            app.enter_char_edit_setting(to_insert);
        }
        KeyCode::Backspace => {
            app.delete_char_edit_setting();
        }
        KeyCode::Left => {
            app.move_cursor_left_edit_setting();
        }
        KeyCode::Right => {
            app.move_cursor_right_edit_setting();
        }
        KeyCode::Enter => {
            let entered_path = app.editing_setting_input.trim().to_string();
            if !entered_path.is_empty() {
                app.settings.download_directory = Some(entered_path);
                // Trigger save settings action
                app.pending_action = Some(UpdateAction::SaveSettings);
                app.current_state = AppState::Browsing; // Return to browsing
                app.editing_setting_input.clear(); // Clear the temp input
                // Set a confirmation message (will be cleared on next update unless error)
                app.error_message = Some("Download directory saved. Press 'd'/'b' again to start download.".to_string());
            } else {
                app.error_message = Some("Download directory cannot be empty. Press Esc to cancel.".to_string());
            }
        }
        _ => {} // Ignore other keys
    }
}


/// Handles input when viewing item details.
fn handle_viewing_item_input(app: &mut App, key_event: KeyEvent) {
    match key_event.code {
        KeyCode::Esc => {
            // Go back to browsing
            app.current_state = AppState::Browsing;
            app.viewing_item_id = None;
            app.current_item_details = None;
            app.file_list_state = ListState::default();
            // Active pane remains Items (usually where you came from)
        }
        KeyCode::Down => app.select_next_file(),
        KeyCode::Up => app.select_previous_file(),
        KeyCode::Enter | KeyCode::Char('d') => {
            // Download selected file
            if let Some(file_details) = app.get_selected_file().cloned() {
                if let Some(item_id) = app.viewing_item_id.clone() {
                    if app.settings.download_directory.is_none() {
                        app.current_state = AppState::AskingDownloadDir;
                        app.editing_setting_input.clear();
                        app.cursor_position = 0;
                    } else {
                        app.pending_action = Some(UpdateAction::StartDownload(DownloadAction::File(item_id, file_details.clone())));
                        app.download_status = Some(format!("Queueing download for file: {}", file_details.name));
                        // Maybe switch back to Browsing view after queuing? Or stay? Staying for now.
                    }
                }
            } else {
                app.error_message = Some("Select a file to download.".to_string());
            }
        }
        KeyCode::Char('b') => { // Download all files for this item
            if let Some(item_id) = app.viewing_item_id.clone() {
                if app.settings.download_directory.is_none() {
                    app.current_state = AppState::AskingDownloadDir;
                    app.editing_setting_input.clear();
                    app.cursor_position = 0;
                } else {
                    app.pending_action = Some(UpdateAction::StartDownload(DownloadAction::ItemAllFiles(item_id.clone())));
                    app.download_status = Some(format!("Queueing download for all files in item: {}", item_id));
                    // Maybe switch back to Browsing view after queuing? Or stay? Staying for now.
                }
            }
        }
        _ => {} // Ignore other keys
    }
}


use crate::settings::DownloadMode; // Import the new enum

/// Handles input when viewing/editing settings.
fn handle_settings_view_input(app: &mut App, key_event: KeyEvent) {
    let num_settings = 4; // Download Dir, Download Mode, File Concurrency, Collection Concurrency
    match key_event.code {
        KeyCode::Esc => {
            // Exit settings view, save, return to browsing
            app.current_state = AppState::Browsing;
            // Trigger save settings action
            app.pending_action = Some(UpdateAction::SaveSettings);
        }
        KeyCode::Down => {
            app.selected_setting_index = (app.selected_setting_index + 1) % num_settings;
            app.settings_list_state.select(Some(app.selected_setting_index));
        }
        KeyCode::Up => {
            app.selected_setting_index = if app.selected_setting_index == 0 {
                num_settings - 1
            } else {
                app.selected_setting_index - 1
            };
            app.settings_list_state.select(Some(app.selected_setting_index));
        }
        KeyCode::Right | KeyCode::Left => { // Use Left/Right to cycle/adjust
            match app.selected_setting_index {
                1 => { // Download Mode (Cycle)
                    app.settings.download_mode = match app.settings.download_mode {
                        DownloadMode::Direct => DownloadMode::TorrentOnly,
                        DownloadMode::TorrentOnly => DownloadMode::Direct,
                    };
                }
                2 => { // File Concurrency (Adjust)
                    let current = app.settings.max_concurrent_downloads.unwrap_or(1);
                    let new_val = if key_event.code == KeyCode::Right {
                        current.saturating_add(1)
                    } else {
                        current.saturating_sub(1).max(1) // Min 1
                    };
                    app.settings.max_concurrent_downloads = Some(new_val);
                }
                3 => { // Collection Concurrency (Adjust)
                    let current = app.settings.max_concurrent_collections.unwrap_or(1);
                     let new_val = if key_event.code == KeyCode::Right {
                        current.saturating_add(1)
                    } else {
                        current.saturating_sub(1).max(1) // Min 1
                    };
                    app.settings.max_concurrent_collections = Some(new_val);
                }
                _ => {} // No Left/Right action for Download Dir (index 0)
            }
        }
        KeyCode::Enter => {
            // Enter edit mode only for Download Directory (index 0)
            if app.selected_setting_index == 0 {
                app.current_state = AppState::EditingSetting;
                app.editing_setting_input = app.settings.download_directory.clone().unwrap_or_default();
                app.cursor_position = app.editing_setting_input.len();
            }
        }
        _ => {} // Ignore other keys
    }
}


/// Handles input when actively editing a setting value (only Download Dir for now).
/// Uses `editing_setting_input` and `cursor_position`.
fn handle_editing_setting_input(app: &mut App, key_event: KeyEvent) {
    match key_event.code {
        KeyCode::Esc => {
            // Cancel editing, revert to SettingsView
            app.current_state = AppState::SettingsView;
            app.editing_setting_input.clear();
            app.error_message = None;
        }
        KeyCode::Char(to_insert) => {
            app.enter_char_edit_setting(to_insert);
        }
        KeyCode::Backspace => {
            app.delete_char_edit_setting();
        }
        KeyCode::Left => {
            app.move_cursor_left_edit_setting();
        }
        KeyCode::Right => {
            app.move_cursor_right_edit_setting();
        }
        KeyCode::Enter => {
            // Save the edited value back to the actual setting
            let edited_value = app.editing_setting_input.trim().to_string();
            if app.selected_setting_index == 0 { // Download Directory
                app.settings.download_directory = if edited_value.is_empty() { None } else { Some(edited_value) };
            }
            // No need to trigger save action here, Esc from SettingsView saves.
            app.current_state = AppState::SettingsView;
            app.editing_setting_input.clear();
            app.error_message = None; // Clear error from input mode
        }
        _ => {} // Ignore other keys
    }
}

/// Handles input when adding a new collection identifier.
/// Uses `add_collection_input` and `add_collection_cursor_pos`.
fn handle_adding_collection_input(app: &mut App, key_event: KeyEvent) {
     match key_event.code {
        KeyCode::Esc => {
            // Cancel adding, revert to Browsing
            app.current_state = AppState::Browsing;
            app.add_collection_input.clear();
            app.error_message = None;
        }
        KeyCode::Char(to_insert) => {
            app.enter_char_add_collection(to_insert);
        }
        KeyCode::Backspace => {
            app.delete_char_add_collection();
        }
        KeyCode::Left => {
            app.move_cursor_left_add_collection();
        }
        KeyCode::Right => {
            app.move_cursor_right_add_collection();
        }
        KeyCode::Enter => {
            let identifier = app.add_collection_input.trim().to_string();
            if !identifier.is_empty() {
                app.add_collection_to_favorites(identifier);
                // Trigger save settings action
                app.pending_action = Some(UpdateAction::SaveSettings);
                app.current_state = AppState::Browsing;
                app.add_collection_input.clear();
            } else {
                app.error_message = Some("Collection identifier cannot be empty. Press Esc to cancel.".to_string());
            }
        }
        _ => {} // Ignore other keys
    }
}


// --- Tests ---
// Note: Many existing tests related to the old input/filter/navigate modes
// will need significant updates or removal due to the UI changes.
// Adding some basic tests for the new pane switching and collection management.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{ActivePane, App, AppState}; // Add ActivePane
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    // Removed unused imports: std::{env, fs}, tempfile::tempdir

    // Helper for setting up test environment with mock config
    // Note: This helper doesn't need to interact with the actual config file system anymore,
    // as App::load_settings uses the default path logic which is tested separately in settings::tests.
    // We just need an App instance with some initial settings for UI interaction tests.
    fn setup_test_app() -> App {
        let mut app = App::new();
        // Set some initial settings directly for testing UI logic
        app.settings.favorite_collections = vec!["coll1".to_string(), "coll2".to_string(), "coll3".to_string()];
        app.settings.download_directory = Some("/fake/test/dir".to_string()); // Assume a dir is set for some tests
        app.collection_list_state.select(Some(0)); // Pre-select first collection
        app
    }

    // Update tests to use the simplified setup helper
    #[test]
    fn test_update_quit_keys() {
        let mut app = setup_test_app();
        assert!(app.running);

        // Test 'q' in Browsing
        app.current_state = AppState::Browsing;
        update(&mut app, KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE));
        assert!(!app.running, "App should not be running after 'q'");

        // Reset and test Ctrl+C in Browsing
        app.running = true;
        update(&mut app, KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));
        assert!(!app.running, "App should not be running after Ctrl+C");

        // Reset and test Esc in Browsing (should quit)
        app.running = true;
        update(&mut app, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(!app.running, "App should not be running after Esc in Browsing");
    }

    #[test]
    fn test_update_tab_switches_panes_in_browsing() {
        let mut app = setup_test_app();
        app.current_state = AppState::Browsing;
        app.active_pane = ActivePane::Collections;

        update(&mut app, KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        assert_eq!(app.active_pane, ActivePane::Items);

        update(&mut app, KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        assert_eq!(app.active_pane, ActivePane::Collections);
    }

    #[test]
    fn test_update_collection_pane_navigation() {
        let mut app = setup_test_app();
        app.current_state = AppState::Browsing;
        app.active_pane = ActivePane::Collections;
        app.collection_list_state.select(Some(0)); // Start at first

        // Down
        update(&mut app, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        assert_eq!(app.collection_list_state.selected(), Some(1));

        // Down again
        update(&mut app, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        assert_eq!(app.collection_list_state.selected(), Some(2));

        // Down (wraps)
        update(&mut app, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        assert_eq!(app.collection_list_state.selected(), Some(0));

        // Up (wraps)
        update(&mut app, KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
        assert_eq!(app.collection_list_state.selected(), Some(2));

        // Up
        update(&mut app, KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
        assert_eq!(app.collection_list_state.selected(), Some(1));
    }

     #[test]
    fn test_update_item_pane_navigation() {
        let mut app = setup_test_app();
        app.current_state = AppState::Browsing;
        app.active_pane = ActivePane::Items; // Focus items pane
        app.items = vec![ // Add some dummy items
            crate::archive_api::ArchiveDoc { identifier: "itemA".to_string() },
            crate::archive_api::ArchiveDoc { identifier: "itemB".to_string() },
        ];
        app.item_list_state.select(None); // Start with nothing selected

        // Down
        update(&mut app, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        assert_eq!(app.item_list_state.selected(), Some(0));

        // Down
        update(&mut app, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        assert_eq!(app.item_list_state.selected(), Some(1));

        // Down (wraps)
        update(&mut app, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        assert_eq!(app.item_list_state.selected(), Some(0));

        // Up (wraps)
        update(&mut app, KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
        assert_eq!(app.item_list_state.selected(), Some(1));
    }

    #[test]
    fn test_update_collection_pane_enter_loads_items() {
        let mut app = setup_test_app();
        app.current_state = AppState::Browsing;
        app.active_pane = ActivePane::Collections;
        app.collection_list_state.select(Some(1)); // Select "coll2"

        let action = update(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

        assert!(action.is_some());
        assert!(matches!(action, Some(UpdateAction::FetchCollectionItems(ref name)) if name == "coll2"));
        assert_eq!(app.current_collection_name, Some("coll2".to_string()));
        assert!(app.items.is_empty()); // Items cleared
        assert!(app.item_list_state.selected().is_none()); // Item selection reset
        assert!(app.is_loading); // Loading flag set
        assert_eq!(app.active_pane, ActivePane::Items); // Focus switched to items pane
    }

     #[test]
    fn test_update_collection_pane_delete_removes_item_and_saves() {
        let mut app = setup_test_app();
        app.current_state = AppState::Browsing;
        app.active_pane = ActivePane::Collections;
        app.collection_list_state.select(Some(1)); // Select "coll2"
        assert_eq!(app.settings.favorite_collections.len(), 3);

        let action = update(&mut app, KeyEvent::new(KeyCode::Delete, KeyModifiers::NONE));

        assert!(action.is_some());
        assert!(matches!(action, Some(UpdateAction::SaveSettings)));
        assert_eq!(app.settings.favorite_collections.len(), 2);
        assert_eq!(app.settings.favorite_collections, vec!["coll1".to_string(), "coll3".to_string()]);
        assert_eq!(app.collection_list_state.selected(), Some(1)); // Selection should move to "coll3"
    }

     #[test]
    fn test_update_collection_pane_delete_removes_last_item() {
        let mut app = setup_test_app();
        app.current_state = AppState::Browsing;
        app.active_pane = ActivePane::Collections;
        app.collection_list_state.select(Some(2)); // Select "coll3" (last item)
        assert_eq!(app.settings.favorite_collections.len(), 3);

        let action = update(&mut app, KeyEvent::new(KeyCode::Delete, KeyModifiers::NONE));

        assert!(action.is_some());
        assert!(matches!(action, Some(UpdateAction::SaveSettings)));
        assert_eq!(app.settings.favorite_collections.len(), 2);
        assert_eq!(app.settings.favorite_collections, vec!["coll1".to_string(), "coll2".to_string()]);
        assert_eq!(app.collection_list_state.selected(), Some(1)); // Selection should move to new last item "coll2"
    }


    #[test]
    fn test_update_collection_pane_a_enters_adding_state() {
        let mut app = setup_test_app();
        app.current_state = AppState::Browsing;
        app.active_pane = ActivePane::Collections;

        let action = update(&mut app, KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE));

        assert!(action.is_none());
        assert_eq!(app.current_state, AppState::AddingCollection);
        assert!(app.add_collection_input.is_empty());
        assert_eq!(app.add_collection_cursor_pos, 0);
    }

    #[test]
    fn test_update_adding_collection_input_and_save() {
        let mut app = setup_test_app();
        app.current_state = AppState::AddingCollection;
        assert_eq!(app.settings.favorite_collections.len(), 3);

        // Simulate typing
        update(&mut app, KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE));
        update(&mut app, KeyEvent::new(KeyCode::Char('e'), KeyModifiers::NONE));
        update(&mut app, KeyEvent::new(KeyCode::Char('w'), KeyModifiers::NONE));
        assert_eq!(app.add_collection_input, "new");

        // Enter to save
        let action = update(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

        assert!(action.is_some());
        assert!(matches!(action, Some(UpdateAction::SaveSettings)));
        assert_eq!(app.current_state, AppState::Browsing);
        assert_eq!(app.settings.favorite_collections.len(), 4);
        assert!(app.settings.favorite_collections.contains(&"new".to_string()));
        // Check if it's selected (depends on sort order)
        let expected_sorted = vec!["coll1", "coll2", "coll3", "new"]; // Assuming simple append then sort
        assert_eq!(app.settings.favorite_collections, expected_sorted);
        assert_eq!(app.collection_list_state.selected(), Some(3)); // Should select the new item
    }

     #[test]
    fn test_update_adding_collection_esc_cancels() {
        let mut app = setup_test_app();
        app.current_state = AppState::AddingCollection;
        app.add_collection_input = "partial".to_string();

        let action = update(&mut app, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));

        assert!(action.is_none());
        assert_eq!(app.current_state, AppState::Browsing);
        assert_eq!(app.settings.favorite_collections.len(), 3); // No change
        assert!(app.add_collection_input.is_empty()); // Input cleared
    }

    use crate::settings::DownloadMode; // Import for test

    #[test]
    fn test_update_settings_navigation_and_adjustment() {
        let mut app = setup_test_app();
        app.current_state = AppState::SettingsView;
        app.selected_setting_index = 0; // Start at Download Dir
        app.settings_list_state.select(Some(0));
        app.settings.download_mode = DownloadMode::Direct; // Start with Direct
        app.settings.max_concurrent_downloads = Some(4);
        app.settings.max_concurrent_collections = Some(1);

        // Down to Download Mode
        update(&mut app, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        assert_eq!(app.selected_setting_index, 1);
        assert_eq!(app.settings_list_state.selected(), Some(1));

        // Right cycles Download Mode to TorrentOnly
        update(&mut app, KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
        assert_eq!(app.settings.download_mode, DownloadMode::TorrentOnly);

        // Left cycles Download Mode back to Direct
        update(&mut app, KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
        assert_eq!(app.settings.download_mode, DownloadMode::Direct);

        // Down to File Concurrency
        update(&mut app, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        assert_eq!(app.selected_setting_index, 2);
        assert_eq!(app.settings_list_state.selected(), Some(2));

        // Right increases File Concurrency
        update(&mut app, KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
        assert_eq!(app.settings.max_concurrent_downloads, Some(5));

        // Left decreases File Concurrency
        update(&mut app, KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
        assert_eq!(app.settings.max_concurrent_downloads, Some(4));

        // Left again (min 1)
        update(&mut app, KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
        update(&mut app, KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
        update(&mut app, KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
        update(&mut app, KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
        assert_eq!(app.settings.max_concurrent_downloads, Some(1));


        // Down to Collection Concurrency
        update(&mut app, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        assert_eq!(app.selected_setting_index, 3);
        assert_eq!(app.settings_list_state.selected(), Some(3));

         // Right increases Collection Concurrency
        update(&mut app, KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
        assert_eq!(app.settings.max_concurrent_collections, Some(2));

        // Left decreases Collection Concurrency
        update(&mut app, KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
        assert_eq!(app.settings.max_concurrent_collections, Some(1));

        // Left again (min 1)
        update(&mut app, KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
        assert_eq!(app.settings.max_concurrent_collections, Some(1));

        // Down wraps to Download Dir
        update(&mut app, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        assert_eq!(app.selected_setting_index, 0);

        // Enter on Download Dir enters EditingSetting state
        let action = update(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(action.is_none());
        assert_eq!(app.current_state, AppState::EditingSetting);
    }

    // TODO: Add tests for download actions ('d', 'b') in both panes
    // TODO: Add tests for item view ('Enter' in items pane)
    // TODO: Add tests for AskingDownloadDir state with new input handling
    // TODO: Add tests for EditingSetting state with new input handling
}
