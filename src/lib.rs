pub mod ast;
mod parser;
pub mod tld;

use std::time::SystemTime;

pub use ast::Node;
pub use parser::ParseReport;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct KatexOptions {
    pub dollar_syntax: bool,
    pub parenthesis_syntax: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Options {
    pub colors: bool,
    pub emoticons: bool,
    pub katex: KatexOptions,
    pub custom_domains: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ParseOutcome {
    pub nodes: Vec<Node>,
    pub report: ParseReport,
}

pub fn parse(input: &str, options: &Options) -> Vec<Node> {
    parse_at(input, options, SystemTime::now()).nodes
}

pub fn parse_at(input: &str, options: &Options, now: SystemTime) -> ParseOutcome {
    let (nodes, report) = parser::parse(input, options, now);
    ParseOutcome { nodes, report }
}
