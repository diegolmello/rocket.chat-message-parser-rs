use super::chars::{
    is, is_alpha_digit, is_alpha_numeric_char, is_any_text, is_ascii_alpha, is_hex_digit,
    is_names_char, is_one_of, is_space,
};
use super::memo::{memoized, Key, Memo};
use super::reduce::{reduce_plain_texts, Item};
use super::repetition::{Piece, Repetition, Run};
use super::{Flags, Parsed, Parser};
use crate::ast::{
    bold, color, inline_code, inline_katex, italic, mention_channel, mention_user, plain, spoiler,
    strike, Node,
};

fn hex_value(unit: u16) -> u8 {
    char::from_u32(u32::from(unit))
        .and_then(|digit| digit.to_digit(16))
        .unwrap_or(0) as u8
}

fn is_plain_run_char(unit: u16) -> bool {
    !is_one_of(unit, b"*_~`:\n<[]! \t()\\|")
}

fn is_emphasis_plain_run_char(unit: u16) -> bool {
    !is_one_of(unit, b"\n* _~")
}

fn is_strike_plain_run_char(unit: u16) -> bool {
    !is_one_of(unit, b"\n~ *_:`@#[]\\<!|")
        && !(0xD83C..=0xD83E).contains(&unit)
        && !(0x2300..=0x27BF).contains(&unit)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Emphasis {
    Bold,
    Italic,
    Strike,
}

#[derive(Clone, Copy)]
pub(super) enum EmphasisContent {
    Bold,
    Italic,
}

impl EmphasisContent {
    fn marker(self) -> u8 {
        match self {
            EmphasisContent::Bold => b'*',
            EmphasisContent::Italic => b'_',
        }
    }

    fn other_emphasis(self) -> Emphasis {
        match self {
            EmphasisContent::Bold => Emphasis::Italic,
            EmphasisContent::Italic => Emphasis::Bold,
        }
    }

    fn is_preferential_memoized(self, memo: &Memo, key: Key) -> bool {
        match self {
            EmphasisContent::Bold => memo.bold_preferential_item.contains_key(&key),
            EmphasisContent::Italic => memo.italic_preferential_item.contains_key(&key),
        }
    }
}

impl Parser<'_> {
    pub(super) fn inline_item_pattern(&mut self, position: usize, flags: Flags) -> Parsed<Item> {
        memoized!(self, inline_item_pattern, position, flags, {
            let unit = self.at(position)?;
            self.dispatched_inline_item(position, unit, flags)
                .or_else(|| self.inline_item_slow_path(position, flags))
        })
    }

    fn dispatched_inline_item(&mut self, position: usize, unit: u16, flags: Flags) -> Parsed<Item> {
        let one = |parsed: Parsed<Node>| parsed.map(|(end, node)| (end, Item::One(node)));
        match char::from_u32(u32::from(unit)) {
            Some(' ' | '\t') => one(self.whitespace(position)),
            Some('\\') => one(self
                .escaped_timestamp_rules(position)
                .or_else(|| self.katex_inline(position))
                .or_else(|| self.escaped(position))),
            Some('[') => one(self.maybe_references(position, flags)),
            Some('<') => one(self
                .timestamp_rules(position)
                .or_else(|| self.maybe_references(position, flags))
                .or_else(|| self.inline_emoticon(position, flags))),
            Some('!') => one(self.image(position)),
            Some('|') => one(self.spoiler(position, flags)),
            Some('@') => one(self.user_mention_direct(position)),
            Some('`') => one(self.inline_code(position)),
            Some('+') => one(self.autolinked_phone(position)),
            Some('$') => one(self.katex_inline(position)),
            Some(':') => one(self
                .emoji(position)
                .or_else(|| self.inline_emoticon(position, flags))),
            Some('*') => self
                .emphasis_with_whitespace(position)
                .map(|(end, node)| (end, Item::One(node)))
                .or_else(|| self.emphasis(position, flags))
                .or_else(|| one(self.inline_emoticon(position, flags))),
            Some('~') => self
                .emphasis_with_whitespace(position)
                .map(|(end, node)| (end, Item::One(node)))
                .or_else(|| self.emphasis(position, flags)),
            Some('-') => one(self
                .plain_run_before_emoticon(position)
                .or_else(|| self.inline_emoticon(position, flags))),
            _ => None,
        }
    }

    fn inline_item_slow_path(&mut self, position: usize, flags: Flags) -> Parsed<Item> {
        let one = |parsed: Parsed<Node>| parsed.map(|(end, node)| (end, Item::One(node)));
        one(self.autolinked_email(position))
            .or_else(|| one(self.plain_underscore_then_domain(position)))
            .or_else(|| one(self.autolinked_url(position)))
            .or_else(|| one(self.emphasis_with_whitespace(position)))
            .or_else(|| self.emphasis(position, flags))
            .or_else(|| one(self.channel_mention(position)))
            .or_else(|| one(self.emoji(position)))
            .or_else(|| one(self.plain_run_before_emoticon(position)))
            .or_else(|| one(self.inline_emoticon(position, flags)))
            .or_else(|| one(self.color(position)))
            .or_else(|| one(self.plain_run(position)))
    }

    fn inline_emoticon(&mut self, position: usize, flags: Flags) -> Parsed<Node> {
        let (end, node) = self.emoticon(position)?;
        self.evaluate_emoticon_chain_backwards(
            end,
            flags,
            |memo, key| memo.inline_item_pattern.contains_key(&key),
            |parser, cursor, flags| {
                parser.inline_item_pattern(cursor, flags);
            },
        );
        (self.emoticon_neighbor(end) || self.inline_item_pattern(end, flags).is_some())
            .then_some((end, node))
    }

    fn evaluate_emoticon_chain_backwards(
        &mut self,
        start: usize,
        flags: Flags,
        is_memoized: impl Fn(&Memo, Key) -> bool,
        evaluate: impl Fn(&mut Self, usize, Flags),
    ) {
        let mut chain = Vec::new();
        let mut cursor = start;
        while !is_memoized(&self.memo, (cursor as u32, flags.bits())) {
            let Some((end, _)) = self.emoticon(cursor) else {
                break;
            };
            chain.push(cursor);
            cursor = end;
        }
        for cursor in chain.into_iter().rev() {
            evaluate(self, cursor, flags);
        }
    }

    fn plain_run_before_emoticon(&mut self, position: usize) -> Parsed<Node> {
        let after_dash = self.char(position, b'-')?;
        let after_underscore = self.char(after_dash, b'_')?;
        let after_second_dash = self.char(after_underscore, b'-')?;
        self.any_unit(after_second_dash)?;
        Some((after_dash, plain("-")))
    }

    fn plain_run(&mut self, position: usize) -> Parsed<Node> {
        let end = self.run1(position, is_plain_run_char)?;
        Some((end, plain(self.text(position, end))))
    }

    pub(super) fn whitespace(&mut self, position: usize) -> Parsed<Node> {
        memoized!(self, whitespace, position, Flags::NONE, {
            let end = self.run1(position, is_space)?;
            Some((end, plain(self.text(position, end))))
        })
    }

    fn escaped(&mut self, position: usize) -> Parsed<Node> {
        let escaped_start = self.char(position, b'\\')?;
        let end = self.class(escaped_start, |unit| is_one_of(unit, b"*_~`#."))?;
        Some((end, plain(self.text(escaped_start, end))))
    }

    pub(super) fn line(&mut self, position: usize) -> Parsed<Node> {
        memoized!(self, line, position, Flags::NONE, {
            let text_start = self.spaces(position);
            let text_end = self.run1(text_start, is_any_text)?;
            let end = self.spaces(text_end);
            Some((end, plain(self.text(position, end))))
        })
    }

    pub(super) fn inline_code(&mut self, position: usize) -> Parsed<Node> {
        memoized!(self, inline_code, position, Flags::NONE, {
            let text_start = self.char(position, b'`')?;
            let text_end = self.run1(text_start, |unit| !is(unit, b'`') && unit != 0x0A)?;
            let end = self.char(text_end, b'`')?;
            Some((end, inline_code(plain(self.text(text_start, text_end)))))
        })
    }

    pub(super) fn katex_inline(&mut self, position: usize) -> Parsed<Node> {
        memoized!(self, katex_inline, position, Flags::NONE, {
            let parenthesis = self.options.katex.parenthesis_syntax;
            let dollar = self.options.katex.dollar_syntax;
            let content_start = if parenthesis && self.literal(position, "\\(").is_some() {
                position + 2
            } else if dollar && self.char(position, b'$').is_some() {
                position + 1
            } else {
                return None;
            };
            let content_end = self.inline_katex_end(content_start);
            if content_end >= self.len() {
                return None;
            }
            let end = if parenthesis && self.literal(content_end, "\\)").is_some() {
                content_end + 2
            } else {
                content_end + 1
            };
            Some((end, inline_katex(self.text(content_start, content_end))))
        })
    }

    fn color(&mut self, position: usize) -> Parsed<Node> {
        if !self.options.colors {
            return None;
        }
        let tuple_start = self.literal(position, "color:#")?;
        let (tuple_end, channels) = self
            .color_bytes(tuple_start)
            .or_else(|| self.color_nibbles(tuple_start))?;
        if self.unit_is(tuple_end, is_any_text) {
            return None;
        }
        let alpha = channels.get(3).copied().unwrap_or(255);
        Some((
            tuple_end,
            color(channels[0], channels[1], channels[2], alpha),
        ))
    }

    fn color_bytes(&mut self, position: usize) -> Parsed<Vec<u8>> {
        let mut channels = Vec::new();
        let mut cursor = position;
        while channels.len() < 4 {
            let Some(high) = self.at(cursor).filter(|&unit| is_hex_digit(unit)) else {
                break;
            };
            let Some(low) = self.at(cursor + 1).filter(|&unit| is_hex_digit(unit)) else {
                break;
            };
            channels.push(hex_value(high) * 16 + hex_value(low));
            cursor += 2;
        }
        (channels.len() >= 3).then_some((cursor, channels))
    }

    fn color_nibbles(&mut self, position: usize) -> Parsed<Vec<u8>> {
        let mut channels = Vec::new();
        let mut cursor = position;
        while channels.len() < 4 {
            let Some(digit) = self.at(cursor).filter(|&unit| is_hex_digit(unit)) else {
                break;
            };
            channels.push(hex_value(digit) * 17);
            cursor += 1;
        }
        (channels.len() >= 3).then_some((cursor, channels))
    }

    pub(super) fn spoiler(&mut self, position: usize, flags: Flags) -> Parsed<Node> {
        memoized!(self, spoiler, position, flags, {
            let mut cursor = self.literal(position, "||")?;
            let mut items = Vec::new();
            loop {
                if self.literal(cursor, "||").is_some() {
                    break;
                }
                if let Some((end, item)) = self.inline_item_pattern(cursor, flags) {
                    items.push(item);
                    cursor = end;
                } else if let Some(end) = self.any(cursor) {
                    items.push(Item::One(plain(self.text(cursor, end))));
                    cursor = end;
                } else {
                    break;
                }
            }
            if items.is_empty() {
                return None;
            }
            let end = self.literal(cursor, "||")?;
            Some((end, spoiler(reduce_plain_texts(items))))
        })
    }

    fn names(&mut self, position: usize) -> Option<usize> {
        self.run1(position, is_names_char)
    }

    pub(super) fn user_mention_direct(&mut self, position: usize) -> Parsed<Node> {
        let name_start = self.run1(position, |unit| is(unit, b'@'))?;
        let mut name_end = self.names(name_start)?;
        if let Some(separator_end) = self.class(name_end, |unit| is(unit, b':') || is(unit, b'@')) {
            if let Some(end) = self.names(separator_end) {
                name_end = end;
            }
        }
        let name = self.text(name_start, name_end);
        (!name.ends_with("__")).then(|| (name_end, mention_user(name)))
    }

    fn user_mention(&mut self, position: usize) -> Parsed<Node> {
        memoized!(self, user_mention, position, Flags::NONE, {
            self.text_then_marker(position, b'@', true)
                .or_else(|| self.user_mention_direct(position))
        })
    }

    fn text_then_marker(&mut self, position: usize, marker: u8, repeated: bool) -> Parsed<Node> {
        let after_text = self.class(position, is_any_text)?;
        let after_marker = if repeated {
            self.run1(after_text, |unit| is(unit, marker))?
        } else {
            self.char(after_text, marker)?
        };
        let end = self.class(after_marker, is_alpha_numeric_char)?;
        let text = self.text(position, after_text);
        let name = self.text(after_marker, end);
        Some((end, plain(format!("{text}{}{name}", marker as char))))
    }

    pub(super) fn channel_mention(&mut self, position: usize) -> Parsed<Node> {
        memoized!(self, channel_mention, position, Flags::NONE, {
            self.text_then_marker(position, b'#', false).or_else(|| {
                let name_start = self.char(position, b'#')?;
                let name_end = self.names(name_start)?;
                Some((name_end, mention_channel(self.text(name_start, name_end))))
            })
        })
    }

    fn emphasis_with_whitespace(&mut self, position: usize) -> Parsed<Node> {
        for marker in [b'*', b'_', b'~'] {
            let Some(opening_end) = self.run_end(Run::Marker(marker), position) else {
                continue;
            };
            let spaces_end = self.run_end(Run::Spaces, opening_end)?;
            let end = self.run_end(Run::Marker(marker), spaces_end)?;
            return Some((end, plain(self.text(position, end))));
        }
        None
    }

    pub(super) fn emphasis(&mut self, position: usize, flags: Flags) -> Parsed<Item> {
        memoized!(self, emphasis, position, flags, {
            self.maybe_emphasis(position, flags, Emphasis::Bold)
                .or_else(|| self.maybe_emphasis(position, flags, Emphasis::Italic))
                .or_else(|| self.maybe_emphasis(position, flags, Emphasis::Strike))
        })
    }

    fn maybe_emphasis(&mut self, position: usize, flags: Flags, kind: Emphasis) -> Parsed<Item> {
        let flag = match kind {
            Emphasis::Bold => Flags::BOLD,
            Emphasis::Italic => Flags::ITALIC,
            Emphasis::Strike => Flags::STRIKE,
        };
        if flags.has(flag) {
            return None;
        }
        let inner = flags.with(flag);
        match kind {
            Emphasis::Bold => self
                .bold(position, inner)
                .map(|(end, node)| (end, Item::One(node))),
            Emphasis::Italic => self.italic(position, inner),
            Emphasis::Strike => self
                .strikethrough(position, inner)
                .map(|(end, node)| (end, Item::One(node))),
        }
    }

    fn delimited(
        &mut self,
        position: usize,
        flags: Flags,
        marker: u8,
        repetition: Repetition,
        wrap: fn(Vec<Node>) -> Node,
    ) -> Parsed<Node> {
        let double = [marker, marker];
        let double = std::str::from_utf8(&double).unwrap_or_default().to_string();
        let single = double[..1].to_string();
        for delimiter in [double, single] {
            let Some(content_start) = self.literal(position, &delimiter) else {
                continue;
            };
            let Some(content_end) = self.repetition_end(repetition, content_start, flags) else {
                continue;
            };
            let Some(end) = self.literal(content_end, &delimiter) else {
                continue;
            };
            let items = self.repetition_items(repetition, content_start, content_end, flags);
            return Some((end, wrap(reduce_plain_texts(items))));
        }
        None
    }

    fn bold(&mut self, position: usize, flags: Flags) -> Parsed<Node> {
        memoized!(
            self,
            bold,
            position,
            flags,
            self.delimited(position, flags, b'*', Repetition::Bold, bold)
        )
    }

    pub(super) fn emphasis_content_item(
        &mut self,
        position: usize,
        flags: Flags,
        content: EmphasisContent,
    ) -> Parsed<Piece> {
        if let Some((end, item)) = self.preferential_item(position, flags, content) {
            return Some((end, Piece::Item(item)));
        }
        if let Some(end) = self.run1(position, is_emphasis_plain_run_char) {
            return Some((end, Piece::Text));
        }
        let marker = content.marker();
        if let Some(end) = self.class(position, |unit| !is_one_of(unit, &[b'\n', marker, b' '])) {
            return Some((end, Piece::Text));
        }
        self.line(position)
            .map(|(end, node)| (end, Piece::Item(Item::One(node))))
    }

    fn preferential_item(
        &mut self,
        position: usize,
        flags: Flags,
        content: EmphasisContent,
    ) -> Parsed<Item> {
        match content {
            EmphasisContent::Bold => memoized!(
                self,
                bold_preferential_item,
                position,
                flags,
                self.preferential_item_uncached(position, flags, content)
            ),
            EmphasisContent::Italic => memoized!(
                self,
                italic_preferential_item,
                position,
                flags,
                self.preferential_item_uncached(position, flags, content)
            ),
        }
    }

    fn preferential_item_uncached(
        &mut self,
        position: usize,
        flags: Flags,
        content: EmphasisContent,
    ) -> Parsed<Item> {
        let one = |parsed: Parsed<Node>| parsed.map(|(end, node)| (end, Item::One(node)));
        one(self.whitespace(position))
            .or_else(|| one(self.inline_code(position)))
            .or_else(|| one(self.timestamp_rules(position)))
            .or_else(|| one(self.maybe_references(position, flags)))
            .or_else(|| one(self.user_mention(position)))
            .or_else(|| one(self.channel_mention(position)))
            .or_else(|| self.maybe_emphasis(position, flags, content.other_emphasis()))
            .or_else(|| self.maybe_emphasis(position, flags, Emphasis::Strike))
            .or_else(|| one(self.emoji(position)))
            .or_else(|| {
                let (end, node) = self.emoticon(position)?;
                self.evaluate_emoticon_chain_backwards(
                    end,
                    flags,
                    |memo, key| content.is_preferential_memoized(memo, key),
                    |parser, cursor, flags| {
                        parser.preferential_item(cursor, flags, content);
                    },
                );
                let followed = self.emoticon_neighbor(end)
                    || self.preferential_item(end, flags, content).is_some()
                    || matches!(content, EmphasisContent::Italic) && self.char(end, b'_').is_some();
                followed.then_some((end, Item::One(node)))
            })
    }

    fn italic(&mut self, position: usize, flags: Flags) -> Parsed<Item> {
        memoized!(
            self,
            italic,
            position,
            flags,
            self.italic_uncached(position, flags)
        )
    }

    fn italic_uncached(&mut self, position: usize, flags: Flags) -> Parsed<Item> {
        if let Some(word_end) = self.run1(position, is_alpha_digit) {
            if let Some(after_underscore) = self.char(word_end, b'_') {
                let end = self
                    .char(after_underscore, b'_')
                    .unwrap_or(after_underscore);
                return Some((end, Item::One(plain(self.text(position, end)))));
            }
        }
        for (delimiter, suffix_is_alpha_digit) in [("__", true), ("_", false)] {
            let Some(content_start) = self.literal(position, delimiter) else {
                continue;
            };
            let Some(content_end) = self.repetition_end(Repetition::Italic, content_start, flags)
            else {
                continue;
            };
            let Some(suffix_start) = self.literal(content_end, delimiter) else {
                continue;
            };
            let suffix_end = if suffix_is_alpha_digit {
                self.run1(suffix_start, is_alpha_digit)
            } else {
                self.run1(suffix_start, is_ascii_alpha)
            };
            let Some(suffix_end) = suffix_end else {
                continue;
            };
            let items = self.italic_content_items(content_start, content_end, flags);
            let reduced = reduce_plain_texts(vec![
                Item::One(plain(delimiter)),
                Item::Many(items),
                Item::One(plain(delimiter)),
                Item::One(plain(self.text(suffix_start, suffix_end))),
            ]);
            return Some((suffix_end, Item::Many(reduced)));
        }
        for delimiter in ["__", "_"] {
            let Some(content_start) = self.literal(position, delimiter) else {
                continue;
            };
            let Some(content_end) = self.repetition_end(Repetition::Italic, content_start, flags)
            else {
                continue;
            };
            let Some(end) = self.literal(content_end, delimiter) else {
                continue;
            };
            let items = self.italic_content_items(content_start, content_end, flags);
            return Some((end, Item::One(italic(items))));
        }
        None
    }

    fn italic_content_items(&mut self, position: usize, end: usize, flags: Flags) -> Vec<Node> {
        let items = self.repetition_items(Repetition::Italic, position, end, flags);
        reduce_plain_texts(items)
    }

    fn strikethrough(&mut self, position: usize, flags: Flags) -> Parsed<Node> {
        memoized!(
            self,
            strikethrough,
            position,
            flags,
            self.delimited(position, flags, b'~', Repetition::Strike, strike)
        )
    }

    pub(super) fn strike_content_item(&mut self, position: usize, flags: Flags) -> Parsed<Piece> {
        let one =
            |parsed: Parsed<Node>| parsed.map(|(end, node)| (end, Piece::Item(Item::One(node))));
        let item = |parsed: Parsed<Item>| parsed.map(|(end, item)| (end, Piece::Item(item)));
        if let Some(end) = self.run1(position, is_strike_plain_run_char) {
            return Some((end, Piece::Text));
        }
        one(self.escaped_timestamp_rules(position))
            .or_else(|| one(self.timestamp_rules(position)))
            .or_else(|| one(self.whitespace(position)))
            .or_else(|| one(self.inline_code(position)))
            .or_else(|| one(self.maybe_references(position, flags)))
            .or_else(|| one(self.user_mention(position)))
            .or_else(|| one(self.channel_mention(position)))
            .or_else(|| item(self.maybe_emphasis(position, flags, Emphasis::Italic)))
            .or_else(|| item(self.maybe_emphasis(position, flags, Emphasis::Bold)))
            .or_else(|| one(self.emoji(position)))
            .or_else(|| one(self.emoticon(position)))
            .or_else(|| {
                let end = self.character(position, |unit| !is_one_of(unit, b"\n~ "))?;
                Some((end, Piece::Text))
            })
            .or_else(|| one(self.line(position)))
    }

    pub(super) fn maybe_references(&mut self, position: usize, flags: Flags) -> Parsed<Node> {
        if flags.has(Flags::REFERENCES) {
            return None;
        }
        self.references(position, flags.with(Flags::REFERENCES))
    }
}
