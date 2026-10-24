//! Real OS adapters. Official plugins are called only from here, and only outside
//! `--sandbox` mode.

use std::io;
use std::sync::mpsc;
use std::time::Duration;

use cvc_core::copy::{Clipboard, ClipboardError};
#[cfg(target_os = "macos")]
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

/// macOS login item through tauri-plugin-autostart. Windows uses `run_entry`.
#[cfg(target_os = "macos")]
pub struct RealAutostart(pub AppHandle);

#[cfg(target_os = "macos")]
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

/// The Windows login item, over the real HKCU.
#[cfg(target_os = "windows")]
pub fn windows_autostart() -> io::Result<crate::run_entry::RunEntry<hkcu::Hkcu>> {
    Ok(crate::run_entry::RunEntry { registry: hkcu::Hkcu, name: APP_DIR.into(), exe: std::env::current_exe()? })
}

/// Stands in when the exe path cannot be read; the menu then shows the item as unreadable.
#[cfg(target_os = "windows")]
pub struct UnavailableAutostart;

#[cfg(target_os = "windows")]
impl cvc_core::startup::Autostart for UnavailableAutostart {
    fn is_enabled(&self) -> io::Result<bool> {
        Err(io::Error::other("executable path unavailable"))
    }
    fn set_enabled(&mut self, _enabled: bool) -> io::Result<()> {
        Err(io::Error::other("executable path unavailable"))
    }
    fn disabled_by_os(&self) -> Option<bool> {
        None
    }
}

#[cfg(target_os = "windows")]
mod hkcu {
    use std::io;

    use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS, WIN32_ERROR};
    use windows_sys::Win32::System::Registry::{
        RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW, HKEY_CURRENT_USER, REG_SZ, REG_ROUTINE_FLAGS,
        RRF_RT_REG_BINARY, RRF_RT_REG_SZ,
    };

    use crate::run_entry::Registry;

    pub struct Hkcu;

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain([0]).collect()
    }

    fn check(status: WIN32_ERROR) -> io::Result<()> {
        if status == ERROR_SUCCESS { Ok(()) } else { Err(io::Error::from_raw_os_error(status as i32)) }
    }

    /// The value's bytes, or `None` when the key or value is absent.
    fn get(key: &str, name: &str, flags: REG_ROUTINE_FLAGS) -> io::Result<Option<Vec<u8>>> {
        let (key, name) = (wide(key), wide(name));
        let mut size = 0u32;
        // SAFETY: NUL-terminated strings; a null buffer asks for the size only.
        let status = unsafe {
            RegGetValueW(HKEY_CURRENT_USER, key.as_ptr(), name.as_ptr(), flags, std::ptr::null_mut(), std::ptr::null_mut(), &mut size)
        };
        if status == ERROR_FILE_NOT_FOUND {
            return Ok(None);
        }
        check(status)?;
        let mut data = vec![0u8; size as usize];
        // SAFETY: the buffer is `size` bytes, as passed.
        let status = unsafe {
            RegGetValueW(HKEY_CURRENT_USER, key.as_ptr(), name.as_ptr(), flags, std::ptr::null_mut(), data.as_mut_ptr().cast(), &mut size)
        };
        if status == ERROR_FILE_NOT_FOUND {
            return Ok(None);
        }
        check(status)?;
        data.truncate(size as usize);
        Ok(Some(data))
    }

    impl Registry for Hkcu {
        fn get_string(&self, key: &str, name: &str) -> io::Result<Option<String>> {
            Ok(get(key, name, RRF_RT_REG_SZ)?.map(|bytes| {
                let units: Vec<u16> = bytes.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
                String::from_utf16_lossy(&units).trim_end_matches('\0').to_string()
            }))
        }
        fn get_binary(&self, key: &str, name: &str) -> io::Result<Option<Vec<u8>>> {
            get(key, name, RRF_RT_REG_BINARY)
        }
        fn set_string(&mut self, key: &str, name: &str, value: &str) -> io::Result<()> {
            let (key, name, value) = (wide(key), wide(name), wide(value));
            // SAFETY: NUL-terminated strings; the data length includes the terminator.
            check(unsafe {
                RegSetKeyValueW(HKEY_CURRENT_USER, key.as_ptr(), name.as_ptr(), REG_SZ, value.as_ptr().cast(), (value.len() * 2) as u32)
            })
        }
        fn delete(&mut self, key: &str, name: &str) -> io::Result<()> {
            let (key, name) = (wide(key), wide(name));
            // SAFETY: NUL-terminated strings.
            let status = unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, key.as_ptr(), name.as_ptr()) };
            if status == ERROR_FILE_NOT_FOUND { Ok(()) } else { check(status) }
        }
    }
}

/// StartupApproved\Run: Task Manager / Settings → Startup apps write a 12-byte value;
/// an odd first byte means the user disabled the item.
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
