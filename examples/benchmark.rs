use std::time::{Duration, Instant, SystemTime};

use message_parser::{parse_with_report, KatexOptions, Options};

const PATTERN: &str = "*_~[$:";
const SAMPLES: u32 = 20;

struct Scenario {
    length: usize,
    options: Options,
}

fn markup_soup(length: usize) -> String {
    PATTERN.chars().cycle().take(length).collect()
}

fn scenarios() -> Vec<Scenario> {
    let everything = Options {
        emoticons: true,
        katex: KatexOptions {
            dollar_syntax: true,
            parenthesis_syntax: true,
        },
        ..Options::default()
    };
    vec![
        Scenario {
            length: 246,
            options: Options::default(),
        },
        Scenario {
            length: 498,
            options: Options::default(),
        },
        Scenario {
            length: 996,
            options: Options::default(),
        },
        Scenario {
            length: 4_902,
            options: Options::default(),
        },
        Scenario {
            length: 4_902,
            options: everything,
        },
    ]
}

fn median(mut durations: Vec<Duration>) -> Duration {
    durations.sort();
    durations[durations.len() / 2]
}

fn main() {
    let now = SystemTime::now();
    println!(
        "{:>7} {:>10} {:>12} {:>11} {:>14}",
        "length", "options", "median", "work", "work per char"
    );
    for scenario in scenarios() {
        let input = markup_soup(scenario.length);
        let label = if scenario.options == Options::default() {
            "default"
        } else {
            "all on"
        };
        let mut durations = Vec::new();
        let mut work = 0;
        for _ in 0..SAMPLES {
            let started = Instant::now();
            let outcome = parse_with_report(&input, &scenario.options, now);
            durations.push(started.elapsed());
            work = outcome.report.work();
        }
        println!(
            "{:>7} {:>10} {:>12?} {:>11} {:>14.1}",
            scenario.length,
            label,
            median(durations),
            work,
            work as f64 / scenario.length as f64
        );
    }
}
