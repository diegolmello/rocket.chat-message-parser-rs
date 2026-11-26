use message_parser::{parse, Options};
use serde_json::json;

#[test]
fn strike_keeps_astral_characters_that_match_no_emoji_whole() {
    for (input, text) in [("~🇧~", "🇧"), ("~x🀄~", "x🀄")] {
        let nodes =
            serde_json::to_value(parse(input, &Options::default())).expect("AST serializes");
        let expected = json!([{ "type": "PARAGRAPH", "value": [{ "type": "STRIKE", "value": [{ "type": "PLAIN_TEXT", "value": text }] }] }]);
        assert_eq!(nodes, expected, "input {input:?}");
    }
}
