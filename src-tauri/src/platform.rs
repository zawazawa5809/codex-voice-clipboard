//! Real OS adapters. Official plugins are called only from here, and only outside
//! `--sandbox` mode.

use std::io;
use std::sync::mpsc;
use std::time::Duration;

use cvc_core::copy::{Clipboard, ClipboardError};
use cvc_core::startup::Autostart;
use tauri::AppHandle;

use crate::paths::APP_DIR;

/// Writes on the UI thread (spec 1.3) and waits for the result.
pub struct RealClipboard(pub AppHandle);

impl Clipboard for RealClipboard {
    fn set_text(&mut self, text: &str) -> Result<(), ClipboardError> {
        use tauri_plugin_clipboard_manager::ClipboardExt;
        let (tx, rx) = mpsc::channel();
        let app = self.0.clone();
        let text = text.to_owned();
        self.0
            .run_on_main_thread(move || {
                let _ = tx.send(app.clipboard().write_text(text).is_ok());
            })
            .map_err(|_| ClipboardError)?;
        match rx.recv_timeout(Duration::from_secs(2)) {
            Ok(true) => Ok(()),
            _ => Err(ClipboardError),
        }
    }
}

pub struct RealAutostart(pub AppHandle);

impl Autostart for RealAutostart {
    fn is_enabled(&self) -> io::Result<bool> {
        use tauri_plugin_autostart::ManagerExt;
        self.0.autolaunch().is_enabled().map_err(io::Error::other)
    }
    fn set_enabled(&mut self, enabled: bool) -> io::Result<()> {
        use tauri_plugin_autostart::ManagerExt;
        let manager = self.0.autolaunch();
        if enabled { manager.enable() } else { manager.disable() }.map_err(io::Error::other)
    }
    /// The plugin only checks that its registration exists, so the OS approval state
    /// is read here directly.
    fn disabled_by_os(&self) -> Option<bool> {
        os_approval::disabled_by_os()
    }
}

#[cfg(target_os = "macos")]
mod os_approval {
    use objc2_foundation::{NSString, NSURL};
    use objc2_service_management::{SMAppService, SMAppServiceStatus};

    /// The plugin registers `~/Library/LaunchAgents/<app name>.plist`. Since macOS 13 the
    /// user can switch it off under System Settings → General → Login Items, which
    /// leaves the file in place and reports `RequiresApproval`.
    pub fn disabled_by_os() -> Option<bool> {
        let plist = dirs::home_dir()?.join("Library/LaunchAgents").join(format!("{}.plist", super::APP_DIR));
        if !plist.exists() {
            return Some(false);
        }
        let url = NSURL::fileURLWithPath(&NSString::from_str(plist.to_str()?));
        // SAFETY: a plain class method taking a file URL.
        let status = unsafe { SMAppService::statusForLegacyURL(&url) };
        match status {
            SMAppServiceStatus::RequiresApproval => Some(true),
            SMAppServiceStatus::Enabled => Some(false),
            _ => None,
        }
    }
}

#[cfg(target_os = "windows")]
mod os_approval {
    use windows_sys::Win32::Foundation::ERROR_SUCCESS;
    use windows_sys::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_BINARY};

    const APPROVED: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run";

    /// Task Manager / Settings → Startup apps write a 12-byte value here; an odd first
    /// byte means the user disabled it. No value means nobody switched it off.
    pub fn disabled_by_os() -> Option<bool> {
        let key: Vec<u16> = APPROVED.encode_utf16().chain([0]).collect();
        let name: Vec<u16> = super::APP_DIR.encode_utf16().chain([0]).collect();
        let mut data = [0u8; 64];
        let mut size = data.len() as u32;
        // SAFETY: NUL-terminated strings and a buffer whose size is passed alongside.
        let status = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                name.as_ptr(),
                RRF_RT_REG_BINARY,
                std::ptr::null_mut(),
                data.as_mut_ptr().cast(),
                &mut size,
            )
        };
        if status != ERROR_SUCCESS {
            return Some(false);
        }
        Some(crate::platform::approval_disabled(&data[..size as usize]))
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
mod os_approval {
    pub fn disabled_by_os() -> Option<bool> {
        None
    }
}

#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub fn approval_disabled(value: &[u8]) -> bool {
    value.first().is_some_and(|b| b & 1 == 1)
}

pub fn dark_mode() -> bool {
    matches!(dark_light::detect(), Ok(dark_light::Mode::Dark))
}

#[cfg(test)]
mod tests {
    #[test]
    fn startup_approved_odd_first_byte_means_disabled() {
        assert!(!super::approval_disabled(&[0x02, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]));
        assert!(super::approval_disabled(&[0x03, 0, 0, 0]));
        assert!(!super::approval_disabled(&[0x06]));
        assert!(super::approval_disabled(&[0x07]));
        assert!(!super::approval_disabled(&[]));
    }
}
