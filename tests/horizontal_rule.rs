use message_parser::{parse, Node, Options};

#[test]
fn asterisk_and_underscore_runs_are_not_horizontal_rules() {
    let failures: Vec<String> = ["***", "___", "*******", "_______"]
        .into_iter()
        .filter_map(|input| match parse(input, &Options::default()).first() {
            None => Some(format!("{input:?}: parse returned no nodes")),
            Some(node @ Node::HorizontalRule { .. }) => {
                Some(format!("{input:?}: first node is {node:?}"))
            }
            Some(_) => None,
        })
        .collect();
    assert!(
        failures.is_empty(),
        "parsed as a horizontal rule:\n{}",
        failures.join("\n")
    );
}
