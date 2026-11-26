use std::collections::HashMap;

use serde::Deserialize;

use super::is_linkable;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FixtureOptions {
    detect_ip: bool,
    valid_hosts: Vec<String>,
}

#[derive(Deserialize)]
struct FixtureCase {
    options: String,
    input: String,
    linkable: bool,
}

#[derive(Deserialize)]
struct Fixture {
    options: HashMap<String, FixtureOptions>,
    cases: Vec<FixtureCase>,
}

#[test]
fn linkability_matches_tldts_for_every_fixture_case() {
    let fixture: Fixture =
        serde_json::from_str(include_str!("../../tests/fixtures/tld/cases.json"))
            .expect("valid tld fixture");
    let mismatches: Vec<String> = fixture
        .cases
        .iter()
        .filter_map(|case| {
            let options = &fixture.options[&case.options];
            let actual = is_linkable(&case.input, options.detect_ip, &options.valid_hosts);
            (actual != case.linkable).then(|| {
                format!(
                    "[{}] {:?}: expected {}, got {actual}",
                    case.options, case.input, case.linkable
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
