#![allow(dead_code)]

use std::any::Any;
use std::collections::HashSet;
use std::ops::RangeInclusive;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::Value;

pub const CLOCK_SECONDS: u64 = 1_753_142_400;

pub fn fixed_clock() -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(CLOCK_SECONDS)
}

pub fn panic_message(payload: &(dyn Any + Send)) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|message| (*message).to_owned())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "non-string panic payload".to_owned())
}

pub fn canonical(value: &Value) -> Value {
    match value {
        Value::Array(items) => Value::Array(items.iter().map(canonical).collect()),
        Value::Object(fields) => Value::Object(
            fields
                .iter()
                .map(|(key, field)| (key.clone(), canonical(field)))
                .collect::<serde_json::Map<_, _>>(),
        ),
        Value::Number(number) => number
            .as_f64()
            .and_then(serde_json::Number::from_f64)
            .map_or_else(|| value.clone(), Value::Number),
        _ => value.clone(),
    }
}

pub fn strings_over(pieces: &[&str], lengths: RangeInclusive<usize>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut strings = Vec::new();
    let mut layer = vec![String::new()];
    for length in 0..=*lengths.end() {
        if length > 0 {
            layer = layer
                .iter()
                .flat_map(|prefix| pieces.iter().map(move |piece| format!("{prefix}{piece}")))
                .collect();
        }
        if lengths.contains(&length) {
            strings.extend(
                layer
                    .iter()
                    .filter(|string| seen.insert((*string).clone()))
                    .cloned(),
            );
        }
    }
    strings
}
