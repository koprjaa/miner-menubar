// Project: miner-widget-stats
// File: src/widget.rs
// Description: Reads the pool and the Tuya sensor and decides what the menu bar title says.
// Author: Jan Alexandr Kopřiva
// License: MIT

use std::net::{SocketAddr, TcpStream};
use std::path::Path;
use std::process::Command;
use std::sync::Mutex;
use std::time::Duration;

use reqwest::blocking::Client;
use serde_json::Value;

use crate::config::Config;
use crate::hashrate::{format_hashrate, format_temperature, format_title};
use crate::logger;
use crate::tuya::{read_temperature, truthy};

/// What the title says before the first refresh finishes.
pub const INITIAL_TITLE: &str = "--";
/// What the title says when the machine has no network.
pub const OFFLINE_TITLE: &str = "Offline";

const POOL_TIMEOUT: Duration = Duration::from_secs(10);

/// Runs `job` with the gate held, so two refreshes never overlap and a slow one
/// cannot be overtaken by a newer title. With `wait` false a busy gate means
/// `None` and the job is skipped; the Python version got this by running a
/// manual refresh on the UI thread.
pub fn run_exclusive<T>(gate: &Mutex<()>, wait: bool, job: impl FnOnce() -> T) -> Option<T> {
    let _held = if wait {
        gate.lock().unwrap_or_else(|e| e.into_inner())
    } else {
        match gate.try_lock() {
            Ok(held) => held,
            Err(std::sync::TryLockError::Poisoned(e)) => e.into_inner(),
            Err(std::sync::TryLockError::WouldBlock) => return None,
        }
    };
    Some(job())
}

/// True when 8.8.8.8 answers on the DNS port.
///
/// Checked before every refresh so an offline machine shows "Offline" instead of
/// waiting out two HTTP timeouts. The socket is closed again; the Python
/// version left it to the garbage collector.
pub fn check_internet(addr: SocketAddr, timeout: Duration) -> bool {
    TcpStream::connect_timeout(&addr, timeout).is_ok()
}

fn google_dns() -> SocketAddr {
    SocketAddr::from(([8, 8, 8, 8], 53))
}

/// The temperature text for a Tuya status, `--` when it could not be read.
pub fn temperature_text(status: &Value) -> String {
    match read_temperature(status) {
        Ok(celsius) => format_temperature(celsius),
        Err(_) => "--".into(),
    }
}

/// The title for a pool answer and a temperature text. A failed pool read
/// arrives as `Value::Null` for both.
pub fn compose_title(profile: &Value, workers: &Value, coin: &str, temp: &str) -> String {
    if !(truthy(profile) && truthy(workers)) {
        logger::warning("Could not read data from pool.braiins.com");
        return format_title("--", temp);
    }
    let reading = profile.get(coin).and_then(|data| {
        let rate = data.get("hash_rate_5m")?;
        let unit = data.get("hash_rate_unit")?.as_str()?;
        Some(format_hashrate(rate, unit))
    });
    match reading {
        Some(rate) => format_title(&rate, temp),
        None => {
            logger::error("Could not update the title");
            format_title("Error", temp)
        }
    }
}

fn get_json(client: &Client, url: &str, token: &str) -> Result<Value, Box<dyn std::error::Error>> {
    let body = client
        .get(url)
        .header("Pool-Auth-Token", token)
        .timeout(POOL_TIMEOUT)
        .send()?
        .error_for_status()?
        .text()?;
    Ok(serde_json::from_str(&body)?)
}

/// The profile and the workers, retrying on failure. `Null, Null` when the
/// token is missing or every attempt fails.
pub fn fetch_data(client: &Client, config: &Config) -> (Value, Value) {
    let Some(token) = config.token.as_deref().filter(|t| !t.is_empty()) else {
        logger::warning("Pool token is not set, skipping pool.braiins.com");
        return (Value::Null, Value::Null);
    };
    let (base, coin) = (&config.base_url, &config.coin);
    for attempt in 1..=config.max_retries {
        logger::info(&format!("Attempt {attempt}/{}", config.max_retries));
        let both = get_json(
            client,
            &format!("{base}/accounts/profile/json/{coin}/"),
            token,
        )
        .and_then(|profile| {
            get_json(
                client,
                &format!("{base}/accounts/workers/json/{coin}/"),
                token,
            )
            .map(|workers| (profile, workers))
        });
        match both {
            Ok(pair) => {
                logger::info("Data read");
                return pair;
            }
            Err(e) => {
                logger::error(&format!(
                    "Could not read data (attempt {attempt}/{}): {}",
                    config.max_retries,
                    logger::chain(e.as_ref())
                ));
                if attempt < config.max_retries {
                    logger::info(&format!(
                        "Waiting {}s before the next attempt",
                        config.retry_delay.as_secs()
                    ));
                    std::thread::sleep(config.retry_delay);
                }
            }
        }
    }
    (Value::Null, Value::Null)
}

fn read_tuya_temperature(client: &Client, config: &Config) -> String {
    logger::info("Reading temperature from the Tuya API");
    let status = config
        .tuya
        .get_access_token(client)
        .and_then(|token| config.tuya.get_device_status(client, &token))
        .and_then(|status| Ok(read_temperature(&status).map(format_temperature)?));
    match status {
        Ok(temp) => {
            logger::info(&format!("Temperature: {temp}°C"));
            temp
        }
        Err(e) => {
            logger::error(&format!(
                "Could not read the temperature: {}",
                logger::chain(e.as_ref())
            ));
            "--".into()
        }
    }
}

/// One refresh: the pool hash rate and the Tuya temperature, as a title.
pub fn update_once(client: &Client, config: &Config) -> String {
    if !check_internet(google_dns(), Duration::from_secs(3)) {
        logger::warning("No internet connection");
        return OFFLINE_TITLE.into();
    }
    logger::info("Fetching data from pool.braiins.com");
    let (profile, workers) = fetch_data(client, config);
    // The temperature is read even when the pool call failed: the two are
    // independent, and a temperature alone is still worth showing.
    let temp = read_tuya_temperature(client, config);
    let title = compose_title(&profile, &workers, &config.coin, &temp);
    logger::info(&format!("Setting title to: {title}"));
    title
}

/// Open the log file in the default editor.
pub fn open_log(path: &Path) {
    if path.exists() {
        logger::info(&format!("Opening log file: {}", path.display()));
        let _ = Command::new("open").arg(path).status();
    } else {
        logger::error(&format!("Log file does not exist: {}", path.display()));
    }
}
