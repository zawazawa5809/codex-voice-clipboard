//! Resident behaviour that the .NET tray class carried, now platform-independent.

use std::cell::RefCell;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

use cvc_core::copy::{Clipboard, ClipboardError};
use cvc_core::engine::{Engine, State};
use cvc_core::log::SafeLog;
use cvc_core::settings::Settings;

#[derive(Clone, Default)]
struct Shared(Rc<RefCell<Vec<String>>>);
impl Clipboard for Shared {
    fn set_text(&mut self, text: &str) -> Result<(), ClipboardError> {
        self.0.borrow_mut().push(text.to_string());
        Ok(())
    }
}
impl Shared {
    fn last(&self) -> Option<String> {
        self.0.borrow().last().cloned()
    }
    fn count(&self) -> usize {
        self.0.borrow().len()
    }
}

struct Fixture {
    _dir: tempfile::TempDir,
    history: PathBuf,
    log: PathBuf,
    clip: Shared,
    engine: Engine<Shared>,
    now: Instant,
}
fn fixture(settings: Settings) -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let history = dir.path().join("history.jsonl");
    let log = dir.path().join("app.log");
    fs::write(&history, "").unwrap();
    let clip = Shared::default();
    let engine = Engine::new(&history, clip.clone(), SafeLog::new(&log), &settings);
    Fixture { history, log, clip, engine, now: Instant::now(), _dir: dir }
}
fn append(path: &Path, id: &str, text: &str) {
    let line = serde_json::json!({ "id": id, "text": text }).to_string() + "\n";
    OpenOptions::new().create(true).append(true).open(path).unwrap().write_all(line.as_bytes()).unwrap();
}
fn log_text(f: &Fixture) -> String {
    fs::read_to_string(&f.log).unwrap_or_default()
}

#[test]
fn new_record_is_copied_after_the_delay() {
    let mut f = fixture(Settings::default());
    f.engine.poll_history(f.now);
    append(&f.history, "a", "本文");
    f.engine.poll_history(f.now);
    f.engine.tick(f.now + Duration::from_millis(1900));
    assert_eq!(0, f.clip.count());
    f.engine.tick(f.now + Duration::from_secs(2));
    assert_eq!(Some("本文".to_string()), f.clip.last());
}

#[test]
fn resume_drains_records_written_while_paused() {
    let mut f = fixture(Settings::default());
    f.engine.poll_history(f.now);
    f.engine.set_auto(false, f.now);
    append(&f.history, "paused", "停止中");
    // No poll happened while paused: resuming must still not copy it.
    f.engine.set_auto(true, f.now);
    f.engine.poll_history(f.now);
    f.engine.tick(f.now + Duration::from_secs(10));
    assert_eq!(0, f.clip.count());
    assert_eq!("paused", f.engine.recent()[0].id);
    append(&f.history, "after", "再開後");
    f.engine.poll_history(f.now + Duration::from_secs(10));
    f.engine.tick(f.now + Duration::from_secs(12));
    assert_eq!(Some("再開後".to_string()), f.clip.last());
}

#[test]
fn manual_copy_of_latest_and_of_recent_index_is_immediate() {
    let mut f = fixture(Settings::default());
    append(&f.history, "a", "one");
    append(&f.history, "b", "two");
    f.engine.poll_history(f.now);
    f.engine.copy_recent(0, f.now);
    assert_eq!(Some("two".to_string()), f.clip.last());
    f.engine.copy_recent(1, f.now);
    assert_eq!(Some("one".to_string()), f.clip.last());
    f.engine.copy_recent(5, f.now);
    assert_eq!(2, f.clip.count());
}

#[test]
fn delay_change_cancels_pending_and_is_clamped() {
    let mut f = fixture(Settings::default());
    f.engine.poll_history(f.now);
    append(&f.history, "a", "x");
    f.engine.poll_history(f.now);
    f.engine.set_delay_seconds(99.0);
    assert_eq!(Duration::from_secs(60), f.engine.delay());
    f.engine.tick(f.now + Duration::from_secs(120));
    assert_eq!(0, f.clip.count());
    f.engine.set_delay_seconds(1.26);
    assert_eq!(Duration::from_millis(1300), f.engine.delay());
}

#[test]
fn state_precedence_is_error_then_paused_then_active() {
    let mut f = fixture(Settings::default());
    assert_eq!(State::Error, f.engine.status().state, "unread history starts as error");
    f.engine.poll_history(f.now);
    assert_eq!(State::Active, f.engine.status().state);
    f.engine.set_auto(false, f.now);
    assert_eq!(State::Paused, f.engine.status().state);
    f.engine.set_watcher_healthy(false);
    assert_eq!(State::Error, f.engine.status().state);
    f.engine.set_watcher_healthy(true);
    fs::remove_file(&f.history).unwrap();
    f.engine.poll_history(f.now);
    assert_eq!(State::Error, f.engine.status().state);
    assert!(f.engine.status().history_error);
}

#[test]
fn unavailable_history_is_logged_once_and_states_are_logged_on_change() {
    let mut f = fixture(Settings::default());
    f.engine.poll_history(f.now);
    fs::remove_file(&f.history).unwrap();
    f.engine.poll_history(f.now);
    f.engine.poll_history(f.now);
    let log = log_text(&f);
    assert_eq!(1, log.matches("history-unavailable").count());
    assert_eq!(1, log.matches("state-active").count());
    assert_eq!(1, log.matches("state-error").count());
}

#[test]
fn invalid_records_are_logged_by_code_without_content() {
    let mut f = fixture(Settings::default());
    f.engine.poll_history(f.now);
    OpenOptions::new().append(true).open(&f.history).unwrap().write_all("秘密の本文\n".as_bytes()).unwrap();
    f.engine.poll_history(f.now);
    let log = log_text(&f);
    assert!(log.contains("history-invalid-record"));
    assert!(!log.contains("秘密"));
}

#[test]
fn settings_carry_auto_copy_and_delay() {
    let f = fixture(Settings { auto_copy: false, delay_seconds: 5.0, ..Settings::default() });
    assert!(!f.engine.status().auto_enabled);
    assert_eq!(Duration::from_secs(5), f.engine.delay());
    let saved = f.engine.settings(&Settings::default());
    assert!(!saved.auto_copy);
    assert_eq!(5.0, saved.delay_seconds);
}

#[test]
fn settings_load_falls_back_and_clamps() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    assert_eq!(Settings::default(), Settings::load(&path));
    fs::write(&path, "not json").unwrap();
    assert_eq!(Settings::default(), Settings::load(&path));
    fs::write(&path, r#"{"autoCopy":false,"delaySeconds":1000}"#).unwrap();
    let loaded = Settings::load(&path);
    assert!(!loaded.auto_copy);
    assert_eq!(60.0, loaded.delay_seconds);
    let round = Settings { risk_notice_seen: true, ..loaded };
    round.save(&path).unwrap();
    assert_eq!(round, Settings::load(&path));
    assert!(!fs::read_to_string(&path).unwrap().contains("text"));
}

#[test]
fn enabling_auto_copy_when_already_on_keeps_the_pending_copy() {
    let mut f = fixture(Settings::default());
    f.engine.poll_history(f.now);
    append(&f.history, "a", "保留中");
    f.engine.poll_history(f.now);
    f.engine.set_auto(true, f.now);
    f.engine.tick(f.now + Duration::from_secs(2));
    assert_eq!(Some("保留中".to_string()), f.clip.last());
}

#[test]
fn nothing_is_logged_as_an_error_before_the_first_history_poll() {
    let mut f = fixture(Settings::default());
    f.engine.tick(f.now);
    f.engine.set_watcher_healthy(true);
    f.engine.set_delay_seconds(1.0);
    assert!(!log_text(&f).contains("state-"));
    f.engine.poll_history(f.now);
    let log = log_text(&f);
    assert_eq!(0, log.matches("state-error").count());
    assert_eq!(1, log.matches("state-active").count());
}

#[test]
fn a_failed_first_poll_still_logs_the_error_state_once() {
    let mut f = fixture(Settings::default());
    fs::remove_file(&f.history).unwrap();
    f.engine.tick(f.now);
    f.engine.poll_history(f.now);
    f.engine.tick(f.now);
    assert_eq!(1, log_text(&f).matches("state-error").count());
}
