//! Word (.docx) document parsing and writing.
//!
//! ## Module layout (after second-phase refactor)
//!
//! Public types live in `mod.rs` so callers can keep using
//! `crate::office::docx::WordDocument` etc. without learning a new path.
//! Streaming-XML helpers have been split out so future format changes
//! don't force a 4 800-line scroll:
//!
//! | File | Responsibility |
//! |------|----------------|
//! | `mod.rs` (~3 400 lines) | Public types (`WordDocument`, `WordParagraph`, …), top-level zip reader entry point (`read_word_document`), the unified `write_word_document` writer, image / header-footer splicing, section / margin / page-size helpers, and the inner `build_*_xml` constructors used by the writer. |
//! | `types.rs` | Re-export surface so future sub-modules can `use crate::office::docx::types::WordDocument`. |
//! | `xml_parser.rs` (~1 070 lines) | `parse_document_xml` + the `RunFormat` parsing helpers + `attr_value_str` used inside the streaming reader. Holds *no* zip / writer state. |
//! | `table_parser.rs` (~270 lines) | Streaming `<w:tbl>` parser with the `RawCell` / `RawTable` / `VMergeKind` intermediates and `vMerge` resolution. |
//! | `reader.rs` (~100 lines) | Plain-text / markdown rendering (`word_document_to_text`). Pure string assembly over the public type tree — no XML or zip traffic. |
//!
//! ## Future splits to consider
//!
//! - `zip_writer.rs` — the `ImageWritePlan` / `HeaderFooterWritePlan` /
//!   `PreservedImageRef` machinery plus the `scan_preserved_*` state
//!   helpers account for ~600 lines and have no business state coupling;
//!   splitting them out leaves `mod.rs` focused on the public schema.
//! - `xml_writer.rs` — `escape_xml` + `build_run_xml` + `build_run_rpr_xml`
//!   + `build_field_run_xml` + `field_instr_text` + `build_document_xml`
//!   are all pure string constructors used only inside the writer; they
//!   would form a focused ~700-line file.

pub mod types;
pub(crate) mod table_parser;
pub(crate) mod xml_parser;
pub(crate) mod reader;

use serde::{Deserialize, Serialize};
use std::io::{Read, Write};

use super::shared::{OfficeError, read_zip_entry, TableCell, TableRow};
use table_parser::parse_table_xml;
use xml_parser::parse_document_xml;

// Re-export so callers can keep using `crate::office::docx::word_document_to_text`.
pub use reader::word_document_to_text;

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
    /// Character-level vertical alignment within the line, e.g. for inline
    /// footnotes or chinese-baseline shift. Maps to `<w:vertAlign>`.
    /// Common values: `"superscript"`, `"subscript"`, `""` (reset to baseline).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vert_align: Option<String>,
    /// Word field code (域代码) to render in place of `text`. When this is
    /// set, the run is emitted as a `<w:fldChar>` / `<w:instrText>` sequence
    /// (begin / separate / end) and `text` carries the cached result Word
    /// shows before the user presses F9 to refresh the field.
    ///
    /// Examples:
    /// - `FieldRef::Page` -> "第 1 页" (live page number)
    /// - `FieldRef::NumPages` -> total page count
    /// - `FieldRef::Date` -> today's date (formatted by `date_format`)
    /// - `FieldRef::Author` / `Title` -> doc properties
    /// - `FieldRef::Custom("DOCPROPERTY MyField")` -> any other instr text
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub field: Option<FieldRef>,
}

/// Word field code (域代码). When a `FontRun` carries one of these, the
/// writer emits a `<w:fldChar>` triplet instead of a plain `<w:r><w:t>` run.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum FieldRef {
    /// `PAGE` — current page number.
    Page,
    /// `NUMPAGES` — total page count in the document.
    NumPages,
    /// `SECTIONPAGES` — total page count in the current section.
    SectionPages,
    /// `SECTION` — current section number.
    Section,
    /// `DATE` — today's date. `format` follows Word's switches,
    /// e.g. `"yyyy-MM-dd"` (default), `"yyyy'年'M'月'd'日'"`.
    Date {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        format: Option<String>,
    },
    /// `TIME` — current time. `format` similar to Date.
    Time {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        format: Option<String>,
    },
    /// `AUTHOR` — document author from core properties.
    Author,
    /// `TITLE` — document title from core properties.
    Title,
    /// `FILENAME` — document filename. `with_ext` controls whether the
    /// extension is included (default `true`).
    Filename {
        #[serde(default = "default_true")]
        with_ext: bool,
    },
    /// Any other field expression not covered above. The `instr` string is
    /// inserted verbatim between `<w:instrText>` tags (without the
    /// surrounding braces; we emit those via the fldChar triplet).
    Custom {
        /// Raw instr text, e.g. `"DOCPROPERTY Department \\* MERGEFORMAT"`.
        instr: String,
    },
}

fn default_true() -> bool { true }

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
    /// Paragraph-level horizontal alignment. One of:
    /// `"left"`, `"right"`, `"center"`, `"both"` (justified), `"distribute"`.
    /// `None` means "use the style's default" (typically left for LTR locales).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alignment: Option<String>,
    /// Paragraph-level text direction. Overrides the section's default.
    /// - `"horizontal"` — left-to-right, top-to-bottom (default)
    /// - `"vertical"` / `"verticalRightToLeft"` — top-to-bottom, right-to-left
    ///   (traditional Chinese / Japanese vertical writing)
    /// - `"verticalLeftToRight"` — top-to-bottom, left-to-right
    /// - `"rotate90"` / `"rotate270"` — text rotated for landscape layout
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text_direction: Option<String>,
}

impl Default for WordParagraph {
    fn default() -> Self {
        WordParagraph {
            id: String::new(),
            text: String::new(),
            style: None,
            runs: None,
            numbering: None,
            alignment: None,
            text_direction: None,
        }
    }
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
        /// Paragraph alignment override. See `WordParagraph::alignment`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        alignment: Option<String>,
        /// Paragraph text direction override. See `WordParagraph::text_direction`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        text_direction: Option<String>,
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
    /// Tables, paragraphs, and images are interleaved by matching position
    /// markers to their backing entries in sequential order.
    ///
    /// Each `DocElement::Image` here carries the same id as the marker
    /// paragraph we *skip* in the paragraph loop below, so `modify()` can
    /// treat image elements as first-class participants in the
    /// delete/replace/insert pipeline. Without this, `modify()` would drop
    /// every pre-existing image on the first pass — see the multi-image
    /// insert regression test for the failure mode.
    pub fn to_elements(&self) -> Vec<DocElement> {
        // Build a map of table id -> table for O(1) lookup.
        let table_map: std::collections::HashMap<&str, &WordTable> =
            self.tables.iter().map(|t| (t.id.as_str(), t)).collect();

        // Build a map of image id -> image for O(1) lookup. The image's
        // stable id is matched against `<__img_pos_<id>__>` marker
        // paragraphs below.
        let image_map: std::collections::HashMap<&str, &WordImage> =
            self.images.iter().map(|i| (i.id.as_str(), i)).collect();

        // Tables already emitted via a marker paragraph (see loop below) are
        // recorded here so the final "append remaining tables" pass doesn't
        // double-count them.
        let mut tables_emitted: std::collections::HashSet<String> = std::collections::HashSet::new();
        // Same idea for images — a marker paragraph consumes one entry.
        let mut images_emitted: std::collections::HashSet<String> = std::collections::HashSet::new();

        let mut elements: Vec<DocElement> = Vec::with_capacity(self.paragraphs.len() + self.tables.len() + self.images.len());

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
            // Image marker paragraph: surface the underlying `WordImage` as
            // a first-class `DocElement::Image` so callers (notably
            // `modify()` and `read_office_file`'s elements payload) see
            // images alongside paragraphs and tables. The marker paragraph
            // itself is consumed here — the writer re-emits it via
            // `from_elements` (or the equivalent inline in `modify()`) so
            // no visible paragraph slot is lost.
            if let Some(rest) = p.text.strip_prefix("<__img_pos_") {
                if let Some(end) = rest.find("__>") {
                    let img_id = &rest[..end];
                    if let Some(img) = image_map.get(img_id) {
                        elements.push(DocElement::Image {
                            id: img.id.clone(),
                            position: elements.len(),
                            path: img.path.clone(),
                            width_emu: img.width_emu,
                            height_emu: img.height_emu,
                        });
                        images_emitted.insert(img.id.clone());
                        continue;
                    }
                    // Marker without a matching `WordImage` (e.g. the image
                    // entry got dropped by a buggy caller): fall through
                    // and emit the marker as a regular paragraph so we
                    // don't silently eat it. The writer won't render a
                    // drawing from it but at least the round-trip stays
                    // lossless.
                }
            }
            elements.push(DocElement::Paragraph {
                id: p.id.clone(),
                text: p.text.clone(),
                omit_text: false,
                style: p.style.clone(),
                runs: p.runs.clone(),
                numbering: p.numbering.clone(),
                alignment: p.alignment.clone(),
                text_direction: p.text_direction.clone(),
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
        // Same defensive pass for images. Today the writer always emits a
        // marker paragraph alongside a `WordImage`, but if a future
        // refactor relaxes that requirement we still don't want to lose
        // images that already lived in `self.images`.
        for img in &self.images {
            if !images_emitted.contains(img.id.as_str()) {
                elements.push(DocElement::Image {
                    id: img.id.clone(),
                    position: elements.len(),
                    path: img.path.clone(),
                    width_emu: img.width_emu,
                    height_emu: img.height_emu,
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
                DocElement::Paragraph { id, text, style, runs, numbering, alignment, text_direction, .. } => {
                    out_paras.push(WordParagraph { id, text, style, runs, numbering, alignment, text_direction });
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
                        alignment: None,
                        text_direction: None,
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
                        alignment: None,
                        text_direction: None,
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

        WordDocument {
            paragraphs: out_paras,
            tables,
            images,
            sections: Vec::new(),
            headers: Vec::new(),
            footers: Vec::new(),
        }
    }

    /// Modify the document by applying a list of edit operations.
    ///
    /// Bug fixes:
    /// - Bug 2: Fixed omit_text logic - when omit_text is false and text is provided, use the new text
    /// - Bug 3: Preserve original IDs by not calling from_elements which reassigns IDs
    /// - Bug 4: Support "before" position for anchor insertions
    /// - Bug 6: Support multiple elements each with their own anchor_id and position
    /// - Bug 7: Preserve pre-existing images across modify(). `to_elements()` now
    ///   surfaces images as `DocElement::Image` (rather than dropping them on the
    ///   floor) and the rebuild loop preserves each one's `internal_path` so
    ///   the writer can reuse the already-embedded media bytes. Without this
    ///   every call to `modify()` would orphan all previously inserted images
    ///   — see the `modify_preserves_existing_images` regression test.
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
                    (DocElement::Paragraph { id: _oi, text: ot, style: os, runs: ors, numbering: onum, alignment: oalign, text_direction: otdir, .. },
                     DocElement::Paragraph { id: ri, text: rt, style: rs, runs: rr, numbering: rnum, omit_text, alignment: ralign, text_direction: rtdir }) => {
                        // Merge strategy:
                        // 1. If runs provided in replacement -> use replacement runs (full override)
                        // 2. If text provided (omit_text=false) but no runs -> use text, clear runs
                        // 3. If nothing provided (omit_text=true, no runs) -> keep originals
                        
                        let merged_style = rs.or(os);
                        let merged_numbering = rnum.or(onum);
                        // `None` on the replacement side means "keep original" for
                        // alignment / text_direction (same omit-semantics as text).
                        let merged_alignment = ralign.or(oalign);
                        let merged_text_direction = rtdir.or(otdir);
                        
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
                            alignment: merged_alignment,
                            text_direction: merged_text_direction,
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
        // from_elements would reassign all IDs via marker paragraphs.
        //
        // Images need extra care: a DocElement::Image that came straight from
        // `self.to_elements()` (no `modifies` hit) is a pre-existing image and
        // should preserve its `internal_path` so the writer reuses the
        // already-embedded bytes from the preserved zip. A hit on `modify_map`
        // means the user is replacing the image: use the new path/width/height
        // and reset `internal_path` to None so the writer re-reads from disk.
        // A hit on `insert_elements` (no original by id) is a brand-new image
        // and naturally has `internal_path = None`.
        let mut out_paras: Vec<WordParagraph> = Vec::new();
        let mut tables: Vec<WordTable> = Vec::new();
        let mut images: Vec<WordImage> = Vec::new();

        // Snapshot the original images so we can look up `internal_path` for
        // unchanged entries. We only consult this map when the result element
        // is *not* in `modify_map`; replacements bypass it intentionally.
        let originals_by_id: std::collections::HashMap<&str, &WordImage> =
            self.images.iter().map(|i| (i.id.as_str(), i)).collect();

        for elem in result {
            match elem {
                DocElement::Paragraph { id, text, style, runs, numbering, alignment, text_direction, .. } => {
                    out_paras.push(WordParagraph { id, text, style, runs, numbering, alignment, text_direction });
                }
                DocElement::Table { id, position: _, header, rows } => {
                    // Emit a position marker whose text matches the table's ID.
                    out_paras.push(WordParagraph {
                        id: format!("__tbl_pos_{}__", id),
                        text: format!("<__tbl_pos_{}__>", id),
                        style: None,
                        runs: None,
                        numbering: None,
                        alignment: None,
                        text_direction: None,
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
                        alignment: None,
                        text_direction: None,
                    });
                    // Preserve `internal_path` for unchanged pre-existing
                    // images so the writer can reuse the bytes already
                    // embedded in the docx (and keep the original rId).
                    // Anything in `modify_map` is a replacement; anything
                    // not found in `originals_by_id` is a brand new image
                    // the user just appended — both reset `internal_path`
                    // so the writer allocates a fresh `imageN.ext`.
                    let internal_path = if modify_map.contains_key(&id) {
                        None
                    } else {
                        originals_by_id
                            .get(id.as_str())
                            .and_then(|orig| orig.internal_path.clone())
                    };
                    images.push(WordImage {
                        id,
                        path,
                        width_emu,
                        height_emu,
                        internal_path,
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
    /// Document sections. The first (and only) entry is the trailing
    /// `<w:sectPr>` injected at the end of the body. When the document
    /// has multiple sections, every entry *except the last* is embedded
    /// inside the `<w:pPr>` of the last paragraph of that section (this
    /// is the OOXML idiom for a "next-page section break"). A document
    /// with no entries falls back to a single A4-portrait section with
    /// 2.54 cm margins on all sides.
    #[serde(default)]
    pub sections: Vec<WordSection>,
    /// Reusable header parts referenced from sections via `HeaderPartRef`.
    /// Each part maps to one `word/headerN.xml` zip entry on write.
    #[serde(default)]
    pub headers: Vec<HeaderPart>,
    /// Reusable footer parts referenced from sections via `FooterPartRef`.
    /// Each part maps to one `word/footerN.xml` zip entry on write.
    #[serde(default)]
    pub footers: Vec<FooterPart>,
}

impl Default for WordDocument {
    fn default() -> Self {
        WordDocument {
            paragraphs: Vec::new(),
            tables: Vec::new(),
            images: Vec::new(),
            sections: Vec::new(),
            headers: Vec::new(),
            footers: Vec::new(),
        }
    }
}

/// One section of the document. Sections are delimited by `<w:sectPr>` —
/// every section *except the last* becomes a section break ("next page" by
/// default) embedded inside the `<w:pPr>` of the section's last paragraph;
/// the final section is the body-level `<w:sectPr>` injected before
/// `</w:body>`. This lets a single document mix e.g. a landscape cover
/// page with a portrait main body, or a horizontal chapter heading band
/// with a vertical body.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WordSection {
    /// Stable id (used by the tool layer for "modify section X" calls).
    pub id: String,
    /// Section type. `None` defaults to "next page", which is the most
    /// common (and what the user usually wants).
    /// - `"nextPage"` (default) — break and start a new page
    /// - `"continuous"` — no page break, just change properties
    /// - `"evenPage"` / `"oddPage"` — break to next even/odd page
    /// - `"nextColumn"` — break to next column (when `cols > 1`)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub section_type: Option<String>,
    /// Page size in twentieths of a point (twips). 1 inch = 1440 twips.
    /// The writer also accepts millimetre-friendly inputs via the
    /// `page_size_mm` alias below when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page_size_twips: Option<PageSize>,
    /// Same as `page_size_twips` but expressed in millimetres. When both
    /// are set, `page_size_twips` wins (twips are lossless).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page_size_mm: Option<PageSizeMm>,
    /// Page margins in twips. Fields default to reasonable A4 values when
    /// omitted (top 1440, right 1440, bottom 1440, left 1440 twips).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub margins: Option<PageMargins>,
    /// Page-level text direction (overrides the locale default).
    /// - `"horizontal"` (default) — left-to-right, top-to-bottom
    /// - `"verticalRightToLeft"` — top-to-bottom, right-to-left
    ///   (traditional Chinese / Japanese vertical)
    /// - `"verticalLeftToRight"` — top-to-bottom, left-to-right
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text_direction: Option<String>,
    /// When true, the first page of this section uses a different
    /// header/footer. Triggers a `<w:titlePg/>` in the sectPr; combined
    /// with `header_refs` / `footer_refs` that include a `First` entry,
    /// the cover page gets its own decoration.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub title_pg: bool,
    /// Number of text columns. `1` = single column (default). `> 1` =
    /// multi-column layout, with `<w:cols w:num="N" w:space="..."/>` in
    /// the sectPr.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cols: Option<u32>,
    /// Starting page number for this section. `None` = continue from
    /// previous section. `Some(1)` = restart at page 1.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page_num_start: Option<u32>,
    /// Page number format: `"decimal"` (default), `"upperRoman"`,
    /// `"lowerRoman"`, `"upperLetter"`, `"lowerLetter"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page_num_format: Option<String>,
    /// Header references for this section. Multiple entries are allowed —
    /// one per `<w:headerReference w:type="..."/>` in the sectPr. The
    /// `header_id` must match a `HeaderPart.id` in `WordDocument.headers`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub header_refs: Vec<HeaderPartRef>,
    /// Footer references for this section. Same shape as `header_refs`,
    /// but pointing at `WordDocument.footers`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub footer_refs: Vec<FooterPartRef>,
}

/// `<w:pgSz>` content. All measurements in twips (1 inch = 1440 twips,
/// 1 cm ≈ 567 twips, 1 mm ≈ 56.7 twips). For convenience the tool layer
/// also accepts `page_size_mm` (see `WordSection::page_size_mm`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PageSize {
    pub width: u32,
    pub height: u32,
    /// `"portrait"` (default) or `"landscape"`. When `"landscape"` the
    /// writer swaps width and height to satisfy OOXML's invariant
    /// `width <= height` even though Word treats the `orient` attribute
    /// as the source of truth.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub orient: Option<String>,
}

/// Page size expressed in millimetres. Converted to twips by the writer
/// (rounded to nearest twip — half-point granularity). Use this when the
/// user thinks in "A4" / "Letter" / "16K" rather than raw twips.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PageSizeMm {
    pub width: f32,
    pub height: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub orient: Option<String>,
}

impl PageSizeMm {
    /// Convert to twips. 1 mm = 56.6929 twips.
    pub fn to_twips(&self) -> PageSize {
        PageSize {
            width: (self.width * 56.6929) as u32,
            height: (self.height * 56.6929) as u32,
            orient: self.orient.clone(),
        }
    }
}

/// `<w:pgMar>` content. All measurements in twips. Defaults to a
/// "normal" 1-inch margin (1440 twips) on each side when omitted.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PageMargins {
    pub top: u32,
    pub right: u32,
    pub bottom: u32,
    pub left: u32,
    /// Distance from the top of the page to the header's bottom edge.
    /// Defaults to 720 (0.5 in).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub header: Option<u32>,
    /// Distance from the bottom of the page to the footer's top edge.
    /// Defaults to 720.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub footer: Option<u32>,
    /// Binding-edge gutter for duplex printing. Defaults to 0.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gutter: Option<u32>,
}

/// Reference to a header part from inside a section. The `header_id` is
/// the stable id of a `HeaderPart` in `WordDocument.headers`; the writer
/// resolves it to a `word/headerN.xml` zip entry and a rels rId.
///
/// `kind` maps to the `w:type` attribute on `<w:headerReference>`:
/// - `"default"` — every page in the section (when no `first`/`even` is set)
/// - `"first"`   — only the first page (requires `WordSection::title_pg = true`)
/// - `"even"`    — only even pages (requires `settings.xml.evenAndOddHeaders`)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HeaderPartRef {
    pub header_id: String,
    /// `"default" | "first" | "even"`. Defaults to `"default"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
}

/// Footer reference, same shape as `HeaderPartRef`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FooterPartRef {
    pub footer_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
}

/// A header part. Renders as a standalone `word/headerN.xml` file and
/// contains its own paragraphs, tables, and inline images (which become
/// the header's body — headers can carry small logos, chapter names,
/// page numbers, etc.).
///
/// Header content uses the same `WordParagraph` / `WordTable` /
/// `WordImage` model as the body, with a few caveats:
/// - Style names should target `Header` / `FirstHeader` / `EvenHeader`
///   (added to `STYLES_XML` alongside this feature).
/// - Field codes (e.g. `Page`) are common in headers and supported.
/// - Headers cannot have their own headers (no recursive sectioning).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeaderPart {
    pub id: String,
    #[serde(default)]
    pub paragraphs: Vec<WordParagraph>,
    #[serde(default)]
    pub tables: Vec<WordTable>,
    #[serde(default)]
    pub images: Vec<WordImage>,
}

/// Footer part. Same shape as `HeaderPart`. Common content: page
/// numbers, "X of Y", copyright, document filename, current date.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FooterPart {
    pub id: String,
    #[serde(default)]
    pub paragraphs: Vec<WordParagraph>,
    #[serde(default)]
    pub tables: Vec<WordTable>,
    #[serde(default)]
    pub images: Vec<WordImage>,
}

impl Default for WordSection {
    /// A4 portrait, 1-inch margins on all sides, single column, default
    /// header/footer refs. Used when `WordDocument.sections` is empty so
    /// the writer always has at least one `<w:sectPr>` to emit.
    fn default() -> Self {
        WordSection {
            id: "section-default".to_string(),
            section_type: None,
            page_size_twips: Some(PageSize {
                width: 11906,   // A4 width  (210 mm)
                height: 16838,  // A4 height (297 mm)
                orient: Some("portrait".to_string()),
            }),
            page_size_mm: None,
            margins: Some(PageMargins {
                top: 1440,
                right: 1440,
                bottom: 1440,
                left: 1440,
                header: Some(720),
                footer: Some(720),
                gutter: Some(0),
            }),
            text_direction: Some("horizontal".to_string()),
            title_pg: false,
            cols: Some(1),
            page_num_start: None,
            page_num_format: Some("decimal".to_string()),
            header_refs: Vec::new(),
            footer_refs: Vec::new(),
        }
    }
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
    let (mut paragraphs, image_markers, mut sections) = parse_document_xml(&doc_content)?;
    let images = parse_image_xml(&doc_content, &rels_content, &image_markers);
    // `image_markers` are synthetic paragraphs each carrying the image's
    // stable id as their `<inkuo:id>` so the writer can pair them with
    // `WordImage` entries during `<w:drawing>` emission.
    paragraphs.extend(image_markers);
    let tables = parse_table_xml(&doc_content)?;
    // Load header / footer parts from the zip. We scan every
    // `word/headerN.xml` / `word/footerN.xml` entry and parse each one
    // back into a `HeaderPart` / `FooterPart` so the writer can
    // re-emit them on save. References from sections (which carry
    // `rIdN` strings as `header_id`) are resolved to those parts below.
    let (headers, footers) = parse_header_footer_parts(bytes)?;
    // Resolve section -> header/footer rels: rels file maps rIdN to
    // the zip-internal path (`header2.xml`, `footer1.xml`, etc.).
    // We translate every section's ref into a `HeaderPart.id` /
    // `FooterPart.id` so the writer can look them up directly.
    resolve_section_refs(&mut sections, &rels_content, &headers, &footers);
    Ok(WordDocument {
        paragraphs,
        tables,
        images,
        sections,
        headers,
        footers,
    })
}

/// Walk every `word/headerN.xml` / `word/footerN.xml` zip entry and
/// parse each one into a `HeaderPart` / `FooterPart`. The returned
/// `HeaderPart.id` / `FooterPart.id` is the file's basename
/// (`header2`, `footer1`) so it's easy to correlate with the rId map
/// built from the rels file.
///
/// We also extract the part's EMU-stable rels id by reading
/// `word/_rels/document.xml.rels` so the writer can reuse existing
/// rIds when round-tripping — the rels id is stored in the part's
/// `internal_path` field (re-purposed: for header/footer parts we
/// stuff the rels id there as a "stable id" so the writer knows which
/// `rIdN` to reuse when constructing the new rels file).
fn parse_header_footer_parts(bytes: &[u8]) -> Result<(Vec<HeaderPart>, Vec<FooterPart>), OfficeError> {
    let mut headers = Vec::new();
    let mut footers = Vec::new();
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes))?;
    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        let name = file.name().to_string();
        if !name.starts_with("word/header") && !name.starts_with("word/footer") {
            continue;
        }
        if !name.ends_with(".xml") {
            continue;
        }
        let mut content = String::new();
        let _ = std::io::Read::read_to_string(&mut file.by_ref().take(8 * 1024 * 1024), &mut content);
        let (paras, image_markers, _sects) = parse_document_xml(&content)
            .map_err(|e| OfficeError::Xml(format!("Failed to parse {}: {}", name, e)))?;
        let id = std::path::Path::new(&name)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("?")
            .to_string();
        // Materialise a `WordDocument`-shaped struct for the part so the
        // writer can iterate paragraphs uniformly. Inline images inside
        // header/footer parts are a follow-up — the model round-trips
        // them as plain text for now, which is correct for the
        // overwhelmingly common case (page numbers, titles, dates).
        let mut all_paras = paras;
        all_paras.extend(image_markers);
        if name.starts_with("word/header") {
            headers.push(HeaderPart {
                id,
                paragraphs: all_paras,
                tables: Vec::new(),
                images: Vec::new(),
            });
        } else {
            footers.push(FooterPart {
                id,
                paragraphs: all_paras,
                tables: Vec::new(),
                images: Vec::new(),
            });
        }
    }
    Ok((headers, footers))
}

/// Walk the rels file once to build a `rId -> target_path` map, then
/// re-write each section's `header_refs` / `footer_refs` so the
/// `header_id` / `footer_id` is the *file basename* (e.g. `header2`)
/// of the matching part. The writer will resolve that to a
/// `HeaderPart` by id and re-use the original rId when minting fresh
/// rels.
fn resolve_section_refs(
    sections: &mut [WordSection],
    rels_content: &str,
    headers: &[HeaderPart],
    footers: &[FooterPart],
) {
    if rels_content.is_empty() {
        return;
    }
    // Build rId -> target map. The rels file format is
    // `<Relationship Id="rId6" Type="...header" Target="header2.xml"/>`.
    let mut rid_to_target: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    let mut reader = quick_xml::Reader::from_str(rels_content);
    reader.config_mut().trim_text(false);
    let mut buf = Vec::new();
    loop {
        let event = reader.read_event_into(&mut buf);
        match event {
            Ok(quick_xml::events::Event::Start(ref e)) | Ok(quick_xml::events::Event::Empty(ref e)) => {
                let name = e.local_name();
                if name.as_ref() == b"Relationship" {
                    let mut rid: Option<String> = None;
                    let mut target: Option<String> = None;
                    let mut is_header_or_footer = false;
                    for attr in e.attributes().with_checks(false).flatten() {
                        let key = attr.key.as_ref().to_vec();
                        let local = key
                            .iter()
                            .position(|&b| b == b':')
                            .map(|i| &key[i + 1..])
                            .unwrap_or(&key[..]);
                        let val = attr.value.as_ref();
                        if local == b"Id" {
                            if let Ok(s) = std::str::from_utf8(val) {
                                rid = Some(s.to_string());
                            }
                        } else if local == b"Type" {
                            if let Ok(s) = std::str::from_utf8(val) {
                                is_header_or_footer = s.contains("header") || s.contains("footer");
                            }
                        } else if local == b"Target" {
                            if let Ok(s) = std::str::from_utf8(val) {
                                target = Some(s.to_string());
                            }
                        }
                    }
                    if is_header_or_footer {
                        if let (Some(r), Some(t)) = (rid, target) {
                            rid_to_target.insert(r, t);
                        }
                    }
                }
            }
            Ok(quick_xml::events::Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
        buf.clear();
    }
    // Convert rId refs in sections to file basenames.
    for sect in sections.iter_mut() {
        for hr in sect.header_refs.iter_mut() {
            if let Some(target) = rid_to_target.get(&hr.header_id) {
                if let Some(stem) = std::path::Path::new(target)
                    .file_stem()
                    .and_then(|s| s.to_str())
                {
                    hr.header_id = stem.to_string();
                }
            }
        }
        for fr in sect.footer_refs.iter_mut() {
            if let Some(target) = rid_to_target.get(&fr.footer_id) {
                if let Some(stem) = std::path::Path::new(target)
                    .file_stem()
                    .and_then(|s| s.to_str())
                {
                    fr.footer_id = stem.to_string();
                }
            }
        }
    }
    // Defensive: if a section has a `header_id` / `footer_id` that
    // doesn't match any loaded part (e.g. the rels entry was missing),
    // drop the ref. The writer can re-allocate later if the user
    // provides a fresh header/footer.
    for sect in sections.iter_mut() {
        sect.header_refs.retain(|hr| headers.iter().any(|h| h.id == hr.header_id));
        sect.footer_refs.retain(|fr| footers.iter().any(|f| f.id == fr.footer_id));
    }
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
    let mut next_rid_u32: u32 = 5; // WORD_RELS_XML reserves rId1..rId5
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
        next_rid_u32 = next_rid + 1;

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

    // ── Header / Footer planning ───────────────────────────────────────────
    //
    // Collect every header and footer part from the model. We allocate a
    // sequential basename (header1, header2, footer1, ...) and a fresh
    // rId for each, matching the existing `scan_preserved_hf_state`
    // logic for round-trips. The actual XML bytes for each part are
    // generated on-demand (lazily) when we write the zip entry so we
    // don't allocate strings for parts we never write.
    let mut hf_writes: Vec<HeaderFooterWritePlan> = Vec::new();
    let mut header_index: u32 = 1;
    let mut footer_index: u32 = 1;
    let (preserved_max_rid, preserved_max_header_index, preserved_max_footer_index, preserved_hf_refs) =
        scan_preserved_hf_state(preserve_from).unwrap_or((5, 0, 0, vec![]));
    // Continue from the highest rId the image-planning pass already used,
    // so headers/footers don't collide with image rels (both share one
    // rId pool inside `word/_rels/document.xml.rels`). This is the same
    // counter pattern Word follows: rIds are global to the rels file.
    next_rid_u32 = next_rid_u32.max(preserved_max_rid) + 1;
    // Same idea for the basenames: when we're preserving an existing
    // docx that already has `word/header2.xml` etc., newly-allocated
    // headers/footers must start past that index so we don't try to
    // overwrite a zip entry that was just copied through from the
    // original archive.
    header_index = header_index.max(preserved_max_header_index + 1);
    footer_index = footer_index.max(preserved_max_footer_index + 1);

    // Build a preserved lookup: basename -> rid
    let preserved_by_basename: std::collections::HashMap<String, String> =
        preserved_hf_refs.into_iter().collect();

    for part in doc.headers.iter() {
        // Try to reuse an existing preserved rId if the basename matches.
        let rid = preserved_by_basename
            .get(&part.id)
            .cloned()
            .unwrap_or_else(|| {
                let r = format!("rId{}", next_rid_u32);
                next_rid_u32 += 1;
                r
            });
        let basename = if preserved_by_basename.contains_key(&part.id) {
            part.id.clone()
        } else {
            // Allocate a new sequential basename that won't collide.
            let b = format!("header{}", header_index);
            header_index += 1;
            b
        };
        hf_writes.push(HeaderFooterWritePlan {
            part: HeaderFooterPart::Header(part.clone()),
            part_id: part.id.clone(),
            basename: basename.clone(),
            internal_path: format!("word/{}.xml", basename),
            rid,
            is_header: true,
        });
    }
    for part in doc.footers.iter() {
        let rid = preserved_by_basename
            .get(&part.id)
            .cloned()
            .unwrap_or_else(|| {
                let r = format!("rId{}", next_rid_u32);
                next_rid_u32 += 1;
                r
            });
        let basename = if preserved_by_basename.contains_key(&part.id) {
            part.id.clone()
        } else {
            let b = format!("footer{}", footer_index);
            footer_index += 1;
            b
        };
        hf_writes.push(HeaderFooterWritePlan {
            part: HeaderFooterPart::Footer(part.clone()),
            part_id: part.id.clone(),
            basename: basename.clone(),
            internal_path: format!("word/{}.xml", basename),
            rid,
            is_header: false,
        });
    }

    // Collect the set of zip paths we will OVERWRITE later in the
    // hf_writes loop. In preserve mode, copying the original archive's
    // entries first would collide with these (zip forbids duplicate
    // filenames), so the preserve copy below has to skip them.
    let hf_overwrite_paths: std::collections::HashSet<String> = hf_writes
        .iter()
        .map(|p| p.internal_path.clone())
        .collect();

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
            // Header / footer parts the user asked us to (re-)write.
            // Skipping them here lets the hf_writes loop emit the new
            // content without colliding with the copy of the original
            // entry — see the `hf_overwrite_paths` set built above.
            if hf_overwrite_paths.contains(&name) {
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
    for plan in &hf_writes {
        zip.start_file(&plan.internal_path, opts)?;
        let hf_xml = build_header_footer_xml(&plan.part);
        zip.write_all(hf_xml.as_bytes())?;
    }

    // Build the post-image doc_xml so each placeholder can be substituted.
    let doc_xml_raw = build_document_xml(doc);
    let doc_xml = substitute_image_placeholders(&doc_xml_raw, &image_writes);
    let doc_xml = substitute_hf_placeholders(&doc_xml, &hf_writes);

    // Compose the final `[Content_Types].xml` and `word/_rels/document.xml.rels`
    // with image + header/footer Overrides / Relationships appended.
    let content_types = append_image_overrides(content_types_base, &image_writes);
    let content_types = append_hf_overrides(&content_types, &hf_writes);
    let word_rels = append_image_relationships(word_rels_base, &image_writes);
    let word_rels = append_hf_relationships(&word_rels, &hf_writes);

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

/// One header (or footer) part's worth of writer bookkeeping.
struct HeaderFooterWritePlan {
    /// The header or footer part from the model.
    part: HeaderFooterPart,
    /// e.g. `header1` or `footer2`.
    basename: String,
    /// e.g. `word/header1.xml`.
    internal_path: String,
    /// e.g. `rId6`.
    rid: String,
    /// The user-supplied `HeaderPart.id` (or `FooterPart.id`). This is the
    /// key the `substitute_hf_placeholders` pass uses to find
    /// `rIdHeaderPlaceholder_<id>` / `rIdFooterPlaceholder_<id>` in
    /// `document.xml` and swap in the real rId.
    part_id: String,
    /// Whether this is a header (false = footer).
    is_header: bool,
}

/// Either a header or a footer part, stored in the write plan.
#[derive(Debug, Clone)]
enum HeaderFooterPart {
    Header(HeaderPart),
    Footer(FooterPart),
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

// ─── Header / Footer part writing ─────────────────────────────────────────────

/// Build the XML content for a header or footer part. This is a
/// stripped-down `<w:hdr>` / `<w:ftr>` document — no `<w:body>` wrapper,
/// just the part root with paragraphs inside. Images are not supported
/// inside header/footer parts in v1 (the model still emits them as
/// plain text).
fn build_header_footer_xml(part: &HeaderFooterPart) -> String {
    let (paragraphs, is_header) = match part {
        HeaderFooterPart::Header(h) => (&h.paragraphs, true),
        HeaderFooterPart::Footer(f) => (&f.paragraphs, false),
    };
    let tag = if is_header { "hdr" } else { "ftr" };
    let mut xml = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:{tag} xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"
             xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"
             xmlns:inkuo="http://inkuo.app/wordprocessingml/2026/main">"#,
    );
    for para in paragraphs {
        if para.text.starts_with("<__tbl_pos_") || para.text.starts_with("<__img_pos_") {
            // Skip table/image markers inside header/footer parts — they
            // can't contain inline drawings and the body handles tables.
            continue;
        }
        xml.push_str("\n  <w:p>");
        // pPr
        let has_style = para.style.is_some();
        let has_alignment = para.alignment.is_some();
        let has_id = !para.id.is_empty();
        if has_style || has_alignment || has_id {
            xml.push_str("<w:pPr>");
            if let Some(ref s) = para.style {
                xml.push_str(&format!("<w:pStyle w:val=\"{}\"/>", escape_xml(s)));
            }
            if let Some(ref a) = para.alignment {
                if !a.is_empty() {
                    xml.push_str(&format!("<w:jc w:val=\"{}\"/>", escape_xml(a)));
                }
            }
            if has_id {
                xml.push_str(&format!("<inkuo:id w:val=\"{}\"/>", escape_xml(&para.id)));
            }
            xml.push_str("</w:pPr>");
        }
        // Runs
        if let Some(ref runs) = para.runs {
            for run in runs {
                xml.push_str(&build_run_xml(run));
            }
        } else if !para.text.is_empty() {
            xml.push_str(&format!(
                "<w:r><w:t xml:space=\"preserve\">{}</w:t></w:r>",
                escape_xml(&para.text)
            ));
        }
        xml.push_str("</w:p>");
    }
    xml.push_str(&format!("\n</w:{tag}>"));
    xml
}

/// Append `<Override>` rows for header and footer parts to
/// `[Content_Types].xml`.
fn append_hf_overrides(base: &str, plans: &[HeaderFooterWritePlan]) -> String {
    if plans.is_empty() {
        return base.to_string();
    }
    let close = "</Types>";
    let Some(pos) = base.rfind(close) else {
        return base.to_string();
    };
    let mut out = String::with_capacity(base.len() + plans.len() * 128);
    out.push_str(&base[..pos]);
    let tag = |plan: &HeaderFooterWritePlan| if plan.is_header { "header" } else { "footer" };
    for plan in plans {
        out.push_str(&format!(
            "  <Override PartName=\"/word/{}.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.{}+xml\"/>\n",
            escape_xml(&plan.basename),
            escape_xml(tag(plan)),
        ));
    }
    out.push_str(&base[pos..]);
    out
}

/// Append `<Relationship>` rows for header and footer parts to
/// `word/_rels/document.xml.rels`. The `Target` MUST include the
/// `.xml` extension — the corresponding zip entry is stored at
/// `word/<basename>.xml`, and OOXML requires `Target` to be the
/// part-relative path WITH the extension. Word/WPS silently drop
/// the relationship when the Target doesn't resolve, which is what
/// was producing "页眉页脚好像没有生效".
fn append_hf_relationships(base: &str, plans: &[HeaderFooterWritePlan]) -> String {
    if plans.is_empty() {
        return base.to_string();
    }
    let close = "</Relationships>";
    let Some(pos) = base.rfind(close) else {
        return base.to_string();
    };
    let mut out = String::with_capacity(base.len() + plans.len() * 256);
    out.push_str(&base[..pos]);
    for plan in plans {
        let rel_type = if plan.is_header {
            "http://schemas.openxmlformats.org/officeDocument/2006/relationships/header"
        } else {
            "http://schemas.openxmlformats.org/officeDocument/2006/relationships/footer"
        };
        out.push_str(&format!(
            "  <Relationship Id=\"{}\" Type=\"{}\" Target=\"{}.xml\"/>\n",
            escape_xml(&plan.rid),
            rel_type,
            escape_xml(&plan.basename),
        ));
    }
    out.push_str(&base[pos..]);
    out
}

/// Substitute the `rIdHeaderPlaceholder_<partid>` and
/// `rIdFooterPlaceholder_<partid>` tokens in doc_xml with the real
/// rIds minted by the writer. The mapping comes from `HeaderFooterWritePlan`.
fn substitute_hf_placeholders(
    doc_xml: &str,
    plans: &[HeaderFooterWritePlan],
) -> String {
    if plans.is_empty() {
        return doc_xml.to_string();
    }
    let mut out = doc_xml.to_string();
    for plan in plans {
        // Match the exact placeholder string emitted by `build_sectpr_xml`,
        // which uses `escape_xml(&hr.header_id)` (or footer_id) verbatim.
        // The writer substitutes the placeholders at the very end of write,
        // so we use a plain string replace on the same escaped form here.
        let placeholder = if plan.is_header {
            format!("rIdHeaderPlaceholder_{}", escape_xml(&plan.part_id))
        } else {
            format!("rIdFooterPlaceholder_{}", escape_xml(&plan.part_id))
        };
        out = out.replace(&placeholder, &plan.rid);
    }
    out
}

/// Scan the preserved zip's rels file for existing header / footer
/// relationships so we can reuse their rIds on round-trip. Returns
/// `(max_rid, preserved_hf_refs)` where each ref is a tuple of
/// `(rid, basename)` for the preserved relationship.
fn scan_preserved_hf_state(
    preserve_from: Option<&[u8]>,
) -> Result<(u32, u32, u32, Vec<(String, String)>), OfficeError> {
    let mut max_rid: u32 = 5;
    let mut max_header_index: u32 = 0;
    let mut max_footer_index: u32 = 0;
    let mut preserved: Vec<(String, String)> = Vec::new();
    if let Some(bytes) = preserve_from {
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes))?;
        let mut rels_xml: Option<String> = None;
        for i in 0..archive.len() {
            let mut file = archive.by_index(i)?;
            let name = file.name().to_string();
            if name == "word/_rels/document.xml.rels" {
                let mut s = String::new();
                let mut limited = (&mut file).take(1 << 20);
                let _ = std::io::Read::read_to_string(&mut limited, &mut s);
                rels_xml = Some(s);
            }
        }
        if let Some(s) = rels_xml {
            let bytes = s.as_bytes();
            let mut idx = 0;
            while idx + 8 < bytes.len() {
                if &bytes[idx..idx + 4] == b"Id=\"" && &bytes[idx + 4..idx + 7] == b"rId" {
                    let mut j = idx + 7;
                    let mut digits = String::new();
                    while j < bytes.len() && bytes[j].is_ascii_digit() {
                        digits.push(bytes[j] as char);
                        j += 1;
                    }
                    let next_id = find_next_relationship_id(&s, j);
                    let advance = if digits.is_empty() { idx + 1 } else { next_id };
                    if !digits.is_empty() {
                        if let Ok(n) = digits.parse::<u32>() {
                            if n > max_rid {
                                max_rid = n;
                            }
                        }
                        let window = &s[j..next_id];
                        if let Some((rid_str, target)) = extract_hf_target(window) {
                            // Strip the `.xml` extension so we can use the
                            // bare basename as a key for the planning
                            // lookup and also so we can recognise the
                            // numeric index (e.g. `header3` → 3).
                            let bare = target
                                .strip_suffix(".xml")
                                .unwrap_or(&target)
                                .to_string();
                            if let Some(n) = parse_hf_basename_index(&bare, true) {
                                if n > max_header_index {
                                    max_header_index = n;
                                }
                            }
                            if let Some(n) = parse_hf_basename_index(&bare, false) {
                                if n > max_footer_index {
                                    max_footer_index = n;
                                }
                            }
                            preserved.push((rid_str, bare));
                        }
                    }
                    idx = advance;
                } else {
                    idx += 1;
                }
            }
        }
    }
    Ok((max_rid, max_header_index, max_footer_index, preserved))
}

/// Given a basename like `header3` or `footer2`, return the trailing
/// integer index. `expect_header` picks which prefix to look for; the
/// other prefix is ignored so we don't accidentally double-count a
/// `header1` as a `footer1`.
fn parse_hf_basename_index(basename: &str, expect_header: bool) -> Option<u32> {
    let prefix = if expect_header { "header" } else { "footer" };
    let rest = basename.strip_prefix(prefix)?;
    rest.parse::<u32>().ok()
}

/// Within a Relationship element, extract the Target if it's a header/footer.
fn extract_hf_target(window: &str) -> Option<(String, String)> {
    // Look for Type containing "header" or "footer" and a Target attribute.
    let mut rid: Option<String> = None;
    let mut target: Option<String> = None;
    let mut is_hf = false;
    let bytes = window.as_bytes();
    let mut i = 0;
    while i + 8 < bytes.len() {
        if &bytes[i..i + 6] == b"Id=\"" {
            let mut j = i + 6;
            let mut v = Vec::new();
            while j < bytes.len() && bytes[j] != b'"' {
                v.push(bytes[j]);
                j += 1;
            }
            if let Ok(s) = String::from_utf8(v) {
                rid = Some(s);
            }
            i = j;
        } else if &bytes[i..i + 6] == b"Type=\"" {
            let mut j = i + 6;
            let mut v = Vec::new();
            while j < bytes.len() && bytes[j] != b'"' {
                v.push(bytes[j]);
                j += 1;
            }
            if let Ok(s) = String::from_utf8(v) {
                is_hf = s.contains("header") || s.contains("footer");
            }
            i = j;
        } else if &bytes[i..i + 8] == b"Target=\"" {
            let mut j = i + 8;
            let mut v = Vec::new();
            while j < bytes.len() && bytes[j] != b'"' {
                v.push(bytes[j]);
                j += 1;
            }
            if let Ok(s) = String::from_utf8(v) {
                target = Some(s);
            }
            i = j;
        } else {
            i += 1;
        }
    }
    if is_hf {
        rid.zip(target)
    } else {
        None
    }
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
    // Field runs render as a `<w:fldChar>` triplet plus a cached result
    // run. We don't emit the cached result inside the field (Word
    // refreshes it on F9), so the structure is: begin run, instrText
    // run, separate run, cached-text run, end run. All five share the
    // same formatting (bold, font, color, etc.) as the parent run.
    if let Some(ref field) = run.field {
        return build_field_run_xml(run, field);
    }

    let mut xml = String::from("<w:r>");
    let rpr = build_run_rpr_xml(run);
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

/// Render the `<w:rPr>` (run-property) XML for a `FontRun`, including
/// the new `vert_align` field. Used by both the regular run builder
/// and the field-run builder (fields inherit the run's formatting on
/// every run of the fldChar triplet).
pub fn build_run_rpr_xml(run: &FontRun) -> String {
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
        if !font.is_empty() {
            rpr.push_str(&format!("<w:rFonts w:ascii=\"{}\" w:hAnsi=\"{}\"/>", escape_xml(font), escape_xml(font)));
        }
    }
    if let Some(ref va) = run.vert_align {
        if !va.is_empty() {
            rpr.push_str(&format!("<w:vertAlign w:val=\"{}\"/>", escape_xml(va)));
        }
    }
    rpr
}

/// Render a Word field-code run as a 5-run fldChar triplet. The
/// `cached_text` (the visible value Word shows before F9 refresh) is
/// taken from `run.text`.
///
/// Word's required structure:
/// ```xml
/// <w:r><w:fldChar w:fldCharType="begin"/></w:r>
/// <w:r><w:instrText xml:space="preserve"> PAGE </w:instrText></w:r>
/// <w:r><w:fldChar w:fldCharType="separate"/></w:r>
/// <w:r><w:t>1</w:t></w:r>            ← cached result
/// <w:r><w:fldChar w:fldCharType="end"/></w:r>
/// ```
///
/// All five runs share the run's formatting. The `instr` text is
/// reconstructed from the `FieldRef` variant; see `field_instr_text`
/// for the inverse (parse-side) helper.
fn build_field_run_xml(run: &FontRun, field: &FieldRef) -> String {
    let rpr = build_run_rpr_xml(run);
    let rpr_open = if rpr.is_empty() {
        String::new()
    } else {
        format!("<w:rPr>{}</w:rPr>", rpr)
    };
    let instr = field_instr_text(field);
    let cached = if run.text.is_empty() { "".to_string() } else { run.text.clone() };
    format!(
        concat!(
            "<w:r>{rpr}<w:fldChar w:fldCharType=\"begin\"/></w:r>",
            "<w:r>{rpr}<w:instrText xml:space=\"preserve\">{instr}</w:instrText></w:r>",
            "<w:r>{rpr}<w:fldChar w:fldCharType=\"separate\"/></w:r>",
            "<w:r>{rpr}<w:t xml:space=\"preserve\">{cached}</w:t></w:r>",
            "<w:r>{rpr}<w:fldChar w:fldCharType=\"end\"/></w:r>"
        ),
        rpr = rpr_open,
        instr = escape_xml(&instr),
        cached = escape_xml(&cached),
    )
}

/// Reconstruct the raw `<w:instrText>` payload for a `FieldRef`. This
/// is the inverse of `parse_field_instr` and must stay in sync with
/// it. The leading space and trailing space match Word's own emit
/// style (` PAGE `) so the field is recognised on round-trip.
pub fn field_instr_text(field: &FieldRef) -> String {
    match field {
        FieldRef::Page => " PAGE ".to_string(),
        FieldRef::NumPages => " NUMPAGES ".to_string(),
        FieldRef::SectionPages => " SECTIONPAGES ".to_string(),
        FieldRef::Section => " SECTION ".to_string(),
        FieldRef::Date { format: Some(f) } => format!(" DATE \\@ \"{}\" ", f),
        FieldRef::Date { format: None } => " DATE ".to_string(),
        FieldRef::Time { format: Some(f) } => format!(" TIME \\@ \"{}\" ", f),
        FieldRef::Time { format: None } => " TIME ".to_string(),
        FieldRef::Author => " AUTHOR ".to_string(),
        FieldRef::Title => " TITLE ".to_string(),
        FieldRef::Filename { with_ext: true } => " FILENAME ".to_string(),
        FieldRef::Filename { with_ext: false } => " FILENAME \\p ".to_string(),
        FieldRef::Custom { instr } => format!(" {} ", instr),
    }
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

    // Resolve sections: the model carries an ordered list. We treat every
    // section except the last as "next-page break" and embed its `<w:sectPr>`
    // inside the `<w:pPr>` of the last paragraph of *that* section. The
    // final section's `<w:sectPr>` goes right before `</w:body>`.
    //
    // Without per-paragraph section assignment info we use a simple
    // position-based split: section 0 covers the first N0 paragraphs,
    // section 1 covers the next N1, etc. We default to "all paragraphs
    // belong to the last (and only) section" — the writer emits a
    // single body-level sectPr in that case. When there are multiple
    // sections we need to know how many paragraphs each one owns; the
    // tool layer is responsible for telling us via
    // `WordSection.id`-tagged marker paragraphs (one per section break).
    //
    // We look for marker paragraphs whose `id` is `__sect_break_<id>__`
    // and use those as section boundaries. Anything between two markers
    // (or before the first / after the last) belongs to the
    // corresponding section. A document with no markers and N sections
    // is treated as "all paragraphs in the last section, N-1 empty
    // leading sections" — pragmatic and matches what an unmodified
    // existing document looks like on read.
    let section_breaks: Vec<usize> = collect_section_breaks(&doc.paragraphs, doc.sections.len());
    let _total_sections = doc.sections.len().max(1);
    let sections: Vec<WordSection> = if doc.sections.is_empty() {
        vec![WordSection::default()]
    } else {
        doc.sections.clone()
    };
    // Make `total_sections` agree with `sections` (one section, one
    // element).
    let total_sections = sections.len();

    // Walk paragraphs, assigning each to a section index. The last
    // section is the trailing one (no marker; sectPr is body-level).
    // For N sections and M paragraphs without markers we put all M
    // paragraphs into the last section and emit empty (placeholder)
    // sectPrs at body-level for the rest. With markers we honour
    // them precisely.
    let mut para_section_idx: Vec<usize> = vec![total_sections - 1; doc.paragraphs.len()];
    if total_sections > 1 && section_breaks.len() + 1 == total_sections {
        let mut current_section = 0;
        for (i, p) in doc.paragraphs.iter().enumerate() {
            para_section_idx[i] = current_section;
            if section_breaks.contains(&i) {
                current_section = current_section.saturating_add(1).min(total_sections - 1);
            }
            let _ = p;
        }
    }

    // Iterate over paragraphs directly - markers contain position info
    for (idx, para) in doc.paragraphs.iter().enumerate() {
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

        // Section-break marker: a synthetic paragraph the model emits
        // to mark "this is the last paragraph of section N". We
        // promote it to a regular paragraph carrying the in-paragraph
        // `<w:sectPr>` of the closing section.
        if let Some(break_section_idx) = section_break_section_idx(para) {
            if break_section_idx < total_sections {
                xml.push_str("\n    <w:p>");
                let sect = &sections[break_section_idx];
                xml.push_str(&build_paragraph_ppr_xml(para, Some(sect)));
                xml.push_str("</w:p>");
                continue;
            }
        }

        // Regular paragraph - output as normal
        xml.push_str("\n    <w:p>");
        // Build paragraph properties: style (if any) + numbering (if any) + alignment + text direction + stable ID.
        // For the *last* paragraph of a non-final section we also embed
        // that section's `<w:sectPr>` here (the OOXML idiom for an
        // in-paragraph section break).
        let sect_idx = para_section_idx[idx];
        let is_last_para_of_section = if sect_idx + 1 < total_sections {
            // The next paragraph is the section-break marker for THIS
            // section, or this is the very last paragraph in the
            // section's range. We check the marker at idx+1.
            idx + 1 < doc.paragraphs.len()
                && section_break_section_idx(&doc.paragraphs[idx + 1]) == Some(sect_idx)
        } else {
            false
        };
        let embedded_sectpr = if is_last_para_of_section {
            Some(&sections[sect_idx])
        } else {
            None
        };
        xml.push_str(&build_paragraph_ppr_xml(para, embedded_sectpr));

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

    // Trailing body-level `<w:sectPr>` for the final section. If the
    // document has no sections the writer still emits a default A4
    // portrait sectPr so Word can open the file without complaint.
    let final_section = sections.last().cloned().unwrap_or_else(WordSection::default);
    xml.push_str(&build_body_sectpr_xml(&final_section));

    xml.push_str("\n  </w:body>\n</w:document>");
    xml
}

/// Build the `<w:pPr>` block for a paragraph, optionally with an
/// in-paragraph `<w:sectPr>` (used for section breaks). The `sect`
/// argument is `Some(_)` when this paragraph closes out a
/// non-trailing section.
fn build_paragraph_ppr_xml(para: &WordParagraph, sect: Option<&WordSection>) -> String {
    let has_alignment = para.alignment.is_some();
    let has_text_direction = para.text_direction.is_some();
    let has_sectpr = sect.is_some();
    let has_ppr = para.style.is_some()
        || para.numbering.is_some()
        || !para.id.is_empty()
        || has_alignment
        || has_text_direction
        || has_sectpr;
    if !has_ppr {
        return String::new();
    }
    let mut xml = String::from("<w:pPr>");
    if let Some(ref s) = para.style {
        xml.push_str(&format!("<w:pStyle w:val=\"{}\"/>", escape_xml(s)));
    }
    if let Some(ref num) = para.numbering {
        xml.push_str("<w:numPr>");
        xml.push_str(&format!("<w:ilvl w:val=\"{}\"/>", num.level));
        xml.push_str(&format!("<w:numId w:val=\"{}\"/>", num.num_id));
        xml.push_str("</w:numPr>");
    }
    if let Some(ref a) = para.alignment {
        if !a.is_empty() {
            xml.push_str(&format!("<w:jc w:val=\"{}\"/>", escape_xml(a)));
        }
    }
    if let Some(ref td) = para.text_direction {
        if !td.is_empty() {
            let v = emit_text_direction(td);
            xml.push_str(&format!("<w:textDirection w:val=\"{}\"/>", v));
        }
    }
    if let Some(s) = sect {
        xml.push_str(&build_sectpr_xml(s));
    }
    if !para.id.is_empty() {
        xml.push_str(&format!("<inkuo:id w:val=\"{}\"/>", escape_xml(&para.id)));
    }
    xml.push_str("</w:pPr>");
    xml
}

/// Build a body-level `<w:sectPr>` block (the trailing section
/// properties for the entire document). Header / footer references
/// carry a placeholder rId of the form `rIdHeaderPlaceholder_<partid>`
/// / `rIdFooterPlaceholder_<partid>`. The writer's
/// `substitute_header_footer_placeholders` pass rewrites them to the
/// real rIds once they're allocated.
fn build_body_sectpr_xml(sect: &WordSection) -> String {
    format!("\n    {}", build_sectpr_xml(sect))
}

/// Build a `<w:sectPr>` block (the inner contents — no surrounding
/// whitespace). Used both at body level and inside `<w:pPr>`.
fn build_sectpr_xml(sect: &WordSection) -> String {
    let mut xml = String::from("<w:sectPr>");
    // Header / footer references MUST come first inside sectPr (Word
    // is strict about element ordering). The placeholder rIds are
    // rewritten by the writer after rId allocation.
    for hr in &sect.header_refs {
        let kind = hr.kind.as_deref().unwrap_or("default");
        xml.push_str(&format!(
            "<w:headerReference r:id=\"rIdHeaderPlaceholder_{}\" w:type=\"{}\"/>",
            escape_xml(&hr.header_id),
            escape_xml(kind),
        ));
    }
    for fr in &sect.footer_refs {
        let kind = fr.kind.as_deref().unwrap_or("default");
        xml.push_str(&format!(
            "<w:footerReference r:id=\"rIdFooterPlaceholder_{}\" w:type=\"{}\"/>",
            escape_xml(&fr.footer_id),
            escape_xml(kind),
        ));
    }
    if let Some(ref st) = sect.section_type {
        if !st.is_empty() {
            xml.push_str(&format!("<w:type w:val=\"{}\"/>", escape_xml(st)));
        }
    }
    if let Some(ref ps) = sect.page_size_twips {
        let mut attrs = format!("w:w=\"{}\" w:h=\"{}\"", ps.width, ps.height);
        if let Some(ref o) = ps.orient {
            if !o.is_empty() {
                attrs.push_str(&format!(" w:orient=\"{}\"", escape_xml(o)));
            }
        }
        xml.push_str(&format!("<w:pgSz {}/>", attrs));
    }
    if let Some(ref m) = sect.margins {
        let mut attrs = format!(
            "w:top=\"{}\" w:right=\"{}\" w:bottom=\"{}\" w:left=\"{}\"",
            m.top, m.right, m.bottom, m.left
        );
        if let Some(h) = m.header {
            attrs.push_str(&format!(" w:header=\"{}\"", h));
        }
        if let Some(f) = m.footer {
            attrs.push_str(&format!(" w:footer=\"{}\"", f));
        }
        if let Some(g) = m.gutter {
            attrs.push_str(&format!(" w:gutter=\"{}\"", g));
        }
        xml.push_str(&format!("<w:pgMar {}/>", attrs));
    }
    // Section-level text direction (rare but valid). Writer writes
    // the OOXML vocabulary (`btLr` etc.) regardless of the input name
    // we use on the model side.
    if let Some(ref td) = sect.text_direction {
        if !td.is_empty() {
            let v = emit_text_direction(td);
            xml.push_str(&format!("<w:textDirection w:val=\"{}\"/>", v));
        }
    }
    if let Some(n) = sect.cols {
        if n > 1 {
            xml.push_str(&format!("<w:cols w:num=\"{}\" w:space=\"720\"/>", n));
        } else {
            xml.push_str("<w:cols w:space=\"720\"/>");
        }
    }
    if sect.title_pg {
        xml.push_str("<w:titlePg/>");
    }
    if let Some(start) = sect.page_num_start {
        let mut attrs = format!("w:start=\"{}\"", start);
        if let Some(ref fmt) = sect.page_num_format {
            if !fmt.is_empty() {
                attrs.push_str(&format!(" w:fmt=\"{}\"", escape_xml(fmt)));
            }
        }
        xml.push_str(&format!("<w:pgNumType {}/>", attrs));
    } else if let Some(ref fmt) = sect.page_num_format {
        if !fmt.is_empty() {
            xml.push_str(&format!("<w:pgNumType w:fmt=\"{}\"/>", escape_xml(fmt)));
        }
    }
    xml.push_str("</w:sectPr>");
    xml
}

/// Inverse of `normalise_text_direction` — turn the friendly
/// vocabulary the model uses back into OOXML's `w:textDirection`
/// attribute values. Unknown values pass through verbatim (Word
/// accepts `lrTb`, `tbRl`, `btLr`, `lrTbV`, `tbRlV`, `btLrV`).
fn emit_text_direction(v: &str) -> &str {
    match v {
        "horizontal" => "lrTb",
        "vertical" => "tbRlV",
        "verticalRightToLeft" => "btLr",
        "verticalLeftToRight" => "btLrV",
        "rotate90" => "lrTbV",
        "rotate270" => "lrTb",
        other => other,
    }
}

/// Find section-break marker paragraphs in the body. A marker is any
/// paragraph whose `id` looks like `__sect_break_<idx>__`. Returns
/// the index of each marker so the caller can compute which section
/// a given paragraph belongs to.
fn collect_section_breaks(paragraphs: &[WordParagraph], section_count: usize) -> Vec<usize> {
    if section_count <= 1 {
        return Vec::new();
    }
    let mut breaks = Vec::new();
    for (i, p) in paragraphs.iter().enumerate() {
        if section_break_section_idx(p).is_some() {
            breaks.push(i);
        }
    }
    breaks
}

/// If `p` is a section-break marker, return the section index it
/// closes (zero-based). Returns `None` for regular paragraphs.
fn section_break_section_idx(p: &WordParagraph) -> Option<usize> {
    if let Some(rest) = p.id.strip_prefix("__sect_break_") {
        if let Some(idx_str) = rest.strip_suffix("__") {
            if let Ok(n) = idx_str.parse::<usize>() {
                return Some(n);
            }
        }
    }
    None
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

  <w:style w:type="paragraph" w:styleId="Header">
    <w:name w:val="header"/>
    <w:basedOn w:val="Normal"/>
    <w:link w:val="HeaderChar"/>
    <w:pPr>
      <w:tabs>
        <w:tab w:val="center" w:pos="4680"/>
        <w:tab w:val="right" w:pos="9360"/>
      </w:tabs>
      <w:spacing w:after="0" w:line="240" w:lineRule="auto"/>
    </w:pPr>
    <w:rPr>
      <w:rFonts w:ascii="Calibri" w:hAnsi="Calibri"/>
      <w:sz w:val="20"/>
    </w:rPr>
  </w:style>

  <w:style w:type="character" w:styleId="HeaderChar" w:customStyle="1">
    <w:name w:val="Header Char"/>
    <w:basedOn w:val="DefaultParagraphFont"/>
    <w:link w:val="Header"/>
    <w:rPr>
      <w:rFonts w:ascii="Calibri" w:hAnsi="Calibri"/>
      <w:sz w:val="20"/>
    </w:rPr>
  </w:style>

  <w:style w:type="paragraph" w:styleId="Footer">
    <w:name w:val="footer"/>
    <w:basedOn w:val="Normal"/>
    <w:link w:val="FooterChar"/>
    <w:pPr>
      <w:tabs>
        <w:tab w:val="center" w:pos="4680"/>
        <w:tab w:val="right" w:pos="9360"/>
      </w:tabs>
      <w:spacing w:after="0" w:line="240" w:lineRule="auto"/>
    </w:pPr>
    <w:rPr>
      <w:rFonts w:ascii="Calibri" w:hAnsi="Calibri"/>
      <w:sz w:val="20"/>
    </w:rPr>
  </w:style>

  <w:style w:type="character" w:styleId="FooterChar" w:customStyle="1">
    <w:name w:val="Footer Char"/>
    <w:basedOn w:val="DefaultParagraphFont"/>
    <w:link w:val="Footer"/>
    <w:rPr>
      <w:rFonts w:ascii="Calibri" w:hAnsi="Calibri"/>
      <w:sz w:val="20"/>
    </w:rPr>
  </w:style>

  <w:style w:type="character" w:default="1" w:styleId="DefaultParagraphFont">
    <w:name w:val="Default Paragraph Font"/>
    <w:uiPriority w:val="1"/>
    <w:semiHidden/>
    <w:unhideWhenUsed/>
  </w:style>

  <w:style w:type="character" w:styleId="PageNumber">
    <w:name w:val="page number"/>
    <w:basedOn w:val="DefaultParagraphFont"/>
    <w:rPr/>
  </w:style>

  <w:style w:type="character" w:styleId="TotalPages">
    <w:name w:val="total pages"/>
    <w:basedOn w:val="DefaultParagraphFont"/>
    <w:rPr/>
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

