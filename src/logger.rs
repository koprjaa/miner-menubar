// Project: miner-widget-stats
// File: src/logger.rs
// Description: Rotating log file plus console output in the format the Python widget used.
// Author: Jan Alexandr Kopřiva
// License: MIT

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

const MAX_BYTES: u64 = 1024 * 1024;
const BACKUPS: u32 = 5;

pub struct Logger {
    path: PathBuf,
    max_bytes: u64,
    file: Mutex<Option<File>>,
}

static GLOBAL: OnceLock<Logger> = OnceLock::new();

impl Logger {
    pub fn new(path: PathBuf, max_bytes: u64) -> Logger {
        if let Some(dir) = path.parent() {
            let _ = fs::create_dir_all(dir);
        }
        Logger {
            file: Mutex::new(
                OpenOptions::new()
                    .append(true)
                    .create(true)
                    .open(&path)
                    .ok(),
            ),
            path,
            max_bytes,
        }
    }

    /// `miner.log` becomes `miner.log.1`, that one `.2`, and the oldest is dropped.
    fn rotate(&self, slot: &mut Option<File>) {
        *slot = None;
        let numbered = |n: u32| PathBuf::from(format!("{}.{n}", self.path.display()));
        let _ = fs::remove_file(numbered(BACKUPS));
        for n in (1..BACKUPS).rev() {
            let _ = fs::rename(numbered(n), numbered(n + 1));
        }
        let _ = fs::rename(&self.path, numbered(1));
        *slot = OpenOptions::new()
            .append(true)
            .create(true)
            .open(&self.path)
            .ok();
    }

    pub fn log(&self, level: &str, message: &str) {
        let stamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S,%3f");
        let line = format!("{stamp} - {level} - {message}\n");
        eprint!("{line}");
        let mut slot = self.file.lock().unwrap_or_else(|e| e.into_inner());
        let size = fs::metadata(&self.path).map_or(0, |m| m.len());
        if size + line.len() as u64 > self.max_bytes && size > 0 {
            self.rotate(&mut slot);
        }
        if let Some(file) = slot.as_mut() {
            let _ = file.write_all(line.as_bytes());
        }
    }
}

pub fn init(logs_dir: &Path) {
    let _ = GLOBAL.set(Logger::new(logs_dir.join("miner.log"), MAX_BYTES));
}

fn emit(level: &str, message: &str) {
    match GLOBAL.get() {
        Some(logger) => logger.log(level, message),
        None => eprintln!("{level} - {message}"),
    }
}

pub fn info(message: &str) {
    emit("INFO", message);
}

pub fn warning(message: &str) {
    emit("WARNING", message);
}

pub fn error(message: &str) {
    emit("ERROR", message);
}

/// An error with the causes under it, one line.
pub fn chain(error: &dyn std::error::Error) -> String {
    let mut text = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        text.push_str(&format!(": {cause}"));
        source = cause.source();
    }
    text
}
