//! Windows login item: a value under HKCU\...\Run (spec 1.6, 2).
//!
//! Written here instead of through tauri-plugin-autostart, whose value is the bare exe
//! path plus a space. The install folder contains spaces, so that value is an unquoted
//! path, and its StartupApproved value outlives both disable and uninstall.

use std::io;
use std::path::PathBuf;

use cvc_core::startup::Autostart;

pub const RUN: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
/// Task Manager / Settings → Startup apps record the user's on/off choice here.
pub const APPROVED: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run";

/// HKCU values, injected so tests never touch the real registry.
pub trait Registry {
    fn get_string(&self, key: &str, name: &str) -> io::Result<Option<String>>;
    fn get_binary(&self, key: &str, name: &str) -> io::Result<Option<Vec<u8>>>;
    fn set_string(&mut self, key: &str, name: &str, value: &str) -> io::Result<()>;
    /// Succeeds when the value is already absent.
    fn delete(&mut self, key: &str, name: &str) -> io::Result<()>;
}

pub struct RunEntry<R: Registry> {
    pub registry: R,
    pub name: String,
    pub exe: PathBuf,
}

impl<R: Registry> RunEntry<R> {
    /// The quoted exe path, so a space in the folder name cannot split it.
    pub fn command(&self) -> String {
        format!("\"{}\"", self.exe.display())
    }
}

impl<R: Registry> Autostart for RunEntry<R> {
    fn is_enabled(&self) -> io::Result<bool> {
        Ok(self.registry.get_string(RUN, &self.name)?.is_some())
    }
    fn set_enabled(&mut self, enabled: bool) -> io::Result<()> {
        if enabled {
            let command = self.command();
            self.registry.set_string(RUN, &self.name, &command)
        } else {
            self.registry.delete(RUN, &self.name)?;
            self.registry.delete(APPROVED, &self.name)
        }
    }
    /// No value means nobody switched it off.
    fn disabled_by_os(&self) -> Option<bool> {
        match self.registry.get_binary(APPROVED, &self.name) {
            Ok(Some(value)) => Some(crate::platform::approval_disabled(&value)),
            Ok(None) => Some(false),
            Err(_) => None,
        }
    }
    fn remove_leftovers(&mut self) -> io::Result<()> {
        self.registry.delete(APPROVED, &self.name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[derive(Default)]
    struct FakeRegistry {
        strings: HashMap<(String, String), String>,
        binaries: HashMap<(String, String), Vec<u8>>,
        fail_reads: bool,
    }

    fn k(key: &str, name: &str) -> (String, String) {
        (key.to_string(), name.to_string())
    }

    impl Registry for FakeRegistry {
        fn get_string(&self, key: &str, name: &str) -> io::Result<Option<String>> {
            if self.fail_reads {
                return Err(io::Error::other("denied"));
            }
            Ok(self.strings.get(&k(key, name)).cloned())
        }
        fn get_binary(&self, key: &str, name: &str) -> io::Result<Option<Vec<u8>>> {
            if self.fail_reads {
                return Err(io::Error::other("denied"));
            }
            Ok(self.binaries.get(&k(key, name)).cloned())
        }
        fn set_string(&mut self, key: &str, name: &str, value: &str) -> io::Result<()> {
            self.strings.insert(k(key, name), value.to_string());
            Ok(())
        }
        fn delete(&mut self, key: &str, name: &str) -> io::Result<()> {
            self.strings.remove(&k(key, name));
            self.binaries.remove(&k(key, name));
            Ok(())
        }
    }

    const EXE: &str = r"C:\Users\me\AppData\Local\Codex Voice Clipboard\codex-voice-clipboard.exe";

    fn entry() -> RunEntry<FakeRegistry> {
        RunEntry { registry: FakeRegistry::default(), name: "CodexVoiceClipboard".into(), exe: PathBuf::from(EXE) }
    }

    #[test]
    fn enabling_writes_the_quoted_exe_path_with_nothing_after_it() {
        let mut e = entry();
        e.set_enabled(true).unwrap();
        let value = e.registry.strings[&k(RUN, "CodexVoiceClipboard")].clone();
        assert_eq!(format!("\"{EXE}\""), value);
        assert_eq!(value, e.command());
    }

    #[test]
    fn enabled_only_while_the_run_value_exists() {
        let mut e = entry();
        assert!(!e.is_enabled().unwrap());
        e.set_enabled(true).unwrap();
        assert!(e.is_enabled().unwrap());
        e.set_enabled(false).unwrap();
        assert!(!e.is_enabled().unwrap());
    }

    #[test]
    fn disabling_also_removes_the_startup_approved_value() {
        let mut e = entry();
        e.set_enabled(true).unwrap();
        e.registry.binaries.insert(k(APPROVED, "CodexVoiceClipboard"), vec![2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        e.set_enabled(false).unwrap();
        assert!(e.registry.strings.is_empty());
        assert!(e.registry.binaries.is_empty());
    }

    #[test]
    fn other_apps_values_are_left_alone() {
        let mut e = entry();
        e.registry.strings.insert(k(RUN, "Other"), "other.exe".into());
        e.registry.binaries.insert(k(APPROVED, "Other"), vec![3]);
        e.set_enabled(true).unwrap();
        e.set_enabled(false).unwrap();
        assert_eq!(1, e.registry.strings.len());
        assert_eq!(1, e.registry.binaries.len());
    }

    #[test]
    fn read_failure_is_reported_not_treated_as_off() {
        let mut e = entry();
        e.registry.fail_reads = true;
        assert!(e.is_enabled().is_err());
        assert_eq!(None, e.disabled_by_os());
    }

    #[test]
    fn leftovers_are_removed_while_the_run_value_is_already_gone() {
        // An older build turned autostart off and left its StartupApproved value.
        let mut e = entry();
        e.registry.binaries.insert(k(APPROVED, "CodexVoiceClipboard"), vec![2, 0, 0, 0]);
        e.registry.binaries.insert(k(APPROVED, "Other"), vec![2]);
        e.remove_leftovers().unwrap();
        assert_eq!(vec![k(APPROVED, "Other")], e.registry.binaries.keys().cloned().collect::<Vec<_>>());
    }

    #[test]
    fn os_switch_is_read_from_startup_approved() {
        let mut e = entry();
        assert_eq!(Some(false), e.disabled_by_os());
        e.registry.binaries.insert(k(APPROVED, "CodexVoiceClipboard"), vec![3, 0, 0, 0]);
        assert_eq!(Some(true), e.disabled_by_os());
        e.registry.binaries.insert(k(APPROVED, "CodexVoiceClipboard"), vec![2, 0, 0, 0]);
        assert_eq!(Some(false), e.disabled_by_os());
    }
}
