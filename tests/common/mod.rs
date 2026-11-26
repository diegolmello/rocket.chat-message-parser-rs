#![allow(dead_code)]

use std::any::Any;
use std::collections::HashSet;
use std::ops::RangeInclusive;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use message_parser::Options;
use serde::{Deserialize, Serialize};
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

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct JsOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub colors: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub emoticons: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub katex: Option<JsKatexOptions>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub custom_domains: Option<Vec<String>>,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct JsKatexOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dollar_syntax: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parenthesis_syntax: Option<bool>,
}

impl JsOptions {
    pub fn to_options(&self) -> Options {
        let upstream = serde_json::to_value(self).expect("options serialize");
        serde_json::from_value(upstream).expect("upstream options deserialize")
    }
}

pub fn js_equal(actual: &Value, expected: &Value) -> bool {
    canonical(actual) == canonical(expected)
}

pub fn pretty(value: &Value) -> String {
    serde_json::to_string_pretty(value).expect("JSON values always serialize")
}

pub fn clock_at(instant: &str) -> SystemTime {
    chrono::DateTime::parse_from_rfc3339(instant)
        .expect("fixture clock is an RFC 3339 instant")
        .into()
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Fixture {
    pub file: String,
    #[serde(rename = "upstreamVersion")]
    pub upstream_version: String,
    pub groups: Vec<Group>,
    #[serde(default)]
    pub uncaptured: Vec<Uncaptured>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Uncaptured {
    pub name: String,
    #[serde(rename = "reason")]
    _reason: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Group {
    pub needs_clock: Option<String>,
    pub cases: Vec<Case>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Case {
    pub name: String,
    pub input: String,
    #[serde(default)]
    pub options: JsOptions,
    pub matcher: Matcher,
    pub expected: Option<Value>,
}

#[derive(Deserialize, Clone, Copy, Debug)]
#[serde(rename_all = "camelCase")]
pub enum Matcher {
    ToEqual,
    ToStrictEqual,
    ToMatchObject,
    NotToThrow,
}
