//! The 22 core cases ported from the .NET `tests/CodexVoiceClipboard.Tests/Program.cs`.
//! No real clipboard, login item, installation, or Codex history is used.

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use cvc_core::copy::{Clipboard, ClipboardError, CopyCoordinator};
use cvc_core::log::SafeLog;
use cvc_core::startup::{Autostart, StartupController};
use cvc_core::uninstall::{self, UninstallOperations};
use cvc_core::{HistorySession, Transcript};

fn line(id: &str, text: &str) -> String {
    serde_json::json!({ "id": id, "text": text }).to_string() + "\n"
}
fn write(path: &Path, id: &str, text: &str) {
    fs::write(path, line(id, text)).unwrap();
}
fn append_raw(path: &Path, bytes: &[u8]) {
    OpenOptions::new().create(true).append(true).open(path).unwrap().write_all(bytes).unwrap();
}
fn append(path: &Path, id: &str, text: &str) {
    append_raw(path, line(id, text).as_bytes());
}
fn with_history(test: impl FnOnce(&Path, &mut HistorySession)) {
    let dir = tempfile::tempdir().unwrap();
    let path: PathBuf = dir.path().join("history.jsonl");
    let mut session = HistorySession::new(&path);
    test(&path, &mut session);
}
fn new_ids(session: &mut HistorySession) -> Vec<String> {
    session.poll().unwrap().new_records.into_iter().map(|r| r.id).collect()
}

#[test]
fn startup_history_is_available_manually_but_never_auto_copied() {
    with_history(|path, session| {
        write(path, "old", "古い履歴");
        assert!(new_ids(session).is_empty());
        assert_eq!("old", session.recent()[0].id);
        append(path, "new", "新しい文字起こし");
        assert_eq!(vec!["new"], new_ids(session));
    });
}

#[test]
fn same_id_is_processed_only_once_including_after_file_replacement() {
    with_history(|path, session| {
        write(path, "old", "old");
        session.poll().unwrap();
        append(path, "n", "日本語");
        assert_eq!(1, new_ids(session).len());
        append(path, "n", "duplicate");
        assert!(new_ids(session).is_empty());
        fs::remove_file(path).unwrap();
        write(path, "n", "duplicate");
        assert!(new_ids(session).is_empty());
    });
}

#[test]
fn partial_json_and_split_utf8_are_retried() {
    with_history(|path, session| {
        fs::write(path, "").unwrap();
        session.poll().unwrap();
        let bytes = serde_json::json!({ "id": "split", "text": "日本語🙂" }).to_string().into_bytes();
        // Cut right after the first byte of "日" (0xE6), inside a multi-byte character.
        let cut = bytes.iter().position(|&b| b == 0xe6).unwrap() + 1;
        assert!(std::str::from_utf8(&bytes[..cut]).is_err(), "cut must split a code point");
        fs::write(path, &bytes[..cut]).unwrap();
        assert!(new_ids(session).is_empty());
        append_raw(path, &bytes[cut..]);
        let batch = session.poll().unwrap();
        assert_eq!(1, batch.new_records.len());
        assert_eq!("日本語🙂", batch.new_records[0].text);
        assert!(new_ids(session).is_empty());
    });
}

#[test]
fn malformed_complete_lines_do_not_block_subsequent_records() {
    with_history(|path, session| {
        fs::write(path, "").unwrap();
        session.poll().unwrap();
        append_raw(path, b"invalid\n{\"id\":\"missing-text\"}\n");
        append(path, "ok", "recover");
        let batch = session.poll().unwrap();
        assert_eq!(1, batch.new_records.len());
        assert!(batch.invalid_lines >= 2);
    });
}

#[test]
fn japanese_long_multiline_and_unicode_remain_exact() {
    with_history(|path, session| {
        fs::write(path, "").unwrap();
        session.poll().unwrap();
        let text = "日本語𠮷👩🏽‍💻\r\n二行目\n".repeat(30000);
        append(path, "long", &text);
        let batch = session.poll().unwrap();
        assert_eq!(1, batch.new_records.len());
        assert!(batch.new_records[0].text == text);
    });
}

#[test]
fn missing_at_startup_then_creation_emits_new_records() {
    with_history(|path, session| {
        assert_eq!(io::ErrorKind::NotFound, session.poll().unwrap_err().kind());
        write(path, "created", "created");
        assert_eq!(1, new_ids(session).len());
    });
}

#[test]
fn delete_and_recreate_including_equal_length_recovers() {
    with_history(|path, session| {
        write(path, "one", "one");
        session.poll().unwrap();
        fs::remove_file(path).unwrap();
        assert_eq!(io::ErrorKind::NotFound, session.poll().unwrap_err().kind());
        write(path, "two", "two");
        assert_eq!(vec!["two"], new_ids(session));
    });
}

#[test]
fn truncation_is_recovered() {
    with_history(|path, session| {
        write(path, "long-initial", &"x".repeat(100));
        session.poll().unwrap();
        write(path, "tiny", "x");
        assert_eq!(vec!["tiny"], new_ids(session));
    });
}

#[test]
fn same_length_in_place_rewrite_is_recovered() {
    with_history(|path, session| {
        write(path, "one", "one");
        session.poll().unwrap();
        write(path, "two", "two");
        assert_eq!(vec!["two"], new_ids(session));
    });
}

/// Windows holds a share-none lock; macOS has no mandatory lock, so the closest
/// unreadable-but-present state is a permission denial. Either way the first
/// successful read must be treated as pre-launch history.
#[test]
fn initial_lock_cannot_replay_old_history_after_unlock() {
    with_history(|path, session| {
        write(path, "old", "old");
        {
            let _lock = Locked::new(path);
            let err = session.poll().unwrap_err();
            assert_ne!(io::ErrorKind::NotFound, err.kind());
        }
        assert!(new_ids(session).is_empty());
        append(path, "fresh", "fresh");
        assert_eq!(1, new_ids(session).len());
    });
}

#[cfg(unix)]
struct Locked(PathBuf);
#[cfg(unix)]
impl Locked {
    fn new(path: &Path) -> Self {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o000)).unwrap();
        Locked(path.to_path_buf())
    }
}
#[cfg(unix)]
impl Drop for Locked {
    fn drop(&mut self) {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&self.0, fs::Permissions::from_mode(0o644)).unwrap();
    }
}
#[cfg(windows)]
struct Locked(#[allow(dead_code)] fs::File);
#[cfg(windows)]
impl Locked {
    fn new(path: &Path) -> Self {
        use std::os::windows::fs::OpenOptionsExt;
        Locked(OpenOptions::new().read(true).write(true).share_mode(0).open(path).unwrap())
    }
}

#[test]
fn recent_history_is_capped_to_ten_newest_first() {
    with_history(|path, session| {
        fs::write(path, "").unwrap();
        session.poll().unwrap();
        for i in 0..15 {
            append(path, &i.to_string(), "text");
        }
        assert_eq!(15, new_ids(session).len());
        assert_eq!(10, session.recent().len());
        assert_eq!("14", session.recent()[0].id);
    });
}

#[test]
fn reader_never_changes_source_bytes() {
    with_history(|path, session| {
        write(path, "a", "source");
        let before = fs::read(path).unwrap();
        session.poll().unwrap();
        session.poll().unwrap();
        assert_eq!(before, fs::read(path).unwrap());
    });
}

#[derive(Default)]
struct FakeClipboard {
    failures: i32,
    attempts: i32,
    text: Option<String>,
}
impl Clipboard for FakeClipboard {
    fn set_text(&mut self, text: &str) -> Result<(), ClipboardError> {
        self.attempts += 1;
        self.failures -= 1;
        if self.failures >= 0 {
            return Err(ClipboardError);
        }
        self.text = Some(text.to_string());
        Ok(())
    }
}
fn t(id: &str, text: &str) -> Transcript {
    Transcript::new(id, text)
}
fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}
fn secs(n: u64) -> Duration {
    Duration::from_secs(n)
}

#[test]
fn copy_waits_two_seconds_and_retries_contention() {
    let mut engine = CopyCoordinator::new(FakeClipboard { failures: 2, ..Default::default() });
    let now = Instant::now();
    engine.observe(&[t("a", "text")], now);
    engine.tick(now + ms(1999));
    assert_eq!(0, engine.clipboard().attempts);
    assert_eq!(Some("clipboard-retry"), engine.tick(now + secs(2)));
    assert_eq!(1, engine.clipboard().attempts);
    engine.tick(now + secs(3));
    assert_eq!(Some("clipboard-copied"), engine.tick(now + secs(4)));
    assert_eq!(Some("text"), engine.clipboard().text.as_deref());
    assert!(!engine.has_error());
}

#[test]
fn pause_cancels_pending_copy_manual_recovery_still_works_resume_does_not_replay() {
    let mut engine = CopyCoordinator::new(FakeClipboard::default());
    let now = Instant::now();
    engine.observe(&[t("a", "first")], now);
    engine.set_enabled(false);
    engine.tick(now + secs(10));
    assert_eq!(0, engine.clipboard().attempts);
    engine.copy_manually(t("a", "first"), now);
    engine.tick(now);
    assert_eq!(Some("first"), engine.clipboard().text.as_deref());
    engine.observe(&[t("b", "paused")], now);
    engine.set_enabled(true);
    engine.tick(now + secs(20));
    assert_eq!(1, engine.clipboard().attempts);
    engine.observe(&[t("c", "after resume")], now);
    engine.tick(now + secs(2));
    assert_eq!(Some("after resume"), engine.clipboard().text.as_deref());
}

#[test]
fn newest_pending_record_replaces_older_pending_record() {
    let mut engine = CopyCoordinator::new(FakeClipboard::default());
    let now = Instant::now();
    engine.observe(&[t("a", "a"), t("b", "b")], now);
    engine.tick(now + secs(2));
    assert_eq!(Some("b"), engine.clipboard().text.as_deref());
    assert_eq!(1, engine.clipboard().attempts);
}

#[test]
fn delay_is_configurable() {
    let mut engine = CopyCoordinator::new(FakeClipboard::default());
    engine.set_delay(secs(5));
    let now = Instant::now();
    engine.observe(&[t("a", "a")], now);
    engine.tick(now + secs(2));
    assert_eq!(0, engine.clipboard().attempts);
    engine.tick(now + secs(5));
    assert_eq!(Some("a"), engine.clipboard().text.as_deref());
}

#[test]
fn clipboard_retries_are_bounded_and_manual_retry_recovers() {
    let mut engine = CopyCoordinator::new(FakeClipboard { failures: 100, ..Default::default() });
    let now = Instant::now();
    engine.copy_manually(t("a", "a"), now);
    let mut last = None;
    for i in 0..20 {
        last = engine.tick(now + secs(i)).or(last);
    }
    assert_eq!(8, engine.clipboard().attempts);
    assert!(engine.has_error());
    assert_eq!(Some("clipboard-failed"), last);
    engine.clipboard_mut().failures = 0;
    engine.copy_manually(t("a", "a"), now + secs(30));
    engine.tick(now + secs(30));
    assert_eq!(Some("a"), engine.clipboard().text.as_deref());
    assert!(!engine.has_error());
}

#[test]
fn manual_copy_cancels_delayed_automatic_overwrite() {
    let mut engine = CopyCoordinator::new(FakeClipboard::default());
    let now = Instant::now();
    engine.observe(&[t("auto", "auto")], now);
    engine.copy_manually(t("manual", "manual"), now);
    engine.tick(now);
    engine.tick(now + secs(10));
    assert_eq!(Some("manual"), engine.clipboard().text.as_deref());
    assert_eq!(1, engine.clipboard().attempts);
}

#[test]
fn logs_contain_event_codes_only_never_supplied_transcript() {
    let dir = tempfile::tempdir().unwrap();
    let log = SafeLog::new(dir.path().join("app.log"));
    log.write("clipboard-copied").unwrap();
    let text = fs::read_to_string(log.path()).unwrap();
    assert!(text.contains("clipboard-copied"));
    assert!(!text.contains("日本語"));
    assert!(log.write("secret transcript 日本語").is_err());
    assert!(!fs::read_to_string(log.path()).unwrap().contains("日本語"));
}

#[derive(Default)]
struct FakeStartup {
    enabled: bool,
    writes: i32,
}
impl Autostart for FakeStartup {
    fn is_enabled(&self) -> io::Result<bool> {
        Ok(self.enabled)
    }
    fn set_enabled(&mut self, enabled: bool) -> io::Result<()> {
        self.enabled = enabled;
        self.writes += 1;
        Ok(())
    }
    fn disabled_by_os(&self) -> Option<bool> {
        Some(false)
    }
}

/// .NET wrote a quoted command into an injected store. The Tauri plugin owns the
/// command line, so the port checks that only the injected store is touched and that
/// a switch already in the wanted state is not rewritten (Windows `disable` errors
/// when the value is absent).
#[test]
fn startup_toggles_use_injected_store_only() {
    let mut startup = StartupController::new(FakeStartup::default());
    assert!(startup.set_enabled(true).unwrap());
    assert!(startup.enabled().unwrap());
    assert!(!startup.set_enabled(false).unwrap());
    assert!(!startup.enabled().unwrap());
    assert_eq!(2, startup.store().writes);
    startup.set_enabled(false).unwrap();
    assert_eq!(2, startup.store().writes);
}

#[derive(Default)]
struct FakeUninstall {
    calls: Vec<&'static str>,
    stop_fails: bool,
}
impl UninstallOperations for FakeUninstall {
    fn stop_resident(&mut self) -> bool {
        self.calls.push("stop");
        !self.stop_fails
    }
    fn disable_startup(&mut self) -> io::Result<()> {
        self.calls.push("disable");
        Ok(())
    }
    fn remove_data(&mut self) -> io::Result<()> {
        self.calls.push("remove-data");
        Ok(())
    }
    fn delete_application(&mut self) -> io::Result<()> {
        self.calls.push("delete");
        Ok(())
    }
}

#[test]
fn uninstall_stops_process_before_removing_startup_data_and_files() {
    let mut ops = FakeUninstall::default();
    uninstall::execute(&mut ops).unwrap();
    assert_eq!(vec!["stop", "disable", "remove-data", "delete"], ops.calls);
}

#[test]
fn uninstall_refuses_deletion_when_resident_fails_to_exit() {
    let mut ops = FakeUninstall { stop_fails: true, ..Default::default() };
    assert!(uninstall::execute(&mut ops).is_err());
    assert_eq!(vec!["stop"], ops.calls);
}
