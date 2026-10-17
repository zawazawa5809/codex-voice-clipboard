//! The resident's state machine (spec 1.3–1.6), free of any UI or OS call.
//!
//! One thread owns it and calls it in sequence, so a scan, a manual copy, a pause and a
//! delay change can never interleave. That replaces the generation counters the .NET
//! tray needed for its async scans.

use std::path::Path;
use std::time::{Duration, Instant};

use crate::copy::{Clipboard, CopyCoordinator};
use crate::log::SafeLog;
use crate::settings::{normalize_delay, Settings};
use crate::{HistorySession, Transcript};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Active,
    Paused,
    Error,
}

impl State {
    pub fn code(self) -> &'static str {
        match self {
            State::Active => "active",
            State::Paused => "paused",
            State::Error => "error",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Status {
    pub state: State,
    pub auto_enabled: bool,
    pub history_error: bool,
    pub watcher_healthy: bool,
    pub clipboard_error: bool,
    pub pending: bool,
}

impl Status {
    /// The menu's status line (spec 1.6).
    pub fn label(&self) -> &'static str {
        if self.history_error {
            "エラー: 履歴を読み取れません"
        } else if self.clipboard_error {
            "エラー: コピー失敗（手動で再試行可）"
        } else if !self.watcher_healthy {
            "エラー: 通知監視失敗（定期確認中）"
        } else if self.auto_enabled {
            "監視中: 自動コピー ON"
        } else {
            "一時停止中: 手動コピー可能"
        }
    }
}

pub struct Engine<C: Clipboard> {
    history: HistorySession,
    copies: CopyCoordinator<C>,
    log: SafeLog,
    history_error: bool,
    watcher_healthy: bool,
    /// Until the first poll the history is only presumed unreadable, so no state is logged.
    polled: bool,
    logged_state: Option<State>,
}

impl<C: Clipboard> Engine<C> {
    pub fn new(history: &Path, clipboard: C, log: SafeLog, settings: &Settings) -> Self {
        let mut copies = CopyCoordinator::new(clipboard);
        copies.set_delay(Duration::from_secs_f64(normalize_delay(settings.delay_seconds)));
        copies.set_enabled(settings.auto_copy);
        Self {
            history: HistorySession::new(history),
            copies,
            log,
            history_error: true,
            watcher_healthy: true,
            polled: false,
            logged_state: None,
        }
    }

    pub fn log(&self, code: &str) {
        let _ = self.log.write(code);
    }

    pub fn poll_history(&mut self, now: Instant) {
        match self.history.poll() {
            Ok(batch) => {
                self.history_error = false;
                self.copies.observe(&batch.new_records, now);
                if batch.invalid_lines > 0 {
                    self.log("history-invalid-record");
                }
            }
            Err(_) => {
                if !self.history_error {
                    self.log("history-unavailable");
                }
                self.history_error = true;
            }
        }
        self.polled = true;
        self.note_state();
    }

    pub fn tick(&mut self, now: Instant) {
        if let Some(code) = self.copies.tick(now) {
            self.log(code);
        }
        self.note_state();
    }

    /// Resuming first drains whatever was written while paused, with copying still
    /// off, so a quick pause → append → resume never auto-copies the paused record.
    pub fn set_auto(&mut self, enabled: bool, now: Instant) {
        if enabled == self.copies.enabled() {
            return;
        }
        self.copies.set_enabled(false);
        if enabled {
            self.poll_history(now);
            self.copies.set_enabled(true);
        }
        self.log(if enabled { "auto-copy-enabled" } else { "auto-copy-paused" });
        self.note_state();
    }

    /// Index 0 is the newest. Out-of-range indexes do nothing.
    pub fn copy_recent(&mut self, index: usize, now: Instant) {
        if let Some(record) = self.history.recent().get(index).cloned() {
            self.copies.copy_manually(record, now);
            self.tick(now);
        }
    }

    pub fn set_delay_seconds(&mut self, seconds: f64) {
        self.copies.set_delay(Duration::from_secs_f64(normalize_delay(seconds)));
        self.note_state();
    }

    pub fn delay(&self) -> Duration {
        self.copies.delay()
    }

    pub fn set_watcher_healthy(&mut self, healthy: bool) {
        if self.watcher_healthy && !healthy {
            self.log("watcher-unavailable");
        }
        self.watcher_healthy = healthy;
        self.note_state();
    }

    pub fn cancel(&mut self) {
        self.copies.cancel();
    }

    pub fn recent(&self) -> &[Transcript] {
        self.history.recent()
    }

    pub fn status(&self) -> Status {
        let auto_enabled = self.copies.enabled();
        let clipboard_error = self.copies.has_error();
        let state = if self.history_error || clipboard_error || !self.watcher_healthy {
            State::Error
        } else if auto_enabled {
            State::Active
        } else {
            State::Paused
        };
        Status {
            state,
            auto_enabled,
            history_error: self.history_error,
            watcher_healthy: self.watcher_healthy,
            clipboard_error,
            pending: self.copies.has_pending(),
        }
    }

    /// `base` with this engine's auto-copy and delay applied.
    pub fn settings(&self, base: &Settings) -> Settings {
        Settings {
            auto_copy: self.copies.enabled(),
            delay_seconds: self.copies.delay().as_secs_f64(),
            ..base.clone()
        }
    }

    fn note_state(&mut self) {
        if !self.polled {
            return;
        }
        let state = self.status().state;
        if self.logged_state != Some(state) {
            self.logged_state = Some(state);
            self.log(match state {
                State::Active => "state-active",
                State::Paused => "state-paused",
                State::Error => "state-error",
            });
        }
    }
}
