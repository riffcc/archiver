use crate::app::App;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// Handle key events and update the application state.
/// Returns `true` if an API call should be triggered (e.g., Enter pressed in input).
pub fn update(app: &mut App, key_event: KeyEvent) -> bool {
    let mut trigger_api_call = false;
    match key_event.code {
        // Basic navigation and exit
        KeyCode::Esc | KeyCode::Char('q') => app.quit(),
        KeyCode::Char('c') | KeyCode::Char('C') if key_event.modifiers == KeyModifiers::CONTROL => {
            app.quit()
        }

        // Input field handling
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

        _ => {} // Ignore other keys for now
    };
    trigger_api_call
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::App;
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
    }

     #[test]
    fn test_update_quit_keys() {
        let mut app = App::new();
        assert!(app.running);

        // Test 'q'
        update(&mut app, KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE));
        assert!(!app.running, "App should not be running after 'q'");

        // Reset and test Esc
        app.running = true;
        update(&mut app, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(!app.running, "App should not be running after Esc");

        // Reset and test Ctrl+C
        app.running = true;
        update(&mut app, KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));
        assert!(!app.running, "App should not be running after Ctrl+C");
    }

    #[test]
    fn test_update_list_navigation() {
        let mut app = App::new();
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
    }
}
