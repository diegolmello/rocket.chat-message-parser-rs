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

use rustc_hash::FxHashMap;

use crate::ast::Node;
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

#[cfg(feature = "work-report")]
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
    link_hosts: Vec<String>,
}

struct Scans {
    email_stop: Vec<u32>,
    url_stop: Vec<u32>,
    block_katex_end: Vec<u32>,
    inline_katex_end: Vec<u32>,
    extra_run_end: Vec<u32>,
    long_code_closes: FxHashMap<usize, Vec<blocks::FenceClose>>,
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
            long_code_closes: blocks::long_code_closes(source),
        }
    }
}

impl<'source> Parser<'source> {
    pub(crate) fn new(input: &str, options: &'source Options, now: SystemTime) -> Self {
        let source: Vec<u16> = input.encode_utf16().collect();
        assert!(
            source.len() < u32::MAX as usize,
            "input must be shorter than u32::MAX UTF-16 code units"
        );
        let scans = Scans::new(&source, options);
        let link_hosts = links::link_hosts(options.custom_domains.as_deref());
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
            link_hosts,
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

    pub(crate) fn character(
        &mut self,
        position: usize,
        predicate: impl Fn(u16) -> bool,
    ) -> Option<usize> {
        let unit = self.at(position).filter(|&unit| predicate(unit))?;
        let after_unit = position + 1;
        let starts_pair = (0xD800..=0xDBFF).contains(&unit);
        if starts_pair && self.unit_is(after_unit, |unit| (0xDC00..=0xDFFF).contains(&unit)) {
            return Some(after_unit + 1);
        }
        Some(after_unit)
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

    pub(crate) fn text(&mut self, start: usize, end: usize) -> String {
        self.report.input_reads += (end - start) as u64;
        String::from_utf16_lossy(&self.source[start..end])
    }

    pub(crate) fn is_end(&mut self, position: usize) -> bool {
        self.at(position).is_none()
    }

    pub(crate) fn end_of_line(&mut self, position: usize) -> Option<usize> {
        self.report.input_reads += 1;
        let length = chars::line_break_length(self.source.get(position..)?)?;
        Some(position + length)
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

    fn scan(&mut self, table: fn(&Scans) -> &[u32], position: usize) -> usize {
        self.report.input_reads += 1;
        table(&self.scans)[position.min(self.len())] as usize
    }

    pub(crate) fn email_guard(&mut self, position: usize) -> bool {
        let stop = self.scan(|scans| &scans.email_stop, position);
        self.source.get(stop) == Some(&(b'@' as u16))
    }

    pub(crate) fn url_guard(&mut self, position: usize) -> bool {
        let stop = self.scan(|scans| &scans.url_stop, position);
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
        self.scan(|scans| &scans.block_katex_end, position)
    }

    pub(crate) fn inline_katex_end(&mut self, position: usize) -> usize {
        self.scan(|scans| &scans.inline_katex_end, position)
    }

    pub(crate) fn extra_run_end(&mut self, position: usize) -> usize {
        self.scan(|scans| &scans.extra_run_end, position)
    }
}

pub(crate) fn parse(input: &str, options: &Options, now: SystemTime) -> (Vec<Node>, ParseReport) {
    let mut parser = Parser::new(input, options, now);
    let nodes = parser.start();
    (nodes, parser.report)
}
