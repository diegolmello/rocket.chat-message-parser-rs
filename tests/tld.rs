use std::collections::HashMap;

use message_parser::tld::{parse, TldOptions, TldResult};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FixtureOptions {
    detect_ip: Option<bool>,
    allow_private_domains: Option<bool>,
    valid_hosts: Option<Vec<String>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FixtureCase {
    options: String,
    input: String,
    hostname: Option<String>,
    domain: Option<String>,
    public_suffix: Option<String>,
    is_icann: Option<bool>,
    is_private: Option<bool>,
    is_ip: Option<bool>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Fixture {
    tldts_version: String,
    options: HashMap<String, FixtureOptions>,
    cases: Vec<FixtureCase>,
}

fn load_fixture() -> Fixture {
    serde_json::from_str(include_str!("fixtures/tld/cases.json")).expect("valid tld fixture")
}

fn to_tld_options(fixture_options: &FixtureOptions) -> TldOptions {
    let defaults = TldOptions::default();
    TldOptions {
        detect_ip: fixture_options.detect_ip.unwrap_or(defaults.detect_ip),
        allow_private_domains: fixture_options
            .allow_private_domains
            .unwrap_or(defaults.allow_private_domains),
        valid_hosts: fixture_options.valid_hosts.clone(),
    }
}

#[test]
fn fixture_comes_from_tldts_6_1_86() {
    let fixture = load_fixture();
    assert_eq!(fixture.tldts_version, "6.1.86");
    assert!(fixture.cases.len() > 1000);
}

#[test]
fn matches_tldts_for_every_fixture_case() {
    let fixture = load_fixture();
    let options: HashMap<&str, TldOptions> = fixture
        .options
        .iter()
        .map(|(name, fixture_options)| (name.as_str(), to_tld_options(fixture_options)))
        .collect();

    let mismatches: Vec<String> = fixture
        .cases
        .iter()
        .filter_map(|case| {
            let expected = TldResult {
                hostname: case.hostname.clone(),
                domain: case.domain.clone(),
                public_suffix: case.public_suffix.clone(),
                is_icann: case.is_icann,
                is_private: case.is_private,
                is_ip: case.is_ip,
            };
            let actual = parse(&case.input, &options[case.options.as_str()]);
            (actual != expected).then(|| {
                format!(
                    "[{}] {:?}\n  expected {:?}\n  actual   {:?}",
                    case.options, case.input, expected, actual
                )
            })
        })
        .collect();

    assert!(
        mismatches.is_empty(),
        "{} of {} cases differ from tldts:\n{}",
        mismatches.len(),
        fixture.cases.len(),
        mismatches.join("\n")
    );
}

#[test]
fn default_options_mirror_tldts_defaults() {
    let defaults = TldOptions::default();
    assert!(defaults.detect_ip);
    assert!(!defaults.allow_private_domains);
    assert_eq!(defaults.valid_hosts, None);
}
