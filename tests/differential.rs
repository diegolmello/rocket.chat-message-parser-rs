use std::collections::BTreeSet;
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::panic::{self, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use message_parser::{parse_at, Options};
use proptest::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

mod common;

use common::{
    canonical, clock_at, fixed_clock, js_equal, panic_message, pretty, strings_over, Fixture,
    JsKatexOptions, JsOptions, CLOCK_SECONDS,
};

const UPSTREAM_DIRECTORY: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/upstream");
const FIXTURE_DIRECTORY: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/upstream");
const DEFAULT_GENERATED_CASES: u32 = 20_000;
const CLOCK_MILLISECONDS: u64 = CLOCK_SECONDS * 1000;

struct Oracle {
    process: Child,
    requests: BufWriter<ChildStdin>,
    responses: BufReader<ChildStdout>,
}

#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
enum OracleResponse {
    Nodes(Value),
    Error(String),
}

impl OracleResponse {
    fn into_result(self) -> Result<Value, String> {
        match self {
            OracleResponse::Nodes(nodes) => Ok(nodes),
            OracleResponse::Error(error) => Err(format!("upstream threw: {error}")),
        }
    }
}

fn read_response(line: &str) -> Result<Value, String> {
    serde_json::from_str::<OracleResponse>(line)
        .expect("oracle responds with one JSON object per line")
        .into_result()
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(deny_unknown_fields)]
struct ParseCase {
    input: String,
    options: JsOptions,
    now: u64,
}

fn installed_upstream_directory() -> PathBuf {
    std::env::var_os("UPSTREAM_PARSER_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(UPSTREAM_DIRECTORY))
}

impl Oracle {
    fn start() -> Oracle {
        let installed = installed_upstream_directory();
        assert!(
            installed
                .join("node_modules/@rocket.chat/message-parser")
                .is_dir(),
            "install the pinned upstream parser first: `bun install --frozen-lockfile` in {} \
             (or set UPSTREAM_PARSER_DIR to a copy of {UPSTREAM_DIRECTORY} where it is installed)",
            installed.display()
        );
        let mut process = Command::new("node")
            .arg(Path::new(UPSTREAM_DIRECTORY).join("oracle.mjs"))
            .arg(&installed)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .expect("node is on PATH");
        let requests = BufWriter::new(process.stdin.take().expect("oracle stdin is piped"));
        let responses = BufReader::new(process.stdout.take().expect("oracle stdout is piped"));
        Oracle {
            process,
            requests,
            responses,
        }
    }

    fn parse(&mut self, request: &str) -> Result<Value, String> {
        let mut line = String::new();
        let exchanged = writeln!(self.requests, "{request}")
            .and_then(|()| self.requests.flush())
            .and_then(|()| self.responses.read_line(&mut line));
        if !matches!(exchanged, Ok(length) if length > 0) {
            let _ = self.process.kill();
            let status = self.process.wait().expect("oracle process can be awaited");
            *self = Oracle::start();
            return Err(format!("upstream oracle exited ({status})"));
        }
        read_response(&line)
    }

    fn parse_all(&mut self, requests: &[String]) -> Vec<Result<Value, String>> {
        let mut answers = Vec::with_capacity(requests.len());
        let Oracle {
            requests: writer,
            responses,
            ..
        } = self;
        std::thread::scope(|scope| {
            scope.spawn(|| {
                for request in requests {
                    writeln!(writer, "{request}").expect("oracle accepts input");
                }
                writer.flush().expect("oracle accepts input");
            });
            for _ in requests {
                let mut line = String::new();
                responses.read_line(&mut line).expect("oracle answers");
                assert!(!line.is_empty(), "oracle exited early");
                answers.push(read_response(&line));
            }
        });
        answers
    }
}

impl Drop for Oracle {
    fn drop(&mut self) {
        let _ = self.process.kill();
        let _ = self.process.wait();
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

fn printable_json(case: &ParseCase) -> String {
    let json = serde_json::to_string(case).expect("parse case serializes");
    let mut printable = String::with_capacity(json.len());
    for character in json.chars() {
        if (' '..='~').contains(&character) {
            printable.push(character);
            continue;
        }
        for unit in character.encode_utf16(&mut [0; 2]) {
            printable.push_str(&format!("\\u{unit:04x}"));
        }
    }
    printable
}

fn compare(case: &ParseCase) -> Result<(), String> {
    let request = printable_json(case);
    let upstream = oracle()
        .parse(&request)
        .map_err(|reason| format!("{request}\n  {reason}"))?;
    let rust = rust_nodes(case);
    if js_equal(&rust, &upstream) {
        return Ok(());
    }
    Err(format!(
        "{request}\n  AST mismatch\n  upstream: {}\n  rust:     {}",
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
                .map_or(CLOCK_MILLISECONDS, |clock| {
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
            now: CLOCK_MILLISECONDS,
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

const DEFAULT_RANDOM_CASES: usize = 20_000;
const DEFAULT_SEED: u64 = 0x5eed_0033;
const MINIMIZED_FAILURE_LIMIT: usize = 400;
const PRINTED_FAILURE_LIMIT: usize = 60;

const WIDE_ALPHABET: &[&str] = &[
    "*", "_", "~", "[", "]", "(", ")", "$", ":", "`", "<", ">", "|", "\\", "#", "-", ".", "@", "!",
    "\n", " ", "a", "1",
];
const WIDE_LENGTH: usize = 3;

const CORE_ALPHABET: &[&str] = &[
    "*", "_", "~", "[", "]", "(", ")", "$", ":", "`", "\n", " ", "a",
];
const CORE_LENGTH: usize = 4;

const TOKENS: &[&str] = &[
    "*",
    "_",
    "~",
    "~~",
    "`",
    "```",
    "||",
    "[",
    "](",
    ")",
    "![",
    "$",
    "$$",
    "\\(",
    "\\)",
    "\\[",
    "\\]",
    ":",
    ":)",
    ":smile:",
    "<t:1753142400:R>",
    "<t:2025-03-09T02:30:00>",
    "#",
    "# ",
    "- ",
    "* ",
    "1. ",
    "- [ ] ",
    "- [x] ",
    "> ",
    "| a |",
    "\n",
    " ",
    "a",
    "b.com",
    "http://a.b",
    "https://rocket.chat/a?b=1#c",
    "a@b.co",
    "@a",
    "#a",
    "color:#f00",
    "+5511999999999",
    "😀",
    "🇧",
    "\\*",
    "__",
    "**",
    "||\n",
    "||\r\n",
    "\n||",
    "a\nb",
    ";-)",
    "*\\0/*",
    "````\n",
    "\n````",
    "`````",
    "<t:2038-01-19T03:14:08+00:00>",
    "<t:9999-12-31T23:59:59.999>",
    "<t:1800-06-15T12:00:00.500-03:00>",
    "a+b://c.co",
    "http://a.b:99999",
    "abcdefghijklmnopqrstuvwxyz012345://b.com",
    "abcdefghijklmnopqrstuvwxyz0123456://b.com",
    "abcdefghijklmnopqrstuvwxyz01.com://b.com:99999",
    "abcdefghijklmnopqrstuvwxyz012.com://b.com:99999",
    ":99999",
    "\r",
    "||:)",
    "||-___-",
    "||*a*",
    "<t:12:00>",
    "<t:12:34:56+01:00>",
    "<t:2025-13-45T25:61:61>",
    "<t:2025-07-22T10:00:00+24:00>",
    "<t:2025-07-22T24:00:00.000-03:00>",
    "x.local",
    "X.LOCAL",
    "http://x.Local/a",
    "http://localhost",
    "@.:]",
];
const FAST_TOKEN_LENGTH: u64 = 2;
const RANDOM_TOKEN_MAX: usize = 12;

struct OptionSet {
    name: &'static str,
    upstream: Value,
    rust: Options,
}

fn option_sets() -> Vec<OptionSet> {
    let custom_domains = ["a", "local", ".", ""];
    [
        ("default", json!({})),
        (
            "explicit defaults",
            json!({
                "emoticons": false,
                "colors": false,
                "katex": { "dollarSyntax": false, "parenthesisSyntax": false },
                "customDomains": [],
            }),
        ),
        ("emoticons", json!({ "emoticons": true })),
        ("colors", json!({ "colors": true })),
        ("katex dollar", json!({ "katex": { "dollarSyntax": true } })),
        (
            "katex parenthesis",
            json!({ "katex": { "parenthesisSyntax": true } }),
        ),
        ("customDomains", json!({ "customDomains": custom_domains })),
        (
            "customDomains near misses",
            json!({ "customDomains": ["LOCAL", "ocal", "chat", "", "."] }),
        ),
        (
            "all",
            json!({
                "emoticons": true,
                "colors": true,
                "katex": { "dollarSyntax": true, "parenthesisSyntax": true },
                "customDomains": custom_domains,
            }),
        ),
    ]
    .into_iter()
    .map(|(name, upstream)| OptionSet {
        name,
        rust: serde_json::from_value(upstream.clone()).expect("upstream options deserialize"),
        upstream,
    })
    .collect()
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Outcome {
    Ast(String),
    Failed(String),
}

impl Outcome {
    fn ast(value: &Value) -> Self {
        Outcome::Ast(canonical(value).to_string())
    }
}

impl From<Result<Value, String>> for Outcome {
    fn from(answer: Result<Value, String>) -> Self {
        match answer {
            Ok(nodes) => Outcome::ast(&nodes),
            Err(reason) => Outcome::Failed(reason),
        }
    }
}

fn upstream_request(input: &str, options: &Value) -> String {
    json!({ "input": input, "options": options, "now": CLOCK_MILLISECONDS }).to_string()
}

fn rust_outcome(input: &str, options: &Options) -> Outcome {
    match panic::catch_unwind(AssertUnwindSafe(|| parse_at(input, options, fixed_clock()))) {
        Ok(nodes) => Outcome::ast(&serde_json::to_value(&nodes).expect("AST serializes")),
        Err(payload) => Outcome::Failed(format!("panic: {}", panic_message(payload.as_ref()))),
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Mismatch {
    input: String,
    options: &'static str,
    upstream: Outcome,
    rust: Outcome,
}

fn mismatch(
    oracle: &mut Oracle,
    input: &str,
    option_set: &OptionSet,
) -> Option<(Outcome, Outcome)> {
    let upstream = oracle
        .parse(&upstream_request(input, &option_set.upstream))
        .into();
    let rust = rust_outcome(input, &option_set.rust);
    (upstream != rust).then_some((upstream, rust))
}

fn character_boundaries(input: &str) -> Vec<usize> {
    input
        .char_indices()
        .map(|(index, _)| index)
        .chain([input.len()])
        .collect()
}

fn minimize(oracle: &mut Oracle, input: &str, option_set: &OptionSet) -> Mismatch {
    let mut smallest = input.to_owned();
    let mut chunk = smallest.chars().count().div_ceil(2).max(1);
    loop {
        let boundaries = character_boundaries(&smallest);
        let characters = boundaries.len() - 1;
        let mut shrunk = false;
        let mut start = 0;
        while start + chunk <= characters && characters > 1 {
            let candidate = format!(
                "{}{}",
                &smallest[..boundaries[start]],
                &smallest[boundaries[start + chunk]..]
            );
            if !candidate.is_empty() && mismatch(oracle, &candidate, option_set).is_some() {
                smallest = candidate;
                shrunk = true;
                break;
            }
            start += 1;
        }
        if shrunk {
            continue;
        }
        if chunk == 1 {
            break;
        }
        chunk /= 2;
    }
    let (upstream, rust) =
        mismatch(oracle, &smallest, option_set).expect("minimized input still mismatches");
    Mismatch {
        input: smallest,
        options: option_set.name,
        upstream,
        rust,
    }
}

struct SplitMix(u64);

impl SplitMix {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut mixed = self.0;
        mixed = (mixed ^ (mixed >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        mixed = (mixed ^ (mixed >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        mixed ^ (mixed >> 31)
    }

    fn below(&mut self, bound: usize) -> usize {
        (self.next() % bound as u64) as usize
    }
}

fn random_inputs(count: usize, seed: u64) -> Vec<String> {
    let mut random = SplitMix(seed);
    (0..count)
        .map(|_| {
            let length = 1 + random.below(RANDOM_TOKEN_MAX);
            (0..length)
                .map(|_| {
                    if random.below(4) == 0 {
                        WIDE_ALPHABET[random.below(WIDE_ALPHABET.len())].to_owned()
                    } else {
                        TOKENS[random.below(TOKENS.len())].to_owned()
                    }
                })
                .collect()
        })
        .collect()
}

fn environment_number(name: &str, default: u64) -> u64 {
    std::env::var(name)
        .ok()
        .map(|value| {
            value
                .parse()
                .unwrap_or_else(|_| panic!("{name} must be a number, got {value:?}"))
        })
        .unwrap_or(default)
}

fn show(outcome: &Outcome) -> &str {
    match outcome {
        Outcome::Ast(json) | Outcome::Failed(json) => json,
    }
}

fn oracle_count() -> usize {
    let cores = std::thread::available_parallelism().map_or(1, |cores| cores.get());
    environment_number("DIFFERENTIAL_JOBS", cores as u64).max(1) as usize
}

fn failing_cases<'sets>(
    inputs: &[String],
    option_sets: &'sets [OptionSet],
) -> Vec<(String, &'sets OptionSet)> {
    let pairs: Vec<(&str, &OptionSet)> = option_sets
        .iter()
        .flat_map(|option_set| inputs.iter().map(move |input| (input.as_str(), option_set)))
        .collect();
    let share = pairs.len().div_ceil(oracle_count()).max(1);
    std::thread::scope(|scope| {
        let workers: Vec<_> = pairs
            .chunks(share)
            .map(|share| {
                scope.spawn(move || {
                    let requests: Vec<String> = share
                        .iter()
                        .map(|(input, option_set)| upstream_request(input, &option_set.upstream))
                        .collect();
                    let upstream = Oracle::start().parse_all(&requests);
                    share
                        .iter()
                        .zip(upstream)
                        .filter(|((input, option_set), upstream)| {
                            Outcome::from(upstream.clone()) != rust_outcome(input, &option_set.rust)
                        })
                        .map(|((input, option_set), _)| ((*input).to_owned(), *option_set))
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        workers
            .into_iter()
            .flat_map(|worker| {
                worker
                    .join()
                    .unwrap_or_else(|payload| panic::resume_unwind(payload))
            })
            .collect()
    })
}

fn check_equivalence(inputs: &[String]) {
    let option_sets = option_sets();
    let mut failing_cases = failing_cases(inputs, &option_sets);
    if failing_cases.is_empty() {
        println!(
            "{} inputs x {} option sets: every AST matches upstream",
            inputs.len(),
            option_sets.len()
        );
        return;
    }
    failing_cases.sort_by_key(|(input, _)| input.len());
    let mut oracle = Oracle::start();
    let minimized: BTreeSet<Mismatch> = failing_cases
        .iter()
        .take(MINIMIZED_FAILURE_LIMIT)
        .map(|(input, option_set)| minimize(&mut oracle, input, option_set))
        .collect();
    let mut by_size: Vec<&Mismatch> = minimized.iter().collect();
    by_size.sort_by_key(|mismatch| (mismatch.input.chars().count(), mismatch.input.clone()));
    let report: Vec<String> = by_size
        .iter()
        .take(PRINTED_FAILURE_LIMIT)
        .map(|mismatch| {
            format!(
                "input:    {:?}\noptions:  {}\nupstream: {}\nrust:     {}",
                mismatch.input,
                mismatch.options,
                show(&mismatch.upstream),
                show(&mismatch.rust)
            )
        })
        .collect();
    panic!(
        "{} of {} (input, option set) pairs differ from upstream; the first {} minimize to {} distinct cases:\n\n{}",
        failing_cases.len(),
        inputs.len() * option_sets.len(),
        failing_cases.len().min(MINIMIZED_FAILURE_LIMIT),
        minimized.len(),
        report.join("\n\n")
    );
}

#[test]
#[ignore = "needs node and the pinned upstream parser; see .claude/skills/verify-upstream-equivalence"]
fn every_short_character_string_matches_upstream() {
    let mut inputs = strings_over(WIDE_ALPHABET, 1..=WIDE_LENGTH);
    inputs.extend(strings_over(CORE_ALPHABET, CORE_LENGTH..=CORE_LENGTH));
    check_equivalence(&inputs);
}

#[test]
#[ignore = "needs node and the pinned upstream parser; see .claude/skills/verify-upstream-equivalence"]
fn every_short_token_sequence_matches_upstream() {
    let length = environment_number("DIFFERENTIAL_TOKEN_LENGTH", FAST_TOKEN_LENGTH) as usize;
    check_equivalence(&strings_over(TOKENS, 1..=length));
}

#[test]
#[ignore = "needs node and the pinned upstream parser; see .claude/skills/verify-upstream-equivalence"]
fn random_token_sequences_match_upstream() {
    let count = environment_number("DIFFERENTIAL_CASES", DEFAULT_RANDOM_CASES as u64) as usize;
    let seed = environment_number("DIFFERENTIAL_SEED", DEFAULT_SEED);
    check_equivalence(&random_inputs(count, seed));
}
