use crate::app::{App, AppState, DownloadAction, UpdateAction}; // Import new types
use crate::settings;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::widgets::ListState;

/// Handle key events based on the current application state and input mode.
/// Returns an optional `UpdateAction` to be performed by the main loop.
pub fn update(app: &mut App, key_event: KeyEvent) -> Option<UpdateAction> {
    // Clear pending action at the start of handling a new event
    app.pending_action = None;
    // Clear previous download status message if not currently downloading
    if !app.is_downloading {
        app.download_status = None;
    }


    // Global quit keys take precedence
    match key_event.code {
        KeyCode::Char('q') => {
            app.quit();
            return None; // No action needed, just quit
        }
        KeyCode::Char('c') | KeyCode::Char('C') if key_event.modifiers == KeyModifiers::CONTROL => {
            app.quit();
            return None; // No action needed, just quit
        }
        // Allow Esc to exit filter mode if currently filtering, otherwise quit (or handle state-specific Esc)
        KeyCode::Esc => {
            if app.is_filtering_input && app.current_state == AppState::Browsing {
                app.is_filtering_input = false;
                return None; // Don't quit yet, just exit filter mode
            } else if app.current_state == AppState::AskingDownloadDir || app.current_state == AppState::ViewingItem {
                 // Let the state handlers manage Esc for these states
            }
             else {
                // If not filtering and not in a state with specific Esc handling, Esc quits
                app.quit();
                return None; // No action needed, just quit
            }
        }
        _ => {} // Other keys are handled by state/mode
    }


    match app.current_state {
        AppState::Browsing => handle_browsing_input(app, key_event),
        AppState::AskingDownloadDir => handle_asking_download_dir_input(app, key_event), // This state implies filtering
        AppState::ViewingItem => handle_viewing_item_input(app, key_event),
        AppState::Downloading => {} // Ignore input during download for now
    }
    // Return the pending action, if any was set
    app.pending_action.clone()
}

/// Handles input when in the main browsing state (`AppState::Browsing`).
/// Dispatches to specific handlers based on whether the input field is being filtered.
fn handle_browsing_input(app: &mut App, key_event: KeyEvent) {
    if app.is_filtering_input {
        handle_browsing_input_filter_mode(app, key_event)
    } else {
        handle_browsing_input_navigate_mode(app, key_event)
    }
}

/// Handles key events when filtering the collection input field in Browsing state.
fn handle_browsing_input_filter_mode(app: &mut App, key_event: KeyEvent) {
    match key_event.code {
        // Esc is handled globally to exit filter mode
        // Ignore navigation/action keys first (Up/Down for list nav, 'i' to enter filter mode)
        KeyCode::Up | KeyCode::Down | KeyCode::Char('i') => {}

        // Then handle actual input editing keys
        KeyCode::Char(to_insert) => {
            app.enter_char(to_insert);
        }
        KeyCode::Backspace => {
            app.delete_char();
        }
        KeyCode::Left => {
            app.move_cursor_left();
        }
        KeyCode::Right => {
            app.move_cursor_right();
        }
        KeyCode::Enter => {
            // Submit search, set action, exit filter mode
            app.pending_action = Some(UpdateAction::FetchCollection);
            app.current_collection_name = Some(app.collection_input.clone()); // Store collection name
            app.items.clear(); // Clear old items
            app.list_state.select(None); // Reset selection
            app.error_message = None; // Clear previous errors
            app.is_filtering_input = false; // Switch to navigate mode
        }
        // Ignore other keys not handled above
        _ => {}
    }
}

/// Handles key events when navigating the item list in Browsing state.
fn handle_browsing_input_navigate_mode(app: &mut App, key_event: KeyEvent) {
     match key_event.code {
        // List navigation
        KeyCode::Down => {
            app.select_next_item();
        }
        KeyCode::Up => {
            app.select_previous_item();
        }
        // Enter filter mode
        KeyCode::Char('i') => {
            app.is_filtering_input = true;
        }
        // View selected item or enter filter mode if none selected
        KeyCode::Enter => {
            if let Some(selected_index) = app.list_state.selected() {
                 if let Some(item) = app.items.get(selected_index) {
                    app.viewing_item_id = Some(item.identifier.clone());
                    app.current_state = AppState::ViewingItem;
                    app.is_filtering_input = false; // Ensure not filtering when viewing
                    app.error_message = None; // Clear any previous message
                    app.current_item_details = None; // Clear previous details
                    app.file_list_state = ListState::default(); // Reset file list selection
                    app.is_loading_details = true; // Set flag
                    app.pending_action = Some(UpdateAction::FetchItemDetails); // Set action
                 }
            } else {
                // If no item is selected, Enter goes to filter mode
                app.is_filtering_input = true;
            }
        }
        // Download trigger
        KeyCode::Char('d') => {
            if app.list_state.selected().is_some() { // Only if an item is selected
                if app.settings.download_directory.is_none() {
                    // No download directory set, prompt the user
                    app.current_state = AppState::AskingDownloadDir;
                    app.collection_input.clear(); // Reuse input field for dir path
                    app.cursor_position = 0;
                    app.error_message = None; // Clear any previous errors
                    app.is_filtering_input = true; // Asking for dir implies filtering input
                } else if let Some(selected_index) = app.list_state.selected() {
                    // Directory is set, trigger download for the selected item
                    if let Some(item) = app.items.get(selected_index) {
                         app.pending_action = Some(UpdateAction::StartDownload(DownloadAction::Item(item.identifier.clone())));
                         app.download_status = Some(format!("Queueing download for item: {}", item.identifier));
                         // Main loop will set is_downloading = true when task starts
                    }
                } else {
                    app.error_message = Some("Select an item to download first.".to_string());
                }
            } else {
                 app.error_message = Some("Select an item to download first.".to_string()); // Should not happen if list_state has selection
            }
        }
        // Ignore input filtering keys while navigating
        KeyCode::Char(c) if c != 'i' && c != 'd' && c != 'q' => {} // Ignore other chars not handled above
        KeyCode::Backspace | KeyCode::Left | KeyCode::Right => {}
        // Esc and Quit keys are handled globally or by state handlers
        _ => {} // Ignore other keys
    }
    // No return value needed here anymore
}

/// Handles input when prompting for the download directory.
fn handle_asking_download_dir_input(app: &mut App, key_event: KeyEvent) {
     match key_event.code {
        KeyCode::Esc => {
            // Cancel entering download dir and return to browsing (navigate mode)
            app.current_state = AppState::Browsing;
            app.collection_input.clear(); // Clear the potentially partial path
            app.cursor_position = 0;
            app.error_message = None;
            app.is_filtering_input = false; // Ensure we return to navigate mode
        }
        KeyCode::Char(to_insert) => {
            // Use the same input logic as collection input (this state implies filtering)
            app.enter_char(to_insert);
        }
        KeyCode::Backspace => {
            app.delete_char();
        }
        KeyCode::Left => {
            app.move_cursor_left();
        }
        KeyCode::Right => {
            app.move_cursor_right();
        }
        KeyCode::Enter => {
            // Save the entered path as the download directory
            let entered_path = app.collection_input.trim().to_string();
            if !entered_path.is_empty() {
                // Basic validation: check if it looks like a path (optional, could be more robust)
                // For now, just save what was entered. Consider adding path validation/creation.
                app.settings.download_directory = Some(entered_path);
                if let Err(e) = settings::save_settings(&app.settings) {
                     app.error_message = Some(format!("Failed to save settings: {}", e));
                     // Stay in AskingDownloadDir state on save error? Or revert? Reverting for now.
                     app.settings.download_directory = None; // Revert in-memory setting
                     // Stay in AskingDownloadDir state on save error
                } else {
                    app.error_message = Some("Download directory saved. Press 'd' again to download.".to_string());
                    app.current_state = AppState::Browsing; // Return to browsing
                    app.collection_input.clear(); // Clear the path from input
                    app.cursor_position = 0;
                    app.is_filtering_input = false; // Ensure we return to navigate mode
                }
            } else {
                app.error_message = Some("Download directory cannot be empty. Press Esc to cancel.".to_string());
                // Stay in AskingDownloadDir state
            }
        }
        _ => {} // Ignore other keys
    }
}

/// Handles input when viewing item details.
fn handle_viewing_item_input(app: &mut App, key_event: KeyEvent) {
    match key_event.code {
        KeyCode::Esc => {
            // Go back to browsing navigate mode
            app.current_state = AppState::Browsing;
            app.viewing_item_id = None; // Clear the viewed item ID
            app.current_item_details = None; // Clear details
            app.file_list_state = ListState::default(); // Reset file list state
            app.is_filtering_input = false; // Ensure back in navigate mode
            app.error_message = None;
        }
        KeyCode::Down => {
            app.select_next_file();
        }
        KeyCode::Up => {
            app.select_previous_file();
        }
        KeyCode::Enter | KeyCode::Char('d') => {
            // Download selected file
            if app.settings.download_directory.is_none() {
                // No download directory set, prompt the user
                app.current_state = AppState::AskingDownloadDir;
                app.collection_input.clear(); // Reuse input field for dir path
                app.cursor_position = 0;
                app.error_message = None; // Clear any previous errors
                app.is_filtering_input = true; // Asking for dir implies filtering input
            } else if let Some(file_details) = app.get_selected_file().cloned() { // Clone details
                 if let Some(item_id) = app.viewing_item_id.clone() {
                    app.pending_action = Some(UpdateAction::StartDownload(DownloadAction::File(item_id, file_details.clone())));
                    app.download_status = Some(format!("Queueing download for file: {}", file_details.name));
                 }
            } else {
                app.error_message = Some("Select a file to download first.".to_string());
            }
        }
        _ => {} // Ignore other keys for now
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    // Settings struct itself is not directly used here, only functions from settings module
    use crate::app::App;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use std::{env, fs}; // For test setup
    use tempfile::tempdir; // For test setup

    // Helper for setting up test environment with mock config
    fn setup_test_app_with_config() -> (App, tempfile::TempDir) {
        let temp_dir = tempdir().unwrap();
        let mock_home = temp_dir.path().to_path_buf();
        env::set_var("HOME", mock_home.to_str().unwrap()); // Mock HOME for ProjectDirs

        // Ensure the config dir exists for saving settings later if needed
        let config_dir = temp_dir.path().join(".config").join(crate::settings::APPLICATION);
        fs::create_dir_all(&config_dir).unwrap();


        let mut app = App::new();
        // Ensure settings are loaded (or defaults used) based on the mocked env
        app.settings = crate::settings::load_settings().unwrap();
        (app, temp_dir)
    }


    #[test]
    fn test_update_enter_key_triggers_api_call_and_exits_filter_mode_in_browsing() {
        let (mut app, _temp_dir) = setup_test_app_with_config();
        app.current_state = AppState::Browsing;
        app.is_filtering_input = true; // Start in filter mode
        // Simulate some existing state
        app.collection_input = "test_collection".to_string();
        app.items = vec![crate::archive_api::ArchiveDoc { identifier: "item1".to_string() }];
        app.list_state.select(Some(0));
        app.error_message = Some("Previous error".to_string());

        let key_event = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);

        // Act
        let should_trigger_api = update(&mut app, key_event);

        // Assert
        assert!(should_trigger_api, "Enter key should trigger an API call");
        assert!(app.items.is_empty(), "Items should be cleared");
        assert!(app.list_state.selected().is_none(), "List selection should be reset");
        assert!(app.error_message.is_none(), "Error message should be cleared");
        assert_eq!(app.current_state, AppState::Browsing, "State should remain Browsing");
        assert!(!app.is_filtering_input, "Should exit input filtering mode");
    }

     #[test]
    fn test_update_enter_key_enters_filter_mode_when_navigating_and_no_item_selected() {
        let (mut app, _temp_dir) = setup_test_app_with_config();
        app.current_state = AppState::Browsing;
        app.is_filtering_input = false; // Start in navigate mode
        app.list_state.select(None); // Ensure nothing selected

        let key_event = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
        let should_trigger_api = update(&mut app, key_event);

        assert!(!should_trigger_api, "Enter should not trigger API call");
        assert!(app.is_filtering_input, "Should enter input filtering mode");
        assert_eq!(app.current_state, AppState::Browsing); // Should stay in browsing state
    }

     #[test]
    fn test_update_enter_key_enters_viewing_mode_when_navigating_and_item_selected() {
        let (mut app, _temp_dir) = setup_test_app_with_config();
        app.current_state = AppState::Browsing;
        app.is_filtering_input = false; // Start in navigate mode
        app.items = vec![crate::archive_api::ArchiveDoc { identifier: "item1".to_string() }];
        app.list_state.select(Some(0)); // Select the item

        let key_event = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
        let should_trigger_api = update(&mut app, key_event);

        assert!(!should_trigger_api, "Enter should not trigger API call");
        assert!(!app.is_filtering_input, "Should not be filtering");
        assert_eq!(app.current_state, AppState::ViewingItem, "Should enter ViewingItem state");
        assert_eq!(app.viewing_item_id, Some("item1".to_string()));
    }


     #[test]
    fn test_update_quit_keys_in_browsing() {
        let (mut app, _temp_dir) = setup_test_app_with_config();
        app.current_state = AppState::Browsing;
        assert!(app.running);

        // Test 'q'
        update(&mut app, KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE));
        assert!(!app.running, "App should not be running after 'q'");

        // Reset and test Esc (should quit if not filtering)
        app.running = true;
        app.current_state = AppState::Browsing;
        app.is_filtering_input = false; // Ensure not filtering
        update(&mut app, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(!app.running, "App should not be running after Esc when navigating");

        // Reset and test Ctrl+C
        app.running = true;
        app.current_state = AppState::Browsing; // Reset state too
        update(&mut app, KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));
        assert!(!app.running, "App should not be running after Ctrl+C");
    }

     #[test]
    fn test_update_esc_in_asking_download_dir_reverts_state() {
        let (mut app, _temp_dir) = setup_test_app_with_config();
        app.current_state = AppState::AskingDownloadDir;
        app.collection_input = "/some/path".to_string(); // Simulate partial input

        update(&mut app, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));

        assert_eq!(app.current_state, AppState::Browsing, "State should revert to Browsing");
        assert!(app.collection_input.is_empty(), "Input should be cleared");
        assert!(app.error_message.is_none(), "Error message should be cleared");
        assert!(!app.is_filtering_input, "Should exit input filtering mode");
    }


     #[test]
    fn test_update_esc_exits_filter_mode_in_browsing() {
        let (mut app, _temp_dir) = setup_test_app_with_config();
        app.current_state = AppState::Browsing;
        app.is_filtering_input = true; // Start filtering

        update(&mut app, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));

        assert!(app.running, "App should still be running");
        assert!(!app.is_filtering_input, "Should exit input filtering mode");
        assert_eq!(app.current_state, AppState::Browsing); // State remains Browsing
    }

     #[test]
    fn test_update_esc_exits_viewing_item_mode() {
        let (mut app, _temp_dir) = setup_test_app_with_config();
        app.current_state = AppState::ViewingItem;
        app.viewing_item_id = Some("item1".to_string());
        app.is_filtering_input = false; // Should be false when viewing

        update(&mut app, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));

        assert!(app.running, "App should still be running");
        assert_eq!(app.current_state, AppState::Browsing, "Should return to Browsing state");
        assert!(!app.is_filtering_input, "Should be in navigate mode");
        assert!(app.viewing_item_id.is_none(), "Viewing item ID should be cleared");
    }


    #[test]
    fn test_update_list_navigation_only_when_navigating_in_browsing() {
        let (mut app, _temp_dir) = setup_test_app_with_config();
        app.current_state = AppState::Browsing;
        app.is_filtering_input = false; // Start in navigate mode
        app.items = vec![
            crate::archive_api::ArchiveDoc { identifier: "item1".to_string() },
            crate::archive_api::ArchiveDoc { identifier: "item2".to_string() },
            crate::archive_api::ArchiveDoc { identifier: "item3".to_string() },
        ];

        // Initial state: nothing selected
        assert_eq!(app.list_state.selected(), None);

        // Press Down
        update(&mut app, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        assert_eq!(app.list_state.selected(), Some(0));

        // Press Down again
        update(&mut app, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        assert_eq!(app.list_state.selected(), Some(1));

        // Press Up
        update(&mut app, KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
        assert_eq!(app.list_state.selected(), Some(0));

        // Press Up (wraps around)
        update(&mut app, KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
        assert_eq!(app.list_state.selected(), Some(2));

         // Press Down (wraps around)
        update(&mut app, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        assert_eq!(app.list_state.selected(), Some(0));

        // Switch to filter mode and try navigating - should be ignored
        app.is_filtering_input = true;
        let initial_selection = app.list_state.selected();
        update(&mut app, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        assert_eq!(app.list_state.selected(), initial_selection, "Down key should be ignored when filtering");
        update(&mut app, KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
        assert_eq!(app.list_state.selected(), initial_selection, "Up key should be ignored when filtering");

    }

     #[test]
    fn test_update_input_handling_only_when_filtering_in_browsing() {
        let (mut app, _temp_dir) = setup_test_app_with_config();
        app.current_state = AppState::Browsing;
        app.is_filtering_input = true; // Start filtering

        // Enter 'a'
        update(&mut app, KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE));
        assert_eq!(app.collection_input, "a");
        assert_eq!(app.cursor_position, 1);

        // Enter 'b'
        update(&mut app, KeyEvent::new(KeyCode::Char('b'), KeyModifiers::NONE));
        assert_eq!(app.collection_input, "ab");
        assert_eq!(app.cursor_position, 2);

        // Move Left
        update(&mut app, KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
        assert_eq!(app.cursor_position, 1);

        // Enter 'c'
        update(&mut app, KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE));
        assert_eq!(app.collection_input, "acb");
        assert_eq!(app.cursor_position, 2);

        // Move Right
        update(&mut app, KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
        assert_eq!(app.cursor_position, 3);

        // Backspace
        update(&mut app, KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));
        assert_eq!(app.collection_input, "ac");
        assert_eq!(app.cursor_position, 2);

        // Backspace again
        update(&mut app, KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));
        assert_eq!(app.collection_input, "a");
        assert_eq!(app.cursor_position, 1);

         // Backspace at start
        update(&mut app, KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
        assert_eq!(app.cursor_position, 0);
        update(&mut app, KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));
        assert_eq!(app.collection_input, "a"); // No change
        assert_eq!(app.cursor_position, 0);
        assert_eq!(app.current_state, AppState::Browsing); // State unchanged
        assert!(app.is_filtering_input, "Should still be filtering");

        // Switch to navigate mode and try typing - should be ignored
        app.is_filtering_input = false;
        let initial_input = app.collection_input.clone();
        let initial_cursor = app.cursor_position;

        update(&mut app, KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));
        assert_eq!(app.collection_input, initial_input, "Char 'x' should be ignored when navigating");
        assert_eq!(app.cursor_position, initial_cursor, "Cursor should not move");

        update(&mut app, KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));
         assert_eq!(app.collection_input, initial_input, "Backspace should be ignored when navigating");
        assert_eq!(app.cursor_position, initial_cursor, "Cursor should not move");

        update(&mut app, KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
        assert_eq!(app.cursor_position, initial_cursor, "Left key should be ignored when navigating");

        update(&mut app, KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
        assert_eq!(app.cursor_position, initial_cursor, "Right key should be ignored when navigating");

    }

     #[test]
    fn test_update_i_key_enters_filter_mode_when_navigating_in_browsing() {
        let (mut app, _temp_dir) = setup_test_app_with_config();
        app.current_state = AppState::Browsing;
        app.is_filtering_input = false; // Start navigating

        update(&mut app, KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE));

        assert!(app.is_filtering_input, "Should enter input filtering mode");
        assert_eq!(app.current_state, AppState::Browsing);
    }


    #[test]
    fn test_update_download_key_no_dir_set_changes_state_when_navigating() {
        let (mut app, _temp_dir) = setup_test_app_with_config();
        app.current_state = AppState::Browsing;
        app.is_filtering_input = false; // Must be navigating
        app.settings.download_directory = None; // Ensure no dir is set
        app.items = vec![crate::archive_api::ArchiveDoc { identifier: "item1".to_string() }];
        app.list_state.select(Some(0)); // Select an item

        let should_trigger_api = update(&mut app, KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE));

        assert!(!should_trigger_api);
        assert_eq!(app.current_state, AppState::AskingDownloadDir, "State should change to AskingDownloadDir");
        assert!(app.collection_input.is_empty(), "Input field should be cleared for new input");
        assert_eq!(app.cursor_position, 0);
        assert!(app.error_message.is_none());
        assert!(app.is_filtering_input, "Should switch to filtering mode for AskingDownloadDir");
    }

     #[test]
    fn test_update_download_key_ignored_when_filtering() {
        let (mut app, _temp_dir) = setup_test_app_with_config();
        app.current_state = AppState::Browsing;
        app.is_filtering_input = true; // Filtering mode
        app.settings.download_directory = None;
        app.items = vec![crate::archive_api::ArchiveDoc { identifier: "item1".to_string() }];
        app.list_state.select(Some(0));

        let should_trigger_api = update(&mut app, KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE));

        assert!(!should_trigger_api);
        assert_eq!(app.current_state, AppState::Browsing); // State should not change
        assert!(app.is_filtering_input); // Should remain filtering
    }


     #[test]
    fn test_update_download_key_no_item_selected_when_navigating() {
        let (mut app, _temp_dir) = setup_test_app_with_config();
        app.current_state = AppState::Browsing;
        app.is_filtering_input = false; // Navigating mode
        app.settings.download_directory = Some("/tmp/test".to_string()); // Dir is set
        app.items = vec![crate::archive_api::ArchiveDoc { identifier: "item1".to_string() }];
        app.list_state.select(None); // No item selected

        let should_trigger_api = update(&mut app, KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE));

        assert!(!should_trigger_api);
        assert_eq!(app.current_state, AppState::Browsing); // State remains Browsing
        assert!(app.error_message.is_some());
        assert!(app.error_message.unwrap().contains("Select an item"));
    }


    #[test]
    fn test_update_download_key_dir_set_triggers_placeholder_when_navigating() {
        let (mut app, _temp_dir) = setup_test_app_with_config();
        app.current_state = AppState::Browsing;
        app.is_filtering_input = false; // Navigating mode
        app.settings.download_directory = Some("/tmp/test".to_string()); // Dir is set
        app.items = vec![crate::archive_api::ArchiveDoc { identifier: "item1".to_string() }];
        app.list_state.select(Some(0)); // Select an item

        let should_trigger_api = update(&mut app, KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE));

        assert!(!should_trigger_api);
        assert_eq!(app.current_state, AppState::Browsing); // State remains Browsing (until download starts)
        assert!(app.error_message.is_some()); // Placeholder message is set
        assert!(app.error_message.unwrap().contains("Download started"));
        // Later: Assert that state changes to Downloading and a task is spawned
    }

    #[test]
    fn test_update_asking_download_dir_input_and_save() {
        let (mut app, _temp_dir) = setup_test_app_with_config();
        app.current_state = AppState::AskingDownloadDir;
        app.settings.download_directory = None; // Start with no dir set

        // Simulate typing a path
        update(&mut app, KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
        update(&mut app, KeyEvent::new(KeyCode::Char('t'), KeyModifiers::NONE));
        update(&mut app, KeyEvent::new(KeyCode::Char('m'), KeyModifiers::NONE));
        update(&mut app, KeyEvent::new(KeyCode::Char('p'), KeyModifiers::NONE));
        assert_eq!(app.collection_input, "/tmp");
        assert_eq!(app.current_state, AppState::AskingDownloadDir);

        // Press Enter to save
        let should_trigger_api = update(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

        assert!(!should_trigger_api);
        assert_eq!(app.current_state, AppState::Browsing, "State should revert to Browsing after save");
        assert!(app.collection_input.is_empty(), "Input field should be cleared");
        assert_eq!(app.settings.download_directory, Some("/tmp".to_string()), "Download directory should be saved in app state");
        assert!(!app.is_filtering_input, "Should exit input filtering mode after save");
        assert!(app.error_message.is_some());
        assert!(app.error_message.unwrap().contains("Download directory saved"));

        // Verify it was saved to file by reloading
        let reloaded_settings = crate::settings::load_settings().unwrap();
        assert_eq!(reloaded_settings.download_directory, Some("/tmp".to_string()), "Download directory should persist in settings file");
    }

     #[test]
    fn test_update_asking_download_dir_enter_empty_shows_error() {
        let (mut app, _temp_dir) = setup_test_app_with_config();
        app.current_state = AppState::AskingDownloadDir;
        app.collection_input.clear(); // Ensure input is empty

        update(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

        assert_eq!(app.current_state, AppState::AskingDownloadDir, "State should remain AskingDownloadDir");
        assert!(app.error_message.is_some());
        assert!(app.error_message.unwrap().contains("cannot be empty"));
        assert!(app.settings.download_directory.is_none(), "Download directory should not be set");
    }
}
