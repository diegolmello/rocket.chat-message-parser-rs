use super::chars::{in_table, is, is_ascii_digit, is_emoji_short_code_char, is_variation_selector};
use super::memo::memoized;
use super::{Flags, Parsed, Parser};
use crate::ast::{big_emoji, emoji, emoji_unicode, emoticon, Node};

const EMOTICONS: &[(&str, &str)] = &[
    (":-D", "smiley"),
    (":D", "smiley"),
    (":')", "joy"),
    (":'-)", "joy"),
    (":-)", "slight_smile"),
    (":)", "slight_smile"),
    (":]", "slight_smile"),
    (":-*", "kissing_heart"),
    (":*", "kissing_heart"),
    (":^*", "kissing_heart"),
    (":-P", "stuck_out_tongue"),
    (":P", "stuck_out_tongue"),
    (":-\u{de}", "stuck_out_tongue"),
    (":\u{de}", "stuck_out_tongue"),
    (":-b", "stuck_out_tongue"),
    (":b", "stuck_out_tongue"),
    (":-(", "disappointed"),
    (":(", "disappointed"),
    (":-[", "disappointed"),
    (":[", "disappointed"),
    (":-/", "confused"),
    (":-.", "confused"),
    (":/", "confused"),
    (r":\", "confused"),
    (":L", "confused"),
    (":'(", "cry"),
    (":@", "angry"),
    (":$", "flushed"),
    (":-X", "no_mouth"),
    (":X", "no_mouth"),
    (":-#", "no_mouth"),
    (":#", "no_mouth"),
    (":-O", "open_mouth"),
    (":O", "open_mouth"),
    (";)", "wink"),
    (";-)", "wink"),
    (";-]", "wink"),
    (";]", "wink"),
    (";D", "wink"),
    (";^)", "wink"),
    (";(", "cry"),
    (";-(", "cry"),
    ("</3", "broken_heart"),
    ("<3", "heart"),
    (">:)", "laughing"),
    (">;)", "laughing"),
    (">:-)", "laughing"),
    (">=)", "laughing"),
    (">.<", "persevere"),
    (">:P", "stuck_out_tongue_winking_eye"),
    (r">:\", "confused"),
    (">;/", "confused"),
    (">:(", "angry"),
    (">:-(", "angry"),
    (">:[", "disappointed"),
    (">:O", "open_mouth"),
    ("=D", "smiley"),
    ("=]", "slight_smile"),
    ("=)", "slight_smile"),
    ("=*", "kissing_heart"),
    ("=P", "stuck_out_tongue"),
    ("=(", "disappointed"),
    ("=/", "confused"),
    (r"=\", "confused"),
    ("=L", "confused"),
    ("=$", "flushed"),
    ("=X", "no_mouth"),
    ("=#", "no_mouth"),
    ("D:", "fearful"),
    ("B-)", "sunglasses"),
    ("B)", "sunglasses"),
    ("B-D", "sunglasses"),
    ("8)", "sunglasses"),
    ("8-)", "sunglasses"),
    ("8-D", "sunglasses"),
    ("':)", "sweat_smile"),
    ("':-)", "sweat_smile"),
    ("'=)", "sweat_smile"),
    ("':D", "sweat_smile"),
    ("':-D", "sweat_smile"),
    ("'=D", "sweat_smile"),
    ("':(", "sweat"),
    ("':-(", "sweat"),
    ("'=(", "sweat"),
    ("O:-)", "innocent"),
    ("O:)", "innocent"),
    ("O;-)", "innocent"),
    ("O=)", "innocent"),
    ("O:-3", "innocent"),
    ("O:3", "innocent"),
    ("O_O", "open_mouth"),
    ("0:-3", "innocent"),
    ("0:3", "innocent"),
    ("0:-)", "innocent"),
    ("0:)", "innocent"),
    ("0;^)", "innocent"),
    ("0;-)", "innocent"),
    ("*-)", "wink"),
    ("*)", "wink"),
    (r"*\0/*", "person_gesturing_ok"),
    (r"*\O/*", "person_gesturing_ok"),
    ("X-P", "stuck_out_tongue_winking_eye"),
    ("X)", "dizzy_face"),
    ("X-)", "dizzy_face"),
    ("#-)", "dizzy_face"),
    ("#)", "dizzy_face"),
    ("%-)", "dizzy_face"),
    ("%)", "dizzy_face"),
    ("(y)", "thumbsup"),
    ("-___-", "expressionless"),
    ("-__-", "expressionless"),
    ("-_-", "expressionless"),
    (r"\0\/", "person_gesturing_ok"),
    (r"\O\/", "person_gesturing_ok"),
];

const TEXT_PRESENTATION: &[(u16, u16)] = &[
    (0x00A9, 0x00A9),
    (0x00AE, 0x00AE),
    (0x203C, 0x203C),
    (0x2049, 0x2049),
    (0x2122, 0x2122),
    (0x2139, 0x2139),
    (0x2196, 0x2199),
    (0x21A9, 0x21AA),
    (0x24C2, 0x24C2),
    (0x25AA, 0x25AB),
    (0x25B6, 0x25B6),
    (0x25C0, 0x25C0),
    (0x25FB, 0x25FE),
    (0x2934, 0x2935),
    (0x2B05, 0x2B07),
    (0x2B50, 0x2B50),
    (0x2B55, 0x2B55),
    (0x3030, 0x3030),
    (0x303D, 0x303D),
    (0x3297, 0x3297),
    (0x3299, 0x3299),
];

const TEXT_PRESENTATION_SUPPLEMENTARY: &[(u16, u16)] = &[
    (0xDC04, 0xDC04),
    (0xDD70, 0xDD71),
    (0xDD7E, 0xDD7F),
    (0xDE02, 0xDE02),
    (0xDE1A, 0xDE1A),
    (0xDE2F, 0xDE2F),
];

const ENCLOSED_BADGES: &[(u16, u16)] = &[
    (0xDCCF, 0xDCCF),
    (0xDD8E, 0xDD8E),
    (0xDD91, 0xDD9A),
    (0xDE01, 0xDE01),
    (0xDE32, 0xDE3A),
    (0xDE50, 0xDE51),
];

const VARIATION_SELECTOR_16: u16 = 0xFE0F;
const ZERO_WIDTH_JOINER: u16 = 0x200D;
const COMBINING_KEYCAP: u16 = 0x20E3;

fn between(low: u16, high: u16) -> impl Fn(u16) -> bool {
    move |unit| (low..=high).contains(&unit)
}

impl Parser<'_> {
    pub(super) fn big_emoji(&mut self, position: usize) -> Parsed<Vec<Node>> {
        let mut cursor = self.emoji_separators(position);
        let mut emojis = Vec::new();
        while emojis.len() < 3 {
            let Some((end, node)) = self.emoji(cursor).or_else(|| self.emoticon(cursor)) else {
                break;
            };
            emojis.push(node);
            cursor = self.emoji_separators(end);
        }
        (!emojis.is_empty()).then(|| (cursor, vec![big_emoji(emojis)]))
    }

    fn emoji_separators(&mut self, position: usize) -> usize {
        let mut cursor = position;
        while let Some(end) = self.end_of_line(cursor).or_else(|| self.space(cursor)) {
            cursor = end;
        }
        cursor
    }

    pub(super) fn emoji(&mut self, position: usize) -> Parsed<Node> {
        memoized!(self, emoji, position, Flags::NONE, {
            self.emoji_short_code(position).or_else(|| {
                let end = self.unicode_emoji(position)?;
                Some((end, emoji_unicode(self.text(position, end))))
            })
        })
    }

    fn emoji_short_code(&mut self, position: usize) -> Parsed<Node> {
        let name_start = self.char(position, b':')?;
        let name_end = self.run1(name_start, is_emoji_short_code_char)?;
        let end = self.char(name_end, b':')?;
        Some((end, emoji(self.text(name_start, name_end))))
    }

    pub(super) fn emoticon(&mut self, position: usize) -> Parsed<Node> {
        if !self.options.emoticons {
            return None;
        }
        memoized!(self, emoticon, position, Flags::NONE, {
            let first = self.at(position)?;
            EMOTICONS
                .iter()
                .filter(|(text, _)| text.encode_utf16().next() == Some(first))
                .find_map(|(text, short_code)| {
                    let end = self.literal(position, text)?;
                    Some((end, emoticon(*text, *short_code)))
                })
        })
    }

    pub(super) fn emoticon_neighbor(&mut self, position: usize) -> bool {
        self.end_of_line(position).is_some()
            || self.space(position).is_some()
            || self.char(position, b'*').is_some()
            || self.is_end(position)
    }

    fn optional_variation_selector(&mut self, position: usize) -> usize {
        self.class(position, is_variation_selector)
            .unwrap_or(position)
    }

    fn surrogate_pair(
        &mut self,
        position: usize,
        high: u16,
        low: impl Fn(u16) -> bool,
    ) -> Option<usize> {
        let after_high = self.class(position, |unit| unit == high)?;
        self.class(after_high, low)
    }

    fn with_variation_selector(&mut self, end: Option<usize>) -> Option<usize> {
        end.map(|end| self.optional_variation_selector(end))
    }

    fn unicode_emoji(&mut self, position: usize) -> Option<usize> {
        self.emoji_tag_sequence(position)
            .or_else(|| self.emoji_keycap_sequence(position))
            .or_else(|| self.emoji_zwj_sequence(position))
            .or_else(|| self.emoji_emoticon(position))
            .or_else(|| self.emoji_transport(position))
            .or_else(|| self.emoji_bmp_block(position, 0x2300, 0x23FF))
            .or_else(|| self.emoji_bmp_block(position, 0x2600, 0x26FF))
            .or_else(|| self.emoji_bmp_block(position, 0x2700, 0x27BF))
            .or_else(|| self.emoji_geometric_squares(position))
            .or_else(|| self.emoji_enclosed_badges(position))
            .or_else(|| self.emoji_text_presentation(position))
            .or_else(|| self.emoji_flag(position))
    }

    fn emoji_tag_sequence(&mut self, position: usize) -> Option<usize> {
        let mut cursor = self.surrogate_pair(position, 0xD83C, |unit| unit == 0xDFF4)?;
        let mut tags = 0;
        while let Some(end) = self.surrogate_pair(cursor, 0xDB40, between(0xDC20, 0xDC7E)) {
            cursor = end;
            tags += 1;
        }
        if tags == 0 {
            return None;
        }
        self.surrogate_pair(cursor, 0xDB40, |unit| unit == 0xDC7F)
    }

    fn emoji_keycap_sequence(&mut self, position: usize) -> Option<usize> {
        let after_key = self.class(position, |unit| {
            is_ascii_digit(unit) || is(unit, b'#') || is(unit, b'*')
        })?;
        let after_selector = self
            .class(after_key, |unit| unit == VARIATION_SELECTOR_16)
            .unwrap_or(after_key);
        self.class(after_selector, |unit| unit == COMBINING_KEYCAP)
    }

    fn emoji_zwj_sequence(&mut self, position: usize) -> Option<usize> {
        let mut cursor = position;
        while let Some(component_end) = self.emoji_zwj_component(cursor) {
            match self.class(component_end, |unit| unit == ZERO_WIDTH_JOINER) {
                Some(joiner_end) => cursor = joiner_end,
                None => break,
            }
        }
        self.emoji_zwj_component(cursor)
    }

    fn emoji_zwj_component(&mut self, position: usize) -> Option<usize> {
        let end = self
            .surrogate_pair(position, 0xD83E, between(0xDD00, 0xDFFF))
            .or_else(|| self.emoji_miscellaneous_symbols_and_pictographs(position))
            .or_else(|| self.emoji_emoticon(position))
            .or_else(|| self.emoji_transport(position))
            .or_else(|| self.emoji_bmp_block(position, 0x2700, 0x27BF))
            .or_else(|| self.emoji_bmp_block(position, 0x2600, 0x26FF))
            .or_else(|| self.emoji_bmp_block(position, 0x2194, 0x2195))
            .or_else(|| self.emoji_geometric_squares(position))?;
        Some(
            self.surrogate_pair(end, 0xD83C, between(0xDFFB, 0xDFFF))
                .unwrap_or(end),
        )
    }

    fn emoji_miscellaneous_symbols_and_pictographs(&mut self, position: usize) -> Option<usize> {
        let end = self
            .surrogate_pair(position, 0xD83C, between(0xDF00, 0xDFFF))
            .or_else(|| self.surrogate_pair(position, 0xD83D, between(0xDC00, 0xDDFF)));
        self.with_variation_selector(end)
    }

    fn emoji_emoticon(&mut self, position: usize) -> Option<usize> {
        let end = self.surrogate_pair(position, 0xD83D, between(0xDE00, 0xDE4F));
        self.with_variation_selector(end)
    }

    fn emoji_transport(&mut self, position: usize) -> Option<usize> {
        let end = self.surrogate_pair(position, 0xD83D, between(0xDE80, 0xDEFF));
        self.with_variation_selector(end)
    }

    fn emoji_bmp_block(&mut self, position: usize, low: u16, high: u16) -> Option<usize> {
        let end = self.class(position, between(low, high));
        self.with_variation_selector(end)
    }

    fn emoji_geometric_squares(&mut self, position: usize) -> Option<usize> {
        let end = self.class(position, between(0x2B1B, 0x2B1C)).or_else(|| {
            self.surrogate_pair(position, 0xD83D, |unit| {
                (0xDFE0..=0xDFEB).contains(&unit) || unit == 0xDFF0
            })
        });
        self.with_variation_selector(end)
    }

    fn emoji_enclosed_badges(&mut self, position: usize) -> Option<usize> {
        let end = self.surrogate_pair(position, 0xD83C, |unit| in_table(ENCLOSED_BADGES, unit));
        self.with_variation_selector(end)
    }

    fn emoji_text_presentation(&mut self, position: usize) -> Option<usize> {
        let base = self
            .class(position, |unit| in_table(TEXT_PRESENTATION, unit))
            .and_then(|end| self.class(end, |unit| unit == VARIATION_SELECTOR_16));
        if base.is_some() {
            return base;
        }
        let supplementary = self.surrogate_pair(position, 0xD83C, |unit| {
            in_table(TEXT_PRESENTATION_SUPPLEMENTARY, unit)
        })?;
        self.class(supplementary, |unit| unit == VARIATION_SELECTOR_16)
    }

    fn emoji_flag(&mut self, position: usize) -> Option<usize> {
        let first = self.surrogate_pair(position, 0xD83C, between(0xDDE6, 0xDDFF))?;
        self.surrogate_pair(first, 0xD83C, between(0xDDE6, 0xDDFF))
    }
}
