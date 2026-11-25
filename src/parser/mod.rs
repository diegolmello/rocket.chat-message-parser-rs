mod blocks;
mod char_tables;
mod chars;
mod emoji;
mod inline;
mod links;
mod memo;
mod reduce;
mod repetition;
mod timestamp;

use std::time::SystemTime;

use crate::ast::{paragraph, plain, Node};
use crate::Options;
use chars::{is, is_ascii_digit, is_space};
use memo::Memo;

pub(crate) type Parsed<T> = Option<(usize, T)>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub(crate) struct Flags(u8);

impl Flags {
    pub(crate) const NONE: Flags = Flags(0);
    pub(crate) const BOLD: Flags = Flags(1);
    pub(crate) const ITALIC: Flags = Flags(2);
    pub(crate) const STRIKE: Flags = Flags(4);
    pub(crate) const REFERENCES: Flags = Flags(8);

    pub(crate) fn has(self, flag: Flags) -> bool {
        self.0 & flag.0 != 0
    }

    pub(crate) fn with(self, flag: Flags) -> Flags {
        Flags(self.0 | flag.0)
    }

    pub(crate) fn bits(self) -> u8 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ParseReport {
    pub input_length: usize,
    pub input_reads: u64,
    pub rule_calls: u64,
    pub rule_evaluations: u64,
    pub repeated_evaluations: u64,
}

impl ParseReport {
    pub fn work(&self) -> u64 {
        self.input_reads + self.rule_calls
    }
}

pub(crate) struct Parser<'source> {
    source: Vec<u16>,
    options: &'source Options,
    now: SystemTime,
    memo: Memo,
    report: ParseReport,
    scans: Scans,
}

struct Scans {
    email_stop: Vec<u32>,
    url_stop: Vec<u32>,
    block_katex_end: Vec<u32>,
    inline_katex_end: Vec<u32>,
    extra_run_end: Vec<u32>,
}

fn next_index_where(length: usize, matches: impl Fn(usize) -> bool) -> Vec<u32> {
    let mut next = vec![length as u32; length + 1];
    for index in (0..length).rev() {
        next[index] = if matches(index) {
            index as u32
        } else {
            next[index + 1]
        };
    }
    next
}

impl Scans {
    fn new(source: &[u16], options: &Options) -> Self {
        let length = source.len();
        let unit_at = |index: usize| source.get(index).copied().unwrap_or(0);
        let starts_with = |index: usize, text: &[u8]| {
            text.iter()
                .enumerate()
                .all(|(offset, &byte)| unit_at(index + offset) == byte as u16)
        };
        let dollar = options.katex.dollar_syntax;
        let parenthesis = options.katex.parenthesis_syntax;
        Scans {
            email_stop: next_index_where(length, |index| {
                chars::is_one_of(source[index], b" \t\r\n@")
            }),
            url_stop: next_index_where(length, |index| {
                chars::is_one_of(source[index], b" \t\r\n:./@")
            }),
            block_katex_end: next_index_where(length, |index| {
                (parenthesis && starts_with(index, b"\\]")) || (dollar && starts_with(index, b"$$"))
            }),
            inline_katex_end: next_index_where(length, |index| {
                (parenthesis && starts_with(index, b"\\)")) || (dollar && starts_with(index, b"$"))
            }),
            extra_run_end: next_index_where(length, |index| !chars::is_extra(source[index])),
        }
    }
}

impl<'source> Parser<'source> {
    pub(crate) fn new(input: &str, options: &'source Options, now: SystemTime) -> Self {
        let source: Vec<u16> = input.encode_utf16().collect();
        let scans = Scans::new(&source, options);
        let report = ParseReport {
            input_length: source.len(),
            ..ParseReport::default()
        };
        Parser {
            source,
            options,
            now,
            memo: Memo::default(),
            report,
            scans,
        }
    }

    pub(crate) fn len(&self) -> usize {
        self.source.len()
    }

    pub(crate) fn at(&mut self, position: usize) -> Option<u16> {
        self.report.input_reads += 1;
        self.source.get(position).copied()
    }

    pub(crate) fn unit_is(&mut self, position: usize, predicate: impl Fn(u16) -> bool) -> bool {
        self.at(position).is_some_and(predicate)
    }

    pub(crate) fn class(
        &mut self,
        position: usize,
        predicate: impl Fn(u16) -> bool,
    ) -> Option<usize> {
        self.unit_is(position, predicate).then_some(position + 1)
    }

    pub(crate) fn char(&mut self, position: usize, ascii: u8) -> Option<usize> {
        self.class(position, |unit| is(unit, ascii))
    }

    pub(crate) fn literal(&mut self, position: usize, text: &str) -> Option<usize> {
        let mut cursor = position;
        for unit in text.encode_utf16() {
            if self.at(cursor)? != unit {
                return None;
            }
            cursor += 1;
        }
        Some(cursor)
    }

    pub(crate) fn run(&mut self, position: usize, predicate: impl Fn(u16) -> bool) -> usize {
        let mut cursor = position;
        while self.unit_is(cursor, &predicate) {
            cursor += 1;
        }
        cursor
    }

    pub(crate) fn run1(
        &mut self,
        position: usize,
        predicate: impl Fn(u16) -> bool,
    ) -> Option<usize> {
        let end = self.run(position, predicate);
        (end > position).then_some(end)
    }

    pub(crate) fn text(&self, start: usize, end: usize) -> String {
        String::from_utf16_lossy(&self.source[start..end])
    }

    pub(crate) fn is_end(&mut self, position: usize) -> bool {
        self.at(position).is_none()
    }

    pub(crate) fn end_of_line(&mut self, position: usize) -> Option<usize> {
        match self.at(position)? {
            0x0D if self.at(position + 1) == Some(0x0A) => Some(position + 2),
            0x0D | 0x0A => Some(position + 1),
            _ => None,
        }
    }

    pub(crate) fn space(&mut self, position: usize) -> Option<usize> {
        self.class(position, is_space)
    }

    pub(crate) fn spaces(&mut self, position: usize) -> usize {
        self.run(position, is_space)
    }

    pub(crate) fn digits(&mut self, position: usize) -> Option<usize> {
        self.run1(position, is_ascii_digit)
    }

    pub(crate) fn exact_digits(&mut self, position: usize, count: usize) -> Option<usize> {
        (0..count).try_fold(position, |cursor, _| self.class(cursor, is_ascii_digit))
    }

    pub(crate) fn any(&mut self, position: usize) -> Option<usize> {
        self.class(position, |unit| unit != 0x0A && unit != 0x0D)
    }

    pub(crate) fn any_unit(&mut self, position: usize) -> Option<usize> {
        self.at(position).map(|_| position + 1)
    }

    pub(crate) fn email_guard(&mut self, position: usize) -> bool {
        self.report.input_reads += 1;
        let stop = self.scans.email_stop[position.min(self.len())] as usize;
        self.source.get(stop) == Some(&(b'@' as u16))
    }

    pub(crate) fn url_guard(&mut self, position: usize) -> bool {
        self.report.input_reads += 1;
        let stop = self.scans.url_stop[position.min(self.len())] as usize;
        match self.source.get(stop) {
            Some(&unit) if is(unit, b'.') => true,
            Some(&unit) if is(unit, b':') => {
                self.source.get(stop + 1) == Some(&(b'/' as u16))
                    && self.source.get(stop + 2) == Some(&(b'/' as u16))
            }
            _ => false,
        }
    }

    pub(crate) fn block_katex_end(&mut self, position: usize) -> usize {
        self.report.input_reads += 1;
        self.scans.block_katex_end[position.min(self.len())] as usize
    }

    pub(crate) fn inline_katex_end(&mut self, position: usize) -> usize {
        self.report.input_reads += 1;
        self.scans.inline_katex_end[position.min(self.len())] as usize
    }

    pub(crate) fn extra_run_end(&mut self, position: usize) -> usize {
        self.report.input_reads += 1;
        self.scans.extra_run_end[position.min(self.len())] as usize
    }

    fn start(&mut self) -> Vec<Node> {
        if let Some((end, nodes)) = self.big_emoji(0) {
            if end == self.len() {
                return nodes;
            }
        }
        let mut nodes = Vec::new();
        let mut position = 0;
        loop {
            if let Some((end, node)) = self.blocks(position) {
                nodes.push(node);
                position = end;
            } else if let Some((end, node)) = self.paragraph(position) {
                nodes.push(node);
                position = end;
            } else if let Some(end) = self.end_of_line(position) {
                nodes.push(paragraph(vec![plain("")]));
                position = end;
            } else {
                break;
            }
        }
        nodes
    }
}

pub(crate) fn parse(input: &str, options: &Options, now: SystemTime) -> (Vec<Node>, ParseReport) {
    let mut parser = Parser::new(input, options, now);
    let nodes = parser.start();
    let mut report = parser.report;
    report.repeated_evaluations = parser.memo.repeated_evaluations();
    (nodes, report)
}
