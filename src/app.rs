use crate::archive_api::{ArchiveDoc, FileDetails, ItemDetails};
use crate::settings::Settings;
// Use SystemClock and align middleware Instant type
use anyhow::{Context, Result}; // Add Result
use governor::{RateLimiter, clock::SystemClock, state::{InMemoryState, direct::NotKeyed}, middleware::NoOpMiddleware}; // Corrected InMemoryState path
use ratatui::widgets::ListState;
use reqwest::Client;
use serde_json; // Add serde_json
// Import SystemTime to match SystemClock
use std::{fs, path::{Path, PathBuf}, sync::Arc, time::{Instant, Duration, SystemTime}}; // Add fs, Path

/// Type alias for the specific RateLimiter used in the app
// Use SystemClock, the public InMemoryState path, and SystemTime for middleware
pub type AppRateLimiter = Arc<RateLimiter<NotKeyed, InMemoryState, SystemClock, NoOpMiddleware<SystemTime>>>;

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
    /// Adding a new collection to favorites.
    AddingCollection,
}

/// Indicates which pane is currently active/focused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ActivePane {
    Collections,
    Items,
}


/// Application state
pub struct App {
    /// Current application state/mode.
    pub current_state: AppState,
    /// Which pane has focus in the Browsing state.
    pub active_pane: ActivePane,
    /// Loaded application settings.
    pub settings: Settings,
    /// Is the application running?
    pub running: bool,
    // Removed: collection_input, cursor_position (replaced by specific input states)
    // Removed: is_filtering_input (replaced by specific input states)
    /// Items fetched from the API for the currently selected collection
    pub items: Vec<ArchiveDoc>,
    /// State for the collection list widget
    pub collection_list_state: ListState,
    /// State for the item list widget (tracks selection) - Renamed from list_state
    pub item_list_state: ListState,
    /// Reqwest client for making API calls
    pub client: Client,
    /// Optional error message to display
    pub error_message: Option<String>,
    /// Flag to indicate if we are currently fetching items for a collection
    pub is_loading: bool,
    /// Total number of items found in the last item search for the selected collection
    pub total_items_found: Option<usize>,
    /// Identifier of the item currently being viewed (if any)
    pub viewing_item_id: Option<String>,
    /// Details of the item currently being viewed
    pub current_item_details: Option<ItemDetails>,
    /// State for the file list widget when viewing an item
    pub file_list_state: ListState,
    /// Flag indicating if item details are being loaded
    pub is_loading_details: bool,
    /// Name of the collection currently selected and being browsed
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
    /// Total bytes downloaded in the current operation
    pub total_bytes_downloaded: u64,
    /// Start time of the current download operation
    pub download_start_time: Option<Instant>,


    // --- Settings State ---
    /// State for the settings list widget
    pub settings_list_state: ListState,
    /// Index of the currently selected setting (for editing)
    pub selected_setting_index: usize,
    /// Temporary buffer for editing a setting value (used for Download Dir and AskingDownloadDir)
    pub editing_setting_input: String,
    /// Cursor position for the editing_setting_input buffer
    pub cursor_position: usize, // Reusing cursor_position for editing setting / asking dir

    // --- Add Collection State ---
    /// Temporary buffer for adding a new collection
    pub add_collection_input: String,
    /// Cursor position for the add collection input
    pub add_collection_cursor_pos: usize,
    /// Shared global rate limiter for API calls and downloads
    pub rate_limiter: AppRateLimiter,
}

/// Actions that the main loop should perform based on user input or events.
#[derive(Clone, Debug)]
pub enum UpdateAction {
    /// Start fetching items incrementally for a collection identifier.
    StartIncrementalItemFetch(String),
    /// Fetch details for the currently selected item.
    FetchItemDetails,
    /// Start a download operation.
    StartDownload(DownloadAction),
    /// Save the current settings (e.g., after adding/removing a collection or exiting settings).
    SaveSettings,
}

/// Specifies what to download.
#[derive(Clone, Debug)]
pub enum DownloadAction {
    /// Download all files for a specific item.
    ItemAllFiles(String), // item_identifier
    /// Download a single specific file.
    File(String, FileDetails), // item_identifier, file details
    /// Download all items for a specific collection identifier.
    Collection(String), // collection_identifier
    // Maybe add CollectionAllFavorites later
}

/// Represents progress updates sent from download tasks.
#[derive(Debug, Clone)]
pub enum DownloadProgress {
    /// Information about the collection download starting.
    CollectionInfo(usize), // total items
    /// Started processing an item.
    ItemStarted(String),
    /// Determined the number of files for an item.
    ItemFileCount(usize),
    /// A chunk of bytes was downloaded for a file.
    BytesDownloaded(u64),
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
    pub fn new(rate_limiter: AppRateLimiter) -> Self { // Accept rate limiter
        // Configure Reqwest client with increased timeouts
        let client = Client::builder()
            .timeout(Duration::from_secs(60)) // General request timeout (increased to 60s)
            .connect_timeout(Duration::from_secs(60)) // Connection timeout (increased to 60s)
            .build()
            .unwrap_or_else(|_| Client::new()); // Fallback to default if builder fails

        Self {
            running: true,
            // Removed: collection_input, is_filtering_input
            items: Vec::new(),
            collection_list_state: ListState::default(), // Initialize collection list state
            item_list_state: ListState::default(), // Rename list_state to item_list_state
            client, // Use the configured client
            error_message: None,
            is_loading: false,
            // Initialize with default state and settings (will be loaded properly in main)
            current_state: AppState::Browsing,
            active_pane: ActivePane::Collections, // Start with collections pane active
            settings: Settings::default(),
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
            total_bytes_downloaded: 0,
            download_start_time: None,
            settings_list_state: ListState::default(),
            selected_setting_index: 0, // Start with the first setting selected
            editing_setting_input: String::new(),
            cursor_position: 0, // Initialize cursor for editing setting / asking dir
            add_collection_input: String::new(), // Initialize add collection input
            add_collection_cursor_pos: 0, // Initialize add collection cursor
            rate_limiter, // Store the passed rate limiter
        }
    }

    /// Load settings into the App state.
    pub fn load_settings(&mut self, settings: Settings) {
        self.settings = settings;
        // Select the first collection if the list is not empty after loading
        if !self.settings.favorite_collections.is_empty() {
            self.collection_list_state.select(Some(0));
            // Optionally trigger fetch for the first collection? Maybe not automatically.
        } else {
            self.collection_list_state.select(None); // Ensure nothing selected if list is empty
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

    // --- Input Handling Helpers (Adapted for different input fields) ---

    // Helper for editing_setting_input (used for Settings Edit & AskingDownloadDir)
    pub fn move_cursor_left_edit_setting(&mut self) {
        let cursor_moved_left = self.cursor_position.saturating_sub(1);
        self.cursor_position = self.clamp_cursor_edit_setting(cursor_moved_left);
    }

    pub fn move_cursor_right_edit_setting(&mut self) {
        let cursor_moved_right = self.cursor_position.saturating_add(1);
        self.cursor_position = self.clamp_cursor_edit_setting(cursor_moved_right);
    }

    pub fn enter_char_edit_setting(&mut self, new_char: char) {
        self.editing_setting_input.insert(self.cursor_position, new_char);
        self.move_cursor_right_edit_setting();
    }

    pub fn delete_char_edit_setting(&mut self) {
        let is_not_cursor_leftmost = self.cursor_position != 0;
        if is_not_cursor_leftmost {
            let current_index = self.cursor_position;
            let from_left_to_current_index = current_index - 1;
            let before_char_to_delete = self.editing_setting_input.chars().take(from_left_to_current_index);
            let after_char_to_delete = self.editing_setting_input.chars().skip(current_index);
            self.editing_setting_input = before_char_to_delete.chain(after_char_to_delete).collect();
            self.move_cursor_left_edit_setting();
        }
    }

    fn clamp_cursor_edit_setting(&self, new_cursor_pos: usize) -> usize {
        new_cursor_pos.clamp(0, self.editing_setting_input.chars().count())
    }

    // Helper for add_collection_input
    pub fn move_cursor_left_add_collection(&mut self) {
        let cursor_moved_left = self.add_collection_cursor_pos.saturating_sub(1);
        self.add_collection_cursor_pos = self.clamp_cursor_add_collection(cursor_moved_left);
    }

    pub fn move_cursor_right_add_collection(&mut self) {
        let cursor_moved_right = self.add_collection_cursor_pos.saturating_add(1);
        self.add_collection_cursor_pos = self.clamp_cursor_add_collection(cursor_moved_right);
    }

    pub fn enter_char_add_collection(&mut self, new_char: char) {
        self.add_collection_input.insert(self.add_collection_cursor_pos, new_char);
        self.move_cursor_right_add_collection();
    }

    pub fn delete_char_add_collection(&mut self) {
        let is_not_cursor_leftmost = self.add_collection_cursor_pos != 0;
        if is_not_cursor_leftmost {
            let current_index = self.add_collection_cursor_pos;
            let from_left_to_current_index = current_index - 1;
            let before_char_to_delete = self.add_collection_input.chars().take(from_left_to_current_index);
            let after_char_to_delete = self.add_collection_input.chars().skip(current_index);
            self.add_collection_input = before_char_to_delete.chain(after_char_to_delete).collect();
            self.move_cursor_left_add_collection();
        }
    }

    fn clamp_cursor_add_collection(&self, new_cursor_pos: usize) -> usize {
        new_cursor_pos.clamp(0, self.add_collection_input.chars().count())
    }


    // --- Collection List Navigation & Management ---

    /// Selects the next collection in the favorite collections list.
    pub fn select_next_collection(&mut self) {
        let count = self.settings.favorite_collections.len();
        if count == 0 {
            return;
        }
        let i = match self.collection_list_state.selected() {
            Some(i) => {
                if i >= count - 1 { 0 } else { i + 1 }
            }
            None => 0,
        };
        self.collection_list_state.select(Some(i));
    }

    /// Selects the previous collection in the favorite collections list.
    pub fn select_previous_collection(&mut self) {
        let count = self.settings.favorite_collections.len();
        if count == 0 {
            return;
        }
        let i = match self.collection_list_state.selected() {
            Some(i) => {
                if i == 0 { count - 1 } else { i - 1 }
            }
            None => 0,
        };
        self.collection_list_state.select(Some(i));
    }

    /// Gets the identifier of the currently selected collection, if any.
    pub fn get_selected_collection(&self) -> Option<&String> {
        match self.collection_list_state.selected() {
            Some(index) => self.settings.favorite_collections.get(index),
            None => None,
        }
    }

    /// Removes the currently selected collection from the favorites list.
    /// Returns true if a collection was removed, false otherwise.
    pub fn remove_selected_collection(&mut self) -> bool {
        if let Some(index) = self.collection_list_state.selected() {
            if index < self.settings.favorite_collections.len() {
                self.settings.favorite_collections.remove(index);
                // Adjust selection if the removed item was the last one
                let new_selection = if self.settings.favorite_collections.is_empty() {
                    None
                } else if index >= self.settings.favorite_collections.len() {
                    // If removed last item, select the new last item
                    Some(self.settings.favorite_collections.len() - 1)
                } else {
                    // Otherwise, keep selection at the same index
                    Some(index)
                };
                self.collection_list_state.select(new_selection);
                return true; // Indicate removal occurred
            }
        }
        false // Indicate nothing was removed
    }

    /// Adds a new collection identifier to the favorites list if it doesn't exist.
    pub fn add_collection_to_favorites(&mut self, identifier: String) {
        let trimmed_id = identifier.trim().to_string();
        if !trimmed_id.is_empty() && !self.settings.favorite_collections.contains(&trimmed_id) {
            self.settings.favorite_collections.push(trimmed_id.clone());
            self.settings.favorite_collections.sort(); // Keep the list sorted
            // Select the newly added item
            if let Some(index) = self.settings.favorite_collections.iter().position(|c| c == &trimmed_id) {
                 self.collection_list_state.select(Some(index));
            }
        }
    }


    // --- Item List Navigation (Uses item_list_state) ---

    pub fn select_next_item(&mut self) {
        if self.items.is_empty() {
            return;
        }
        let i = match self.item_list_state.selected() {
            Some(i) => {
                if i >= self.items.len() - 1 {
                    0
                } else {
                    i + 1
                }
            }
            None => 0,
        };
        self.item_list_state.select(Some(i));
    }

    pub fn select_previous_item(&mut self) {
        if self.items.is_empty() {
            return;
        }
        let i = match self.item_list_state.selected() {
            Some(i) => {
                if i == 0 {
                    self.items.len() - 1
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.item_list_state.select(Some(i));
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
    /// Path structure: base_dir / item_id / filename
    /// Returns None if download directory is not set or item ID is missing.
    pub fn get_download_path_for_file(&self, file: &FileDetails) -> Option<PathBuf> {
        match (
            self.settings.download_directory.as_ref(),
            self.viewing_item_id.as_ref(), // Item ID is sufficient
        ) {
            (Some(base_dir), Some(item_id)) => {
                let mut path = PathBuf::from(base_dir);
                // path.push(collection); // Removed collection from path
                path.push(item_id);
                path.push(&file.name);
                Some(path)
            }
            _ => None, // Missing necessary info
        }
    }

     /// Constructs the directory path for a given item.
     /// Path structure: base_dir / item_id
     /// Returns None if download directory is not set or item ID is missing.
     pub fn get_download_path_for_item(&self) -> Option<PathBuf> {
         match (
             self.settings.download_directory.as_ref(),
             self.viewing_item_id.as_ref(), // Item ID is sufficient
         ) {
             (Some(base_dir), Some(item_id)) => {
                 let mut path = PathBuf::from(base_dir);
                 // path.push(collection); // Removed collection from path
                 path.push(item_id);
                 Some(path)
             }
             _ => None, // Missing necessary info
         }
     }

    /// Helper function to construct the path to the item cache file for a collection.
    /// Returns None if download directory or collection name is missing.
    fn get_item_cache_path(&self, collection_name: &str) -> Option<PathBuf> {
        self.settings
            .download_directory
            .as_ref()
            .map(|base_dir| {
                Path::new(base_dir)
                    .join(".item_cache")
                    .join(format!("{}.json", collection_name))
            })
    }

    /// Attempts to load the item list for a collection from its cache file.
    /// Returns `Ok(Vec<ArchiveDoc>)` on success, or an `anyhow::Error` if the cache
    /// file doesn't exist, cannot be read, or contains invalid JSON.
    pub fn load_items_from_cache(&self, collection_name: &str) -> Result<Vec<ArchiveDoc>> {
        log::debug!("Attempting to load items from cache for collection: {}", collection_name);

        let cache_file_path = self.get_item_cache_path(collection_name)
            .context("Cannot determine cache path (download directory or collection name missing)")?;
        log::debug!("Looking for cache file at: {}", cache_file_path.display());

        if !cache_file_path.exists() {
            log::debug!("Cache file not found: {}", cache_file_path.display());
            return Err(anyhow!("Cache file not found")); // Specific error for not found
        }

        log::debug!("Reading cache file: {}", cache_file_path.display());
        let json_data = fs::read_to_string(&cache_file_path)
            .context(format!("Failed to read cache file: {}", cache_file_path.display()))?;
        log::debug!("Read {} bytes from cache file.", json_data.len());

        log::debug!("Parsing JSON data from cache file...");
        let items: Vec<ArchiveDoc> = serde_json::from_str(&json_data)
            .context(format!("Failed to parse JSON from cache file: {}", cache_file_path.display()))?;
        log::debug!("Successfully parsed {} items from cache.", items.len());

        log::info!("Successfully loaded {} items from cache file: {}", items.len(), cache_file_path.display());
        Ok(items)
    }


    /// Appends newly fetched items to the list and saves the entire list to the cache file.
    /// The cache path is constructed as `$download_dir/.item_cache/$collection_name.json`.
    /// Returns `Ok(())` on success, or an `anyhow::Error` if saving fails or prerequisites are missing.
    pub fn append_and_save_items(&mut self, new_items: Vec<ArchiveDoc>) -> Result<()> {
        log::debug!("Attempting to append and save items. New items count: {}", new_items.len());

        // 1. Append items to the internal list
        let new_total = self.items.len() + new_items.len();
        self.items.extend(new_items);
        log::debug!("Items appended. Total items now: {}", self.items.len());
        // Basic check after append
        if self.items.len() != new_total {
             log::warn!("Item count mismatch after appending. Expected {}, got {}", new_total, self.items.len());
        }


        // 2. Get necessary components for the path
        log::debug!("Checking prerequisites for saving item cache...");
        let download_dir = self
            .settings
            .download_directory
            .as_ref()
            .context("Download directory is not set in settings")?;
        log::debug!("Download directory found: {}", download_dir);
        let collection_name = self
            .current_collection_name
            .as_ref()
            .context("Current collection name is not set in app state")?;
        log::debug!("Collection name found: {}", collection_name);

        // 3. Construct the cache file path using the helper
        let cache_file_path = self.get_item_cache_path(collection_name)
            .context("Cannot determine cache path for saving (download directory missing)")?;
        log::debug!("Target cache file path: {}", cache_file_path.display());

        // Ensure the parent directory exists (the helper doesn't create it)
        if let Some(parent_dir) = cache_file_path.parent() {
             log::debug!("Ensuring cache directory exists: {}", parent_dir.display());
             fs::create_dir_all(parent_dir).context(format!(
                 "Failed to create cache directory: {}",
                 parent_dir.display()
             ))?;
             log::debug!("Cache directory ensured.");
        } else {
             // This case should be unlikely if the path is constructed correctly
             return Err(anyhow!("Could not determine parent directory for cache file"));
        }

        // 4. Serialize the *entire current* items list to JSON
        log::debug!("Serializing {} items to JSON...", self.items.len());
        let json_data = serde_json::to_string_pretty(&self.items)
            .context("Failed to serialize item list to JSON")?;
        log::debug!("Serialization successful. JSON size: {} bytes", json_data.len());

        // 6. Write the JSON data to the file (overwrite)
        log::debug!("Writing JSON data to cache file: {}", cache_file_path.display());
        fs::write(&cache_file_path, json_data).context(format!(
            "Failed to write item cache file: {}",
            cache_file_path.display()
        ))?;
        log::debug!("Successfully wrote cache file.");

        log::info!( // Changed to info for successful save confirmation
            "Successfully saved {} items to cache file: {}",
            self.items.len(),
            cache_file_path.display()
        );

        Ok(())
    }
}
