/// Application state
pub struct App {
    /// Is the application running?
    pub running: bool,
    // Add other application state fields here
}

impl App {
    /// Constructs a new instance of [`App`].
    pub fn new() -> Self {
        Self { running: true }
    }

    /// Handles the tick event of the terminal.
    pub fn tick(&self) {
        // Placeholder for tick logic
    }

    /// Set running to false to quit the application.
    pub fn quit(&mut self) {
        self.running = false;
    }
}
