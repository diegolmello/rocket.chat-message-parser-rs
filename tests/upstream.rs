use std::panic::{self, AssertUnwindSafe};
use std::time::SystemTime;

mod support;

use message_parser::{parse_at, Node};
use serde_json::Value;
use support::{clock_at, js_equal, pretty, Case, Fixture, Matcher};

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

fn check_case(case: &Case, now: SystemTime) -> Result<(), String> {
    let options = case.options.to_options();
    let nodes: Vec<Node> =
        panic::catch_unwind(AssertUnwindSafe(|| parse_at(&case.input, &options, now)))
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
