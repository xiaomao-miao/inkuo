//! Streaming parser for `<w:tbl>` blocks inside `word/document.xml`.
//!
//! This file used to live inside `mod.rs` (the canonical 4 800-line
//! document module). Splitting it out keeps the streaming-XML state
//! machine local to one place, so future changes — e.g. supporting nested
//! tables, `tcPr` extensions, or per-row `trHeight` — touch one file
//! rather than scroll through unrelated reader/writer code in `mod.rs`.
//!
//! Two phases:
//!
//! 1. [`parse_table_xml`] streams the XML once and produces
//!    [`RawTable`]s holding raw per-cell state (`vMerge` restart/continue
//!    markers are kept verbatim).
//! 2. [`resolve_vmerge`] walks each column of each `RawTable`, converts
//!    `vMerge` markers into concrete `row_span` values, and emits
//!    public-type [`WordTable`]s ready for the rest of the pipeline.
//!
//! Both phases are entirely deterministic, no I/O, no allocation beyond
//! string parsing — split them out so they can be unit-tested directly
//! without spinning up a `docx` zip.

use crate::office::shared::{OfficeError, TableCell, TableRow};
use crate::office::docx::WordTable;

/// Raw cell as captured during streaming XML parsing. vMerge is held as the
/// raw "restart"/"continue" flag so the row_span can be computed per-column
/// after all rows for the table are known.
#[derive(Debug, Clone)]
struct RawCell {
    text: String,
    col_span: usize,
    vmerge: Option<VMergeKind>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VMergeKind {
    Restart,
    Continue,
}

/// Raw table that holds un-merged cells until vMerge resolution finishes.
struct RawTable {
    id: String,
    rows: Vec<Vec<RawCell>>,
}

pub(crate) fn parse_table_xml(content: &str) -> Result<Vec<WordTable>, OfficeError> {
    let mut raw_tables: Vec<RawTable> = Vec::new();
    let mut reader = quick_xml::Reader::from_str(content);
    reader.config_mut().trim_text(false);

    let mut buf = Vec::new();
    let mut current_table: Option<RawTable> = None;
    let mut current_row: Option<Vec<RawCell>> = None;
    let mut current_cell_text = String::new();
    let mut cell_col_span: usize = 1;
    let mut cell_vmerge: Option<VMergeKind> = None;
    let mut table_depth = 0;
    let mut row_depth = 0;
    let mut cell_depth = 0;
    let mut table_counter = 0usize;

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(quick_xml::events::Event::Start(ref e)) => {
                let name = e.local_name();
                match name.as_ref() {
                    b"tbl" => {
                        table_depth += 1;
                        current_table = Some(RawTable {
                            id: format!("t{}", table_counter),
                            rows: Vec::new(),
                        });
                        table_counter += 1;
                    }
                    b"tr" => {
                        row_depth += 1;
                        current_row = Some(Vec::new());
                    }
                    b"tc" => {
                        cell_depth += 1;
                        current_cell_text.clear();
                        cell_col_span = 1;
                        cell_vmerge = None;
                    }
                    b"t" if cell_depth > 0 => {
                        if let Ok(quick_xml::events::Event::Text(t)) = reader.read_event_into(&mut buf) {
                            current_cell_text.push_str(&t.unescape().unwrap_or_default());
                        }
                    }
                    b"vMerge" if cell_depth > 0 => {
                        let mut val: Option<String> = None;
                        for attr in e.attributes().with_checks(false).flatten() {
                            if attr.key.local_name().as_ref() == b"val" {
                                val = Some(std::str::from_utf8(&attr.value).unwrap_or("").to_string());
                            }
                        }
                        cell_vmerge = Some(match val.as_deref() {
                            Some("restart") => VMergeKind::Restart,
                            _ => VMergeKind::Continue,
                        });
                    }
                    _ => {}
                }
            }
            Ok(quick_xml::events::Event::Empty(ref e)) => {
                let name = e.local_name();
                match name.as_ref() {
                    b"gridSpan" if cell_depth > 0 => {
                        for attr in e.attributes().with_checks(false).flatten() {
                            if attr.key.local_name().as_ref() == b"val" {
                                let val = std::str::from_utf8(&attr.value).unwrap_or("1");
                                if let Ok(n) = val.parse::<usize>() {
                                    cell_col_span = n;
                                }
                            }
                        }
                    }
                    b"vMerge" if cell_depth > 0 => {
                        let mut val: Option<String> = None;
                        for attr in e.attributes().with_checks(false).flatten() {
                            if attr.key.local_name().as_ref() == b"val" {
                                val = Some(std::str::from_utf8(&attr.value).unwrap_or("").to_string());
                            }
                        }
                        cell_vmerge = Some(match val.as_deref() {
                            Some("restart") => VMergeKind::Restart,
                            _ => VMergeKind::Continue,
                        });
                    }
                    _ => {}
                }
            }
            Ok(quick_xml::events::Event::End(ref e)) => {
                let name = e.local_name();
                match name.as_ref() {
                    b"tc" => {
                        cell_depth -= 1;
                        if cell_depth == 0 {
                            if let Some(ref mut row) = current_row {
                                row.push(RawCell {
                                    text: current_cell_text.trim().to_string(),
                                    col_span: cell_col_span,
                                    vmerge: cell_vmerge,
                                });
                            }
                        }
                    }
                    b"tr" => {
                        row_depth -= 1;
                        if row_depth == 0 {
                            if let Some(row) = current_row.take() {
                                if let Some(ref mut tbl) = current_table {
                                    tbl.rows.push(row);
                                }
                            }
                        }
                    }
                    b"tbl" => {
                        table_depth -= 1;
                        if table_depth == 0 {
                            if let Some(table) = current_table.take() {
                                if !table.rows.is_empty() {
                                    raw_tables.push(table);
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
            Ok(quick_xml::events::Event::Eof) => break,
            Err(e) => return Err(OfficeError::Xml(format!("XML parse error: {}", e))),
            _ => {}
        }
        buf.clear();
    }

    // Resolve vMerge restart/continue markers into concrete row_span values
    // and convert into the public TableCell type.
    Ok(resolve_vmerge(raw_tables))
}

/// Resolve each table's vMerge restart/continue markers into concrete
/// `row_span` values and convert into the public `TableCell` type.
///
/// `gridSpan` (col_span) is taken straight from the parser. `row_span` is
/// computed by walking each column and counting how many following rows in
/// the same column are `vMerge="continue"` before the next non-merged cell.
/// Per the OOXML spec the first cell of each merge group uses
/// `vMerge="restart"` and the span is the total height of the region.
fn resolve_vmerge(raw_tables: Vec<RawTable>) -> Vec<WordTable> {
    let mut out = Vec::with_capacity(raw_tables.len());
    for raw in raw_tables {
        let max_col = {
            let mut m = 0usize;
            for row in &raw.rows {
                let mut c = 0;
                for cell in row {
                    c += cell.col_span.max(1);
                }
                m = m.max(c);
            }
            m
        };

        let mut row_spans: Vec<Vec<usize>> =
            vec![vec![1; raw.rows.len()]; max_col];
        for col in 0..max_col {
            let mut i = 0;
            while i < raw.rows.len() {
                let Some(start_cell) = cell_at(&raw.rows[i], col) else {
                    i += 1;
                    continue;
                };
                if start_cell.vmerge == Some(VMergeKind::Restart) {
                    let mut span = 1usize;
                    let mut j = i + 1;
                    while j < raw.rows.len() {
                        match cell_at(&raw.rows[j], col) {
                            Some(c) if c.vmerge == Some(VMergeKind::Continue) => {
                                span += 1;
                                j += 1;
                            }
                            _ => break,
                        }
                    }
                    row_spans[col][i] = span;
                    i = j;
                } else {
                    i += 1;
                }
            }
        }

        let mut rows = Vec::with_capacity(raw.rows.len());
        for (row_idx, row) in raw.rows.into_iter().enumerate() {
            let mut col_cursor = 0usize;
            let cells = row
                .into_iter()
                .map(|c| {
                    let span = c.col_span.max(1);
                    let row_span = if c.vmerge == Some(VMergeKind::Restart) {
                        row_spans[col_cursor][row_idx]
                    } else {
                        1
                    };
                    col_cursor += span;
                    TableCell {
                        text: c.text,
                        col_span: c.col_span,
                        row_span,
                    }
                })
                .collect();
            rows.push(TableRow { cells });
        }
        out.push(WordTable {
            id: raw.id,
            rows,
            cell_paragraphs: Vec::new(),
        });
    }
    out
}

/// Locate the raw cell at a given column index within a row, accounting for
/// col_span. Returns `None` if the row is shorter than `col`.
fn cell_at(cells: &[RawCell], col: usize) -> Option<&RawCell> {
    let mut cursor = 0usize;
    for c in cells {
        let span = c.col_span.max(1);
        if col >= cursor && col < cursor + span {
            return Some(c);
        }
        cursor += span;
    }
    None
}
