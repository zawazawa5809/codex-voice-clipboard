//! Event-code-only log (spec 1.7). Never accepts free text.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

const ROTATE_AT: u64 = 512 * 1024;

#[derive(Debug, PartialEq, Eq)]
pub struct InvalidEventCode;

pub struct SafeLog {
    path: PathBuf,
    lock: Mutex<()>,
}

impl SafeLog {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into(), lock: Mutex::new(()) }
    }
    pub fn path(&self) -> &Path {
        &self.path
    }
    /// Rejects anything but `^[a-z]+(-[a-z]+)*$`. I/O failures are swallowed: logging
    /// must never stop recovery.
    pub fn write(&self, code: &str) -> Result<(), InvalidEventCode> {
        if !is_event_code(code) {
            return Err(InvalidEventCode);
        }
        let _guard = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        let _ = self.append(code);
        Ok(())
    }
    fn append(&self, code: &str) -> std::io::Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        if fs::metadata(&self.path).is_ok_and(|m| m.len() > ROTATE_AT) {
            let mut rotated = self.path.clone().into_os_string();
            rotated.push(".1");
            fs::rename(&self.path, rotated)?;
        }
        let stamp = chrono::Local::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, false);
        let mut file = OpenOptions::new().create(true).append(true).open(&self.path)?;
        writeln!(file, "{stamp} {code}")
    }
}

pub fn is_event_code(code: &str) -> bool {
    !code.is_empty()
        && code.split('-').all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_lowercase()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_code_grammar() {
        for ok in ["a", "clipboard-copied", "state-error"] {
            assert!(is_event_code(ok), "{ok}");
        }
        for bad in ["", "-a", "a-", "a--b", "A", "a b", "a1", "日本語", "a\n"] {
            assert!(!is_event_code(bad), "{bad:?}");
        }
    }

    #[test]
    fn rotates_one_generation_past_512_kib() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("app.log");
        fs::write(&path, vec![b'x'; 512 * 1024 + 1]).unwrap();
        let log = SafeLog::new(&path);
        log.write("after-rotate").unwrap();
        assert!(fs::read_to_string(&path).unwrap().contains("after-rotate"));
        assert_eq!(512 * 1024 + 1, fs::metadata(dir.path().join("app.log.1")).unwrap().len());
    }
}
