use anyhow::Result;
use crossterm::event::{self, Event as CrosstermEvent, KeyEvent, MouseEvent};
use std::{
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

/// Terminal events.
#[derive(Clone, Copy, Debug)]
pub enum Event {
    /// Terminal tick.
    Tick,
    /// Key press.
    Key(KeyEvent),
    /// Mouse click/scroll.
    Mouse(MouseEvent),
    /// Terminal resize.
    Resize(u16, u16),
}

/// Terminal event handler.
#[derive(Debug)]
pub struct EventHandler {
    /// Event sender channel.
    _sender: mpsc::Sender<Event>, // Prefixed with _
    /// Event receiver channel.
    receiver: mpsc::Receiver<Event>,
    /// Event handler thread.
    _handler: thread::JoinHandle<()>, // Prefixed with _
}

impl EventHandler {
    /// Constructs a new instance of [`EventHandler`].
    pub fn new(tick_rate: u64) -> Self {
        let tick_rate = Duration::from_millis(tick_rate);
        let (sender, receiver) = mpsc::channel();
        let handler = {
            let sender = sender.clone();
            let sender = sender.clone(); // Keep the clone for the thread
            thread::spawn(move || {
                let mut last_tick = Instant::now();
                loop {
                    let timeout = tick_rate
                        .checked_sub(last_tick.elapsed())
                        .unwrap_or(tick_rate);

                    if event::poll(timeout).expect("unable to poll for event") {
                        match event::read().expect("unable to read event") {
                            CrosstermEvent::Key(e) => sender.send(Event::Key(e)), // Use the cloned sender
                            CrosstermEvent::Mouse(e) => sender.send(Event::Mouse(e)), // Use the cloned sender
                            CrosstermEvent::Resize(w, h) => sender.send(Event::Resize(w, h)), // Use the cloned sender
                            _ => Ok(()), // Ignore other event types
                        }
                        .expect("failed to send terminal event")
                    }

                    if last_tick.elapsed() >= tick_rate {
                        sender.send(Event::Tick).expect("failed to send tick event");
                        last_tick = Instant::now();
                    }
                }
            })
        };
        Self {
            _sender: sender, // Assign to the prefixed field
            receiver,
            _handler: handler, // Assign to the prefixed field
        }
    }

    /// Receive the next event from the handler thread.
    ///
    /// This function will block indefinitely until an event is received.
    pub async fn next(&self) -> Result<Event> {
        Ok(self.receiver.recv()?)
    }
}
