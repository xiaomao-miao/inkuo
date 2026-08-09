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

    // Full-width table (auto, 0 twips). Word will scale the columns
    // to fill the printable area when we lay out a `<w:tblGrid>`.
    xml.push_str("<w:tblW w:type=\"auto\" w:w=\"0\"/>");

    // Borders
    let border_color = style.border_color.clone().unwrap_or_else(|| "DDDDDD".to_string());
    xml.push_str("<w:tblBorders>");
    xml.push_str(&format!(
        "<w:top w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"{}\"/>",
        border_color
    ));
    xml.push_str(&format!(
        "<w:left w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"{}\"/>",
        border_color
    ));
    xml.push_str(&format!(
        "<w:bottom w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"{}\"/>",
        border_color
    ));
    xml.push_str(&format!(
        "<w:right w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"{}\"/>",
        border_color
    ));
    xml.push_str(&format!(
        "<w:insideH w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"{}\"/>",
        border_color
    ));
    xml.push_str(&format!(
        "<w:insideV w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"{}\"/>",
        border_color
    ));
    xml.push_str("</w:tblBorders>");

    // Cell margins (inner padding).
    xml.push_str("<w:tblCellMar>");
    xml.push_str("<w:top w:w=\"100\" w:type=\"dxa\"/>");
    xml.push_str("<w:left w:w=\"140\" w:type=\"dxa\"/>");
    xml.push_str("<w:bottom w:w=\"100\" w:type=\"dxa\"/>");
    xml.push_str("<w:right w:w=\"140\" w:type=\"dxa\"/>");
    xml.push_str("</w:tblCellMar>");

    xml.push_str("</w:tblPr>");

    // No explicit tblGrid — Word auto-sizes. We rely on per-cell
    // text widths via `<w:tcW w:type="auto"/>`.

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
        if header_repeat {
            xml.push_str("<w:trPr><w:tblHeader/></w:trPr>");
        }
        if let Some(fill) = body_default_fill {
            for cell in &row.cells {
                xml.push_str("<w:tc><w:tcPr>");
                xml.push_str(&format!(
                    "<w:tcW w:type=\"auto\" w:w=\"0\"/>"
                ));
                xml.push_str(&format!(
                    "<w:shd w:val=\"clear\" w:color=\"auto\" w:fill=\"{}\"/>",
                    fill
                ));
                xml.push_str("</w:tcPr><w:p>");
                render_cell_text(&mut xml, &cell.text, is_header, style);
                xml.push_str("</w:p></w:tc>");
            }
        } else {
            for cell in &row.cells {
                xml.push_str("<w:tc><w:tcPr>");
                xml.push_str("<w:tcW w:type=\"auto\" w:w=\"0\"/>");
                xml.push_str("</w:tcPr><w:p>");
                render_cell_text(&mut xml, &cell.text, is_header, style);
                xml.push_str("</w:p></w:tc>");
            }
        }
        xml.push_str("</w:tr>");
    }

    xml.push_str("\n    </w:tbl>");
    let _ = table_id;
    xml
}

fn render_cell_text(xml: &mut String, text: &str, is_header: bool, style: &TableStyle) {
    if text.is_empty() {
        return;
    }
    let color = if is_header {
        style
            .header_text_color
            .clone()
            .unwrap_or_else(|| "FFFFFF".to_string())
    } else {
        "2A2A2A".to_string()
    };
    let size = if is_header { 17 } else { 17 }; // 8.5pt half-points
    let bold = is_header;
    for (i, line) in text.split('\n').enumerate() {
        if i > 0 {
            xml.push_str("<w:r><w:br/></w:r>");
        }
        xml.push_str(&format!(
            "<w:r><w:rPr><w:b/>{}<w:sz w:val=\"{}\"/><w:szCs w:val=\"{}\"/></w:rPr><w:t xml:space=\"preserve\">{}</w:t></w:r>",
            if color.is_empty() { String::new() } else { format!("<w:color w:val=\"{}\"/>", color) },
            size,
            size,
            super::writer::escape_xml(line),
        ));
    }
    // Unused param suppression for style (we already used `style.header_text_color`).
    let _ = style;
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
    format!(
        concat!(
            "<w:tbl>",
            "<w:tblPr>",
            "<w:tblW w:type=\"pct\" w:w=\"5000\"/>",
            "<w:tblBorders>",
            "<w:top w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"{bg}\"/>",
            "<w:left w:val=\"single\" w:sz=\"24\" w:space=\"0\" w:color=\"{accent}\"/>",
            "<w:bottom w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"{bg}\"/>",
            "<w:right w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"{bg}\"/>",
            "</w:tblBorders>",
            "<w:tblCellMar>",
            "<w:top w:w=\"200\" w:type=\"dxa\"/>",
            "<w:left w:w=\"280\" w:type=\"dxa\"/>",
            "<w:bottom w:w=\"200\" w:type=\"dxa\"/>",
            "<w:right w:w=\"240\" w:type=\"dxa\"/>",
            "</w:tblCellMar>",
            "</w:tblPr>",
            "<w:tr><w:tc><w:tcPr><w:tcW w:type=\"pct\" w:w=\"5000\"/>",
            "<w:shd w:val=\"clear\" w:color=\"auto\" w:fill=\"{bg}\"/>",
            "</w:tcPr>"
        ),
        bg = bg,
        accent = accent,
    )
}

/// Emit the closing tags of a callout / code-block container cell.
pub fn build_callout_close_xml() -> String {
    "</w:tc></w:tr></w:tbl>".to_string()
}

/// Render a code-block container — same shape as a callout but
/// uniformly shaded and with no accent border.
pub fn build_code_block_container_xml(bg: &str) -> String {
    format!(
        concat!(
            "<w:tbl>",
            "<w:tblPr>",
            "<w:tblW w:type=\"pct\" w:w=\"5000\"/>",
            "<w:tblBorders>",
            "<w:top w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"{bg}\"/>",
            "<w:left w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"{bg}\"/>",
            "<w:bottom w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"{bg}\"/>",
            "<w:right w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"{bg}\"/>",
            "</w:tblBorders>",
            "<w:tblCellMar>",
            "<w:top w:w=\"240\" w:type=\"dxa\"/>",
            "<w:left w:w=\"280\" w:type=\"dxa\"/>",
            "<w:bottom w:w=\"240\" w:type=\"dxa\"/>",
            "<w:right w:w=\"280\" w:type=\"dxa\"/>",
            "</w:tblCellMar>",
            "</w:tblPr>",
            "<w:tr><w:tc><w:tcPr><w:tcW w:type=\"pct\" w:w=\"5000\"/>",
            "<w:shd w:val=\"clear\" w:color=\"auto\" w:fill=\"{bg}\"/>",
            "</w:tcPr>"
        ),
        bg = bg,
    )
}

/// Emit a `<w:br w:type="page"/>` inside a `<w:r>` to force a
/// page break. Returned as a complete `<w:r>…</w:r>` snippet.
pub fn page_break_run_xml() -> String {
    "<w:r><w:br w:type=\"page\"/></w:r>".to_string()
}
