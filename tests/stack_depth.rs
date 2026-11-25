use std::time::{SystemTime, UNIX_EPOCH};

use message_parser::{parse_at, Options};

fn fixed_clock() -> SystemTime {
    UNIX_EPOCH + std::time::Duration::from_secs(1_753_142_400)
}

#[test]
fn long_emoticon_chains_parse_within_a_main_thread_stack() {
    const MAIN_THREAD_STACK_BYTES: usize = 8 * 1024 * 1024;
    let options = Options {
        emoticons: true,
        ..Options::default()
    };
    let inputs = [
        ":)".repeat(50_000),
        format!("*{}", ":)".repeat(50_000)),
        format!("_{}", ":)".repeat(50_000)),
    ];
    let parser_thread = std::thread::Builder::new()
        .stack_size(MAIN_THREAD_STACK_BYTES)
        .spawn(move || {
            inputs
                .iter()
                .map(|input| parse_at(input, &options, fixed_clock()).len())
                .sum::<usize>()
        })
        .expect("parser thread starts");
    assert!(
        parser_thread
            .join()
            .expect("parser thread finishes without overflowing")
            > 0
    );
}

#[test]
fn deeply_nested_lists_stop_nesting_at_the_depth_limit_and_keep_every_item() {
    const SPAWNED_THREAD_STACK_BYTES: usize = 2 * 1024 * 1024;
    const INDENTED_LINES: usize = 2_000;
    const MAXIMUM_LIST_DEPTH: usize = 64;
    let input: Vec<String> = (0..INDENTED_LINES)
        .map(|level| format!("{}- a", " ".repeat(level)))
        .collect();
    let input = input.join("\n");
    let parser_thread = std::thread::Builder::new()
        .stack_size(SPAWNED_THREAD_STACK_BYTES)
        .spawn(move || {
            let nodes = parse_at(&input, &Options::default(), fixed_clock());
            let serialized = serde_json::to_string(&nodes).expect("AST serializes");
            let mut depth = 0;
            let mut deepest_items = 0;
            let mut lists = nodes.clone();
            while let Some(message_parser::Node::UnorderedList { value }) = lists.pop() {
                depth += 1;
                deepest_items = value.len();
                let Some(message_parser::Node::ListItem {
                    nested: Some(nested),
                    ..
                }) = value.into_iter().last()
                else {
                    break;
                };
                lists = nested;
            }
            drop(nodes);
            (depth, deepest_items, serialized.matches("\"a\"").count())
        })
        .expect("parser thread starts");
    let (depth, deepest_items, items) = parser_thread
        .join()
        .expect("parser thread finishes without overflowing");
    assert_eq!(depth, MAXIMUM_LIST_DEPTH);
    assert_eq!(deepest_items, INDENTED_LINES - MAXIMUM_LIST_DEPTH + 1);
    assert_eq!(items, INDENTED_LINES);
}
