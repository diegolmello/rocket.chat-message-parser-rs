use message_parser::{parse_at, Node, Options};

mod common;

use common::fixed_clock;

const MAIN_THREAD_STACK_BYTES: usize = 8 * 1024 * 1024;
const SPAWNED_THREAD_STACK_BYTES: usize = 2 * 1024 * 1024;

fn on_stack<T: Send + 'static>(bytes: usize, work: impl FnOnce() -> T + Send + 'static) -> T {
    std::thread::Builder::new()
        .stack_size(bytes)
        .spawn(work)
        .expect("parser thread starts")
        .join()
        .expect("parser thread finishes without overflowing")
}

fn parse_with_emoticons(inputs: Vec<String>) {
    let options = Options {
        emoticons: true,
        ..Options::default()
    };
    for input in inputs {
        drop(parse_at(&input, &options, fixed_clock()));
    }
}

#[test]
fn long_emoticon_chains_parse_within_a_main_thread_stack() {
    let inputs = vec![
        ":)".repeat(50_000),
        format!("*{}", ":)".repeat(50_000)),
        format!("_{}", ":)".repeat(50_000)),
    ];
    on_stack(MAIN_THREAD_STACK_BYTES, move || {
        parse_with_emoticons(inputs)
    });
}

#[test]
fn spoilers_ending_in_emoticons_parse_within_a_spawned_thread_stack() {
    let inputs = vec![
        "||:)".repeat(25_000),
        "||x<3".repeat(20_000),
        format!("*{}", "||:)".repeat(25_000)),
        format!("_{}", "||:)".repeat(25_000)),
        format!("![{}", "||:)".repeat(25_000)),
    ];
    on_stack(SPAWNED_THREAD_STACK_BYTES, move || {
        parse_with_emoticons(inputs)
    });
}

#[test]
fn deeply_nested_lists_stop_nesting_at_the_depth_limit_and_keep_every_item() {
    const INDENTED_LINES: usize = 2_000;
    const MAXIMUM_LIST_DEPTH: usize = 64;
    let input: Vec<String> = (0..INDENTED_LINES)
        .map(|level| format!("{}- a", " ".repeat(level)))
        .collect();
    let input = input.join("\n");
    let (depth, deepest_items, items) = on_stack(SPAWNED_THREAD_STACK_BYTES, move || {
        let nodes = parse_at(&input, &Options::default(), fixed_clock());
        let serialized = serde_json::to_string(&nodes).expect("AST serializes");
        let mut depth = 0;
        let mut deepest_items = 0;
        let mut lists = nodes.clone();
        while let Some(Node::UnorderedList { value }) = lists.pop() {
            depth += 1;
            deepest_items = value.len();
            let Some(Node::ListItem {
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
    });
    assert_eq!(depth, MAXIMUM_LIST_DEPTH);
    assert_eq!(deepest_items, INDENTED_LINES - MAXIMUM_LIST_DEPTH + 1);
    assert_eq!(items, INDENTED_LINES);
}
