#![allow(dead_code)]

use std::time::SystemTime;

use message_parser::{KatexOptions, Options};
use serde::{Deserialize, Serialize};
use serde_json::Value;

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
        let katex = self.katex.as_ref();
        Options {
            colors: self.colors.unwrap_or(false),
            emoticons: self.emoticons.unwrap_or(false),
            katex: KatexOptions {
                dollar_syntax: katex.and_then(|katex| katex.dollar_syntax).unwrap_or(false),
                parenthesis_syntax: katex
                    .and_then(|katex| katex.parenthesis_syntax)
                    .unwrap_or(false),
            },
            custom_domains: self.custom_domains.clone(),
        }
    }
}

pub fn js_equal(actual: &Value, expected: &Value) -> bool {
    match (actual, expected) {
        (Value::Number(actual), Value::Number(expected)) => actual.as_f64() == expected.as_f64(),
        (Value::Array(actual), Value::Array(expected)) => {
            actual.len() == expected.len()
                && actual
                    .iter()
                    .zip(expected)
                    .all(|(actual, expected)| js_equal(actual, expected))
        }
        (Value::Object(actual), Value::Object(expected)) => {
            actual.len() == expected.len()
                && expected.iter().all(|(key, expected)| {
                    actual
                        .get(key)
                        .is_some_and(|actual| js_equal(actual, expected))
                })
        }
        (actual, expected) => actual == expected,
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Fixture {
    pub file: String,
    #[serde(rename = "upstreamVersion")]
    _upstream_version: String,
    pub groups: Vec<Group>,
    #[serde(rename = "uncaptured", default)]
    _uncaptured: Vec<Value>,
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

pub fn pretty(value: &Value) -> String {
    serde_json::to_string_pretty(value).expect("JSON values always serialize")
}

pub fn clock_at(instant: &str) -> SystemTime {
    chrono::DateTime::parse_from_rfc3339(instant)
        .expect("fixture clock is an RFC 3339 instant")
        .into()
}
