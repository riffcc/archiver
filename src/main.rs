use gilrs::{Gilrs, Button, Event, EventType, Axis};
use slint::{ComponentHandle, Model};
use std::sync::{Arc, Mutex};

slint::include_modules!();

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let ui = MainWindow::new()?;
    
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