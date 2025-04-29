use crate::app::App;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// Handle key events and update the application state.
pub fn update(app: &mut App, key_event: KeyEvent) {
    match key_event.code {
        // Exit application on `ESC` or `q`
        KeyCode::Esc | KeyCode::Char('q') => app.quit(),
        // Exit application on `Ctrl-C`
        KeyCode::Char('c') | KeyCode::Char('C') => {
            if key_event.modifiers == KeyModifiers::CONTROL {
                app.quit();
            }
        }
        // Other handlers you could add here.
        _ => {}
    };
}
