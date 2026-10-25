// Project: miner-menubar
// File: src/config.rs
// Description: Reads the settings from the environment and finds the .env file.
// Author: Jan Alexandr Kopřiva
// License: AGPL-3.0-or-later
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::tuya::Account;

pub struct Config {
    pub token: Option<String>,
    pub coin: String,
    pub base_url: String,
    pub update_interval: Duration,
    pub max_retries: u32,
    pub retry_delay: Duration,
    pub tuya: Account,
}

/// A whole number setting. Python's int() would crash at import on a typo;
/// this names the variable instead.
fn number(
    lookup: &impl Fn(&str) -> Option<String>,
    name: &str,
    default: i64,
) -> Result<i64, String> {
    match lookup(name) {
        None => Ok(default),
        Some(text) => text
            .trim()
            .parse()
            .map_err(|_| format!("{name} must be a whole number, got {text:?}")),
    }
}

impl Config {
    /// Settings from any source of variables, so tests need not touch the process environment.
    pub fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Result<Config, String> {
        let text = |name: &str, default: &str| lookup(name).unwrap_or_else(|| default.to_string());
        let update_interval = number(&lookup, "UPDATE_INTERVAL", 300)?;
        if update_interval < 1 {
            // Python slept zero seconds here and refreshed in a tight loop.
            return Err(format!(
                "UPDATE_INTERVAL must be at least 1, got {update_interval}"
            ));
        }
        Ok(Config {
            token: lookup("TOKEN"),
            coin: text("COIN", "btc"),
            base_url: text("BASE_URL", "https://pool.braiins.com"),
            update_interval: Duration::from_secs(update_interval as u64),
            // At least one attempt: 0 would skip the pool without a word.
            max_retries: number(&lookup, "MAX_RETRIES", 3)?.clamp(1, i64::from(u32::MAX)) as u32,
            retry_delay: Duration::from_secs(number(&lookup, "RETRY_DELAY", 5)?.max(0) as u64),
            tuya: Account {
                access_id: lookup("TUYA_ACCESS_ID"),
                secret: lookup("TUYA_ACCESS_SECRET"),
                region: text("TUYA_REGION", "eu"),
                device_id: lookup("TUYA_DEVICE_ID"),
            },
        })
    }

    pub fn from_env() -> Result<Config, String> {
        Config::from_lookup(|name| std::env::var(name).ok())
    }
}

/// Hands every variable of a `.env` file to `set`, skipping lines that do not
/// parse (python-dotenv does the same; dotenvy's `from_path` would stop at the
/// first one and drop everything after it). Returns one message per bad line
/// or an unreadable file.
pub fn load_dotenv(path: &Path, mut set: impl FnMut(&str, &str)) -> Vec<String> {
    let iter = match dotenvy::from_path_iter(path) {
        Ok(iter) => iter,
        Err(e) => return vec![format!("Could not read {}: {e}", path.display())],
    };
    let mut problems = Vec::new();
    for item in iter {
        match item {
            Ok((key, value)) => set(&key, &value),
            Err(e) => problems.push(format!("Skipped a line of {}: {e}", path.display())),
        }
    }
    problems
}

/// The nearest `.env` at or above `start`, like python-dotenv's search.
pub fn find_dotenv(start: &Path) -> Option<PathBuf> {
    start
        .ancestors()
        .map(|dir| dir.join(".env"))
        .find(|candidate| candidate.is_file())
}

/// Where the project lives: the folder holding `.env`, found from the binary's
/// folder first (as Python searched from the script) and then the working
/// directory. Falls back to the working directory. Logs go in `<this>/logs`.
pub fn locate() -> (PathBuf, Option<PathBuf>) {
    let from_exe = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().and_then(find_dotenv));
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let env_file = from_exe.or_else(|| find_dotenv(&cwd));
    let root = env_file
        .as_deref()
        .and_then(Path::parent)
        .map_or(cwd, Path::to_path_buf);
    (root, env_file)
}
