use super::chars::{is, is_code_language_char, is_js_whitespace};
use super::memo::memoized;
use super::reduce::{reduce_plain_texts, Item};
use super::{Flags, Parsed, Parser};
use crate::ast::{
    code, code_line, heading, horizontal_rule, katex, line_break, list_item, ordered_list,
    paragraph, plain, quote, spoiler_block, table, table_cell, table_row, task, tasks,
    unordered_list, Node, TableAlign,
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum ListMarker {
    Task,
    Ordered,
    Hyphen,
    Asterisk,
}

const LIST_MARKERS: [ListMarker; 4] = [
    ListMarker::Task,
    ListMarker::Ordered,
    ListMarker::Hyphen,
    ListMarker::Asterisk,
];

#[derive(Clone)]
pub(super) struct NestedLine {
    width: usize,
    marker: ListMarker,
    item: Node,
}

const MAXIMUM_NESTED_LEVELS: usize = 63;

fn indent_width(indent: &[u16]) -> usize {
    indent.iter().fold(0, |width, &unit| {
        if is(unit, b'\t') {
            width + 4 - width % 4
        } else {
            width + 1
        }
    })
}

fn attach_nested(item: &mut Node, nested_lists: Vec<Node>) {
    if let Node::ListItem { nested, .. } | Node::Task { nested, .. } = item {
        *nested = Some(nested_lists);
    }
}

fn list_of(marker: ListMarker, items: Vec<Node>) -> Node {
    match marker {
        ListMarker::Task => tasks(items),
        ListMarker::Ordered => ordered_list(items),
        ListMarker::Hyphen | ListMarker::Asterisk => unordered_list(items),
    }
}

struct NestingLevel {
    parent_width: Option<usize>,
    lists: Vec<Node>,
    marker: ListMarker,
    items: Vec<Node>,
    last_item: Node,
    last_width: usize,
}

impl NestingLevel {
    fn new(parent_width: Option<usize>, line: NestedLine) -> Self {
        NestingLevel {
            parent_width,
            lists: Vec::new(),
            marker: line.marker,
            items: Vec::new(),
            last_item: line.item,
            last_width: line.width,
        }
    }

    fn holds(&self, width: usize) -> bool {
        self.parent_width
            .is_none_or(|parent_width| width > parent_width)
    }

    fn push_item(&mut self, line: NestedLine) {
        let previous = std::mem::replace(&mut self.last_item, line.item);
        self.items.push(previous);
        if line.marker != self.marker {
            let items = std::mem::take(&mut self.items);
            self.lists.push(list_of(self.marker, items));
            self.marker = line.marker;
        }
        self.last_width = line.width;
    }

    fn finish(mut self) -> Vec<Node> {
        self.items.push(self.last_item);
        self.lists.push(list_of(self.marker, self.items));
        self.lists
    }
}

fn close_level(levels: &mut Vec<NestingLevel>) {
    let closed = levels.pop().expect("a nested level is open").finish();
    let parent = levels.last_mut().expect("a nested level has a parent");
    attach_nested(&mut parent.last_item, closed);
}

fn group_nested_lines(lines: Vec<NestedLine>) -> Vec<Node> {
    let mut lines = lines.into_iter();
    let Some(first) = lines.next() else {
        return Vec::new();
    };
    let mut levels = vec![NestingLevel::new(None, first)];
    for line in lines {
        while !levels
            .last()
            .expect("the outermost level stays open")
            .holds(line.width)
        {
            close_level(&mut levels);
        }
        let below_depth_limit = levels.len() < MAXIMUM_NESTED_LEVELS;
        let current = levels.last_mut().expect("the outermost level stays open");
        if line.width > current.last_width && below_depth_limit {
            let parent_width = Some(current.last_width);
            levels.push(NestingLevel::new(parent_width, line));
        } else {
            current.push_item(line);
        }
    }
    while levels.len() > 1 {
        close_level(&mut levels);
    }
    levels
        .pop()
        .expect("the outermost level stays open")
        .finish()
}

fn trim_cell_content(mut value: Vec<Node>) -> Vec<Node> {
    if let Some(Node::PlainText { value: text }) = value.first() {
        let trimmed = text.trim_start_matches(is_js_whitespace).to_string();
        if trimmed.is_empty() {
            value.remove(0);
        } else {
            value[0] = plain(trimmed);
        }
    }
    if let Some(Node::PlainText { value: text }) = value.last() {
        let trimmed = text.trim_end_matches(is_js_whitespace).to_string();
        let last = value.len() - 1;
        if trimmed.is_empty() {
            value.pop();
        } else {
            value[last] = plain(trimmed);
        }
    }
    value
}

fn build_table(
    header: Vec<Vec<Node>>,
    aligns: Vec<Option<TableAlign>>,
    rows: Vec<Vec<Vec<Node>>>,
    range: [usize; 2],
) -> Node {
    let align_at = |index: usize| aligns.get(index).copied().flatten();
    let column_count = header.len();
    let header_cells = header
        .into_iter()
        .enumerate()
        .map(|(index, cell)| table_cell(trim_cell_content(cell), align_at(index)))
        .collect();
    let body_rows = rows
        .into_iter()
        .map(|mut cells| {
            cells.resize_with(column_count.max(cells.len()), Vec::new);
            table_row(
                cells
                    .into_iter()
                    .take(column_count)
                    .enumerate()
                    .map(|(index, cell)| table_cell(trim_cell_content(cell), align_at(index)))
                    .collect(),
            )
        })
        .collect();
    table(header_cells, body_rows, Some(range))
}

impl Parser<'_> {
    pub(super) fn start(&mut self) -> Vec<Node> {
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

    pub(super) fn blocks(&mut self, position: usize) -> Parsed<Node> {
        self.blockquote(position)
            .or_else(|| self.block_spoiler(position))
            .or_else(|| self.long_code(position))
            .or_else(|| self.code(position))
            .or_else(|| self.horizontal_rule(position))
            .or_else(|| self.table(position))
            .or_else(|| self.heading(position))
            .or_else(|| self.list(position))
            .or_else(|| self.katex(position))
            .or_else(|| self.line_break(position))
    }

    pub(super) fn paragraph(&mut self, position: usize) -> Parsed<Node> {
        self.inline(position)
            .map(|(end, value)| (end, paragraph(value)))
    }

    pub(super) fn inline(&mut self, position: usize) -> Parsed<Vec<Node>> {
        memoized!(
            self,
            inline,
            position,
            Flags::NONE,
            self.inline_uncached(position)
        )
    }

    fn inline_uncached(&mut self, position: usize) -> Parsed<Vec<Node>> {
        let (mut cursor, items) = self.inline_items(position, Flags::NONE)?;
        if let Some(end) = self.end_of_line(cursor) {
            cursor = end;
        }
        Some((cursor, reduce_plain_texts(items)))
    }

    pub(super) fn inline_items(&mut self, position: usize, flags: Flags) -> Parsed<Vec<Item>> {
        let mut items = Vec::new();
        let mut cursor = position;
        while let Some((end, item)) = self.inline_item_or_any(cursor, flags) {
            items.push(item);
            cursor = end;
        }
        (!items.is_empty()).then_some((cursor, items))
    }

    pub(super) fn inline_item_or_any(&mut self, position: usize, flags: Flags) -> Parsed<Item> {
        self.inline_item_pattern(position, flags).or_else(|| {
            let end = self.any(position)?;
            Some((end, Item::One(plain(self.text(position, end)))))
        })
    }

    fn blockquote(&mut self, position: usize) -> Parsed<Node> {
        let mut lines = Vec::new();
        let mut cursor = position;
        while let Some((end, line)) = self.blockquote_line(cursor) {
            lines.push(line);
            cursor = end;
        }
        (!lines.is_empty()).then(|| (cursor, quote(lines)))
    }

    fn blockquote_line(&mut self, position: usize) -> Parsed<Node> {
        let after_marker = self.char(position, b'>')?;
        let content_start = self.spaces(after_marker);
        if let Some(end) = self.end_of_line(content_start) {
            return Some((end, paragraph(vec![plain("")])));
        }
        self.paragraph(content_start)
    }

    fn block_spoiler(&mut self, position: usize) -> Parsed<Node> {
        let after_open = self.literal(position, "||")?;
        let mut cursor = self.end_of_line(after_open)?;
        let mut paragraphs = Vec::new();
        loop {
            if self.literal(cursor, "||").is_some() {
                break;
            }
            let Some((end, line)) = self.paragraph(cursor) else {
                break;
            };
            paragraphs.push(line);
            cursor = end;
        }
        if paragraphs.is_empty() {
            return None;
        }
        if let Some(end) = self.end_of_line(cursor) {
            cursor = end;
        }
        let end = self.literal(cursor, "||")?;
        Some((end, spoiler_block(paragraphs)))
    }

    fn code_language(&mut self, position: usize) -> (usize, Option<String>) {
        match self.run1(position, is_code_language_char) {
            Some(end) => (end, Some(self.text(position, end))),
            None => (position, None),
        }
    }

    fn long_code(&mut self, position: usize) -> Parsed<Node> {
        let after_minimum_fence = self.literal(position, "````")?;
        let fence_end = self.run(after_minimum_fence, |unit| is(unit, b'`'));
        let fence_length = fence_end - position;
        let (after_language, language) = self.code_language(fence_end);
        let mut cursor = self.end_of_line(after_language)?;
        let mut lines = Vec::new();
        loop {
            if self.long_code_close(cursor, fence_length).is_some() {
                break;
            }
            let text_end = self.run(cursor, |unit| unit != 0x0A && unit != 0x0D);
            let Some(line_end) = self.end_of_line(text_end) else {
                break;
            };
            lines.push(code_line(plain(self.text(cursor, text_end))));
            cursor = line_end;
        }
        if lines.is_empty() {
            return None;
        }
        let end = self.long_code_close(cursor, fence_length)?;
        Some((end, code(lines, language)))
    }

    fn long_code_close(&mut self, position: usize, fence_length: usize) -> Option<usize> {
        let fence_end = self.run1(position, |unit| is(unit, b'`'))?;
        if fence_end - position != fence_length {
            return None;
        }
        let end = self.spaces(fence_end);
        (self.end_of_line(end).is_some() || self.is_end(end)).then_some(end)
    }

    fn code(&mut self, position: usize) -> Parsed<Node> {
        let after_fence = self.literal(position, "```")?;
        let (after_language, language) = self.code_language(after_fence);
        let mut cursor = self.end_of_line(after_language)?;
        let mut lines = Vec::new();
        while let Some((end, line)) = self.code_line(cursor) {
            lines.push(line);
            cursor = end;
        }
        if lines.is_empty() {
            return None;
        }
        let after_newline = self.end_of_line(cursor)?;
        let end = self.literal(after_newline, "```")?;
        Some((end, code(lines, language)))
    }

    fn code_line(&mut self, position: usize) -> Parsed<Node> {
        if let Some((end, chunk)) = self.code_chunk(position) {
            return Some((end, code_line(chunk)));
        }
        let after_newline = self.char(position, b'\n')?;
        if let Some((end, chunk)) = self.code_chunk(after_newline) {
            return Some((end, code_line(chunk)));
        }
        if self.literal(after_newline, "```").is_some() {
            return None;
        }
        Some((after_newline, code_line(plain(""))))
    }

    fn code_chunk_char(&mut self, position: usize) -> Option<usize> {
        let is_line_end_or_fence = |unit: u16| is(unit, b'`') || unit == 0x0A || unit == 0x0D;
        let is_line_end = |unit: Option<u16>| matches!(unit, None | Some(0x0A) | Some(0x0D));
        let unit = self.at(position)?;
        if !is_line_end_or_fence(unit) {
            return Some(position + 1);
        }
        if !is(unit, b'`') {
            return None;
        }
        let second = self.at(position + 1);
        match second {
            Some(next) if !is_line_end_or_fence(next) => Some(position + 2),
            Some(next) if is(next, b'`') => {
                let third = self.at(position + 2);
                match third {
                    Some(last) if !is_line_end_or_fence(last) => Some(position + 3),
                    _ if is_line_end(third) => Some(position + 2),
                    _ => None,
                }
            }
            _ if is_line_end(second) => Some(position + 1),
            _ => None,
        }
    }

    fn code_chunk(&mut self, position: usize) -> Parsed<Node> {
        let mut cursor = position;
        while let Some(end) = self.code_chunk_char(cursor) {
            cursor = end;
        }
        (cursor > position).then(|| (cursor, plain(self.text(position, cursor))))
    }

    fn horizontal_rule(&mut self, position: usize) -> Parsed<Node> {
        let markers_start = self.spaces(position);
        let markers_end = self.run(markers_start, |unit| is(unit, b'-'));
        if markers_end - markers_start < 3 {
            return None;
        }
        let trailing_end = self.spaces(markers_end);
        let end = match self.end_of_line(trailing_end) {
            Some(end) => end,
            None if self.is_end(trailing_end) => trailing_end,
            None => return None,
        };
        Some((end, horizontal_rule(Some([markers_start, markers_end]))))
    }

    fn table(&mut self, position: usize) -> Parsed<Node> {
        let (after_header, header) = self.table_row_line(position)?;
        let (mut cursor, aligns) = self.table_delimiter_row(after_header)?;
        let mut rows = Vec::new();
        while let Some((end, row)) = self.table_row_line(cursor) {
            rows.push(row);
            cursor = end;
        }
        Some((
            cursor,
            build_table(header, aligns, rows, [position, cursor]),
        ))
    }

    fn table_row_line(&mut self, position: usize) -> Parsed<Vec<Vec<Node>>> {
        let mut cursor = self.char(position, b'|')?;
        let mut cells = Vec::new();
        loop {
            let (cell_end, cell) = self.table_cell(cursor);
            let Some(after_pipe) = self.char(cell_end, b'|') else {
                break;
            };
            cells.push(cell);
            cursor = after_pipe;
        }
        if cells.is_empty() {
            return None;
        }
        if let Some(end) = self.end_of_line(cursor) {
            cursor = end;
        }
        Some((cursor, cells))
    }

    fn table_cell(&mut self, position: usize) -> (usize, Vec<Node>) {
        let mut items = Vec::new();
        let mut cursor = position;
        loop {
            if let Some(end) = self.literal(cursor, "\\|") {
                items.push(Item::One(plain("|")));
                cursor = end;
                continue;
            }
            if self.char(cursor, b'|').is_some() || self.end_of_line(cursor).is_some() {
                break;
            }
            let Some((end, item)) = self.inline_item_or_any(cursor, Flags::NONE) else {
                break;
            };
            items.push(item);
            cursor = end;
        }
        (cursor, reduce_plain_texts(items))
    }

    fn table_delimiter_row(&mut self, position: usize) -> Parsed<Vec<Option<TableAlign>>> {
        let mut cursor = self.char(position, b'|')?;
        let mut aligns = Vec::new();
        while let Some((cell_end, align)) = self.table_delimiter_cell(cursor) {
            let Some(after_pipe) = self.char(cell_end, b'|') else {
                break;
            };
            aligns.push(align);
            cursor = after_pipe;
        }
        if aligns.is_empty() {
            return None;
        }
        if let Some(end) = self.end_of_line(cursor) {
            cursor = end;
        }
        Some((cursor, aligns))
    }

    fn table_delimiter_cell(&mut self, position: usize) -> Parsed<Option<TableAlign>> {
        let mut cursor = self.spaces(position);
        let left = self.char(cursor, b':');
        if let Some(end) = left {
            cursor = end;
        }
        cursor = self.run1(cursor, |unit| is(unit, b'-'))?;
        let right = self.char(cursor, b':');
        if let Some(end) = right {
            cursor = end;
        }
        let end = self.spaces(cursor);
        let align = match (left.is_some(), right.is_some()) {
            (true, true) => Some(TableAlign::Center),
            (_, true) => Some(TableAlign::Right),
            (true, _) => Some(TableAlign::Left),
            _ => None,
        };
        Some((end, align))
    }

    fn heading(&mut self, position: usize) -> Parsed<Node> {
        let mut level_end = position;
        while level_end - position < 4 && self.char(level_end, b'#').is_some() {
            level_end += 1;
        }
        if level_end == position {
            return None;
        }
        let text_start = self.run1(level_end, super::chars::is_space)?;
        let (end, text) = self.inline(text_start)?;
        Some((end, heading(text, (level_end - position) as u8)))
    }

    fn task_flag(&mut self, position: usize) -> Parsed<bool> {
        match self.at(position)? {
            unit if is(unit, b'x') => Some((position + 1, true)),
            unit if is(unit, b' ') => Some((position + 1, false)),
            _ => None,
        }
    }

    fn task_head(&mut self, position: usize) -> Parsed<bool> {
        let after_open = self.literal(position, "- [")?;
        let (after_flag, status) = self.task_flag(after_open)?;
        let after_close = self.char(after_flag, b']')?;
        let text_start = self.run1(after_close, super::chars::is_space)?;
        Some((text_start, status))
    }

    fn ordered_head(&mut self, position: usize) -> Parsed<f64> {
        let digits_end = self.digits(position)?;
        let after_dot = self.char(digits_end, b'.')?;
        let text_start = self.run1(after_dot, super::chars::is_space)?;
        let number = self.text(position, digits_end).parse::<f64>().ok()?;
        Some((text_start, number))
    }

    fn unordered_list_item_content(&mut self, position: usize) -> Parsed<Vec<Node>> {
        memoized!(self, unordered_list_item_content, position, Flags::NONE, {
            self.unordered_list_item_content_uncached(position)
        })
    }

    fn unordered_list_item_content_uncached(&mut self, position: usize) -> Parsed<Vec<Node>> {
        let mut items = Vec::new();
        let mut cursor = position;
        loop {
            if let Some((end, item)) = self.inline_item_pattern(cursor, Flags::NONE) {
                items.push(item);
                cursor = end;
                continue;
            }
            if self.char(cursor, b'*').is_some() {
                break;
            }
            let Some(end) = self.any(cursor) else {
                break;
            };
            items.push(Item::One(plain(self.text(cursor, end))));
            cursor = end;
        }
        if items.is_empty() || self.char(cursor, b'*').is_some() {
            return None;
        }
        if let Some(end) = self.end_of_line(cursor) {
            cursor = end;
        }
        Some((cursor, reduce_plain_texts(items)))
    }

    fn nested_lists(&mut self, position: usize) -> Parsed<Vec<Node>> {
        let (end, lines) = self.nested_list_lines(position)?;
        Some((end, group_nested_lines(lines)))
    }

    fn nested_list_lines(&mut self, position: usize) -> Parsed<Vec<NestedLine>> {
        memoized!(self, nested_list_lines, position, Flags::NONE, {
            let mut lines = Vec::new();
            let mut cursor = position;
            while let Some((end, line)) = self.nested_list_line(cursor) {
                lines.push(line);
                cursor = end;
            }
            (!lines.is_empty()).then_some((cursor, lines))
        })
    }

    fn nested_list_line(&mut self, position: usize) -> Parsed<NestedLine> {
        let indent_end = self.run1(position, super::chars::is_space)?;
        let width = indent_width(&self.source[position..indent_end]);
        let (end, marker, item) = self.nested_list_line_item(indent_end)?;
        Some((
            end,
            NestedLine {
                width,
                marker,
                item,
            },
        ))
    }

    fn nested_list_line_item(&mut self, position: usize) -> Option<(usize, ListMarker, Node)> {
        LIST_MARKERS.into_iter().find_map(|marker| {
            self.list_line_item(position, marker)
                .map(|(end, item)| (end, marker, item))
        })
    }

    fn list(&mut self, position: usize) -> Parsed<Node> {
        LIST_MARKERS
            .into_iter()
            .find_map(|marker| self.list_of_marker(position, marker))
    }

    fn list_of_marker(&mut self, position: usize, marker: ListMarker) -> Parsed<Node> {
        let mut items = Vec::new();
        let mut cursor = position;
        while let Some((end, mut item)) = self.list_line_item(cursor, marker) {
            cursor = end;
            if let Some((end, nested)) = self.nested_lists(cursor) {
                attach_nested(&mut item, nested);
                cursor = end;
            }
            items.push(item);
        }
        (!items.is_empty()).then(|| (cursor, list_of(marker, items)))
    }

    fn list_line_item(&mut self, position: usize, marker: ListMarker) -> Parsed<Node> {
        match marker {
            ListMarker::Task => {
                let (text_start, status) = self.task_head(position)?;
                let (end, text) = self.inline(text_start)?;
                Some((end, task(text, status, None)))
            }
            ListMarker::Ordered => {
                let (text_start, number) = self.ordered_head(position)?;
                let (end, text) = self.inline(text_start)?;
                Some((end, list_item(text, Some(number), None)))
            }
            ListMarker::Hyphen => {
                let after_marker = self.char(position, b'-')?;
                let text_start = self.run1(after_marker, super::chars::is_space)?;
                let (end, text) = self.inline(text_start)?;
                Some((end, list_item(text, None, None)))
            }
            ListMarker::Asterisk => {
                let after_marker = self.char(position, b'*')?;
                let text_start = self.run1(after_marker, super::chars::is_space)?;
                let (end, text) = self.unordered_list_item_content(text_start)?;
                Some((end, list_item(text, None, None)))
            }
        }
    }

    fn katex(&mut self, position: usize) -> Parsed<Node> {
        let options = &self.options.katex;
        let (parenthesis, dollar) = (options.parenthesis_syntax, options.dollar_syntax);
        let opens = (parenthesis && self.literal(position, "\\[").is_some())
            || (dollar && self.literal(position, "$$").is_some());
        if !opens {
            return None;
        }
        let content_start = position + 2;
        let content_end = self.block_katex_end(content_start);
        if content_end >= self.len() {
            return None;
        }
        Some((
            content_end + 2,
            katex(self.text(content_start, content_end)),
        ))
    }

    fn line_break(&mut self, position: usize) -> Parsed<Node> {
        let after_spaces = self.spaces(position);
        let end = self.end_of_line(after_spaces)?;
        Some((end, line_break()))
    }
}
