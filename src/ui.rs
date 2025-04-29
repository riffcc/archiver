use crate::app::{App, AppState}; // Import AppState
use ratatui::{
    prelude::{Alignment, Constraint, Direction, Frame, Layout, Rect}, // Add Alignment
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, List, ListItem, Paragraph},
};

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
    }

    render_status_bar(app, frame, main_layout[2]);
}

fn render_input(app: &mut App, frame: &mut Frame, area: Rect) {
    let (input_prompt, mut block_title) = match app.current_state {
         AppState::Browsing => ("> ", "Collection Name"),
         AppState::AskingDownloadDir => ("Enter Path: ", "Set Download Directory (Enter to save, Esc to cancel)"),
         AppState::ViewingItem => ("", "Collection Filter"), // Input not active, show default title
         AppState::Downloading => ("> ", "Collection Name"), // Or maybe disable input?
     };

    // Modify title based on filtering mode only when Browsing
    let border_style = if app.is_filtering_input && app.current_state == AppState::Browsing {
        block_title = "Collection Filter (Filtering)";
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
    if app.is_filtering_input && (app.current_state == AppState::Browsing || app.current_state == AppState::AskingDownloadDir) {
        frame.set_cursor_position((
            area.x + app.cursor_position as u16 + input_prompt.len() as u16, // Adjust for prompt length
            area.y + 1, // Inside the border
        ));
    }
}


fn render_item_list(app: &mut App, frame: &mut Frame, area: Rect) {
    let list_title = if app.is_filtering_input && app.current_state == AppState::Browsing {
        "Items (Press Esc to navigate)"
    } else if app.current_state == AppState::Browsing {
         "Items ('i' to filter, Enter to view, 'd' to download, Up/Down to navigate)"
    } else {
         "Items" // Default title if not browsing
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

    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!("Viewing Item: {} (Press Esc to go back)", item_id))
        .border_style(Style::default().fg(Color::Cyan)); // Highlight view border

    // TODO: Fetch and display actual metadata here later
    let text = vec![
        format!("Identifier: {}", item_id),
        " ".into(),
        "Metadata will be shown here.".into(),
        "File list will be shown here.".into(),
    ];

    let paragraph = Paragraph::new(text.join("\n"))
        .block(block)
        .alignment(Alignment::Left); // Or Center

    frame.render_widget(paragraph, area);
}


fn render_status_bar(app: &mut App, frame: &mut Frame, area: Rect) {
    let status_text = if let Some(err) = &app.error_message {
        err.as_str()
    } else if app.is_loading {
        "Fetching data..."
    } else if app.current_state == AppState::AskingDownloadDir {
        "Enter the full path for downloads. Esc to cancel."
    } else if app.current_state == AppState::ViewingItem {
        "Viewing item details. Press Esc to return to list."
    } else if app.is_filtering_input {
        "Filtering Input. Press Esc to navigate list, Enter to search."
    } else {
        "Navigating List. Press 'q' to quit, 'i' to filter, Enter to view, 'd' to download."
    };

    let status_style = if app.error_message.is_some() {
        Style::default().fg(Color::Red)
    } else {
        Style::default()
    };

    let status_paragraph = Paragraph::new(status_text).style(status_style);
    frame.render_widget(status_paragraph, area);
}
