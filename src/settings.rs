use anyhow::{Context, Result};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf};

const QUALIFIER: &str = "com";
const ORGANIZATION: &str = "riffcc"; // Updated organization
pub const APPLICATION: &str = "archiver"; // Updated application name

#[derive(Serialize, Deserialize, Debug, Clone)] // Removed Default here
pub struct Settings {
    pub download_directory: Option<String>,
    pub max_concurrent_downloads: Option<usize>, // Add concurrency limit
}

// Implement Default manually to set a default concurrency
impl Default for Settings {
    fn default() -> Self {
        Self {
            download_directory: None,
            max_concurrent_downloads: Some(4), // Default to 4 concurrent downloads
        }
    }
}


/// Returns the path to the configuration file.
fn get_config_path() -> Result<PathBuf> {
    let proj_dirs = ProjectDirs::from(QUALIFIER, ORGANIZATION, APPLICATION)
        .context("Could not find project directories")?;
    let config_dir = proj_dirs.config_dir();
    fs::create_dir_all(config_dir)?; // Ensure the config directory exists
    Ok(config_dir.join("settings.toml"))
}

/// Loads settings from the configuration file.
/// If the file doesn't exist, returns default settings.
pub fn load_settings() -> Result<Settings> {
    let config_path = get_config_path()?;
    if !config_path.exists() {
        return Ok(Settings::default()); // Return default if no config file
    }

    let settings = config::Config::builder()
        .add_source(config::File::from(config_path))
        .build()?
        .try_deserialize::<Settings>()?;

    Ok(settings)
}

/// Saves the given settings to the configuration file.
pub fn save_settings(settings: &Settings) -> Result<()> {
    let config_path = get_config_path()?;
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
        let config_dir = temp_dir.path().join(".config").join(APPLICATION);
        fs::create_dir_all(&config_dir).unwrap();

        // Mock the config directory path for the test duration
        // Note: This relies on internal details of `get_config_path` using ProjectDirs.
        // A more robust approach might involve dependency injection for the path.
        // For simplicity, we'll assume this works for now.
        // We need to simulate the structure ProjectDirs expects.
        let mock_home = temp_dir.path().to_path_buf();
        env::set_var("HOME", mock_home.to_str().unwrap()); // For Linux/macOS simulation
                                                           // Add similar vars for Windows if needed (`APPDATA`, `USERPROFILE`)

        (temp_dir, config_dir.join("settings.toml"))
    }


    #[test]
    fn test_load_settings_default() {
        let _temp_dir = setup_test_env(); // Keep temp_dir alive
        let settings = load_settings().unwrap();
        assert_eq!(settings.download_directory, None);
    }

    #[test]
    fn test_save_and_load_settings() {
        let (_temp_dir, config_path) = setup_test_env(); // Keep temp_dir alive

        let mut settings_to_save = Settings::default();
        settings_to_save.download_directory = Some("/tmp/downloads".to_string());

        save_settings(&settings_to_save).unwrap();
        assert!(config_path.exists());

        let loaded_settings = load_settings().unwrap();
        assert_eq!(loaded_settings.download_directory, Some("/tmp/downloads".to_string()));
    }

     #[test]
    fn test_load_settings_file_not_found_returns_default() {
         // Ensure no real config interferes
         let _temp_dir = setup_test_env();
         // Don't save anything, just try loading
         let settings = load_settings().unwrap();
         assert_eq!(settings.download_directory, None);
         assert!(settings.download_directory.is_none()); // Double check
     }

     #[test]
     fn test_save_settings_creates_directory() {
         let temp_dir = tempdir().unwrap();
         let config_dir_base = temp_dir.path().join(".config"); // Don't create APPLICATION subdir yet

         // Mock HOME to point to temp_dir
         let mock_home = temp_dir.path().to_path_buf();
         env::set_var("HOME", mock_home.to_str().unwrap());

         let settings_to_save = Settings { download_directory: Some("test_dir".to_string()) };
         save_settings(&settings_to_save).unwrap();

         let expected_config_path = config_dir_base.join(APPLICATION).join("settings.toml");
         assert!(expected_config_path.exists(), "Config file should be created");
         assert!(expected_config_path.parent().unwrap().exists(), "Config directory should be created");
     }
}
