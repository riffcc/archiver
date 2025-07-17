use directories::UserDirs;
use std::path::PathBuf;
use std::fs;
use std::error::Error;

pub struct LibrarianConfig {
    pub downloads_dir: PathBuf,
    pub processing_dir: PathBuf,
    pub library_dir: PathBuf,
    pub cache_dir: PathBuf,
}

impl LibrarianConfig {
    pub fn new() -> Result<Self, Box<dyn Error>> {
        let user_dirs = UserDirs::new().ok_or("Could not determine user directories")?;
        let home = user_dirs.home_dir();
        
        let base_dir = home.join(".riffcc").join("librarian");
        
        let config = Self {
            downloads_dir: base_dir.join("downloads"),
            processing_dir: base_dir.join("processing"),
            library_dir: base_dir.join("library"),
            cache_dir: base_dir.join("cache"),
        };
        
        // Create directories if they don't exist
        fs::create_dir_all(&config.downloads_dir)?;
        fs::create_dir_all(&config.processing_dir)?;
        fs::create_dir_all(&config.library_dir)?;
        fs::create_dir_all(&config.cache_dir)?;
        
        Ok(config)
    }
    
    pub fn thumbnail_cache_path(&self, identifier: &str) -> PathBuf {
        self.cache_dir.join("thumbnails").join(format!("{}.jpg", identifier))
    }
    
    pub fn ensure_thumbnail_cache_dir(&self) -> Result<(), Box<dyn Error>> {
        fs::create_dir_all(self.cache_dir.join("thumbnails"))?;
        Ok(())
    }
}