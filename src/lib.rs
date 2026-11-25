pub mod ast;
mod parser;
pub mod tld;

pub use ast::Node;

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

pub fn parse(input: &str, options: &Options) -> Vec<Node> {
    parser::parse(input, options)
}
