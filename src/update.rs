use crate::app::{App, AppState}; // Import AppState
use crate::settings; // Import settings module
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// Handle key events based on the current application state.
/// Returns `true` if a collection search API call should be triggered.
pub fn update(app: &mut App, key_event: KeyEvent) -> bool {
    match app.current_state {
        AppState::Browsing => handle_browsing_input(app, key_event),
        AppState::AskingDownloadDir => handle_asking_download_dir_input(app, key_event),
        AppState::Downloading => false, // Ignore input during download for now
    }
}

/// Handles input when in the main browsing state.
fn handle_browsing_input(app: &mut App, key_event: KeyEvent) -> bool {
    let mut trigger_api_call = false;
    match key_event.code {
        // Basic navigation and exit
        KeyCode::Esc => app.quit(), // Allow Esc to always quit
        KeyCode::Char('q') => app.quit(),
        KeyCode::Char('c') | KeyCode::Char('C') if key_event.modifiers == KeyModifiers::CONTROL => {
            app.quit()
        }

        // Collection Input field handling
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
            // Signal that the main loop should trigger the API call
            trigger_api_call = true;
            app.items.clear(); // Clear old items
            app.list_state.select(None); // Reset selection
            app.error_message = None; // Clear previous errors
        }

        // List navigation
        KeyCode::Down => {
            app.select_next_item();
        }
        KeyCode::Up => {
            app.select_previous_item();
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
                } else {
                    // Directory is set, trigger download (logic to be added later)
                    println!("Download triggered for selected item!"); // Placeholder
                    // TODO: Set state to Downloading and trigger async download task
                    app.error_message = Some("Download started (placeholder)...".to_string()); // Temp feedback
                }
            } else {
                 app.error_message = Some("Select an item to download first.".to_string());
            }
        }


        _ => {} // Ignore other keys
    };
    trigger_api_call
}

/// Handles input when prompting for the download directory.
fn handle_asking_download_dir_input(app: &mut App, key_event: KeyEvent) -> bool {
     match key_event.code {
        KeyCode::Esc => {
            // Cancel entering download dir and return to browsing
            app.current_state = AppState::Browsing;
            app.collection_input.clear(); // Clear the potentially partial path
            app.cursor_position = 0;
            app.error_message = None;
        }
        KeyCode::Char(to_insert) => {
            // Use the same input logic as collection input
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
                } else {
                    app.error_message = Some("Download directory saved. Press 'd' again to download.".to_string());
                    app.current_state = AppState::Browsing; // Return to browsing
                    app.collection_input.clear(); // Clear the path from input
                    app.cursor_position = 0;
                }
            } else {
                app.error_message = Some("Download directory cannot be empty. Press Esc to cancel.".to_string());
            }
        }
        _ => {} // Ignore other keys
    }
    false // Never trigger collection search from this state
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{App, AppState}; // Import AppState
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    #[test]
    fn test_update_enter_key_triggers_api_call_and_resets_state() {
        let mut app = App::new();
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
    }

     #[test]
    fn test_update_quit_keys_in_browsing() {
        let (mut app, _temp_dir) = setup_test_app_with_config();
        app.current_state = AppState::Browsing;
        assert!(app.running);

        // Test 'q'
        update(&mut app, KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE));
        assert!(!app.running, "App should not be running after 'q'");

        // Reset and test Esc
        app.running = true;
        app.current_state = AppState::Browsing; // Reset state too
        update(&mut app, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(!app.running, "App should not be running after Esc");

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
    }


    #[test]
    fn test_update_list_navigation_in_browsing() {
        let (mut app, _temp_dir) = setup_test_app_with_config();
        app.current_state = AppState::Browsing;
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
    }

     #[test]
    fn test_update_input_handling() {
        let mut app = App::new();

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
    }

    #[test]
    fn test_update_download_key_no_dir_set_changes_state() {
        let (mut app, _temp_dir) = setup_test_app_with_config();
        app.current_state = AppState::Browsing;
        app.settings.download_directory = None; // Ensure no dir is set
        app.items = vec![crate::archive_api::ArchiveDoc { identifier: "item1".to_string() }];
        app.list_state.select(Some(0)); // Select an item

        let should_trigger_api = update(&mut app, KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE));

        assert!(!should_trigger_api);
        assert_eq!(app.current_state, AppState::AskingDownloadDir, "State should change to AskingDownloadDir");
        assert!(app.collection_input.is_empty(), "Input field should be cleared for new input");
        assert_eq!(app.cursor_position, 0);
        assert!(app.error_message.is_none());
    }

     #[test]
    fn test_update_download_key_no_item_selected() {
        let (mut app, _temp_dir) = setup_test_app_with_config();
        app.current_state = AppState::Browsing;
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
    fn test_update_download_key_dir_set_triggers_placeholder() {
        let (mut app, _temp_dir) = setup_test_app_with_config();
        app.current_state = AppState::Browsing;
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
