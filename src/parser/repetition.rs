use super::chars::{is, is_markdown_link_extra, is_space};
use super::inline::EmphasisContent;
use super::links::is_link_title_bulk_char;
use super::reduce::Item;
use super::{Flags, Parsed, Parser};
use crate::ast::plain;

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum Repetition {
    Bold,
    Italic,
    Strike,
    LinkTitle,
}

pub(super) enum Piece {
    Text,
    Item(Item),
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum Run {
    Marker(u8),
    Spaces,
    LinkTitleText,
    DomainChars,
    DomainGroups,
    UrlBody,
    MarkdownLinkUrlBody,
    MarkdownLinkExtras,
    MarkdownLinkParenthesized,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum Loop {
    Repetition(Repetition),
    Run(Run),
}

impl Repetition {
    fn piece(self, parser: &mut Parser, position: usize, flags: Flags) -> Parsed<Piece> {
        match self {
            Repetition::Bold => {
                parser.emphasis_content_item(position, flags, EmphasisContent::Bold)
            }
            Repetition::Italic => {
                parser.emphasis_content_item(position, flags, EmphasisContent::Italic)
            }
            Repetition::Strike => parser.strike_content_item(position, flags),
            Repetition::LinkTitle => parser.link_title(position, flags),
        }
    }
}

impl Run {
    fn step(self, parser: &mut Parser, position: usize) -> Option<usize> {
        match self {
            Run::Marker(marker) => parser.class(position, |unit| is(unit, marker)),
            Run::Spaces => parser.class(position, is_space),
            Run::LinkTitleText => parser.class(position, is_link_title_bulk_char),
            Run::DomainChars => parser.domain_char(position),
            Run::DomainGroups => parser.domain_group(position),
            Run::UrlBody => parser.url_body_step(position),
            Run::MarkdownLinkUrlBody => parser.markdown_link_url_body_step(position),
            Run::MarkdownLinkExtras => parser.class(position, is_markdown_link_extra),
            Run::MarkdownLinkParenthesized => parser.markdown_link_parenthesized_step(position),
        }
    }
}

impl Parser<'_> {
    fn steps_end(
        &mut self,
        kind: Loop,
        position: usize,
        flags: Flags,
        step: impl Fn(&mut Self, usize) -> Option<usize>,
    ) -> Option<usize> {
        let key = |cursor: usize| (cursor as u32, flags.bits(), kind);
        let mut visited = Vec::new();
        let mut cursor = position;
        let end = loop {
            self.report.rule_calls += 1;
            if let Some(&end) = self.memo.repetition_end.get(&key(cursor)) {
                break end as usize;
            }
            self.report.rule_evaluations += 1;
            visited.push(cursor);
            match step(self, cursor) {
                Some(next) => cursor = next,
                None => break cursor,
            }
        };
        for cursor in visited {
            self.memo.repetition_end.insert(key(cursor), end as u32);
        }
        (end > position).then_some(end)
    }

    pub(super) fn repetition_end(
        &mut self,
        repetition: Repetition,
        position: usize,
        flags: Flags,
    ) -> Option<usize> {
        self.steps_end(
            Loop::Repetition(repetition),
            position,
            flags,
            |parser, cursor| {
                repetition
                    .piece(parser, cursor, flags)
                    .map(|(next, _)| next)
            },
        )
    }

    pub(super) fn run_end(&mut self, run: Run, position: usize) -> Option<usize> {
        self.steps_end(Loop::Run(run), position, Flags::NONE, |parser, cursor| {
            run.step(parser, cursor)
        })
    }

    pub(super) fn repetition_items(
        &mut self,
        repetition: Repetition,
        position: usize,
        end: usize,
        flags: Flags,
    ) -> Vec<Item> {
        let mut items = Vec::new();
        let mut cursor = position;
        while cursor < end {
            let Some((next, piece)) = repetition.piece(self, cursor, flags) else {
                break;
            };
            items.push(match piece {
                Piece::Text => Item::One(plain(self.text(cursor, next))),
                Piece::Item(item) => item,
            });
            cursor = next;
        }
        items
    }
}
