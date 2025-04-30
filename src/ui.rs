use crate::app::{ActivePane, App, AppState}; // Add ActivePane
use ratatui::{
    prelude::{Alignment, Constraint, Direction, Frame, Layout, Line, Rect, Span},
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Wrap}, // Add Clear
};

/// Renders the user interface widgets.
pub fn render(app: &mut App, frame: &mut Frame) {
    // Main layout: Status bar at the bottom, rest is the main content area
    let main_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(0),    // Main content area
            Constraint::Length(1), // Status/Error message
        ])
        .split(frame.area());

    let content_area = main_layout[0];
    let status_area = main_layout[1];

    // Render content based on state
    match app.current_state {
        AppState::Browsing => {
            render_browsing_panes(app, frame, content_area);
        }
        AppState::ViewingItem => {
            render_item_view(app, frame, content_area);
        }
        AppState::SettingsView | AppState::EditingSetting => {
            render_settings_view(app, frame, content_area);
            // Render editing input overlay if needed
            if app.current_state == AppState::EditingSetting {
                render_editing_setting_input(app, frame); // Needs frame ref
            }
        }
         AppState::AddingCollection => {
            // Render browsing panes underneath
            render_browsing_panes(app, frame, content_area);
            // Render the add collection input overlay
            render_add_collection_input(app, frame); // Needs frame ref
        }
        AppState::AskingDownloadDir => {
            // Render browsing panes underneath (or maybe just grey out?)
            render_browsing_panes(app, frame, content_area);
            // Render the ask download dir input overlay
            render_ask_download_dir_input(app, frame); // Needs frame ref
        }
        AppState::Downloading => {
             // Render browsing panes underneath, status bar shows progress
             render_browsing_panes(app, frame, content_area);
        }
    }

    render_status_bar(app, frame, status_area);
}

/// Renders the two-pane view for Collections and Items.
fn render_browsing_panes(app: &mut App, frame: &mut Frame, area: Rect) {
    let browser_layout = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(30), // Collections pane width
            Constraint::Percentage(70), // Items pane width
        ])
        .split(area);

    render_collection_list_pane(app, frame, browser_layout[0]);
    render_item_list_pane(app, frame, browser_layout[1]);
}

/// Renders the list of favorite collections.
fn render_collection_list_pane(app: &mut App, frame: &mut Frame, area: Rect) {
    let border_style = if app.active_pane == ActivePane::Collections {
        Style::default().fg(Color::Yellow) // Highlight active pane
    } else {
        Style::default()
    };

    let list_block = Block::default()
        .borders(Borders::ALL)
        .title("Collections ('a': Add, Del: Remove, Enter: Load, Tab: Switch)")
        .border_style(border_style);

    let _inner_area = list_block.inner(area); // Prefix with underscore

    if app.settings.favorite_collections.is_empty() {
        let empty_msg = Paragraph::new("No collections saved.\nPress 'a' to add one.")
            .block(list_block) // Render block border anyway
            .style(Style::default().fg(Color::DarkGray))
            .alignment(Alignment::Center);
        frame.render_widget(empty_msg, area);
        return;
    }

    let list_items: Vec<ListItem> = app
        .settings
        .favorite_collections
        .iter()
        .map(|collection_name| ListItem::new(collection_name.clone()))
        .collect();

    let list = List::new(list_items)
        .block(list_block) // Attach the block here
        .highlight_style(
            Style::default()
                .bg(Color::Blue)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("> ");

    frame.render_stateful_widget(list, area, &mut app.collection_list_state);
}


/// Renders the list of items for the selected collection. (Renamed from render_item_list)
fn render_item_list_pane(app: &mut App, frame: &mut Frame, area: Rect) {
     let border_style = if app.active_pane == ActivePane::Items {
        Style::default().fg(Color::Yellow) // Highlight active pane
    } else {
        Style::default()
    };

    let list_title = if let Some(collection_name) = app.current_collection_name.as_deref() {
        let count_str = app.total_items_found.map_or("?".to_string(), |t| t.to_string());
        let shown_count = app.items.len();
        format!(
            "Items for '{}' ({} / {}) (Enter: View, 'd': Item, 'b': All, Tab: Switch)",
            collection_name, shown_count, count_str
        )
    } else {
        "Items (Select a collection) (Tab: Switch)".to_string()
    };


    let list_block = Block::default()
        .borders(Borders::ALL)
        .title(list_title)
        .border_style(border_style);

    let _inner_area = list_block.inner(area); // Prefix with underscore

    if app.is_loading {
        let loading_paragraph = Paragraph::new("Loading items...")
            .block(list_block) // Render block border anyway
            .style(Style::default().fg(Color::Yellow))
            .alignment(Alignment::Center);
        frame.render_widget(loading_paragraph, area);
        return;
    }

    // Handle error message display
    if let Some(err) = &app.error_message {
        // Only show error if it's relevant to the item list (e.g., fetch failed)
        // We might need more specific error types later.
        if app.current_collection_name.is_some() { // Only show if we tried loading a collection
            let error_paragraph = Paragraph::new(format!("Error: {}", err))
                .block(list_block)
                .style(Style::default().fg(Color::Red))
                .alignment(Alignment::Center);
            frame.render_widget(error_paragraph, area);
            return;
        }
    }

    // Handle empty list or no collection selected
    if app.current_collection_name.is_none() || (app.items.is_empty() && !app.is_loading) {
        let empty_msg = if app.current_collection_name.is_none() {
            "<- Select a collection"
        } else {
            "No items found for this collection."
        };
        let empty_paragraph = Paragraph::new(empty_msg)
            .block(list_block) // Render block border anyway
            .style(Style::default().fg(Color::DarkGray))
            .alignment(Alignment::Center);
        frame.render_widget(empty_paragraph, area);
        return;
    }

    // Render the actual item list
    let list_items: Vec<ListItem> = app
        .items
        .iter()
        .map(|item| ListItem::new(item.identifier.clone()))
        .collect();

    let list = List::new(list_items)
        .block(list_block) // Attach block here
        .highlight_style(
            Style::default()
                .bg(Color::Blue)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("> ");

    frame.render_stateful_widget(list, area, &mut app.item_list_state);
}


/// Renders the item detail view.
fn render_item_view(app: &mut App, frame: &mut Frame, area: Rect) {
    let item_id = app.viewing_item_id.as_deref().unwrap_or("Unknown"); // Get the ID

    let collection_name = app.current_collection_name.as_deref().unwrap_or("Unknown");
    let outer_block = Block::default()
        .borders(Borders::ALL)
        .title(format!(
            "Item: {} / {} (Esc: Back, ↑/↓: Files, Enter/'d': File, 'b': All Files)",
            collection_name, item_id
        ))
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

    // Use the details if available
    if let Some(details) = &app.current_item_details { // Removed underscore from pattern
        let mut lines = Vec::new(); // Changed to Vec<Line>

        // Use app.current_item_details directly below where needed
        let details = app.current_item_details.as_ref().unwrap(); // Safe to unwrap due to if let

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
    // Use a block to potentially add a border later if desired
    let block = Block::default().title("Files");
    // let inner_area = block.inner(area); // Use area directly if no border
    frame.render_widget(block.clone(), area); // Render the block title/borders if any

    // Prefix `details` with `_` again to satisfy the compiler warning.
    if let Some(_details) = &app.current_item_details {
        // Use app.current_item_details directly below where needed
        let details = app.current_item_details.as_ref().unwrap(); // Safe to unwrap due to if let

        if details.files.is_empty() {
            let empty_msg = Paragraph::new("No files found for this item.")
                .style(Style::default().fg(Color::DarkGray))
                .alignment(Alignment::Center);
            // Render inside the block's inner area
            frame.render_widget(empty_msg, block.inner(area));
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

        // Render list inside the block's area
        frame.render_stateful_widget(list, block.inner(area), &mut app.file_list_state);
    }
}

/// Helper function to create a centered rectangle for popups.
fn centered_rect(percent_x: u16, height: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - height) / 2), // Empty space above
            Constraint::Length(height),                 // Popup height
            Constraint::Percentage((100 - height) / 2), // Empty space below
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2), // Empty space left
            Constraint::Percentage(percent_x),             // Popup width
            Constraint::Percentage((100 - percent_x) / 2), // Empty space right
        ])
        .split(popup_layout[1])[1] // Take the middle horizontal chunk
}

/// Renders a centered input box overlay for editing a setting.
fn render_editing_setting_input(app: &mut App, frame: &mut Frame) {
    let area = centered_rect(60, 3, frame.area()); // Use frame.area()

    let input_prompt = "Edit Value: ";
    let input_text = format!("{}{}", input_prompt, app.editing_setting_input);

    let input = Paragraph::new(input_text)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Editing Setting (Enter: Save, Esc: Cancel)")
                .border_style(Style::default().fg(Color::Yellow)),
        );

    frame.render_widget(Clear, area); // Clear the area behind the input box
    frame.render_widget(input, area);

    // Set cursor position
    frame.set_cursor_position((
        area.x + app.cursor_position as u16 + input_prompt.len() as u16,
        area.y + 1,
    ));
}

/// Renders a centered input box overlay for adding a new collection.
fn render_add_collection_input(app: &mut App, frame: &mut Frame) {
    let area = centered_rect(60, 3, frame.area()); // Use frame.area()

    let input_prompt = "Collection ID: ";
    let input_text = format!("{}{}", input_prompt, app.add_collection_input);

    let input = Paragraph::new(input_text)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Add Collection (Enter: Save, Esc: Cancel)")
                .border_style(Style::default().fg(Color::Yellow)),
        );

    frame.render_widget(Clear, area); // Clear the area behind the input box
    frame.render_widget(input, area);

    // Set cursor position
    frame.set_cursor_position((
        area.x + app.add_collection_cursor_pos as u16 + input_prompt.len() as u16,
        area.y + 1,
    ));
}

/// Renders a centered input box overlay for asking the download directory.
fn render_ask_download_dir_input(app: &mut App, frame: &mut Frame) {
    let area = centered_rect(80, 3, frame.area()); // Use frame.area()

    let input_prompt = "Download Path: ";
    // Reuse editing_setting_input for this temporary input
    let input_text = format!("{}{}", input_prompt, app.editing_setting_input);

    let input = Paragraph::new(input_text)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Set Download Directory (Enter: Save, Esc: Cancel)")
                .border_style(Style::default().fg(Color::Yellow)),
        );

    frame.render_widget(Clear, area); // Clear the area behind the input box
    frame.render_widget(input, area);

    // Set cursor position (reuse cursor_position from editing setting)
    frame.set_cursor_position((
        area.x + app.cursor_position as u16 + input_prompt.len() as u16,
        area.y + 1,
    ));
}


/// Renders the settings view.
fn render_settings_view(app: &mut App, frame: &mut Frame, area: Rect) {
    let settings_block = Block::default()
        .borders(Borders::ALL)
        .title("Settings (Esc: Save & Back, ↑/↓: Select, ←/→: Adjust)")
        .border_style(Style::default().fg(Color::Magenta)); // Distinct border color

    let inner_area = settings_block.inner(area);
    frame.render_widget(settings_block.clone(), area); // Render the block itself

    // Define settings items
    let download_dir_text = format!(
        "Download Directory: {}",
        app.settings.download_directory.as_deref().unwrap_or("Not Set")
    );

    let file_concurrency_text = format!(
        "Max Concurrent File Downloads: {} {}",
        app.settings.max_concurrent_downloads.map_or("Unlimited".to_string(), |n| n.to_string()),
        if app.selected_setting_index == 1 { "< >" } else { "" } // Hint for adjustment
    );

    let collection_concurrency_text = format!(
        "Max Concurrent Collection Downloads: {} {}",
        app.settings.max_concurrent_collections.map_or("Unlimited".to_string(), |n| n.to_string()),
        if app.selected_setting_index == 2 { "< >" } else { "" } // Hint for adjustment
    );


    let settings_items = vec![
        ListItem::new(download_dir_text), // Index 0
        ListItem::new(file_concurrency_text), // Index 1
        ListItem::new(collection_concurrency_text), // Index 2
    ];

    let list = List::new(settings_items)
        .highlight_style(
            Style::default()
                .bg(Color::DarkGray) // Different highlight for settings
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol(">> ");

    // Render the list inside the block's inner area
    frame.render_stateful_widget(list, inner_area, &mut app.settings_list_state);
}

/// Formats a download speed in bytes per second into a human-readable string (KB/s, MB/s, etc.).
fn format_speed(bytes_per_sec: f64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;

    if bytes_per_sec >= GB {
        format!("{:.2} GB/s", bytes_per_sec / GB)
    } else if bytes_per_sec >= MB {
        format!("{:.2} MB/s", bytes_per_sec / MB)
    } else if bytes_per_sec >= KB {
        format!("{:.1} KB/s", bytes_per_sec / KB)
    } else {
        format!("{:.0} B/s", bytes_per_sec)
    } // <-- Add missing closing brace here
}

/// Renders the status bar at the bottom of the screen.
fn render_status_bar(app: &mut App, frame: &mut Frame, area: Rect) {
    let status_text = if app.is_downloading {
        // Calculate speed if start time is available
        let speed_str = if let Some(start_time) = app.download_start_time {
            let elapsed = start_time.elapsed().as_secs_f64();
            if elapsed > 0.1 { // Avoid division by zero or tiny elapsed times
                let speed = app.total_bytes_downloaded as f64 / elapsed;
                format!(" ({})", format_speed(speed))
            } else {
                "".to_string() // Not enough time elapsed yet
            }
        } else {
            "".to_string() // Start time not set yet
        };

        // Format progress string if downloading
        let item_progress = app.total_items_to_download.map_or("?".to_string(), |t| t.to_string());
        let file_progress = app.total_files_to_download.map_or("?".to_string(), |t| t.to_string());
        format!(
            "Downloading [Items: {}/{} | Files: {}/{}{}]: {}", // Added speed, changed Last: to :
            app.items_downloaded_count,
            item_progress,
            app.files_downloaded_count,
            file_progress,
            speed_str, // Include speed string
            app.download_status.as_deref().unwrap_or("...") // Show last status message
        )
    } else if let Some(status) = &app.download_status {
        status.clone() // Clone the String status
    } else if let Some(err) = &app.error_message {
        err.clone() // Clone the String error
    } else if app.is_loading {
        "Fetching collection data...".to_string() // Convert literal to String
    } else if app.is_loading_details {
         "Fetching item details...".to_string()
    } else if app.current_state == AppState::AskingDownloadDir {
        // Status handled by the input overlay title
        " ".to_string() // Empty status bar while asking for dir
    } else if app.current_state == AppState::ViewingItem {
        // Status handled by the item view title
        " ".to_string()
    } else if app.current_state == AppState::SettingsView {
         // Status handled by the settings view title
         " ".to_string()
    } else if app.current_state == AppState::EditingSetting {
         // Status handled by the editing overlay title
         " ".to_string()
    } else if app.current_state == AppState::AddingCollection {
         // Status handled by the add collection overlay title
         " ".to_string()
    } else { // Browsing state
        match app.active_pane {
            ActivePane::Collections => "Collections Pane. 'q': Quit, 's': Settings, Tab: Switch, ↑/↓: Nav, Enter: Load, 'a': Add, Del: Remove, 'd'/'b': Download Collection".to_string(),
            ActivePane::Items => "Items Pane. 'q': Quit, 's': Settings, Tab: Switch, ↑/↓: Nav, Enter: View Details, 'd': Download Item, 'b': Download All Items".to_string(),
        }
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
