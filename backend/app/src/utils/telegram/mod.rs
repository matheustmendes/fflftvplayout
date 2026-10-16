//! The Telegram bot of the team: alerts when something goes wrong and
//! read-only commands to ask for the state of the channel.
//!
//! One bot serves the installation; every channel says which group it talks to.
//! The bot only reads. Commands that change what is on the air are not part of
//! it yet: see `.dev/telegram.md` for how they are meant to be added.

pub mod bot;
pub mod client;
pub mod commands;
pub mod events;
#[cfg(test)]
pub(crate) mod fake;
pub mod messages;
pub mod monitor;
pub mod snapshot;

pub use bot::run;
