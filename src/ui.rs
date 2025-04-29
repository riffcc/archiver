use crate::app::{App, AppState}; // Keep only one import line
use ratatui::{
    prelude::{Alignment, Constraint, Direction, Frame, Layout, Line, Rect, Span}, // Remove unused Text
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
};
// Removed unused FileDetails import for now

/// Renders the user interface widgets.
pub fn render(app: &mut App, frame: &mut Frame) {
    let main_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Input field
            Constraint::Min(0),    // Main content area (List or Item View)
            Constraint::Length(1), // Status/Error message
        ])
        .split(frame.area());

    render_input(app, frame, main_layout[0]);

    // Render either item list or item view based on state
    match app.current_state {
        AppState::Browsing | AppState::AskingDownloadDir => { // Show list when browsing or asking for dir
            render_item_list(app, frame, main_layout[1]);
        }
        AppState::ViewingItem => {
            render_item_view(app, frame, main_layout[1]);
        }
        AppState::Downloading => {
             // Potentially show download progress here later
             render_item_list(app, frame, main_layout[1]); // Show list for now
        }
        AppState::SettingsView => {
            render_settings_view(app, frame, main_layout[1]);
        }
        AppState::EditingSetting => {
            // When editing, still render the settings view underneath
            render_settings_view(app, frame, main_layout[1]);
        }
    }

    render_status_bar(app, frame, main_layout[2]);
}

fn render_input(app: &mut App, frame: &mut Frame, area: Rect) {
    let (input_prompt, mut block_title) = match app.current_state {
         AppState::Browsing => ("> ", "Collection Name"),
         AppState::AskingDownloadDir => ("Enter Path: ", "Set Download Directory (Enter to save, Esc to cancel)"),
         AppState::ViewingItem => ("", "Collection Filter"), // Input not active
         AppState::Downloading => ("> ", "Collection Name"), // Input not active
         AppState::SettingsView => ("", "Settings"), // Input not active in settings view
         AppState::EditingSetting => ("Edit: ", "Editing Download Directory"), // Prompt for editing
     };

    // Modify title based on filtering mode only when Browsing or EditingSetting
    let border_style = if app.is_filtering_input && (app.current_state == AppState::Browsing || app.current_state == AppState::EditingSetting) {
        block_title = "Collection Filter (Filtering)"; // Keep this title for now, maybe change later for settings edit
        Style::default().fg(Color::Yellow) // Highlight border when filtering
    } else if app.current_state == AppState::AskingDownloadDir {
         Style::default().fg(Color::Yellow) // Also highlight when asking for dir
    }
     else {
        Style::default() // Default border style
    };


    let input_text = format!("{}{}", input_prompt, app.collection_input);
    let input_block = Block::default()
        .borders(Borders::ALL)
        .title(block_title)
        .border_style(border_style);

    let input = Paragraph::new(input_text).block(input_block);

    frame.render_widget(input, area);

    // Only show cursor if we are actively filtering input in a relevant state
    if app.is_filtering_input && (app.current_state == AppState::Browsing || app.current_state == AppState::AskingDownloadDir || app.current_state == AppState::EditingSetting) {
         // Adjust cursor position based on which input field is active
         let text_to_measure = match app.current_state {
             AppState::EditingSetting => &app.editing_setting_input,
             _ => &app.collection_input, // Browsing or AskingDownloadDir
         };
         let current_cursor_pos = app.cursor_position.clamp(0, text_to_measure.len());

        frame.set_cursor_position((
            area.x + current_cursor_pos as u16 + input_prompt.len() as u16, // Adjust for prompt length
            area.y + 1, // Inside the border
        ));
    }
}


fn render_item_list(app: &mut App, frame: &mut Frame, area: Rect) {
    let list_title: String = if app.is_filtering_input && app.current_state == AppState::Browsing {
        "Items (Press Esc to navigate)".to_string()
    } else if app.current_state == AppState::Browsing {
        let count_str = app.total_items_found.map_or("?".to_string(), |t| t.to_string());
        let shown_count = app.items.len(); // Assuming only one page shown for now
        format!(
            "Items (Showing {} / {} Total) ('i': Filter, 's': Settings, Enter: View, 'd': Item, 'b': All, ↑/↓: Nav)",
            shown_count, count_str
        )
    } else {
         "Items".to_string()
    };

    let border_style = if !app.is_filtering_input && app.current_state == AppState::Browsing {
        Style::default().fg(Color::Yellow) // Highlight border when navigating list
    } else {
        Style::default() // Default border style otherwise
    };

    let list_block = Block::default()
        .borders(Borders::ALL)
        .title(list_title)
        .border_style(border_style);

    if app.is_loading {
        let loading_paragraph = Paragraph::new("Loading...")
            .block(list_block)
            .style(Style::default().fg(Color::Yellow));
        frame.render_widget(loading_paragraph, area);
        return;
    }

     // Handle empty list message specifically for Browsing state
     if app.current_state == AppState::Browsing && app.items.is_empty() && !app.is_loading {
         let empty_msg = if let Some(err) = &app.error_message {
             // Don't show fetch error if we are not actively showing results for the input
             if !app.collection_input.is_empty() {
                 format!("Error fetching items: {}", err)
             } else {
                  "Enter a collection name above and press Enter.".to_string()
             }
         } else if !app.collection_input.is_empty() {
             "No items found for this collection. Press Enter to search.".to_string()
         } else {
             "Enter a collection name above and press Enter.".to_string()
         };

         let empty_paragraph = Paragraph::new(empty_msg)
             .block(list_block.clone()) // Clone block for styling
             .style(Style::default().fg(Color::DarkGray))
             .alignment(Alignment::Center);
         frame.render_widget(empty_paragraph, area);
         return;
     } else if app.current_state != AppState::Browsing {
         // Don't show the item list if not in browsing state
         // Render the block border anyway
         frame.render_widget(list_block, area);
         return;
     }


    let list_items: Vec<ListItem> = app
        .items
        .iter()
        .map(|item| ListItem::new(item.identifier.clone()))
        .collect();

    let list = List::new(list_items)
        .block(list_block)
        .highlight_style(
            Style::default()
                .bg(Color::Blue) // Example highlight color
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("> "); // Symbol in front of the selected item

    // Use app.list_state to render the list and handle selection state
    frame.render_stateful_widget(list, area, &mut app.list_state);
}

/// Renders the item detail view (placeholder).
fn render_item_view(app: &mut App, frame: &mut Frame, area: Rect) {
    let item_id = app.viewing_item_id.as_deref().unwrap_or("Unknown Item"); // Get the ID

    let outer_block = Block::default()
        .borders(Borders::ALL)
        .title(format!("Viewing Item: {} (Esc: Back, ↑/↓: Files)", item_id))
        .border_style(Style::default().fg(Color::Cyan)); // Highlight view border

    // Create inner area excluding the border
    let inner_area = outer_block.inner(area);
    frame.render_widget(outer_block, area); // Render the outer block first

    if app.is_loading_details {
        let loading_paragraph = Paragraph::new("Loading details...")
            .style(Style::default().fg(Color::Yellow))
            .alignment(Alignment::Center);
        frame.render_widget(loading_paragraph, inner_area);
        return;
    }

    if let Some(details) = &app.current_item_details {
        // Split the inner area for metadata and file list
        let view_layout = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(40), // Metadata pane
                Constraint::Percentage(60), // File list pane
            ])
            .split(inner_area);

        render_metadata_pane(app, frame, view_layout[0]);
        render_file_list_pane(app, frame, view_layout[1]);

    } else {
        // Display error if details are None and not loading
        let error_msg = app.error_message.as_deref().unwrap_or("Failed to load item details.");
         let error_paragraph = Paragraph::new(error_msg)
             .style(Style::default().fg(Color::Red))
             .alignment(Alignment::Center);
         frame.render_widget(error_paragraph, inner_area);
    }
}

/// Renders the metadata pane within the item view.
fn render_metadata_pane(app: &App, frame: &mut Frame, area: Rect) {
    let block = Block::default().borders(Borders::RIGHT).title("Metadata"); // Add right border
    let inner_area = block.inner(area);
    frame.render_widget(block, area);

    // Use `details` directly from the `if let` binding.
    if let Some(details) = &app.current_item_details {
        let mut lines = Vec::new(); // Changed to Vec<Line>

        // Remove inner re-binding, use `details` directly

        lines.push(Line::from(vec![
            Span::styled("Title: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(details.title.as_deref().unwrap_or("N/A")),
        ]));
        lines.push(Line::from("")); // Spacer

        lines.push(Line::from(vec![
            Span::styled("Creator: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(details.creator.as_deref().unwrap_or("N/A")),
        ]));
        lines.push(Line::from(""));

        lines.push(Line::from(vec![
            Span::styled("Date: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(details.date.as_deref().unwrap_or("N/A")),
        ]));
        lines.push(Line::from(""));

        lines.push(Line::from(vec![
            Span::styled("Uploader: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(details.uploader.as_deref().unwrap_or("N/A")),
        ]));
        lines.push(Line::from(""));

        lines.push(Line::from(Span::styled(
            "Collections: ",
            Style::default().add_modifier(Modifier::BOLD),
        )));
        if details.collections.is_empty() {
            lines.push(Line::from("N/A"));
        } else {
            // Wrap collections manually if needed, or just join
            lines.push(Line::from(details.collections.join(", ")));
        }
        lines.push(Line::from(""));

        lines.push(Line::from(Span::styled(
            "Description: ",
            Style::default().add_modifier(Modifier::BOLD),
        )));
        // Handle potential multi-line description
        let description = details.description.as_deref().unwrap_or("N/A");
        for desc_line in description.lines() {
             lines.push(Line::from(desc_line));
        }


        let paragraph = Paragraph::new(lines) // Pass Vec<Line>
            .wrap(Wrap { trim: true }); // Wrap long lines

        frame.render_widget(paragraph, inner_area);
    }
}

/// Renders the file list pane within the item view.
fn render_file_list_pane(app: &mut App, frame: &mut Frame, area: Rect) {
    let block = Block::default().title("Files"); // No border needed here
    let inner_area = block.inner(area);
    frame.render_widget(block, area);

    // Prefix `details` with `_` as it's not directly used after the `if let`.
    if let Some(_details) = &app.current_item_details {
        // Use app.current_item_details directly below where needed
        let details = app.current_item_details.as_ref().unwrap(); // Safe to unwrap due to if let

        if details.files.is_empty() {
            let empty_msg = Paragraph::new("No files found for this item.")
                .style(Style::default().fg(Color::DarkGray))
                .alignment(Alignment::Center);
            frame.render_widget(empty_msg, inner_area);
            return;
        }

        let list_items: Vec<ListItem> = details.files.iter().map(|file| {
            // Combine relevant file info into one line
            let line = format!(
                "{} (Format: {}, Size: {})",
                file.name,
                file.format.as_deref().unwrap_or("N/A"),
                file.size.as_deref().unwrap_or("N/A")
            );
            ListItem::new(line)
        }).collect();

        let list = List::new(list_items)
            .highlight_style(
                Style::default()
                    .bg(Color::Blue)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("> ");

        frame.render_stateful_widget(list, inner_area, &mut app.file_list_state);
    }
}

/// Renders the settings view.
fn render_settings_view(app: &mut App, frame: &mut Frame, area: Rect) {
    let settings_block = Block::default()
        .borders(Borders::ALL)
        .title("Settings (Esc: Save & Back, ↑/↓: Select, ←/→: Adjust)")
        .border_style(Style::default().fg(Color::Magenta)); // Distinct border color

    let inner_area = settings_block.inner(area);
    frame.render_widget(settings_block, area);

    // Define settings items, showing edit buffer if active
    let download_dir_text = if app.current_state == AppState::EditingSetting && app.selected_setting_index == 0 {
         format!("Download Directory: {}", app.editing_setting_input)
    } else {
         format!(
            "Download Directory: {}",
            app.settings.download_directory.as_deref().unwrap_or("Not Set")
        )
    };

    let concurrency_text = format!(
        "Max Concurrent Downloads: {} {}",
        app.settings.max_concurrent_downloads.map_or("Unlimited".to_string(), |n| n.to_string()),
        if app.selected_setting_index == 1 { "< >" } else { "" } // Hint for adjustment
    );

    let settings_items = vec![
        ListItem::new(download_dir_text),
        ListItem::new(concurrency_text),
    ];

    let list = List::new(settings_items)
        .highlight_style(
            Style::default()
                .bg(Color::DarkGray) // Different highlight for settings
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol(">> ");

    frame.render_stateful_widget(list, inner_area, &mut app.settings_list_state);
}


fn render_status_bar(app: &mut App, frame: &mut Frame, area: Rect) {
    let status_text = if app.is_downloading {
        // Format progress string if downloading
        let item_progress = app.total_items_to_download.map_or("?".to_string(), |t| t.to_string());
        let file_progress = app.total_files_to_download.map_or("?".to_string(), |t| t.to_string());
        format!(
            "Downloading [Items: {}/{} | Files: {}/{}] Last: {}",
            app.items_downloaded_count,
            item_progress,
            app.files_downloaded_count,
            file_progress,
            app.download_status.as_deref().unwrap_or("...") // Show last status message
        )
    } else if let Some(status) = &app.download_status {
        status.clone() // Clone the String status
    } else if let Some(err) = &app.error_message {
        err.clone() // Clone the String error
    } else if app.is_loading {
        "Fetching collection data...".to_string() // Convert literal to String
    } else if app.is_loading_details {
         "Fetching item details...".to_string() // Convert literal to String
    } else if app.current_state == AppState::AskingDownloadDir {
        "Enter the full path for downloads. Esc to cancel.".to_string()
    } else if app.current_state == AppState::ViewingItem {
        "Viewing item details. Esc: Back, ↑/↓: Files, Enter/d: Download File, 'b': Download All Files".to_string()
    } else if app.current_state == AppState::SettingsView {
         "Settings. Esc: Save & Back, ↑/↓: Select, Enter: Edit Dir, ←/→: Adjust Concurrency".to_string()
    } else if app.current_state == AppState::EditingSetting {
         "Editing Download Directory. Enter: Save, Esc: Cancel".to_string()
    }
     else if app.is_filtering_input {
        "Filtering Input. Press Esc to navigate list, Enter to search.".to_string()
    } else {
        "Navigating List. 'q': Quit, 'i': Filter, 's': Settings, Enter: View, 'd': Download Item, 'b': Download All".to_string()
    };

    let status_style = if app.error_message.is_some() || app.download_status.as_deref().unwrap_or("").contains("Error") || app.download_status.as_deref().unwrap_or("").contains("Failed") {
        Style::default().fg(Color::Red)
    } else if app.is_downloading {
         Style::default().fg(Color::Yellow) // Indicate ongoing download
    } else if app.download_status.is_some() {
         Style::default().fg(Color::Green) // Indicate completed download (if no error)
    } else {
        Style::default()
    };

    let status_paragraph = Paragraph::new(status_text).style(status_style);
    frame.render_widget(status_paragraph, area);
}
