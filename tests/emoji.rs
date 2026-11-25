use std::collections::HashSet;

use message_parser::{parse, Node, Options};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct EmojiFixture {
    emojibase_version: String,
    emoji: Vec<String>,
}

const KNOWN_UNSUPPORTED: &str = "🇦 🇧 🇨 🇩 🇪 🇫 🇬 🇭 🇮 🇯 🇰 🇱 🇲 🇳 🇴 🇵 🇶 🇷 🇸 🇹 🇺 🇻 🇼 🇽 🇾 🇿";

fn parses_whole(input: &str) -> bool {
    let nodes = parse(input, &Options::default());
    let [Node::BigEmoji { value }] = nodes.as_slice() else {
        return false;
    };
    let [Node::Emoji {
        unicode: Some(unicode),
        ..
    }] = value.as_slice()
    else {
        return false;
    };
    unicode == input
}

#[test]
fn every_emoji_parses_as_one_token_except_documented_components() {
    let fixture: EmojiFixture = serde_json::from_str(include_str!("fixtures/emoji/all-emoji.json"))
        .expect("all-emoji.json matches the extractor's schema");
    let known_unsupported: HashSet<&str> = KNOWN_UNSUPPORTED.split(' ').collect();
    let in_fixture: HashSet<&str> = fixture.emoji.iter().map(String::as_str).collect();

    assert!(
        fixture.emoji.len() > 3000,
        "emojibase@{} fixture is too small",
        fixture.emojibase_version
    );

    let not_parsed_whole: Vec<&str> = fixture
        .emoji
        .iter()
        .map(String::as_str)
        .filter(|emoji| !known_unsupported.contains(emoji) && !parses_whole(emoji))
        .collect();
    assert!(
        not_parsed_whole.is_empty(),
        "{} emoji from emojibase@{} do not parse as a single BIG_EMOJI token: {not_parsed_whole:?}",
        not_parsed_whole.len(),
        fixture.emojibase_version
    );

    let mut stale_unsupported: Vec<&str> = known_unsupported
        .iter()
        .copied()
        .filter(|emoji| !in_fixture.contains(emoji) || parses_whole(emoji))
        .collect();
    stale_unsupported.sort_unstable();
    assert!(
        stale_unsupported.is_empty(),
        "KNOWN_UNSUPPORTED entries that are missing from the fixture or now parse whole: {stale_unsupported:?}"
    );
}
