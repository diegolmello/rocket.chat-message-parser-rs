use message_parser::ast::{plain, TableValue};
use message_parser::{parse, Node, Options};

fn first_table(input: &str) -> (TableValue, Option<[usize; 2]>) {
    match parse(input, &Options::default()).into_iter().next() {
        Some(Node::Table { value, fallback }) => (value, fallback),
        other => panic!("expected {input:?} to parse to a table first, got {other:?}"),
    }
}

fn row_cells(row: &Node) -> &[Node] {
    match row {
        Node::TableRow { value } => value,
        other => panic!("expected a TABLE_ROW, got {other:?}"),
    }
}

fn cell_content(cell: &Node) -> &[Node] {
    match cell {
        Node::TableCell { value, .. } => value,
        other => panic!("expected a TABLE_CELL, got {other:?}"),
    }
}

#[test]
fn normalizes_ragged_body_rows_to_the_header_column_count() {
    let input = ["| a | b |", "| - | - |", "| 1 |", "| 1 | 2 | 3 |"].join("\n");
    let (table, _) = first_table(&input);

    let row_lengths: Vec<usize> = table.rows.iter().map(|row| row_cells(row).len()).collect();
    assert_eq!(row_lengths, [2, 2]);
    assert!(cell_content(&row_cells(&table.rows[0])[1]).is_empty());
    let second_row_contents: Vec<&[Node]> =
        row_cells(&table.rows[1]).iter().map(cell_content).collect();
    assert_eq!(second_row_contents, [&[plain("1")][..], &[plain("2")][..]]);
}

#[test]
fn exposes_the_source_offsets_as_fallback_for_renderers_without_table_support() {
    let input = "| a | b |\n| - | - |\n| 1 | 2 |";
    let (_, fallback) = first_table(input);

    assert_eq!(fallback, Some([0, input.len()]));
    let [start, end] = fallback.expect("fallback was just checked");
    assert_eq!(&input[start..end], input);
}
