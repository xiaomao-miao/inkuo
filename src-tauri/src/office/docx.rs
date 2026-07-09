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

/// A document element — either a paragraph, a table, or an image.
/// Tables carry `position` (index in the flattened document order) so the
/// write path knows exactly where to insert each table. Images use the
/// same marker-paragraph trick as tables: a placeholder paragraph carrying
/// `<__img_pos_<id>__>` lets the write path splice the `<w:drawing>` run in
/// the right spot without losing stable ordering across round-trips.
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
    /// Inline picture. `path` points at the source image on disk at write
    /// time; the writer copies the bytes into `word/media/image{N}.{ext}`
    /// and rewrites `path` to that internal location. Width and height are
    /// in EMU (914400 EMU = 1 inch; 360000 EMU = 1 cm).
    #[serde(rename = "image")]
    Image {
        id: String,
        /// Zero-based position among all document elements.
        #[serde(default)]
        position: usize,
        /// Absolute path to the source image on disk (PNG / JPEG / GIF).
        /// May also be the rewritten internal `media/...` path after a
        /// round-trip through `read_word_document` — see `WordDocument`.
        path: String,
        width_emu: u32,
        height_emu: u32,
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
    /// to tables in sequential order. Images are skipped here (v1 writes
    /// inline `<w:drawing>` runs but the read path doesn't surface them in
    /// the element list — a future PR can add round-trip support once the
    /// parser can match image markers to their parent paragraphs).
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
            // Image marker paragraphs are emitted by the writer as inline
            // `<w:drawing>` runs; here in the read path we drop the marker
            // so it doesn't show up as a stray blank paragraph. v1: images
            // are write-only as far as round-tripping is concerned.
            if p.text.starts_with("<__img_pos_") {
                continue;
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
        let mut images: Vec<WordImage> = Vec::new();

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
                DocElement::Image { id, position: _, path, width_emu, height_emu } => {
                    // Mirror the table-marker trick: a placeholder paragraph
                    // carrying `<__img_pos_<id>__>` text lets `build_document_xml`
                    // splice in the `<w:drawing>` run at the right spot, while
                    // keeping the element's id stable across read/write cycles.
                    out_paras.push(WordParagraph {
                        id: format!("__img_pos_{}__", id),
                        text: format!("<__img_pos_{}__>", id),
                        style: None,
                        runs: None,
                        numbering: None,
                    });
                    // The `path` here is the *source* path on disk; the writer
                    // overwrites this with the rewritten `word/media/imageN.ext`
                    // path. We seed the field with the source so debug prints
                    // are still useful before the first write.
                    images.push(WordImage {
                        id,
                        path,
                        width_emu,
                        height_emu,
                        internal_path: None,
                    });
                }
            }
        }

        WordDocument { paragraphs: out_paras, tables, images }
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
        let mut images: Vec<WordImage> = Vec::new();

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
                DocElement::Image { id, position: _, path, width_emu, height_emu } => {
                    // Mirror the table-marker trick so the writer can splice
                    // the inline `<w:drawing>` run into the right paragraph.
                    out_paras.push(WordParagraph {
                        id: format!("__img_pos_{}__", id),
                        text: format!("<__img_pos_{}__>", id),
                        style: None,
                        runs: None,
                        numbering: None,
                    });
                    images.push(WordImage {
                        id,
                        path,
                        width_emu,
                        height_emu,
                        internal_path: None,
                    });
                }
            }
        }

        self.paragraphs = out_paras;
        self.tables = tables;
        self.images = images;
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
            DocElement::Image { ref id, .. } => id,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WordDocument {
    pub paragraphs: Vec<WordParagraph>,
    pub tables: Vec<WordTable>,
    /// Images embedded in the document. Each entry carries the (possibly
    /// rewritten) `word/media/...` path and the EMU dimensions captured at
    /// write time. The write path uses these to skip re-copying the bytes
    /// if the user re-saves without touching the image.
    #[serde(default)]
    pub images: Vec<WordImage>,
}

/// An image embedded in the document.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WordImage {
    /// Stable id (also embedded as the marker paragraph's stable id).
    pub id: String,
    /// Source image on disk (`png`/`jpeg`/`gif` bytes); the writer reads
    /// from here at first write and copies bytes into the package. After
    /// the document is re-read from a `.docx`, the writer remembers the
    /// already-inline copy via `internal_path` (see below) and reads from
    /// the preserved zip on subsequent writes instead.
    pub path: String,
    pub width_emu: u32,
    pub height_emu: u32,
    /// `Some("word/media/imageN.ext")` when this `WordImage` was
    /// recovered from an existing .docx (rather than built fresh from a
    /// `DocElement::Image`). Writer uses this to (a) skip re-reading the
    /// source bytes from disk and (b) reuse the original `rId` and
    /// `word/media/...` filename so the existing relationships in the
    /// preserved zip survive intact.
    ///
    /// `None` while the entry is still pending a first write — the
    /// model field default keeps backward compatibility with any callers
    /// that didn't set it.
    #[serde(default)]
    pub internal_path: Option<String>,
}

pub fn read_word_document(bytes: &[u8]) -> Result<WordDocument, OfficeError> {
    let doc_content = read_zip_entry(bytes, "word/document.xml")?;
    let rels_content = read_zip_entry(bytes, "word/_rels/document.xml.rels")
        .unwrap_or_default();
    let (mut paragraphs, image_markers) = parse_document_xml(&doc_content)?;
    let images = parse_image_xml(&doc_content, &rels_content, &image_markers);
    // `image_markers` are synthetic paragraphs each carrying the image's
    // stable id as their `<inkuo:id>` so the writer can pair them with
    // `WordImage` entries during `<w:drawing>` emission.
    paragraphs.extend(image_markers);
    let tables = parse_table_xml(&doc_content)?;
    Ok(WordDocument { paragraphs, tables, images })
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

fn parse_document_xml(content: &str) -> Result<(Vec<WordParagraph>, Vec<WordParagraph>), OfficeError> {
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
    let mut is_image_marker = false;  // Tracks if current paragraph is an image position marker

    // ── Image marker state ──────────────────────────────────────────────────
    // Image-bearing paragraphs are emitted as `<w:p><w:pPr><inkuo:id
    // w:val="__img_pos_<img_id>__"/></w:pPr><w:r>...drawing...</w:r></w:p>`
    // by the writer. On read we want to round-trip them back into a
    // `WordParagraph` (id = `__img_pos_<img_id>__`, text = same shape) so
    // the writer can re-emit the drawing next time. We also stash the
    // marker paragraphs separately so `parse_image_xml` can pull the
    // image id out without the marker being double-counted as a regular
    // paragraph.
    let mut image_markers: Vec<WordParagraph> = Vec::new();

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
                    is_image_marker = false; // Reset image-marker flag for new paragraph
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
                    if let Some(v) = attr_value_str(e, b"val") {
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
                    if let Some(v) = attr_value_str(e, b"val") {
                        if let Ok(s) = std::str::from_utf8(v.as_ref()) {
                            if let Ok(n) = s.parse::<u32>() {
                                pending_num_id = Some(n);
                            }
                        }
                    }
                } else if in_numpr && name.as_ref() == b"ilvl" {
                    if let Some(v) = attr_value_str(e, b"val") {
                        if let Ok(s) = std::str::from_utf8(v.as_ref()) {
                            if let Ok(n) = s.parse::<u32>() {
                                pending_ilvl = Some(n);
                            }
                        }
                    }
                } else if name.as_ref() == b"id" && para_depth > 0 && tbl_cell_depth == 0 {
                    // Read stable ID from custom inkuo:id element
                    // Also detect table markers (format: __tbl_pos_<table_id>__)
                    if let Some(v) = attr_value_str(e, b"val") {
                        if let Ok(s) = std::str::from_utf8(v.as_ref()) {
                            if !s.is_empty() {
                                if s.starts_with("__tbl_pos_") && s.ends_with("__") {
                                    // This is a table position marker
                                    current_stable_id = Some(s.to_string());
                                    is_table_marker = true;
                                } else if s.starts_with("__img_pos_") && s.ends_with("__") {
                                    // This is an image position marker — the writer
                                    // emits `<inkuo:id w:val="__img_pos_<img_id>__"/>`
                                    // and the actual `<w:drawing>` block in the same
                                    // paragraph. Round-tripping requires capturing the
                                    // id here so the marker paragraph can be re-emitted
                                    // and `parse_image_xml` can pair it with the
                                    // `<a:blip>` rId it finds deeper in the XML.
                                    current_stable_id = Some(s.to_string());
                                    is_image_marker = true;
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
                    if let Some(v) = attr_value_str(e, b"val") {
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
                    if let Some(v) = attr_value_str(e, b"val") {
                        if let Ok(s) = std::str::from_utf8(v.as_ref()) {
                            if let Ok(n) = s.parse::<u32>() {
                                pending_num_id = Some(n);
                            }
                        }
                    }
                } else if in_numpr && name.as_ref() == b"ilvl" {
                    if let Some(v) = attr_value_str(e, b"val") {
                        if let Ok(s) = std::str::from_utf8(v.as_ref()) {
                            if let Ok(n) = s.parse::<u32>() {
                                pending_ilvl = Some(n);
                            }
                        }
                    }
                } else if name.as_ref() == b"id" && tbl_cell_depth == 0 {
                    // Read stable ID from custom inkuo:id element (empty tag)
                    // This can fire even when para_depth is 0 for self-closing tags
                    if let Some(v) = attr_value_str(e, b"val") {
                        if let Ok(s) = std::str::from_utf8(v.as_ref()) {
                            if !s.is_empty() {
                                if s.starts_with("__tbl_pos_") && s.ends_with("__") {
                                    current_stable_id = Some(s.to_string());
                                    is_table_marker = true;
                                } else if s.starts_with("__img_pos_") && s.ends_with("__") {
                                    current_stable_id = Some(s.to_string());
                                    is_image_marker = true;
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
                        // Keep if: has content, or style, or formatting, or is a
                        // table or image marker. Image markers carry no text and
                        // produce no runs themselves (the run lives in a sub-parse
                        // of `<w:drawing>` in `parse_image_xml`), but we still
                        // want them present so the writer can re-emit the
                        // `<w:drawing>` on the next save.
                        let keep = !current_text.is_empty()
                            || current_style.is_some()
                            || current_numbering.is_some()
                            || has_format
                            || paragraph_saw_run
                            || is_table_marker
                            || is_image_marker;
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
                            // Markers carry a synthetic text that the writer's
                            // build_document_xml() recognises when splicing the
                            // `<w:tbl>` or `<w:drawing>` element back in.
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
                            } else if is_image_marker {
                                // Mirror the writer's marker text so the next
                                // write can splice the `<w:drawing>` back in
                                // via `image_map.get(img_id)`.
                                if let Some(stable_id) = &current_stable_id {
                                    if let Some(rest) = stable_id.strip_prefix("__img_pos_") {
                                        if let Some(img_id) = rest.strip_suffix("__") {
                                            format!("<__img_pos_{}__>", img_id)
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
                            let para = WordParagraph {
                                id,
                                text,
                                style: current_style.clone(),
                                runs: runs_opt,
                                numbering: current_numbering.clone(),
                            };
                            // Image markers go to the side channel so the
                            // caller (read_word_document) can pair them
                            // with the WordImage entries we recover in
                            // parse_image_xml.
                            if is_image_marker {
                                image_markers.push(para);
                            } else {
                                paragraphs.push(para);
                            }
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

    Ok((paragraphs, image_markers))
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

    // ── Image dedup + rels planning ────────────────────────────────────────
    //
    // Walk every image on `doc.images` (the high-level model) and figure out:
    //   1. Which `imageN.ext` filename to write it under inside `word/media/`.
    //      We scan `preserve_from` for existing media entries and continue
    //      counting from `image<max+1>.ext` so we don't collide with the
    //      original document's images.
    //   2. What rels id to use (e.g. `rId6`, `rId7`, ...) — counting from
    //      whatever ids are already in use by the original `word/_rels/document.xml.rels`.
    //   3. What content-type to register in `[Content_Types].xml`.
    //
    // The result `image_writes` carries everything we need to (a) write the
    // bytes into the zip, (b) append the right Override / Relationship rows,
    // and (c) substitute the placeholder `rIdImgPlaceholder` in `doc_xml`
    // with the real rels id.
    let mut image_writes: Vec<ImageWritePlan> = Vec::new();
    if !doc.images.is_empty() {
        let (existing_media_max, next_rid, preserved_refs) =
            scan_preserved_zip_for_image_state(preserve_from)?;
        // Build a lookup table keyed by the zip-internal media path so
        // we can quickly spot which (if any) preserved rels entry
        // already covers a round-tripped image.
        let mut preserved_by_path: std::collections::HashMap<String, PreservedImageRef> =
            std::collections::HashMap::new();
        for r in preserved_refs {
            preserved_by_path.insert(r.target.clone(), r);
        }
        // Start counting fresh media filenames AFTER whatever the
        // preserved zip already has, and start fresh rels ids AFTER
        // whatever the preserved rels file already has. Both counters
        // are inclusive of the next free value (i.e. `media_index = 1`
        // means "use image1.png next"), so we add 1 here.
        let mut media_index = existing_media_max + 1;
        let mut next_rid_u32 = next_rid + 1;

        for img in &doc.images {
            // Two distinct code paths:
            //
            // 1. `internal_path = Some(...)` — the reader recovered this
            //    image from an existing .docx and we want to round-trip
            //    it byte-for-byte. Look up the matching preserved
            //    relationship by media target and reuse its rId; if no
            //    match exists (corrupt/missing rels), fall back to the
            //    new-image path with a fresh rId.
            //
            // 2. `internal_path = None` — caller built a fresh image
            //    from a disk source. Read its bytes, mint a new rId,
            //    allocate a fresh `imageN.ext` filename.
            if let Some(internal) = img.internal_path.as_deref() {
                let target = internal
                    .strip_prefix("word/")
                    .unwrap_or(internal)
                    .to_string();
                if let Some(preserved_ref) = preserved_by_path.get(&target).cloned() {
                    let mut bytes = Vec::new();
                    if let Some(pf) = preserve_from {
                        if let Ok(mut a) =
                            zip::ZipArchive::new(std::io::Cursor::new(pf))
                        {
                            if let Ok(mut f) = a.by_name(internal) {
                                let _ = std::io::Read::read_to_end(&mut f, &mut bytes);
                            }
                        }
                    }
                    let ext_normalised = preserved_ref
                        .target
                        .rsplit('.')
                        .next()
                        .unwrap_or("png")
                        .to_string();
                    let normalised = if ext_normalised == "jpg" {
                        "jpeg".to_string()
                    } else {
                        ext_normalised
                    };
                    let content_type = match normalised.as_str() {
                        "png" => "image/png".to_string(),
                        "jpeg" => "image/jpeg".to_string(),
                        "gif" => "image/gif".to_string(),
                        _ => "image/png".to_string(),
                    };
                    let basename = preserved_ref
                        .target
                        .rsplit('/')
                        .next()
                        .unwrap_or(&preserved_ref.target)
                        .to_string();
                    image_writes.push(ImageWritePlan {
                        bytes,
                        internal_path: internal.to_string(),
                        internal_basename: basename,
                        content_type,
                        rid: preserved_ref.rid,
                    });
                    continue;
                }
                // Fall through to the new-image path when no preserved
                // rel matches — we still don't want to drop the image,
                // we just allocate it fresh.
            }

            let src_path = std::path::Path::new(&img.path);
            let ext = match src_path.extension().and_then(|s| s.to_str()) {
                Some(e) => e.to_ascii_lowercase(),
                None => {
                    return Err(OfficeError::Xml(format!(
                        "Image '{}' has no extension; supported: png, jpeg, jpg, gif",
                        img.path
                    )));
                }
            };
            let ext_normalised = match ext.as_str() {
                "jpg" => "jpeg".to_string(),
                other => other.to_string(),
            };
            let (content_type, _override_extension) = match ext_normalised.as_str() {
                "png" => (
                    "image/png",
                    "png",
                ),
                "jpeg" => ("image/jpeg", "jpeg"),
                "gif" => ("image/gif", "gif"),
                other => {
                    return Err(OfficeError::Xml(format!(
                        "Unsupported image extension '.{}'; supported: png, jpeg, jpg, gif",
                        other
                    )));
                }
            };

            // Read source bytes. Caller is responsible for providing an
            // absolute path; workspace validation happens in the tool layer.
            let bytes = std::fs::read(src_path).map_err(|e| {
                OfficeError::Xml(format!(
                    "Failed to read image source '{}': {}",
                    img.path, e
                ))
            })?;

            let internal_basename = format!("image{}.{}", media_index, ext_normalised);
            media_index += 1;
            let internal_path = format!("word/media/{}", internal_basename);

            let rid = format!("rId{}", next_rid_u32);
            next_rid_u32 += 1;

            image_writes.push(ImageWritePlan {
                bytes,
                internal_path,
                internal_basename,
                content_type: content_type.to_string(),
                rid,
            });
        }
    }

    // Build the generated content strings once. `doc_xml` is built later
    // *after* image rels ids are known, because each image's placeholder
    // needs to be substituted with the real rId. So we keep `doc_xml`
    // mutable below.
    let content_types_base = CONTENT_TYPES_XML;
    let rels = RELS_XML;
    let word_rels_base = WORD_RELS_XML;
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
            // Word/media entries are written by the image planning loop
            // below. The reader→writer round-trip reuses the original
            // media bytes when an image is preserved (`internal_path`
            // set), so copying these entries here would produce
            // duplicate filenames in the resulting archive and trip the
            // zip writer. Preserve-from never touches media.
            if name.starts_with("word/media/") {
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

    // ── Write media entries ────────────────────────────────────────────────
    //
    // Even when there's no `preserve_from` (i.e. brand-new docx), image bytes
    // need to land in the archive before document.xml so the rels forward
    // reference resolves correctly. We write them in plan order.
    for plan in &image_writes {
        zip.start_file(&plan.internal_path, opts)?;
        zip.write_all(&plan.bytes)?;
    }

    // Build the post-image doc_xml so each placeholder can be substituted.
    let doc_xml_raw = build_document_xml(doc);
    let doc_xml = substitute_image_placeholders(&doc_xml_raw, &image_writes);

    // Compose the final `[Content_Types].xml` and `word/_rels/document.xml.rels`
    // with image Overrides / Relationships appended.
    let content_types = append_image_overrides(content_types_base, &image_writes);
    let word_rels = append_image_relationships(word_rels_base, &image_writes);

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

/// One image's worth of writer bookkeeping: bytes, target filename inside
/// the zip, content-type, and the rels id we minted for it.
struct ImageWritePlan {
    bytes: Vec<u8>,
    /// e.g. `word/media/image1.png`.
    internal_path: String,
    /// e.g. `image1.png` — used for the `<Override PartName=...>` path.
    internal_basename: String,
    /// e.g. `image/png`.
    content_type: String,
    /// e.g. `rId6`.
    rid: String,
}

/// Image reference already present in a preserved `.docx`. Re-used on
/// rewrite so the existing rId stays stable and the corresponding media
/// file (still inside the preserved zip) is what the writer points at.
#[derive(Debug, Clone)]
struct PreservedImageRef {
    /// e.g. `rId6`.
    rid: String,
    /// e.g. `media/image1.png` — target path from the preserved rels.
    target: String,
}

/// Look at the original `.docx` (if any) and figure out:
///  - the highest `imageN.ext` index already in `word/media/`
///  - the highest `rId` already used in `word/_rels/document.xml.rels`
///  - the list of image relationships (`Target` paths under `media/`)
///    already declared by the preserved rels file. The writer uses this
///    so when an existing image is round-tripped (`WordImage` with
///    `internal_path` set) it can reuse the original rId instead of
///    allocating a brand new one. Without this each append would alias
///    the preserved image's rId to a freshly allocated imageN.ext and
///    silently orphan every previously-present `<w:drawing>` because
///    their rIds no longer resolve to the right media file.
///
/// The first two numbers are then bumped by 1 in the caller to allocate
/// fresh, non-colliding values for *new* images. Returns
/// `(0, 6, vec![])` for a fresh document with no `preserve_from`.
fn scan_preserved_zip_for_image_state(
    preserve_from: Option<&[u8]>,
) -> Result<(u32, u32, Vec<PreservedImageRef>), OfficeError> {
    let mut max_media_index: u32 = 0;
    let mut max_rid: u32 = 5; // matches WORD_RELS_XML: rId1..rId5
    let mut preserved: Vec<PreservedImageRef> = Vec::new();
    let mut rels_xml: Option<String> = None;
    if let Some(bytes) = preserve_from {
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes))?;
        for i in 0..archive.len() {
            let mut file = archive.by_index(i)?;
            let name = file.name().to_string();
            if let Some(rest) = name.strip_prefix("word/media/image") {
                // rest looks like "12.png" — pull the integer prefix.
                let digits: String = rest
                    .chars()
                    .take_while(|c| c.is_ascii_digit())
                    .collect();
                if !digits.is_empty() {
                    if let Ok(n) = digits.parse::<u32>() {
                        if n > max_media_index {
                            max_media_index = n;
                        }
                    }
                }
            } else if name == "word/_rels/document.xml.rels" {
                // Read up to 1 MB of rels xml (the rels file is always
                // tiny — a few KB). We use `read_to_end` via a small
                // take() shim because ZipFile isn't `io::Read` by value
                // — we have to go through `read`.
                let mut s = String::new();
                let mut limited = file.by_ref().take(1 << 20);
                let _ = std::io::Read::read_to_string(&mut limited, &mut s);
                rels_xml = Some(s);
            }
        }
    }

    if let Some(s) = rels_xml.as_deref() {
        // Cheap rId scanner: pull every `Id="rId<digits>"`.
        // We scan the byte slice once, looking for the 4-byte
        // pattern `Id="` and then verifying the next 3 bytes are
        // `rId`. This is much more robust than sliding over the
        // raw `rId"` because attribute value boundaries vary.
        let bytes = s.as_bytes();
        let mut idx = 0;
        while idx + 8 < bytes.len() {
            if &bytes[idx..idx + 4] == b"Id=\""
                && &bytes[idx + 4..idx + 7] == b"rId"
            {
                let mut j = idx + 7;
                let mut digits = String::new();
                while j < bytes.len() && bytes[j].is_ascii_digit() {
                    digits.push(bytes[j] as char);
                    j += 1;
                }
                if !digits.is_empty() {
                    let id_str = format!("rId{}", digits);
                    if let Ok(n) = digits.parse::<u32>() {
                        if n > max_rid {
                            max_rid = n;
                        }
                    }
                    // Hunt for the sibling `Target=` attribute on the
                    // same Relationship element. We scan backwards/forwards
                    // for the nearest `Target="media/..."` substring.
                    let after_id = j;
                    // Coarse scan — find a `Target="media/<…>"` substring
                    // anywhere after the current rId but before the next
                    // `Id=` (or end of buffer).
                    let next_id = find_next_relationship_id(s, after_id);
                    let window = &s[after_id..next_id];
                    let target = extract_media_target(window);
                    if let Some(t) = target {
                        preserved.push(PreservedImageRef {
                            rid: id_str,
                            target: t,
                        });
                    }
                    idx = next_id;
                } else {
                    idx += 1;
                }
            } else {
                idx += 1;
            }
        }
    }

    Ok((max_media_index, max_rid, preserved))
}

/// Find the byte offset of the next `<… Id="…` after `from` in the
/// rels buffer, or `s.len()` when none. Used to bound the `Target=`
/// search window for one relationship at a time.
fn find_next_relationship_id(s: &str, from: usize) -> usize {
    let bytes = s.as_bytes();
    let mut idx = from;
    while idx + 4 < bytes.len() {
        if &bytes[idx..idx + 4] == b"Id=\"" {
            return idx;
        }
        idx += 1;
    }
    bytes.len()
}

/// Within a single `<Relationship …/>` (or `<Relationship …></Relationship>`)
/// substring, pull out the value of `Target="media/…"`. Returns `None`
/// when the relationship is not an image (`Target` doesn't start with
/// `media/`) — non-image relationships are noise from this scanner's
/// point of view.
fn extract_media_target(window: &str) -> Option<String> {
    let bytes = window.as_bytes();
    let needle = b"Target=\"";
    let mut idx = 0;
    while idx + needle.len() < bytes.len() {
        if &bytes[idx..idx + needle.len()] == needle {
            let start = idx + needle.len();
            if let Some(end_rel) = window[start..].find('"') {
                let target = &window[start..start + end_rel];
                let normalised = target
                    .trim_start_matches('/')
                    .strip_prefix("word/")
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| target.to_string());
                if normalised.starts_with("media/") {
                    return Some(normalised);
                }
                return None;
            }
            return None;
        }
        idx += 1;
    }
    None
}

/// Replace every `rIdImgPlaceholder` in `doc_xml` with the corresponding
/// real rels id from `image_writes`. The placeholder is intentionally
/// unique; missing rewrites show up as obvious `rIdImgPlaceholder`
/// strings in the resulting docx and would fail Word's strict rels check.
fn substitute_image_placeholders(doc_xml: &str, image_writes: &[ImageWritePlan]) -> String {
    let mut out = String::with_capacity(doc_xml.len());
    let mut idx = 0;
    let placeholder = "rIdImgPlaceholder";
    for plan in image_writes {
        // No-op for empty plans — the placeholder is only injected when an
        // image was actually emitted by `build_document_xml`.
        if let Some(found) = doc_xml[idx..].find(placeholder) {
            let abs = idx + found;
            out.push_str(&doc_xml[idx..abs]);
            out.push_str(&plan.rid);
            idx = abs + placeholder.len();
        } else {
            // No more placeholders to rewrite; copy the rest verbatim.
            out.push_str(&doc_xml[idx..]);
            return out;
        }
    }
    out.push_str(&doc_xml[idx..]);
    out
}

/// Append an `<Override>` row for each image's media entry. The
/// `Default Extension="png"` rows already in the base `CONTENT_TYPES_XML`
/// cover most cases, but a brand-new `.docx` (no preserved zip) should
/// still declare Overrides explicitly so Word's "missing part" check
/// doesn't reject the package.
fn append_image_overrides(base: &str, image_writes: &[ImageWritePlan]) -> String {
    if image_writes.is_empty() {
        return base.to_string();
    }
    // De-duplicate on `/word/media/<basename>`. Round-tripped images can
    // land in `image_writes` twice (once via `internal_path` reuse, once
    // via a fresh `DocElement::Image` sharing the same source path) —
    // emitting the same Override twice would corrupt the resulting
    // `[Content_Types].xml`.
    let mut emitted: std::collections::HashSet<String> = std::collections::HashSet::new();
    // The base ends with `</Types>`. Inject overrides right before that.
    let close = "</Types>";
    let Some(pos) = base.rfind(close) else {
        return base.to_string();
    };
    let mut out = String::with_capacity(base.len() + image_writes.len() * 128);
    out.push_str(&base[..pos]);
    for plan in image_writes {
        if !emitted.insert(plan.internal_basename.clone()) {
            continue;
        }
        out.push_str(&format!(
            "  <Override PartName=\"/word/media/{}\" ContentType=\"{}\"/>\n",
            escape_xml(&plan.internal_basename),
            escape_xml(&plan.content_type),
        ));
    }
    out.push_str(&base[pos..]);
    out
}

/// Append an `<Relationship>` row for each image's media entry. Targets
/// are relative to `word/document.xml` (i.e. just `media/image1.png`),
/// and ids are the `rid` we minted in the planning phase.
///
/// When round-tripping an existing image we re-use the original rId, so
/// the same Relationship gets re-emitted if it shows up twice in
/// `image_writes` (one entry from `internal_path` reuse, one from a fresh
/// `DocElement::Image`). We de-duplicate on the rId so the resulting
/// rels file stays valid.
fn append_image_relationships(base: &str, image_writes: &[ImageWritePlan]) -> String {
    if image_writes.is_empty() {
        return base.to_string();
    }
    let close = "</Relationships>";
    let Some(pos) = base.rfind(close) else {
        return base.to_string();
    };
    let mut out = String::with_capacity(base.len() + image_writes.len() * 192);
    let mut emitted_ids: std::collections::HashSet<String> = std::collections::HashSet::new();
    out.push_str(&base[..pos]);
    for plan in image_writes {
        if !emitted_ids.insert(plan.rid.clone()) {
            continue;
        }
        out.push_str(&format!(
            "  <Relationship Id=\"{}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/image\" Target=\"media/{}\"/>\n",
            escape_xml(&plan.rid),
            escape_xml(&plan.internal_basename),
        ));
    }
    out.push_str(&base[pos..]);
    out
}

/// True when the document contains at least one paragraph with a numbering
/// reference. Used to decide whether `word/numbering.xml` should be emitted.
fn doc_has_numbering(doc: &WordDocument) -> bool {
    doc.paragraphs.iter().any(|p| p.numbering.is_some())
}

/// Recover existing `<w:drawing>` images from the document so the model
/// can re-emit them on the next save. Without this, appending a *new*
/// image to a docx that already embeds an older one would silently drop
/// the older image's `<w:drawing>` and its relationship — the picture
/// bytes would still be inside the zip, but Word would have no idea
/// where they belong.
///
/// Strategy:
///   1. Parse `word/_rels/document.xml.rels` once to build a
///      rId → relative-path (e.g. `media/image3.png`) lookup.
///   2. Walk `word/document.xml` for every `<a:blip
///      r:embed="rIdN"/>` element. For each, also pick up the
///      neighbouring `<wp:extent cx="..." cy="..."/>` for the EMU size
///      and scan the same enclosing paragraph for an `<inkuo:id
///      w:val="__img_pos_<img_id>__"/>` marker. The marker id is the
///      stable id the writer uses to pair this drawing with its
///      `WordImage` entry. When the marker is missing we synthesise a
///      fresh id from the rId so we still surface the picture to the
///      model.
///
/// Every recovered entry sets `internal_path = Some(...)` so the writer
/// knows to reuse the existing zip bytes and rId instead of allocating
/// a new `imageN.ext`.
fn parse_image_xml(
    doc_content: &str,
    rels_content: &str,
    _image_markers: &[WordParagraph],
) -> Vec<WordImage> {
    let rid_to_target = parse_image_rels(rels_content);
    if rid_to_target.is_empty() {
        return Vec::new();
    }

    let mut reader = quick_xml::Reader::from_str(doc_content);
    reader.config_mut().trim_text(false);
    let mut buf = Vec::new();

    // Per-drawing state. We reset these at the start of each
    // `<w:drawing>` element.
    let mut in_drawing = false;
    let mut blip_rid: Option<String> = None;
    let mut cx: u32 = 0;
    let mut cy: u32 = 0;

    // Per-paragraph state. The writer decorates every image-bearing
    // paragraph with a `<inkuo:id w:val="__img_pos_<img_id>__"/>`; we
    // capture it here so the recovered entry uses the same stable id
    // the writer will key off on the next save. The id may appear
    // *before* the `<w:drawing>` child element inside `<w:pPr>` (Start
    // tag) or right at the start of the paragraph (Empty tag).
    let mut current_para_id: Option<String> = None;
    let mut current_para_depth = 0usize;

    let mut images: Vec<WordImage> = Vec::new();

    loop {
        let event = reader.read_event_into(&mut buf);
        match event {
            Ok(quick_xml::events::Event::Start(ref e)) | Ok(quick_xml::events::Event::Empty(ref e)) => {
                let name = e.local_name();
                let is_empty = matches!(event, Ok(quick_xml::events::Event::Empty(_)));
                if name.as_ref() == b"p" {
                    current_para_depth += 1;
                    current_para_id = None;
                } else if name.as_ref() == b"id" && current_para_depth > 0 {
                    // inkuo:id inside the paragraph — could be the marker.
                    if let Some(v) = attr_value_str(e, b"val") {
                        if !v.is_empty() {
                            current_para_id = Some(v);
                        }
                    }
                } else if name.as_ref() == b"drawing" {
                    in_drawing = true;
                    blip_rid = None;
                    cx = 0;
                    cy = 0;
                } else if in_drawing && name.as_ref() == b"extent" {
                    if let Some(v) = attr_value_str(e, b"cx") {
                        if let Ok(n) = v.parse::<u32>() {
                            cx = n;
                        }
                    }
                    if let Some(v) = attr_value_str(e, b"cy") {
                        if let Ok(n) = v.parse::<u32>() {
                            cy = n;
                        }
                    }
                } else if in_drawing && name.as_ref() == b"blip" {
                    if let Some(v) = attr_value_str(e, b"embed") {
                        blip_rid = Some(v);
                    }
                    if is_empty {
                        // `<a:blip ... />` is usually self-closing — flush
                        // the recovery record now.
                        flush_image(&mut images, &blip_rid, cx, cy, current_para_id.as_deref(), &rid_to_target);
                        in_drawing = false;
                        blip_rid = None;
                    }
                }
            }
            Ok(quick_xml::events::Event::End(ref e)) => {
                let name = e.local_name();
                if name.as_ref() == b"drawing" {
                    // Empty / non-self-closing blip flush. `<a:blip />`
                    // cases were flushed inside the Start handler.
                    flush_image(&mut images, &blip_rid, cx, cy, current_para_id.as_deref(), &rid_to_target);
                    in_drawing = false;
                    blip_rid = None;
                } else if name.as_ref() == b"p" && current_para_depth > 0 {
                    current_para_depth -= 1;
                    if current_para_depth == 0 {
                        current_para_id = None;
                    }
                }
            }
            Ok(quick_xml::events::Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
        buf.clear();
    }

    images
}

/// Push one recovered `WordImage` onto `images` if `blip_rid` resolves
/// to a media entry we recognise. Centralises the policy so the
/// self-closing and balanced-tag code paths stay in lockstep.
fn flush_image(
    images: &mut Vec<WordImage>,
    blip_rid: &Option<String>,
    cx: u32,
    cy: u32,
    para_id: Option<&str>,
    rid_to_target: &std::collections::HashMap<String, String>,
) {
    let Some(rid) = blip_rid.as_deref() else {
        return;
    };
    let Some(target) = rid_to_target.get(rid) else {
        return;
    };
    let internal_path = format!("word/{}", target);
    let img_id = para_id
        .and_then(|p| p.strip_prefix("__img_pos_"))
        .and_then(|rest| rest.strip_suffix("__"))
        .map(|s| s.to_string())
        .unwrap_or_else(|| format!("_recovered_{}", rid));
    images.push(WordImage {
        id: img_id,
        path: internal_path.clone(),
        width_emu: cx,
        height_emu: cy,
        internal_path: Some(internal_path),
    });
}

/// Parse `word/_rels/document.xml.rels` and return a map from
/// `rIdN` → `media/imageN.ext` (the path is kept *relative* to `word/`
/// so the writer can prepend the prefix when needed).
fn parse_image_rels(rels_content: &str) -> std::collections::HashMap<String, String> {
    let mut map = std::collections::HashMap::new();
    let mut reader = quick_xml::Reader::from_str(rels_content);
    reader.config_mut().trim_text(false);
    let mut buf = Vec::new();
    loop {
        let event = reader.read_event_into(&mut buf);
        match event {
            Ok(quick_xml::events::Event::Start(ref e)) | Ok(quick_xml::events::Event::Empty(ref e)) => {
                if e.local_name().as_ref() == b"Relationship" {
                    let id = attr_value_str(e, b"Id").unwrap_or_default();
                    let target = attr_value_str(e, b"Target").unwrap_or_default();
                    let ty = attr_value_str(e, b"Type").unwrap_or_default();
                    if id.is_empty() || target.is_empty() {
                        continue;
                    }
                    // Only image relationships carry forward — styles,
                    // settings, etc. must not enter the image rels map.
                    if !ty.contains("/image") && !ty.contains("/chart") {
                        continue;
                    }
                    if !target.starts_with("media/") && !target.starts_with("/word/media/") {
                        continue;
                    }
                    let normalised = target
                        .trim_start_matches('/')
                        .strip_prefix("word/")
                        .map(|s| s.to_string())
                        .unwrap_or(target);
                    map.insert(id.to_string(), normalised);
                }
            }
            Ok(quick_xml::events::Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
        buf.clear();
    }
    map
}

/// Pull a `String` out of a quick-xml attribute. Convenience wrapper
/// over the file's existing `attr_value` (which returns a `Cow<[u8]>`)
/// for callers that already know they want an owned `String`.
fn attr_value_str(e: &quick_xml::events::BytesStart<'_>, attr: &[u8]) -> Option<String> {
    for a in e.attributes().with_checks(false).flatten() {
        let key = a.key.as_ref();
        // quick_xml emits `inkuo:id` (namespaced) in the doc but raw
        // `Id` / `cx` in the rels file, so match on either the full or
        // the local part of the key.
        let local = key
            .iter()
            .position(|&b| b == b':')
            .map(|i| &key[i + 1..])
            .unwrap_or(key);
        if local == attr {
            return Some(String::from_utf8_lossy(&a.value).into_owned());
        }
    }
    None
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
            xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing"
            xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"
            xmlns:pic="http://schemas.openxmlformats.org/drawingml/2006/picture"
            xmlns:inkuo="http://inkuo.app/wordprocessingml/2026/main">
  <w:body>"#
    );

    // Build a map of table id -> table for O(1) lookup.
    let table_map: std::collections::HashMap<&str, &WordTable> =
        doc.tables.iter().map(|t| (t.id.as_str(), t)).collect();

    // Build a map of image id -> image for O(1) lookup. The image's stable id
    // is matched against `<__img_pos_<id>__>` marker paragraphs.
    let image_map: std::collections::HashMap<&str, &WordImage> =
        doc.images.iter().map(|i| (i.id.as_str(), i)).collect();

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

        // Image position marker: emit a paragraph that carries the inline
        // `<w:drawing>` run. The image's internal `word/media/imageN.ext`
        // path was resolved by the writer; the `rId` is looked up from
        // the rels table the writer builds up alongside this XML. We use
        // a placeholder `rId` here (`rIdImg<index>`) and rewrite it in
        // `write_word_document` after the rels are finalised.
        if let Some(rest) = para.text.strip_prefix("<__img_pos_") {
            if let Some(end) = rest.find("__>") {
                let img_id = &rest[..end];
                if let Some(img) = image_map.get(img_id) {
                    xml.push_str("\n    <w:p>");
                    xml.push_str(&format!(
                        "<w:pPr><inkuo:id w:val=\"__img_pos_{}__\"/></w:pPr>",
                        escape_xml(img_id)
                    ));
                    xml.push_str(&build_image_drawing_xml(img));
                    xml.push_str("</w:p>");
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

/// Render an inline picture run as a `<w:drawing>` element.
///
/// `r:embed="rIdImgPlaceholder"` is rewritten by `write_word_document` after
/// the writer knows the final rels id (which may shift when the original
/// document already used `rId1`, `rId2`, ... for its own styles / numbering
/// / hyperlinks). The placeholder is deliberately unique so a missed
/// rewrite is impossible to miss in QA.
fn build_image_drawing_xml(img: &WordImage) -> String {
    let cx = img.width_emu;
    let cy = img.height_emu;
    // Image element name (the `pic:name` is a cosmetic label — Word picks
    // it up from the picture's own embedded metadata; we use the stable id
    // so debug dumps correlate with `WordDocument.images`).
    let name = escape_xml(&img.id);
    format!(
        concat!(
            "<w:r><w:drawing>",
            "<wp:inline distT=\"0\" distB=\"0\" distL=\"0\" distR=\"0\">",
            "<wp:extent cx=\"{cx}\" cy=\"{cy}\"/>",
            "<wp:effectExtent l=\"0\" t=\"0\" r=\"0\" b=\"0\"/>",
            "<wp:docPr id=\"{docpr_id}\" name=\"{name}\"/>",
            "<wp:cNvGraphicFramePr>",
            "<a:graphicFrameLocks xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" noChangeAspect=\"1\"/>",
            "</wp:cNvGraphicFramePr>",
            "<a:graphic xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\">",
            "<a:graphicData uri=\"http://schemas.openxmlformats.org/drawingml/2006/picture\">",
            "<pic:pic xmlns:pic=\"http://schemas.openxmlformats.org/drawingml/2006/picture\">",
            "<pic:nvPicPr>",
            "<pic:cNvPr id=\"{docpr_id}\" name=\"{name}\"/>",
            "<pic:cNvPicPr/>",
            "</pic:nvPicPr>",
            "<pic:blipFill>",
            "<a:blip xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" r:embed=\"rIdImgPlaceholder\"/>",
            "<a:stretch><a:fillRect/></a:stretch>",
            "</pic:blipFill>",
            "<pic:spPr>",
            "<a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"{cx}\" cy=\"{cy}\"/></a:xfrm>",
            "<a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom>",
            "</pic:spPr>",
            "</pic:pic>",
            "</a:graphicData>",
            "</a:graphic>",
            "</wp:inline>",
            "</w:drawing></w:r>"
        ),
        cx = cx,
        cy = cy,
        name = name,
        // `docPr id` and `pic:cNvPr id` are namespace-local identifiers. We
        // derive them from the image's stable id (stable, non-negative) by
        // hashing. For v1 we just use the count of leading hex chars of the
        // id as a quick-and-deterministic number; collisions are harmless
        // because Word treats the id as opaque within a document.
        docpr_id = stable_id_to_docpr_id(&img.id),
    )
}

/// Deterministically derive a positive integer id (1..i32::MAX) from a
/// string stable id. Used as `wp:docPr id` and `pic:cNvPr id` for inline
/// pictures. Stable across writes, so the docx diff stays clean.
fn stable_id_to_docpr_id(id: &str) -> u32 {
    // FNV-1a 32-bit; output fits in u32.
    let mut hash: u32 = 2_166_136_261u32;
    for b in id.as_bytes() {
        hash ^= *b as u32;
        hash = hash.wrapping_mul(16_777_619u32);
    }
    // Avoid the 0 sentinel — Word occasionally treats 0 as "no id".
    hash.max(1)
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
        let (paragraphs, _image_markers) = parse_document_xml(src).expect("parse should succeed");
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
            images: vec![],
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
            images: vec![],
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
            images: vec![],
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
            images: vec![],
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
            images: vec![],
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
        let doc = WordDocument { paragraphs, tables: tbls, images: vec![] };
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
        let rebuilt_doc = WordDocument { paragraphs: vec![], tables: vec![rebuilt.tables.into_iter().next().unwrap()], images: vec![] };
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

    // ─── Image insertion: dedup, drawing XML, rels / content types ──────────

    /// Helper: build a Word document that embeds a single inline image and
    /// write it through the public writer so the zip, rels and
    /// `[Content_Types].xml` are all produced.
    fn build_single_image_doc(
        id: &str,
        source: std::path::PathBuf,
        width_emu: u32,
        height_emu: u32,
    ) -> WordDocument {
        WordDocument::from_elements(vec![DocElement::Image {
            id: id.to_string(),
            position: 0,
            path: source.to_string_lossy().to_string(),
            width_emu,
            height_emu,
        }])
    }

    /// End-to-end: write a docx with one image, open the zip, and assert
    /// the media file, rels entry, content-type override, and `<w:drawing>`
    /// placeholder substitution all line up. The placeholder rewrite is
    /// the most subtle part — if it ever silently fails, Word's "missing
    /// rels" repair prompt kicks in.
    #[test]
    fn write_image_emits_media_rels_and_drawing() {
        // Use a tiny synthetic PNG (1x1 white pixel, smallest valid PNG).
        let png_bytes: &[u8] = &[
            0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, // signature
            0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52, // IHDR chunk
            0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01,
            0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4,
            0x89,
            0x00, 0x00, 0x00, 0x0D, 0x49, 0x44, 0x41, 0x54, // IDAT chunk
            0x78, 0x9C, 0x62, 0x00, 0x01, 0x00, 0x00, 0x05,
            0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4,
            0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, // IEND chunk
            0xAE, 0x42, 0x60, 0x82,
        ];
        let tmpdir = std::env::temp_dir().join(format!("inkuo_img_{}", std::process::id()));
        std::fs::create_dir_all(&tmpdir).expect("create tmpdir");
        let source = tmpdir.join("pixel.png");
        std::fs::write(&source, png_bytes).expect("write png");

        // 1.65" wide × 1.24" tall — 1507215 / 1132987 EMU, no significance.
        let doc = build_single_image_doc("img1", source.clone(), 1507215, 1132987);
        let mut buf = std::io::Cursor::new(Vec::<u8>::new());
        write_word_document(&doc, &mut buf, None).expect("write should succeed");
        let bytes = buf.into_inner();

        // ── 1. media entry exists in the zip ───────────────────────────────
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes.as_slice()))
            .expect("output must be a valid zip");
        let names: Vec<String> = (0..archive.len())
            .map(|i| archive.by_index(i).unwrap().name().to_string())
            .collect();
        assert!(
            names.contains(&"word/media/image1.png".to_string()),
            "expected word/media/image1.png in archive; got {:?}",
            names
        );

        // ── 2. content-types override references the media part ───────────
        let mut ct = String::new();
        std::io::Read::read_to_string(
            &mut archive.by_name("[Content_Types].xml").unwrap(),
            &mut ct,
        )
        .unwrap();
        assert!(
            ct.contains("PartName=\"/word/media/image1.png\""),
            "content types missing override: {}",
            ct
        );
        assert!(
            ct.contains("ContentType=\"image/png\""),
            "content types missing image/png: {}",
            ct
        );

        // ── 3. document rels has the image relationship ───────────────────
        let mut rels = String::new();
        std::io::Read::read_to_string(
            &mut archive.by_name("word/_rels/document.xml.rels").unwrap(),
            &mut rels,
        )
        .unwrap();
        // The first image gets rId6 (rId1..rId5 are reserved for
        // styles/settings/fontTable/theme/numbering).
        assert!(
            rels.contains("Id=\"rId6\""),
            "rels missing rId6: {}",
            rels
        );
        assert!(
            rels.contains("Target=\"media/image1.png\""),
            "rels missing media/image1.png target: {}",
            rels
        );
        assert!(
            rels.contains("relationships/image"),
            "rels missing image relationship type: {}",
            rels
        );

        // ── 4. document.xml has the inline drawing with the rId filled in ─
        let mut doc_xml = String::new();
        std::io::Read::read_to_string(
            &mut archive.by_name("word/document.xml").unwrap(),
            &mut doc_xml,
        )
        .unwrap();
        assert!(
            doc_xml.contains("<w:drawing>"),
            "document.xml missing <w:drawing>: {}",
            doc_xml
        );
        assert!(
            doc_xml.contains("r:embed=\"rId6\""),
            "document.xml missing r:embed=\"rId6\" (placeholder rewrite failed): {}",
            doc_xml
        );
        assert!(
            !doc_xml.contains("rIdImgPlaceholder"),
            "placeholder should have been replaced; still present in: {}",
            doc_xml
        );
        // The marker paragraph id must round-trip too so the read path
        // can match it later.
        assert!(
            doc_xml.contains("__img_pos_img1__"),
            "document.xml missing image marker: {}",
            doc_xml
        );

        // cleanup
        let _ = std::fs::remove_dir_all(&tmpdir);
    }

    /// Dedup: a single doc that contains two images must land in
    /// `word/media/image1.png` and `word/media/image2.png` with rels
    /// `rId6` and `rId7` (since rId1..rId5 are used by the boilerplate).
    #[test]
    fn write_two_images_increments_index_and_rid() {
        let png_bytes: &[u8] = &[
            0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A,
            0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52,
            0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01,
            0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4, 0x89,
            0x00, 0x00, 0x00, 0x0D, 0x49, 0x44, 0x41, 0x54,
            0x78, 0x9C, 0x62, 0x00, 0x01, 0x00, 0x00, 0x05,
            0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4,
            0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44,
            0xAE, 0x42, 0x60, 0x82,
        ];
        let tmpdir = std::env::temp_dir().join(format!("inkuo_img2_{}", std::process::id()));
        std::fs::create_dir_all(&tmpdir).expect("create tmpdir");
        let src_a = tmpdir.join("a.png");
        let src_b = tmpdir.join("b.png");
        std::fs::write(&src_a, png_bytes).expect("write a");
        std::fs::write(&src_b, png_bytes).expect("write b");

        let doc = WordDocument::from_elements(vec![
            DocElement::Image {
                id: "first".into(),
                position: 0,
                path: src_a.to_string_lossy().to_string(),
                width_emu: 914400,
                height_emu: 914400,
            },
            DocElement::Image {
                id: "second".into(),
                position: 1,
                path: src_b.to_string_lossy().to_string(),
                width_emu: 914400,
                height_emu: 914400,
            },
        ]);
        let mut buf = std::io::Cursor::new(Vec::<u8>::new());
        write_word_document(&doc, &mut buf, None).expect("write should succeed");
        let bytes = buf.into_inner();

        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes.as_slice())).unwrap();
        let names: Vec<String> = (0..archive.len())
            .map(|i| archive.by_index(i).unwrap().name().to_string())
            .collect();
        assert!(names.contains(&"word/media/image1.png".to_string()));
        assert!(names.contains(&"word/media/image2.png".to_string()));

        let mut rels = String::new();
        std::io::Read::read_to_string(
            &mut archive.by_name("word/_rels/document.xml.rels").unwrap(),
            &mut rels,
        )
        .unwrap();
        assert!(rels.contains("Id=\"rId6\""));
        assert!(rels.contains("Id=\"rId7\""));

        // Cleanup
        let _ = std::fs::remove_dir_all(&tmpdir);
    }

    /// `from_elements` must drop a `DocElement::Image` into the right
    /// shape: marker paragraph + `WordImage` entry. The marker paragraph
    /// uses the same `__<kind>_pos_<id>__` convention as tables so the
    /// existing parser doesn't have to grow.
    #[test]
    fn from_elements_records_image_with_marker_paragraph() {
        let doc = WordDocument::from_elements(vec![DocElement::Image {
            id: "img42".into(),
            position: 0,
            path: "/tmp/foo.png".into(),
            width_emu: 914400,
            height_emu: 914400,
        }]);
        assert_eq!(doc.images.len(), 1);
        assert_eq!(doc.images[0].id, "img42");
        assert_eq!(doc.images[0].path, "/tmp/foo.png");
        // The marker paragraph carries the image id so `build_document_xml`
        // can pair it with the `WordImage` entry.
        assert_eq!(doc.paragraphs.len(), 1);
        assert_eq!(doc.paragraphs[0].id, "__img_pos_img42__");
        assert_eq!(doc.paragraphs[0].text, "<__img_pos_img42__>");
    }

    /// Round-tripping through `scan_preserved_zip_for_image_state` must
    /// count existing `imageN.png` entries and existing rId numbers, so
    /// fresh additions never collide.
    #[test]
    fn scan_preserved_zip_finds_max_image_index_and_rid() {
        // Build a minimal docx-with-images by hand: a single
        // `word/media/image7.png` and a rels file with `rId9`.
        let mut zip_buf = std::io::Cursor::new(Vec::<u8>::new());
        {
            let mut zip = zip::ZipWriter::new(&mut zip_buf);
            let opts = zip::write::SimpleFileOptions::default();
            zip.start_file("word/media/image7.png", opts).unwrap();
            zip.write_all(b"fake").unwrap();
            zip.start_file("word/media/image3.png", opts).unwrap();
            zip.write_all(b"fake").unwrap();
            zip.start_file("word/_rels/document.xml.rels", opts).unwrap();
            zip.write_all(
                br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/>
  <Relationship Id="rId9" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="media/image7.png"/>
</Relationships>"#,
            )
            .unwrap();
            zip.finish().unwrap();
        }
        let bytes = zip_buf.into_inner();
        let (max_idx, max_rid, _preserved) =
            scan_preserved_zip_for_image_state(Some(&bytes)).expect("scan ok");
        assert_eq!(max_idx, 7, "max image index should be 7");
        assert_eq!(max_rid, 9, "max rId should be 9");
    }

    /// Reproduction test for "append wipes earlier images".
    ///
    /// Before the fix this assertion fails: read_word_document's
    /// `parse_image_xml` was a no-op stub, so the marker paragraph and
    /// the `WordImage` entry that the writer had emitted in round 1 were
    /// dropped on round 2's reload, leaving only the freshly-added
    /// image2.png live in the model.
    #[test]
    fn append_image_preserves_existing_image_relationships() {
        let png_bytes: &[u8] = &[
            0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A,
            0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52,
            0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01,
            0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4,
            0x89, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x44, 0x41,
            0x54, 0x78, 0x9C, 0x62, 0x00, 0x01, 0x00, 0x00,
            0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00,
            0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE,
            0x42, 0x60, 0x82,
        ];
        let tmpdir = std::env::temp_dir().join(format!("inkuo_append_{}", std::process::id()));
        std::fs::create_dir_all(&tmpdir).expect("tmpdir");
        let src_a = tmpdir.join("a.png");
        let src_b = tmpdir.join("b.png");
        std::fs::write(&src_a, png_bytes).unwrap();
        std::fs::write(&src_b, png_bytes).unwrap();

        // Round 1: write a fresh docx containing one image.
        let doc1 = WordDocument::from_elements(vec![
            DocElement::Paragraph {
                id: "p0".into(),
                text: "intro".into(),
                omit_text: false,
                style: None,
                runs: None,
                numbering: None,
            },
            DocElement::Image {
                id: "imgA".into(),
                position: 0,
                path: src_a.to_string_lossy().to_string(),
                width_emu: 1000000,
                height_emu: 1000000,
            },
        ]);
        let mut buf1 = std::io::Cursor::new(Vec::<u8>::new());
        write_word_document(&doc1, &mut buf1, None).expect("round 1 write");
        let bytes1 = buf1.into_inner();

        // Round 2: simulate "append a new image" by reading + adding a
        // second image + writing back, preserving the original zip.
        let mut doc2 = read_word_document(&bytes1).expect("round 2 read");
        doc2.images.push(WordImage {
            id: "imgB".into(),
            // `path` is the source on disk — the writer reads this.
            path: src_b.to_string_lossy().to_string(),
            width_emu: 1000000,
            height_emu: 1000000,
            internal_path: None,
        });
        // Marker paragraph for the new image so it gets emitted in doc.xml.
        doc2.paragraphs.push(WordParagraph {
            id: "__img_pos_imgB__".into(),
            text: "<__img_pos_imgB__>".into(),
            style: None,
            runs: None,
            numbering: None,
        });
        let mut buf2 = std::io::Cursor::new(Vec::<u8>::new());
        write_word_document(&doc2, &mut buf2, Some(&bytes1)).expect("round 2 write");
        let bytes2 = buf2.into_inner();

        // Now reload and check both images survived, AND both <w:drawing>
        // runs are still in the document.
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes2.as_slice()))
            .expect("round 2 must be a valid zip");
        let mut doc_xml = String::new();
        std::io::Read::read_to_string(
            &mut archive.by_name("word/document.xml").unwrap(),
            &mut doc_xml,
        )
        .unwrap();
        assert_eq!(
            doc_xml.matches("<w:drawing>").count(),
            2,
            "expected 2 inline drawings after append, got {}: {}",
            doc_xml.matches("<w:drawing>").count(),
            doc_xml
        );

        // The actual model must also see both images when re-read, otherwise
        // the next round of append would silently lose one.
        let reread = read_word_document(&bytes2).expect("reload after append");
        assert_eq!(
            reread.images.len(),
            2,
            "expected 2 images in the model after re-read, got {} (ids: {:?})",
            reread.images.len(),
            reread.images.iter().map(|i| &i.id).collect::<Vec<_>>()
        );

        // imgA was round-tripped through the preserved zip, so its
        // `internal_path` must point at the existing media file so a
        // subsequent append knows not to re-read its bytes from disk.
        let img_a = reread.images.iter().find(|i| i.id == "imgA")
            .expect("imgA survived the round trip");
        assert_eq!(
            img_a.internal_path.as_deref(),
            Some("word/media/image1.png"),
            "imgA.internal_path should point at the preserved media file: {:?}",
            img_a.internal_path
        );
        // imgB was inserted in round 2, but after writer pass + read
        // it now lives at `word/media/image2.png` inside the zip. The
        // re-read is expected to surface that internal_path too, so
        // appending a third image in round 3 will reuse its bytes
        // rather than chancing a re-read off the (now-stale) original
        // source path.
        let img_b = reread.images.iter().find(|i| i.id == "imgB")
            .expect("imgB was added in round 2");
        assert_eq!(
            img_b.internal_path.as_deref(),
            Some("word/media/image2.png"),
            "imgB.internal_path should reflect the zip path it landed in: {:?}",
            img_b.internal_path
        );

        // And the rels file should reference both media files with distinct
        // rIds pointing at non-colliding targets.
        let mut rels = String::new();
        std::io::Read::read_to_string(
            &mut archive.by_name("word/_rels/document.xml.rels").unwrap(),
            &mut rels,
        )
        .unwrap();
        assert!(rels.contains("media/image1.png"), "missing rels for image1: {}", rels);
        assert!(rels.contains("media/image2.png"), "missing rels for image2: {}", rels);

        let _ = std::fs::remove_dir_all(&tmpdir);
    }

    /// Tighter regression: three successive appends (each read+write
    /// cycle preserving the original zip) must keep every previously
    /// inserted image's `<w:drawing>` and relationship intact. The bug
    /// the prior test caught was about the *second* round; this one
    /// walks a third round too because the failure pattern only fully
    /// reproduces when we exercise the "reuse preserved rId" code path
    /// on already-recovered images.
    #[test]
    fn three_round_appends_keep_every_image() {
        let png_bytes: &[u8] = &[
            0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A,
            0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52,
            0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01,
            0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4,
            0x89, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x44, 0x41,
            0x54, 0x78, 0x9C, 0x62, 0x00, 0x01, 0x00, 0x00,
            0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00,
            0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE,
            0x42, 0x60, 0x82,
        ];
        let tmpdir = std::env::temp_dir().join(format!("inkuo_3r_{}", std::process::id()));
        std::fs::create_dir_all(&tmpdir).expect("tmpdir");
        let sources: [std::path::PathBuf; 3] = [
            tmpdir.join("a.png"),
            tmpdir.join("b.png"),
            tmpdir.join("c.png"),
        ];
        for s in &sources {
            std::fs::write(s, png_bytes).unwrap();
        }

        // Round 1: a single inline image.
        let mut current_bytes = {
            let mut buf = std::io::Cursor::new(Vec::<u8>::new());
            let doc = WordDocument::from_elements(vec![DocElement::Image {
                id: "imgA".into(),
                position: 0,
                path: sources[0].to_string_lossy().to_string(),
                width_emu: 1000000,
                height_emu: 1000000,
            }]);
            write_word_document(&doc, &mut buf, None).expect("round 1 write");
            buf.into_inner()
        };

        // Rounds 2 and 3: read+add an image, write back. We exercise
        // the "append after multiple existing images" path on each
        // iteration.
        for round_idx in 1..=2usize {
            let next_id = if round_idx == 1 { "imgB" } else { "imgC" };
            let next_path = &sources[round_idx];
            let mut doc = read_word_document(&current_bytes)
                .expect("read before append");
            doc.images.push(WordImage {
                id: next_id.into(),
                path: next_path.to_string_lossy().to_string(),
                width_emu: 1000000,
                height_emu: 1000000,
                internal_path: None,
            });
            doc.paragraphs.push(WordParagraph {
                id: format!("__img_pos_{}__", next_id),
                text: format!("<__img_pos_{}__>", next_id),
                style: None,
                runs: None,
                numbering: None,
            });
            let mut buf = std::io::Cursor::new(Vec::<u8>::new());
            write_word_document(&doc, &mut buf, Some(&current_bytes))
                .expect("append write");
            current_bytes = buf.into_inner();
        }

        // After all three rounds, every image must survive — both as a
        // `<w:drawing>` run and as a record in the re-read model.
        let mut archive =
            zip::ZipArchive::new(std::io::Cursor::new(current_bytes.as_slice()))
                .expect("zip");
        let mut doc_xml = String::new();
        std::io::Read::read_to_string(
            &mut archive.by_name("word/document.xml").unwrap(),
            &mut doc_xml,
        )
        .unwrap();
        let drawing_count = doc_xml.matches("<w:drawing>").count();
        assert_eq!(
            drawing_count, 3,
            "expected 3 inline drawings after 3 rounds of append, got {}: {}",
            drawing_count, doc_xml
        );

        let reread = read_word_document(&current_bytes)
            .expect("final read");
        assert_eq!(
            reread.images.len(),
            3,
            "expected 3 images in the model after 3 rounds, got {}",
            reread.images.len()
        );
        // All three ids must still be present (none should have been
        // silently aliased onto the same rId).
        let mut ids: Vec<&str> = reread.images.iter().map(|i| i.id.as_str()).collect();
        ids.sort();
        assert_eq!(
            ids,
            vec!["imgA", "imgB", "imgC"],
            "image ids after 3 rounds: {:?}",
            ids
        );

        let _ = std::fs::remove_dir_all(&tmpdir);
    }
}
