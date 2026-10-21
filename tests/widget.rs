// Project: miner-widget-stats
// File: tests/widget.rs
// Description: Tests for Tuya signing, value formatting, configuration, logging and pool reads.
// Author: Jan Alexandr Kopřiva
// License: MIT

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener};
use std::time::Duration;

use hmac::{Hmac, KeyInit, Mac};
use miner_widget_stats::config::{find_dotenv, Config};
use miner_widget_stats::hashrate::*;
use miner_widget_stats::logger::Logger;
use miner_widget_stats::tuya::*;
use miner_widget_stats::widget::{check_internet, compose_title, fetch_data};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

const ID: &str = "test-id";
const SECRET: &str = "test-secret";
const PATH: &str = "/v1.0/token?grant_type=1";

// --- signature ---------------------------------------------------------------

#[test]
fn the_signature_matches_the_documented_string_to_sign() {
    let (ts, nonce) = ("1700000000000", "abc123");
    let empty: String = Sha256::digest(b"")
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let base = format!("{ID}{ts}{nonce}GET\n{empty}\n\n{PATH}");
    let mut mac = Hmac::<Sha256>::new_from_slice(SECRET.as_bytes()).unwrap();
    mac.update(base.as_bytes());
    let expected: String = mac
        .finalize()
        .into_bytes()
        .iter()
        .map(|b| format!("{b:02X}"))
        .collect();
    assert_eq!(signature(ID, SECRET, PATH, ts, nonce, ""), expected);
}

#[test]
fn a_token_goes_into_the_signature() {
    let without = signature(ID, SECRET, PATH, "1700000000000", "abc123", "");
    let with = signature(ID, SECRET, PATH, "1700000000000", "abc123", "tok");
    assert_ne!(without, with);
}

#[test]
fn the_signature_is_uppercase_hex() {
    let sig = signature(ID, SECRET, PATH, "1", "n", "");
    assert_eq!(sig, sig.to_uppercase());
    assert_eq!(sig.len(), 64);
}

#[test]
fn every_signed_part_changes_the_signature() {
    let base = signature(ID, SECRET, PATH, "1700000000000", "abc123", "");
    assert_ne!(
        base,
        signature(
            ID,
            SECRET,
            "/v1.0/devices/x/status",
            "1700000000000",
            "abc123",
            ""
        )
    );
    assert_ne!(
        base,
        signature(ID, SECRET, PATH, "1700000000001", "abc123", "")
    );
    assert_ne!(
        base,
        signature(ID, SECRET, PATH, "1700000000000", "different", "")
    );
    assert_ne!(
        base,
        signature(ID, "other", PATH, "1700000000000", "abc123", "")
    );
}

// --- make_headers ------------------------------------------------------------

fn header<'a>(headers: &'a [(&str, String)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(k, _)| *k == name)
        .map(|(_, v)| v.as_str())
}

#[test]
fn the_headers_carry_everything_the_api_needs() {
    let headers = make_headers(ID, SECRET, PATH, "");
    assert_eq!(header(&headers, "client_id"), Some(ID));
    assert_eq!(header(&headers, "sign_method"), Some("HMAC-SHA256"));
    assert!(header(&headers, "t")
        .unwrap()
        .bytes()
        .all(|b| b.is_ascii_digit()));
    assert_eq!(header(&headers, "nonce").unwrap().len(), 32);
    assert_eq!(header(&headers, "sign").unwrap().len(), 64);
}

#[test]
fn the_access_token_header_appears_only_with_a_token() {
    assert_eq!(
        header(&make_headers(ID, SECRET, PATH, ""), "access_token"),
        None
    );
    assert_eq!(
        header(&make_headers(ID, SECRET, PATH, "tok"), "access_token"),
        Some("tok")
    );
}

#[test]
fn two_requests_get_different_nonces() {
    let first = make_headers(ID, SECRET, PATH, "");
    let second = make_headers(ID, SECRET, PATH, "");
    assert_ne!(header(&first, "nonce"), header(&second, "nonce"));
}

#[test]
fn the_headers_are_signed_with_their_own_timestamp_and_nonce() {
    let h = make_headers(ID, SECRET, PATH, "tok");
    let expected = signature(
        ID,
        SECRET,
        PATH,
        header(&h, "t").unwrap(),
        header(&h, "nonce").unwrap(),
        "tok",
    );
    assert_eq!(header(&h, "sign"), Some(expected.as_str()));
}

// --- base_url ----------------------------------------------------------------

#[test]
fn the_region_goes_into_the_host() {
    assert_eq!(base_url("eu"), "https://openapi.tuyaeu.com");
    assert_eq!(base_url("us"), "https://openapi.tuyaus.com");
    assert_eq!(base_url("cn"), "https://openapi.tuyacn.com");
}

// --- read_temperature --------------------------------------------------------

fn temp(status: Value) -> Option<f64> {
    read_temperature(&status).unwrap()
}

#[test]
fn the_reading_is_scaled_from_tenths_of_a_degree() {
    assert_eq!(
        temp(json!([{"code": "va_temperature", "value": 235}])),
        Some(23.5)
    );
}

#[test]
fn the_temperature_is_picked_out_of_the_other_readings() {
    let status = json!([
        {"code": "va_humidity", "value": 55},
        {"code": "battery_percentage", "value": 90},
        {"code": "va_temperature", "value": 212},
    ]);
    assert_eq!(temp(status), Some(21.2));
}

#[test]
fn a_missing_or_unusable_reading_gives_none() {
    for status in [
        json!([]),
        json!(null),
        json!([{"code": "va_humidity", "value": 55}]),
        json!([{"code": "va_temperature"}]),
        json!([{"code": "va_temperature", "value": null}]),
        json!([{"code": "va_temperature", "value": "warm"}]),
    ] {
        assert_eq!(temp(status.clone()), None, "{status}");
    }
}

#[test]
fn a_reading_below_zero_survives() {
    assert_eq!(
        temp(json!([{"code": "va_temperature", "value": -55}])),
        Some(-5.5)
    );
}

#[test]
fn the_scale_is_a_named_constant() {
    assert_eq!(TEMPERATURE_SCALE, 10.0);
}

// --- unwrap ------------------------------------------------------------------

#[test]
fn a_successful_response_gives_its_result() {
    assert_eq!(
        unwrap(&json!({"success": true, "result": {"a": 1}}), "Token").unwrap(),
        json!({"a": 1})
    );
}

#[test]
fn a_refused_response_raises_with_the_payload() {
    let e = unwrap(&json!({"success": false, "msg": "sign invalid"}), "Token").unwrap_err();
    assert!(e.to_string().starts_with("Token failed"), "{e}");
    assert!(e.to_string().contains("sign invalid"));
}

#[test]
fn a_response_without_a_success_field_is_treated_as_refused() {
    assert!(unwrap(&json!({"result": {}}), "Status").is_err());
}

#[test]
fn the_real_expired_subscription_answer_is_refused() {
    let payload = json!({"code": 28841002, "msg": "IoT Core service subscription has expired.", "success": false, "t": 1, "tid": "x"});
    assert!(unwrap(&payload, "Status")
        .unwrap_err()
        .to_string()
        .contains("expired"));
}

// --- format_hashrate ---------------------------------------------------------

fn rate(value: Value, unit: &str) -> String {
    format_hashrate(&value, unit)
}

#[test]
fn the_readings_the_pool_sends() {
    for (value, unit, expected) in [
        (json!(12500), "Gh/s", "12.5TH/s"),
        (json!(1000), "Gh/s", "1.0TH/s"),
        (json!(999), "Gh/s", "999.0Gh/s"),
        (json!(500), "Gh/s", "500.0Gh/s"),
        (json!(12.5), "TH/s", "12.5TH/s"),
        (json!(1.5), "Ph/s", "1500.0TH/s"),
        (json!(0.5), "Ph/s", "500.0TH/s"),
        (json!(100), "Mh/s", "100.0Mh/s"),
    ] {
        assert_eq!(rate(value, unit), expected);
    }
}

#[test]
fn an_unknown_unit_is_shown_as_it_arrived() {
    assert_eq!(rate(json!(42), "Zh/s"), "42.0Zh/s");
}

#[test]
fn a_reading_that_is_not_a_number_shows_as_unknown() {
    for value in [json!(null), json!(""), json!("abc"), json!([]), json!({})] {
        assert_eq!(rate(value, "TH/s"), "--");
    }
}

#[test]
fn a_zero_reading_is_shown_rather_than_hidden() {
    assert_eq!(rate(json!(0), "TH/s"), "0.0TH/s");
}

#[test]
fn a_numeric_string_is_accepted() {
    assert_eq!(rate(json!("12.5"), "TH/s"), "12.5TH/s");
}

#[test]
fn every_known_unit_converts_once_it_is_large_enough() {
    for (unit, factor) in TO_TERAHASH {
        let ten_terahash = 10.0 / factor;
        assert!(rate(json!(ten_terahash), unit).ends_with("TH/s"), "{unit}");
    }
}

#[test]
fn a_reading_below_a_terahash_keeps_its_own_unit() {
    for (unit, factor) in TO_TERAHASH {
        let below = 0.5 / factor;
        assert!(rate(json!(below), unit).ends_with(unit), "{unit}");
    }
}

// --- format_temperature and format_title ---------------------------------------

#[test]
fn the_temperature_is_shown_to_one_decimal() {
    assert_eq!(format_temperature(Some(23.456)), "23.5");
}

#[test]
fn a_missing_temperature_shows_as_unknown() {
    assert_eq!(format_temperature(None), "--");
}

#[test]
fn a_temperature_below_zero_keeps_its_sign() {
    assert_eq!(format_temperature(Some(-5.5)), "-5.5");
}

#[test]
fn the_title_puts_both_readings_together() {
    assert_eq!(format_title("12.5TH/s", "23.5"), "12.5TH/s 23.5°C");
}

#[test]
fn the_title_still_reads_when_a_part_is_missing() {
    assert_eq!(format_title("--", "--"), "-- --°C");
}

// --- compose_title: what the bar shows when a source fails ----------------------

#[test]
fn a_failed_pool_read_keeps_the_temperature() {
    assert_eq!(
        compose_title(&Value::Null, &Value::Null, "btc", "23.5"),
        "-- 23.5°C"
    );
}

#[test]
fn a_malformed_pool_answer_shows_error_and_keeps_the_temperature() {
    let profile = json!({"btc": {"hash_rate_unit": "Gh/s"}});
    assert_eq!(
        compose_title(&profile, &json!({"x": 1}), "btc", "23.5"),
        "Error 23.5°C"
    );
}

// --- configuration (Python bugs fixed) ------------------------------------------

fn config(vars: &[(&str, &str)]) -> Result<Config, String> {
    let map: HashMap<String, String> = vars
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    Config::from_lookup(move |name| map.get(name).cloned())
}

#[test]
fn the_defaults_match_the_python_widget() {
    let c = config(&[]).unwrap();
    assert_eq!(c.coin, "btc");
    assert_eq!(c.base_url, "https://pool.braiins.com");
    assert_eq!(c.update_interval, Duration::from_secs(300));
    assert_eq!(c.max_retries, 3);
    assert_eq!(c.retry_delay, Duration::from_secs(5));
    assert_eq!(c.tuya.region, "eu");
    assert!(c.token.is_none());
}

#[test]
fn a_zero_interval_is_refused_instead_of_spinning() {
    let e = config(&[("UPDATE_INTERVAL", "0")]).err().unwrap();
    assert!(e.contains("UPDATE_INTERVAL"), "{e}");
}

#[test]
fn a_typo_in_a_number_names_the_variable() {
    let e = config(&[("MAX_RETRIES", "three")]).err().unwrap();
    assert!(e.contains("MAX_RETRIES"), "{e}");
}

#[test]
fn a_negative_retry_count_means_no_attempts() {
    assert_eq!(config(&[("MAX_RETRIES", "-2")]).unwrap().max_retries, 0);
}

#[test]
fn missing_tuya_variables_are_listed_in_order() {
    let c = config(&[("TUYA_SECRET_UNUSED", "x"), ("TUYA_ACCESS_SECRET", "s")]).unwrap();
    let client = reqwest::blocking::Client::new();
    let e = c.tuya.get_access_token(&client).unwrap_err().to_string();
    assert_eq!(e, "Missing in .env: TUYA_ACCESS_ID, TUYA_DEVICE_ID");
}

#[test]
fn dotenv_is_found_in_a_parent_folder() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join(".env"), "A=1\n").unwrap();
    let deep = dir.path().join("target/release");
    std::fs::create_dir_all(&deep).unwrap();
    assert_eq!(find_dotenv(&deep), Some(dir.path().join(".env")));
}

// --- check_internet: the probe socket is closed again (Python bug fixed) ---------

#[test]
fn the_probe_reports_a_listening_port_and_a_closed_one() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    assert!(check_internet(addr, Duration::from_secs(1)));
    // The probe closed its end: the listener sees EOF instead of an open connection.
    let (mut stream, _) = listener.accept().unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    assert_eq!(stream.read(&mut [0u8; 1]).unwrap(), 0);
    drop(listener);
    assert!(!check_internet(addr, Duration::from_secs(1)));
}

// --- logger -----------------------------------------------------------------------

#[test]
fn the_log_rotates_and_keeps_five_backups() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("miner.log");
    let logger = Logger::new(path.clone(), 200);
    for i in 0..60 {
        logger.log("INFO", &format!("line {i:03} padding padding padding"));
    }
    assert!(path.exists());
    for n in 1..=5 {
        assert!(
            dir.path().join(format!("miner.log.{n}")).exists(),
            "backup {n}"
        );
    }
    assert!(!dir.path().join("miner.log.6").exists());
    let last = std::fs::read_to_string(&path).unwrap();
    assert!(last.contains(" - INFO - line 059"), "{last}");
}

// --- fetch_data against a local server ----------------------------------------------

fn serve(
    responses: Vec<(u16, &'static str)>,
) -> (SocketAddr, std::thread::JoinHandle<Vec<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let handle = std::thread::spawn(move || {
        let mut requests = Vec::new();
        for (status, body) in responses {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buf = [0u8; 4096];
            let n = stream.read(&mut buf).unwrap();
            requests.push(String::from_utf8_lossy(&buf[..n]).to_string());
            let reply = format!(
                "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
            stream.write_all(reply.as_bytes()).unwrap();
        }
        requests
    });
    (addr, handle)
}

#[test]
fn fetch_data_retries_and_sends_the_token_header() {
    let (addr, server) = serve(vec![
        (500, "{}"),
        (200, r#"{"btc":{}}"#),
        (200, r#"{"btc":{"workers":{}}}"#),
    ]);
    let url = format!("http://{addr}");
    let c = config(&[
        ("TOKEN", "secret-token"),
        ("BASE_URL", &url),
        ("RETRY_DELAY", "0"),
    ])
    .unwrap();
    let (profile, workers) = fetch_data(&reqwest::blocking::Client::new(), &c);
    assert_eq!(profile, json!({"btc": {}}));
    assert_eq!(workers, json!({"btc": {"workers": {}}}));
    let requests = server.join().unwrap();
    assert!(requests[0].starts_with("GET /accounts/profile/json/btc/ "));
    assert!(requests[2].starts_with("GET /accounts/workers/json/btc/ "));
    for r in &requests {
        assert!(r.to_lowercase().contains("pool-auth-token: secret-token"));
    }
}

#[test]
fn fetch_data_gives_up_after_the_configured_attempts() {
    let (addr, server) = serve(vec![(503, "{}"), (503, "{}")]);
    let url = format!("http://{addr}");
    let c = config(&[
        ("TOKEN", "t"),
        ("BASE_URL", &url),
        ("MAX_RETRIES", "2"),
        ("RETRY_DELAY", "0"),
    ])
    .unwrap();
    assert_eq!(
        fetch_data(&reqwest::blocking::Client::new(), &c),
        (Value::Null, Value::Null)
    );
    assert_eq!(server.join().unwrap().len(), 2);
}

#[test]
fn fetch_data_without_a_token_makes_no_request() {
    let c = config(&[("TOKEN", "")]).unwrap();
    assert_eq!(
        fetch_data(&reqwest::blocking::Client::new(), &c),
        (Value::Null, Value::Null)
    );
}
