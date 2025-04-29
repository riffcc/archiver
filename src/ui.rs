use crate::app::App;
use ratatui::{
    prelude::{Constraint, Direction, Frame, Layout, Rect},
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, List, ListItem, Paragraph},
};
use ratatui_widgets::widgets::Input; // Import the Input widget from the correct module

/// Renders the user interface widgets.
pub fn render(app: &mut App, frame: &mut Frame) {
    let main_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Input field
            Constraint::Min(0),    // List of items
            Constraint::Length(1), // Status/Error message
        ])
        .split(frame.size());

    render_input(app, frame, main_layout[0]);
    render_item_list(app, frame, main_layout[1]);
    render_status_bar(app, frame, main_layout[2]);
}

fn render_input(app: &mut App, frame: &mut Frame, area: Rect) {
    let input = Input::default()
        .value(&app.collection_input)
        .title("Collection Name")
        .borders(Borders::ALL)
        .prompt(" > "); // Optional: Add a prompt indicator

    frame.render_widget(input, area);
    // Make the cursor visible and styled
    frame.set_cursor(
        // Put cursor past the end of the input text
        area.x + app.cursor_position as u16 + 3, // +3 for border and prompt "> "
        // Move one line down, from the border to the input line
        area.y + 1,
    );
}

fn render_item_list(app: &mut App, frame: &mut Frame, area: Rect) {
    let list_block = Block::default().borders(Borders::ALL).title("Items");

    if app.is_loading {
        let loading_paragraph = Paragraph::new("Loading...")
            .block(list_block)
            .style(Style::default().fg(Color::Yellow));
        frame.render_widget(loading_paragraph, area);
        return;
    }

    if app.items.is_empty() && !app.collection_input.is_empty() && !app.is_loading {
        let empty_msg = if let Some(err) = &app.error_message {
             format!("Error fetching items: {}", err)
        } else if !app.collection_input.is_empty() {
            "No items found for this collection, or press Enter to search.".to_string()
        } else {
            "Enter a collection name above and press Enter.".to_string()
        };

        let empty_paragraph = Paragraph::new(empty_msg)
            .block(list_block)
            .style(Style::default().fg(Color::DarkGray));
        frame.render_widget(empty_paragraph, area);
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


fn render_status_bar(app: &mut App, frame: &mut Frame, area: Rect) {
    let status_text = if let Some(err) = &app.error_message {
        err.as_str()
    } else if app.is_loading {
        "Fetching data..."
    } else {
        "Ready. Press 'q' to quit."
    };

    let status_style = if app.error_message.is_some() {
        Style::default().fg(Color::Red)
    } else {
        Style::default()
    };

    let status_paragraph = Paragraph::new(status_text).style(status_style);
    frame.render_widget(status_paragraph, area);
}
