//! The single worker thread (spec 1.3): reads history, schedules copies, applies menu
//! actions, and publishes a snapshot for the tray. Clipboard writes are marshalled to
//! the UI thread by the real clipboard adapter.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};

use cvc_core::copy::Clipboard;
use cvc_core::engine::{Engine, Status};
use cvc_core::log::SafeLog;
use cvc_core::settings::Settings;
use cvc_core::startup::{Autostart, StartupController};
use cvc_core::uninstall;
use notify::{RecommendedWatcher, RecursiveMode, Watcher};

use crate::cleanup::Cleanup;
use crate::paths::AppPaths;
use crate::sandbox;

const TICK: Duration = Duration::from_millis(100);
const POLL: Duration = Duration::from_secs(2);
const PREVIEW_CHARS: usize = 64;

#[derive(Debug)]
pub enum Msg {
    ToggleAuto,
    CopyRecent(usize),
    SetDelay(f64),
    ToggleStartup,
    /// The menu's アンインストール: cleanup, then the app itself. `confirmed: false` asks
    /// the user first (outside sandbox mode).
    Uninstall { confirmed: bool },
    /// A second launch with `--uninstall`: cleanup only, then exit (spec 1.9). On Windows
    /// this is the NSIS uninstaller's hook, which removes the files itself.
    Cleanup { confirmed: bool },
    Quit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StartupView {
    On,
    Off,
    DisabledByOs,
    Unavailable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum View {
    Notice,
}

/// Everything the tray shows. Transcript text appears only in `previews`, in memory.
#[derive(Clone, Debug, PartialEq)]
pub struct Snapshot {
    pub status: Status,
    pub label: &'static str,
    pub previews: Vec<String>,
    pub startup: StartupView,
    pub delay_seconds: f64,
    pub dark: bool,
}

/// The UI side, implemented over Tauri. Methods may be called from the worker thread.
pub trait Ui: Send + Sync + 'static {
    fn show(&self, snapshot: &Snapshot);
    fn tray_icons(&self) -> usize;
    /// Blocking question. Never called on the worker thread itself.
    fn confirm(&self, title: &str, message: &str) -> bool;
    fn open(&self, view: View);
    fn open_login_items_settings(&self);
    /// Windows hands removal to the NSIS uninstaller. Returns whether it launched.
    fn launch_os_uninstaller(&self) -> bool;
    fn exit(&self, code: i32);
}

pub struct Platform {
    pub clipboard: Box<dyn Clipboard + Send>,
    pub autostart: Box<dyn Autostart + Send>,
    pub dark_mode: fn() -> bool,
}

pub struct Resident<U: Ui> {
    paths: AppPaths,
    engine: Engine<Box<dyn Clipboard + Send>>,
    startup: StartupController<Box<dyn Autostart + Send>>,
    settings: Settings,
    ui: Arc<U>,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    no_watcher: bool,
    watcher: Option<RecommendedWatcher>,
    dirty: Arc<AtomicBool>,
    watcher_failed: Arc<AtomicBool>,
    next_poll: Instant,
    dark_mode: fn() -> bool,
    dark: bool,
    startup_view: StartupView,
    published: Option<Snapshot>,
    state_json: String,
}

enum Flow {
    Continue,
    Exit(i32),
}

impl<U: Ui> Resident<U> {
    pub fn new(paths: AppPaths, platform: Platform, ui: Arc<U>, channel: (Sender<Msg>, Receiver<Msg>), no_watcher: bool) -> Self {
        let settings = Settings::load(&paths.settings);
        let engine = Engine::new(&paths.history, platform.clipboard, SafeLog::new(&paths.log), &settings);
        Self {
            engine,
            startup: StartupController::new(platform.autostart),
            settings,
            ui,
            tx: channel.0,
            rx: channel.1,
            no_watcher,
            watcher: None,
            dirty: Arc::new(AtomicBool::new(true)),
            watcher_failed: Arc::new(AtomicBool::new(false)),
            next_poll: Instant::now(),
            dark_mode: platform.dark_mode,
            dark: (platform.dark_mode)(),
            startup_view: StartupView::Off,
            published: None,
            state_json: String::new(),
            paths,
        }
    }

    pub fn run(mut self) {
        self.engine.log("application-started");
        if self.paths.sandbox.is_none() && !self.settings.risk_notice_seen {
            self.ui.open(View::Notice);
            self.settings.risk_notice_seen = true;
            self.save_settings();
        }
        loop {
            let flow = match self.rx.recv_timeout(TICK) {
                Ok(msg) => self.handle(msg),
                Err(RecvTimeoutError::Timeout) => Flow::Continue,
                Err(RecvTimeoutError::Disconnected) => Flow::Exit(0),
            };
            let flow = match (flow, &self.paths.sandbox) {
                (Flow::Continue, Some(root)) => match sandbox::take_command(root) {
                    Some(command) => self.sandbox_command(&command),
                    None => Flow::Continue,
                },
                (flow, _) => flow,
            };
            if let Flow::Exit(code) = flow {
                self.ui.exit(code);
                return;
            }
            let now = Instant::now();
            self.engine.tick(now);
            let periodic = now >= self.next_poll;
            if self.dirty.swap(false, Ordering::SeqCst) || periodic {
                self.ensure_watcher();
                self.engine.poll_history(now);
            }
            if periodic {
                self.next_poll = now + POLL;
                self.dark = (self.dark_mode)();
                self.refresh_startup();
            }
            self.publish();
        }
    }

    fn handle(&mut self, msg: Msg) -> Flow {
        let now = Instant::now();
        match msg {
            Msg::ToggleAuto => {
                let enabled = !self.engine.status().auto_enabled;
                self.set_auto(enabled, now);
            }
            Msg::CopyRecent(index) => self.engine.copy_recent(index, now),
            Msg::SetDelay(seconds) => {
                self.engine.set_delay_seconds(seconds);
                self.save_settings();
            }
            Msg::ToggleStartup => self.toggle_startup(),
            Msg::Uninstall { confirmed } => return self.uninstall(confirmed, true),
            Msg::Cleanup { confirmed } => return self.uninstall(confirmed, false),
            Msg::Quit => return self.quit(),
        }
        Flow::Continue
    }

    fn sandbox_command(&mut self, command: &str) -> Flow {
        let now = Instant::now();
        match command {
            "pause" => self.set_auto(false, now),
            "resume" => self.set_auto(true, now),
            "manual" => self.engine.copy_recent(0, now),
            "startup-on" | "startup-off" => {
                if self.startup.set_enabled(command == "startup-on").is_err() {
                    self.engine.log("startup-change-failed");
                }
                self.refresh_startup();
            }
            "uninstall" => return self.uninstall(true, true),
            "cleanup" => return self.uninstall(true, false),
            "exit" => return self.quit(),
            _ => self.engine.log("sandbox-command-unknown"),
        }
        Flow::Continue
    }

    fn set_auto(&mut self, enabled: bool, now: Instant) {
        self.engine.set_auto(enabled, now);
        self.save_settings();
    }

    fn save_settings(&mut self) {
        self.settings = self.engine.settings(&self.settings);
        if self.settings.save(&self.paths.settings).is_err() {
            self.engine.log("settings-save-failed");
        }
    }

    fn toggle_startup(&mut self) {
        // Never switch an OS-disabled item back on behind the user's back (spec 2).
        if self.startup.store().disabled_by_os() == Some(true) {
            let ui = self.ui.clone();
            std::thread::spawn(move || {
                if ui.confirm("自動起動", "自動起動はOSの設定で無効になっています。設定を開いて切り替えますか？") {
                    ui.open_login_items_settings();
                }
            });
            return;
        }
        let wanted = !matches!(self.startup_view, StartupView::On);
        let code = match self.startup.set_enabled(wanted) {
            Ok(_) => "startup-changed",
            Err(_) => "startup-change-failed",
        };
        self.engine.log(code);
        self.refresh_startup();
    }

    fn refresh_startup(&mut self) {
        self.startup_view = match (self.startup.enabled(), self.startup.store().disabled_by_os()) {
            (Ok(true), Some(true)) => StartupView::DisabledByOs,
            (Ok(true), _) => StartupView::On,
            (Ok(false), _) => StartupView::Off,
            (Err(_), _) => StartupView::Unavailable,
        };
    }

    fn uninstall(&mut self, confirmed: bool, delete_app: bool) -> Flow {
        if !confirmed && self.paths.sandbox.is_none() {
            let (ui, tx) = (self.ui.clone(), self.tx.clone());
            std::thread::spawn(move || {
                let question = if delete_app { UNINSTALL_QUESTION } else { CLEANUP_QUESTION };
                if ui.confirm("アンインストール", question) {
                    let _ = tx.send(if delete_app { Msg::Uninstall { confirmed: true } } else { Msg::Cleanup { confirmed: true } });
                }
            });
            return Flow::Continue;
        }
        if delete_app && self.paths.sandbox.is_none() && cfg!(target_os = "windows") {
            // The NSIS uninstaller calls `--uninstall` itself before removing files.
            return if self.ui.launch_os_uninstaller() { self.quit() } else { self.uninstall_failed() };
        }
        let watcher = &mut self.watcher;
        let engine = &mut self.engine;
        let mut stop = || {
            *watcher = None;
            engine.cancel();
            true
        };
        let result = uninstall::execute(&mut Cleanup {
            paths: &self.paths,
            autostart: &mut **self.startup.store_mut(),
            stop: &mut stop,
            delete_app,
        });
        match result {
            // Nothing is logged after success: the log was just removed.
            Ok(()) => Flow::Exit(0),
            Err(_) => self.uninstall_failed(),
        }
    }

    fn uninstall_failed(&mut self) -> Flow {
        self.engine.log("uninstall-failed");
        Flow::Continue
    }

    fn quit(&mut self) -> Flow {
        self.watcher = None;
        self.engine.cancel();
        self.engine.log("application-stopped");
        Flow::Exit(0)
    }

    fn ensure_watcher(&mut self) {
        if self.no_watcher {
            return;
        }
        let Some(dir) = self.paths.history.parent().filter(|d| d.is_dir()) else {
            self.watcher = None;
            self.engine.set_watcher_healthy(false);
            return;
        };
        if self.watcher.is_some() && !self.watcher_failed.load(Ordering::SeqCst) {
            return;
        }
        self.watcher = None;
        self.watcher_failed.store(false, Ordering::SeqCst);
        let name = self.paths.history.file_name().map(|n| n.to_os_string()).unwrap_or_default();
        match watch(dir, name, self.dirty.clone(), self.watcher_failed.clone()) {
            Ok(watcher) => {
                self.watcher = Some(watcher);
                self.engine.set_watcher_healthy(true);
            }
            Err(_) => self.engine.set_watcher_healthy(false),
        }
    }

    fn publish(&mut self) {
        let status = self.engine.status();
        let snapshot = Snapshot {
            label: status.label(),
            status,
            previews: self.engine.recent().iter().map(|r| preview(&r.text)).collect(),
            startup: self.startup_view,
            delay_seconds: self.engine.delay().as_secs_f64(),
            dark: self.dark,
        };
        if self.published.as_ref() != Some(&snapshot) {
            self.ui.show(&snapshot);
            self.published = Some(snapshot);
        }
        if let Some(root) = self.paths.sandbox.clone() {
            self.write_state(&root);
        }
    }

    /// Test-facing state for the sandbox. Counts only, never text.
    fn write_state(&mut self, root: &Path) {
        let Some(snapshot) = &self.published else { return };
        let s = &snapshot.status;
        let json = serde_json::json!({
            "pid": std::process::id(),
            "recentCount": snapshot.previews.len(),
            "historyError": s.history_error,
            "watcherHealthy": self.watcher.is_some() && s.watcher_healthy,
            "autoEnabled": s.auto_enabled,
            "pending": s.pending,
            "clipboardError": s.clipboard_error,
            "state": s.state.code(),
            "trayIcons": self.ui.tray_icons(),
            "startup": format!("{:?}", snapshot.startup),
        })
        .to_string();
        if json != self.state_json && sandbox::write_atomic(&root.join("state.json"), json.as_bytes()).is_ok() {
            self.state_json = json;
        }
    }
}

pub const CLEANUP_QUESTION: &str =
    "自動起動の登録・設定・ログを削除して終了します。文字起こしの履歴（~/.codex）は削除しません。よろしいですか？";

const UNINSTALL_QUESTION: &str =
    "自動起動の登録・設定・ログを削除し、アプリ本体も削除して終了します。文字起こしの履歴（~/.codex）は削除しません。よろしいですか？";

/// Codex writes other files in `~/.codex`; only events about the history file (or a
/// watcher error) trigger an early poll.
fn watch(dir: &Path, name: std::ffi::OsString, dirty: Arc<AtomicBool>, failed: Arc<AtomicBool>) -> notify::Result<RecommendedWatcher> {
    let mut watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| match event {
        Ok(event) => {
            if event.need_rescan() || event.paths.iter().any(|p| p.file_name() == Some(name.as_os_str())) {
                dirty.store(true, Ordering::SeqCst);
            }
        }
        Err(_) => {
            failed.store(true, Ordering::SeqCst);
            dirty.store(true, Ordering::SeqCst);
        }
    })?;
    watcher.watch(dir, RecursiveMode::NonRecursive)?;
    Ok(watcher)
}

/// One menu line: newlines flattened, cut at 64 characters.
pub fn preview(text: &str) -> String {
    let flat: String = text.chars().map(|c| if c == '\r' || c == '\n' { ' ' } else { c }).collect();
    let mut chars = flat.chars();
    let head: String = chars.by_ref().take(PREVIEW_CHARS).collect();
    if chars.next().is_some() {
        head + "…"
    } else {
        head
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_flattens_lines_and_cuts_by_character() {
        assert_eq!("a b  c", preview("a\nb\r\nc"));
        let long = "日".repeat(70);
        assert_eq!(format!("{}…", "日".repeat(64)), preview(&long));
        assert_eq!("日".repeat(64), preview(&"日".repeat(64)));
    }
}
