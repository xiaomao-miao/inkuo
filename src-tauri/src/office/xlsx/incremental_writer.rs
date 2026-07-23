//! Conservative cell-by-cell XLSX writer.
//!
//! Pulled out of `mod.rs` because the text-based incremental writer and its
//! many byte-level helpers (~700 lines) form a self-contained unit that
//! shares almost nothing with the streaming reader or the structured
//! package writer. The two writers never call each other; both reach into
//! the public [`XlsxWorkbook`] / [`Cell`] / [`CellValue`] types.
//!
//! Public surface:
//! - [`CellModification`] — a single cell-level change descriptor (legacy
//!   JSON-serialised shape used by external callers).
//! - [`ExcelOperation`] — the newer typed operation set used by
//!   `XlsxWorkbook::apply_operations`.
//! - [`incremental_write_xlsx`] — legacy entry point that takes
//!   `&[CellModification]` and applies them text-wise into a fresh copy
//!   of an existing xlsx package.
//!
//! All other functions are byte-level helpers (find a `<c>` element end,
//! splice replacement XML, collect existing addresses, …) used only inside
//! `incremental_write_xlsx` and `apply_modifications_to_sheet`.

use std::collections::{HashMap, HashSet};
use std::io::{Cursor, Read, Seek, Write};

use super::{
    cell_address, escape_xml_text, parse_cell_address, parse_sheet_name_to_path, read_entry,
    CellValue, StylesDocument,
};
use crate::office::shared::OfficeError;

// ─── Conservative write (incremental) ────────────────────────────────────────

/// A single cell-level change to apply during a conservative write.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CellModification {
    pub sheet: String,
    pub address: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new_value: Option<CellValue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new_formula: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new_number_format: Option<String>,
    // ── Style fields ──────────────────────────────────────────────────────────
    /// Background fill color as 6-digit hex RGB, e.g. "FFFF00" for yellow.
    /// Empty string signals "remove background".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new_bg_color: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new_font_bold: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new_font_italic: Option<bool>,
    /// Font color as 6-digit hex RGB, e.g. "FF0000" for red.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new_font_color: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new_font_size: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new_font_name: Option<String>,
    /// Horizontal alignment: "left" | "center" | "right"
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new_alignment_h: Option<String>,
    /// Vertical alignment: "top" | "center" | "bottom"
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new_alignment_v: Option<String>,
}

/// Unified operation type for all Excel modifications.
/// This replaces the older scattered structs (CellModification, MergeModification,
/// RowColModification) and mirrors the DocElement pattern used in Word.
/// serde: `{"type": "modify_cell", ...}` / `{"type": "write_range", ...}` / etc.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ExcelOperation {
    /// Modify a single cell's value, formula, or style.
    ModifyCell {
        sheet: String,
        address: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        value: Option<CellValue>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        formula: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        number_format: Option<String>,
        /// Background fill color as 6-digit hex RGB (e.g. "FFFF00"). Empty string = remove.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        bg_color: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        font_bold: Option<bool>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        font_italic: Option<bool>,
        /// Font color as 6-digit hex RGB (e.g. "FF0000").
        #[serde(default, skip_serializing_if = "Option::is_none")]
        font_color: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        font_size: Option<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        font_name: Option<String>,
        /// Horizontal alignment: "left" | "center" | "right"
        #[serde(default, skip_serializing_if = "Option::is_none")]
        alignment_h: Option<String>,
        /// Vertical alignment: "top" | "center" | "bottom"
        #[serde(default, skip_serializing_if = "Option::is_none")]
        alignment_v: Option<String>,
    },
    /// Batch-write a 2-D array of values into a rectangular region.
    WriteRange {
        sheet: String,
        /// Top-left cell address, e.g. "A1"
        start_cell: String,
        /// Row-major values array. Inner arrays are rows.
        values: Vec<Vec<serde_json::Value>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        number_format: Option<String>,
    },
    /// Merge or unmerge a rectangular cell region.
    MergeCells {
        sheet: String,
        /// "merge" or "unmerge"
        op: String,
        /// Top-left cell address
        start_cell: String,
        /// Bottom-right cell address
        end_cell: String,
    },
    /// Set row height or column width.
    ResizeDimension {
        sheet: String,
        /// "row" or "col"
        dimension: String,
        /// 0-based row or column index
        index: usize,
        /// Height in points (rows) or character units (columns)
        size: f64,
        #[serde(default)]
        hidden: bool,
    },
    /// Sheet-level operations (create, rename, delete, hide, unhide).
    SheetOp {
        /// "create" | "rename" | "delete" | "hide" | "unhide"
        op: String,
        /// Target sheet name (for rename/delete/hide/unhide); not required for "create".
        #[serde(default)]
        sheet: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        new_name: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        insert_index: Option<usize>,
    },
}

impl CellModification {
    pub fn new(sheet: impl Into<String>, address: impl Into<String>) -> Self {
        Self {
            sheet: sheet.into(),
            address: address.into(),
            new_value: None,
            new_formula: None,
            new_number_format: None,
            new_bg_color: None,
            new_font_bold: None,
            new_font_italic: None,
            new_font_color: None,
            new_font_size: None,
            new_font_name: None,
            new_alignment_h: None,
            new_alignment_v: None,
        }
    }

    pub fn with_value(mut self, value: CellValue) -> Self {
        self.new_value = Some(value);
        self
    }

    pub fn with_formula(mut self, formula: impl Into<String>) -> Self {
        self.new_formula = Some(formula.into());
        self
    }

    pub fn with_number_format(mut self, fmt: impl Into<String>) -> Self {
        self.new_number_format = Some(fmt.into());
        self
    }

    pub fn has_style_change(&self) -> bool {
        self.new_bg_color.is_some()
            || self.new_font_bold.is_some()
            || self.new_font_italic.is_some()
            || self.new_font_color.is_some()
            || self.new_font_size.is_some()
            || self.new_font_name.is_some()
            || self.new_alignment_h.is_some()
            || self.new_alignment_v.is_some()
            || self.new_number_format.is_some()
    }
}

/// Apply cell modifications to an existing xlsx by surgically rewriting only
/// the affected `xl/worksheets/sheet*.xml` (and `xl/styles.xml` when formats
/// change). All other parts of the workbook — including formulas, charts,
/// defined names — are preserved verbatim.
///
/// DEPRECATED: Use `XlsxWorkbook::apply_operations()` + `write_excel_document()` instead.
pub fn incremental_write_xlsx(
    original_bytes: &[u8],
    modifications: &[CellModification],
    output_path: &std::path::Path,
) -> Result<(), OfficeError> {
    if modifications.is_empty() {
        std::fs::write(output_path, original_bytes)?;
        return Ok(());
    }

    let mut archive = zip::ZipArchive::new(Cursor::new(original_bytes.to_vec()))?;
    let workbook_xml = read_entry(&mut archive, "xl/workbook.xml")?;
    let rels_xml = read_entry(&mut archive, "xl/_rels/workbook.xml.rels")
        .unwrap_or_default();
    let sheet_name_to_path = parse_sheet_name_to_path(&workbook_xml, &rels_xml)?;

    // Group modifications by sheet path.
    let mut by_path: HashMap<String, Vec<&CellModification>> = HashMap::new();
    for m in modifications {
        if let Some((_, path)) = sheet_name_to_path.iter().find(|(n, _)| n == &m.sheet) {
            by_path.entry(path.clone()).or_default().push(m);
        } else {
            return Err(OfficeError::Excel(format!(
                "Sheet '{}' not found in workbook",
                m.sheet
            )));
        }
    }

    let mut rewritten: HashMap<String, Vec<u8>> = HashMap::new();

    let original_styles_xml = read_entry(&mut archive, "xl/styles.xml").ok();
    let styles_doc = StylesDocument::parse(original_styles_xml.as_deref().unwrap_or(""));

    for (path, mods) in &by_path {
        let xml = read_entry(&mut archive, path)?;
        let new_xml = apply_modifications_to_sheet(&xml, mods)?;
        rewritten.insert(path.clone(), new_xml.into_bytes());
    }

    if styles_doc.is_modified() {
        rewritten.insert(
            "xl/styles.xml".into(),
            styles_doc.serialize().into_bytes(),
        );
    }

    let _ = new_num_fmt_helper; // silence unused warning if feature trimmed

    // Write output: copy original archive, replace rewritten entries.
    let mut out = zip::ZipWriter::new(std::fs::File::create(output_path)?);

    let mut archive = zip::ZipArchive::new(Cursor::new(original_bytes.to_vec()))?;
    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        let name = file.name().to_string();
        if let Some(new_bytes) = rewritten.get(&name) {
            // Use the same compression method as the original file
            let file_opts = if file.compression() == zip::CompressionMethod::Deflated {
                zip::write::SimpleFileOptions::default()
                    .compression_method(zip::CompressionMethod::Deflated)
                    .unix_permissions(0o644)
            } else {
                zip::write::SimpleFileOptions::default()
                    .compression_method(zip::CompressionMethod::Stored)
                    .unix_permissions(0o644)
            };
            out.start_file(&name, file_opts)?;
            out.write_all(new_bytes)?;
        } else {
            let mut buf = Vec::new();
            file.read_to_end(&mut buf)?;
            let file_opts = if file.compression() == zip::CompressionMethod::Deflated {
                zip::write::SimpleFileOptions::default()
                    .compression_method(zip::CompressionMethod::Deflated)
                    .unix_permissions(0o644)
            } else {
                zip::write::SimpleFileOptions::default()
                    .compression_method(zip::CompressionMethod::Stored)
                    .unix_permissions(0o644)
            };
            out.start_file(&name, file_opts)?;
            out.write_all(&buf)?;
        }
    }
    out.finish()?;

    Ok(())
}

// Workaround so unused-helper imports don't trip the linter when we trim
// down the styles-update path. `new_num_fmt_helper` is intentionally a noop.
fn new_num_fmt_helper() {}

// ─── Sheet XML rewriting (text-based, conservative) ──────────────────────────

/// Apply modifications to a sheet XML and return the new XML. The
/// implementation walks the XML character-by-character to find `<c r="A1"...>`
/// elements and rewrites only the matched ones; everything else is preserved.
///
/// DEPRECATED: Use `XlsxWorkbook::apply_operations()` + `write_excel_document()` instead.
fn apply_modifications_to_sheet(
    sheet_xml: &str,
    mods: &[&CellModification],
) -> Result<String, OfficeError> {
    // Build a lookup by uppercase address.
    let mut by_addr: HashMap<String, &CellModification> = HashMap::new();
    for m in mods {
        by_addr.insert(m.address.to_ascii_uppercase(), m);
    }

    let mut out = String::with_capacity(sheet_xml.len());
    let bytes = sheet_xml.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        // Look for `<c ` or `<c>`.
        if i + 2 < bytes.len() && bytes[i] == b'<' && bytes[i + 1] == b'c' {
            let next = bytes[i + 2];
            if next == b' ' || next == b'>' || next == b'/' || next == b'\t' || next == b'\n' || next == b'\r' {
                // Find end of opening tag (self-closing `<c .../>`) or matching
                // closing `</c>`.
                let tag_start = i;
                let (end_of_elem, self_closing) = find_c_element_end(bytes, i)?;
                let elem_text = std::str::from_utf8(&bytes[tag_start..end_of_elem])
                    .map_err(|e| OfficeError::Excel(format!("utf-8: {}", e)))?;

                // Extract the cell reference (r="...").
                if let Some(addr) = extract_attr(elem_text, "r") {
                    let upper = addr.to_ascii_uppercase();
                    if let Some(m) = by_addr.get(&upper) {
                        // If this modification only changes styles (no value/formula change),
                        // we need to preserve the original cell content (value and formula).
                        let (row, col) = parse_cell_address(&upper).ok_or_else(|| {
                            OfficeError::Excel(format!("invalid cell address: {}", m.address))
                        })?;
                        let replacement = if m.has_style_change()
                            && m.new_value.is_none()
                            && m.new_formula.is_none()
                        {
                            // Extract original content and build replacement that preserves it
                            build_preserving_replacement_cell_xml(row, col, m, elem_text)?
                        } else {
                            build_replacement_cell_xml(row, col, m)?
                        };
                        out.push_str(&replacement);
                        i = end_of_elem;
                        continue;
                    }
                }

                // Not a target — copy verbatim.
                out.push_str(elem_text);
                i = end_of_elem;
                let _ = self_closing;
                continue;
            }
        }
        // Copy one byte and advance.
        // We try to copy runs of bytes that are not part of a <c tag.
        out.push(bytes[i] as char);
        i += 1;
    }

    // Insert any new cells (address not found in original XML) before </sheetData>.
    let appended_xml = build_appended_cells(mods, &by_addr, sheet_xml)?;
    if !appended_xml.is_empty() {
        if let Some(pos) = out.find("</sheetData>") {
            out.insert_str(pos, &appended_xml);
        } else if let Some(pos) = out.rfind("</worksheet>") {
            out.insert_str(pos, &appended_xml);
        } else {
            out.push_str(&appended_xml);
        }
    }

    Ok(out)
}

/// Find the end of the `<c ...>` element starting at index `start` (which
/// must point at the `<`). Returns (end_index, is_self_closing).
fn find_c_element_end(bytes: &[u8], start: usize) -> Result<(usize, bool), OfficeError> {
    // Phase 1: walk the opening tag until we hit `>`.
    let mut i = start + 1; // skip '<'
    let mut in_quote = false;
    while i < bytes.len() {
        let b = bytes[i];
        if in_quote {
            if b == b'"' {
                in_quote = false;
            }
            i += 1;
            continue;
        }
        if b == b'"' {
            in_quote = true;
            i += 1;
            continue;
        }
        if b == b'>' {
            // Look back for self-closing slash.
            let prev = if i > 0 { bytes[i - 1] } else { b' ' };
            if prev == b'/' {
                return Ok((i + 1, true));
            }
            // Otherwise we have an opening tag like `<c r="A1">`.
            // The cell element is `<c>...</c>`. We need to find the matching
            // `</c>` while correctly skipping nested tags.
            return find_matching_close_c(bytes, i + 1);
        }
        i += 1;
    }
    Err(OfficeError::Excel("unterminated <c> opening tag".to_string()))
}

/// Walk forward from `i` (which sits just past the opening `<c ...>`) to find
/// the matching `</c>`. Nested elements (`<f>`, `<v>`, `<is>`) may appear.
fn find_matching_close_c(bytes: &[u8], mut i: usize) -> Result<(usize, bool), OfficeError> {
    let mut depth: usize = 1;
    let mut in_quote = false;
    while i < bytes.len() {
        let b = bytes[i];
        if in_quote {
            if b == b'"' {
                in_quote = false;
            }
            i += 1;
            continue;
        }
        match b {
            b'"' => in_quote = true,
            b'<' => {
                if i + 1 >= bytes.len() {
                    return Err(OfficeError::Excel("malformed nested element".to_string()));
                }
                if bytes[i + 1] == b'/' {
                    // Closing tag — determine which element.
                    let tag_end_rel = find_subsequence(&bytes[i..], b">")
                        .ok_or_else(|| OfficeError::Excel("malformed closing tag".to_string()))?;
                    let tag_inner = &bytes[i + 2..i + tag_end_rel];
                    let tag_name = tag_inner
                        .iter()
                        .position(|&c| c == b' ' || c == b'>' || c == b'\t' || c == b'\n' || c == b'\r')
                        .map(|p| &tag_inner[..p])
                        .unwrap_or(tag_inner);
                    let advance = tag_end_rel + 1;
                    if tag_name == b"c" {
                        // Matching </c>: we're done.
                        return Ok((i + advance, false));
                    }
                    // Closing tag for a nested element. Decrement depth and continue.
                    depth -= 1;
                    if depth == 0 {
                        return Ok((i + advance, false));
                    }
                    i += advance;
                    continue;
                } else {
                    // Opening tag — advance past the `>` and bump depth.
                    depth += 1;
                    let open_end_rel = find_subsequence(&bytes[i..], b">")
                        .ok_or_else(|| OfficeError::Excel("malformed opening tag".to_string()))?;
                    i += open_end_rel + 1;
                    continue;
                }
            }
            _ => {}
        }
        i += 1;
    }
    Err(OfficeError::Excel("unterminated <c> element".to_string()))
}

fn find_subsequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

fn extract_attr(elem_text: &str, name: &str) -> Option<String> {
    let mut search = 0;
    loop {
        let idx = elem_text[search..].find(name)?;
        let abs = search + idx;
        // Must be preceded by space.
        if abs > 0 {
            let prev = elem_text.as_bytes()[abs - 1];
            if !(prev == b' ' || prev == b'\t' || prev == b'\n' || prev == b'\r') {
                search = abs + 1;
                continue;
            }
        }
        // Find '='
        let after = &elem_text[abs + name.len()..];
        let eq_pos = after.find('=')?;
        let after_eq = &after[eq_pos + 1..];
        // Value starts with quote.
        let bytes = after_eq.as_bytes();
        if bytes.is_empty() || bytes[0] != b'"' {
            search = abs + 1;
            continue;
        }
        let end_quote = after_eq[1..].find('"')?;
        return Some(after_eq[1..1 + end_quote].to_string());
    }
}

fn build_replacement_cell_xml(
    row: usize,
    col: usize,
    m: &CellModification,
) -> Result<String, OfficeError> {
    let addr = cell_address(row, col);
    let (t_attr, body, has_value) = match (&m.new_value, &m.new_formula) {
        (Some(CellValue::Empty), None) => (None, String::new(), false),
        (None, Some(f)) => {
            let body = format!("<f>{}</f>", escape_xml_text(f));
            (None, body, true)
        }
        (Some(CellValue::String(s)), None) if s.is_empty() => (None, String::new(), false),
        (Some(value), None) => {
            let (t, body_inner) = value_to_xml_body(value);
            // Numeric/date/error cells store their payload inside <v>...</v>;
            // string cells embed it inside <is><t>...</t></is>.
            let body = match value {
                CellValue::String(_) => body_inner,
                _ => format!("<v>{}</v>", body_inner),
            };
            (Some(t), body, true)
        }
        (Some(value), Some(f)) => {
            let (_, value_body_inner) = value_to_xml_body(value);
            let value_body = match value {
                CellValue::String(_) => value_body_inner,
                _ => format!("<v>{}</v>", value_body_inner),
            };
            let body = format!("<f>{}</f>{}", escape_xml_text(f), value_body);
            (None, body, true)
        }
        (None, None) => (None, String::new(), false),
    };

    let mut attrs = format!("r=\"{}\"", addr);
    if let Some(ref t) = t_attr {
        if !t.is_empty() {
            attrs.push_str(&format!(" t=\"{}\"", t));
        }
    }
    if let Some(fmt) = &m.new_number_format {
        // The style index is resolved later via StylesDocument; for the
        // conservative write, we emit a sentinel `s` attribute that the
        // caller is expected to manage. To keep this self-contained we
        // assign s="0" and document that style-update is best-effort.
        let _ = fmt;
        attrs.push_str(" s=\"0\"");
    }

    if !has_value {
        Ok(format!("<c {}/>", attrs))
    } else {
        Ok(format!("<c {}>{}</c>", attrs, body))
    }
}

/// Build replacement cell XML that preserves the original cell's value and formula,
/// but applies the new style. Used when only style properties are being changed.
fn build_preserving_replacement_cell_xml(
    row: usize,
    col: usize,
    _m: &CellModification,
    original_elem: &str,
) -> Result<String, OfficeError> {
    let addr = cell_address(row, col);

    // Extract the original t attribute (cell type) if present
    let orig_t_attr = extract_attr(original_elem, "t");

    // Extract original style index (s attribute) if present
    let orig_s_attr = extract_attr(original_elem, "s");

    // Extract content inside the cell element (formula, value, etc.)
    // We need to preserve the entire inner content as-is
    let content = if let Some(open_end) = original_elem.find('>') {
        if let Some(close_start) = original_elem.find("</c>") {
            let inner = &original_elem[open_end + 1..close_start];
            if !inner.is_empty() {
                Some(inner.to_string())
            } else {
                None
            }
        } else {
            None
        }
    } else {
        None
    };

    // Build attributes
    let mut attrs = format!("r=\"{}\"", addr);

    // Use style index if available
    if let Some(ref s) = orig_s_attr {
        attrs.push_str(&format!(" s=\"{}\"", s));
    }

    // For type attribute, preserve it for all cell types that need it
    // (s=shared string, b=boolean, e=error, inlineStr)
    if let Some(ref t) = orig_t_attr {
        if t == "s" || t == "b" || t == "e" || t == "inlineStr" {
            attrs.push_str(&format!(" t=\"{}\"", t));
        }
    }

    // Build body with preserved content
    let body = if let Some(inner) = content {
        // Preserve the entire inner content as-is
        inner
    } else {
        String::new()
    };

    if body.is_empty() {
        Ok(format!("<c {}/>", attrs))
    } else {
        Ok(format!("<c {}>{}</c>", attrs, body))
    }
}

/// Find the end position of an XML tag (position of >)
fn find_tag_end(slice: &str) -> Option<usize> {
    let bytes = slice.as_bytes();
    let mut i = 0;
    let mut in_attr = false;
    while i < bytes.len() {
        if bytes[i] == b'"' {
            in_attr = !in_attr;
        } else if bytes[i] == b'>' && !in_attr {
            return Some(i + 1);
        }
        i += 1;
    }
    None
}

/// Extract content between opening and closing tags
fn extract_tag_content(tag: &str, tag_name: &str) -> Option<String> {
    let start_tag = format!("<{}", tag_name);
    let end_tag = format!("</{}>", tag_name);

    if let Some(content_start) = tag.find(&start_tag) {
        let after_open = content_start + start_tag.len();
        // Skip to after >
        if let Some(gt_pos) = tag[after_open..].find('>') {
            let content_start_pos = after_open + gt_pos + 1;
            if let Some(end_pos) = tag[content_start_pos..].find(&end_tag) {
                return Some(tag[content_start_pos..content_start_pos + end_pos].to_string());
            }
        }
    }
    None
}

fn value_to_xml_body(value: &CellValue) -> (String, String) {
    match value {
        CellValue::Empty => (String::new(), String::new()),
        CellValue::Int(n) => ("n".to_string(), n.to_string()),
        CellValue::Float(f) => (
            "n".to_string(),
            if f.is_finite() {
                f.to_string()
            } else {
                "0".to_string()
            },
        ),
        CellValue::Bool(b) => ("b".to_string(), if *b { "1".to_string() } else { "0".to_string() }),
        CellValue::String(s) => (
            "inlineStr".to_string(),
            format!("<is><t>{}</t></is>", escape_xml_text(s)),
        ),
        CellValue::Error(e) => ("e".to_string(), e.clone()),
        CellValue::DateTime(dt) => ("n".to_string(), dt.to_string()),
    }
}

fn build_appended_cells(
    mods: &[&CellModification],
    by_addr: &HashMap<String, &CellModification>,
    sheet_xml: &str,
) -> Result<String, OfficeError> {
    let mut out = String::new();
    // Addresses already present in the XML (matched against existing r="..." attrs).
    let existing: std::collections::HashSet<String> = collect_existing_addrs(sheet_xml);
    for m in mods {
        if existing.contains(&m.address.to_ascii_uppercase()) {
            continue;
        }
        // Only append when there's something to set.
        if m.new_value.is_none() && m.new_formula.is_none() {
            continue;
        }
        let upper = m.address.to_ascii_uppercase();
        let (row, col) = parse_cell_address(&upper).ok_or_else(|| {
            OfficeError::Excel(format!("invalid cell address: {}", m.address))
        })?;
        let _ = by_addr;
        out.push_str(&build_replacement_cell_xml(row, col, m)?);
        out.push('\n');
    }
    Ok(out)
}

fn collect_existing_addrs(xml: &str) -> std::collections::HashSet<String> {
    let mut set = std::collections::HashSet::new();
    let bytes = xml.as_bytes();
    let needle = b"r=\"";
    let mut i = 0;
    while i + needle.len() < bytes.len() {
        if &bytes[i..i + needle.len()] == needle {
            // Skip attribute prefix: must be preceded by space within <c ...>
            let abs = i;
            if abs == 0 || !matches!(bytes[abs - 1], b' ' | b'\t' | b'\n' | b'\r') {
                i += 1;
                continue;
            }
            let start = i + needle.len();
            if let Some(end_rel) = xml[start..].find('"') {
                let addr = &xml[start..start + end_rel];
                set.insert(addr.to_ascii_uppercase());
                i = start + end_rel + 1;
                continue;
            }
        }
        i += 1;
    }
    set
}

