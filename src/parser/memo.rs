use rustc_hash::FxHashMap as HashMap;

use super::blocks::NestedLine;
use super::links::Href;
use super::reduce::Item;
use super::Parsed;
use crate::ast::Node;

pub(crate) type Key = (u32, u8);

macro_rules! memo_tables {
    ($($table:ident: $value:ty),* $(,)?) => {
        #[derive(Default)]
        pub(crate) struct Memo {
            $(pub(crate) $table: HashMap<Key, Parsed<$value>>,)*
            pub(crate) repetition_end: HashMap<(u32, u8, u16), u32>,
            repeated_evaluations: u64,
        }

        impl Memo {
            pub(crate) fn repeated_evaluations(&self) -> u64 {
                self.repeated_evaluations
            }

            pub(crate) fn record_repeat(&mut self) {
                self.repeated_evaluations += 1;
            }
        }
    };
}

memo_tables! {
    inline: Vec<Node>,
    inline_item_pattern: Item,
    emphasis: Item,
    bold: Node,
    italic: Item,
    bold_preferential_item: Item,
    italic_preferential_item: Item,
    strikethrough: Node,
    references: Node,
    emoji: Node,
    emoticon: Node,
    timestamp_rules: Node,
    escaped_timestamp_rules: Node,
    user_mention: Node,
    channel_mention: Node,
    markdown_link_ref: Href,
    link_ref: Href,
    domain_name: (),
    url_authority_host: (),
    url_scheme: (),
    autolinked_url: Node,
    autolinked_email: Node,
    inline_code: Node,
    whitespace: Node,
    line: Node,
    katex_inline: Node,
    spoiler: Node,
    image: Node,
    unordered_list_item_content: Vec<Node>,
    nested_list_lines: Vec<NestedLine>,
}

macro_rules! memoized {
    ($parser:ident, $table:ident, $position:expr, $flags:expr, $body:expr) => {{
        let key: $crate::parser::memo::Key = ($position as u32, $flags.bits());
        $parser.report.rule_calls += 1;
        if let Some(hit) = $parser.memo.$table.get(&key) {
            hit.clone()
        } else {
            $parser.report.rule_evaluations += 1;
            #[allow(clippy::redundant_closure_call)]
            let parsed: $crate::parser::Parsed<_> = (|| -> $crate::parser::Parsed<_> { $body })();
            if $parser.memo.$table.insert(key, parsed.clone()).is_some() {
                $parser.memo.record_repeat();
            }
            parsed
        }
    }};
}

pub(crate) use memoized;
