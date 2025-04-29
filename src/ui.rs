use crate::app::App;
use ratatui::{
    prelude::{Frame, Rect},
    widgets::{Block, Borders, Paragraph},
};

/// Renders the user interface widgets.
pub fn render(app: &mut App, frame: &mut Frame) {
    // This is where you add new widgets.
    // See the following resources:
    // - https://docs.rs/ratatui/latest/ratatui/widgets/index.html
    // - https://github.com/ratatui-org/ratatui/tree/master/examples
    frame.render_widget(
        Paragraph::new(format!(
            "
        Press `Esc`, `Ctrl-C` or `q` to stop running.\n\
        Running: {}\n\
        ",
            app.running
        ))
        .block(
            Block::default()
                .title("Template")
                .borders(Borders::ALL),
        ),
        frame.size(),
    );
}
