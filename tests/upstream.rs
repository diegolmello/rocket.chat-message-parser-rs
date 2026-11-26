use std::panic::{self, AssertUnwindSafe};
use std::time::SystemTime;

use message_parser::{parse_at, Node, Options};
use serde::Deserialize;
use serde_json::Value;

mod common;

use common::{canonical, fixed_clock, panic_message};

const UPSTREAM_VERSION: &str = "0.33.0";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Fixture {
    file: String,
    #[serde(rename = "upstreamVersion")]
    upstream_version: String,
    groups: Vec<Group>,
    #[serde(default)]
    uncaptured: Vec<Uncaptured>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Uncaptured {
    name: String,
    #[serde(rename = "reason")]
    _reason: String,
}

const NOT_OBSERVABLE_THROUGH_PARSE: &str = "calls src/utils.autoLink directly";

const UNCAPTURED_UPSTREAM_TESTS: &[(&str, &str, &str)] = &[
    (
        "emoji.test.ts",
        "every emoji parses as one token, except documented components (emojibase@17.0.0)",
        "tests/emoji.rs::every_emoji_parses_as_one_token_except_documented_components",
    ),
    (
        "horizontalRule.test.ts",
        "does not treat \"***\" as a horizontal rule",
        "tests/horizontal_rule.rs::asterisk_and_underscore_runs_are_not_horizontal_rules",
    ),
    (
        "horizontalRule.test.ts",
        "does not treat \"___\" as a horizontal rule",
        "tests/horizontal_rule.rs::asterisk_and_underscore_runs_are_not_horizontal_rules",
    ),
    (
        "horizontalRule.test.ts",
        "does not treat \"*******\" as a horizontal rule",
        "tests/horizontal_rule.rs::asterisk_and_underscore_runs_are_not_horizontal_rules",
    ),
    (
        "horizontalRule.test.ts",
        "does not treat \"_______\" as a horizontal rule",
        "tests/horizontal_rule.rs::asterisk_and_underscore_runs_are_not_horizontal_rules",
    ),
    (
        "table.test.ts",
        "normalizes ragged body rows to the header column count",
        "tests/table.rs::normalizes_ragged_body_rows_to_the_header_column_count",
    ),
    (
        "table.test.ts",
        "exposes the source offsets as fallback for renderers without table support",
        "tests/table.rs::exposes_the_source_offsets_as_fallback_for_renderers_without_table_support",
    ),
    (
        "url.test.ts",
        "autoLink helper function > should preserve the original protocol if the protocol is http or https",
        NOT_OBSERVABLE_THROUGH_PARSE,
    ),
    (
        "url.test.ts",
        "autoLink helper function > should preserve the original protocol for invalid absolute URLs",
        NOT_OBSERVABLE_THROUGH_PARSE,
    ),
    (
        "url.test.ts",
        "autoLink helper function > should preserve the original protocol even if for custom protocols",
        NOT_OBSERVABLE_THROUGH_PARSE,
    ),
    (
        "url.test.ts",
        "autoLink helper function > should return // as the protocol if // is the protocol specified",
        NOT_OBSERVABLE_THROUGH_PARSE,
    ),
    (
        "url.test.ts",
        "autoLink helper function > should return an url concatenated '//' if the url has no protocol",
        NOT_OBSERVABLE_THROUGH_PARSE,
    ),
    (
        "url.test.ts",
        "autoLink helper function > should return an url concatenated '//' if the url has no protocol and has sub-domain",
        NOT_OBSERVABLE_THROUGH_PARSE,
    ),
    (
        "url.test.ts",
        "autoLink helper function > should return an plain text url due to invalid TLD that's validate with the external library TLDTS",
        NOT_OBSERVABLE_THROUGH_PARSE,
    ),
];

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
    options: Options,
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

fn js_equal(actual: &Value, expected: &Value) -> bool {
    canonical(actual) == canonical(expected)
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

fn pretty(value: &Value) -> String {
    serde_json::to_string_pretty(value).expect("JSON values always serialize")
}

fn check_case(case: &Case, now: SystemTime) -> Result<(), String> {
    let options = &case.options;
    let nodes: Vec<Node> =
        panic::catch_unwind(AssertUnwindSafe(|| parse_at(&case.input, options, now)))
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

fn fixture_failures(fixture: &Fixture) -> Vec<String> {
    let mut failures = Vec::new();
    for group in &fixture.groups {
        let now = match &group.needs_clock {
            Some(clock) => clock_at(clock),
            None => fixed_clock(),
        };
        for case in &group.cases {
            if let Err(reason) = check_case(case, now) {
                failures.push(format!(
                    "[{}] {}\n  input:   {:?}\n  options: {:?}\n  {reason}",
                    fixture.file, case.name, case.input, case.options
                ));
            }
        }
    }
    failures
}

const FIXTURE_DIRECTORY: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/upstream");

fn fixture_paths() -> Vec<std::path::PathBuf> {
    let mut paths: Vec<std::path::PathBuf> = std::fs::read_dir(FIXTURE_DIRECTORY)
        .expect("fixture directory exists")
        .map(|entry| entry.expect("fixture directory entry is readable").path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "json")
        })
        .collect();
    paths.sort();
    paths
}

fn load_fixtures() -> Vec<Fixture> {
    let fixtures: Vec<Fixture> = fixture_paths()
        .iter()
        .map(|path| {
            let source = std::fs::read_to_string(path).expect("fixture is readable");
            serde_json::from_str(&source).expect("fixture JSON matches the extractor's schema")
        })
        .collect();
    assert!(!fixtures.is_empty(), "no fixtures in {FIXTURE_DIRECTORY}");
    for fixture in &fixtures {
        assert_eq!(
            fixture.upstream_version, UPSTREAM_VERSION,
            "{} was extracted from a different upstream version",
            fixture.file
        );
    }
    fixtures
}

#[test]
fn every_uncaptured_upstream_test_has_a_hand_port_or_a_reason() {
    let mut uncaptured: Vec<(String, String)> = load_fixtures()
        .into_iter()
        .flat_map(|fixture| {
            let file = fixture.file;
            fixture
                .uncaptured
                .into_iter()
                .map(move |test| (file.clone(), test.name))
        })
        .collect();
    uncaptured.sort();
    let mut accounted: Vec<(String, String)> = UNCAPTURED_UPSTREAM_TESTS
        .iter()
        .map(|(file, name, _)| ((*file).to_owned(), (*name).to_owned()))
        .collect();
    accounted.sort();
    assert_eq!(
        uncaptured, accounted,
        "an upstream test the fixtures cannot capture needs a hand port or a reason in UNCAPTURED_UPSTREAM_TESTS"
    );
    for (_, name, covered_by) in UNCAPTURED_UPSTREAM_TESTS {
        if *covered_by != NOT_OBSERVABLE_THROUGH_PARSE {
            let (file, test_function) = covered_by
                .split_once("::")
                .expect("a hand port is named as <file>::<test function>");
            let hand_port = concat!(env!("CARGO_MANIFEST_DIR"), "/").to_owned() + file;
            let source = std::fs::read_to_string(&hand_port)
                .unwrap_or_else(|_| panic!("{name}: hand port {file} is missing"));
            assert!(
                source.contains(&format!("fn {test_function}()")),
                "{name}: hand port {covered_by} is missing"
            );
        }
    }
}

#[test]
fn every_upstream_fixture_case_matches() {
    let fixtures = load_fixtures();
    let checked: usize = fixtures
        .iter()
        .flat_map(|fixture| &fixture.groups)
        .map(|group| group.cases.len())
        .sum();
    let failures: Vec<String> = fixtures.iter().flat_map(fixture_failures).collect();
    assert!(
        failures.is_empty(),
        "{} of {checked} case(s) failed:\n\n{}",
        failures.len(),
        failures.join("\n\n")
    );
}
