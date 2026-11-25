use std::time::{Duration, Instant, SystemTime};

use message_parser::{parse_with_report, Options};
use serde::Deserialize;
use serde_json::{json, Value};

const SCENARIOS: &str = include_str!("../benchmarks/scenarios.json");

#[derive(Deserialize)]
struct Scenarios {
    pattern: String,
    samples: usize,
    scenarios: Vec<Scenario>,
}

#[derive(Deserialize)]
struct Scenario {
    length: usize,
    label: String,
    options: Options,
}

struct Measurement {
    median: Duration,
    work: u64,
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
    let Scenarios {
        pattern,
        samples,
        scenarios,
    } = serde_json::from_str(SCENARIOS).expect("scenarios.json matches the scenario format");
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
