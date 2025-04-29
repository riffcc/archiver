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
