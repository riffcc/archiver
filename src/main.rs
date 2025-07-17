use gilrs::{Gilrs, Button, Event, EventType, Axis};
use slint::{ComponentHandle, Model, SharedPixelBuffer, Rgba8Pixel};
use std::sync::{Arc, Mutex};
use std::path::Path;

mod archive_org;
mod config;

use archive_org::ArchiveOrgClient;
use config::LibrarianConfig;

slint::include_modules!();

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let ui = MainWindow::new()?;
    
    // Set up configuration
    let config = Arc::new(LibrarianConfig::new()?);
    config.ensure_thumbnail_cache_dir()?;
    
    // Load initial Archive.org audio collections
    let ui_load = ui.as_weak();
    let config_clone = config.clone();
    tokio::spawn(async move {
        let client = ArchiveOrgClient::new();
        
        match client.search_audio_collections(1, 50).await {
            Ok(response) => {
                let _ = ui_load.upgrade_in_event_loop(move |ui| {
                    load_archive_items(ui, response.response.docs, &*config_clone);
                });
            }
            Err(e) => {
                eprintln!("Failed to load Archive.org collections: {}", e);
            }
        }
    });
    
    // Shared state for analog stick deadzone and repeat timing
    let last_stick_move = Arc::new(Mutex::new(std::time::Instant::now()));
    const STICK_DEADZONE: f32 = 0.5;
    const STICK_REPEAT_DELAY: std::time::Duration = std::time::Duration::from_millis(200);
    
    // Clone for the gamepad thread
    let ui_handle = ui.as_weak();
    let last_stick_move_gamepad = last_stick_move.clone();
    
    // Setup gamepad handling in a separate thread
    std::thread::spawn(move || {
        let mut gilrs = Gilrs::new().unwrap();
        
        loop {
            while let Some(Event { event, .. }) = gilrs.next_event() {
                let last_stick_move_clone = last_stick_move_gamepad.clone();
                let _ = ui_handle.upgrade_in_event_loop(move |ui| {
                    let current_index = ui.get_focused_index();
                    let cols_per_row = ui.get_cols_per_row();
                    let items = ui.get_items();
                    let max_index = items.iter().count() as i32 - 1;
                    
                    match event {
                        EventType::ButtonPressed(button, _) => {
                            match button {
                                // D-pad navigation
                                Button::DPadLeft => {
                                    if current_index > 0 {
                                        ui.set_focused_index(current_index - 1);
                                    }
                                }
                                Button::DPadRight => {
                                    if current_index < max_index {
                                        ui.set_focused_index(current_index + 1);
                                    }
                                }
                                Button::DPadUp => {
                                    let new_index = current_index - cols_per_row;
                                    if new_index >= 0 {
                                        ui.set_focused_index(new_index);
                                    }
                                }
                                Button::DPadDown => {
                                    let new_index = current_index + cols_per_row;
                                    if new_index <= max_index {
                                        ui.set_focused_index(new_index);
                                    }
                                }
                                
                                // A button - Download
                                Button::South => {
                                    // If items are selected, download all
                                    // Otherwise download current item
                                    let selected_count = ui.get_selected_count();
                                    if selected_count > 0 {
                                        ui.invoke_download_selected();
                                    } else {
                                        // Download current item
                                        println!("Download item at index {}", current_index);
                                    }
                                }
                                
                                // X button - Select/Deselect
                                Button::West => {
                                    ui.invoke_toggle_select(current_index);
                                    
                                    // Update selected count
                                    // Since we can't directly modify the items model,
                                    // we'll handle this in the toggle_select callback
                                }
                                
                                // Y button - Play
                                Button::North => {
                                    ui.invoke_play_item(current_index);
                                    ui.set_player_visible(true);
                                }
                                
                                // B button - Back
                                Button::East => {
                                    // TODO: Navigate back or close player
                                    if ui.get_player_visible() {
                                        ui.set_player_visible(false);
                                    }
                                }
                                
                                // Right bumper - Play/Pause
                                Button::RightTrigger => {
                                    if ui.get_player_visible() {
                                        println!("Toggle play/pause");
                                    }
                                }
                                
                                _ => {}
                            }
                        }
                        // Analog stick movement
                        EventType::AxisChanged(axis, value, _) => {
                            let now = std::time::Instant::now();
                            let mut last_move = last_stick_move_clone.lock().unwrap();
                            
                            // Only process if enough time has passed to avoid jitter
                            if now.duration_since(*last_move) > STICK_REPEAT_DELAY {
                                match axis {
                                    Axis::LeftStickX => {
                                        if value < -STICK_DEADZONE {
                                            // Left
                                            if current_index > 0 {
                                                ui.set_focused_index(current_index - 1);
                                                *last_move = now;
                                            }
                                        } else if value > STICK_DEADZONE {
                                            // Right
                                            if current_index < max_index {
                                                ui.set_focused_index(current_index + 1);
                                                *last_move = now;
                                            }
                                        }
                                    }
                                    Axis::LeftStickY => {
                                        if value < -STICK_DEADZONE {
                                            // Up (inverted Y axis)
                                            let new_index = current_index - cols_per_row;
                                            if new_index >= 0 {
                                                ui.set_focused_index(new_index);
                                                *last_move = now;
                                            }
                                        } else if value > STICK_DEADZONE {
                                            // Down
                                            let new_index = current_index + cols_per_row;
                                            if new_index <= max_index {
                                                ui.set_focused_index(new_index);
                                                *last_move = now;
                                            }
                                        }
                                    }
                                    _ => {}
                                }
                            }
                        }
                        _ => {}
                    }
                });
            }
            std::thread::sleep(std::time::Duration::from_millis(16)); // ~60fps
        }
    });
    
    // Setup callbacks
    let ui_weak = ui.as_weak();
    ui.on_toggle_select(move |index| {
        let ui = ui_weak.unwrap();
        
        // Get current items
        let items = ui.get_items();
        let mut new_items = vec![];
        
        // Clone items and toggle selection
        for (i, item) in items.iter().enumerate() {
            let mut new_item = item.clone();
            if i == index as usize {
                new_item.selected = !new_item.selected;
                if !new_item.visited {
                    new_item.visited = true;
                }
            }
            new_items.push(new_item);
        }
        
        // Update selected count
        let selected = new_items.iter().filter(|item| item.selected).count() as i32;
        ui.set_selected_count(selected);
        
        // Set the new items back
        let items_model = std::rc::Rc::new(slint::VecModel::from(new_items));
        ui.set_items(slint::ModelRc::from(items_model));
        
        println!("Toggle select for item {}", index);
    });
    
    let ui_weak2 = ui.as_weak();
    ui.on_download_selected(move || {
        let ui = ui_weak2.unwrap();
        let items = ui.get_items();
        
        println!("Download all selected items:");
        for (_i, item) in items.iter().enumerate() {
            if item.selected {
                println!("  - Downloading: {} by {}", item.title, item.artist);
            }
        }
        // TODO: Implement qBittorrent integration
    });
    
    ui.on_play_item(move |index| {
        println!("Play item at index {}", index);
        // TODO: Implement media playback
    });
    
    let ui_weak3 = ui.as_weak();
    ui.on_enter_item(move |index| {
        let ui = ui_weak3.unwrap();
        let items = ui.get_items();
        
        if let Some(item) = items.iter().nth(index as usize) {
            if item.item_type == "Collection" {
                println!("Enter collection: {} (ID: {})", item.title, item.id);
                // TODO: Navigate into collection
            } else {
                println!("View item details: {} by {} (ID: {})", item.title, item.artist, item.id);
                // TODO: Show item details view
            }
        }
    });
    
    ui.run()?;
    
    Ok(())
}

fn load_archive_items(ui: &MainWindow, items: Vec<archive_org::ArchiveOrgItem>, config: &LibrarianConfig) {
    let mut media_items = vec![];
    
    for (i, item) in items.iter().take(50).enumerate() {
        let media_item = MediaItem {
            id: slint::SharedString::from(item.identifier.clone()),
            title: slint::SharedString::from(item.title.clone().unwrap_or_else(|| item.identifier.clone())),
            artist: slint::SharedString::from(item.creator.clone().unwrap_or_default()),
            year: item.date.as_ref()
                .and_then(|d| d.split('-').next())
                .and_then(|y| y.parse::<i32>().ok())
                .unwrap_or(0),
            visited: false,
            selected: false,
            item_type: slint::SharedString::from(item.mediatype.clone().unwrap_or_else(|| "audio".to_string())),
            thumbnail: slint::Image::default(),
        };
        media_items.push(media_item);
        
        // Load thumbnail asynchronously
        let ui_weak = ui.as_weak();
        let identifier = item.identifier.clone();
        let cache_path = config.thumbnail_cache_path(&identifier);
        let index = i;
        
        tokio::spawn(async move {
            if let Ok(image_data) = load_or_fetch_thumbnail(&identifier, &cache_path).await {
                let _ = ui_weak.upgrade_in_event_loop(move |ui| {
                    if let Ok(img) = image::load_from_memory(&image_data) {
                        let rgba = img.to_rgba8();
                        let width = rgba.width();
                        let height = rgba.height();
                        let buffer = SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(
                            rgba.as_raw(),
                            width,
                            height,
                        );
                        
                        let items = ui.get_items();
                        if let Some(mut item) = items.iter().nth(index).cloned() {
                            item.thumbnail = slint::Image::from_rgba8(buffer);
                            
                            // Update the specific item
                            let mut new_items: Vec<MediaItem> = items.iter().cloned().collect();
                            if index < new_items.len() {
                                new_items[index] = item;
                                let items_model = std::rc::Rc::new(slint::VecModel::from(new_items));
                                ui.set_items(slint::ModelRc::from(items_model));
                            }
                        }
                    }
                });
            }
        });
    }
    
    let items_model = std::rc::Rc::new(slint::VecModel::from(media_items));
    ui.set_items(slint::ModelRc::from(items_model));
}

async fn load_or_fetch_thumbnail(identifier: &str, cache_path: &Path) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    // Check if cached
    if cache_path.exists() {
        return Ok(tokio::fs::read(cache_path).await?);
    }
    
    // Fetch from Archive.org
    let client = ArchiveOrgClient::new();
    let image_data = client.download_thumbnail(identifier).await?;
    
    // Cache it
    if let Some(parent) = cache_path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    tokio::fs::write(cache_path, &image_data).await?;
    
    Ok(image_data)
}