use super::chars::is_js_whitespace;
use super::Parsed;
use crate::ast::{plain, Node};

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Item {
    One(Node),
    Many(Vec<Node>),
}

pub(crate) fn one(parsed: Parsed<Node>) -> Parsed<Item> {
    parsed.map(|(end, node)| (end, Item::One(node)))
}

fn is_blank(text: &str) -> bool {
    text.chars().all(is_js_whitespace)
}

fn is_non_blank_plain(node: Option<&Node>) -> bool {
    matches!(node, Some(Node::PlainText { value }) if !is_blank(value))
}

fn is_emoji(node: Option<&Node>) -> bool {
    matches!(node, Some(Node::Emoji { .. }))
}

fn join_emoji(current: &Node, previous: Option<&Node>, next: Option<&Node>) -> Node {
    let Node::Emoji {
        value: Some(text_node),
        short_code,
        ..
    } = current
    else {
        return current.clone();
    };
    let has_emoji_neighbor = is_emoji(previous) || is_emoji(next);
    let has_plain_neighbor = is_non_blank_plain(previous) || is_non_blank_plain(next);
    if !(has_emoji_neighbor || has_plain_neighbor) {
        return current.clone();
    }
    let Node::PlainText { value: text } = text_node.as_ref() else {
        return current.clone();
    };
    let is_emoticon = short_code.as_deref() != Some(text.as_str());
    if is_emoticon {
        plain(text.clone())
    } else {
        plain(format!(":{text}:"))
    }
}

fn push_merging(result: &mut Vec<Node>, node: Node) {
    if let (Some(Node::PlainText { value: previous }), Node::PlainText { value: current }) =
        (result.last_mut(), &node)
    {
        previous.push_str(current);
        return;
    }
    result.push(node);
}

pub(crate) fn reduce_plain_texts(items: Vec<Item>) -> Vec<Node> {
    let flattened: Vec<Node> = items
        .into_iter()
        .flat_map(|item| match item {
            Item::One(node) => vec![node],
            Item::Many(nodes) => nodes,
        })
        .collect();
    let mut result = Vec::with_capacity(flattened.len());
    for index in 0..flattened.len() {
        let previous = index
            .checked_sub(1)
            .map(|previous_index| &flattened[previous_index]);
        let joined = join_emoji(&flattened[index], previous, flattened.get(index + 1));
        push_merging(&mut result, joined);
    }
    result
}
