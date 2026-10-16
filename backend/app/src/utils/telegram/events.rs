//! Things the playout reports as they happen. The monitor of the bot listens;
//! when nobody does, sending an event costs nothing.

use std::sync::LazyLock;

use tokio::sync::broadcast;

const CAPACITY: usize = 64;

static EVENTS: LazyLock<broadcast::Sender<Event>> =
    LazyLock::new(|| broadcast::channel(CAPACITY).0);

#[derive(Debug, Clone)]
pub enum Event {
    /// The output of the channel failed and the supervisor is restarting it.
    OutputFailed { channel: i32, reason: String },
}

pub fn emit(event: Event) {
    // An error only means that nobody is listening.
    let _ = EVENTS.send(event);
}

pub fn subscribe() -> broadcast::Receiver<Event> {
    EVENTS.subscribe()
}
