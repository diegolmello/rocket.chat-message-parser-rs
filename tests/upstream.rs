use std::panic::{self, AssertUnwindSafe};

use std::time::SystemTime;

use message_parser::{parse_at, KatexOptions, Node, Options};
use serde::Deserialize;
use serde_json::Value;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Fixture {
    file: String,
    #[serde(rename = "upstreamVersion")]
    _upstream_version: String,
    groups: Vec<Group>,
    #[serde(rename = "uncaptured", default)]
    _uncaptured: Vec<Value>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Group {
    needs_clock: Option<String>,
    cases: Vec<Case>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Case {
    name: String,
    input: String,
    #[serde(default)]
    options: JsOptions,
    matcher: Matcher,
    expected: Option<Value>,
}

#[derive(Deserialize, Clone, Copy, Debug)]
#[serde(rename_all = "camelCase")]
enum Matcher {
    ToEqual,
    ToStrictEqual,
    ToMatchObject,
    NotToThrow,
}

#[derive(Deserialize, Default, Debug)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct JsOptions {
    colors: Option<bool>,
    emoticons: Option<bool>,
    katex: Option<JsKatexOptions>,
    custom_domains: Option<Vec<String>>,
}

#[derive(Deserialize, Default, Debug)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct JsKatexOptions {
    dollar_syntax: Option<bool>,
    parenthesis_syntax: Option<bool>,
}

impl JsOptions {
    fn to_options(&self) -> Options {
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

fn js_equal(actual: &Value, expected: &Value) -> bool {
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

fn matches_object(actual: &Value, expected: &Value) -> bool {
    match (actual, expected) {
        (Value::Array(actual), Value::Array(expected)) => {
            actual.len() == expected.len()
                && actual
                    .iter()
                    .zip(expected)
                    .all(|(actual, expected)| matches_object(actual, expected))
        }
        (Value::Object(actual), Value::Object(expected)) => {
            expected.iter().all(|(key, expected)| {
                actual
                    .get(key)
                    .is_some_and(|actual| matches_object(actual, expected))
            })
        }
        (actual, expected) => js_equal(actual, expected),
    }
}

fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|message| (*message).to_owned())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "non-string panic payload".to_owned())
}

fn pretty(value: &Value) -> String {
    serde_json::to_string_pretty(value).expect("JSON values always serialize")
}

fn check_case(case: &Case, now: SystemTime) -> Result<(), String> {
    let options = case.options.to_options();
    let nodes: Vec<Node> = panic::catch_unwind(AssertUnwindSafe(|| {
        parse_at(&case.input, &options, now).nodes
    }))
    .map_err(|payload| format!("parse panicked: {}", panic_message(payload.as_ref())))?;
    let expected = match (case.matcher, &case.expected) {
        (Matcher::NotToThrow, _) => return Ok(()),
        (_, Some(expected)) => expected,
        (matcher, None) => {
            return Err(format!(
                "fixture case has matcher {matcher:?} but no expected value"
            ))
        }
    };
    let actual = serde_json::to_value(&nodes)
        .map_err(|error| format!("AST failed to serialize: {error}"))?;
    let holds = match case.matcher {
        Matcher::ToMatchObject => matches_object(&actual, expected),
        _ => js_equal(&actual, expected),
    };
    if holds {
        return Ok(());
    }
    Err(format!(
        "{:?} mismatch\n  expected: {}\n  actual:   {}",
        case.matcher,
        pretty(expected),
        pretty(&actual)
    ))
}

fn clock_at(instant: &str) -> SystemTime {
    chrono::DateTime::parse_from_rfc3339(instant)
        .expect("fixture clock is an RFC 3339 instant")
        .into()
}

fn run_fixture(source: &str) {
    let fixture: Fixture =
        serde_json::from_str(source).expect("fixture JSON matches the extractor's schema");
    let mut failures = Vec::new();
    let mut checked = 0;
    for group in &fixture.groups {
        let now = match &group.needs_clock {
            Some(clock) => clock_at(clock),
            None => SystemTime::now(),
        };
        for case in &group.cases {
            checked += 1;
            if let Err(reason) = check_case(case, now) {
                failures.push(format!(
                    "[{}] {}\n  input:   {:?}\n  options: {:?}\n  {reason}",
                    fixture.file, case.name, case.input, case.options
                ));
            }
        }
    }
    if !failures.is_empty() {
        panic!(
            "{} of {checked} case(s) from {} failed:\n\n{}",
            failures.len(),
            fixture.file,
            failures.join("\n\n")
        );
    }
}

macro_rules! upstream_fixtures {
    ($($test_name:ident => $fixture_stem:literal),* $(,)?) => {
        $(
            #[test]
            fn $test_name() {
                run_fixture(include_str!(concat!("fixtures/upstream/", $fixture_stem, ".json")));
            }
        )*

        const FIXTURE_STEMS: &[&str] = &[$($fixture_stem),*];
    };
}

upstream_fixtures! {
    abuse => "abuse",
    any => "any",
    blockquotes => "blockquotes",
    code_fence => "codeFence",
    color => "color",
    email => "email",
    emoji => "emoji",
    emoticons => "emoticons",
    emphasis => "emphasis",
    emphasis_with_emoticons => "emphasisWithEmoticons",
    escaped => "escaped",
    heading => "heading",
    horizontal_rule => "horizontalRule",
    image => "image",
    inline_code => "inlineCode",
    inline_code_strike => "inlineCodeStrike",
    katex => "katex",
    line_break => "lineBreak",
    link => "link",
    long_code_fence => "longCodeFence",
    mention => "mention",
    nested_list => "nestedList",
    ordered_list => "orderedList",
    phone_number => "phoneNumber",
    skip_flags_regression => "skip-flags-regression",
    spoiler => "spoiler",
    spoiler_block => "spoilerBlock",
    strikethrough => "strikethrough",
    strong_emphasis => "strongEmphasis",
    table => "table",
    tasks => "tasks",
    timestamp => "timestamp",
    unordered_list => "unorderedList",
    url => "url",
}

#[test]
fn every_fixture_file_has_a_test() {
    let directory = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/upstream");
    let mut on_disk: Vec<String> = std::fs::read_dir(directory)
        .expect("fixture directory exists")
        .map(|entry| entry.expect("fixture directory entry is readable").path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "json")
        })
        .map(|path| {
            path.file_stem()
                .expect("fixture has a name")
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    on_disk.sort();
    let mut covered: Vec<String> = FIXTURE_STEMS
        .iter()
        .map(|stem| (*stem).to_owned())
        .collect();
    covered.sort();
    assert_eq!(
        on_disk, covered,
        "tests/upstream.rs must list every fixture in {directory}"
    );
}
