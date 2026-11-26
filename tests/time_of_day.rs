use std::time::{Duration, UNIX_EPOCH};

use message_parser::{parse_at, Options};
use serde_json::json;

#[test]
fn time_of_day_without_a_timezone_is_zero_like_upstream() {
    let now = UNIX_EPOCH + Duration::from_millis(1_700_000_000_000);
    for (input, timestamp, format) in [
        ("<t:10:00>", "0", "t"),
        ("<t:10:00:30:R>", "0", "R"),
        ("<t:10:00+01:00>", "1699952400", "t"),
    ] {
        let nodes = serde_json::to_value(parse_at(input, &Options::default(), now))
            .expect("AST serializes");
        assert_eq!(
            nodes[0]["value"][0]["value"],
            json!({ "timestamp": timestamp, "format": format }),
            "input {input:?}"
        );
    }
}
