use crate::archive_api::ArchiveDoc;
use ratatui::widgets::ListState;
use reqwest::Client;

/// Application state
pub struct App {
    /// Is the application running?
    pub running: bool,
    /// Current value of the input box
    pub collection_input: String,
    /// Position of cursor in the input box
    pub cursor_position: usize,
    /// Items fetched from the API
    pub items: Vec<ArchiveDoc>,
    /// State for the list widget (tracks selection)
    pub list_state: ListState,
    /// Reqwest client for making API calls
    pub client: Client,
    /// Optional error message to display
    pub error_message: Option<String>,
    /// Flag to indicate if we are currently fetching data
    pub is_loading: bool,
}

impl App {
    /// Constructs a new instance of [`App`].
    pub fn new() -> Self {
        Self {
            running: true,
            collection_input: String::new(),
            cursor_position: 0,
            items: Vec::new(),
            list_state: ListState::default(),
            client: Client::new(),
            error_message: None,
            is_loading: false,
        }
    }

    /// Handles the tick event of the terminal.
    pub fn tick(&self) {
        // Placeholder for tick logic
    }

    /// Set running to false to quit the application.
    pub fn quit(&mut self) {
        self.running = false;
    }

    pub fn move_cursor_left(&mut self) {
        let cursor_moved_left = self.cursor_position.saturating_sub(1);
        self.cursor_position = self.clamp_cursor(cursor_moved_left);
    }

    pub fn move_cursor_right(&mut self) {
        let cursor_moved_right = self.cursor_position.saturating_add(1);
        self.cursor_position = self.clamp_cursor(cursor_moved_right);
    }

    pub fn enter_char(&mut self, new_char: char) {
        self.collection_input
            .insert(self.cursor_position, new_char);
        self.move_cursor_right();
    }

    pub fn delete_char(&mut self) {
        let is_not_cursor_leftmost = self.cursor_position != 0;
        if is_not_cursor_leftmost {
            // Method "remove" is not supported on the stable toolchain
            let current_index = self.cursor_position;
            let from_left_to_current_index = current_index - 1;

            // Getting all characters before the selected character.
            let before_char_to_delete = self.collection_input.chars().take(from_left_to_current_index);
            // Getting all characters after selected character.
            let after_char_to_delete = self.collection_input.chars().skip(current_index);

            // Put all characters together except the selected one.
            // By leaving the selected one out, it is forgotten and therefore deleted.
            self.collection_input = before_char_to_delete.chain(after_char_to_delete).collect();
            self.move_cursor_left();
        }
    }

    fn clamp_cursor(&self, new_cursor_pos: usize) -> usize {
        new_cursor_pos.clamp(0, self.collection_input.len())
    }

    pub fn select_next_item(&mut self) {
        if self.items.is_empty() {
            return;
        }
        let i = match self.list_state.selected() {
            Some(i) => {
                if i >= self.items.len() - 1 {
                    0
                } else {
                    i + 1
                }
            }
            None => 0,
        };
        self.list_state.select(Some(i));
    }

    pub fn select_previous_item(&mut self) {
        if self.items.is_empty() {
            return;
        }
        let i = match self.list_state.selected() {
            Some(i) => {
                if i == 0 {
                    self.items.len() - 1
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.list_state.select(Some(i));
    }

    // Add methods to trigger API fetch, handle results, etc.
    // We'll integrate this with main.rs later.
}
