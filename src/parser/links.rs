use super::chars::{
    is, is_alpha_digit, is_alpha_numeric_or_mark_char, is_any_text, is_extra, is_line_break,
    is_markdown_url_symbol, is_one_of, is_safe, is_space, is_url_symbol,
};
use super::memo::memoized;
use super::reduce::{reduce_plain_texts, Item};
use super::repetition::{Piece, Repetition, Run};
use super::{Flags, Parsed, Parser};
use crate::ast::{image, link, plain, Node};
use crate::tld;

fn phone_digits(phone_text: &str) -> String {
    phone_text.chars().filter(char::is_ascii_digit).collect()
}

pub(super) fn is_link_title_bulk_char(unit: u16) -> bool {
    !is_one_of(unit, b"]()*_~ \t\r\n")
}

fn is_link_title2_char(unit: u16) -> bool {
    unit >= 0x20 && !is_one_of(unit, b"<>|")
}

fn is_domain_forbidden(unit: u16) -> bool {
    is_one_of(unit, b"\\/|><%`[]")
}

fn is_auto_link_body_char(unit: u16) -> bool {
    !is_one_of(unit, b" \t\r\n.,!%~*\"':;()=")
}

fn has_absolute_scheme_prefix(source: &str) -> bool {
    let bytes = source.as_bytes();
    let Some(first) = bytes.first() else {
        return false;
    };
    if !first.is_ascii_alphabetic() {
        return false;
    }
    let scheme_length = bytes[1..]
        .iter()
        .take(31)
        .take_while(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'.' | b'-'))
        .count();
    bytes[1 + scheme_length..].starts_with(b"://")
}

pub(super) fn link_hosts(custom_domains: Option<&[String]>) -> Vec<String> {
    std::iter::once("localhost".to_owned())
        .chain(custom_domains.unwrap_or_default().iter().cloned())
        .collect()
}

fn auto_link(source: String, valid_hosts: &[String]) -> Node {
    if !tld::is_linkable(&source, true, valid_hosts) {
        return plain(source);
    }
    let is_absolute = url::Url::parse(&source).is_ok()
        || source.starts_with("//")
        || has_absolute_scheme_prefix(&source);
    let href = if is_absolute {
        source.clone()
    } else {
        format!("//{source}")
    };
    link(href, Some(vec![plain(source)]))
}

fn auto_email(source: String) -> Node {
    let href = format!("mailto:{source}");
    if !tld::is_linkable(&href, false, &[]) {
        return plain(source);
    }
    link(href, Some(vec![plain(source)]))
}

#[derive(Clone, Copy)]
pub(crate) enum Href {
    Source,
    Telephone,
}

impl Parser<'_> {
    pub(super) fn references(&mut self, position: usize, flags: Flags) -> Parsed<Node> {
        memoized!(self, references, position, flags, {
            self.markdown_reference(position, flags)
                .or_else(|| self.angle_reference(position))
        })
    }

    fn markdown_reference(&mut self, position: usize, flags: Flags) -> Parsed<Node> {
        let titles_start = self.char(position, b'[')?;
        let titles_end = self
            .repetition_end(Repetition::LinkTitle, titles_start, flags)
            .unwrap_or(titles_start);
        let href_start = self.literal(titles_end, "](")?;
        let (href_end, href) = self.markdown_link_ref(href_start)?;
        let end = self.char(href_end, b')')?;
        let href = self.resolve_href(href_start, href_end, href);
        let titles = self.repetition_items(Repetition::LinkTitle, titles_start, titles_end, flags);
        let label = (!titles.is_empty()).then(|| reduce_plain_texts(titles));
        Some((end, link(href, label)))
    }

    fn angle_reference(&mut self, position: usize) -> Parsed<Node> {
        let href_start = self.char(position, b'<')?;
        let (href_end, href) = self.link_ref(href_start)?;
        let title_start = self.char(href_end, b'|')?;
        let title_end = self.run1(title_start, is_link_title2_char)?;
        let end = self.char(title_end, b'>')?;
        let href = self.resolve_href(href_start, href_end, href);
        Some((
            end,
            link(href, Some(vec![plain(self.text(title_start, title_end))])),
        ))
    }

    pub(super) fn link_title(&mut self, position: usize, flags: Flags) -> Parsed<Piece> {
        if let Some((end, node)) = self.whitespace(position) {
            return Some((end, Piece::Item(Item::One(node))));
        }
        if let Some(parsed) = self.emphasis(position, flags) {
            return Some((parsed.0, Piece::Item(parsed.1)));
        }
        if let Some(end) = self.run_end(Run::LinkTitleText, position) {
            return Some((end, Piece::Text));
        }
        if self.literal(position, "](").is_some() && self.any_unit(position + 2).is_some() {
            return None;
        }
        if self.literal(position, "] [").is_some() {
            let label_end = self.run(position + 3, |unit| !is(unit, b']'));
            if self.literal(label_end, "](").is_some() {
                return None;
            }
        }
        let end = self.any_unit(position)?;
        Some((end, Piece::Text))
    }

    pub(super) fn image(&mut self, position: usize) -> Parsed<Node> {
        memoized!(self, image, position, Flags::NONE, {
            let mut cursor = self.literal(position, "![")?;
            let title = self.line(cursor).map(|(end, node)| {
                cursor = end;
                node
            });
            let href_start = self.literal(cursor, "](")?;
            let (href_end, href) = self.markdown_link_ref(href_start)?;
            let end = self.char(href_end, b')')?;
            let href = self.resolve_href(href_start, href_end, href);
            Some((end, image(href, title)))
        })
    }

    fn markdown_link_ref(&mut self, position: usize) -> Parsed<Href> {
        memoized!(self, markdown_link_ref, position, Flags::NONE, {
            self.url_with_body(position, Self::markdown_link_url_body)
                .or_else(|| self.file_path(position, Self::markdown_link_url_body))
                .or_else(|| self.phone_href(position))
        })
    }

    fn link_ref(&mut self, position: usize) -> Parsed<Href> {
        memoized!(self, link_ref, position, Flags::NONE, {
            self.url_with_body(position, Self::url_body)
                .or_else(|| self.file_path(position, Self::url_body))
                .or_else(|| self.phone_href(position))
        })
    }

    fn phone_href(&mut self, position: usize) -> Parsed<Href> {
        let end = self.phone(position)?;
        Some((end, Href::Telephone))
    }

    fn resolve_href(&mut self, start: usize, end: usize, href: Href) -> String {
        match href {
            Href::Source => self.text(start, end),
            Href::Telephone => format!("tel:{}", phone_digits(&self.text(start, end))),
        }
    }

    fn url_head(&mut self, position: usize) -> Option<usize> {
        if let Some(scheme_end) = self.url_scheme(position) {
            if let Some(end) = self.url_authority(scheme_end) {
                return Some(end);
            }
        }
        self.url_authority_host(position)
    }

    fn url_with_body(
        &mut self,
        position: usize,
        body: fn(&mut Self, usize) -> usize,
    ) -> Parsed<Href> {
        let head_end = self.url_head(position)?;
        let end = body(self, head_end);
        Some((end, Href::Source))
    }

    fn file_path(&mut self, position: usize, body: fn(&mut Self, usize) -> usize) -> Parsed<Href> {
        let scheme_end = self.url_scheme(position)?;
        let end = body(self, scheme_end);
        (end > scheme_end).then_some((end, Href::Source))
    }

    fn url_body(&mut self, position: usize) -> usize {
        self.run_end(Run::UrlBody, position).unwrap_or(position)
    }

    pub(super) fn url_body_step(&mut self, position: usize) -> Option<usize> {
        let extras_end = self.extra_run_end(position);
        let ends_url = extras_end > position
            && (self.space(extras_end).is_some()
                || self.end_of_line(extras_end).is_some()
                || self.is_end(extras_end));
        if ends_url || self.space(position).is_some() {
            return None;
        }
        self.class(position, |unit| is_any_text(unit) || is_url_symbol(unit))
    }

    fn markdown_link_url_body(&mut self, position: usize) -> usize {
        self.run_end(Run::MarkdownLinkUrlBody, position)
            .unwrap_or(position)
    }

    pub(super) fn markdown_link_url_body_step(&mut self, position: usize) -> Option<usize> {
        let extras_end = self
            .run_end(Run::MarkdownLinkExtras, position)
            .unwrap_or(position);
        let ends_url = extras_end > position
            && (self.space(extras_end).is_some() || self.end_of_line(extras_end).is_some());
        if ends_url || self.space(position).is_some() || self.char(position, b')').is_some() {
            return None;
        }
        self.class(position, |unit| {
            is_any_text(unit) || is_markdown_url_symbol(unit)
        })
        .or_else(|| self.markdown_link_parenthesized(position))
    }

    fn markdown_link_parenthesized(&mut self, position: usize) -> Option<usize> {
        let content_start = self.char(position, b'(')?;
        let content_end = self
            .run_end(Run::MarkdownLinkParenthesized, content_start)
            .unwrap_or(content_start);
        self.char(content_end, b')')
    }

    pub(super) fn markdown_link_parenthesized_step(&mut self, position: usize) -> Option<usize> {
        let stops = self.space(position).is_some()
            || self.end_of_line(position).is_some()
            || self.char(position, b')').is_some();
        if stops {
            return None;
        }
        self.class(position, |unit| {
            is_any_text(unit) || is_markdown_url_symbol(unit) || is(unit, b'(')
        })
    }

    fn url_scheme(&mut self, position: usize) -> Option<usize> {
        memoized!(self, url_scheme, position, Flags::NONE, {
            let mut cursor = position;
            while cursor - position < 32
                && self
                    .class(cursor, |unit| {
                        is_alpha_digit(unit) || is_one_of(unit, b"+-")
                    })
                    .is_some()
            {
                cursor += 1;
            }
            if cursor == position {
                return None;
            }
            self.char(cursor, b':').map(|end| (end, ()))
        })
        .map(|(end, ())| end)
    }

    fn url_authority(&mut self, position: usize) -> Option<usize> {
        let host_start = self.literal(position, "//")?;
        let host_start = self.url_user_info(host_start).unwrap_or(host_start);
        self.url_authority_host(host_start)
    }

    fn url_credential(&mut self, position: usize) -> Option<usize> {
        self.run1(position, |unit| {
            is_alpha_digit(unit) || (is_safe(unit) && !is(unit, b'@') && !is(unit, b'/'))
        })
    }

    fn url_user_info(&mut self, position: usize) -> Option<usize> {
        let mut cursor = self.url_credential(position)?;
        if let Some(password_start) = self.char(cursor, b':') {
            if let Some(end) = self.url_credential(password_start) {
                cursor = end;
            }
        }
        self.char(cursor, b'@')
    }

    fn url_authority_host(&mut self, position: usize) -> Option<usize> {
        memoized!(self, url_authority_host, position, Flags::NONE, {
            let mut cursor = self
                .domain_name(position)
                .or_else(|| self.dotted_digits(position))?;
            if let Some(port_start) = self.char(cursor, b':') {
                if let Some(end) = self.digits(port_start) {
                    cursor = end;
                }
            }
            Some((cursor, ()))
        })
        .map(|(end, ())| end)
    }

    fn dotted_digits(&mut self, position: usize) -> Option<usize> {
        let mut cursor = self.digits(position)?;
        for _ in 0..3 {
            let after_dot = self.char(cursor, b'.')?;
            cursor = self.digits(after_dot)?;
        }
        Some(cursor)
    }

    pub(super) fn domain_name(&mut self, position: usize) -> Option<usize> {
        memoized!(self, domain_name, position, Flags::NONE, {
            if let Some(end) = self.literal(position, "localhost") {
                return Some((end, ()));
            }
            if self.char(position, b'_').is_some() {
                return None;
            }
            let labels_end = self.domain_label(position)?;
            let end = self.run_end(Run::DomainGroups, labels_end)?;
            Some((end, ()))
        })
        .map(|(end, ())| end)
    }

    fn domain_label(&mut self, position: usize) -> Option<usize> {
        let mut cursor = self.domain_chars(position)?;
        while let Some(after_dash) = self.char(cursor, b'-') {
            match self.domain_chars(after_dash) {
                Some(end) => cursor = end,
                None => break,
            }
        }
        Some(cursor)
    }

    fn domain_chars(&mut self, position: usize) -> Option<usize> {
        self.run_end(Run::DomainChars, position)
    }

    pub(super) fn domain_group(&mut self, position: usize) -> Option<usize> {
        let after_dot = self.char(position, b'.')?;
        let mut group_end = self.domain_char(after_dot)?;
        while let Some(end) = self.domain_label(group_end) {
            group_end = end;
        }
        Some(group_end)
    }

    pub(super) fn domain_char(&mut self, position: usize) -> Option<usize> {
        let unit = self.at(position)?;
        if is_alpha_digit(unit) {
            return Some(position + 1);
        }
        if is_extra(unit) {
            return None;
        }
        let consumed_start = if is_one_of(unit, b"_-") {
            position + 1
        } else if is_safe(unit) {
            return None;
        } else {
            position
        };
        let consumed = self.at(consumed_start)?;
        if is_line_break(consumed) || is_space(consumed) || is_domain_forbidden(consumed) {
            return None;
        }
        Some(consumed_start + 1)
    }

    fn phone(&mut self, position: usize) -> Option<usize> {
        let number_start = self.char(position, b'+')?;
        self.phone_number(number_start)
    }

    fn phone_number(&mut self, position: usize) -> Option<usize> {
        if let Some(prefix_end) = self.phone_prefix(position) {
            if let Some(after_dash) = self.char(prefix_end, b'-') {
                if let Some(digits_end) = self.digits(after_dash) {
                    return Some(digits_end);
                }
            }
            if let Some(first_end) = self.digits(prefix_end) {
                let second_end = self
                    .char(first_end, b'-')
                    .and_then(|after_dash| self.digits(after_dash));
                return Some(second_end.unwrap_or(first_end));
            }
        }
        self.digits(position)
    }

    fn phone_prefix(&mut self, position: usize) -> Option<usize> {
        if let Some(end) = self.digits(position) {
            return Some(end);
        }
        let digits_start = self.char(position, b'(')?;
        let digits_end = self.digits(digits_start)?;
        self.char(digits_end, b')')
    }

    pub(super) fn autolinked_phone(&mut self, position: usize) -> Parsed<Node> {
        let end = self.phone(position)?;
        let phone_text = self.text(position, end);
        let number = phone_digits(&phone_text);
        if number.len() < 5 {
            return Some((end, plain(phone_text)));
        }
        Some((
            end,
            link(format!("tel:{number}"), Some(vec![plain(phone_text)])),
        ))
    }

    fn local_part_chars(&mut self, position: usize) -> Option<usize> {
        let mut cursor = position;
        while let Some(end) = self.run1(cursor, is_alpha_numeric_or_mark_char) {
            cursor = self.run(end, |unit| is_one_of(unit, b"!#$%&'*+/=?^_`{|}~-"));
        }
        (cursor > position).then_some(cursor)
    }

    fn local_part(&mut self, position: usize) -> Option<usize> {
        let mut cursor = self.local_part_chars(position)?;
        while let Some(after_dot) = self.char(cursor, b'.') {
            match self.local_part_chars(after_dot) {
                Some(end) => cursor = end,
                None => break,
            }
        }
        Some(cursor)
    }

    fn email(&mut self, position: usize) -> Parsed<String> {
        let address_start = self.literal(position, "mailto:").unwrap_or(position);
        let local_end = self.local_part(address_start)?;
        let domain_start = self.char(local_end, b'@')?;
        let end = self.domain_name(domain_start)?;
        Some((end, self.text(address_start, end)))
    }

    pub(super) fn autolinked_email(&mut self, position: usize) -> Parsed<Node> {
        if !self.email_guard(position) {
            return None;
        }
        memoized!(self, autolinked_email, position, Flags::NONE, {
            let (end, address) = self.email(position)?;
            Some((end, auto_email(address)))
        })
    }

    pub(super) fn plain_underscore_then_domain(&mut self, position: usize) -> Parsed<Node> {
        let domain_start = self.char(position, b'_')?;
        let end = self.domain_name(domain_start)?;
        if self.char(end, b'_').is_some() {
            return None;
        }
        Some((end, plain(self.text(position, end))))
    }

    pub(super) fn autolinked_url(&mut self, position: usize) -> Parsed<Node> {
        if !self.url_guard(position) {
            return None;
        }
        memoized!(self, autolinked_url, position, Flags::NONE, {
            let head_end = self.url_head(position)?;
            let end = self.auto_link_body(head_end);
            let source = self.text(position, end);
            Some((end, auto_link(source, &self.link_hosts)))
        })
    }

    fn auto_link_body(&mut self, position: usize) -> usize {
        let mut cursor = position;
        loop {
            let Some(unit) = self.at(cursor) else {
                break;
            };
            if is_auto_link_body_char(unit) {
                cursor += 1;
                continue;
            }
            let extras_end = self.extra_run_end(cursor);
            if self.space(extras_end).is_some()
                || self.end_of_line(extras_end).is_some()
                || self.is_end(extras_end)
            {
                break;
            }
            cursor += 1;
        }
        cursor
    }
}
