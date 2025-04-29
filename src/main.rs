use anyhow::Result;
use rust_tui_app::{app::App, event::EventHandler, tui::Tui, update::update};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io;

#[tokio::main]
async fn main() -> Result<()> {
    // Create an application.
    let mut app = App::new();

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
        // Handle events.
        match tui.events.next().await? {
            rust_tui_app::event::Event::Tick => app.tick(),
            rust_tui_app::event::Event::Key(key_event) => update(&mut app, key_event),
            rust_tui_app::event::Event::Mouse(_) => {}
            rust_tui_app::event::Event::Resize(_, _) => {}
        }
    }

    // Exit the user interface.
    tui.exit()?;
    Ok(())
}
