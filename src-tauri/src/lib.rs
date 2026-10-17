//! Codex Voice Clipboard: Tauri 2 shell around `cvc-core` (spec 2).

mod args;
mod cleanup;
mod paths;
mod platform;
mod resident;
mod sandbox;
mod tray;

use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use cvc_core::uninstall;
use tauri::{Manager, RunEvent};

use crate::args::Args;
use crate::cleanup::Cleanup;
use crate::paths::AppPaths;
use crate::resident::{Msg, Platform, Resident, Ui};
use crate::sandbox::{SandboxAutostart, SandboxClipboard};
use crate::tray::TauriUi;

pub(crate) struct Channel(Mutex<Sender<Msg>>);

impl Channel {
    fn send(&self, msg: Msg) {
        let _ = self.0.lock().unwrap_or_else(|e| e.into_inner()).send(msg);
    }
}

/// Returns the process exit code.
pub fn run() -> i32 {
    let Ok(args) = args::parse(std::env::args().skip(1)) else {
        eprintln!("usage: codex-voice-clipboard [--sandbox <absolute path> [--no-watcher]] [--uninstall [--assume-yes]]");
        return 1;
    };
    match &args.sandbox {
        Some(root) => run_sandbox(root.clone(), &args),
        None => match AppPaths::real() {
            Some(paths) => run_tauri(paths, &args, None),
            None => 1,
        },
    }
}

fn run_sandbox(root: std::path::PathBuf, args: &Args) -> i32 {
    if std::fs::create_dir_all(&root).is_err() {
        return 1;
    }
    let lock = match sandbox::acquire_lock(&root) {
        Ok(Some(lock)) => lock,
        // A resident already owns this sandbox: hand over and leave without an icon.
        Ok(None) if args.uninstall => {
            if sandbox::forward(&root, "cleanup").is_err() {
                return 1;
            }
            return if sandbox::wait_for_release(&root, Duration::from_secs(30)) { 0 } else { 1 };
        }
        Ok(None) => return 0,
        Err(_) => return 1,
    };
    let paths = AppPaths::sandbox(&root);
    if args.uninstall {
        let mut autostart = SandboxAutostart::new(&root);
        let mut stop = || true;
        let ops = &mut Cleanup { paths: &paths, autostart: &mut autostart, stop: &mut stop, delete_app: false };
        return if uninstall::execute(ops).is_ok() { 0 } else { 1 };
    }
    run_tauri(paths, args, Some(lock))
}

fn run_tauri(paths: AppPaths, args: &Args, _sandbox_lock: Option<std::fs::File>) -> i32 {
    let sandboxed = paths.sandbox.is_some();
    let (tx, rx) = mpsc::channel::<Msg>();
    let mut builder = tauri::Builder::default();

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    if !sandboxed {
        // Must be the first plugin. A second launch forwards its arguments here and exits.
        builder = builder
            .plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
                if argv.iter().any(|a| a == "--uninstall") {
                    let confirmed = argv.iter().any(|a| a == "--assume-yes");
                    app.state::<Channel>().send(Msg::Cleanup { confirmed });
                }
            }))
            .plugin({
                let autostart = tauri_plugin_autostart::Builder::new().app_name(paths::APP_DIR);
                // A LaunchAgent at a fixed name, so `platform.rs` can read its approval state.
                #[cfg(target_os = "macos")]
                let autostart = autostart.macos_launcher(tauri_plugin_autostart::MacosLauncher::LaunchAgent);
                autostart.build()
            })
            .plugin(tauri_plugin_clipboard_manager::init());
    }

    let uninstall_only = args.uninstall;
    let assume_yes = args.assume_yes;
    let no_watcher = args.no_watcher;
    let app = builder
        .plugin(tauri_plugin_dialog::init())
        .manage(Channel(Mutex::new(tx.clone())))
        .invoke_handler(tauri::generate_handler![get_delay, set_delay, read_log])
        .setup(move |app| {
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);
            let handle = app.handle().clone();
            let ui = TauriUi::new(handle.clone(), paths.clone());
            app.manage(ui.clone());
            let platform = platform_for(&handle, sandboxed);
            if uninstall_only {
                // Real `--uninstall` with no resident running: clean up and leave.
                std::thread::spawn(move || {
                    let ok = assume_yes || ui.confirm("アンインストール", resident::CLEANUP_QUESTION);
                    if ok {
                        let mut autostart = platform.autostart;
                        let mut stop = || true;
                        let ops = &mut Cleanup { paths: &paths, autostart: &mut *autostart, stop: &mut stop, delete_app: false };
                        let _ = uninstall::execute(ops);
                    }
                    ui.exit(0);
                });
                return Ok(());
            }
            let resident = Resident::new(paths.clone(), platform, ui, (tx.clone(), rx), no_watcher);
            std::thread::Builder::new().name("history".into()).spawn(move || resident.run())?;
            Ok(())
        })
        .on_menu_event(|app, event| {
            let channel = app.state::<Channel>();
            match event.id().as_ref() {
                "auto" => channel.send(Msg::ToggleAuto),
                "latest" => channel.send(Msg::CopyRecent(0)),
                "startup" => channel.send(Msg::ToggleStartup),
                "uninstall" => channel.send(Msg::Uninstall { confirmed: false }),
                "quit" => channel.send(Msg::Quit),
                view @ ("delay" | "log" | "notice") => tray::open_window(app, view),
                id => {
                    if let Some(index) = id.strip_prefix("recent-").and_then(|i| i.parse().ok()) {
                        channel.send(Msg::CopyRecent(index));
                    }
                }
            }
        })
        .build(tauri::generate_context!());
    let Ok(app) = app else { return 1 };
    app.run_return(|_app, event| {
        // Closing the last small window must not end the resident.
        if let RunEvent::ExitRequested { api, code: None, .. } = event {
            api.prevent_exit();
        }
    })
}

fn platform_for(app: &tauri::AppHandle, sandboxed: bool) -> Platform {
    let state = app.state::<Arc<TauriUi>>();
    if let (true, Some(root)) = (sandboxed, state.paths.sandbox.as_ref()) {
        return Platform {
            clipboard: Box::new(SandboxClipboard::new(root)),
            autostart: Box::new(SandboxAutostart::new(root)),
            dark_mode: platform::dark_mode,
        };
    }
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    {
        Platform {
            clipboard: Box::new(platform::RealClipboard(app.clone())),
            autostart: Box::new(platform::RealAutostart(app.clone())),
            dark_mode: platform::dark_mode,
        }
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    unreachable!("only macOS and Windows are supported")
}

#[tauri::command]
fn get_delay(ui: tauri::State<'_, Arc<TauriUi>>) -> f64 {
    ui.latest.lock().unwrap_or_else(|e| e.into_inner()).as_ref().map_or(2.0, |s| s.delay_seconds)
}

#[tauri::command]
fn set_delay(seconds: f64, channel: tauri::State<'_, Channel>) {
    channel.send(Msg::SetDelay(seconds));
}

/// The log holds only time stamps and event codes.
#[tauri::command]
fn read_log(ui: tauri::State<'_, Arc<TauriUi>>) -> String {
    let log = &ui.paths.log;
    let mut older = log.as_os_str().to_owned();
    older.push(".1");
    let read = |p: &std::path::Path| std::fs::read_to_string(p).unwrap_or_default();
    read(std::path::Path::new(&older)) + &read(log)
}
