// Project: miner-widget-stats
// File: src/tuya.rs
// Description: Signs Tuya Cloud requests and reads a temperature from a device.
// Author: Jan Alexandr Kopřiva
// License: MIT

use hmac::{Hmac, KeyInit, Mac};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::fmt;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Tuya reports temperature in tenths of a degree.
pub const TEMPERATURE_SCALE: f64 = 10.0;

/// Status code holding the ambient temperature.
pub const TEMPERATURE_CODE: &str = "va_temperature";

/// Seconds to wait on a Tuya call. The widget updates every few minutes, so a
/// stalled request must not hold its update thread.
const TIMEOUT: Duration = Duration::from_secs(15);

/// Tuya answered, but refused the request, or the answer made no sense.
#[derive(Debug)]
pub struct TuyaError(pub String);

impl fmt::Display for TuyaError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for TuyaError {}

pub fn base_url(region: &str) -> String {
    format!("https://openapi.tuya{region}.com")
}

/// Python truthiness of a JSON value.
pub fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64() != Some(0.0),
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}

/// HMAC-SHA256 signature for one GET request, upper case hex.
///
/// Tuya signs the method, a hash of the body, an empty header line and the
/// path, then prefixes the client id, the token, the timestamp and the nonce.
/// The order is fixed by the API and every part is required.
pub fn signature(
    access_id: &str,
    secret: &str,
    path: &str,
    ts: &str,
    nonce: &str,
    token: &str,
) -> String {
    let empty_body = hex(&Sha256::digest(b""));
    let string_to_sign = ["GET", &empty_body, "", path].join("\n");
    let base = format!("{access_id}{token}{ts}{nonce}{string_to_sign}");
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).expect("HMAC takes any key");
    mac.update(base.as_bytes());
    hex(&mac.finalize().into_bytes()).to_uppercase()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Signed headers for a GET with a given timestamp and nonce.
pub fn headers_at(
    access_id: &str,
    secret: &str,
    path: &str,
    token: &str,
    ts: &str,
    nonce: &str,
) -> Vec<(&'static str, String)> {
    let mut headers = vec![
        ("client_id", access_id.to_string()),
        ("sign_method", "HMAC-SHA256".to_string()),
        ("t", ts.to_string()),
        ("nonce", nonce.to_string()),
        ("sign", signature(access_id, secret, path, ts, nonce, token)),
    ];
    if !token.is_empty() {
        headers.push(("access_token", token.to_string()));
    }
    headers
}

/// Signed headers for a GET. The timestamp and nonce are generated here.
pub fn make_headers(
    access_id: &str,
    secret: &str,
    path: &str,
    token: &str,
) -> Vec<(&'static str, String)> {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis());
    let nonce = uuid::Uuid::new_v4().simple().to_string();
    headers_at(access_id, secret, path, token, &millis.to_string(), &nonce)
}

/// Ambient temperature in degrees, `Ok(None)` when the device did not report
/// it, and an error when the status is not a list of objects.
pub fn read_temperature(status: &Value) -> Result<Option<f64>, TuyaError> {
    if !truthy(status) {
        return Ok(None);
    }
    let Value::Array(items) = status else {
        return Err(TuyaError("status is not a list".into()));
    };
    for item in items {
        let Value::Object(item) = item else {
            return Err(TuyaError("status item is not an object".into()));
        };
        if item.get("code").and_then(Value::as_str) == Some(TEMPERATURE_CODE) {
            return Ok(match item.get("value") {
                Some(Value::Number(n)) => n.as_f64().map(|v| v / TEMPERATURE_SCALE),
                Some(Value::Bool(b)) => Some(f64::from(u8::from(*b)) / TEMPERATURE_SCALE),
                _ => None,
            });
        }
    }
    Ok(None)
}

/// The result of a Tuya response, or an error when the call was refused.
pub fn unwrap(payload: &Value, what: &str) -> Result<Value, TuyaError> {
    let Value::Object(map) = payload else {
        return Err(TuyaError(format!("{what} failed: {payload}")));
    };
    if !map.get("success").is_some_and(truthy) {
        return Err(TuyaError(format!("{what} failed: {payload}")));
    }
    map.get("result")
        .cloned()
        .ok_or_else(|| TuyaError(format!("{what} failed: no result in {payload}")))
}

fn set(v: &Option<String>) -> Option<&str> {
    v.as_deref().filter(|s| !s.is_empty())
}

/// Account settings for the Tuya Cloud call.
pub struct Account {
    pub access_id: Option<String>,
    pub secret: Option<String>,
    pub region: String,
    pub device_id: Option<String>,
}

impl Account {
    /// Stop with a readable message when the account is not configured. Called
    /// from the read, not at start, so the widget runs without Tuya set up.
    fn require_credentials(&self) -> Result<(&str, &str, &str), TuyaError> {
        match (
            set(&self.access_id),
            set(&self.secret),
            set(&self.device_id),
        ) {
            (Some(a), Some(s), Some(d)) => Ok((a, s, d)),
            _ => {
                let missing: Vec<_> = [
                    ("TUYA_ACCESS_ID", &self.access_id),
                    ("TUYA_ACCESS_SECRET", &self.secret),
                    ("TUYA_DEVICE_ID", &self.device_id),
                ]
                .into_iter()
                .filter(|(_, v)| set(v).is_none())
                .map(|(name, _)| name)
                .collect();
                Err(TuyaError(format!(
                    "Missing in .env: {}",
                    missing.join(", ")
                )))
            }
        }
    }

    fn get(
        &self,
        client: &reqwest::blocking::Client,
        path: &str,
        token: &str,
        what: &str,
    ) -> Result<Value, Box<dyn std::error::Error>> {
        let (id, secret, _) = self.require_credentials()?;
        let mut request = client.get(base_url(&self.region) + path).timeout(TIMEOUT);
        for (name, value) in make_headers(id, secret, path, token) {
            request = request.header(name, value);
        }
        let body: Value = serde_json::from_str(&request.send()?.error_for_status()?.text()?)?;
        Ok(unwrap(&body, what)?)
    }

    /// Access token for the configured account.
    pub fn get_access_token(
        &self,
        client: &reqwest::blocking::Client,
    ) -> Result<String, Box<dyn std::error::Error>> {
        let result = self.get(client, "/v1.0/token?grant_type=1", "", "Token")?;
        result
            .get("access_token")
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| TuyaError("Token failed: no access_token".into()).into())
    }

    /// Status list for the configured device.
    pub fn get_device_status(
        &self,
        client: &reqwest::blocking::Client,
        token: &str,
    ) -> Result<Value, Box<dyn std::error::Error>> {
        let (_, _, device) = self.require_credentials()?;
        self.get(
            client,
            &format!("/v1.0/devices/{device}/status"),
            token,
            "Status",
        )
    }
}
