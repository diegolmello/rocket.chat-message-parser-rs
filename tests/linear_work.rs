use std::time::{SystemTime, UNIX_EPOCH};

use message_parser::{parse_with_report, KatexOptions, Options, ParseReport};

const WORK_PER_UNIT_LIMIT: u64 = 400;
const DENSITY_GROWTH_LIMIT: f64 = 2.0;
const MINIMUM_LENGTH: usize = 400;
const SCALES: [usize; 2] = [1, 8];
const EXHAUSTIVE_LENGTH: u32 = 4;

const EXHAUSTIVE_ALPHABET: &[char] = &['*', '_', '~', '[', ']', '(', '$', ':', '`', '<', '\n', 'a'];

const SEED_ALPHABET: &[char] = &[
    '*', '_', '~', '[', ']', '(', ')', '$', ':', '`', '<', '>', '|', '@', '#', ' ', '\n', '\\',
    '!', '-', '.', '/', '+', '1', 't', 'a',
];

const CORE_SEED_ALPHABET: &[char] = &['*', '_', '~', '[', ']', '$', ':', '`', ' ', 'a'];

const KNOWN_SEEDS: &[&str] = &[
    "*_~[$:",
    "<t:1",
    "<t:2025-03-09T02:30:00>",
    "<t:2025-11-02T01:30:00-03:00:R>",
    "\\(",
    "\\[",
    "$$",
    "||",
    ":)",
    ":a",
    "](",
    "[a](",
    "![",
    "http://a.b",
    "a.b/",
    "a@b",
    "@a.",
    "#a",
    "```\n",
    "- ",
    "1. ",
    "> ",
    "| a |",
    "- [ ] ",
    "#ff",
    "+1",
    "__a",
    "~🀄",
    "~x🇧~",
];

const PREFIXED_SEEDS: &[(&str, &str)] = &[
    ("- a", "\n - a"),
    ("- a", "\n  * b\n 1. c\n - [ ] d"),
    ("1. a", "\n - a\n  - b"),
];

fn fixed_clock() -> SystemTime {
    UNIX_EPOCH + std::time::Duration::from_secs(1_753_142_400)
}

fn option_sets() -> [(&'static str, Options); 2] {
    let everything = Options {
        colors: true,
        emoticons: true,
        katex: KatexOptions {
            dollar_syntax: true,
            parenthesis_syntax: true,
        },
        custom_domains: Some(vec!["local".into()]),
    };
    [("default", Options::default()), ("everything", everything)]
}

fn report(input: &str, options: &Options) -> ParseReport {
    parse_with_report(input, options, fixed_clock()).report
}

fn work_per_unit(report: &ParseReport) -> f64 {
    report.work() as f64 / (report.input_length + 1) as f64
}

fn repeated_whole((prefix, seed): &(String, String), scale: usize) -> String {
    let repetitions = MINIMUM_LENGTH.div_ceil(seed.chars().count()) * scale;
    format!("{prefix}{}", seed.repeat(repetitions))
}

fn strings_up_to(alphabet: &[char], max_length: u32) -> Vec<String> {
    let mut all = vec![String::new()];
    let mut previous = vec![String::new()];
    for _ in 0..max_length {
        let next: Vec<String> = previous
            .iter()
            .flat_map(|prefix| {
                alphabet
                    .iter()
                    .map(move |character| format!("{prefix}{character}"))
            })
            .collect();
        all.extend(next.iter().cloned());
        previous = next;
    }
    all
}

fn seeds() -> Vec<(String, String)> {
    let mut seeds: Vec<String> = strings_up_to(SEED_ALPHABET, 2)
        .into_iter()
        .filter(|seed| !seed.is_empty())
        .collect();
    seeds.extend(
        strings_up_to(CORE_SEED_ALPHABET, 3)
            .into_iter()
            .filter(|seed| seed.chars().count() == 3),
    );
    seeds.extend(KNOWN_SEEDS.iter().map(|seed| (*seed).to_owned()));
    let mut prefixed: Vec<(String, String)> = seeds
        .into_iter()
        .map(|seed| (String::new(), seed))
        .collect();
    prefixed.extend(
        PREFIXED_SEEDS
            .iter()
            .map(|(prefix, seed)| ((*prefix).to_owned(), (*seed).to_owned())),
    );
    prefixed
}

#[test]
fn every_short_input_evaluates_each_rule_position_once_within_the_work_limit() {
    let mut violations = Vec::new();
    for (label, options) in option_sets() {
        for input in strings_up_to(EXHAUSTIVE_ALPHABET, EXHAUSTIVE_LENGTH) {
            let report = report(&input, &options);
            let within_limit =
                report.work() <= WORK_PER_UNIT_LIMIT * (report.input_length as u64 + 1);
            if report.repeated_evaluations > 0 || !within_limit {
                violations.push(format!("{label} {input:?}: {report:?}"));
            }
        }
    }
    assert!(
        violations.is_empty(),
        "{} violation(s):\n{}",
        violations.len(),
        violations.join("\n")
    );
}

#[test]
#[cfg_attr(
    debug_assertions,
    ignore = "parses about 15 million units; run with cargo test --release --test linear_work"
)]
fn repeated_seeds_grow_linearly() {
    let mut violations = Vec::new();
    let mut worst_growth = (0.0, String::new());
    let mut worst_density = (0.0, String::new());
    for (label, options) in option_sets() {
        for seed in seeds() {
            let [small, large] =
                SCALES.map(|scale| report(&repeated_whole(&seed, scale), &options));
            let density_growth = work_per_unit(&large) / work_per_unit(&small);
            let density = work_per_unit(&large);
            let repeated = small.repeated_evaluations + large.repeated_evaluations;
            let case = format!("{label} {:?}+{:?}", seed.0, seed.1);
            if density_growth > worst_growth.0 {
                worst_growth = (density_growth, case.clone());
            }
            if density > worst_density.0 {
                worst_density = (density, case.clone());
            }
            if repeated > 0
                || density_growth > DENSITY_GROWTH_LIMIT
                || density > WORK_PER_UNIT_LIMIT as f64
            {
                violations.push(format!(
                    "{case}: work {} at {} units, {} at {} units (work per unit grew {density_growth:.2}x to {density:.0}, {repeated} repeated evaluations)",
                    small.work(),
                    small.input_length,
                    large.work(),
                    large.input_length
                ));
            }
        }
    }
    eprintln!(
        "worst work-per-unit growth {:.3}x for {}",
        worst_growth.0, worst_growth.1
    );
    eprintln!(
        "worst work per unit {:.1} for {}",
        worst_density.0, worst_density.1
    );
    assert!(
        violations.is_empty(),
        "{} violation(s):\n{}",
        violations.len(),
        violations.join("\n")
    );
}
