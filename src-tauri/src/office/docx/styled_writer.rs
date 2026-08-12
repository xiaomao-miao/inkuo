//! Extended table / callout / code-block XML builders.
//!
//! The base `writer.rs` builders emit "boring" Word tables — single
//! border colour, no per-cell fill, no row banding, no header
//! repeat. That's fine for hand-edited documents but visually flat
//! for the brand the design system describes.
//!
//! This module **adds** the styled variants:
//!
//!   - [`build_styled_table_xml`] — recognises the `__STYLE__|...`
//!     marker row that [`super::components::styled_table`] injects
//!     and emits per-cell `<w:shd>`, coloured borders, and an
//!     optional `<w:tblHeader/>` so the header row repeats on every
//!     page.
//!
//!   - [`build_callout_table_xml`] — renders a single-cell table
//!     whose background is the callout's `bg` colour and whose left
//!     border is the `accent` colour. The title + body paragraphs
//!     emitted by [`super::renderer`] sit between the table's open
//!     and close tags.
//!
//!   - [`build_code_block_table_xml`] — same shape as a callout but
//!     uniformly shaded with the code background colour and no
//!     accent border.
//!
//! These builders slot in next to `build_table_xml` in the writer;
//! the orchestrator picks one or the other based on a marker prefix
//! in the first cell's text.

use super::components::{decode_table_style, TableStyle};
use crate::office::shared::TableRow;

/// Marker prefix for style metadata rows. The writer skips these
/// rows when emitting real `<w:tr>` tags and reads them as visual
/// config instead.
pub const STYLE_MARKER_PREFIX: &str = "__STYLE__|";
/// Marker prefix for callout container tables.
pub const CALLOUT_MARKER_PREFIX: &str = "__CALLOUT__|";
/// Marker prefix for code-block container tables.
pub const CODE_MARKER_PREFIX: &str = "__CODE__|";

/// Parse a `<w:tbl>`-emitting function. Returns the XML body
/// (without `<w:tbl>` tags — caller wraps), or `None` if `rows`
/// doesn't begin with a recognised marker.
#[derive(Debug, Clone)]
pub enum TableKind {
    /// Brand-styled table: per-cell fills, optional zebra striping,
    /// optional header-row repeat.
    Styled(Box<TableStyle>),
    /// Callout container — 1×1 cell with `bg` fill and `accent` left border.
    Callout { bg: String, accent: String },
    /// Code-block container — 1×1 cell with `bg` fill, no border.
    Code { bg: String },
    /// Plain table — no style marker, fall through to the default
    /// builder.
    Plain,
}

/// Inspect the first cell of `rows` to figure out which builder to
/// use. Strips the marker row from `rows` so the caller can pass the
/// remaining rows to the appropriate builder.
pub fn classify_and_strip(rows: &[TableRow]) -> (TableKind, Vec<TableRow>) {
    if let Some(first) = rows.first() {
        if let Some(cell) = first.cells.first() {
            let text = cell.text.as_str();
            if text.starts_with(STYLE_MARKER_PREFIX) {
                if let Some((style, _header_color)) = decode_table_style(rows) {
                    let rest = rows[1..].to_vec();
                    return (TableKind::Styled(Box::new(style)), rest);
                }
            } else if text.starts_with(CALLOUT_MARKER_PREFIX) {
                let rest = rows[1..].to_vec();
                let payload = text.trim_start_matches(CALLOUT_MARKER_PREFIX);
                let parts: Vec<&str> = payload.split('|').collect();
                let bg = parts.first().map(|s| s.to_string()).unwrap_or_default();
                let accent = parts.get(1).map(|s| s.to_string()).unwrap_or_default();
                return (TableKind::Callout { bg, accent }, rest);
            } else if text.starts_with(CODE_MARKER_PREFIX) {
                let rest = rows[1..].to_vec();
                let bg = text.trim_start_matches(CODE_MARKER_PREFIX).to_string();
                return (TableKind::Code { bg }, rest);
            }
        }
    }
    (TableKind::Plain, rows.to_vec())
}

/// Render a styled table to XML. Caller wraps in `<w:tbl>...</w:tbl>`
/// (which we also emit here so the signature matches `build_table_xml`).
pub fn build_styled_table_xml(table_id: &str, rows: &[TableRow], style: &TableStyle) -> String {
    let mut xml = String::new();
    xml.push_str("\n    <w:tbl>");
    xml.push_str("\n      <w:tblPr>");
    xml.push_str("<w:tblStyle w:val=\"BrandTable\"/>");

    // Table width: A4 page with 1-inch margins = ~9026 twips available
    const TABLE_WIDTH: u32 = 9026;
    xml.push_str(&format!("<w:tblW w:type=\"dxa\" w:w=\"{}\"/>", TABLE_WIDTH));

    // Table indent (left margin)
    xml.push_str("<w:tblInd w:type=\"dxa\" w:w=\"0\"/>");

    // Table layout: fixed ensures consistent rendering
    xml.push_str("<w:tblLayout w:type=\"fixed\"/>");

    // Table look for row banding and header styling
    let has_zebra = style.zebra && style.zebra_fill.is_some();
    xml.push_str(&format!(
        "<w:tblLook w:firstColumn=\"{}\" w:firstRow=\"1\" w:lastColumn=\"0\" w:lastRow=\"0\" w:noHBand=\"{}\" w:noVBand=\"1\" w:val=\"04A0\"/>",
        if has_zebra { "1" } else { "0" },
        if has_zebra { "0" } else { "1" }
    ));

    // Borders
    let border_color = style.border_color.clone().unwrap_or_else(|| "DDDDDD".to_string());
    let border_size = "5"; // Slightly thicker borders for styled tables
    xml.push_str("<w:tblBorders>");
    xml.push_str(&format!(
        "<w:top w:val=\"single\" w:sz=\"{}\" w:space=\"0\" w:color=\"{}\"/>",
        border_size, border_color
    ));
    xml.push_str(&format!(
        "<w:left w:val=\"single\" w:sz=\"{}\" w:space=\"0\" w:color=\"{}\"/>",
        border_size, border_color
    ));
    xml.push_str(&format!(
        "<w:bottom w:val=\"single\" w:sz=\"{}\" w:space=\"0\" w:color=\"{}\"/>",
        border_size, border_color
    ));
    xml.push_str(&format!(
        "<w:right w:val=\"single\" w:sz=\"{}\" w:space=\"0\" w:color=\"{}\"/>",
        border_size, border_color
    ));
    xml.push_str(&format!(
        "<w:insideH w:val=\"single\" w:sz=\"{}\" w:space=\"0\" w:color=\"{}\"/>",
        border_size, border_color
    ));
    xml.push_str(&format!(
        "<w:insideV w:val=\"single\" w:sz=\"{}\" w:space=\"0\" w:color=\"{}\"/>",
        border_size, border_color
    ));
    xml.push_str("</w:tblBorders>");

    // Cell margins (inner padding) for comfortable reading
    xml.push_str("<w:tblCellMar>");
    xml.push_str("<w:top w:w=\"100\" w:type=\"dxa\"/>");
    xml.push_str("<w:left w:w=\"140\" w:type=\"dxa\"/>");
    xml.push_str("<w:bottom w:w=\"100\" w:type=\"dxa\"/>");
    xml.push_str("<w:right w:w=\"140\" w:type=\"dxa\"/>");
    xml.push_str("</w:tblCellMar>");

    xml.push_str("</w:tblPr>");

    // Build tblGrid: calculate column widths based on column count
    // Use fixed proportional widths that sum to TABLE_WIDTH
    let col_count = rows.first().map(|r| r.cells.len()).unwrap_or(0);
    let col_widths: Vec<u32> = match col_count {
        1 => vec![TABLE_WIDTH],
        2 => vec![TABLE_WIDTH / 3, TABLE_WIDTH - TABLE_WIDTH / 3],  // 1/3, 2/3
        3 => vec![TABLE_WIDTH / 4, TABLE_WIDTH / 4, TABLE_WIDTH / 2],  // 25%, 25%, 50%
        _ => {
            let base = TABLE_WIDTH / col_count as u32;
            let remainder = TABLE_WIDTH % col_count as u32;
            (0..col_count as u32).map(|i| base + if i < remainder { 1 } else { 0 }).collect()
        }
    };
    if col_count > 0 {
        xml.push_str("\n      <w:tblGrid>");
        for &w in &col_widths {
            xml.push_str(&format!("<w:gridCol w:w=\"{}\"/>", w));
        }
        xml.push_str("</w:tblGrid>");
    }

    for (idx, row) in rows.iter().enumerate() {
        let is_header = idx == 0;
        let body_zebra = style.zebra && style.zebra_fill.is_some() && !is_header && idx % 2 == 0;
        let body_default_fill = if is_header {
            style.header_fill.clone()
        } else if body_zebra {
            style.zebra_fill.clone()
        } else {
            None
        };
        let header_repeat = is_header && style.repeat_header;

        xml.push_str("\n        <w:tr>");
        
        // Row properties
        if header_repeat || has_zebra {
            xml.push_str("<w:trPr>");
            if header_repeat {
                xml.push_str("<w:tblHeader/>");
            }
            xml.push_str("</w:trPr>");
        }
        
        // Track cumulative offset for cell positioning
        let mut col_offset = 0usize;
        
        for cell in &row.cells {
            let col_span = cell.col_span.max(1);
            let row_span = cell.row_span.max(1);
            
            // Calculate cell width based on col_span
            let cell_width: u32 = col_widths.iter()
                .skip(col_offset)
                .take(col_span)
                .sum();
            col_offset += col_span;
            
            xml.push_str("<w:tc><w:tcPr>");
            
            // Grid span for merged cells
            if col_span > 1 {
                xml.push_str(&format!("<w:gridSpan w:val=\"{}\"/>", col_span));
            }
            
            // Vertical merge for row-spanning cells
            if row_span > 1 {
                xml.push_str("<w:vMerge w:val=\"restart\"/>");
            }
            
            // Cell width in twips
            xml.push_str(&format!("<w:tcW w:type=\"dxa\" w:w=\"{}\"/>", cell_width));
            
            // Cell margins
            xml.push_str("<w:tcMar>");
            xml.push_str("<w:top w:w=\"100\" w:type=\"dxa\"/>");
            xml.push_str("<w:left w:w=\"120\" w:type=\"dxa\"/>");
            xml.push_str("<w:bottom w:w=\"100\" w:type=\"dxa\"/>");
            xml.push_str("<w:right w:w=\"120\" w:type=\"dxa\"/>");
            xml.push_str("</w:tcMar>");
            
            // Cell vertical alignment
            xml.push_str("<w:vAlign w:val=\"center\"/>");
            
            // Cell background fill
            if let Some(ref fill) = body_default_fill {
                xml.push_str(&format!(
                    "<w:shd w:val=\"clear\" w:color=\"auto\" w:fill=\"{}\"/>",
                    fill
                ));
            }
            
            // Cell borders
            xml.push_str("<w:tcBorders>");
            xml.push_str(&format!(
                "<w:top w:val=\"single\" w:sz=\"{}\" w:space=\"0\" w:color=\"{}\"/>",
                border_size, border_color
            ));
            xml.push_str(&format!(
                "<w:left w:val=\"single\" w:sz=\"{}\" w:space=\"0\" w:color=\"{}\"/>",
                border_size, border_color
            ));
            xml.push_str(&format!(
                "<w:bottom w:val=\"single\" w:sz=\"{}\" w:space=\"0\" w:color=\"{}\"/>",
                border_size, border_color
            ));
            xml.push_str(&format!(
                "<w:right w:val=\"single\" w:sz=\"{}\" w:space=\"0\" w:color=\"{}\"/>",
                border_size, border_color
            ));
            if col_count > 1 {
                xml.push_str(&format!(
                    "<w:insideH w:val=\"single\" w:sz=\"{}\" w:space=\"0\" w:color=\"{}\"/>",
                    border_size, border_color
                ));
                xml.push_str(&format!(
                    "<w:insideV w:val=\"single\" w:sz=\"{}\" w:space=\"0\" w:color=\"{}\"/>",
                    border_size, border_color
                ));
            }
            xml.push_str("</w:tcBorders>");
            
            xml.push_str("</w:tcPr><w:p>");
            render_cell_text(&mut xml, &cell.text, is_header, style);
            xml.push_str("</w:p></w:tc>");
        }
        xml.push_str("</w:tr>");
    }

    xml.push_str("\n    </w:tbl>");
    let _ = table_id;
    xml
}

fn render_cell_text(xml: &mut String, text: &str, is_header: bool, style: &TableStyle) {
    let color = if is_header {
        style
            .header_text_color
            .clone()
            .unwrap_or_else(|| "FFFFFF".to_string())
    } else {
        "2A2A2A".to_string()
    };
    let size = 18; // 9pt half-points for compact cells
    let bold = is_header;
    
    // Split text by newlines and render each line
    let lines: Vec<&str> = text.split('\n').collect();
    for (i, line) in lines.iter().enumerate() {
        // Add line break between lines (except for the first line)
        if i > 0 {
            xml.push_str("<w:r><w:br/></w:r>");
        }
        
        if line.is_empty() {
            continue;
        }
        
        // Build run properties
        let mut rpr = String::new();
        if bold {
            rpr.push_str("<w:b/>");
        }
        rpr.push_str(&format!("<w:color w:val=\"{}\"/>", color));
        rpr.push_str(&format!("<w:sz w:val=\"{}\"/>", size));
        rpr.push_str(&format!("<w:szCs w:val=\"{}\"/>", size));
        
        if rpr.is_empty() {
            xml.push_str(&format!(
                "<w:r><w:t xml:space=\"preserve\">{}</w:t></w:r>",
                super::writer::escape_xml(line),
            ));
        } else {
            xml.push_str(&format!(
                "<w:r><w:rPr>{}</w:rPr><w:t xml:space=\"preserve\">{}</w:t></w:r>",
                rpr,
                super::writer::escape_xml(line),
            ));
        }
    }
}

/// Render a callout container — a 1-row, 1-cell table with the
/// given background fill and a thick accent-coloured left border.
/// The cell is empty; the caller emits the callout's title / body
/// paragraphs *inside* the cell. We handle that by emitting the
/// `<w:tc><w:tcPr>…</w:tcPr>` followed by caller-supplied runs;
/// but because the existing writer stitches cells to one paragraph,
/// we instead emit just the wrapper `<w:tbl>…</w:tbl>` and rely on
/// the caller (the orchestrator) to splice in the inner content.
pub fn build_callout_container_xml(bg: &str, accent: &str) -> String {
    // Use dxa type for full page width (A4 ~9026 twips with 1-inch margins)
    const FULL_WIDTH: u32 = 9026;
    let mut xml = String::new();
    xml.push_str("<w:tbl>");
    xml.push_str("<w:tblPr>");
    xml.push_str(&format!("<w:tblW w:type=\"dxa\" w:w=\"{}\"/>", FULL_WIDTH));
    xml.push_str("<w:tblInd w:type=\"dxa\" w:w=\"0\"/>");
    xml.push_str("<w:tblLayout w:type=\"fixed\"/>");
    xml.push_str("<w:tblBorders>");
    xml.push_str(&format!("<w:top w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"{}\"/>", bg));
    xml.push_str(&format!("<w:left w:val=\"single\" w:sz=\"24\" w:space=\"0\" w:color=\"{}\"/>", accent));
    xml.push_str(&format!("<w:bottom w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"{}\"/>", bg));
    xml.push_str(&format!("<w:right w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"{}\"/>", bg));
    xml.push_str("</w:tblBorders>");
    xml.push_str("<w:tblCellMar>");
    xml.push_str("<w:top w:w=\"200\" w:type=\"dxa\"/>");
    xml.push_str("<w:left w:w=\"280\" w:type=\"dxa\"/>");
    xml.push_str("<w:bottom w:w=\"200\" w:type=\"dxa\"/>");
    xml.push_str("<w:right w:w=\"240\" w:type=\"dxa\"/>");
    xml.push_str("</w:tblCellMar>");
    xml.push_str("</w:tblPr>");
    xml.push_str(&format!("<w:tblGrid><w:gridCol w:w=\"{}\"/></w:tblGrid>", FULL_WIDTH));
    xml.push_str("<w:tr>");
    xml.push_str("<w:tc>");
    xml.push_str("<w:tcPr>");
    xml.push_str(&format!("<w:tcW w:type=\"dxa\" w:w=\"{}\"/>", FULL_WIDTH));
    xml.push_str("<w:tcMar>");
    xml.push_str("<w:top w:w=\"200\" w:type=\"dxa\"/>");
    xml.push_str("<w:left w:w=\"280\" w:type=\"dxa\"/>");
    xml.push_str("<w:bottom w:w=\"200\" w:type=\"dxa\"/>");
    xml.push_str("<w:right w:w=\"240\" w:type=\"dxa\"/>");
    xml.push_str("</w:tcMar>");
    xml.push_str(&format!("<w:shd w:val=\"clear\" w:color=\"auto\" w:fill=\"{}\"/>", bg));
    xml.push_str("</w:tcPr>");
    xml
}

/// Emit the closing tags of a callout / code-block container cell.
/// The inner paragraphs are emitted with <w:p>...</w:p> by emit_inner_paragraph,
/// so we only need to close the cell, row, and table here.
pub fn build_callout_close_xml() -> String {
    "</w:tc></w:tr></w:tbl>".to_string()
}

/// Render a code-block container — same shape as a callout but
/// uniformly shaded and with no accent border.
pub fn build_code_block_container_xml(bg: &str) -> String {
    // Use dxa type for full page width (A4 ~9026 twips with 1-inch margins)
    const FULL_WIDTH: u32 = 9026;
    let mut xml = String::new();
    xml.push_str("<w:tbl>");
    xml.push_str("<w:tblPr>");
    xml.push_str(&format!("<w:tblW w:type=\"dxa\" w:w=\"{}\"/>", FULL_WIDTH));
    xml.push_str("<w:tblInd w:type=\"dxa\" w:w=\"0\"/>");
    xml.push_str("<w:tblLayout w:type=\"fixed\"/>");
    xml.push_str("<w:tblBorders>");
    xml.push_str(&format!("<w:top w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"{}\"/>", bg));
    xml.push_str(&format!("<w:left w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"{}\"/>", bg));
    xml.push_str(&format!("<w:bottom w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"{}\"/>", bg));
    xml.push_str(&format!("<w:right w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"{}\"/>", bg));
    xml.push_str("</w:tblBorders>");
    xml.push_str("<w:tblCellMar>");
    xml.push_str("<w:top w:w=\"240\" w:type=\"dxa\"/>");
    xml.push_str("<w:left w:w=\"280\" w:type=\"dxa\"/>");
    xml.push_str("<w:bottom w:w=\"240\" w:type=\"dxa\"/>");
    xml.push_str("<w:right w:w=\"280\" w:type=\"dxa\"/>");
    xml.push_str("</w:tblCellMar>");
    xml.push_str("</w:tblPr>");
    xml.push_str(&format!("<w:tblGrid><w:gridCol w:w=\"{}\"/></w:tblGrid>", FULL_WIDTH));
    xml.push_str("<w:tr>");
    xml.push_str("<w:tc>");
    xml.push_str("<w:tcPr>");
    xml.push_str(&format!("<w:tcW w:type=\"dxa\" w:w=\"{}\"/>", FULL_WIDTH));
    xml.push_str("<w:tcMar>");
    xml.push_str("<w:top w:w=\"240\" w:type=\"dxa\"/>");
    xml.push_str("<w:left w:w=\"280\" w:type=\"dxa\"/>");
    xml.push_str("<w:bottom w:w=\"240\" w:type=\"dxa\"/>");
    xml.push_str("<w:right w:w=\"280\" w:type=\"dxa\"/>");
    xml.push_str("</w:tcMar>");
    xml.push_str(&format!("<w:shd w:val=\"clear\" w:color=\"auto\" w:fill=\"{}\"/>", bg));
    xml.push_str("</w:tcPr>");
    xml
}

/// Emit a `<w:br w:type="page"/>` inside a `<w:r>` to force a
/// page break. Returned as a complete `<w:r>…</w:r>` snippet.
pub fn page_break_run_xml() -> String {
    "<w:r><w:br w:type=\"page\"/></w:r>".to_string()
}
