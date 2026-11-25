use std::time::{Duration, Instant, SystemTime};

use message_parser::{parse_with_report, KatexOptions, Options};
use serde_json::{json, Value};

const SCENARIOS: &str = include_str!("../benchmarks/scenarios.json");

struct Scenario {
    length: usize,
    label: String,
    options: Options,
}

struct Measurement {
    median: Duration,
    work: u64,
}

fn flag(options: &Value, path: &[&str]) -> bool {
    path.iter()
        .try_fold(options, |value, key| value.get(key))
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

fn options_from(options: &Value) -> Options {
    Options {
        colors: flag(options, &["colors"]),
        emoticons: flag(options, &["emoticons"]),
        katex: KatexOptions {
            dollar_syntax: flag(options, &["katex", "dollarSyntax"]),
            parenthesis_syntax: flag(options, &["katex", "parenthesisSyntax"]),
        },
        custom_domains: options
            .get("customDomains")
            .and_then(Value::as_array)
            .map(|domains| {
                domains
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect()
            }),
    }
}

fn load_scenarios() -> (String, usize, Vec<Scenario>) {
    let definition: Value = serde_json::from_str(SCENARIOS).expect("scenarios.json is valid JSON");
    let pattern = definition["pattern"]
        .as_str()
        .expect("pattern is a string")
        .to_owned();
    let samples = definition["samples"].as_u64().expect("samples is a number") as usize;
    let scenarios = definition["scenarios"]
        .as_array()
        .expect("scenarios is a list")
        .iter()
        .map(|scenario| Scenario {
            length: scenario["length"].as_u64().expect("length is a number") as usize,
            label: scenario["label"]
                .as_str()
                .expect("label is a string")
                .to_owned(),
            options: options_from(&scenario["options"]),
        })
        .collect();
    (pattern, samples, scenarios)
}

fn markup_soup(pattern: &str, length: usize) -> String {
    pattern.chars().cycle().take(length).collect()
}

fn median(mut durations: Vec<Duration>) -> Duration {
    durations.sort();
    durations[durations.len() / 2]
}

fn measure(input: &str, options: &Options, samples: usize, now: SystemTime) -> Measurement {
    let mut durations = Vec::new();
    let mut work = 0;
    for _ in 0..samples {
        let started = Instant::now();
        let outcome = parse_with_report(input, options, now);
        durations.push(started.elapsed());
        work = outcome.report.work();
    }
    Measurement {
        median: median(durations),
        work,
    }
}

fn main() {
    let as_json = std::env::args().any(|argument| argument == "--json");
    let (pattern, samples, scenarios) = load_scenarios();
    let now = SystemTime::now();
    let measurements: Vec<Measurement> = scenarios
        .iter()
        .map(|scenario| {
            let input = markup_soup(&pattern, scenario.length);
            measure(&input, &scenario.options, samples, now)
        })
        .collect();
    if as_json {
        let rows: Vec<Value> = scenarios
            .iter()
            .zip(&measurements)
            .map(|(scenario, measurement)| {
                json!({
                    "length": scenario.length,
                    "label": scenario.label,
                    "medianMilliseconds": measurement.median.as_secs_f64() * 1000.0,
                    "work": measurement.work,
                })
            })
            .collect();
        println!("{}", Value::Array(rows));
        return;
    }
    println!(
        "{:>7} {:>10} {:>12} {:>11} {:>14}",
        "length", "options", "median", "work", "work per char"
    );
    for (scenario, measurement) in scenarios.iter().zip(&measurements) {
        println!(
            "{:>7} {:>10} {:>12?} {:>11} {:>14.1}",
            scenario.length,
            scenario.label,
            measurement.median,
            measurement.work,
            measurement.work as f64 / scenario.length as f64
        );
    }
}
