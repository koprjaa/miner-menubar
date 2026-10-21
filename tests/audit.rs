// Project: miner-menubar
// File: tests/audit.rs
// Description: Tests for differences from the Python widget found in the port audit.
// Author: Jan Alexandr Kopřiva
// License: MIT

use std::collections::HashMap;
use std::sync::Mutex;

use miner_menubar::config::{load_dotenv, Config};
use miner_menubar::widget::run_exclusive;

#[test]
fn a_bad_dotenv_line_does_not_hide_the_variables_after_it() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join(".env");
    std::fs::write(&file, "A=1\nbad line here\nTOKEN=secret\n").unwrap();
    let mut loaded = HashMap::new();
    let problems = load_dotenv(&file, |k, v| {
        loaded.insert(k.to_string(), v.to_string());
    });
    assert_eq!(loaded.get("A").map(String::as_str), Some("1"));
    assert_eq!(loaded.get("TOKEN").map(String::as_str), Some("secret"));
    assert_eq!(problems.len(), 1, "{problems:?}");
}

#[test]
fn a_huge_retry_count_is_not_wrapped_to_zero() {
    let c = Config::from_lookup(|name| (name == "MAX_RETRIES").then(|| "4294967296".to_string()))
        .unwrap();
    assert_eq!(c.max_retries, u32::MAX);
}

#[test]
fn a_manual_refresh_is_skipped_while_one_is_running() {
    let gate = Mutex::new(());
    let held = gate.lock().unwrap();
    assert_eq!(run_exclusive(&gate, false, || 1), None);
    drop(held);
    assert_eq!(run_exclusive(&gate, false, || 1), Some(1));
    assert_eq!(run_exclusive(&gate, true, || 2), Some(2));
}
