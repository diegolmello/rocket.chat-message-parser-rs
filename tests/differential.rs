use std::collections::BTreeSet;
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::panic::{self, AssertUnwindSafe};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use message_parser::{parse_at, KatexOptions, Options};
use serde_json::{json, Value};

const CLOCK_SECONDS: u64 = 1_753_142_400;
const DEFAULT_RANDOM_CASES: usize = 20_000;
const DEFAULT_SEED: u64 = 0x5eed_0033;
const MINIMIZED_FAILURE_LIMIT: usize = 400;
const PRINTED_FAILURE_LIMIT: usize = 60;

const WIDE_ALPHABET: &[char] = &[
    '*', '_', '~', '[', ']', '(', ')', '$', ':', '`', '<', '>', '|', '\\', '#', '-', '.', '@', '!',
    '\n', ' ', 'a', '1',
];
const WIDE_LENGTH: u32 = 3;

const CORE_ALPHABET: &[char] = &[
    '*', '_', '~', '[', ']', '(', ')', '$', ':', '`', '\n', ' ', 'a',
];
const CORE_LENGTH: u32 = 4;

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
];
const TOKEN_LENGTH: u32 = 3;
const RANDOM_TOKEN_MAX: usize = 12;

struct OptionSet {
    name: &'static str,
    upstream: Value,
    rust: Options,
}

fn option_sets() -> Vec<OptionSet> {
    let katex = |dollar_syntax, parenthesis_syntax| KatexOptions {
        dollar_syntax,
        parenthesis_syntax,
    };
    let custom_domains = vec!["a".to_owned(), "local".to_owned()];
    vec![
        OptionSet {
            name: "default",
            upstream: json!({}),
            rust: Options::default(),
        },
        OptionSet {
            name: "emoticons",
            upstream: json!({ "emoticons": true }),
            rust: Options {
                emoticons: true,
                ..Options::default()
            },
        },
        OptionSet {
            name: "colors",
            upstream: json!({ "colors": true }),
            rust: Options {
                colors: true,
                ..Options::default()
            },
        },
        OptionSet {
            name: "katex dollar",
            upstream: json!({ "katex": { "dollarSyntax": true } }),
            rust: Options {
                katex: katex(true, false),
                ..Options::default()
            },
        },
        OptionSet {
            name: "katex parenthesis",
            upstream: json!({ "katex": { "parenthesisSyntax": true } }),
            rust: Options {
                katex: katex(false, true),
                ..Options::default()
            },
        },
        OptionSet {
            name: "customDomains",
            upstream: json!({ "customDomains": custom_domains }),
            rust: Options {
                custom_domains: Some(custom_domains.clone()),
                ..Options::default()
            },
        },
        OptionSet {
            name: "all",
            upstream: json!({
                "emoticons": true,
                "colors": true,
                "katex": { "dollarSyntax": true, "parenthesisSyntax": true },
                "customDomains": custom_domains,
            }),
            rust: Options {
                colors: true,
                emoticons: true,
                katex: katex(true, true),
                custom_domains: Some(custom_domains.clone()),
            },
        },
    ]
}

fn clock() -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(CLOCK_SECONDS)
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

fn canonical(value: &Value) -> Value {
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

fn rust_outcome(input: &str, options: &Options) -> Outcome {
    match panic::catch_unwind(AssertUnwindSafe(|| parse_at(input, options, clock()))) {
        Ok(nodes) => Outcome::ast(&serde_json::to_value(&nodes).expect("AST serializes")),
        Err(payload) => Outcome::Failed(format!(
            "panic: {}",
            payload
                .downcast_ref::<&str>()
                .map(|message| (*message).to_owned())
                .or_else(|| payload.downcast_ref::<String>().cloned())
                .unwrap_or_default()
        )),
    }
}

struct Oracle {
    child: Child,
    requests: BufWriter<ChildStdin>,
    answers: BufReader<ChildStdout>,
}

impl Oracle {
    fn start() -> Self {
        let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let upstream_directory = std::env::var_os("UPSTREAM_PARSER_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| manifest.join("benchmarks/upstream"));
        assert!(
            upstream_directory
                .join("node_modules/@rocket.chat/message-parser")
                .is_dir(),
            "install the pinned upstream parser first: `bun install --frozen-lockfile` in {} \
             (or set UPSTREAM_PARSER_DIR to a copy of benchmarks/upstream where it is installed)",
            upstream_directory.display()
        );
        let mut child = Command::new("node")
            .arg(manifest.join("tests/differential/oracle.mjs"))
            .arg(&upstream_directory)
            .arg((CLOCK_SECONDS * 1000).to_string())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("node is on PATH");
        let requests = BufWriter::new(child.stdin.take().expect("piped stdin"));
        let answers = BufReader::new(child.stdout.take().expect("piped stdout"));
        Oracle {
            child,
            requests,
            answers,
        }
    }

    fn ask(&mut self, cases: &[(&str, &Value)]) -> Vec<Outcome> {
        let mut outcomes = Vec::with_capacity(cases.len());
        let Oracle {
            requests, answers, ..
        } = self;
        std::thread::scope(|scope| {
            scope.spawn(|| {
                for (input, options) in cases {
                    let request = json!({ "input": input, "options": options });
                    writeln!(requests, "{request}").expect("oracle accepts input");
                }
                requests.flush().expect("oracle accepts input");
            });
            for _ in cases {
                let mut line = String::new();
                answers.read_line(&mut line).expect("oracle answers");
                assert!(!line.is_empty(), "oracle exited early");
                let answer: Value = serde_json::from_str(&line).expect("oracle answers JSON");
                outcomes.push(match answer.get("ast") {
                    Some(ast) => Outcome::ast(ast),
                    None => Outcome::Failed(format!("throws: {}", answer["error"])),
                });
            }
        });
        outcomes
    }
}

impl Drop for Oracle {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
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
    let upstream = oracle.ask(&[(input, &option_set.upstream)]).remove(0);
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

fn strings_up_to(alphabet: &[char], max_length: u32) -> Vec<String> {
    let mut all = Vec::new();
    let mut layer = vec![String::new()];
    for _ in 0..max_length {
        layer = layer
            .iter()
            .flat_map(|prefix| {
                alphabet.iter().map(move |character| {
                    let mut next = prefix.clone();
                    next.push(*character);
                    next
                })
            })
            .collect();
        all.extend(layer.iter().cloned());
    }
    all
}

fn token_sequences_up_to(tokens: &[&str], max_length: u32) -> Vec<String> {
    let mut all = BTreeSet::new();
    let mut layer = vec![String::new()];
    for _ in 0..max_length {
        layer = layer
            .iter()
            .flat_map(|prefix| tokens.iter().map(move |token| format!("{prefix}{token}")))
            .collect();
        all.extend(layer.iter().cloned());
    }
    all.into_iter().collect()
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
                        WIDE_ALPHABET[random.below(WIDE_ALPHABET.len())].to_string()
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

fn check_equivalence(inputs: &[String]) {
    let option_sets = option_sets();
    let mut oracle = Oracle::start();
    let mut failing_cases = Vec::new();
    for option_set in &option_sets {
        let cases: Vec<(&str, &Value)> = inputs
            .iter()
            .map(|input| (input.as_str(), &option_set.upstream))
            .collect();
        let upstream = oracle.ask(&cases);
        for (input, upstream) in inputs.iter().zip(upstream) {
            if upstream != rust_outcome(input, &option_set.rust) {
                failing_cases.push((input.clone(), option_set));
            }
        }
    }
    if failing_cases.is_empty() {
        println!(
            "{} inputs x {} option sets: every AST matches upstream",
            inputs.len(),
            option_sets.len()
        );
        return;
    }
    failing_cases.sort_by_key(|(input, _)| input.len());
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
    let mut inputs = strings_up_to(WIDE_ALPHABET, WIDE_LENGTH);
    inputs.extend(
        strings_up_to(CORE_ALPHABET, CORE_LENGTH)
            .into_iter()
            .filter(|input| input.chars().count() == CORE_LENGTH as usize),
    );
    check_equivalence(&inputs);
}

#[test]
#[ignore = "needs node and the pinned upstream parser; see .claude/skills/verify-upstream-equivalence"]
fn every_short_token_sequence_matches_upstream() {
    check_equivalence(&token_sequences_up_to(TOKENS, TOKEN_LENGTH));
}

#[test]
#[ignore = "needs node and the pinned upstream parser; see .claude/skills/verify-upstream-equivalence"]
fn random_token_sequences_match_upstream() {
    let count = environment_number("DIFFERENTIAL_CASES", DEFAULT_RANDOM_CASES as u64) as usize;
    let seed = environment_number("DIFFERENTIAL_SEED", DEFAULT_SEED);
    check_equivalence(&random_inputs(count, seed));
}
