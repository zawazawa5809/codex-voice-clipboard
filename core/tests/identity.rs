//! File identity regression tests (spec 6.2: both OSes keep these).

use std::fs::{self, File};

use cvc_core::identity::file_identity;

#[test]
fn identity_is_stable_for_the_same_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a");
    fs::write(&path, "x").unwrap();
    let first = file_identity(&File::open(&path).unwrap()).unwrap();
    fs::write(&path, "rewritten in place").unwrap();
    assert_eq!(first, file_identity(&File::open(&path).unwrap()).unwrap());
}

#[test]
fn identity_changes_when_a_file_is_replaced_by_rename() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a");
    let other = dir.path().join("b");
    fs::write(&path, "x").unwrap();
    let before = File::open(&path).unwrap();
    fs::write(&other, "x").unwrap();
    fs::rename(&other, &path).unwrap();
    assert_ne!(file_identity(&before).unwrap(), file_identity(&File::open(&path).unwrap()).unwrap());
}

#[test]
fn identity_is_equal_through_a_hard_link() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a");
    let link = dir.path().join("link");
    fs::write(&path, "x").unwrap();
    fs::hard_link(&path, &link).unwrap();
    assert_eq!(
        file_identity(&File::open(&path).unwrap()).unwrap(),
        file_identity(&File::open(&link).unwrap()).unwrap()
    );
}
