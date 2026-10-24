//! Platform-independent core of Codex Voice Clipboard (spec section 1).
//!
//! Nothing here touches the clipboard, login items, or the tray. Those sit behind
//! traits ([`copy::Clipboard`], [`startup::Autostart`], [`uninstall::UninstallOperations`])
//! so tests and `--sandbox` runs can swap in file-backed fakes.

pub mod copy;
pub mod history;
pub mod identity;
pub mod log;
pub mod startup;
pub mod uninstall;

pub use history::{HistoryBatch, HistorySession, Transcript};
pub mod engine;
pub mod settings;
