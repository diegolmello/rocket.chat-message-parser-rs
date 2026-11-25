use message_parser::{parse, Node, Options};

fn parsed_seconds(input: &str) -> String {
    let nodes = parse(input, &Options::default());
    let Some(Node::Paragraph { value: paragraph }) = nodes.first() else {
        panic!("{input:?} parses to a paragraph");
    };
    let value = serde_json::to_value(&paragraph[0]).expect("AST serializes");
    value["value"]["timestamp"]
        .as_str()
        .expect("timestamp node")
        .to_owned()
}

#[test]
fn local_times_around_daylight_saving_changes_match_javascript_dates() {
    std::env::set_var("TZ", "America/New_York");
    let cases = [
        ("<t:2025-03-09T01:59:59>", "1741503599"),
        ("<t:2025-03-09T02:30:00>", "1741505400"),
        ("<t:2025-03-09T03:00:00>", "1741503600"),
        ("<t:2025-11-02T00:59:59>", "1762059599"),
        ("<t:2025-11-02T01:30:00>", "1762061400"),
        ("<t:2025-11-02T02:00:00>", "1762066800"),
    ];
    for (input, seconds) in cases {
        assert_eq!(parsed_seconds(input), seconds, "input {input:?}");
    }
}
