use message_parser::{parse_with_report, KatexOptions, Options, ParseReport};

mod common;

use common::{fixed_clock, strings_over};

const WORK_PER_UNIT_LIMIT: f64 = 400.0;
const DENSITY_GROWTH_LIMIT: f64 = 2.0;
const MINIMUM_LENGTH: usize = 400;
const SCALES: [usize; 2] = [1, 8];
const EXHAUSTIVE_LENGTH: usize = 4;

const EXHAUSTIVE_ALPHABET: &[&str] = &["*", "_", "~", "[", "]", "(", "$", ":", "`", "<", "\n", "a"];

const SEED_ALPHABET: &[&str] = &[
    "*", "_", "~", "[", "]", "(", ")", "$", ":", "`", "<", ">", "|", "@", "#", " ", "\n", "\\",
    "!", "-", ".", "/", "+", "1", "t", "a",
];

const CORE_SEED_ALPHABET: &[&str] = &["*", "_", "~", "[", "]", "$", ":", "`", " ", "a"];

const KNOWN_SEEDS: &[&str] = &[
    "*_~[$:",
    "<t:1",
    "<t:2025-03-09T02:30:00>",
    "<t:2025-11-02T01:30:00-03:00:R>",
    "\\(",
    "\\[",
    "$$",
    "||",
    "||:)",
    "||x:)",
    "||`",
    "`||",
    "||`:)",
    "||[",
    "||a.b",
    "||\n\r",
    "*||:)",
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
    "````x\n",
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

fn seeds() -> Vec<(String, String)> {
    let mut seeds = strings_over(SEED_ALPHABET, 1..=2);
    seeds.extend(strings_over(CORE_SEED_ALPHABET, 3..=3));
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
        for input in strings_over(EXHAUSTIVE_ALPHABET, 0..=EXHAUSTIVE_LENGTH) {
            let report = report(&input, &options);
            if report.repeated_evaluations > 0 || work_per_unit(&report) > WORK_PER_UNIT_LIMIT {
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
            let density = work_per_unit(&large);
            let density_growth = density / work_per_unit(&small);
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
                || density > WORK_PER_UNIT_LIMIT
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
