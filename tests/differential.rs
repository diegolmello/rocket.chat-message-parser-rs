use std::collections::BTreeSet;
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::panic::{self, AssertUnwindSafe};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use message_parser::{parse_at, Options};
use serde_json::{json, Value};

mod common;

use common::{canonical, fixed_clock, panic_message, strings_over, CLOCK_SECONDS};

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

fn rust_outcome(input: &str, options: &Options) -> Outcome {
    match panic::catch_unwind(AssertUnwindSafe(|| parse_at(input, options, fixed_clock()))) {
        Ok(nodes) => Outcome::ast(&serde_json::to_value(&nodes).expect("AST serializes")),
        Err(payload) => Outcome::Failed(format!("panic: {}", panic_message(payload.as_ref()))),
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
                    let cases: Vec<(&str, &Value)> = share
                        .iter()
                        .map(|(input, option_set)| (*input, &option_set.upstream))
                        .collect();
                    let upstream = Oracle::start().ask(&cases);
                    share
                        .iter()
                        .zip(upstream)
                        .filter(|((input, option_set), upstream)| {
                            *upstream != rust_outcome(input, &option_set.rust)
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
