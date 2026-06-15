//! Word (.docx) document parsing and writing

use serde::{Deserialize, Serialize};
use std::io::{Read, Write};

use super::shared::{OfficeError, read_zip_entry, TableCell, TableRow};

/// Inline text formatting — embedded within a paragraph's text.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FontRun {
    pub text: String,
    #[serde(default)]
    pub bold: bool,
    #[serde(default)]
    pub italic: bool,
    #[serde(default)]
    pub underline: bool,
    #[serde(default)]
    pub font_size: Option<u32>,  // half-points, e.g. 24 = 12pt
    #[serde(default)]
    pub color: Option<String>,   // hex RGB, e.g. "FF0000"
    #[serde(default)]
    pub font_name: Option<String>,
}

/// Rich paragraph: either plain text OR an array of formatted runs.
/// If `runs` is present, `text` is ignored.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WordParagraph {
    /// Unique identifier for this paragraph, stable across reads.
    pub id: String,
    /// Plain text (used when runs is absent).
    pub text: String,
    /// Paragraph-level style, e.g. "Heading1", "Heading2", "Title", "Normal".
    #[serde(default)]
    pub style: Option<String>,
    /// Rich formatted runs. When present, overrides `text`.
    #[serde(default)]
    pub runs: Option<Vec<FontRun>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WordTable {
    /// Unique identifier for this table, stable across reads.
    pub id: String,
    pub rows: Vec<TableRow>,
}

/// A document element — either a paragraph or a table.
/// Tables carry `position` (index in the flattened document order) so the
/// write path knows exactly where to insert each table.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum DocElement {
    #[serde(rename = "paragraph")]
    Paragraph {
        id: String,
        text: String,
        style: Option<String>,
        #[serde(default)]
        runs: Option<Vec<FontRun>>,
    },
    #[serde(rename = "table")]
    Table {
        id: String,
        /// Zero-based position among all document elements (0 = before p0).
        #[serde(default)]
        position: usize,
        header: Vec<String>,
        rows: Vec<Vec<String>>,
    },
}

impl WordDocument {
    /// Convert the document to a flat list of elements with stable IDs.
    /// Tables and paragraphs are interleaved by matching position markers
    /// to tables in sequential order.
    pub fn to_elements(&self) -> Vec<DocElement> {
        // Build a map of table id -> table for O(1) lookup.
        let table_map: std::collections::HashMap<&str, &WordTable> =
            self.tables.iter().map(|t| (t.id.as_str(), t)).collect();

        let mut elements: Vec<DocElement> = Vec::with_capacity(self.paragraphs.len() + self.tables.len());

        for p in &self.paragraphs {
            if let Some(rest) = p.text.strip_prefix("<__tbl_pos_") {
                if let Some(end) = rest.find("__>") {
                    let tbl_id = &rest[..end];
                    if let Some(tbl) = table_map.get(tbl_id) {
                        let (header, rows) = if tbl.rows.is_empty() {
                            (vec![], vec![])
                        } else {
                            let h = tbl.rows[0].cells.iter().map(|c| c.text.clone()).collect();
                            let r: Vec<Vec<String>> = tbl.rows[1..].iter()
                                .map(|r| r.cells.iter().map(|c| c.text.clone()).collect())
                                .collect();
                            (h, r)
                        };
                        elements.push(DocElement::Table {
                            id: tbl.id.clone(),
                            position: elements.len(),
                            header,
                            rows,
                        });
                    }
                    continue;
                }
            }
            elements.push(DocElement::Paragraph {
                id: p.id.clone(),
                text: p.text.clone(),
                style: p.style.clone(),
                runs: p.runs.clone(),
            });
        }
        // Tables without preceding markers (e.g. added via append mode) go at the end.
        for tbl in &self.tables {
            if !table_map.contains_key(tbl.id.as_str()) {
                let (header, rows) = if tbl.rows.is_empty() {
                    (vec![], vec![])
                } else {
                    let h = tbl.rows[0].cells.iter().map(|c| c.text.clone()).collect();
                    let r: Vec<Vec<String>> = tbl.rows[1..].iter()
                        .map(|r| r.cells.iter().map(|c| c.text.clone()).collect())
                        .collect();
                    (h, r)
                };
                elements.push(DocElement::Table {
                    id: tbl.id.clone(),
                    position: elements.len(),
                    header,
                    rows,
                });
            }
        }

        elements
    }

    /// Returns a map: table ID -> marker paragraph ID for tables that have a
    /// preceding `<__tbl_pos_>` marker. This lets `modify()` delete the marker
    /// along with the table so no stale visible paragraphs are left behind.
    pub fn marker_to_table_map(&self) -> std::collections::HashMap<String, String> {
        let mut map = std::collections::HashMap::new();
        for p in &self.paragraphs {
            if let Some(rest) = p.text.strip_prefix("<__tbl_pos_") {
                if let Some(end) = rest.find("__>") {
                    let tbl_id = &rest[..end];
                    // The marker paragraph's ID is p.id; the table's ID is tbl_id
                    map.insert(tbl_id.to_string(), p.id.clone());
                }
            }
        }
        map
    }

    /// Build a WordDocument from a list of elements.
    /// Each table is preceded by a marker paragraph whose ID encodes the table's own ID
    /// (e.g. table id "t0" → marker id "__tbl_pos_t0__"), so deletions of the table
    /// by ID also remove the marker.
    pub fn from_elements(elements: Vec<DocElement>) -> Self {
        let mut out_paras: Vec<WordParagraph> = Vec::new();
        let mut tables: Vec<WordTable> = Vec::new();

        for elem in elements {
            match elem {
                DocElement::Paragraph { id, text, style, runs } => {
                    out_paras.push(WordParagraph { id, text, style, runs });
                }
                DocElement::Table { id, position: _, header, rows } => {
                    // Emit a position marker whose ID matches the table's ID.
                    // This lets delete_set remove both the marker and the table together.
                    out_paras.push(WordParagraph {
                        id: format!("__tbl_pos_{}__", id),
                        text: format!("<__tbl_pos_{}__>", id),
                        style: None,
                        runs: None,
                    });

                    let mut table_rows = vec![];
                    if !header.is_empty() {
                        table_rows.push(crate::office::TableRow {
                            cells: header.into_iter()
                                .map(|text| crate::office::TableCell { text, col_span: 1, row_span: 1 })
                                .collect()
                        });
                    }
                    for row in rows {
                        table_rows.push(crate::office::TableRow {
                            cells: row.into_iter()
                                .map(|text| crate::office::TableCell { text, col_span: 1, row_span: 1 })
                                .collect()
                        });
                    }
                    tables.push(WordTable { id, rows: table_rows });
                }
            }
        }

        WordDocument { paragraphs: out_paras, tables }
    }

    /// Modify the document by applying a list of edit operations.
    pub fn modify(
        &mut self,
        modifies: Vec<DocElement>,
        deletes: Vec<String>,
        insert_after: Option<String>,
        insert_elements: Vec<DocElement>,
    ) {
        // When a table is deleted, also delete its marker paragraph (if any).
        // Build the marker map before consuming `deletes`.
        let marker_map = self.marker_to_table_map();
        let delete_ids_for_tables: Vec<String> = deletes.clone();

        // Build a set of IDs to delete (includes marker paragraphs for tables).
        let mut delete_set: std::collections::HashSet<String> = deletes.into_iter().collect();
        for tbl_id in &delete_ids_for_tables {
            if let Some(marker_id) = marker_map.get(tbl_id) {
                delete_set.insert(marker_id.clone());
            }
        }

        // Start from current elements
        let elements = self.to_elements();

        // Partition modifies into a lookup map (id -> element)
        let modify_map: std::collections::HashMap<String, DocElement> = modifies
            .into_iter()
            .map(|e| (e.id().to_string(), e))
            .collect();

        // Build result: apply deletes and replaces
        let mut result: Vec<DocElement> = Vec::new();

        for elem in elements {
            if delete_set.contains(elem.id()) {
                continue; // skip deleted
            }
            if let Some(replacement) = modify_map.get(elem.id()) {
                // Preserve original style/runs when replacing a paragraph unless the
                // replacement explicitly provides them
                let to_push = match (elem, replacement.clone()) {
                    (DocElement::Paragraph { id: oi, text: ot, style: os, runs: ors },
                     DocElement::Paragraph { id: ri, text: rt, style: rs, runs: rr }) => {
                        DocElement::Paragraph {
                            id: ri,
                            text: rt,
                            style: rs.or(os),
                            runs: rr.or(ors),
                        }
                    }
                    (e, r) => {
                        // Non-paragraph: replace as-is
                        if r.id() != e.id() {
                            r
                        } else {
                            r
                        }
                    }
                };
                result.push(to_push);
            } else {
                result.push(elem);
            }
        }

        // Handle insertions
        if let Some(ref aid) = insert_after {
            let pos = result.iter().position(|e| e.id() == aid);
            if let Some(idx) = pos {
                // Insert new elements after position idx
                let insert_idx = idx + 1;
                let mut new_items: Vec<DocElement> = Vec::new();
                for e in insert_elements {
                    // Don't re-insert elements that are also in modifies (already placed)
                    if !modify_map.contains_key(e.id()) {
                        new_items.push(e);
                    }
                }
                result.splice(insert_idx..insert_idx, new_items);
            } else {
                // anchor not found, append
                for e in insert_elements {
                    if !modify_map.contains_key(e.id()) {
                        result.push(e);
                    }
                }
            }
        } else if !insert_elements.is_empty() {
            for e in insert_elements {
                if !modify_map.contains_key(e.id()) {
                    result.push(e);
                }
            }
        }

        // Rebuild document from result
        *self = Self::from_elements(result);
    }
}

/// Trait for getting the ID out of a DocElement.
pub trait ElementId {
    fn id(&self) -> &str;
}

impl ElementId for DocElement {
    fn id(&self) -> &str {
        match self {
            DocElement::Paragraph { ref id, .. } => id,
            DocElement::Table { ref id, .. } => id,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WordDocument {
    pub paragraphs: Vec<WordParagraph>,
    pub tables: Vec<WordTable>,
}

pub fn read_word_document(bytes: &[u8]) -> Result<WordDocument, OfficeError> {
    let doc_content = read_zip_entry(bytes, "word/document.xml")?;
    let paragraphs = parse_document_xml(&doc_content)?;
    let tables = parse_table_xml(&doc_content)?;
    Ok(WordDocument { paragraphs, tables })
}

pub fn word_document_to_text(doc: &WordDocument) -> String {
    let mut output = String::new();

    if doc.tables.is_empty() {
        for para in &doc.paragraphs {
            if let Some(ref style) = para.style {
                output.push_str(&format!("[{}] ", style));
            }
            output.push_str(&para.text);
            output.push_str("\n\n");
        }
    } else {
        let mut para_idx = 0;
        let mut rendered_table_rows: usize = 0;
        let mut in_table_block = false;

        for para in &doc.paragraphs {
            if rendered_table_rows > 0 {
                rendered_table_rows -= 1;
                if rendered_table_rows == 0 {
                    in_table_block = false;
                }
                para_idx += 1;
                continue;
            }

            if !in_table_block && !doc.tables.is_empty() && para.text.len() < 80 {
                let ahead_end = (para_idx + 5).min(doc.paragraphs.len());
                let ahead: Vec<_> = doc.paragraphs[para_idx..ahead_end]
                    .iter()
                    .filter(|p| !p.text.trim().is_empty())
                    .collect();

                if ahead.len() >= 2 {
                    let all_short = ahead.iter().all(|p| p.text.len() < 100);
                    let similar_length = ahead.len() > 1
                        && ahead.windows(2).all(|w| {
                            let diff = (w[0].text.len() as i32 - w[1].text.len() as i32).abs();
                            diff < 30
                        });

                    if all_short && similar_length {
                        output.push_str("--- Tables ---\n");
                        for tbl in &doc.tables {
                            for row in &tbl.rows {
                                let cells: Vec<String> = row.cells.iter().map(|c| c.text.clone()).collect();
                                output.push_str(&format!("| {}\n", cells.join(" | ")));
                            }
                            output.push('\n');
                            rendered_table_rows = tbl.rows.len();
                        }
                        in_table_block = true;
                        para_idx += 1;
                        continue;
                    }
                }
            }

            if !in_table_block {
                if let Some(ref style) = para.style {
                    output.push_str(&format!("[{}] ", style));
                }
                output.push_str(&para.text);
                output.push_str("\n\n");
            }
            para_idx += 1;
        }
    }

    output.trim().to_string()
}

// ─── XML Parsing ───────────────────────────────────────────────────────────────

fn parse_document_xml(content: &str) -> Result<Vec<WordParagraph>, OfficeError> {
    let mut paragraphs = Vec::new();
    let mut reader = quick_xml::Reader::from_str(content);
    reader.config_mut().trim_text(false);

    let mut buf = Vec::new();
    let mut in_para = false;
    let mut current_text = String::new();
    let mut current_style: Option<String> = None;
    let mut para_depth = 0usize;
    // Track whether we are inside a table cell — paragraphs inside cells must NOT
    // be added to the top-level paragraph list (they are stored separately in tables).
    let mut tbl_cell_depth = 0usize;
    let mut para_counter = 0usize;

    loop {
        let event = reader.read_event_into(&mut buf);
        match event {
            Ok(quick_xml::events::Event::Start(ref e)) => {
                let name = e.local_name();
                if name.as_ref() == b"tc" {
                    tbl_cell_depth += 1;
                } else if name.as_ref() == b"p" {
                    in_para = true;
                    para_depth += 1;
                    current_text.clear();
                    current_style = None;
                } else if name.as_ref() == b"t" && in_para {
                    if let Ok(quick_xml::events::Event::Text(t)) = reader.read_event_into(&mut buf) {
                        current_text.push_str(&t.unescape().unwrap_or_default());
                    }
                } else if name.as_ref() == b"pStyle" {
                    for attr in e.attributes().with_checks(false) {
                        if let Ok(attr) = attr {
                            if attr.key.as_ref() == b"val" {
                                if let Ok(v) = std::str::from_utf8(&attr.value) {
                                    current_style = Some(v.to_string());
                                }
                            }
                        }
                    }
                    if let Ok(quick_xml::events::Event::Text(t)) = reader.read_event_into(&mut buf) {
                        let val = t.unescape().unwrap_or_default();
                        if !val.is_empty() {
                            current_style = Some(val.to_string());
                        }
                    }
                }
            }
            Ok(quick_xml::events::Event::Empty(ref e)) => {
                let name = e.local_name();
                if name.as_ref() == b"p" {
                    in_para = true;
                    para_depth += 1;
                    current_text.clear();
                    current_style = None;
                } else if name.as_ref() == b"pStyle" {
                    for attr in e.attributes().with_checks(false) {
                        if let Ok(attr) = attr {
                            if attr.key.as_ref() == b"val" {
                                if let Ok(v) = std::str::from_utf8(&attr.value) {
                                    current_style = Some(v.to_string());
                                }
                            }
                        }
                    }
                }
            }
            Ok(quick_xml::events::Event::End(ref e)) => {
                let name = e.local_name();
                if name.as_ref() == b"tc" {
                    tbl_cell_depth = tbl_cell_depth.saturating_sub(1);
                } else if name.as_ref() == b"p" {
                    para_depth = para_depth.saturating_sub(1);
                    if para_depth == 0 && tbl_cell_depth == 0 {
                        in_para = false;
                        let text = current_text.trim().to_string();
                        if !text.is_empty() {
                            let id = format!("p{}", para_counter);
                            para_counter += 1;
                            paragraphs.push(WordParagraph {
                                id,
                                text,
                                style: current_style.clone(),
                                runs: None,
                            });
                        }
                    }
                }
            }
            Ok(quick_xml::events::Event::Eof) => break,
            Err(e) => return Err(OfficeError::Xml(format!("XML parse error: {}", e))),
            _ => {}
        }
        buf.clear();
    }

    Ok(paragraphs)
}

fn parse_table_xml(content: &str) -> Result<Vec<WordTable>, OfficeError> {
    let mut tables = Vec::new();
    let mut reader = quick_xml::Reader::from_str(content);
    reader.config_mut().trim_text(false);

    let mut buf = Vec::new();
    let mut current_table: Option<WordTable> = None;
    let mut current_row: Option<Vec<TableCell>> = None;
    let mut current_cell_text = String::new();
    let mut cell_col_span: usize = 1;
    let mut cell_row_span: usize = 1;
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
                        current_table = Some(WordTable {
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
                        cell_row_span = 1;
                    }
                    b"t" if cell_depth > 0 => {
                        if let Ok(quick_xml::events::Event::Text(t)) = reader.read_event_into(&mut buf) {
                            current_cell_text.push_str(&t.unescape().unwrap_or_default());
                        }
                    }
                    b"gridSpan" => {
                        if let Ok(quick_xml::events::Event::Text(t)) = reader.read_event_into(&mut buf) {
                            if let Ok(n) = t.unescape().unwrap_or_default().parse::<usize>() {
                                cell_col_span = n;
                            }
                        }
                    }
                    _ => {}
                }
            }
            Ok(quick_xml::events::Event::Empty(ref e)) => {
                let name = e.local_name();
                if name.as_ref() == b"gridSpan" {
                    for attr in e.attributes().with_checks(false) {
                        if let Ok(attr) = attr {
                            if attr.key.as_ref() == b"val" {
                                let val = std::str::from_utf8(&attr.value).unwrap_or("1");
                                if let Ok(n) = val.parse::<usize>() {
                                    cell_col_span = n;
                                }
                            }
                        }
                    }
                }
            }
            Ok(quick_xml::events::Event::End(ref e)) => {
                let name = e.local_name();
                match name.as_ref() {
                    b"tc" => {
                        cell_depth -= 1;
                        if cell_depth == 0 {
                            if let Some(ref mut row) = current_row {
                                row.push(TableCell {
                                    text: current_cell_text.trim().to_string(),
                                    col_span: cell_col_span,
                                    row_span: cell_row_span,
                                });
                            }
                        }
                    }
                    b"tr" => {
                        row_depth -= 1;
                        if row_depth == 0 {
                            if let Some(row) = current_row.take() {
                                if let Some(ref mut tbl) = current_table {
                                    tbl.rows.push(TableRow { cells: row });
                                }
                            }
                        }
                    }
                    b"tbl" => {
                        table_depth -= 1;
                        if table_depth == 0 {
                            if let Some(table) = current_table.take() {
                                if !table.rows.is_empty() {
                                    tables.push(table);
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

    Ok(tables)
}

// ─── Write Functions ──────────────────────────────────────────────────────────

/// Files we always regenerate (these define the document structure).
/// All other entries (styles, settings, fonts, images, etc.) are copied from
/// the original file to preserve custom formatting and embedded objects.
const GENERATED_FILES: &[&str] = &[
    "[Content_Types].xml",
    "_rels/.rels",
    "word/_rels/document.xml.rels",
    "word/document.xml",
    // NOTE: word/styles.xml, word/settings.xml, word/fontTable.xml and
    // word/theme/theme1.xml are intentionally NOT here — they are copied from
    // the original file so custom styles and formatting are preserved.
];

/// Write a Word document to a .docx file.
/// If `preserve_from` is Some(bytes), all ZIP entries from the original file are
/// copied over first, then the generated content (document.xml, styles.xml, etc.)
/// is used to replace the corresponding entries. This preserves styles, images,
/// headers, footers, custom settings and any other embedded parts.
/// Falls back to hardcoded boilerplate when `preserve_from` is None.
pub fn write_word_document<W: std::io::Write + std::io::Seek>(
    doc: &WordDocument,
    output: W,
    preserve_from: Option<&[u8]>,
) -> Result<(), OfficeError> {
    let mut zip = zip::ZipWriter::new(output);
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .unix_permissions(0o644);

    // Build the generated content strings once
    let doc_xml = build_document_xml(doc);
    let content_types = CONTENT_TYPES_XML;
    let rels = RELS_XML;
    let word_rels = WORD_RELS_XML;
    let styles = STYLES_XML;
    let settings = SETTINGS_XML;
    let font_table = FONT_TABLE_XML;
    let theme = THEME_XML;

    if let Some(bytes) = preserve_from {
        // Copy all original entries first
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes))?;
        for i in 0..archive.len() {
            let mut file = archive.by_index(i)?;
            let name = file.name().to_string();
            // Skip entries we'll generate fresh (they'll be overwritten below)
            if GENERATED_FILES.contains(&name.as_str()) {
                continue;
            }
            let mut content = Vec::new();
            file.read_to_end(&mut content)?;

            // Preserve the original compression method
            let file_opts = if file.compression() == zip::CompressionMethod::Deflated {
                opts
            } else {
                zip::write::SimpleFileOptions::default()
                    .compression_method(zip::CompressionMethod::Stored)
                    .unix_permissions(0o644)
            };

            zip.start_file(&name, file_opts)?;
            zip.write_all(&content)?;
        }
    }

    // Always write the generated (up-to-date) entries last so they take precedence
    zip.start_file("[Content_Types].xml", opts)?;
    zip.write_all(content_types.as_bytes())?;

    zip.start_file("_rels/.rels", opts)?;
    zip.write_all(rels.as_bytes())?;

    zip.start_file("word/_rels/document.xml.rels", opts)?;
    zip.write_all(word_rels.as_bytes())?;

    zip.start_file("word/document.xml", opts)?;
    zip.write_all(doc_xml.as_bytes())?;

    // Only write hardcoded styles/settings/fontTable/theme when no original is
    // being preserved. When `preserve_from` is Some, those entries are already
    // copied from the original zip above, so writing them again would produce
    // duplicate filenames in the resulting archive.
    if preserve_from.is_none() {
        zip.start_file("word/styles.xml", opts)?;
        zip.write_all(styles.as_bytes())?;

        zip.start_file("word/settings.xml", opts)?;
        zip.write_all(settings.as_bytes())?;

        zip.start_file("word/fontTable.xml", opts)?;
        zip.write_all(font_table.as_bytes())?;

        zip.start_file("word/theme/theme1.xml", opts)?;
        zip.write_all(theme.as_bytes())?;
    }

    zip.finish()?;
    Ok(())
}

/// Convenience wrapper that writes to a file path.
pub fn write_word_document_to_path(
    doc: &WordDocument,
    output_path: &std::path::Path,
    preserve_from: Option<&[u8]>,
) -> Result<(), OfficeError> {
    let file = std::fs::File::create(output_path)?;
    let buf = std::io::BufWriter::new(file);
    write_word_document(doc, buf, preserve_from)
}

pub fn build_run_xml(run: &FontRun) -> String {
    let mut xml = String::from("<w:r>");
    let mut rpr = String::new();

    if run.bold { rpr.push_str("<w:b/>"); }
    if run.italic { rpr.push_str("<w:i/>"); }
    if run.underline { rpr.push_str("<w:u w:val=\"single\"/>"); }
    if let Some(ref color) = run.color {
        if !color.is_empty() {
            rpr.push_str(&format!("<w:color w:val=\"{}\"/>", escape_xml(color)));
        }
    }
    if let Some(size) = run.font_size {
        rpr.push_str(&format!("<w:sz w:val=\"{}\"/>", size));
        rpr.push_str(&format!("<w:szCs w:val=\"{}\"/>", size));
    }
    if let Some(ref font) = run.font_name {
        rpr.push_str(&format!("<w:rFonts w:ascii=\"{}\" w:hAnsi=\"{}\"/>", escape_xml(font), escape_xml(font)));
    }

    if !rpr.is_empty() {
        xml.push_str("<w:rPr>");
        xml.push_str(&rpr);
        xml.push_str("</w:rPr>");
    }

    xml.push_str(&format!(
        "<w:t xml:space=\"preserve\">{}</w:t></w:r>",
        escape_xml(&run.text)
    ));
    xml
}

pub fn build_document_xml(doc: &WordDocument) -> String {
    let mut xml = String::from(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"
            xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
  <w:body>"#
    );

    for para in &doc.paragraphs {
        // Skip table-position marker paragraphs entirely (they are used only to
        // link tables to their position in the flattened element list).
        if para.text.starts_with("<__tbl_pos_") {
            continue;
        }
        xml.push_str("\n    <w:p>");
        if let Some(ref style) = para.style {
            xml.push_str(&format!("<w:pPr><w:pStyle w:val=\"{}\"/></w:pPr>", escape_xml(style)));
        }

        if let Some(ref runs) = para.runs {
            for run in runs {
                xml.push_str(&build_run_xml(run));
            }
        } else {
            for chunk in para.text.split('\n') {
                if !chunk.is_empty() {
                    xml.push_str(&format!(
                        "<w:r><w:t xml:space=\"preserve\">{}</w:t></w:r>",
                        escape_xml(chunk)
                    ));
                }
                xml.push_str("<w:r><w:br/></w:r>");
            }
        }
        xml.push_str("</w:p>");
    }

    // Render tables separately
    for table in &doc.tables {
        xml.push_str("\n    <w:tbl>");
        xml.push_str("\n      <w:tblPr>");
        xml.push_str("<w:tblStyle w:val=\"TableGrid\"/>");
        xml.push_str("<w:tblW w:type=\"auto\" w:w=\"0\"/>");
        xml.push_str("<w:tblBorders>");
        xml.push_str("<w:top w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"auto\"/>");
        xml.push_str("<w:left w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"auto\"/>");
        xml.push_str("<w:bottom w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"auto\"/>");
        xml.push_str("<w:right w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"auto\"/>");
        xml.push_str("<w:insideH w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"auto\"/>");
        xml.push_str("<w:insideV w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"auto\"/>");
        xml.push_str("</w:tblBorders>");
        xml.push_str("</w:tblPr>");
        for row in &table.rows {
            xml.push_str("\n        <w:tr>");
            for cell in &row.cells {
                xml.push_str("<w:tc><w:tcPr>");
                if cell.col_span > 1 {
                    xml.push_str(&format!("<w:gridSpan w:val=\"{}\"/>", cell.col_span));
                }
                xml.push_str("</w:tcPr><w:p>");
                let lines: Vec<&str> = cell.text.split('\n').collect();
                for (chunk_idx, chunk) in lines.iter().enumerate() {
                    if !chunk.is_empty() {
                        xml.push_str(&format!(
                            "<w:r><w:t xml:space=\"preserve\">{}</w:t></w:r>",
                            escape_xml(chunk)
                        ));
                    }
                    if chunk_idx < lines.len().saturating_sub(1) {
                        xml.push_str("<w:r><w:br/></w:r>");
                    }
                }
                xml.push_str("</w:p></w:tc>");
            }
            xml.push_str("</w:tr>");
        }
        xml.push_str("\n    </w:tbl>");
    }

    xml.push_str("\n  </w:body>\n</w:document>");
    xml
}

pub fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

// ─── Minimal OOXML boilerplate ────────────────────────────────────────────────

pub const CONTENT_TYPES_XML: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
  <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
  <Default Extension="xml" ContentType="application/xml"/>
  <Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>
  <Override PartName="/word/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml"/>
  <Override PartName="/word/settings.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.settings+xml"/>
  <Override PartName="/word/fontTable.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.fontTable+xml"/>
  <Override PartName="/word/theme/theme1.xml" ContentType="application/vnd.openxmlformats-officedocument.theme+xml"/>
</Types>"#;

pub const RELS_XML: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/>
</Relationships>"#;

pub const WORD_RELS_XML: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/>
  <Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/settings" Target="settings.xml"/>
  <Relationship Id="rId3" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/fontTable" Target="fontTable.xml"/>
  <Relationship Id="rId4" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/theme" Target="theme/theme1.xml"/>
</Relationships>"#;

pub const STYLES_XML: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:docDefaults>
    <w:rPrDefault>
      <w:rPr>
        <w:rFonts w:ascii="Calibri" w:hAnsi="Calibri" w:cs="Times New Roman"/>
        <w:sz w:val="22"/>
        <w:szCs w:val="22"/>
      </w:rPr>
    </w:rPrDefault>
  </w:docDefaults>

  <w:style w:type="paragraph" w:styleId="Normal">
    <w:name w:val="Normal"/>
    <w:pPr>
      <w:spacing w:after="200" w:line="276" w:lineRule="auto"/>
    </w:pPr>
    <w:rPr>
      <w:rFonts w:ascii="Calibri" w:hAnsi="Calibri"/>
      <w:sz w:val="22"/>
    </w:rPr>
  </w:style>

  <w:style w:type="paragraph" w:styleId="Title">
    <w:name w:val="Title"/>
    <w:basedOn w:val="Normal"/>
    <w:pPr>
      <w:jc w:val="center"/>
      <w:spacing w:after="0" w:before="240"/>
    </w:pPr>
    <w:rPr>
      <w:rFonts w:ascii="Calibri" w:hAnsi="Calibri"/>
      <w:b/>
      <w:sz w:val="56"/>
      <w:szCs w:val="56"/>
      <w:color w:val="1F3864"/>
    </w:rPr>
  </w:style>

  <w:style w:type="paragraph" w:styleId="Heading1">
    <w:name w:val="Heading 1"/>
    <w:basedOn w:val="Normal"/>
    <w:pPr>
      <w:keepNext/>
      <w:keepLines/>
      <w:spacing w:before="480" w:after="120"/>
      <w:outlineLvl w:val="0"/>
    </w:pPr>
    <w:rPr>
      <w:rFonts w:ascii="Calibri" w:hAnsi="Calibri"/>
      <w:b/>
      <w:sz w:val="32"/>
      <w:szCs w:val="32"/>
      <w:color w:val="2E74B5"/>
    </w:rPr>
  </w:style>

  <w:style w:type="paragraph" w:styleId="Heading2">
    <w:name w:val="Heading 2"/>
    <w:basedOn w:val="Normal"/>
    <w:pPr>
      <w:keepNext/>
      <w:keepLines/>
      <w:spacing w:before="360" w:after="80"/>
      <w:outlineLvl w:val="1"/>
    </w:pPr>
    <w:rPr>
      <w:rFonts w:ascii="Calibri" w:hAnsi="Calibri"/>
      <w:b/>
      <w:sz w:val="26"/>
      <w:szCs w:val="26"/>
      <w:color w:val="2F5496"/>
    </w:rPr>
  </w:style>

  <w:style w:type="paragraph" w:styleId="Heading3">
    <w:name w:val="Heading 3"/>
    <w:basedOn w:val="Normal"/>
    <w:pPr>
      <w:keepNext/>
      <w:keepLines/>
      <w:spacing w:before="240" w:after="60"/>
      <w:outlineLvl w:val="2"/>
    </w:pPr>
    <w:rPr>
      <w:rFonts w:ascii="Calibri" w:hAnsi="Calibri"/>
      <w:b/>
      <w:sz w:val="24"/>
      <w:szCs w:val="24"/>
      <w:color w:val="1F497D"/>
    </w:rPr>
  </w:style>

  <w:style w:type="table" w:styleId="TableGrid">
    <w:name w:val="Table Grid"/>
    <w:tblPr>
      <w:tblBorders>
        <w:top w:val="single" w:sz="4" w:space="0" w:color="auto"/>
        <w:left w:val="single" w:sz="4" w:space="0" w:color="auto"/>
        <w:bottom w:val="single" w:sz="4" w:space="0" w:color="auto"/>
        <w:right w:val="single" w:sz="4" w:space="0" w:color="auto"/>
        <w:insideH w:val="single" w:sz="4" w:space="0" w:color="auto"/>
        <w:insideV w:val="single" w:sz="4" w:space="0" w:color="auto"/>
      </w:tblBorders>
    </w:tblPr>
    <w:tcPr>
      <w:tcMar>
        <w:top w:w="80" w:type="dxa"/>
        <w:left w:w="108" w:type="dxa"/>
        <w:bottom w:w="80" w:type="dxa"/>
        <w:right w:w="108" w:type="dxa"/>
      </w:tcMar>
    </w:tcPr>
    <w:rPr>
      <w:sz w:val="20"/>
    </w:rPr>
  </w:style>

</w:styles>"#;

pub const SETTINGS_XML: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:settings xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:defaultTabStop w:val="720"/>
</w:settings>"#;

pub const FONT_TABLE_XML: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:fonts xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:font w:name="Calibri">
    <w:panose1 w:val="020F0502020204030204"/>
    <w:charset w:val="00"/>
    <w:family w:val="swiss"/>
    <w:pitch w:val="variable"/>
  </w:font>
  <w:font w:name="Times New Roman">
    <w:panose1 w:val="02020603050405020304"/>
    <w:charset w:val="00"/>
    <w:family w:val="roman"/>
    <w:pitch w:val="variable"/>
  </w:font>
</w:fonts>"#;

pub const THEME_XML: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<a:theme xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" name="Office Theme">
  <a:themeElements>
    <a:clrScheme name="Office">
      <a:dk1><a:sysClr val="windowText" lastClr="000000"/></a:dk1>
      <a:lt1><a:sysClr val="window" lastClr="FFFFFF"/></a:lt1>
      <a:dk2><a:srgbClr val="1F497D"/></a:dk2>
      <a:lt2><a:srgbClr val="EEECE1"/></a:lt2>
      <a:accent1><a:srgbClr val="4F81BD"/></a:accent1>
      <a:accent2><a:srgbClr val="C0504D"/></a:accent2>
      <a:accent3><a:srgbClr val="9BBB59"/></a:accent3>
      <a:accent4><a:srgbClr val="8064A2"/></a:accent4>
      <a:accent5><a:srgbClr val="4BACC6"/></a:accent5>
      <a:accent6><a:srgbClr val="F79646"/></a:accent6>
      <a:hlink><a:srgbClr val="0000FF"/></a:hlink>
      <a:folHlink><a:srgbClr val="800080"/></a:folHlink>
    </a:clrScheme>
    <a:fontScheme name="Office">
      <a:majorFont><a:latin typeface="Calibri"/><a:ea typeface=""/><a:cs typeface="Times New Roman"/></a:majorFont>
      <a:minorFont><a:latin typeface="Calibri"/><a:ea typeface=""/><a:cs typeface="Times New Roman"/></a:minorFont>
    </a:fontScheme>
    <a:fmtScheme name="Office">
      <a:fillStyleLst>
        <a:solidFill><a:schemeClr val="phClr"/></a:solidFill>
        <a:gradFill rotWithShape="1"><a:gsLst><a:gs pos="0"><a:schemeClr val="phClr"><a:tint val="65000"/></a:schemeClr></a:gs><a:gs pos="100000"><a:schemeClr val="phClr"><a:shade val="99000"/></a:schemeClr></a:gs></a:gsLst><a:lin ang="5400000" scaled="0"/></a:gradFill>
      </a:fillStyleLst>
      <a:lnStyleLst>
        <a:ln w="9525" cap="flat" cmpd="sng" algn="ctr"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:prstDash val="solid"/></a:ln>
        <a:ln w="25400" cap="flat" cmpd="sng" algn="ctr"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:prstDash val="solid"/></a:ln>
        <a:ln w="38100" cap="flat" cmpd="sng" algn="ctr"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:prstDash val="solid"/></a:ln>
      </a:lnStyleLst>
      <a:effectStyleLst>
        <a:effectStyle><a:effectLst/></a:effectStyle>
        <a:effectStyle><a:effectLst/></a:effectStyle>
        <a:effectStyle><a:effectLst/></a:effectStyle>
      </a:effectStyleLst>
      <a:bgFillStyleLst>
        <a:solidFill><a:schemeClr val="phClr"/></a:solidFill>
        <a:gradFill rotWithShape="1"><a:gsLst><a:gs pos="0"><a:schemeClr val="phClr"><a:tint val="95000"/></a:schemeClr></a:gs><a:gs pos="100000"><a:schemeClr val="phClr"><a:shade val="85000"/></a:schemeClr></a:gs></a:gsLst></a:gradFill>
      </a:bgFillStyleLst>
    </a:fmtScheme>
  </a:themeElements>
  <a:objectDefaults/>
  <a:extraClrSchemeLst/>
</a:theme>"#;
