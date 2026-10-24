//! Tray / menu bar UI (spec 1.6). Built and updated on the UI thread.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use cvc_core::engine::State;
use tauri::image::Image;
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::tray::{MouseButton, TrayIcon, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, Runtime, WebviewUrl, WebviewWindowBuilder, Wry};

use crate::paths::AppPaths;
use crate::resident::{Snapshot, StartupView, Ui, View};

/// The Windows tray shows 16 px (32 px at 2x); the macOS menu bar 18 pt at up to 3x.
#[cfg(target_os = "windows")]
macro_rules! tray_size {
    () => {
        "32"
    };
}
#[cfg(not(target_os = "windows"))]
macro_rules! tray_size {
    () => {
        "64"
    };
}

pub struct TauriUi {
    pub app: AppHandle,
    pub paths: AppPaths,
    /// The tray and its menu are built once; later snapshots update the items in place,
    /// so a click on an open menu is never lost to a menu swap.
    tray: Mutex<Option<(TrayIcon, MenuHandles)>>,
    created: AtomicUsize,
    /// Read by the delay window.
    pub latest: Mutex<Option<Snapshot>>,
}

impl TauriUi {
    pub fn new(app: AppHandle, paths: AppPaths) -> Arc<Self> {
        Arc::new(Self { app, paths, tray: Mutex::new(None), created: AtomicUsize::new(0), latest: Mutex::new(None) })
    }

    fn render(self: &Arc<Self>, snapshot: &Snapshot) -> tauri::Result<()> {
        let model = menu_model(snapshot);
        let icon = icon(snapshot.status.state, snapshot.dark)?;
        let tooltip = format!("Codex Voice Clipboard: {}", snapshot.label);
        let mut tray = self.tray.lock().unwrap_or_else(|e| e.into_inner());
        match tray.as_mut() {
            Some((existing, menu)) => {
                menu.apply(&self.app, &model)?;
                existing.set_icon(Some(icon))?;
                existing.set_tooltip(Some(&tooltip))?;
            }
            None => {
                let mut menu = MenuHandles::build(&self.app)?;
                menu.apply(&self.app, &model)?;
                let built = TrayIconBuilder::with_id("main")
                    .icon(icon)
                    .icon_as_template(false)
                    .tooltip(&tooltip)
                    .menu(&menu.menu)
                    // macOS has no double-click on menu bar items, so a left click opens
                    // the menu there; Windows keeps the menu on right click and copies
                    // the latest record on double click (spec 1.5).
                    .show_menu_on_left_click(cfg!(not(target_os = "windows")))
                    .on_tray_icon_event(|tray, event| {
                        if let TrayIconEvent::DoubleClick { button: MouseButton::Left, .. } = event {
                            if let Some(channel) = tray.app_handle().try_state::<crate::Channel>() {
                                channel.send(crate::resident::Msg::CopyRecent(0));
                            }
                        }
                    })
                    .build(&self.app)?;
                self.created.fetch_add(1, Ordering::SeqCst);
                if let Some(root) = &self.paths.sandbox {
                    record_tray_creation(root);
                }
                *tray = Some((built, menu));
            }
        }
        Ok(())
    }
}

/// Sandbox evidence that this process put an icon up. A second instance must never
/// append here (spec 1.8).
fn record_tray_creation(root: &std::path::Path) {
    use std::io::Write;
    if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(root.join("tray-icons.log")) {
        let _ = writeln!(file, "{}", std::process::id());
    }
}

impl Ui for TauriUi {
    fn show(&self, snapshot: &Snapshot) {
        *self.latest.lock().unwrap_or_else(|e| e.into_inner()) = Some(snapshot.clone());
        let Some(this) = self.app.try_state::<Arc<TauriUi>>().map(|s| s.inner().clone()) else { return };
        let snapshot = snapshot.clone();
        let _ = self.app.run_on_main_thread(move || {
            let _ = this.render(&snapshot);
        });
    }

    fn tray_icons(&self) -> usize {
        self.created.load(Ordering::SeqCst)
    }

    fn confirm(&self, title: &str, message: &str) -> bool {
        use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
        self.app
            .dialog()
            .message(message)
            .title(title)
            .kind(MessageDialogKind::Info)
            .buttons(MessageDialogButtons::OkCancelCustom("はい".into(), "キャンセル".into()))
            .blocking_show()
    }

    fn open(&self, view: View) {
        match view {
            View::Notice => open_window(&self.app, "notice"),
        }
    }

    fn open_login_items_settings(&self) {
        #[cfg(target_os = "macos")]
        let _ = std::process::Command::new("open")
            .arg("x-apple.systempreferences:com.apple.LoginItems-Settings.extension")
            .spawn();
        #[cfg(target_os = "windows")]
        let _ = std::process::Command::new("explorer.exe").arg("ms-settings:startupapps").spawn();
    }

    fn launch_os_uninstaller(&self) -> bool {
        let Some(dir) = std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.to_path_buf())) else {
            return false;
        };
        std::process::Command::new(dir.join("uninstall.exe")).spawn().is_ok()
    }

    fn exit(&self, code: i32) {
        self.app.exit(code);
    }
}

/// Opens (or focuses) one of the small UI windows: `delay`, `log`, `notice`.
pub fn open_window<R: Runtime>(app: &AppHandle<R>, view: &str) {
    if let Some(window) = app.get_webview_window(view) {
        let _ = window.set_focus();
        return;
    }
    let (title, width, height) = match view {
        "delay" => ("コピーまでの待機時間", 380.0, 220.0),
        "log" => ("ログ", 720.0, 500.0),
        _ => ("クリップボード上書きについて", 480.0, 360.0),
    };
    let built = WebviewWindowBuilder::new(app, view, WebviewUrl::App(format!("index.html#{view}").into()))
        .title(title)
        .inner_size(width, height)
        .resizable(view == "log")
        .center()
        .focused(true)
        .build();
    if let Ok(window) = built {
        let _ = window.set_focus();
    }
}

/// The menu items whose text, check or enabled state follows the snapshot.
struct MenuHandles {
    menu: Menu<Wry>,
    status: MenuItem<Wry>,
    auto: CheckMenuItem<Wry>,
    latest: MenuItem<Wry>,
    recent: Submenu<Wry>,
    recent_items: Vec<MenuItem<Wry>>,
    delay: MenuItem<Wry>,
    startup: CheckMenuItem<Wry>,
}

impl MenuHandles {
    fn build(app: &AppHandle) -> tauri::Result<Self> {
        let status = MenuItem::with_id(app, "status", "", false, None::<&str>)?;
        let auto = CheckMenuItem::with_id(app, "auto", "自動コピー", true, false, None::<&str>)?;
        let latest = MenuItem::with_id(app, "latest", "最新をコピー", false, None::<&str>)?;
        let recent = Submenu::with_items(app, "最近10件からコピー", false, &[])?;
        let delay = MenuItem::with_id(app, "delay", "", true, None::<&str>)?;
        let startup = CheckMenuItem::with_id(app, "startup", "", true, false, None::<&str>)?;
        let log = MenuItem::with_id(app, "log", "ログを表示", true, None::<&str>)?;
        let notice = MenuItem::with_id(app, "notice", "クリップボード上書きについて", true, None::<&str>)?;
        let uninstall = MenuItem::with_id(app, "uninstall", "アンインストール…", true, None::<&str>)?;
        let quit = MenuItem::with_id(app, "quit", "終了", true, None::<&str>)?;
        let menu = Menu::with_items(
            app,
            &[
                &status,
                &PredefinedMenuItem::separator(app)?,
                &auto,
                &latest,
                &recent,
                &delay,
                &PredefinedMenuItem::separator(app)?,
                &startup,
                &log,
                &notice,
                &PredefinedMenuItem::separator(app)?,
                &uninstall,
                &quit,
            ],
        )?;
        Ok(Self { menu, status, auto, latest, recent, recent_items: Vec::new(), delay, startup })
    }

    /// Check marks are always written: a clicked check item flips its own mark, so the
    /// native state may differ from anything remembered here.
    fn apply(&mut self, app: &AppHandle, m: &MenuModel) -> tauri::Result<()> {
        self.status.set_text(m.status)?;
        self.auto.set_checked(m.auto_checked)?;
        self.latest.set_enabled(m.latest_enabled)?;
        self.delay.set_text(&m.delay)?;
        self.startup.set_text(m.startup_text)?;
        self.startup.set_checked(m.startup_checked)?;
        self.startup.set_enabled(m.startup_enabled)?;
        // Only the recent items are added or removed, and only the ones that changed.
        for (i, (id, text)) in m.recent.iter().enumerate() {
            match self.recent_items.get(i) {
                Some(item) => {
                    if item.text()? != *text {
                        item.set_text(text)?;
                    }
                }
                None => {
                    let item = MenuItem::with_id(app, id.as_str(), text, true, None::<&str>)?;
                    self.recent.append(&item)?;
                    self.recent_items.push(item);
                }
            }
        }
        while self.recent_items.len() > m.recent.len() {
            if let Some(item) = self.recent_items.pop() {
                self.recent.remove(&item)?;
            }
        }
        self.recent.set_enabled(m.recent_enabled)
    }
}

/// Windows menus treat `&` as an access-key marker.
fn menu_text(text: &str) -> String {
    if cfg!(target_os = "windows") {
        text.replace('&', "&&")
    } else {
        text.to_string()
    }
}

/// `dark` means a dark menu bar / taskbar, which takes the pale icons.
fn icon(state: State, dark: bool) -> tauri::Result<Image<'static>> {
    let bytes: &'static [u8] = match (state, dark) {
        (State::Active, true) => include_bytes!(concat!("../icons/tray/active-dark-", tray_size!(), ".png")),
        (State::Active, false) => include_bytes!(concat!("../icons/tray/active-light-", tray_size!(), ".png")),
        (State::Paused, true) => include_bytes!(concat!("../icons/tray/paused-dark-", tray_size!(), ".png")),
        (State::Paused, false) => include_bytes!(concat!("../icons/tray/paused-light-", tray_size!(), ".png")),
        (State::Error, true) => include_bytes!(concat!("../icons/tray/error-dark-", tray_size!(), ".png")),
        (State::Error, false) => include_bytes!(concat!("../icons/tray/error-light-", tray_size!(), ".png")),
    };
    Image::from_bytes(bytes)
}

/// What the menu shows for one snapshot. Pure, so it is tested without a tray.
#[derive(Clone, Debug, PartialEq)]
pub struct MenuModel {
    pub status: &'static str,
    pub auto_checked: bool,
    pub latest_enabled: bool,
    pub recent_enabled: bool,
    /// `(id, text)` per recent record, newest first.
    pub recent: Vec<(String, String)>,
    pub delay: String,
    pub startup_text: &'static str,
    pub startup_checked: bool,
    pub startup_enabled: bool,
}

pub fn menu_model(s: &Snapshot) -> MenuModel {
    let has_recent = !s.previews.is_empty();
    let (startup_text, startup_checked, startup_enabled) = match s.startup {
        StartupView::On => ("ログイン時に自動起動", true, true),
        StartupView::Off => ("ログイン時に自動起動", false, true),
        StartupView::DisabledByOs => ("自動起動: OSの設定で無効（設定を開く）", false, true),
        StartupView::Unavailable => ("ログイン時に自動起動（状態を読み取れません）", false, false),
    };
    MenuModel {
        status: s.label,
        auto_checked: s.status.auto_enabled,
        latest_enabled: has_recent,
        recent_enabled: has_recent,
        recent: s
            .previews
            .iter()
            .enumerate()
            .map(|(i, text)| (format!("recent-{i}"), format!("{}. {}", i + 1, menu_text(text))))
            .collect(),
        delay: format!("コピーまでの待機時間…（{:.1} 秒）", s.delay_seconds),
        startup_text,
        startup_checked,
        startup_enabled,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cvc_core::engine::Status;

    fn snapshot(previews: &[&str], startup: StartupView) -> Snapshot {
        let status = Status {
            state: State::Active,
            auto_enabled: true,
            history_error: false,
            watcher_healthy: true,
            clipboard_error: false,
            pending: false,
        };
        Snapshot {
            label: status.label(),
            status,
            previews: previews.iter().map(|p| p.to_string()).collect(),
            startup,
            delay_seconds: 2.0,
            dark: false,
        }
    }

    #[test]
    fn status_auto_and_delay_follow_the_snapshot() {
        let mut s = snapshot(&[], StartupView::Off);
        let model = menu_model(&s);
        assert_eq!("監視中: 自動コピー ON", model.status);
        assert!(model.auto_checked);
        assert_eq!("コピーまでの待機時間…（2.0 秒）", model.delay);
        s.status.auto_enabled = false;
        s.delay_seconds = 0.5;
        let model = menu_model(&s);
        assert!(!model.auto_checked);
        assert_eq!("コピーまでの待機時間…（0.5 秒）", model.delay);
    }

    #[test]
    fn recent_items_are_numbered_and_enabled_only_when_present() {
        let model = menu_model(&snapshot(&[], StartupView::Off));
        assert!(!model.latest_enabled);
        assert!(!model.recent_enabled);
        assert!(model.recent.is_empty());

        let model = menu_model(&snapshot(&["新しい", "古い"], StartupView::Off));
        assert!(model.latest_enabled);
        assert!(model.recent_enabled);
        assert_eq!(
            vec![("recent-0".to_string(), "1. 新しい".to_string()), ("recent-1".to_string(), "2. 古い".to_string())],
            model.recent
        );
    }

    #[test]
    fn startup_item_shows_each_real_state() {
        let cases = [
            (StartupView::On, "ログイン時に自動起動", true, true),
            (StartupView::Off, "ログイン時に自動起動", false, true),
            (StartupView::DisabledByOs, "自動起動: OSの設定で無効（設定を開く）", false, true),
            (StartupView::Unavailable, "ログイン時に自動起動（状態を読み取れません）", false, false),
        ];
        for (view, text, checked, enabled) in cases {
            let model = menu_model(&snapshot(&[], view));
            assert_eq!((text, checked, enabled), (model.startup_text, model.startup_checked, model.startup_enabled), "{view:?}");
        }
    }
}
