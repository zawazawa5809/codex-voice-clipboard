//! Reader behaviours the .NET implementation had but its tests did not name.

use std::fs::{self, OpenOptions};
use std::io::Write;

use cvc_core::HistorySession;

fn session_on_empty() -> (tempfile::TempDir, std::path::PathBuf, HistorySession) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("history.jsonl");
    fs::write(&path, "").unwrap();
    let mut session = HistorySession::new(&path);
    session.poll().unwrap();
    (dir, path, session)
}
fn append(path: &std::path::Path, bytes: &[u8]) {
    OpenOptions::new().append(true).open(path).unwrap().write_all(bytes).unwrap();
}

#[test]
fn whitespace_only_id_or_text_is_invalid() {
    let (_d, path, mut session) = session_on_empty();
    append(&path, b"{\"id\":\"  \",\"text\":\"x\"}\n{\"id\":\"a\",\"text\":\" \\n\"}\n{\"id\":5,\"text\":\"x\"}\n");
    let batch = session.poll().unwrap();
    assert!(batch.new_records.is_empty());
    assert_eq!(3, batch.invalid_lines);
}

#[test]
fn extra_fields_are_ignored() {
    let (_d, path, mut session) = session_on_empty();
    append(&path, b"{\"createdAtMs\":1,\"id\":\"a\",\"text\":\"x\"}\n");
    assert_eq!(1, session.poll().unwrap().new_records.len());
}

#[test]
fn byte_order_mark_is_ignored() {
    let (_d, path, mut session) = session_on_empty();
    append(&path, b"\xef\xbb\xbf{\"id\":\"bom\",\"text\":\"x\"}\n");
    assert_eq!("bom", session.poll().unwrap().new_records[0].id);
}

#[test]
fn complete_final_object_without_newline_is_accepted_once() {
    let (_d, path, mut session) = session_on_empty();
    append(&path, b"{\"id\":\"a\",\"text\":\"x\"}");
    assert_eq!(1, session.poll().unwrap().new_records.len());
    append(&path, b"\n{\"id\":\"b\",\"text\":\"y\"}\n");
    let batch = session.poll().unwrap();
    assert_eq!(vec!["b"], batch.new_records.iter().map(|r| r.id.as_str()).collect::<Vec<_>>());
    assert_eq!(0, batch.invalid_lines);
}

#[test]
fn invalid_final_line_without_newline_counts_only_once_newline_arrives() {
    let (_d, path, mut session) = session_on_empty();
    append(&path, b"{\"id\":\"a\"}");
    assert_eq!(0, session.poll().unwrap().invalid_lines);
    append(&path, b"\n");
    assert_eq!(1, session.poll().unwrap().invalid_lines);
}

#[test]
fn oversized_line_is_discarded_and_following_records_survive() {
    let (_d, path, mut session) = session_on_empty();
    let mut big = vec![b'x'; 16 * 1024 * 1024 + 10];
    big.push(b'\n');
    append(&path, &big);
    append(&path, b"{\"id\":\"after\",\"text\":\"x\"}\n");
    let batch = session.poll().unwrap();
    assert_eq!(1, batch.invalid_lines);
    assert_eq!("after", batch.new_records[0].id);
}
