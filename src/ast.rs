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
    Emoji {
        #[serde(skip_serializing_if = "Option::is_none")]
        value: Option<Box<Node>>,
        #[serde(rename = "shortCode", skip_serializing_if = "Option::is_none")]
        short_code: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        unicode: Option<String>,
    },
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
        #[serde(skip_serializing_if = "Option::is_none")]
        fallback: Option<SourceRange>,
    },
    Katex {
        value: String,
    },
    InlineKatex {
        value: String,
    },
    Timestamp {
        value: TimestampValue,
        #[serde(skip_serializing_if = "Option::is_none")]
        fallback: Option<SourceRange>,
    },
    Table {
        value: TableValue,
        #[serde(skip_serializing_if = "Option::is_none")]
        fallback: Option<SourceRange>,
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

fn serialize_js_number<S: Serializer>(number: &Option<f64>, serializer: S) -> Result<S::Ok, S::Error> {
    match number {
        Some(value) if value.fract() == 0.0 && value.abs() <= MAX_SAFE_INTEGER => {
            serializer.serialize_i64(*value as i64)
        }
        Some(value) => serializer.serialize_f64(*value),
        None => serializer.serialize_none(),
    }
}

pub fn plain(value: impl Into<String>) -> Node {
    Node::PlainText { value: value.into() }
}

pub fn paragraph(value: Vec<Node>) -> Node {
    Node::Paragraph { value }
}

pub fn bold(value: Vec<Node>) -> Node {
    Node::Bold { value }
}

pub fn italic(value: Vec<Node>) -> Node {
    Node::Italic { value }
}

pub fn strike(value: Vec<Node>) -> Node {
    Node::Strike { value }
}

pub fn spoiler(value: Vec<Node>) -> Node {
    Node::Spoiler { value }
}

pub fn spoiler_block(value: Vec<Node>) -> Node {
    Node::SpoilerBlock { value }
}

pub fn quote(value: Vec<Node>) -> Node {
    Node::Quote { value }
}

pub fn code(value: Vec<Node>, language: Option<String>) -> Node {
    Node::Code {
        language: language.filter(|language| !language.is_empty()).unwrap_or_else(|| "none".into()),
        value,
    }
}

pub fn code_line(value: Node) -> Node {
    Node::CodeLine { value: Box::new(value) }
}

pub fn inline_code(value: Node) -> Node {
    Node::InlineCode { value: Box::new(value) }
}

pub fn heading(value: Vec<Node>, level: u8) -> Node {
    Node::Heading { level, value }
}

pub fn link(src: impl Into<String>, label: Option<Vec<Node>>) -> Node {
    let src = src.into();
    let label = label.unwrap_or_else(|| vec![plain(src.clone())]);
    Node::Link {
        value: LinkValue { src: Box::new(plain(src)), label },
    }
}

pub fn image(src: impl Into<String>, label: Option<Node>) -> Node {
    let src = src.into();
    let label = label.unwrap_or_else(|| plain(src.clone()));
    Node::Image {
        value: ImageValue { src: Box::new(plain(src)), label: Box::new(label) },
    }
}

pub fn mention_user(value: impl Into<String>) -> Node {
    Node::MentionUser { value: Box::new(plain(value)) }
}

pub fn mention_channel(value: impl Into<String>) -> Node {
    Node::MentionChannel { value: Box::new(plain(value)) }
}

pub fn emoji(short_code: impl Into<String>) -> Node {
    let short_code = short_code.into();
    Node::Emoji {
        value: Some(Box::new(plain(short_code.clone()))),
        short_code: Some(short_code),
        unicode: None,
    }
}

pub fn emoji_unicode(unicode: impl Into<String>) -> Node {
    Node::Emoji { value: None, short_code: None, unicode: Some(unicode.into()) }
}

pub fn emoticon(text: impl Into<String>, short_code: impl Into<String>) -> Node {
    Node::Emoji {
        value: Some(Box::new(plain(text))),
        short_code: Some(short_code.into()),
        unicode: None,
    }
}

pub fn big_emoji(value: Vec<Node>) -> Node {
    Node::BigEmoji { value }
}

pub fn color(r: u8, g: u8, b: u8, a: u8) -> Node {
    Node::Color { value: ColorValue { r, g, b, a } }
}

pub fn tasks(value: Vec<Node>) -> Node {
    Node::Tasks { value }
}

pub fn task(value: Vec<Node>, status: bool, nested: Option<Vec<Node>>) -> Node {
    Node::Task { status, value, nested }
}

pub fn ordered_list(value: Vec<Node>) -> Node {
    Node::OrderedList { value }
}

pub fn unordered_list(value: Vec<Node>) -> Node {
    Node::UnorderedList { value }
}

pub fn list_item(value: Vec<Node>, number: Option<f64>, nested: Option<Vec<Node>>) -> Node {
    Node::ListItem { value, number, nested }
}

pub fn line_break() -> Node {
    Node::LineBreak
}

pub fn horizontal_rule(fallback: Option<SourceRange>) -> Node {
    Node::HorizontalRule { fallback }
}

pub fn katex(value: impl Into<String>) -> Node {
    Node::Katex { value: value.into() }
}

pub fn inline_katex(value: impl Into<String>) -> Node {
    Node::InlineKatex { value: value.into() }
}

pub fn timestamp(value: impl Into<String>, format: Option<char>, fallback: Option<SourceRange>) -> Node {
    Node::Timestamp {
        value: TimestampValue { timestamp: value.into(), format: format.unwrap_or('t') },
        fallback,
    }
}

pub fn table_cell(value: Vec<Node>, align: Option<TableAlign>) -> Node {
    Node::TableCell { align, value }
}

pub fn table_row(value: Vec<Node>) -> Node {
    Node::TableRow { value }
}

pub fn table(header: Vec<Node>, rows: Vec<Node>, fallback: Option<SourceRange>) -> Node {
    Node::Table { value: TableValue { header, rows }, fallback }
}
