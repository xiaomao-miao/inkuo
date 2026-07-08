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
    pub strikethrough: bool,
    #[serde(default)]
    pub font_size: Option<u32>,  // half-points, e.g. 24 = 12pt
    #[serde(default)]
    pub color: Option<String>,   // hex RGB, e.g. "FF0000"
    #[serde(default)]
    pub font_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub highlight: Option<String>,  // e.g. "yellow", "green", "red"
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
    /// List/numbering reference id, when this paragraph is part of a list.
    /// `Some((num_id, ilvl))` means "this paragraph is item ilvl of list num_id".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub numbering: Option<NumberingRef>,
}

/// Reference to a list/numbering definition: which numbered/bulleted list the
/// paragraph belongs to and at which indent level.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NumberingRef {
    /// `w:numId` — the list instance id (a docx can have many instances of the
    /// same abstract definition).
    pub num_id: u32,
    /// `w:ilvl` — zero-based indent level (0..=8).
    pub level: u32,
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
        /// `true` means "the caller did not provide a `text` field; the original
        /// text should be kept as-is during a modify operation." This is encoded
        /// as a separate boolean (rather than making `text` an Option) to keep
        /// the JSON wire format simple and avoid breaking existing callers.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        omit_text: bool,
        style: Option<String>,
        #[serde(default)]
        runs: Option<Vec<FontRun>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        numbering: Option<NumberingRef>,
    },
    #[serde(rename = "table")]
    Table {
        id: String,
        /// Zero-based position among all document elements (0 = before p0).
        #[serde(default)]
        position: usize,
        header: Vec<TableCell>,
        rows: Vec<Vec<TableCell>>,
    },
}

/// Element with insertion metadata for positioned insertions
#[derive(Debug, Clone)]
pub struct InsertElement {
    pub element: DocElement,
    pub anchor_id: Option<String>,
    pub position: Option<String>,
}

impl WordDocument {
    /// Convert the document to a flat list of elements with stable IDs.
    /// Tables and paragraphs are interleaved by matching position markers
    /// to tables in sequential order.
    pub fn to_elements(&self) -> Vec<DocElement> {
        // Build a map of table id -> table for O(1) lookup.
        let table_map: std::collections::HashMap<&str, &WordTable> =
            self.tables.iter().map(|t| (t.id.as_str(), t)).collect();

        // Tables already emitted via a marker paragraph (see loop below) are
        // recorded here so the final "append remaining tables" pass doesn't
        // double-count them.
        let mut tables_emitted: std::collections::HashSet<String> = std::collections::HashSet::new();

        let mut elements: Vec<DocElement> = Vec::with_capacity(self.paragraphs.len() + self.tables.len());

        for p in &self.paragraphs {
            if let Some(rest) = p.text.strip_prefix("<__tbl_pos_") {
                if let Some(end) = rest.find("__>") {
                    let tbl_id = &rest[..end];
                    if let Some(tbl) = table_map.get(tbl_id) {
                        let (header, rows) = split_table(tbl);
                        elements.push(DocElement::Table {
                            id: tbl.id.clone(),
                            position: elements.len(),
                            header,
                            rows,
                        });
                        tables_emitted.insert(tbl.id.clone());
                    }
                    continue;
                }
            }
            elements.push(DocElement::Paragraph {
                id: p.id.clone(),
                text: p.text.clone(),
                omit_text: false,
                style: p.style.clone(),
                runs: p.runs.clone(),
                numbering: p.numbering.clone(),
            });
        }
        // Tables without preceding markers (e.g. freshly parsed documents
        // that have no position marker paragraphs yet) are appended at the
        // end so callers like the agent's round-trip logic still see them.
        for tbl in &self.tables {
            if !tables_emitted.contains(tbl.id.as_str()) {
                let (header, rows) = split_table(tbl);
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
}

/// Split a `WordTable` into `(header, rows)` for `DocElement::Table`, carrying
/// merge information (col_span / row_span) all the way through. Used by
/// `WordDocument::to_elements` so that round-tripping a parsed document
/// preserves merged-cell layout.
fn split_table(tbl: &WordTable) -> (Vec<TableCell>, Vec<Vec<TableCell>>) {
    if tbl.rows.is_empty() {
        return (Vec::new(), Vec::new());
    }
    let header: Vec<TableCell> = tbl.rows[0].cells.clone();
    let rows: Vec<Vec<TableCell>> = tbl.rows[1..]
        .iter()
        .map(|r| r.cells.clone())
        .collect();
    (header, rows)
}

impl WordDocument {
    /// Build a WordDocument from a list of elements.
    /// Each table is preceded by a marker paragraph whose ID encodes the table's own ID
    /// (e.g. table id "t0" → marker id "__tbl_pos_t0__"), so deletions of the table
    /// by ID also remove the marker.
    pub fn from_elements(elements: Vec<DocElement>) -> Self {
        let mut out_paras: Vec<WordParagraph> = Vec::new();
        let mut tables: Vec<WordTable> = Vec::new();

        for elem in elements {
            match elem {
                DocElement::Paragraph { id, text, style, runs, numbering, .. } => {
                    out_paras.push(WordParagraph { id, text, style, runs, numbering });
                }
                DocElement::Table { id, position: _, header, rows } => {
                    // Emit a position marker whose ID matches the table's ID.
                    // This lets delete_set remove both the marker and the table together.
                    out_paras.push(WordParagraph {
                        id: format!("__tbl_pos_{}__", id),
                        text: format!("<__tbl_pos_{}__>", id),
                        style: None,
                        runs: None,
                        numbering: None,
                    });

                    let mut table_rows = vec![];
                    if !header.is_empty() {
                        table_rows.push(TableRow { cells: header });
                    }
                    for row in rows {
                        if !row.is_empty() {
                            table_rows.push(TableRow { cells: row });
                        }
                    }
                    tables.push(WordTable { id, rows: table_rows });
                }
            }
        }

        WordDocument { paragraphs: out_paras, tables }
    }

    /// Modify the document by applying a list of edit operations.
    /// 
    /// Bug fixes:
    /// - Bug 2: Fixed omit_text logic - when omit_text is false and text is provided, use the new text
    /// - Bug 3: Preserve original IDs by not calling from_elements which reassigns IDs
    /// - Bug 4: Support "before" position for anchor insertions
    /// - Bug 6: Support multiple elements each with their own anchor_id and position
    pub fn modify(
        &mut self,
        modifies: Vec<DocElement>,
        deletes: Vec<String>,
        insert_elements: Vec<InsertElement>,
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
                // Preserve original style/runs/text when replacing a paragraph
                // unless the replacement explicitly provides them. AI callers
                // can omit fields to mean "keep what's already there".
                let to_push = match (elem, replacement.clone()) {
                    (DocElement::Paragraph { id: _oi, text: ot, style: os, runs: ors, numbering: onum, .. },
                     DocElement::Paragraph { id: ri, text: rt, style: rs, runs: rr, numbering: rnum, omit_text }) => {
                        // Merge strategy:
                        // 1. If runs provided in replacement -> use replacement runs (full override)
                        // 2. If text provided (omit_text=false) but no runs -> use text, clear runs
                        // 3. If nothing provided (omit_text=true, no runs) -> keep originals
                        
                        let merged_style = rs.or(os);
                        let merged_numbering = rnum.or(onum);
                        
                        let (out_text, out_runs) = if rr.is_some() {
                            // User provided runs -> use runs, ignore text field
                            (String::new(), rr)
                        } else if !omit_text {
                            // User provided text but no runs -> use new text, clear runs
                            (rt, None)
                        } else {
                            // User provided neither -> keep originals
                            (ot, ors)
                        };
                        
                        DocElement::Paragraph {
                            id: ri,
                            text: out_text,
                            omit_text: false,
                            style: merged_style,
                            runs: out_runs,
                            numbering: merged_numbering,
                        }
                    }
                    // Table replace: pass through. `modify` is only called with
                    // a modify_map keyed by element id, so id collisions between
                    // a paragraph and a table are impossible by construction.
                    (_e, r) => r,
                };
                result.push(to_push);
            } else {
                result.push(elem);
            }
        }

        // Handle insertions - each element can have its own anchor_id and position
        for insert_elem in insert_elements {
            // Skip if already in modifies (already placed)
            if modify_map.contains_key(insert_elem.element.id()) {
                continue;
            }
            
            if let Some(ref aid) = insert_elem.anchor_id {
                // Find anchor position in current result
                let pos = result.iter().position(|e| e.id() == aid);
                if let Some(idx) = pos {
                    // Insert at the specified position relative to anchor
                    let insert_idx = match insert_elem.position.as_deref() {
                        Some("before") => idx,      // Insert before the anchor
                        _ => idx + 1,                // Default: insert after the anchor
                    };
                    result.insert(insert_idx, insert_elem.element);
                } else {
                    // Anchor not found, append to end
                    result.push(insert_elem.element);
                }
            } else {
                // No anchor specified, append to end
                result.push(insert_elem.element);
            }
        }

        // Bug fix 3: Build document manually to preserve IDs instead of using from_elements
        // from_elements would reassign all IDs via marker paragraphs
        let mut out_paras: Vec<WordParagraph> = Vec::new();
        let mut tables: Vec<WordTable> = Vec::new();

        for elem in result {
            match elem {
                DocElement::Paragraph { id, text, style, runs, numbering, .. } => {
                    out_paras.push(WordParagraph { id, text, style, runs, numbering });
                }
                DocElement::Table { id, position: _, header, rows } => {
                    // Emit a position marker whose text matches the table's ID.
                    out_paras.push(WordParagraph {
                        id: format!("__tbl_pos_{}__", id),
                        text: format!("<__tbl_pos_{}__>", id),
                        style: None,
                        runs: None,
                        numbering: None,
                    });

                    let mut table_rows = vec![];
                    if !header.is_empty() {
                        table_rows.push(super::shared::TableRow { cells: header });
                    }
                    for row in rows {
                        if !row.is_empty() {
                            table_rows.push(super::shared::TableRow { cells: row });
                        }
                    }
                    tables.push(WordTable { id, rows: table_rows });
                }
            }
        }

        self.paragraphs = out_paras;
        self.tables = tables;
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

    // Helper: render a paragraph's runs to a markdown-flavoured string. Falls
    // back to the plain `text` field when no runs are present.
    fn render_paragraph(p: &WordParagraph) -> String {
        if let Some(ref runs) = p.runs {
            if !runs.is_empty() {
                let mut s = String::new();
                for r in runs {
                    let mut chunk = r.text.clone();
                    if r.italic { chunk = format!("*{}*", chunk); }
                    if r.bold { chunk = format!("**{}**", chunk); }
                    if r.underline { chunk = format!("__{}__", chunk); }
                    s.push_str(&chunk);
                }
                return s;
            }
        }
        p.text.clone()
    }

    if doc.tables.is_empty() {
        for para in &doc.paragraphs {
            if let Some(ref style) = para.style {
                output.push_str(&format!("[{}] ", style));
            }
            output.push_str(&render_paragraph(para));
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
                output.push_str(&render_paragraph(para));
                output.push_str("\n\n");
            }
            para_idx += 1;
        }
    }

    output.trim().to_string()
}

// ─── XML Parsing ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
struct RunFormat {
    bold: bool,
    italic: bool,
    underline: bool,
    strikethrough: bool,
    font_size: Option<u32>,
    color: Option<String>,
    font_name: Option<String>,
    highlight: Option<String>,
}

/// Apply a single `<w:rPr>` attribute to a `RunFormat`.
/// `tag` is the local element name (e.g. "b", "i", "u", "color", "sz").
/// When `tag` is "color" or "rFonts" or "sz" / "szCs", `attr_val` carries the attribute.
fn apply_run_attr(fmt: &mut RunFormat, tag: &[u8], attr_val: Option<&[u8]>) {
    match tag {
        b"b" | b"bCs" => fmt.bold = true,
        b"i" | b"iCs" => fmt.italic = true,
        b"u" => fmt.underline = true,
        b"strike" => fmt.strikethrough = true,
        b"highlight" => {
            if let Some(v) = attr_val {
                if let Ok(s) = std::str::from_utf8(v) {
                    if !s.is_empty() {
                        fmt.highlight = Some(s.to_string());
                    }
                }
            }
        }
        b"color" => {
            if let Some(v) = attr_val {
                if let Ok(s) = std::str::from_utf8(v) {
                    // Strip leading '#' if present so output is plain hex.
                    let s = s.trim_start_matches('#');
                    if !s.is_empty() {
                        fmt.color = Some(s.to_string());
                    }
                }
            }
        }
        b"sz" | b"szCs" => {
            if let Some(v) = attr_val {
                if let Ok(s) = std::str::from_utf8(v) {
                    if let Ok(n) = s.parse::<u32>() {
                        fmt.font_size = Some(n);
                    }
                }
            }
        }
        b"rFonts" => {
            // ascii / hAnsi / cs are all valid carriers of the font name.
            if let Some(v) = attr_val {
                if let Ok(s) = std::str::from_utf8(v) {
                    if !s.is_empty() {
                        fmt.font_name = Some(s.to_string());
                    }
                }
            }
        }
        _ => {}
    }
}

/// Walk attributes of an `<w:rPr>` (or any other) start event and apply them to `fmt`.
/// Recognized attributes map to their element siblings — this is the standard OOXML
/// compact form: `<w:b w:val="true"/>` instead of `<w:b><w:val .../></w:b>`.
fn apply_run_attrs_from_event(fmt: &mut RunFormat, e: &quick_xml::events::BytesStart) {
    for attr in e.attributes().with_checks(false).flatten() {
        let key = attr.key.as_ref().to_vec();
        let local = key
            .iter()
            .position(|&b| b == b':')
            .map(|i| &key[i + 1..])
            .unwrap_or(&key[..]);
        let val = attr.value.as_ref();
        apply_run_attr(fmt, local, Some(val));
    }
    // `w:val="false"` / `w:val="0"` should explicitly disable the flag.
    if let Some(val_attr) = e.attributes().with_checks(false).flatten().find(|a| {
        let k = a.key.as_ref();
        k.ends_with(b":val") || k == b"val"
    }) {
        let v = val_attr.value.as_ref();
        let is_off = v == b"false" || v == b"0" || v == b"off";
        if is_off {
            let key = val_attr.key.as_ref().to_vec();
            let local = key
                .iter()
                .position(|&b| b == b':')
                .map(|i| &key[i + 1..])
                .unwrap_or(&key[..]);
            match local {
                b"b" | b"bCs" => fmt.bold = false,
                b"i" | b"iCs" => fmt.italic = false,
                b"u" => fmt.underline = false,
                b"strike" => fmt.strikethrough = false,
                _ => {}
            }
        }
    }
}

fn parse_run_attrs_from_nested(e: &quick_xml::events::BytesStart, fmt: &mut RunFormat) {
    apply_run_attrs_from_event(fmt, e);
}

/// Extract the "val" attribute from a `<w:color w:val="...">` / `<w:sz w:val="...">` / `<w:rFonts w:ascii="...">` etc.
fn attr_value<'a>(e: &'a quick_xml::events::BytesStart, name: &[u8]) -> Option<std::borrow::Cow<'a, [u8]>> {
    for attr in e.attributes().with_checks(false).flatten() {
        let key = attr.key.as_ref().to_vec();
        let local = key
            .iter()
            .position(|&b| b == b':')
            .map(|i| &key[i + 1..])
            .unwrap_or(&key[..]);
        if local == name {
            return Some(std::borrow::Cow::Owned(attr.value.into_owned()));
        }
    }
    None
}

fn parse_document_xml(content: &str) -> Result<Vec<WordParagraph>, OfficeError> {
    let mut paragraphs = Vec::new();
    let mut reader = quick_xml::Reader::from_str(content);
    reader.config_mut().trim_text(false);

    let mut buf = Vec::new();

    // ── Top-level state ────────────────────────────────────────────────────
    let mut para_depth = 0usize;
    let mut tbl_cell_depth = 0usize;
    let mut para_counter = 0usize;

    // ── Per-paragraph state (reset on each <w:p>) ──────────────────────────
    let mut current_text = String::new();
    let mut current_style: Option<String> = None;
    let mut current_runs: Vec<FontRun> = Vec::new();
    let mut current_numbering: Option<NumberingRef> = None;
    let mut current_stable_id: Option<String> = None;
    let mut in_numpr = false;
    let mut pending_num_id: Option<u32> = None;
    let mut pending_ilvl: Option<u32> = None;
    let mut is_table_marker = false;  // Tracks if current paragraph is a table position marker

    // ── Per-run state (reset on each <w:r>) ────────────────────────────────
    let mut in_run = false;
    let mut in_run_props = false;
    let mut current_run_text = String::new();
    let mut current_run_format = RunFormat::default();

    // Track whether this paragraph actually saw any run (even an empty one).
    // We use this to decide whether to keep the paragraph even if it ended up
    // textless — see "preserve empty paragraphs" below.
    let mut paragraph_saw_run = false;

    loop {
        let event = reader.read_event_into(&mut buf);
        match event {
            Ok(quick_xml::events::Event::Start(ref e)) => {
                let name = e.local_name();
                if name.as_ref() == b"tc" {
                    tbl_cell_depth += 1;
                } else if name.as_ref() == b"p" {
                    para_depth += 1;
                    current_text.clear();
                    current_style = None;
                    current_runs.clear();
                    current_numbering = None;
                    current_stable_id = None;
                    is_table_marker = false;  // Reset marker detection for new paragraph
                    in_numpr = false;
                    pending_num_id = None;
                    pending_ilvl = None;
                    paragraph_saw_run = false;
                } else if name.as_ref() == b"r" && tbl_cell_depth == 0 {
                    // Only top-level runs count toward the paragraph's `runs` list.
                    in_run = true;
                    in_run_props = false;
                    current_run_text.clear();
                    current_run_format = RunFormat::default();
                } else if name.as_ref() == b"rPr" && in_run {
                    in_run_props = true;
                    // `<w:rPr>` itself can carry attributes (compact form).
                    parse_run_attrs_from_nested(e, &mut current_run_format);
                } else if in_run_props {
                    // `<w:b/>`, `<w:color w:val="..."/>`, `<w:sz w:val="24"/>` etc.
                    // Use the "compact" attributes path for val-bearing tags.
                    let val = attr_value(e, b"val");
                    let ascii = attr_value(e, b"ascii");
                    let hansi = attr_value(e, b"hAnsi");
                    let cs = attr_value(e, b"cs");
                    apply_run_attr(&mut current_run_format, name.as_ref(), val.as_deref());
                    if let Some(v) = ascii.or(hansi).or(cs) {
                        apply_run_attr(&mut current_run_format, b"rFonts", Some(v.as_ref()));
                    }
                } else if name.as_ref() == b"t" && in_run {
                    if let Ok(quick_xml::events::Event::Text(t)) = reader.read_event_into(&mut buf) {
                        current_run_text.push_str(&t.unescape().unwrap_or_default());
                    }
                } else if name.as_ref() == b"pStyle" {
                    if let Some(v) = attr_value(e, b"val") {
                        if let Ok(s) = std::str::from_utf8(v.as_ref()) {
                            if !s.is_empty() {
                                current_style = Some(s.to_string());
                            }
                        }
                    }
                    // Some writers emit `<w:pStyle>Heading1</w:pStyle>` (text body).
                    if let Ok(quick_xml::events::Event::Text(t)) = reader.read_event_into(&mut buf) {
                        let val = t.unescape().unwrap_or_default();
                        if !val.is_empty() {
                            current_style = Some(val.to_string());
                        }
                    }
                } else if name.as_ref() == b"numPr" {
                    in_numpr = true;
                    pending_num_id = None;
                    pending_ilvl = None;
                } else if in_numpr && name.as_ref() == b"numId" {
                    if let Some(v) = attr_value(e, b"val") {
                        if let Ok(s) = std::str::from_utf8(v.as_ref()) {
                            if let Ok(n) = s.parse::<u32>() {
                                pending_num_id = Some(n);
                            }
                        }
                    }
                } else if in_numpr && name.as_ref() == b"ilvl" {
                    if let Some(v) = attr_value(e, b"val") {
                        if let Ok(s) = std::str::from_utf8(v.as_ref()) {
                            if let Ok(n) = s.parse::<u32>() {
                                pending_ilvl = Some(n);
                            }
                        }
                    }
                } else if name.as_ref() == b"id" && para_depth > 0 && tbl_cell_depth == 0 {
                    // Read stable ID from custom inkuo:id element
                    // Also detect table markers (format: __tbl_pos_<table_id>__)
                    if let Some(v) = attr_value(e, b"val") {
                        if let Ok(s) = std::str::from_utf8(v.as_ref()) {
                            if !s.is_empty() {
                                if s.starts_with("__tbl_pos_") && s.ends_with("__") {
                                    // This is a table position marker
                                    current_stable_id = Some(s.to_string());
                                    is_table_marker = true;
                                } else {
                                    current_stable_id = Some(s.to_string());
                                }
                            }
                        }
                    }
                }
            }
            Ok(quick_xml::events::Event::Empty(ref e)) => {
                let name = e.local_name();
                if name.as_ref() == b"p" {
                    // Self-closing paragraph (e.g. empty <w:p/>).
                    para_depth = para_depth.saturating_sub(0);
                    let id = if let Some(stable_id) = current_stable_id.clone() {
                        stable_id
                    } else {
                        let id = format!("p{}", para_counter);
                        para_counter += 1;
                        id
                    };
                    // Keep if has style OR is a table marker (has inkuo:id)
                    if current_style.is_some() || is_table_marker {
                        let text = if is_table_marker {
                            if let Some(stable_id) = &current_stable_id {
                                if let Some(rest) = stable_id.strip_prefix("__tbl_pos_") {
                                    if let Some(table_id) = rest.strip_suffix("__") {
                                        format!("<__tbl_pos_{}__>", table_id)
                                    } else {
                                        stable_id.clone()
                                    }
                                } else {
                                    stable_id.clone()
                                }
                            } else {
                                String::new()
                            }
                        } else {
                            String::new()
                        };
                        paragraphs.push(WordParagraph {
                            id,
                            text,
                            style: None,
                            runs: None,
                            numbering: None,
                        });
                    }
                } else if name.as_ref() == b"r" && tbl_cell_depth == 0 && para_depth > 0 {
                    // Self-closing run — typically `<w:r><w:br/></w:r>` for line breaks.
                    // We model this by pushing an empty run with whatever format was set.
                    paragraph_saw_run = true;
                    if let Some(style) = current_style.clone() {
                        // ignore runs for paragraphs that haven't been started yet
                        let _ = style;
                    }
                } else if name.as_ref() == b"pStyle" {
                    if let Some(v) = attr_value(e, b"val") {
                        if let Ok(s) = std::str::from_utf8(v.as_ref()) {
                            if !s.is_empty() {
                                current_style = Some(s.to_string());
                            }
                        }
                    }
                } else if in_run_props {
                    // Self-closing run-property children like `<w:strike/>`,
                    // `<w:b/>`, `<w:color w:val="..."/>` come through here.
                    let val = attr_value(e, b"val");
                    let ascii = attr_value(e, b"ascii");
                    let hansi = attr_value(e, b"hAnsi");
                    let cs = attr_value(e, b"cs");
                    apply_run_attr(&mut current_run_format, name.as_ref(), val.as_deref());
                    if let Some(v) = ascii.or(hansi).or(cs) {
                        apply_run_attr(&mut current_run_format, b"rFonts", Some(v.as_ref()));
                    }
                } else if in_numpr && name.as_ref() == b"numId" {
                    // numId is typically a self-closing element like `<w:numId w:val="2"/>`.
                    if let Some(v) = attr_value(e, b"val") {
                        if let Ok(s) = std::str::from_utf8(v.as_ref()) {
                            if let Ok(n) = s.parse::<u32>() {
                                pending_num_id = Some(n);
                            }
                        }
                    }
                } else if in_numpr && name.as_ref() == b"ilvl" {
                    if let Some(v) = attr_value(e, b"val") {
                        if let Ok(s) = std::str::from_utf8(v.as_ref()) {
                            if let Ok(n) = s.parse::<u32>() {
                                pending_ilvl = Some(n);
                            }
                        }
                    }
                } else if name.as_ref() == b"id" && tbl_cell_depth == 0 {
                    // Read stable ID from custom inkuo:id element (empty tag)
                    // This can fire even when para_depth is 0 for self-closing tags
                    if let Some(v) = attr_value(e, b"val") {
                        if let Ok(s) = std::str::from_utf8(v.as_ref()) {
                            if !s.is_empty() {
                                if s.starts_with("__tbl_pos_") && s.ends_with("__") {
                                    current_stable_id = Some(s.to_string());
                                    is_table_marker = true;
                                } else if para_depth > 0 {
                                    current_stable_id = Some(s.to_string());
                                }
                            }
                        }
                    }
                } else if name.as_ref() == b"numPr" {
                    // Self-closing `<w:numPr/>` — empty list (no numId); still
                    // flip the in_numpr flag off in case more events follow.
                    in_numpr = false;
                }
            }
            Ok(quick_xml::events::Event::End(ref e)) => {
                let name = e.local_name();
                if name.as_ref() == b"tc" {
                    tbl_cell_depth = tbl_cell_depth.saturating_sub(1);
                } else if name.as_ref() == b"rPr" {
                    in_run_props = false;
                } else if name.as_ref() == b"numPr" {
                    // Commit the numbering reference when numPr closes.
                    if let Some(num_id) = pending_num_id {
                        current_numbering = Some(NumberingRef {
                            num_id,
                            level: pending_ilvl.unwrap_or(0),
                        });
                    }
                    in_numpr = false;
                } else                 if name.as_ref() == b"r" {
                    in_run = false;
                    in_run_props = false;
                    if tbl_cell_depth == 0 && para_depth > 0 {
                        paragraph_saw_run = true;
                        // Commit this run only if it produced text OR has a format
                        // flag the AI should know about. Empty runs with no flags
                        // are skipped — they would just bloat the response.
                        let has_format = current_run_format.bold
                            || current_run_format.italic
                            || current_run_format.underline
                            || current_run_format.strikethrough
                            || current_run_format.font_size.is_some()
                            || current_run_format.color.is_some()
                            || current_run_format.font_name.is_some()
                            || current_run_format.highlight.is_some();
                        if !current_run_text.is_empty() || has_format {
                            current_text.push_str(&current_run_text);
                            current_runs.push(FontRun {
                                text: std::mem::take(&mut current_run_text),
                                bold: current_run_format.bold,
                                italic: current_run_format.italic,
                                underline: current_run_format.underline,
                                strikethrough: current_run_format.strikethrough,
                                font_size: current_run_format.font_size,
                                color: current_run_format.color.clone(),
                                font_name: current_run_format.font_name.clone(),
                                highlight: current_run_format.highlight.clone(),
                            });
                        }
                    }
                } else if name.as_ref() == b"p" {
                    para_depth = para_depth.saturating_sub(1);
                    if para_depth == 0 && tbl_cell_depth == 0 {
                        // Always preserve the paragraph's slot in the document.
                        // We only skip it if it had zero text AND zero runs AND no
                        // style — i.e. it was a totally empty paragraph that carries
                        // no information at all. Such paragraphs are usually
                        // artefacts of trailing whitespace and dropping them is safe.
                        let has_format = current_runs.iter().any(|r| {
                            r.bold || r.italic || r.underline || r.strikethrough
                                || r.font_size.is_some() || r.color.is_some() || r.font_name.is_some()
                                || r.highlight.is_some()
                        });
                        // Keep if: has content, or style, or formatting, or is a table marker
                        let keep = !current_text.is_empty()
                            || current_style.is_some()
                            || current_numbering.is_some()
                            || has_format
                            || paragraph_saw_run
                            || is_table_marker;
                        if keep {
                            // Use stable ID if available, otherwise generate sequential ID
                            // For table markers, use the special marker text format
                            let id = if let Some(stable_id) = current_stable_id.clone() {
                                stable_id
                            } else {
                                let id = format!("p{}", para_counter);
                                para_counter += 1;
                                id
                            };
                            let runs_opt = if current_runs.is_empty() { None } else { Some(current_runs.clone()) };
                            // For table markers, generate the marker text format
                            let text = if is_table_marker {
                                // Extract table ID from marker format __tbl_pos_<table_id>__
                                if let Some(stable_id) = &current_stable_id {
                                    if let Some(rest) = stable_id.strip_prefix("__tbl_pos_") {
                                        if let Some(table_id) = rest.strip_suffix("__") {
                                            format!("<__tbl_pos_{}__>", table_id)
                                        } else {
                                            stable_id.clone()
                                        }
                                    } else {
                                        stable_id.clone()
                                    }
                                } else {
                                    current_text.clone()
                                }
                            } else {
                                current_text.trim().to_string()
                            };
                            paragraphs.push(WordParagraph {
                                id,
                                text,
                                style: current_style.clone(),
                                runs: runs_opt,
                                numbering: current_numbering.clone(),
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

fn parse_table_xml(content: &str) -> Result<Vec<WordTable>, OfficeError> {
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
        out.push(WordTable { id: raw.id, rows });
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
        let mut has_styles = false;
        let mut has_settings = false;
        let mut has_font_table = false;
        let mut has_theme = false;
        let mut has_numbering = false;
        for i in 0..archive.len() {
            let file = archive.by_index(i)?;
            let name = file.name().to_string();
            match name.as_str() {
                "word/styles.xml" => has_styles = true,
                "word/settings.xml" => has_settings = true,
                "word/fontTable.xml" => has_font_table = true,
                "word/theme/theme1.xml" => has_theme = true,
                "word/numbering.xml" => has_numbering = true,
                _ => {}
            }
        }
        drop(archive);

        // Re-open to copy entries (we needed to scan first for the missing-files case)
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
        drop(archive);

        // If the original docx was missing one of styles/settings/fontTable/theme/numbering,
        // backfill with our hardcoded ones so the new docx stays valid.
        if !has_styles {
            zip.start_file("word/styles.xml", opts)?;
            zip.write_all(STYLES_XML.as_bytes())?;
        }
        if !has_settings {
            zip.start_file("word/settings.xml", opts)?;
            zip.write_all(SETTINGS_XML.as_bytes())?;
        }
        if !has_font_table {
            zip.start_file("word/fontTable.xml", opts)?;
            zip.write_all(FONT_TABLE_XML.as_bytes())?;
        }
        if !has_theme {
            zip.start_file("word/theme/theme1.xml", opts)?;
            zip.write_all(THEME_XML.as_bytes())?;
        }
        // Numbering: only backfill when the doc actually references lists. Without
        // this, references to `numId` would resolve to nothing. The minimum we
        // provide is one bullet list and one decimal list (numId 1 and 2) so
        // that AI-generated `numbering: { num_id: 1, level: 0 }` works out of
        // the box on freshly-created docs.
        if !has_numbering && doc_has_numbering(doc) {
            zip.start_file("word/numbering.xml", opts)?;
            zip.write_all(NUMBERING_XML.as_bytes())?;
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

        // Only emit numbering.xml if the document references any list items.
        if doc_has_numbering(doc) {
            zip.start_file("word/numbering.xml", opts)?;
            zip.write_all(NUMBERING_XML.as_bytes())?;
        }
    }

    zip.finish()?;
    Ok(())
}

/// True when the document contains at least one paragraph with a numbering
/// reference. Used to decide whether `word/numbering.xml` should be emitted.
fn doc_has_numbering(doc: &WordDocument) -> bool {
    doc.paragraphs.iter().any(|p| p.numbering.is_some())
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
    if run.strikethrough { rpr.push_str("<w:strike/>"); }
    if let Some(ref highlight) = run.highlight {
        if !highlight.is_empty() {
            rpr.push_str(&format!("<w:highlight w:val=\"{}\"/>", escape_xml(highlight)));
        }
    }
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
            xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"
            xmlns:inkuo="http://inkuo.app/wordprocessingml/2026/main">
  <w:body>"#
    );

    // Build a map of table id -> table for O(1) lookup.
    let table_map: std::collections::HashMap<&str, &WordTable> =
        doc.tables.iter().map(|t| (t.id.as_str(), t)).collect();

    // Track which tables have been emitted via markers to avoid double-emission
    let mut tables_emitted: std::collections::HashSet<&str> = std::collections::HashSet::new();

    // Iterate over paragraphs directly - markers contain position info
    for para in &doc.paragraphs {
        // Check if this is a table position marker
        if let Some(rest) = para.text.strip_prefix("<__tbl_pos_") {
            if let Some(end) = rest.find("__>") {
                let tbl_id = &rest[..end];
                if let Some(tbl) = table_map.get(tbl_id) {
                    // Output a marker paragraph with empty content but marker ID pattern
                    // The parser will detect this via the inkuo:id pattern
                    xml.push_str("\n    <w:p>");
                    xml.push_str(&format!("<w:pPr><inkuo:id w:val=\"__tbl_pos_{}__\"/></w:pPr>", escape_xml(tbl_id)));
                    xml.push_str("</w:p>");
                    // Also output the table immediately after
                    xml.push_str(&build_table_xml(&tbl.id, &tbl.rows));
                    tables_emitted.insert(tbl_id);
                    continue;
                }
            }
        }

        // Regular paragraph - output as normal
        xml.push_str("\n    <w:p>");
        // Build paragraph properties: style (if any) + numbering (if any) + stable ID
        let has_ppr = para.style.is_some() || para.numbering.is_some() || !para.id.is_empty();
        if has_ppr {
            xml.push_str("<w:pPr>");
            if let Some(ref s) = para.style {
                xml.push_str(&format!("<w:pStyle w:val=\"{}\"/>", escape_xml(s)));
            }
            if let Some(ref num) = para.numbering {
                xml.push_str("<w:numPr>");
                xml.push_str(&format!("<w:ilvl w:val=\"{}\"/>", num.level));
                xml.push_str(&format!("<w:numId w:val=\"{}\"/>", num.num_id));
                xml.push_str("</w:numPr>");
            }
            // Store stable ID as custom property to preserve across read/write cycles
            xml.push_str(&format!("<inkuo:id w:val=\"{}\"/>", escape_xml(&para.id)));
            xml.push_str("</w:pPr>");
        }

        // Output paragraph content
        if let Some(ref run_list) = para.runs {
            for run in run_list {
                xml.push_str(&build_run_xml(run));
            }
        } else if !para.text.is_empty() {
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

    // Output any tables that weren't emitted via markers (orphaned tables)
    for tbl in &doc.tables {
        if !tables_emitted.contains(tbl.id.as_str()) {
            xml.push_str(&build_table_xml(&tbl.id, &tbl.rows));
        }
    }

    xml.push_str("\n  </w:body>\n</w:document>");
    xml
}

fn build_table_xml(_table_id: &str, rows: &[TableRow]) -> String {
    let mut xml = String::new();
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

    let render_row = |xml: &mut String, cells: &[TableCell]| {
        xml.push_str("\n        <w:tr>");
        for cell in cells {
            let col_span = cell.col_span.max(1);
            let row_span = cell.row_span.max(1);
            xml.push_str("<w:tc><w:tcPr>");
            if col_span > 1 {
                xml.push_str(&format!("<w:gridSpan w:val=\"{}\"/>", col_span));
            }
            if row_span > 1 {
                xml.push_str("<w:vMerge w:val=\"restart\"/>");
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
    };

    // Render all rows (first row is header, rest are body)
    for row in rows {
        if !row.cells.is_empty() {
            render_row(&mut xml, &row.cells);
        }
    }

    xml.push_str("\n    </w:tbl>");
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
  <Default Extension="png" ContentType="image/png"/>
  <Default Extension="jpeg" ContentType="image/jpeg"/>
  <Default Extension="jpg" ContentType="image/jpeg"/>
  <Default Extension="gif" ContentType="image/gif"/>
  <Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>
  <Override PartName="/word/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml"/>
  <Override PartName="/word/settings.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.settings+xml"/>
  <Override PartName="/word/fontTable.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.fontTable+xml"/>
  <Override PartName="/word/theme/theme1.xml" ContentType="application/vnd.openxmlformats-officedocument.theme+xml"/>
  <Override PartName="/word/numbering.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.numbering+xml"/>
  <Override PartName="/word/header1.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml"/>
  <Override PartName="/word/footer1.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.footer+xml"/>
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
  <Relationship Id="rId5" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/numbering" Target="numbering.xml"/>
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

/// Minimal numbering definitions: one bullet list (numId 1) and one decimal
/// list (numId 2), each with up to 3 indent levels. AI-created documents that
/// reference `numId: 1` or `numId: 2` will get proper bullet/decimal markers
/// when this file is emitted.
pub const NUMBERING_XML: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:numbering xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:abstractNum w:abstractNumId="0">
    <w:lvl w:ilvl="0">
      <w:start w:val="1"/>
      <w:numFmt w:val="bullet"/>
      <w:lvlText w:val="•"/>
      <w:lvlJc w:val="left"/>
      <w:pPr><w:ind w:left="720" w:hanging="360"/></w:pPr>
    </w:lvl>
    <w:lvl w:ilvl="1">
      <w:start w:val="1"/>
      <w:numFmt w:val="bullet"/>
      <w:lvlText w:val="◦"/>
      <w:lvlJc w:val="left"/>
      <w:pPr><w:ind w:left="1440" w:hanging="360"/></w:pPr>
    </w:lvl>
    <w:lvl w:ilvl="2">
      <w:start w:val="1"/>
      <w:numFmt w:val="bullet"/>
      <w:lvlText w:val="▪"/>
      <w:lvlJc w:val="left"/>
      <w:pPr><w:ind w:left="2160" w:hanging="360"/></w:pPr>
    </w:lvl>
  </w:abstractNum>
  <w:abstractNum w:abstractNumId="1">
    <w:lvl w:ilvl="0">
      <w:start w:val="1"/>
      <w:numFmt w:val="decimal"/>
      <w:lvlText w:val="%1."/>
      <w:lvlJc w:val="left"/>
      <w:pPr><w:ind w:left="720" w:hanging="360"/></w:pPr>
    </w:lvl>
    <w:lvl w:ilvl="1">
      <w:start w:val="1"/>
      <w:numFmt w:val="lowerLetter"/>
      <w:lvlText w:val="%2)"/>
      <w:lvlJc w:val="left"/>
      <w:pPr><w:ind w:left="1440" w:hanging="360"/></w:pPr>
    </w:lvl>
    <w:lvl w:ilvl="2">
      <w:start w:val="1"/>
      <w:numFmt w:val="lowerRoman"/>
      <w:lvlText w:val="%3."/>
      <w:lvlJc w:val="left"/>
      <w:pPr><w:ind w:left="2160" w:hanging="360"/></w:pPr>
    </w:lvl>
  </w:abstractNum>
  <w:num w:numId="1"><w:abstractNumId w:val="0"/></w:num>
  <w:num w:numId="2"><w:abstractNumId w:val="1"/></w:num>
</w:numbering>"#;

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// A paragraph with both inline formatting and a numbering reference
    /// should round-trip: parse the XML, then re-emit it with the same fields
    /// intact.
    #[test]
    fn roundtrip_strikethrough_highlight_numbering() {
        let src = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:body>
    <w:p>
      <w:pPr>
        <w:pStyle w:val="Heading1"/>
        <w:numPr><w:ilvl w:val="0"/><w:numId w:val="2"/></w:numPr>
      </w:pPr>
      <w:r>
        <w:rPr><w:strike/><w:highlight w:val="yellow"/></w:rPr>
        <w:t xml:space="preserve">已删除</w:t>
      </w:r>
      <w:r>
        <w:rPr><w:b/><w:color w:val="FF0000"/></w:rPr>
        <w:t xml:space="preserve">重点</w:t>
      </w:r>
    </w:p>
  </w:body>
</w:document>"#;
        let paragraphs = parse_document_xml(src).expect("parse should succeed");
        assert_eq!(paragraphs.len(), 1);
        let p = &paragraphs[0];
        assert_eq!(p.style.as_deref(), Some("Heading1"));
        let num = p.numbering.as_ref().expect("numbering should be parsed");
        assert_eq!(num.num_id, 2);
        assert_eq!(num.level, 0);
        let runs = p.runs.as_ref().expect("runs should be present");
        assert_eq!(runs.len(), 2);
        assert!(runs[0].strikethrough);
        assert_eq!(runs[0].highlight.as_deref(), Some("yellow"));
        assert!(runs[1].bold);
        assert_eq!(runs[1].color.as_deref(), Some("FF0000"));
    }

    /// Re-emitting the document must include numPr in the pPr block — otherwise
    /// Word will silently drop the list membership.
    #[test]
    fn build_xml_includes_numpr() {
        let doc = WordDocument {
            paragraphs: vec![WordParagraph {
                id: "p0".to_string(),
                text: "Item one".to_string(),
                style: None,
                runs: None,
                numbering: Some(NumberingRef { num_id: 1, level: 0 }),
            }],
            tables: vec![],
        };
        let xml = build_document_xml(&doc);
        assert!(xml.contains("<w:numPr>"), "xml was: {}", xml);
        assert!(xml.contains("<w:numId w:val=\"1\"/>"));
        assert!(xml.contains("<w:ilvl w:val=\"0\"/>"));
    }

    /// Re-emitting a strike/highlight run must include the corresponding rPr
    /// children. This guards against accidentally dropping the new fields.
    #[test]
    fn build_xml_includes_strike_highlight() {
        let run = FontRun {
            text: "x".to_string(),
            strikethrough: true,
            highlight: Some("red".to_string()),
            ..Default::default()
        };
        let xml = build_run_xml(&run);
        assert!(xml.contains("<w:strike/>"), "xml was: {}", xml);
        assert!(xml.contains("<w:highlight w:val=\"red\"/>"), "xml was: {}", xml);
    }

    /// doc_has_numbering must reflect whether the document references lists —
    /// it controls whether `word/numbering.xml` is written.
    #[test]
    fn doc_has_numbering_detection() {
        let doc_no = WordDocument {
            paragraphs: vec![WordParagraph {
                id: "p0".to_string(),
                text: "x".to_string(),
                style: None,
                runs: None,
                numbering: None,
            }],
            tables: vec![],
        };
        assert!(!doc_has_numbering(&doc_no));

        let doc_yes = WordDocument {
            paragraphs: vec![WordParagraph {
                id: "p0".to_string(),
                text: "x".to_string(),
                style: None,
                runs: None,
                numbering: Some(NumberingRef { num_id: 1, level: 0 }),
            }],
            tables: vec![],
        };
        assert!(doc_has_numbering(&doc_yes));
    }

    /// End-to-end: build a doc with bullet list, write it, then read it back.
    /// This guards against the most common regression — emitting
    /// `<w:numPr>` but forgetting to emit `<w:numbering.xml>`, or vice versa.
    #[test]
    fn write_then_read_list_item() {
        let doc = WordDocument {
            paragraphs: vec![
                WordParagraph {
                    id: "p0".to_string(),
                    text: "Title".to_string(),
                    style: Some("Title".to_string()),
                    runs: None,
                    numbering: None,
                },
                WordParagraph {
                    id: "p1".to_string(),
                    text: "first".to_string(),
                    style: None,
                    runs: None,
                    numbering: Some(NumberingRef { num_id: 1, level: 0 }),
                },
            ],
            tables: vec![],
        };
        let mut buf = std::io::Cursor::new(Vec::<u8>::new());
        write_word_document(&doc, &mut buf, None).expect("write should succeed");
        let bytes = buf.into_inner();

        // zip the output and confirm numbering.xml is present
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes.as_slice())).expect("output must be a valid zip");
        let names: Vec<String> = (0..archive.len())
            .map(|i| archive.by_index(i).unwrap().name().to_string())
            .collect();
        assert!(names.contains(&"word/numbering.xml".to_string()), "entries: {:?}", names);
        assert!(names.contains(&"word/document.xml".to_string()));

        // Read it back
        let parsed = read_word_document(&bytes).expect("round-trip read should succeed");
        assert_eq!(parsed.paragraphs.len(), 2);
        assert!(parsed.paragraphs[1].numbering.is_some(), "list membership must survive the round trip");
    }

    // ─── Regression: tables must appear between paragraphs, not at the end ───

    fn extract_document_xml(bytes: &[u8]) -> String {
        let mut archive =
            zip::ZipArchive::new(std::io::Cursor::new(bytes)).expect("output must be a valid zip");
        let mut entry = archive.by_name("word/document.xml").expect("document.xml entry");
        let mut xml = String::new();
        std::io::Read::read_to_string(&mut entry, &mut xml).expect("read xml");
        xml
    }

    #[test]
    fn build_document_xml_keeps_tables_between_paragraphs() {
        // P1 -> T1 -> P2 -> T2 -> P3
        let elements = vec![
            DocElement::Paragraph {
                id: "p1".into(),
                text: "First paragraph".into(),
                omit_text: false,
                style: None,
                runs: None,
                numbering: None,
            },
            DocElement::Table {
                id: "t1".into(),
                position: 0,
                header: vec![TableCell::plain("Col A"), TableCell::plain("Col B")],
                rows: vec![vec![TableCell::plain("a1"), TableCell::plain("b1")]],
            },
            DocElement::Paragraph {
                id: "p2".into(),
                text: "Middle paragraph".into(),
                omit_text: false,
                style: None,
                runs: None,
                numbering: None,
            },
            DocElement::Table {
                id: "t2".into(),
                position: 0,
                header: vec![TableCell::plain("X"), TableCell::plain("Y")],
                rows: vec![vec![TableCell::plain("1"), TableCell::plain("2")]],
            },
            DocElement::Paragraph {
                id: "p3".into(),
                text: "Last paragraph".into(),
                omit_text: false,
                style: None,
                runs: None,
                numbering: None,
            },
        ];

        let doc = WordDocument::from_elements(elements);
        let xml = build_document_xml(&doc);

        let p1 = xml.find("First paragraph").expect("p1 text");
        let t1 = xml.find("Col A").expect("t1 header");
        let p2 = xml.find("Middle paragraph").expect("p2 text");
        let t2 = xml.find(">X<").expect("t2 header cell");
        let p3 = xml.find("Last paragraph").expect("p3 text");

        assert!(p1 < t1, "T1 must appear AFTER P1. p1={} t1={}", p1, t1);
        assert!(t1 < p2, "P2 must appear AFTER T1. t1={} p2={}", t1, p2);
        assert!(p2 < t2, "T2 must appear AFTER P2. p2={} t2={}", p2, t2);
        assert!(t2 < p3, "P3 must appear AFTER T2. t2={} p3={}", t2, p3);
    }

    #[test]
    fn build_document_xml_handles_position_marker_paragraphs() {
        // The doc loaded from disk uses marker paragraphs like
        // `<__tbl_pos_t0__>` to record table positions. The marker must NOT
        // appear in the output, and the table must replace it in place.
        let doc = WordDocument {
            paragraphs: vec![
                WordParagraph {
                    id: "p1".into(),
                    text: "Before table".into(),
                    style: None,
                    runs: None,
                    numbering: None,
                },
                WordParagraph {
                    id: "marker".into(),
                    text: "<__tbl_pos_t0__>".into(),
                    style: None,
                    runs: None,
                    numbering: None,
                },
                WordParagraph {
                    id: "p2".into(),
                    text: "After table".into(),
                    style: None,
                    runs: None,
                    numbering: None,
                },
            ],
            tables: vec![WordTable {
                id: "t0".into(),
                rows: vec![TableRow {
                    cells: vec![TableCell {
                        text: "cell A".into(),
                        col_span: 1,
                        row_span: 1,
                    }],
                }],
            }],
        };

        let xml = build_document_xml(&doc);

        assert!(
            !xml.contains("<__tbl_pos_"),
            "Marker paragraph must be stripped from output, got: {}",
            xml
        );

        let before = xml.find("Before table").expect("before");
        let cell = xml.find("cell A").expect("cell");
        let after = xml.find("After table").expect("after");

        assert!(before < cell, "Table cell must come after 'Before table'");
        assert!(cell < after, "Table cell must come before 'After table'");
    }

    // Note: A separate path exists for documents loaded from disk and
    // re-saved. The reader (`parse_document_xml` + `parse_table_xml`) does
    // not currently insert position markers when it encounters a `<w:tbl>`,
    // so a doc parsed and re-serialized by this codebase would still place
    // tables at the end. That is a pre-existing limitation of the parser
    // and is not the path used when an AI agent creates a new Word doc via
    // `from_elements` (which IS the user-reported regression and IS fixed).

    #[test]
    fn parse_table_xml_extracts_grid_span() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:body>
    <w:tbl>
      <w:tr>
        <w:tc>
          <w:tcPr><w:gridSpan w:val="3"/></w:tcPr>
          <w:p><w:r><w:t>A B C</w:t></w:r></w:p>
        </w:tc>
      </w:tr>
    </w:tbl>
  </w:body>
</w:document>"#;
        let tables = parse_table_xml(xml).expect("parse");
        assert_eq!(tables.len(), 1);
        assert_eq!(tables[0].rows.len(), 1);
        assert_eq!(tables[0].rows[0].cells.len(), 1);
        assert_eq!(tables[0].rows[0].cells[0].col_span, 3);
    }

    #[test]
    fn parse_table_xml_resolves_vmerge_row_span() {
        // Two columns:
        //   col 0: "Header" (not merged)
        //   col 1: 3-row vertical merge ("Span")
        // Plus a non-merged cell at the end to make the structure realistic.
        let xml = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:body>
    <w:tbl>
      <w:tr>
        <w:tc><w:p><w:r><w:t>Header</w:t></w:r></w:p></w:tc>
        <w:tc>
          <w:tcPr><w:vMerge w:val="restart"/></w:tcPr>
          <w:p><w:r><w:t>Span</w:t></w:r></w:p>
        </w:tc>
      </w:tr>
      <w:tr>
        <w:tc><w:p><w:r><w:t>Row 2</w:t></w:r></w:p></w:tc>
        <w:tc>
          <w:tcPr><w:vMerge/></w:tcPr>
          <w:p/>
        </w:tc>
      </w:tr>
      <w:tr>
        <w:tc><w:p><w:r><w:t>Row 3</w:t></w:r></w:p></w:tc>
        <w:tc>
          <w:tcPr><w:vMerge w:val="continue"/></w:tcPr>
          <w:p/>
        </w:tc>
      </w:tr>
    </w:tbl>
  </w:body>
</w:document>"#;
        let tables = parse_table_xml(xml).expect("parse");
        assert_eq!(tables.len(), 1);
        assert_eq!(tables[0].rows.len(), 3);

        // Column 0 ("Header") never merges.
        for (row_idx, expected) in ["Header", "Row 2", "Row 3"].iter().enumerate() {
            assert_eq!(tables[0].rows[row_idx].cells[0].text, *expected);
            assert_eq!(tables[0].rows[row_idx].cells[0].row_span, 1);
        }

        // Column 1: first cell is "Span" with row_span=3, the other two are
        // continue-cells with row_span=1.
        assert_eq!(tables[0].rows[0].cells[1].text, "Span");
        assert_eq!(tables[0].rows[0].cells[1].row_span, 3);
        assert_eq!(tables[0].rows[1].cells[1].text, "");
        assert_eq!(tables[0].rows[1].cells[1].row_span, 1);
        assert_eq!(tables[0].rows[2].cells[1].text, "");
        assert_eq!(tables[0].rows[2].cells[1].row_span, 1);
    }

    #[test]
    fn table_round_trip_preserves_grid_span_and_vmerge() {
        // Parse the vMerge XML, push the table through to_elements / from_elements,
        // then build the document XML and verify the write path emits
        // <w:gridSpan> and <w:vMerge> for the merged cells.
        let xml = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:body>
    <w:tbl>
      <w:tr>
        <w:tc><w:p><w:r><w:t>A</w:t></w:r></w:p></w:tc>
        <w:tc>
          <w:tcPr><w:gridSpan w:val="2"/></w:tcPr>
          <w:p><w:r><w:t>BC</w:t></w:r></w:p>
        </w:tc>
      </w:tr>
      <w:tr>
        <w:tc>
          <w:tcPr><w:vMerge w:val="restart"/></w:tcPr>
          <w:p><w:r><w:t>VM</w:t></w:r></w:p>
        </w:tc>
        <w:tc><w:p><w:r><w:t>X</w:t></w:r></w:p></w:tc>
        <w:tc><w:p><w:r><w:t>Y</w:t></w:r></w:p></w:tc>
      </w:tr>
      <w:tr>
        <w:tc>
          <w:tcPr><w:vMerge/></w:tcPr>
          <w:p/>
        </w:tc>
        <w:tc><w:p><w:r><w:t>X2</w:t></w:r></w:p></w:tc>
        <w:tc><w:p><w:r><w:t>Y2</w:t></w:r></w:p></w:tc>
      </w:tr>
    </w:tbl>
  </w:body>
</w:document>"#;
        let tables = parse_table_xml(xml).expect("parse");
        let mut paragraphs = Vec::new();
        let mut tbls = Vec::new();
        for t in tables {
            tbls.push(t);
        }
        let doc = WordDocument { paragraphs, tables: tbls };
        let elements = doc.to_elements();

        // The re-built document must round-trip without losing span info.
        let rebuilt = WordDocument::from_elements(elements);
        assert_eq!(rebuilt.tables.len(), 1);
        assert_eq!(rebuilt.tables[0].rows.len(), 3);

        // First row: BC has col_span=2.
        assert_eq!(rebuilt.tables[0].rows[0].cells[1].text, "BC");
        assert_eq!(rebuilt.tables[0].rows[0].cells[1].col_span, 2);

        // Vertical merge: first cell of column 0 starts a 2-row merge.
        assert_eq!(rebuilt.tables[0].rows[1].cells[0].text, "VM");
        assert_eq!(rebuilt.tables[0].rows[1].cells[0].row_span, 2);

        // Render to XML and assert the merge attributes appear.
        let rebuilt_doc = WordDocument { paragraphs: vec![], tables: vec![rebuilt.tables.into_iter().next().unwrap()] };
        let xml = build_document_xml(&rebuilt_doc);
        assert!(
            xml.contains("<w:gridSpan w:val=\"2\"/>"),
            "expected gridSpan to survive round-trip; got:\n{}",
            xml
        );
        assert!(
            xml.contains("<w:vMerge w:val=\"restart\"/>"),
            "expected vMerge restart to survive round-trip; got:\n{}",
            xml
        );
    }

    /// Tables should appear between paragraphs in the correct order.
    /// This tests that from_elements + to_elements round-trips correctly.
    #[test]
    fn test_table_order_preserved_in_round_trip() {
        let elements = vec![
            DocElement::Paragraph {
                id: "p1".to_string(),
                text: "段落A".to_string(),
                omit_text: false,
                style: None,
                runs: None,
                numbering: None,
            },
            DocElement::Table {
                id: "t0".to_string(),
                position: 0,
                header: vec![TableCell::plain("列1".to_string())],
                rows: vec![vec![TableCell::plain("数据".to_string())]],
            },
            DocElement::Paragraph {
                id: "p2".to_string(),
                text: "段落B".to_string(),
                omit_text: false,
                style: None,
                runs: None,
                numbering: None,
            },
        ];

        let doc = WordDocument::from_elements(elements);
        let round_trip = doc.to_elements();

        // Verify table is at position 1 (between p1 and p2)
        assert_eq!(round_trip.len(), 3);
        match &round_trip[0] {
            DocElement::Paragraph { text, .. } => assert_eq!(text, "段落A"),
            _ => panic!("First should be paragraph A"),
        }
        match &round_trip[1] {
            DocElement::Table { id, .. } => assert_eq!(id, "t0"),
            _ => panic!("Second should be table"),
        }
        match &round_trip[2] {
            DocElement::Paragraph { text, .. } => assert_eq!(text, "段落B"),
            _ => panic!("Third should be paragraph B"),
        }
    }

    /// Multiple tables should maintain their relative positions.
    #[test]
    fn test_multiple_tables_order_preserved() {
        let elements = vec![
            DocElement::Paragraph {
                id: "p1".to_string(),
                text: "段落1".to_string(),
                omit_text: false,
                style: None,
                runs: None,
                numbering: None,
            },
            DocElement::Table {
                id: "t0".to_string(),
                position: 0,
                header: vec![TableCell::plain("表1".to_string())],
                rows: vec![],
            },
            DocElement::Paragraph {
                id: "p2".to_string(),
                text: "段落2".to_string(),
                omit_text: false,
                style: None,
                runs: None,
                numbering: None,
            },
            DocElement::Table {
                id: "t1".to_string(),
                position: 0,
                header: vec![TableCell::plain("表2".to_string())],
                rows: vec![],
            },
            DocElement::Paragraph {
                id: "p3".to_string(),
                text: "段落3".to_string(),
                omit_text: false,
                style: None,
                runs: None,
                numbering: None,
            },
        ];

        let doc = WordDocument::from_elements(elements);
        let round_trip = doc.to_elements();

        // Verify order: p1, t0, p2, t1, p3
        assert_eq!(round_trip.len(), 5);
        assert!(matches!(&round_trip[0], DocElement::Paragraph { text, .. } if text == "段落1"));
        assert!(matches!(&round_trip[1], DocElement::Table { id, .. } if id == "t0"));
        assert!(matches!(&round_trip[2], DocElement::Paragraph { text, .. } if text == "段落2"));
        assert!(matches!(&round_trip[3], DocElement::Table { id, .. } if id == "t1"));
        assert!(matches!(&round_trip[4], DocElement::Paragraph { text, .. } if text == "段落3"));
    }

    /// Table order must be preserved in the generated XML.
    #[test]
    fn test_table_order_in_xml_output() {
        let elements = vec![
            DocElement::Paragraph {
                id: "p1".to_string(),
                text: "段落A".to_string(),
                omit_text: false,
                style: None,
                runs: None,
                numbering: None,
            },
            DocElement::Table {
                id: "t0".to_string(),
                position: 0,
                header: vec![TableCell::plain("表头".to_string())],
                rows: vec![vec![TableCell::plain("数据".to_string())]],
            },
            DocElement::Paragraph {
                id: "p2".to_string(),
                text: "段落B".to_string(),
                omit_text: false,
                style: None,
                runs: None,
                numbering: None,
            },
        ];

        let doc = WordDocument::from_elements(elements);
        let xml = build_document_xml(&doc);

        // Table should appear between the two paragraphs in XML
        let para_a_pos = xml.find("段落A").expect("should have paragraph A");
        let para_b_pos = xml.find("段落B").expect("should have paragraph B");
        let table_pos = xml.find("<w:tbl>").expect("should have table");

        assert!(
            para_a_pos < table_pos && table_pos < para_b_pos,
            "Table should be between paragraph A and B\npara_a={}, table={}, para_b={}",
            para_a_pos, table_pos, para_b_pos
        );
    }

    /// Full round-trip: create -> write -> read -> verify order.
    #[test]
    fn test_full_roundtrip_with_table() {
        let elements = vec![
            DocElement::Paragraph {
                id: "p1".to_string(),
                text: "段落A".to_string(),
                omit_text: false,
                style: None,
                runs: None,
                numbering: None,
            },
            DocElement::Table {
                id: "t0".to_string(),
                position: 0,
                header: vec![TableCell::plain("表头列".to_string())],
                rows: vec![vec![TableCell::plain("数据".to_string())]],
            },
            DocElement::Paragraph {
                id: "p2".to_string(),
                text: "段落B".to_string(),
                omit_text: false,
                style: None,
                runs: None,
                numbering: None,
            },
        ];

        // Create document
        let doc = WordDocument::from_elements(elements);
        
        // Debug: check document structure
        println!("After from_elements:");
        println!("  paragraphs count: {}", doc.paragraphs.len());
        for (i, p) in doc.paragraphs.iter().enumerate() {
            println!("    [{}] id={}, text={:?}", i, p.id, p.text);
        }
        println!("  tables count: {}", doc.tables.len());
        
        // Build raw XML to check
        let xml = build_document_xml(&doc);
        let marker_pos = xml.find("__tbl_pos_");
        println!("\nMarker in XML: {}", if marker_pos.is_some() { "FOUND" } else { "NOT FOUND" });
        if let Some(pos) = marker_pos {
            let snippet = &xml[pos.saturating_sub(50)..(pos + 80).min(xml.len())];
            println!("Marker context: {:?}", snippet);
        } else {
            // Print first 500 chars of XML body section
            if let Some(body_start) = xml.find("<w:body>") {
                let body_section = &xml[body_start..(body_start + 500).min(xml.len())];
                println!("XML body section (first 500 chars): {}", body_section);
            }
        }
        
        // Write to bytes
        let mut buf = std::io::Cursor::new(Vec::new());
        write_word_document(&doc, &mut buf, None).expect("write should succeed");
        
        // Read back
        let read_doc = read_word_document(&buf.into_inner()).expect("read should succeed");
        let read_elements = read_doc.to_elements();
        
        // Verify order: paragraph A, table, paragraph B
        assert_eq!(read_elements.len(), 3, "Should have 3 elements");
        
        assert!(matches!(&read_elements[0], DocElement::Paragraph { text, .. } if text == "段落A"),
            "First element should be paragraph A");
        assert!(matches!(&read_elements[1], DocElement::Table { .. }),
            "Second element should be table");
        assert!(matches!(&read_elements[2], DocElement::Paragraph { text, .. } if text == "段落B"),
            "Third element should be paragraph B");
    }

    /// Test multiple tables in sequence
    #[test]
    fn test_multiple_tables_inline_positions() {
        let elements = vec![
            DocElement::Paragraph { id: "p1".to_string(), text: "P1".to_string(), omit_text: false, style: None, runs: None, numbering: None },
            DocElement::Table { id: "t0".to_string(), position: 0, header: vec![TableCell::plain("T1".to_string())], rows: vec![] },
            DocElement::Paragraph { id: "p2".to_string(), text: "P2".to_string(), omit_text: false, style: None, runs: None, numbering: None },
            DocElement::Table { id: "t1".to_string(), position: 0, header: vec![TableCell::plain("T2".to_string())], rows: vec![] },
            DocElement::Paragraph { id: "p3".to_string(), text: "P3".to_string(), omit_text: false, style: None, runs: None, numbering: None },
        ];
        
        let doc = WordDocument::from_elements(elements);
        
        // Write and read
        let mut buf = std::io::Cursor::new(Vec::new());
        write_word_document(&doc, &mut buf, None).expect("write should succeed");
        let read_doc = read_word_document(&buf.into_inner()).expect("read should succeed");
        let elements = read_doc.to_elements();
        
        // Verify order: P1, T1, P2, T2, P3
        assert_eq!(elements.len(), 5);
        assert!(matches!(&elements[0], DocElement::Paragraph { text, .. } if text == "P1"));
        assert!(matches!(&elements[1], DocElement::Table { id, .. } if id == "t0"));
        assert!(matches!(&elements[2], DocElement::Paragraph { text, .. } if text == "P2"));
        assert!(matches!(&elements[3], DocElement::Table { id, .. } if id == "t1"));
        assert!(matches!(&elements[4], DocElement::Paragraph { text, .. } if text == "P3"));
    }

    /// Test that table at end is also handled correctly
    #[test]
    fn test_table_at_end() {
        let elements = vec![
            DocElement::Paragraph { id: "p1".to_string(), text: "P1".to_string(), omit_text: false, style: None, runs: None, numbering: None },
            DocElement::Paragraph { id: "p2".to_string(), text: "P2".to_string(), omit_text: false, style: None, runs: None, numbering: None },
            DocElement::Table { id: "t0".to_string(), position: 0, header: vec![TableCell::plain("Table".to_string())], rows: vec![] },
        ];
        
        let doc = WordDocument::from_elements(elements);
        let mut buf = std::io::Cursor::new(Vec::new());
        write_word_document(&doc, &mut buf, None).expect("write should succeed");
        let read_doc = read_word_document(&buf.into_inner()).expect("read should succeed");
        let elements = read_doc.to_elements();
        
        assert_eq!(elements.len(), 3);
        assert!(matches!(&elements[0], DocElement::Paragraph { text, .. } if text == "P1"));
        assert!(matches!(&elements[1], DocElement::Paragraph { text, .. } if text == "P2"));
        assert!(matches!(&elements[2], DocElement::Table { .. }));
    }

    /// Test that table at beginning is also handled correctly
    #[test]
    fn test_table_at_beginning() {
        let elements = vec![
            DocElement::Table { id: "t0".to_string(), position: 0, header: vec![TableCell::plain("Table".to_string())], rows: vec![] },
            DocElement::Paragraph { id: "p1".to_string(), text: "P1".to_string(), omit_text: false, style: None, runs: None, numbering: None },
            DocElement::Paragraph { id: "p2".to_string(), text: "P2".to_string(), omit_text: false, style: None, runs: None, numbering: None },
        ];
        
        let doc = WordDocument::from_elements(elements);
        let mut buf = std::io::Cursor::new(Vec::new());
        write_word_document(&doc, &mut buf, None).expect("write should succeed");
        let read_doc = read_word_document(&buf.into_inner()).expect("read should succeed");
        let elements = read_doc.to_elements();
        
        assert_eq!(elements.len(), 3);
        assert!(matches!(&elements[0], DocElement::Table { .. }));
        assert!(matches!(&elements[1], DocElement::Paragraph { text, .. } if text == "P1"));
        assert!(matches!(&elements[2], DocElement::Paragraph { text, .. } if text == "P2"));
    }
}
