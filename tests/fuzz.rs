use message_parser::{parse, Options};
use proptest::prelude::*;

const UNANCHORED_PADDING: &str = "[ -~]{0,10}";

fn unanchored(pattern: &str) -> String {
    format!("{UNANCHORED_PADDING}{pattern}{UNANCHORED_PADDING}")
}

fn plain_text() -> impl Strategy<Value = String> {
    proptest::string::string_regex(&unanchored("[a-zA-Z0-9 ]{1,20}"))
        .expect("plain text pattern is a valid regex")
        .prop_filter("non-empty", |value| !value.is_empty())
}

fn timestamp() -> impl Strategy<Value = String> {
    (1_000_000_000u32..=2_000_000_000).prop_map(|value| format!("<t:{value}>"))
}

fn relative_timestamp() -> impl Strategy<Value = String> {
    (1_000_000_000u32..=2_000_000_000).prop_map(|value| format!("<t:{value}:R>"))
}

fn mention() -> impl Strategy<Value = String> {
    proptest::string::string_regex(&unanchored("[a-z0-9._-]{1,12}"))
        .expect("mention pattern is a valid regex")
        .prop_map(|value| format!("@{value}"))
}

fn bold() -> impl Strategy<Value = String> {
    plain_text().prop_map(|value| format!("*{value}*"))
}

fn strike() -> impl Strategy<Value = String> {
    prop_oneof![timestamp(), relative_timestamp(), plain_text()]
        .prop_map(|value| format!("~{value}~"))
}

fn inline_code() -> impl Strategy<Value = String> {
    plain_text().prop_map(|value| format!("`{value}`"))
}

fn parser_input() -> impl Strategy<Value = String> {
    proptest::collection::vec(
        prop_oneof![
            plain_text(),
            timestamp(),
            relative_timestamp(),
            mention(),
            bold(),
            strike(),
            inline_code(),
        ],
        1..=8,
    )
    .prop_map(|parts| parts.join(" "))
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    #[test]
    fn parses_valid_parser_like_inputs_without_panicking(input in parser_input()) {
        parse(&input, &Options::default());
    }

    #[test]
    fn is_deterministic_for_valid_parser_like_inputs(input in parser_input()) {
        prop_assert_eq!(parse(&input, &Options::default()), parse(&input, &Options::default()));
    }
}
