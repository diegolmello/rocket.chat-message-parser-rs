use serde::{Serialize, Serializer};

pub type SourceRange = [usize; 2];

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Node {
    Paragraph {
        value: Vec<Node>,
    },
    PlainText {
        value: String,
    },
    Bold {
        value: Vec<Node>,
    },
    Italic {
        value: Vec<Node>,
    },
    Strike {
        value: Vec<Node>,
    },
    Spoiler {
        value: Vec<Node>,
    },
    SpoilerBlock {
        value: Vec<Node>,
    },
    Quote {
        value: Vec<Node>,
    },
    Code {
        language: String,
        value: Vec<Node>,
    },
    CodeLine {
        value: Box<Node>,
    },
    InlineCode {
        value: Box<Node>,
    },
    Heading {
        level: u8,
        value: Vec<Node>,
    },
    Link {
        value: LinkValue,
    },
    Image {
        value: ImageValue,
    },
    MentionUser {
        value: Box<Node>,
    },
    MentionChannel {
        value: Box<Node>,
    },
    Emoji(Emoji),
    BigEmoji {
        value: Vec<Node>,
    },
    Color {
        value: ColorValue,
    },
    Tasks {
        value: Vec<Node>,
    },
    Task {
        status: bool,
        value: Vec<Node>,
        #[serde(skip_serializing_if = "Option::is_none")]
        nested: Option<Vec<Node>>,
    },
    OrderedList {
        value: Vec<Node>,
    },
    UnorderedList {
        value: Vec<Node>,
    },
    ListItem {
        value: Vec<Node>,
        #[serde(
            skip_serializing_if = "Option::is_none",
            serialize_with = "serialize_js_number"
        )]
        number: Option<f64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        nested: Option<Vec<Node>>,
    },
    LineBreak,
    HorizontalRule {
        fallback: SourceRange,
    },
    Katex {
        value: String,
    },
    InlineKatex {
        value: String,
    },
    Timestamp {
        value: TimestampValue,
        fallback: SourceRange,
    },
    Table {
        value: TableValue,
        fallback: SourceRange,
    },
    TableRow {
        value: Vec<Node>,
    },
    TableCell {
        #[serde(skip_serializing_if = "Option::is_none")]
        align: Option<TableAlign>,
        value: Vec<Node>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(untagged)]
pub enum Emoji {
    Unicode {
        unicode: String,
    },
    ShortCode {
        value: Box<Node>,
        #[serde(rename = "shortCode")]
        short_code: String,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LinkValue {
    pub src: Box<Node>,
    pub label: Vec<Node>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ImageValue {
    pub src: Box<Node>,
    pub label: Box<Node>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct ColorValue {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TimestampValue {
    pub timestamp: String,
    pub format: char,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TableValue {
    pub header: Vec<Node>,
    pub rows: Vec<Node>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum TableAlign {
    Left,
    Center,
    Right,
}

const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;

fn serialize_js_number<S: Serializer>(
    number: &Option<f64>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    match number {
        Some(value) if value.fract() == 0.0 && value.abs() <= MAX_SAFE_INTEGER => {
            serializer.serialize_i64(*value as i64)
        }
        Some(value) => serializer.serialize_f64(*value),
        None => serializer.serialize_none(),
    }
}

pub(crate) fn plain(value: impl Into<String>) -> Node {
    Node::PlainText {
        value: value.into(),
    }
}

pub(crate) fn paragraph(value: Vec<Node>) -> Node {
    Node::Paragraph { value }
}

pub(crate) fn bold(value: Vec<Node>) -> Node {
    Node::Bold { value }
}

pub(crate) fn italic(value: Vec<Node>) -> Node {
    Node::Italic { value }
}

pub(crate) fn strike(value: Vec<Node>) -> Node {
    Node::Strike { value }
}

pub(crate) fn spoiler(value: Vec<Node>) -> Node {
    Node::Spoiler { value }
}

pub(crate) fn spoiler_block(value: Vec<Node>) -> Node {
    Node::SpoilerBlock { value }
}

pub(crate) fn quote(value: Vec<Node>) -> Node {
    Node::Quote { value }
}

pub(crate) fn code(value: Vec<Node>, language: Option<String>) -> Node {
    Node::Code {
        language: language
            .filter(|language| !language.is_empty())
            .unwrap_or_else(|| "none".into()),
        value,
    }
}

pub(crate) fn code_line(value: Node) -> Node {
    Node::CodeLine {
        value: Box::new(value),
    }
}

pub(crate) fn inline_code(value: Node) -> Node {
    Node::InlineCode {
        value: Box::new(value),
    }
}

pub(crate) fn heading(value: Vec<Node>, level: u8) -> Node {
    Node::Heading { level, value }
}

pub(crate) fn link(src: impl Into<String>, label: Option<Vec<Node>>) -> Node {
    let src = src.into();
    let label = label.unwrap_or_else(|| vec![plain(src.clone())]);
    Node::Link {
        value: LinkValue {
            src: Box::new(plain(src)),
            label,
        },
    }
}

pub(crate) fn image(src: impl Into<String>, label: Option<Node>) -> Node {
    let src = src.into();
    let label = label.unwrap_or_else(|| plain(src.clone()));
    Node::Image {
        value: ImageValue {
            src: Box::new(plain(src)),
            label: Box::new(label),
        },
    }
}

pub(crate) fn mention_user(value: impl Into<String>) -> Node {
    Node::MentionUser {
        value: Box::new(plain(value)),
    }
}

pub(crate) fn mention_channel(value: impl Into<String>) -> Node {
    Node::MentionChannel {
        value: Box::new(plain(value)),
    }
}

pub(crate) fn emoji(short_code: impl Into<String>) -> Node {
    let short_code = short_code.into();
    Node::Emoji(Emoji::ShortCode {
        value: Box::new(plain(short_code.clone())),
        short_code,
    })
}

pub(crate) fn emoji_unicode(unicode: impl Into<String>) -> Node {
    Node::Emoji(Emoji::Unicode {
        unicode: unicode.into(),
    })
}

pub(crate) fn emoticon(text: impl Into<String>, short_code: impl Into<String>) -> Node {
    Node::Emoji(Emoji::ShortCode {
        value: Box::new(plain(text)),
        short_code: short_code.into(),
    })
}

pub(crate) fn big_emoji(value: Vec<Node>) -> Node {
    Node::BigEmoji { value }
}

pub(crate) fn color(r: u8, g: u8, b: u8, a: u8) -> Node {
    Node::Color {
        value: ColorValue { r, g, b, a },
    }
}

pub(crate) fn tasks(value: Vec<Node>) -> Node {
    Node::Tasks { value }
}

pub(crate) fn task(value: Vec<Node>, status: bool, nested: Option<Vec<Node>>) -> Node {
    Node::Task {
        status,
        value,
        nested,
    }
}

pub(crate) fn ordered_list(value: Vec<Node>) -> Node {
    Node::OrderedList { value }
}

pub(crate) fn unordered_list(value: Vec<Node>) -> Node {
    Node::UnorderedList { value }
}

pub(crate) fn list_item(value: Vec<Node>, number: Option<f64>, nested: Option<Vec<Node>>) -> Node {
    Node::ListItem {
        value,
        number,
        nested,
    }
}

pub(crate) fn line_break() -> Node {
    Node::LineBreak
}

pub(crate) fn horizontal_rule(fallback: SourceRange) -> Node {
    Node::HorizontalRule { fallback }
}

pub(crate) fn katex(value: impl Into<String>) -> Node {
    Node::Katex {
        value: value.into(),
    }
}

pub(crate) fn inline_katex(value: impl Into<String>) -> Node {
    Node::InlineKatex {
        value: value.into(),
    }
}

pub(crate) fn timestamp(
    value: impl Into<String>,
    format: Option<char>,
    fallback: SourceRange,
) -> Node {
    Node::Timestamp {
        value: TimestampValue {
            timestamp: value.into(),
            format: format.unwrap_or('t'),
        },
        fallback,
    }
}

pub(crate) fn table_cell(value: Vec<Node>, align: Option<TableAlign>) -> Node {
    Node::TableCell { align, value }
}

pub(crate) fn table_row(value: Vec<Node>) -> Node {
    Node::TableRow { value }
}

pub(crate) fn table(header: Vec<Node>, rows: Vec<Node>, fallback: SourceRange) -> Node {
    Node::Table {
        value: TableValue { header, rows },
        fallback,
    }
}
