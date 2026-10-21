// Project: miner-menubar
// File: src/hashrate.rs
// Description: Turns a pool hash rate and a temperature reading into menu bar text.
// Author: Jan Alexandr Kopřiva
// License: MIT

use serde_json::Value;

/// Multipliers onto TH/s, which is the unit the widget prefers to show.
pub const TO_TERAHASH: [(&str, f64); 7] = [
    ("Kh/s", 1e-9),
    ("Mh/s", 1e-6),
    ("Gh/s", 1e-3),
    ("Th/s", 1.0),
    ("TH/s", 1.0),
    ("Ph/s", 1e3),
    ("Eh/s", 1e6),
];

/// Below this the reading is shown in its own unit instead. One tenth of a
/// terahash reads better as 100 Gh/s than as 0.1 TH/s.
const MIN_TERAHASH: f64 = 1.0;

/// One decimal, spelled like Python does for the values that are not finite.
fn one_decimal(value: f64) -> String {
    if value.is_nan() {
        "nan".into()
    } else if value.is_infinite() {
        if value < 0.0 { "-inf" } else { "inf" }.into()
    } else {
        format!("{value:.1}")
    }
}

/// The reading as a number, or None when it is not one.
fn as_number(value: &Value) -> Option<f64> {
    match value {
        Value::Number(n) => n.as_f64(),
        Value::Bool(b) => Some(f64::from(u8::from(*b))),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

/// Hash rate as it appears on the menu bar.
///
/// A reading large enough is converted to TH/s. A smaller one keeps the unit
/// the pool used, and an unknown unit is passed through rather than guessed at.
pub fn format_hashrate(value: &Value, unit: &str) -> String {
    let Some(value) = as_number(value) else {
        return "--".into();
    };
    let Some((_, factor)) = TO_TERAHASH.iter().find(|(name, _)| *name == unit) else {
        return format!("{}{unit}", one_decimal(value));
    };
    let terahash = value * factor;
    if terahash >= MIN_TERAHASH {
        format!("{}TH/s", one_decimal(terahash))
    } else {
        format!("{}{unit}", one_decimal(value))
    }
}

/// Temperature as it appears on the menu bar.
pub fn format_temperature(celsius: Option<f64>) -> String {
    celsius.map_or_else(|| "--".into(), one_decimal)
}

/// The whole menu bar entry.
pub fn format_title(hashrate: &str, temperature: &str) -> String {
    format!("{hashrate} {temperature}°C")
}
