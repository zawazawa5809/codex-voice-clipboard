//! Delayed automatic copy with bounded retries (spec 1.4, 3.2).

use std::time::{Duration, Instant};

use crate::Transcript;

const MAX_ATTEMPTS: u32 = 8;
const RETRY: Duration = Duration::from_millis(250);

#[derive(Debug)]
pub struct ClipboardError;

pub trait Clipboard {
    fn set_text(&mut self, text: &str) -> Result<(), ClipboardError>;
}

/// Holds at most one pending copy. Driven by `tick`.
pub struct CopyCoordinator<C: Clipboard> {
    clipboard: C,
    enabled: bool,
    delay: Duration,
    pending: Option<Transcript>,
    due: Option<Instant>,
    attempts: u32,
    has_error: bool,
}

impl<C: Clipboard> CopyCoordinator<C> {
    pub fn new(clipboard: C) -> Self {
        Self {
            clipboard,
            enabled: true,
            delay: Duration::from_secs(2),
            pending: None,
            due: None,
            attempts: 0,
            has_error: false,
        }
    }
    pub fn clipboard(&self) -> &C {
        &self.clipboard
    }
    pub fn clipboard_mut(&mut self) -> &mut C {
        &mut self.clipboard
    }
    pub fn delay(&self) -> Duration {
        self.delay
    }
    /// Changing the delay cancels the pending copy (spec 1.4).
    pub fn set_delay(&mut self, delay: Duration) {
        self.delay = delay;
        self.cancel();
    }
    pub fn enabled(&self) -> bool {
        self.enabled
    }
    /// Either direction cancels the pending copy.
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
        self.cancel();
    }
    pub fn has_error(&self) -> bool {
        self.has_error
    }
    pub fn has_pending(&self) -> bool {
        self.pending.is_some()
    }
    pub fn cancel(&mut self) {
        self.pending = None;
        self.attempts = 0;
        self.has_error = false;
    }
    /// Only the newest record is scheduled; it replaces an older pending one.
    pub fn observe(&mut self, records: &[Transcript], now: Instant) {
        if let (true, Some(last)) = (self.enabled, records.last()) {
            self.schedule(last.clone(), now + self.delay);
        }
    }
    pub fn copy_manually(&mut self, record: Transcript, now: Instant) {
        self.schedule(record, now);
    }
    fn schedule(&mut self, record: Transcript, due: Instant) {
        self.pending = Some(record);
        self.due = Some(due);
        self.attempts = 0;
        self.has_error = false;
    }
    /// Returns the event code to log, if any.
    pub fn tick(&mut self, now: Instant) -> Option<&'static str> {
        let record = self.pending.as_ref()?;
        if self.due.is_some_and(|due| now < due) {
            return None;
        }
        self.attempts += 1;
        match self.clipboard.set_text(&record.text) {
            Ok(()) => {
                self.pending = None;
                self.has_error = false;
                Some("clipboard-copied")
            }
            Err(ClipboardError) if self.attempts >= MAX_ATTEMPTS => {
                self.pending = None;
                self.has_error = true;
                Some("clipboard-failed")
            }
            Err(ClipboardError) => {
                self.due = Some(now + RETRY);
                Some("clipboard-retry")
            }
        }
    }
}

impl<T: Clipboard + ?Sized> Clipboard for Box<T> {
    fn set_text(&mut self, text: &str) -> Result<(), ClipboardError> {
        (**self).set_text(text)
    }
}
