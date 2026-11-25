use super::char_tables::{ALPHA_CHAR, DECIMAL_NUMBER_CHAR, EMOJI_CHAR, MARK_CHAR};

pub(crate) fn in_table(table: &[(u16, u16)], unit: u16) -> bool {
    table
        .binary_search_by(|&(low, high)| {
            if high < unit {
                std::cmp::Ordering::Less
            } else if low > unit {
                std::cmp::Ordering::Greater
            } else {
                std::cmp::Ordering::Equal
            }
        })
        .is_ok()
}

pub(crate) const fn is(unit: u16, ascii: u8) -> bool {
    unit == ascii as u16
}

pub(crate) fn is_one_of(unit: u16, set: &[u8]) -> bool {
    unit < 0x80 && set.contains(&(unit as u8))
}

pub(crate) fn is_ascii_digit(unit: u16) -> bool {
    (b'0' as u16..=b'9' as u16).contains(&unit)
}

pub(crate) fn is_ascii_alpha(unit: u16) -> bool {
    (b'a' as u16..=b'z' as u16).contains(&unit) || (b'A' as u16..=b'Z' as u16).contains(&unit)
}

pub(crate) fn is_alpha_digit(unit: u16) -> bool {
    is_ascii_alpha(unit) || is_ascii_digit(unit)
}

pub(crate) fn is_hex_digit(unit: u16) -> bool {
    is_ascii_digit(unit)
        || (b'a' as u16..=b'f' as u16).contains(&unit)
        || (b'A' as u16..=b'F' as u16).contains(&unit)
}

pub(crate) fn is_space(unit: u16) -> bool {
    is_one_of(unit, b" \t")
}

pub(crate) fn is_non_ascii(unit: u16) -> bool {
    unit >= 0x80
}

pub(crate) fn is_any_text(unit: u16) -> bool {
    matches!(unit, 0x20..=0x27 | 0x2B..=0x5A | 0x61..=0x7A) || is_non_ascii(unit)
}

pub(crate) fn is_alpha_char(unit: u16) -> bool {
    in_table(ALPHA_CHAR, unit)
}

pub(crate) fn is_decimal_number_char(unit: u16) -> bool {
    in_table(DECIMAL_NUMBER_CHAR, unit)
}

pub(crate) fn is_alpha_numeric_char(unit: u16) -> bool {
    is_alpha_char(unit) || is_decimal_number_char(unit)
}

pub(crate) fn is_alpha_numeric_or_mark_char(unit: u16) -> bool {
    is_alpha_char(unit)
        || in_table(EMOJI_CHAR, unit)
        || in_table(MARK_CHAR, unit)
        || is_decimal_number_char(unit)
}

pub(crate) fn is_extra(unit: u16) -> bool {
    is_one_of(unit, b".,!%~*\"':;()=")
}

pub(crate) fn is_markdown_link_extra(unit: u16) -> bool {
    is_one_of(unit, b".,!%*\"':;=")
}

pub(crate) fn is_safe(unit: u16) -> bool {
    is_one_of(unit, b"$@&+_#?-")
}

pub(crate) fn is_url_symbol(unit: u16) -> bool {
    is_one_of(unit, b"*[/]^_`{}~(")
}

pub(crate) fn is_markdown_url_symbol(unit: u16) -> bool {
    is_one_of(unit, b"*[/]^_`{}~")
}

pub(crate) fn is_names_char(unit: u16) -> bool {
    is_one_of(unit, b"-_.") || is_alpha_numeric_char(unit)
}

pub(crate) fn is_emoji_short_code_char(unit: u16) -> bool {
    is_alpha_digit(unit) || is_one_of(unit, b"-_+.")
}

pub(crate) fn is_code_language_char(unit: u16) -> bool {
    is_alpha_digit(unit) || is_one_of(unit, b" _-.")
}

pub(crate) fn is_variation_selector(unit: u16) -> bool {
    (0xFE00..=0xFE0F).contains(&unit)
}

pub(crate) fn is_js_whitespace(character: char) -> bool {
    matches!(
        character,
        '\t' | '\n' | '\u{0B}' | '\u{0C}' | '\r' | ' ' | '\u{A0}' | '\u{1680}' | '\u{2000}'
            ..='\u{200A}'
                | '\u{2028}'
                | '\u{2029}'
                | '\u{202F}'
                | '\u{205F}'
                | '\u{3000}'
                | '\u{FEFF}'
    )
}
