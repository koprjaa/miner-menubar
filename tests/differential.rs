// Project: miner-menubar
// File: tests/differential.rs
// Description: Replay observations recorded from the original Python widget against the Rust code.
// Author: Jan Alexandr Kopřiva
// License: AGPL-3.0-or-later
use miner_menubar::hashrate::{format_hashrate, format_temperature};
use miner_menubar::tuya::{base_url, headers_at, read_temperature, signature, unwrap};
use miner_menubar::widget::{compose_title, temperature_text, OFFLINE_TITLE};
use serde_json::{Map, Value};

/// What the Rust code produces for one recorded case.
fn run(kind: &str, input: &Value) -> String {
    let s = |i: usize| input[i].as_str().unwrap();
    match kind {
        "sig" => signature(s(0), s(1), s(2), s(3), s(4), s(5)),
        "headers" => {
            let map: Map<String, Value> = headers_at(s(0), s(1), s(2), s(3), s(4), s(5))
                .into_iter()
                .map(|(k, v)| (k.to_string(), Value::String(v)))
                .collect();
            Value::Object(map).to_string()
        }
        "base_url" => base_url(input.as_str().unwrap()),
        "hashrate" => match input[1].as_str() {
            Some(unit) => format_hashrate(&input[0], unit),
            None => "ERR".into(),
        },
        "temp" => read_temperature(input).map_or_else(|_| "ERR".into(), format_temperature),
        "unwrap" => unwrap(input, "Token").map_or_else(|_| "ERR".into(), |v| v.to_string()),
        "title" => {
            if input[0] == "OFFLINE" {
                return OFFLINE_TITLE.into();
            }
            let temp = if input[3] == "FAIL" {
                "--".into()
            } else {
                temperature_text(&input[3])
            };
            compose_title(&input[0], &input[1], s(2), &temp)
        }
        other => panic!("unknown kind {other}"),
    }
}

/// JSON outputs are compared as values: Python spaces its separators differently.
fn same(kind: &str, a: &str, b: &str) -> bool {
    if matches!(kind, "headers" | "unwrap") && a != "ERR" && b != "ERR" {
        return serde_json::from_str::<Value>(a).unwrap()
            == serde_json::from_str::<Value>(b).unwrap();
    }
    a == b
}

#[test]
fn replay_python_differential_corpus() {
    let mut failures = Vec::new();
    let mut cases = 0;
    for line in include_str!("fixtures/differential.tsv")
        .lines()
        .filter(|l| !l.starts_with('#') && !l.is_empty())
    {
        let f: Vec<_> = line.split('\t').collect();
        assert_eq!(f.len(), 6, "bad row: {line}");
        let [kind, name, input, python, rust, reason] = f[..] else {
            unreachable!()
        };
        cases += 1;
        if same(kind, python, rust) {
            assert_eq!(reason, "compatible", "{kind}/{name}");
        } else {
            assert!(
                reason.starts_with("intentional:"),
                "undocumented difference: {kind}/{name}"
            );
        }
        let actual = run(kind, &serde_json::from_str(input).unwrap());
        if !same(kind, &actual, rust) {
            failures.push(format!(
                "{kind}/{name}: rust gave {actual:?}, recorded {rust:?} (python {python:?})"
            ));
        }
    }
    assert!(cases > 100, "corpus went missing");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The real pool answers captured from the API, identifiers replaced.
#[test]
fn real_pool_fixtures_give_the_title_python_gave() {
    let profile: Value = serde_json::from_str(include_str!("fixtures/pool_profile.json")).unwrap();
    let workers: Value = serde_json::from_str(include_str!("fixtures/pool_workers.json")).unwrap();
    assert_eq!(
        compose_title(&profile, &workers, "btc", "23.5"),
        "0.0Gh/s 23.5°C"
    );
    assert_eq!(
        compose_title(&profile, &workers, "btc", "--"),
        "0.0Gh/s --°C"
    );
}
