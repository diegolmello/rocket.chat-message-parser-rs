mod support;

use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use message_parser::parse_at;
use proptest::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use support::{clock_at, js_equal, pretty, Fixture, JsKatexOptions, JsOptions};

const UPSTREAM_DIRECTORY: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/upstream");
const FIXTURE_DIRECTORY: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/upstream");
const DEFAULT_GENERATED_CASES: u32 = 20_000;
const FIXTURE_CLOCK_MILLISECONDS: u64 = 1_700_000_000_000;

struct Oracle {
    process: Child,
    requests: ChildStdin,
    responses: BufReader<ChildStdout>,
}

#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
enum OracleResponse {
    Nodes(Value),
    Error(String),
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(deny_unknown_fields)]
struct ParseCase {
    input: String,
    options: JsOptions,
    now: u64,
}

impl Oracle {
    fn start() -> Oracle {
        assert!(
            Path::new(UPSTREAM_DIRECTORY).join("node_modules").is_dir(),
            "run `bun install` in {UPSTREAM_DIRECTORY} first"
        );
        let mut process = Command::new("node")
            .arg("oracle.mjs")
            .current_dir(UPSTREAM_DIRECTORY)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .expect("node is on PATH");
        let requests = process.stdin.take().expect("oracle stdin is piped");
        let responses = BufReader::new(process.stdout.take().expect("oracle stdout is piped"));
        Oracle {
            process,
            requests,
            responses,
        }
    }

    fn parse(&mut self, case: &ParseCase) -> Result<Value, String> {
        let request = serde_json::to_string(case).expect("parse case serializes");
        let mut line = String::new();
        let exchanged =
            writeln!(self.requests, "{request}").and_then(|()| self.responses.read_line(&mut line));
        if !matches!(exchanged, Ok(length) if length > 0) {
            let _ = self.process.kill();
            let status = self.process.wait().expect("oracle process can be awaited");
            *self = Oracle::start();
            return Err(format!("upstream oracle exited ({status})"));
        }
        match serde_json::from_str(&line).expect("oracle responds with one JSON object per line") {
            OracleResponse::Nodes(nodes) => Ok(nodes),
            OracleResponse::Error(error) => Err(format!("upstream threw: {error}")),
        }
    }
}

fn oracle() -> MutexGuard<'static, Oracle> {
    static ORACLE: OnceLock<Mutex<Oracle>> = OnceLock::new();
    ORACLE
        .get_or_init(|| Mutex::new(Oracle::start()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn rust_nodes(case: &ParseCase) -> Value {
    let now = UNIX_EPOCH + Duration::from_millis(case.now);
    serde_json::to_value(parse_at(&case.input, &case.options.to_options(), now))
        .expect("AST serializes")
}

fn ascii_json(case: &ParseCase) -> String {
    let json = serde_json::to_string(case).expect("parse case serializes");
    json.chars()
        .map(|character| {
            if character.is_ascii() {
                return character.to_string();
            }
            character
                .encode_utf16(&mut [0; 2])
                .iter()
                .map(|unit| format!("\\u{unit:04x}"))
                .collect()
        })
        .collect()
}

fn compare(case: &ParseCase) -> Result<(), String> {
    let pasteable_case = ascii_json(case);
    let upstream = oracle()
        .parse(case)
        .map_err(|reason| format!("{pasteable_case}\n  {reason}"))?;
    let rust = rust_nodes(case);
    if js_equal(&rust, &upstream) {
        return Ok(());
    }
    Err(format!(
        "{pasteable_case}\n  AST mismatch\n  upstream: {}\n  rust:     {}",
        pretty(&upstream),
        pretty(&rust)
    ))
}

fn milliseconds_since_epoch(instant: SystemTime) -> u64 {
    let elapsed = instant
        .duration_since(UNIX_EPOCH)
        .expect("fixture clocks are after the epoch");
    u64::try_from(elapsed.as_millis()).expect("fixture clocks fit in u64 milliseconds")
}

fn assert_no_mismatches(mismatches: Vec<String>, checked: usize) {
    assert!(
        mismatches.is_empty(),
        "{} of {checked} input(s) differ from upstream:\n\n{}",
        mismatches.len(),
        mismatches.join("\n\n")
    );
}

#[test]
fn upstream_test_suite_inputs_produce_the_same_ast() {
    let mut mismatches = Vec::new();
    let mut checked = 0;
    let mut fixture_paths: Vec<_> = std::fs::read_dir(FIXTURE_DIRECTORY)
        .expect("fixture directory exists")
        .map(|entry| entry.expect("fixture directory entry is readable").path())
        .collect();
    fixture_paths.sort();
    for path in fixture_paths {
        let source = std::fs::read_to_string(&path).expect("fixture is readable");
        let fixture: Fixture =
            serde_json::from_str(&source).expect("fixture JSON matches the extractor's schema");
        for group in &fixture.groups {
            let now = group
                .needs_clock
                .as_deref()
                .map_or(FIXTURE_CLOCK_MILLISECONDS, |clock| {
                    milliseconds_since_epoch(clock_at(clock))
                });
            for case in group.cases.iter().filter(|case| !case.input.is_empty()) {
                checked += 1;
                let parse_case = ParseCase {
                    input: case.input.clone(),
                    options: case.options.clone(),
                    now,
                };
                if let Err(reason) = compare(&parse_case) {
                    mismatches.push(format!("[{}] {}\n  {reason}", fixture.file, case.name));
                }
            }
        }
    }
    assert_no_mismatches(mismatches, checked);
}

#[test]
fn regression_inputs_produce_the_same_ast() {
    let regressions: Vec<ParseCase> =
        serde_json::from_str(include_str!("fixtures/differential/regressions.json"))
            .expect("regressions.json lists input, options and now for each case");
    let mismatches: Vec<String> = regressions
        .iter()
        .filter_map(|regression| compare(regression).err())
        .collect();
    assert_no_mismatches(mismatches, regressions.len());
}

#[derive(Deserialize)]
struct EmojiFixture {
    emoji: Vec<String>,
}

#[test]
fn every_emoji_produces_the_same_ast() {
    let fixture: EmojiFixture = serde_json::from_str(include_str!("fixtures/emoji/all-emoji.json"))
        .expect("all-emoji.json lists emoji");
    let mismatches: Vec<String> = fixture
        .emoji
        .iter()
        .flat_map(|emoji| [emoji.clone(), format!("a {emoji} b")])
        .map(|input| ParseCase {
            input,
            options: JsOptions::default(),
            now: FIXTURE_CLOCK_MILLISECONDS,
        })
        .filter_map(|case| compare(&case).err())
        .collect();
    assert_no_mismatches(mismatches, fixture.emoji.len() * 2);
}

const FRAGMENTS: &[&str] = &[
    "*",
    "**",
    "_",
    "__",
    "~",
    "~~",
    "`",
    "```",
    "|",
    "||",
    ">",
    "#",
    "##",
    "-",
    "+",
    ".",
    ":",
    "$",
    "$$",
    "\\",
    "[",
    "]",
    "(",
    ")",
    "<",
    "!",
    "=",
    "%",
    "&",
    "'",
    "\"",
    ",",
    ";",
    "?",
    "/",
    " ",
    "  ",
    "\t",
    "\n",
    "\n\n",
    "\r\n",
    "\r",
    "\u{85}",
    "\u{2028}",
    "\u{2029}",
    "\u{a0}",
    "word",
    "a",
    "B9",
    "x_y",
    "über",
    "日本",
    "0",
    "12",
    "3.",
    "https://rocket.chat",
    "https://example.com/path?query=1#hash",
    "http://a.b/c_d",
    "www.rocket.chat",
    "rocket.chat",
    "go.dev",
    "host.local",
    "a.b.c",
    "ftp://files.example.org",
    "user@example.com",
    "mailto:user@example.com",
    "[label](https://rocket.chat)",
    "![image](https://a.co/x.png)",
    "<https://rocket.chat|label>",
    "<https://rocket.chat>",
    "+5511999990000",
    "+1 (555) 123-4567",
    "@user",
    "@user.name",
    "@all",
    "@here",
    "#channel",
    "#general-chat",
    ":smile:",
    ":+1:",
    ":flag_br:",
    ":not_an_emoji:",
    "😀",
    "👍🏽",
    "🇧🇷",
    "❤️",
    "👨‍👩‍👧",
    ":)",
    ":-)",
    ":D",
    ";)",
    ":P",
    ":(",
    "<3",
    "D:",
    ":'(",
    "B)",
    "O:)",
    ":smile:)",
    "$x^2$",
    "$$\\frac{a}{b}$$",
    "\\(a+b\\)",
    "\\[c\\]",
    "\\(",
    "\\)",
    "\\[",
    "\\]",
    "color:#fff",
    "color:#ff0000",
    "color:#12345678",
    "color:#zzz",
    "color:red",
    "<t:1700000000>",
    "<t:1700000000:R>",
    "<t:10:00>",
    "<t:10:00:00+01:00>",
    "<t:2025-03-09T02:30:00>",
    "<t:2025-11-02T01:30:00Z:F>",
    "<t:abc>",
    "- ",
    "* ",
    "1. ",
    "2) ",
    "- [ ] ",
    "- [x] ",
    "> ",
    "# ",
    "### ",
    "---",
    "***",
    "| a | b |",
    "|---|---|",
    "```js\ncode\n```",
    "`code`",
    "||spoiler||",
    "~~strike~~",
    "*bold*",
    "_italic_",
    "**strong**",
    "__under__",
];

fn fragment() -> impl Strategy<Value = String> {
    prop_oneof![
        8 => proptest::sample::select(FRAGMENTS).prop_map(str::to_owned),
        1 => proptest::string::string_regex("[a-z0-9 ]{1,6}").expect("word pattern is a valid regex"),
        1 => any::<char>().prop_map(String::from),
    ]
}

fn message() -> impl Strategy<Value = String> {
    proptest::collection::vec(fragment(), 1..=16).prop_map(|fragments| fragments.concat())
}

fn katex_options() -> impl Strategy<Value = Option<JsKatexOptions>> {
    proptest::option::of(
        (
            proptest::option::of(any::<bool>()),
            proptest::option::of(any::<bool>()),
        )
            .prop_map(|(dollar_syntax, parenthesis_syntax)| JsKatexOptions {
                dollar_syntax,
                parenthesis_syntax,
            }),
    )
}

fn custom_domains() -> impl Strategy<Value = Option<Vec<String>>> {
    proptest::option::of(proptest::sample::subsequence(
        vec![
            "local".to_owned(),
            "chat".to_owned(),
            "rocket.local".to_owned(),
        ],
        0..=3,
    ))
}

fn options() -> impl Strategy<Value = JsOptions> {
    (
        proptest::option::of(any::<bool>()),
        proptest::option::of(any::<bool>()),
        katex_options(),
        custom_domains(),
    )
        .prop_map(|(colors, emoticons, katex, custom_domains)| JsOptions {
            colors,
            emoticons,
            katex,
            custom_domains,
        })
}

fn parse_case() -> impl Strategy<Value = ParseCase> {
    (message(), options(), 0u64..4_102_444_800_000).prop_map(|(input, options, now)| ParseCase {
        input,
        options,
        now,
    })
}

fn generated_cases() -> u32 {
    std::env::var("PROPTEST_CASES")
        .ok()
        .map(|cases| cases.parse().expect("PROPTEST_CASES is a number"))
        .unwrap_or(DEFAULT_GENERATED_CASES)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(generated_cases()))]

    #[test]
    fn generated_messages_produce_the_same_ast(case in parse_case()) {
        compare(&case).map_err(TestCaseError::fail)?;
    }
}
