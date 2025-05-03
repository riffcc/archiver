use anyhow::{Context, Result};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::{fmt, fs, path::PathBuf}; // Add fmt

const QUALIFIER: &str = "com";
const ORGANIZATION: &str = "riffcc"; // Updated organization
pub const APPLICATION: &str = "archiver"; // Updated application name

/// Defines the download strategy.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Copy)] // Added Eq, Copy
pub enum DownloadMode {
    /// Download all files directly.
    Direct,
    /// Download only the .torrent file.
    TorrentOnly,
}

// Implement Display for showing the mode in the UI
impl fmt::Display for DownloadMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DownloadMode::Direct => write!(f, "Direct (All Files)"),
            DownloadMode::TorrentOnly => write!(f, "Torrent Only (.torrent)"),
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)] // Added PartialEq
pub struct Settings {
    pub download_directory: Option<String>,
    /// Download mode (Direct or TorrentOnly).
    #[serde(default = "default_download_mode")]
    pub download_mode: DownloadMode,
    /// Max concurrent file downloads *within* a single item/collection download task.
    pub max_concurrent_downloads: Option<usize>,
    /// List of saved collection identifiers.
    #[serde(default = "Vec::new")] // Ensure field exists even if missing in old config
    pub favorite_collections: Vec<String>,
    /// Max concurrent collection downloads (when downloading multiple collections).
    pub max_concurrent_collections: Option<usize>,
}

// Implement Default manually to set defaults
impl Default for Settings {
    fn default() -> Self {
        Self {
            download_directory: None,
            download_mode: default_download_mode(),
            max_concurrent_downloads: Some(4), // Default to 4 concurrent file downloads
            favorite_collections: Vec::new(),  // Default to empty list
            max_concurrent_collections: Some(1), // Default to downloading 1 collection at a time
        }
    }
}

// Helper function for serde default
fn default_download_mode() -> DownloadMode {
    DownloadMode::Direct // Default download mode
}


/// Returns the path to the configuration file.
fn get_config_path() -> Result<PathBuf> {
    let proj_dirs = ProjectDirs::from(QUALIFIER, ORGANIZATION, APPLICATION)
        .context("Could not find project directories")?;
    let config_dir = proj_dirs.config_dir();
    fs::create_dir_all(config_dir)?; // Ensure the config directory exists
    Ok(config_dir.join("settings.toml"))
}

/// Loads settings from the default configuration file path.
/// If the file doesn't exist, returns default settings.
pub fn load_settings() -> Result<Settings> {
    let config_path = get_config_path()?;
    load_settings_from_path(&config_path)
}

/// Saves the given settings to the default configuration file path.
pub fn save_settings(settings: &Settings) -> Result<()> {
    let config_path = get_config_path()?;
    save_settings_to_path(settings, &config_path)
}


/// Loads settings from the specified configuration file path.
/// If the file doesn't exist, returns default settings.
fn load_settings_from_path(config_path: &PathBuf) -> Result<Settings> {
    if !config_path.exists() {
        return Ok(Settings::default()); // Return default if no config file
    }

    let settings = config::Config::builder()
        // Make the file source optional for the builder.
        // If the file exists (as expected in the test), it will be loaded.
        // If not, build() won't error, and try_deserialize will likely use defaults.
        .add_source(config::File::from(config_path.clone()).required(false))
        .build()?
        .try_deserialize::<Settings>()?;

    Ok(settings)
}

/// Saves the given settings to the specified configuration file path.
/// Ensures the parent directory exists.
fn save_settings_to_path(settings: &Settings, config_path: &PathBuf) -> Result<()> {
    // Ensure the parent directory exists before writing
    if let Some(parent_dir) = config_path.parent() {
        fs::create_dir_all(parent_dir)?;
    }
    let toml_string = toml::to_string_pretty(settings)?;
    fs::write(config_path, toml_string)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use tempfile::tempdir;

    // Helper to set up a temporary config directory for tests
    fn setup_test_env() -> (tempfile::TempDir, PathBuf) {
        let temp_dir = tempdir().unwrap();
        let mock_home = temp_dir.path().to_path_buf();
        // Set HOME environment variable *before* calling ProjectDirs
        env::set_var("HOME", mock_home.to_str().unwrap());

        // Use ProjectDirs to find the config directory based on the mocked HOME
        // This ensures we use the platform-correct path (e.g., Library/Application Support on macOS)
        let proj_dirs = ProjectDirs::from(QUALIFIER, ORGANIZATION, APPLICATION)
            .expect("Could not find project directories in test setup");
        let config_dir = proj_dirs.config_dir();

        // Explicitly create the config directory here to ensure it exists for all tests.
        fs::create_dir_all(config_dir)
            .expect("Failed to create config directory in test setup");

        // Calculate the expected config file path
        let config_file_path = config_dir.join("settings.toml");

        (temp_dir, config_file_path) // Return handle and the *correct* expected path
    }

    #[test]
    fn test_load_settings_default_from_specific_path() {
        let (_temp_dir, config_path) = setup_test_env(); // Keep temp_dir alive
        // Load from the specific (non-existent) path
        let settings = load_settings_from_path(&config_path).unwrap();
        assert_eq!(settings.download_directory, None);
        assert_eq!(settings.download_mode, DownloadMode::Direct); // Check default mode
        assert_eq!(settings, Settings::default()); // Ensure all defaults match
    }

    #[test]
    fn test_save_and_load_settings() {
        let (_temp_dir, config_path) = setup_test_env(); // Keep temp_dir alive

        let mut settings_to_save = Settings::default();
        settings_to_save.download_directory = Some("/tmp/downloads".to_string());
        settings_to_save.download_mode = DownloadMode::TorrentOnly; // Test non-default mode
        settings_to_save.max_concurrent_downloads = Some(10);
        settings_to_save.favorite_collections = vec!["test_coll".to_string()];

        // Save to the specific path
        save_settings_to_path(&settings_to_save, &config_path).unwrap();
        assert!(config_path.exists());

        // Load from the specific path
        let loaded_settings = load_settings_from_path(&config_path).unwrap();
        assert_eq!(loaded_settings.download_directory, Some("/tmp/downloads".to_string()));
        assert_eq!(loaded_settings.download_mode, DownloadMode::TorrentOnly); // Verify loaded mode
        assert_eq!(loaded_settings.max_concurrent_downloads, Some(10));
        assert_eq!(loaded_settings.favorite_collections, vec!["test_coll".to_string()]);
    }

     #[test]
    fn test_load_settings_file_not_found_returns_default_from_specific_path() {
         // Ensure no real config interferes
         let (_temp_dir, config_path) = setup_test_env();
         // Don't save anything, just try loading from the specific path
         let settings = load_settings_from_path(&config_path).unwrap();
         assert_eq!(settings.download_directory, None);
         assert!(settings.download_directory.is_none()); // Double check
         assert_eq!(settings.max_concurrent_downloads, Some(4)); // Check another default
     }

     #[test]
     fn test_save_settings_creates_directory() {
         let temp_dir = tempdir().unwrap();
         let mock_home = temp_dir.path().to_path_buf();
         env::set_var("HOME", mock_home.to_str().unwrap());

         // Use ProjectDirs to find the expected path, consistent with how save_settings works
         let proj_dirs = ProjectDirs::from(QUALIFIER, ORGANIZATION, APPLICATION)
             .expect("Could not find project directories in test");
         let expected_config_dir = proj_dirs.config_dir();
         let expected_config_path = expected_config_dir.join("settings.toml");

         // Ensure the directory does NOT exist initially to confirm save_settings creates it
         assert!(!expected_config_dir.exists(), "Config directory should not exist initially at {:?}", expected_config_dir);

         let settings_to_save = Settings {
             download_directory: Some("test_dir".to_string()),
             download_mode: DownloadMode::Direct, // Add the missing field
             max_concurrent_downloads: Some(5),
             favorite_collections: vec!["coll1".to_string(), "coll2".to_string()],
             max_concurrent_collections: Some(2),
         };
         // This call should create the directory and write the file to the specific path
         save_settings_to_path(&settings_to_save, &expected_config_path).unwrap();

         // Assert that save_settings_to_path created the file and its parent directory
         assert!(expected_config_path.exists(), "Config file should be created at {:?}", expected_config_path);
         assert!(expected_config_path.parent().unwrap().exists(), "Config directory should be created at {:?}", expected_config_dir);
     }
}
