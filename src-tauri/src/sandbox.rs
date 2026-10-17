//! `--sandbox` adapters: every OS touch point becomes a file under the sandbox root, so
//! integration tests never reach the real clipboard, login items, or installation.

use std::fs::{self, File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use cvc_core::copy::{Clipboard, ClipboardError};
use cvc_core::startup::Autostart;

pub const COMMAND: &str = "command.txt";
const LOCK: &str = "instance.lock";

/// Writes `clipboard.txt`. `clipboard-failures.txt` holding N makes the next N writes
/// fail, which stands in for Windows clipboard contention.
pub struct SandboxClipboard {
    root: PathBuf,
}

impl SandboxClipboard {
    pub fn new(root: &Path) -> Self {
        Self { root: root.to_path_buf() }
    }
}

impl Clipboard for SandboxClipboard {
    fn set_text(&mut self, text: &str) -> Result<(), ClipboardError> {
        let failures = self.root.join("clipboard-failures.txt");
        if let Some(n) = fs::read_to_string(&failures).ok().and_then(|s| s.trim().parse::<u32>().ok()) {
            if n > 0 {
                fs::write(&failures, (n - 1).to_string()).map_err(|_| ClipboardError)?;
                return Err(ClipboardError);
            }
        }
        write_atomic(&self.root.join("clipboard.txt"), text.as_bytes()).map_err(|_| ClipboardError)
    }
}

/// `startup.txt` exists while registered. `startup-disabled-by-os.txt` simulates the
/// user switching the item off in OS settings.
pub struct SandboxAutostart {
    root: PathBuf,
}

impl SandboxAutostart {
    pub fn new(root: &Path) -> Self {
        Self { root: root.to_path_buf() }
    }
}

impl Autostart for SandboxAutostart {
    fn is_enabled(&self) -> io::Result<bool> {
        Ok(self.root.join("startup.txt").exists())
    }
    fn set_enabled(&mut self, enabled: bool) -> io::Result<()> {
        let path = self.root.join("startup.txt");
        if enabled {
            fs::write(path, "enabled")
        } else {
            remove_if_exists(&path)
        }
    }
    fn disabled_by_os(&self) -> Option<bool> {
        Some(self.root.join("startup-disabled-by-os.txt").exists())
    }
}

/// One resident per sandbox root, independent of any real instance.
pub fn acquire_lock(root: &Path) -> io::Result<Option<File>> {
    let file = OpenOptions::new().create(true).truncate(false).write(true).open(root.join(LOCK))?;
    match file.try_lock() {
        Ok(()) => Ok(Some(file)),
        Err(fs::TryLockError::WouldBlock) => Ok(None),
        Err(fs::TryLockError::Error(err)) => Err(err),
    }
}

/// Hands a command to the running sandbox resident.
pub fn forward(root: &Path, command: &str) -> io::Result<()> {
    write_atomic(&root.join(COMMAND), command.as_bytes())
}

/// Waits until the resident has released the lock (it has exited).
pub fn wait_for_release(root: &Path, timeout: Duration) -> bool {
    let end = Instant::now() + timeout;
    while Instant::now() < end {
        if matches!(acquire_lock(root), Ok(Some(_))) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    false
}

/// Takes the pending command, if any.
pub fn take_command(root: &Path) -> Option<String> {
    let path = root.join(COMMAND);
    let command = fs::read_to_string(&path).ok()?;
    fs::remove_file(&path).ok()?;
    Some(command.trim().to_string())
}

pub fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut temp = path.as_os_str().to_owned();
    temp.push(".tmp");
    fs::write(&temp, bytes)?;
    fs::rename(&temp, path)
}

pub fn remove_if_exists(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Err(err) if err.kind() != io::ErrorKind::NotFound => Err(err),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clipboard_writes_file_and_honours_failure_count() {
        let dir = tempfile::tempdir().unwrap();
        let mut clip = SandboxClipboard::new(dir.path());
        fs::write(dir.path().join("clipboard-failures.txt"), "2").unwrap();
        assert!(clip.set_text("a").is_err());
        assert!(clip.set_text("a").is_err());
        clip.set_text("日本語").unwrap();
        assert_eq!("日本語", fs::read_to_string(dir.path().join("clipboard.txt")).unwrap());
    }

    #[test]
    fn autostart_is_a_file_and_os_disable_is_simulated() {
        let dir = tempfile::tempdir().unwrap();
        let mut startup = SandboxAutostart::new(dir.path());
        assert!(!startup.is_enabled().unwrap());
        startup.set_enabled(true).unwrap();
        assert!(startup.is_enabled().unwrap());
        startup.set_enabled(false).unwrap();
        startup.set_enabled(false).unwrap();
        assert_eq!(Some(false), startup.disabled_by_os());
        fs::write(dir.path().join("startup-disabled-by-os.txt"), "").unwrap();
        assert_eq!(Some(true), startup.disabled_by_os());
    }

    #[test]
    fn lock_admits_one_holder_until_released() {
        let dir = tempfile::tempdir().unwrap();
        let held = acquire_lock(dir.path()).unwrap().unwrap();
        assert!(acquire_lock(dir.path()).unwrap().is_none());
        drop(held);
        assert!(wait_for_release(dir.path(), Duration::from_secs(1)));
    }

    #[test]
    fn commands_are_taken_once() {
        let dir = tempfile::tempdir().unwrap();
        forward(dir.path(), "pause\n").unwrap();
        assert_eq!(Some("pause".to_string()), take_command(dir.path()));
        assert_eq!(None, take_command(dir.path()));
    }
}
