use crate::archive_api::{ArchiveDoc, FileDetails, ItemDetails};
use crate::settings::Settings;
use ratatui::widgets::ListState;
use std::path::PathBuf; // For constructing download paths
use reqwest::Client;

/// Represents the different states or modes the application can be in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AppState {
    /// Normal operation: browsing collections and items.
    Browsing,
    /// Prompting the user to enter the download directory.
    AskingDownloadDir,
    /// Viewing the details of a selected item.
    ViewingItem,
    /// Currently downloading an item (future state).
    Downloading, // Placeholder for later
    /// Viewing/editing application settings.
    SettingsView,
    /// Actively editing a specific setting value.
    EditingSetting,
}


/// Application state
pub struct App {
    /// Current application state/mode.
    pub current_state: AppState,
    /// Loaded application settings.
    pub settings: Settings,
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
    /// Flag to indicate if the user is currently filtering the collection input
    pub is_filtering_input: bool,
    /// Total number of items found in the last collection search
    pub total_items_found: Option<usize>,
    /// Identifier of the item currently being viewed (if any)
    pub viewing_item_id: Option<String>,
    /// Details of the item currently being viewed
    pub current_item_details: Option<ItemDetails>,
    /// State for the file list widget when viewing an item
    pub file_list_state: ListState,
    /// Flag indicating if item details are being loaded
    pub is_loading_details: bool,
    /// Name of the collection currently being browsed (for download path)
    pub current_collection_name: Option<String>,
    /// Flag indicating if a download is in progress
    pub is_downloading: bool,
    /// Status message for the current or last download
    pub download_status: Option<String>,
    /// Action requested by the user to be performed in the main loop
    pub pending_action: Option<UpdateAction>,

    // --- Download Progress State ---
    /// Total items to download in the current bulk operation (if applicable)
    pub total_items_to_download: Option<usize>,
    /// Number of items completed in the current bulk operation
    pub items_downloaded_count: usize,
    /// Total files to download across all items (estimated, updates as details are fetched)
    pub total_files_to_download: Option<usize>,
     /// Number of files completed in the current bulk operation
    pub files_downloaded_count: usize,

    // --- Settings State ---
    /// State for the settings list widget
    pub settings_list_state: ListState,
    /// Index of the currently selected setting (for editing)
    pub selected_setting_index: usize,
    /// Temporary buffer for editing a setting value
    pub editing_setting_input: String,
}

/// Actions that the main loop should perform based on user input.
#[derive(Clone, Debug)]
pub enum UpdateAction {
    FetchCollection,
    FetchItemDetails,
    StartDownload(DownloadAction),
}

/// Specifies what to download.
#[derive(Clone, Debug)]
pub enum DownloadAction {
    /// Download all files for a specific item (used by 'd' in item list or 'b' in file list).
    ItemAllFiles(String), // identifier
    /// Download a single specific file (used by Enter/'d' in file list).
    File(String, FileDetails), // identifier, file details
    /// Download all items currently listed in the collection browser.
    Collection,
}

/// Represents progress updates sent from download tasks.
#[derive(Debug, Clone)]
pub enum DownloadProgress {
    /// Started processing an item.
    ItemStarted(String),
    /// Determined the number of files for an item.
    ItemFileCount(usize),
    /// A single file download completed successfully.
    FileCompleted(String), // filename
    /// An item download finished (successfully or with partial failure).
    ItemCompleted(String, bool), // identifier, success (true if all files OK)
    /// The entire collection download attempt finished.
    CollectionCompleted(usize, usize), // total items attempted, total items failed
    /// An error occurred during download.
    Error(String),
    /// A general status message.
    Status(String),
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
            // Initialize with default state and settings (will be loaded properly in main)
            current_state: AppState::Browsing,
            settings: Settings::default(),
            is_filtering_input: true, // Start in input filtering mode
            total_items_found: None,
            viewing_item_id: None,
            current_item_details: None,
            file_list_state: ListState::default(),
            is_loading_details: false,
            current_collection_name: None,
            is_downloading: false,
            download_status: None,
            pending_action: None,
            total_items_to_download: None,
            items_downloaded_count: 0,
            total_files_to_download: None,
            files_downloaded_count: 0,
            settings_list_state: ListState::default(),
            selected_setting_index: 0, // Start with the first setting selected
            editing_setting_input: String::new(),
        }
    }

    /// Load settings into the App state.
    pub fn load_settings(&mut self, settings: Settings) {
        self.settings = settings;
        // If download dir is not set, maybe transition state immediately?
        // Or handle this transition based on user action (like pressing 'd').
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

    /// Clamps the cursor position within the valid range of characters in the input string.
    fn clamp_cursor(&self, new_cursor_pos: usize) -> usize {
        new_cursor_pos.clamp(0, self.collection_input.chars().count()) // Use chars().count() instead of len() for correct char boundary clamping
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

    // --- File List Navigation ---

    /// Selects the next file in the file list view.
    pub fn select_next_file(&mut self) {
        let file_count = self.current_item_details.as_ref().map_or(0, |d| d.files.len());
        if file_count == 0 {
            return;
        }
        let i = match self.file_list_state.selected() {
            Some(i) => {
                if i >= file_count - 1 { 0 } else { i + 1 }
            }
            None => 0,
        };
        self.file_list_state.select(Some(i));
    }

    /// Selects the previous file in the file list view.
    pub fn select_previous_file(&mut self) {
        let file_count = self.current_item_details.as_ref().map_or(0, |d| d.files.len());
        if file_count == 0 {
            return;
        }
        let i = match self.file_list_state.selected() {
            Some(i) => {
                if i == 0 { file_count - 1 } else { i - 1 }
            }
            None => 0, // Select the first item if nothing was selected
        };
        self.file_list_state.select(Some(i));
    }

    /// Gets the details of the currently selected file, if any.
    pub fn get_selected_file(&self) -> Option<&FileDetails> {
        match (self.file_list_state.selected(), &self.current_item_details) {
            (Some(index), Some(details)) => details.files.get(index),
            _ => None,
        }
    }

    /// Constructs the full download path for a given file.
    /// Returns None if download directory is not set or item details are missing.
    pub fn get_download_path_for_file(&self, file: &FileDetails) -> Option<PathBuf> {
        match (
            self.settings.download_directory.as_ref(),
            self.current_collection_name.as_ref(),
            self.viewing_item_id.as_ref(), // Use viewing_item_id which should be set
        ) {
            (Some(base_dir), Some(collection), Some(item_id)) => {
                let mut path = PathBuf::from(base_dir);
                path.push(collection);
                path.push(item_id);
                path.push(&file.name);
                Some(path)
            }
            _ => None, // Missing necessary info
        }
    }

     /// Constructs the directory path for a given item.
     /// Returns None if download directory is not set or collection/item ID is missing.
     pub fn get_download_path_for_item(&self) -> Option<PathBuf> {
         match (
             self.settings.download_directory.as_ref(),
             self.current_collection_name.as_ref(),
             self.viewing_item_id.as_ref(), // Use viewing_item_id for item downloads too
         ) {
             (Some(base_dir), Some(collection), Some(item_id)) => {
                 let mut path = PathBuf::from(base_dir);
                 path.push(collection);
                 path.push(item_id);
                 Some(path)
             }
             _ => None, // Missing necessary info
         }
     }
}
