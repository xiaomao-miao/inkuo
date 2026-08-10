//! `CreateWordDocTool` — the largest tool in `agent/tools/office`.
//!
//! Owns:
//!   - All input structs (`DocTextRun`, `DocParagraph`, `NumberingInput`,
//!     `DocTable`, `DocSectionInput`, `DocPageSize`, `DocPageMargins`,
//!     `DocHeaderRef`, `DocFooterRef`, `DocHeaderPart`, `DocFooterPart`,
//!     `CreateWordDocParams`)
//!   - The `CreateWordDocTool` impl (new / definition / execute + all
//!     `to_font_run` / `parse_paragraph` / `parse_table` / `parse_image` /
//!     `convert_sections` / `convert_headers` / `convert_footers` helpers)
//!
//! Pulled out of `office/mod.rs` because the file had grown past 2000
//! lines and most of that weight was this one tool's input schemas.

use std::collections::HashMap;
use std::io::Read;

use serde::Deserialize;
use serde_json::Value;

use super::{ToolDefinition, ToolError, ToolParameters, validate_workspace_path};
use crate::office::ElementId;
use super::paragraph_columns::expand_paragraph_columns;

/// Output of `parse_component_block`. Carries the rendered paragraphs/tables
/// plus the optional positional metadata so the caller can integrate the
/// rendered pieces at the right anchor / order.
struct ComponentRender {
    rendered: crate::office::RenderedDocument,
    /// Anchor element id. Recorded for future per-paragraph insertion; today
    /// component blocks are append-only and this is intentionally unused.
    #[allow(dead_code)]
    anchor_id: Option<String>,
    /// Insertion position relative to `anchor_id`. Same caveat as `anchor_id`.
    #[allow(dead_code)]
    position: Option<String>,
    /// Per-paragraph column-wrap hint extracted from `columns` field on body
    /// component blocks. The writer's `expand_paragraph_columns` uses the id to
    /// locate the target paragraph and wraps it with continuous section breaks.
    column_wrap: Option<(String, u32)>,
}

/// A formatted text segment within a paragraph.
#[derive(Debug, Clone, Deserialize)]
struct DocTextRun {
    text: String,
    #[serde(default)]
    bold: Option<bool>,
    #[serde(default)]
    italic: Option<bool>,
    #[serde(default)]
    underline: Option<bool>,
    #[serde(default)]
    strikethrough: Option<bool>,
    #[serde(default)]
    font_size: Option<u32>,   // half-points, e.g. 24 = 12pt
    #[serde(default)]
    color: Option<String>,    // hex RGB, e.g. "FF0000"
    #[serde(default)]
    font_name: Option<String>,
    #[serde(default)]
    highlight: Option<String>,
    /// Character-level vertical alignment: `"superscript"`, `"subscript"`, or
    /// `null`/`""` for baseline.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    vert_align: Option<String>,
    /// When set, this run renders as a Word field code (域代码) instead of
    /// a plain text run. See `crate::office::FieldRef` for the shape.
    /// Common values: `{"kind": "page"}`, `{"kind": "numpages"}`,
    /// `{"kind": "date", "format": "yyyy-MM-dd"}`,
    /// `{"kind": "custom", "instr": "DOCPROPERTY MyField"}`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    field: Option<crate::office::FieldRef>,
}

/// A paragraph in the document.
#[derive(Debug, Deserialize)]
struct DocParagraph {
    /// Unique ID. If provided, replaces the existing element with this ID.
    /// If absent, creates a new element (appended or inserted).
    #[serde(default)]
    id: Option<String>,
    /// The paragraph text.
    text: String,
    /// Paragraph style: "Heading1" (large blue), "Heading2", "Heading3", "Normal".
    #[serde(default)]
    style: Option<String>,
    /// Rich text runs for inline formatting.
    #[serde(default)]
    runs: Option<Vec<DocTextRun>>,
    /// List/numbering reference: {num_id: u32, level: u32}.
    #[serde(default)]
    numbering: Option<NumberingInput>,
    /// Insert position relative to anchor_id: "before", "after", "end".
    /// Only used when id is absent (new element).
    #[serde(default)]
    #[allow(dead_code)] // accepted from JSON today; insertion logic not yet wired up
    position: Option<String>,
    /// Anchor element ID for insertion. Only used when id is absent.
    #[serde(default)]
    #[allow(dead_code)] // accepted from JSON today; insertion logic not yet wired up
    anchor_id: Option<String>,
    /// If true, delete the element with this id instead.
    #[serde(default, rename = "action")]
    delete_action: Option<String>,
    /// Paragraph alignment: "left" | "right" | "center" | "both" | "distribute".
    #[serde(default)]
    alignment: Option<String>,
    /// Paragraph text direction: "horizontal" | "vertical" |
    /// "verticalRightToLeft" | "verticalLeftToRight" | "rotate90" | "rotate270".
    #[serde(default)]
    text_direction: Option<String>,
    /// Number of text columns for this paragraph only. When set, the tool
    /// injects continuous section breaks around this paragraph so it (and only
    /// it) is laid out in N columns. The surrounding document stays single-column.
    /// Must be 2..=9. A value of 1 is silently ignored.
    #[serde(default)]
    columns: Option<u32>,
}

/// Same shape as `NumberingRef` but deserialized from the wire-format JSON.
#[derive(Debug, Clone, Deserialize)]
struct NumberingInput {
    num_id: u32,
    #[serde(default)]
    level: u32,
}

// ── Numbering conversion ────────────────────────────────────────────────────────

impl From<NumberingInput> for crate::office::NumberingRef {
    fn from(n: NumberingInput) -> Self {
        crate::office::NumberingRef { num_id: n.num_id, level: n.level }
    }
}

/// A table in the document.
#[derive(Debug, Deserialize)]
struct DocTable {
    /// Unique ID. If provided, replaces the existing table with this ID.
    #[serde(default)]
    id: Option<String>,
    /// Column header labels (becomes the first table row).
    header: Vec<String>,
    /// Data rows (each row is an array of cell values).
    rows: Vec<Vec<String>>,
    /// Insert position: "before", "after", "end".
    #[serde(default)]
    #[allow(dead_code)] // accepted from JSON today; insertion logic not yet wired up
    position: Option<String>,
    /// Anchor element ID for insertion.
    #[serde(default)]
    #[allow(dead_code)] // accepted from JSON today; insertion logic not yet wired up
    anchor_id: Option<String>,
    /// If true, delete this table instead.
    #[serde(default, rename = "action")]
    delete_action: Option<String>,
}

/// Top-level document sections. Each entry maps to a `<w:sectPr>` block.
#[derive(Debug, Deserialize)]
struct DocSectionInput {
    id: String,
    #[serde(default)]
    section_type: Option<String>,
    #[serde(default)]
    page_size_mm: Option<DocPageSizeMm>,
    #[serde(default)]
    page_size_twips: Option<DocPageSize>,
    #[serde(default)]
    margins: Option<DocPageMargins>,
    #[serde(default)]
    text_direction: Option<String>,
    #[serde(default)]
    title_pg: Option<bool>,
    #[serde(default)]
    cols: Option<u32>,
    #[serde(default)]
    page_num_start: Option<u32>,
    #[serde(default)]
    page_num_format: Option<String>,
    #[serde(default)]
    header_refs: Option<Vec<DocHeaderRef>>,
    #[serde(default)]
    footer_refs: Option<Vec<DocFooterRef>>,
}

#[derive(Debug, Deserialize)]
struct DocPageSize {
    width: u32,
    height: u32,
    #[serde(default)]
    orient: Option<String>,
}

#[derive(Debug, Deserialize)]
struct DocPageSizeMm {
    width: f32,
    height: f32,
    #[serde(default)]
    orient: Option<String>,
}

#[derive(Debug, Deserialize)]
struct DocPageMargins {
    top: u32,
    right: u32,
    bottom: u32,
    left: u32,
    #[serde(default)]
    header: Option<u32>,
    #[serde(default)]
    footer: Option<u32>,
    #[serde(default)]
    gutter: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct DocHeaderRef {
    header_id: String,
    #[serde(default)]
    kind: Option<String>,
}

#[derive(Debug, Deserialize)]
struct DocFooterRef {
    footer_id: String,
    #[serde(default)]
    kind: Option<String>,
}

/// A header part. Each entry becomes one `word/headerN.xml` file.
#[derive(Debug, Deserialize)]
struct DocHeaderPart {
    id: String,
    #[serde(default)]
    paragraphs: Vec<DocParagraph>,
}

/// A footer part. Each entry becomes one `word/footerN.xml` file.
#[derive(Debug, Deserialize)]
struct DocFooterPart {
    id: String,
    #[serde(default)]
    paragraphs: Vec<DocParagraph>,
}

#[derive(Debug, Deserialize)]
struct CreateWordDocParams {
    /// Absolute path of the .docx file to create or modify.
    path: String,
    /// Document title for newly created documents (ignored when modifying existing).
    #[serde(default)]
    title: Option<String>,
    /// Structured document elements (paragraphs and tables) for new content or modifications.
    /// - With `id`: replaces the existing element with that ID
    /// - Without `id` + with `anchor_id` + `position`: inserts at that position
    /// - Without `id` and `anchor_id`: appends to end
    #[serde(default)]
    elements: Option<Vec<serde_json::Value>>,
    /// IDs of elements to delete.
    #[serde(default)]
    #[allow(dead_code)] // accepted from JSON today; deletion-by-id is not yet implemented
    deletes: Option<Vec<String>>,
    /// Deprecated: use elements[]. Kept for backward compatibility.
    #[serde(default)]
    paragraphs: Option<Vec<DocParagraph>>,
    /// Deprecated: use elements[]. Kept for backward compatibility.
    #[serde(default)]
    tables: Option<Vec<DocTable>>,
    /// Deprecated: use elements[]. Path to an existing .docx to append content to.
    #[serde(default)]
    append_to: Option<String>,
    /// When true, the content in `elements[]` is appended to the end of the existing
    /// document without reading/modifying its current structure. Useful for progressive
    /// document building — call repeatedly as you generate content section by section.
    /// Takes effect only when the file already exists.
    #[serde(default)]
    append: Option<bool>,
    /// Document sections. Each entry maps to one `<w:sectPr>` block at
    /// write time. Sections partition the document; the first (and
    /// usually only) entry is the trailing sectPr, additional entries
    /// inject a "next page" break before them. Required keys per entry:
    /// `id`. All others are optional and have sensible defaults.
    #[serde(default)]
    sections: Option<Vec<DocSectionInput>>,
    /// Reusable header parts. Each entry becomes one `word/headerN.xml`
    /// file and can be referenced from one or more sections via
    /// `sections[].header_refs[]`.
    #[serde(default)]
    headers: Option<Vec<DocHeaderPart>>,
    /// Reusable footer parts. Each entry becomes one `word/footerN.xml`
    /// file. Common contents: page numbers, total pages, dates.
    #[serde(default)]
    footers: Option<Vec<DocFooterPart>>,
}

pub struct CreateWordDocTool;

// ── Tool implementation ─────────────────────────────────────────────────────────

impl CreateWordDocTool {
    pub fn new() -> Self { Self }
    pub fn definition(&self) -> ToolDefinition {
            ToolDefinition::new_with_label(
                "create_word_doc",
                "创建 Word 文档",
                "Create, modify, or append a Word (.docx) document. **IMPORTANT: every call must include the full absolute `path` — including repeated append calls.** The backend does not remember the path between calls. Pass elements[] with paragraph and table objects. Use IDs to modify existing content; omit IDs to append new content. Use anchor_id + position to insert at a specific location.",
            ToolParameters::new(
                vec!["path"],
                vec![
                    ("path", "string", Some("**Required on every call, including append calls.** Absolute path of the .docx file to create or modify. Example: \"/Users/me/docs/report.docx\". Do not omit this field even when you are just appending more content with `append: true`.")),
                    ("title", "string", Some("Document title (for new files only; ignored when modifying existing)")),
                    ("elements", "array", Some(
                        "Array of element objects. Each element is a structured block the agent builds. The element types are split into `low-level` (precise paragraph/table/image control) and `component` (brand-styled, design-system aware).\n\
                         \n\
                         === LOW-LEVEL ELEMENTS ===\n\
                         Paragraph: {id?, text?, style?, runs?, position?, anchor_id?, alignment?, text_direction?, columns?}.\n\
                         Table: {id?, header, rows, position?, anchor_id?}. Cells in header/rows can be plain strings or {text, col_span, row_span} objects.\n\
                         Image: {type:'image', id?, path, width_emu, height_emu, anchor_id?, position?}.\n\
                         \n\
                         **columns (Paragraph only)**: Set this to 2..9 to lay out ONLY this single paragraph (and any paragraphs that immediately follow it in the same section) in N columns. The tool injects continuous section breaks around it so the rest of the document stays single-column. Use this instead of `sections[].cols` when you only want part of the document multi-column.\n\
                         \n\
                         === COMPONENT ELEMENTS (design-system styled) ===\n\
                         Cover: {type:'cover', id?, title, subtitle?}. Emits an oversized centred cover title + subtitle + spacers. Default brand font sizes apply. Use once at the top of a new document.\n\
                         Chapter: {type:'chapter', id?, title}. Emits a chapter-title paragraph (ChapterTitle style).\n\
                         Heading: {type:'heading', id?, level: 1|2|3, text}. Emits Heading1/2/3 (mapped to ChapterTitle/SectionTitle/SubsectionTitle styles).\n\
                         Body: {type:'body', id?, text, columns?} or {type:'body', id?, runs: [{text, bold?, italic?}, ...], columns?}. Emits a BodyParagraph paragraph. The `columns` field (2..9) scopes a multi-column layout to this single body paragraph only — the surrounding document stays single-column.\n\
                         BulletList: {type:'bullet_list', id_prefix, items: [string, ...]}. Emits one bulleted paragraph per item using the design-system numbering (num_id=1).\n\
                         OrderedList: {type:'ordered_list', id_prefix, items: [string, ...]}. Emits one ordered paragraph per item (num_id=2).\n\
                         StyledTable: {type:'styled_table', id?, headers: [string, ...], rows: [[string, ...], ...], style?: {header_fill?, zebra_fill?, border_color?, header_text_color?, repeat_header?, zebra?}}. Emits a table with brand colours + header-repeat + zebra striping. style fields are optional; sensible defaults come from the active palette.\n\
                         Callout: {type:'callout', id?, level: 'info'|'warning'|'important'|'tip', title, body?, body_lines?: [string, ...]}. Emits an icon + title + body callout with level-matching background/accent colours. Use body for single-line, body_lines for multi-line.\n\
                         Code: {type:'code', id?, lines: [string, ...], language?}. Emits a monospace code block with a uniform background and an optional language label.\n\
                         PageBreak: {type:'page_break', id?}. Emits a hard page break (force-chapter use).\n\
                         \n\
                         === INSERTION SEMANTICS ===\n\
                         Elements with id replace existing ones (low-level only; component blocks are append-only). Omit id (and omit id_prefix for lists) to append new content. Without anchor_id, content is appended at the end. With anchor_id, insertion is positioned relative to that anchor via position: 'before'|'after' (default 'after').\n\
                         Component blocks (cover/chapter/heading/body/lists/styled_table/callout/code/page_break) are append-only — they emit a self-contained batch of paragraphs/tables that the tool appends at the end of the document (or after the last anchor_id-pointed element when supplied). Per-paragraph positioning is not supported for component blocks; use a low-level Paragraph element if you need it.\n\
                         When modifying (id present), omit 'text' field to preserve original text. Providing 'text' field will update the paragraph text.\n\
                         Omit 'runs' to keep original formatting, or provide 'runs' array to fully replace paragraph formatting.\n\
                         runs shape (low-level runs): array of {text, bold?, italic?, underline?, font_size? (half-points, e.g. 24=12pt), color? (hex RGB, e.g. 'FF0000'), font_name?, highlight?, vert_align?, field?}.\n\
                         alignment: 'left' | 'right' | 'center' | 'both' | 'distribute'.\n\
                         text_direction: 'horizontal' | 'vertical' | 'verticalRightToLeft' | 'verticalLeftToRight' | 'rotate90' | 'rotate270'.\n\
                         vert_align: 'superscript' | 'subscript' on a run.\n\
                         field: {kind: 'page' | 'numpages' | 'date' | 'time' | 'author' | 'title' | 'custom', format?: '<format-string>', instr?: '<raw field instr>'} for a Word field code. When set, the run renders as a live field instead of plain text (e.g. page number, current date).\n\
                         position can be 'before' or 'after' (default) to control where new elements are inserted relative to anchor_id.\n\
                         Images: `path` must be an absolute local path to a png/jpeg/jpg/gif file; `width_emu`/`height_emu` are in EMU (914400=1in, 360000=1cm). Only inline insertion is supported in v1."
                    )),
                    ("deletes", "array", Some("Array of element IDs to delete. Works alongside elements[] with action:'delete'.")),
                    ("sections", "array", Some(
                        "Top-level document sections. Each entry maps to one `<w:sectPr>` block.\n\
                         Shape: {id (required), section_type?, page_size_mm?, page_size_twips?, margins?, text_direction?, title_pg?, cols?, page_num_start?, page_num_format?, header_refs?, footer_refs?}.\n\
                         - section_type: 'nextPage' (default) | 'continuous' | 'evenPage' | 'oddPage' | 'nextColumn'.\n\
                         - page_size_mm: {width, height, orient?} (orient: 'portrait' | 'landscape'). E.g. {width:210, height:297} for A4 portrait.\n\
                         - page_size_twips: {width, height, orient?} (1 inch = 1440 twips).\n\
                         - margins: {top, right, bottom, left, header?, footer?, gutter?}. Twips.\n\
                         - text_direction: 'horizontal' (default) | 'verticalRightToLeft' | 'verticalLeftToRight'.\n\
                         - title_pg: true to give the first page of the section a different header/footer (cover page).\n\
                         - cols: number of text columns for THIS ENTIRE SECTION. 1 = single column. >1 = multi-column. **WARNING: setting cols>1 on the only/last section will make the ENTIRE document multi-column** — there is no \"apply to just this paragraph\" primitive in Word. Use `columns` on individual paragraph elements instead for partial-column effects.\n\
                         - page_num_start: starting page number (omit to continue from previous section).\n\
                         - page_num_format: 'decimal' (default) | 'upperRoman' | 'lowerRoman' | 'upperLetter' | 'lowerLetter'.\n\
                         - header_refs: array of {header_id, kind?} where kind is 'default' (default) | 'first' | 'even'.\n\
                         - footer_refs: array of {footer_id, kind?} with the same kind values.\n\
                         For multi-section docs (e.g. cover page in landscape + body in portrait vertical), list each section in order; the LAST section's sectPr is the trailing one in the body, the rest are embedded as section breaks at the end of their section's content. For partial-column effects, use `columns` on individual paragraph elements instead of `sections[].cols`."
                    )),
                    ("headers", "array", Some(
                        "Reusable header parts. Each entry becomes one `word/headerN.xml` file. Shape: {id, paragraphs: [...]}. paragraphs uses the same shape as elements[] paragraphs. Common contents: chapter title, page number (with runs:[{text:'', field:{kind:'page'}}]), date. Reference from sections via `sections[].header_refs[]`."
                    )),
                    ("footers", "array", Some(
                        "Reusable footer parts. Each entry becomes one `word/footerN.xml` file. Shape: {id, paragraphs: [...]}. Common contents: 'Page X of Y' (with field:{kind:'page'} and field:{kind:'numpages'} runs), copyright, signature line. Reference from sections via `sections[].footer_refs[]`."
                    )),
                ],
            ),
        )
    }

    fn to_font_run(r: DocTextRun) -> crate::office::FontRun {
        crate::office::FontRun {
            text: r.text,
            bold: r.bold.unwrap_or(false),
            italic: r.italic.unwrap_or(false),
            underline: r.underline.unwrap_or(false),
            strikethrough: r.strikethrough.unwrap_or(false),
            font_size: r.font_size,
            color: r.color,
            font_name: r.font_name,
            highlight: r.highlight,
            vert_align: r.vert_align,
            field: r.field,
            page_break: false,
        }
    }

    /// Parse a low-level paragraph element from raw JSON.
    fn parse_paragraph(
        v: &serde_json::Value,
    ) -> Result<Option<crate::office::DocElement>, String> {
        if v["action"].as_str() == Some("delete") {
            if let Some(id) = v["id"].as_str() {
                return Ok(Some(crate::office::DocElement::Paragraph {
                    id: id.to_string(),
                    text: String::new(),
                    omit_text: false,
                    style: None,
                    runs: None,
                    numbering: None,
                    alignment: None,
                    text_direction: None,
                }));
            }
            return Err("delete action requires an id".to_string());
        }

        let id = v["id"].as_str().map(|s| s.to_string());

        // The `text` field is optional when modifying an existing paragraph
        // (id is set). Omitting it tells the backend to keep the original
        // text. We record that intent via `omit_text` so `WordDocument::modify`
        // can do the right merge.
        let has_text_key = v.as_object().map(|o| o.contains_key("text")).unwrap_or(false);
        let text = v["text"]
            .as_str()
            .map(|s| s.to_string())
            .unwrap_or_default();
        let omit_text = !has_text_key;

        let style = v["style"].as_str().map(|s| s.to_string());

        let has_runs_key = v.as_object().map(|o| o.contains_key("runs")).unwrap_or(false);
        let runs: Option<Vec<_>> = if has_runs_key {
            v["runs"].as_array().map(|arr| {
                arr.iter().filter_map(|r| {
                    let text = r["text"].as_str().unwrap_or("").to_string();
                    if text.is_empty() { return None; }
                    // `field` and `vert_align` round-trip via serde because
                    // `FontRun` is `Serialize + Deserialize`. The other
                    // booleans are cheap to extract by hand.
                    let vert_align = r["vert_align"].as_str().map(|s| s.to_string());
                    let field: Option<crate::office::FieldRef> = r
                        .get("field")
                        .and_then(|f| serde_json::from_value(f.clone()).ok());
                    Some(crate::office::FontRun {
                        text,
                        bold: r["bold"].as_bool().unwrap_or(false),
                        italic: r["italic"].as_bool().unwrap_or(false),
                        underline: r["underline"].as_bool().unwrap_or(false),
                        strikethrough: r["strikethrough"].as_bool().unwrap_or(false),
                        font_size: r["font_size"].as_u64().map(|n| n as u32),
                        color: r["color"].as_str().map(|s| s.to_string()),
                        font_name: r["font_name"].as_str().map(|s| s.to_string()),
                        highlight: r["highlight"].as_str().map(|s| s.to_string()),
                        vert_align,
                        field,
                        page_break: false,
                    })
                }).collect()
            })
        } else {
            None
        };

        let numbering: Option<crate::office::NumberingRef> = v["numbering"].as_object().and_then(|obj| {
            let num_id = obj.get("num_id")?.as_u64()? as u32;
            let level = obj.get("level").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
            Some(crate::office::NumberingRef { num_id, level })
        });

        let alignment = v["alignment"].as_str().map(|s| s.to_string());
        let text_direction = v["text_direction"].as_str().map(|s| s.to_string());

        Ok(Some(crate::office::DocElement::Paragraph {
            id: id.unwrap_or_else(|| format!("__new_p{}", uuid_simple())),
            text,
            omit_text,
            style,
            runs,
            numbering,
            alignment,
            text_direction,
        }))
    }

    fn parse_table(v: &serde_json::Value) -> Result<Option<crate::office::DocElement>, String> {
        if v["action"].as_str() == Some("delete") {
            if let Some(id) = v["id"].as_str() {
                return Ok(Some(crate::office::DocElement::Table {
                    id: id.to_string(),
                    position: 0,
                    header: vec![],
                    rows: vec![],
                }));
            }
            return Err("delete action requires an id".to_string());
        }

        let id = v["id"].as_str().map(|s| s.to_string());

        // Header / rows are arrays of cells. For backwards compatibility we
        // accept both bare strings ("A") and objects with span info
        // ({"text": "A", "col_span": 2, "row_span": 1}). The custom
        // `Deserialize` impl on `TableCell` handles both shapes uniformly.
        let parse_cells = |arr: &serde_json::Value| -> Vec<crate::office::TableCell> {
            arr.as_array()
                .map(|items| {
                    items
                        .iter()
                        .filter_map(|c| serde_json::from_value::<crate::office::TableCell>(c.clone()).ok())
                        .collect()
                })
                .unwrap_or_default()
        };
        let header = parse_cells(&v["header"]);
        let rows: Vec<Vec<crate::office::TableCell>> = v["rows"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .map(|r| parse_cells(r))
                    .collect()
            })
            .unwrap_or_default();

        Ok(Some(crate::office::DocElement::Table {
            id: id.unwrap_or_else(|| format!("__new_t{}", uuid_simple())),
            position: 0,
            header,
            rows,
        }))
    }

    /// Parse an `{type: "image", ...}` element.
    ///
    /// Required: `path` (absolute path on disk to png/jpeg/gif),
    /// `width_emu`, `height_emu`. Optional: `id` (defaults to a fresh
    /// uuid), `anchor_id`, `position`.
    fn parse_image(v: &serde_json::Value) -> Result<Option<crate::office::DocElement>, String> {
        if v["action"].as_str() == Some("delete") {
            return Err("delete action is not supported for image elements; use office_word_expert to remove them".to_string());
        }

        let path = v["path"]
            .as_str()
            .ok_or_else(|| "image element requires `path`".to_string())?;
        if path.is_empty() {
            return Err("image element requires non-empty `path`".to_string());
        }
        let width_emu = v["width_emu"]
            .as_u64()
            .ok_or_else(|| "image element requires `width_emu` (integer EMU, 914400=1in)".to_string())?
            as u32;
        let height_emu = v["height_emu"]
            .as_u64()
            .ok_or_else(|| "image element requires `height_emu` (integer EMU, 914400=1in)".to_string())?
            as u32;
        if width_emu == 0 || height_emu == 0 {
            return Err("image element requires non-zero width_emu and height_emu".to_string());
        }
        // Validate the file extension up-front so the writer doesn't have
        // to surface a half-broken docx; the user gets a clear "fix your
        // payload" message instead.
        let ext = std::path::Path::new(path)
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        match ext.as_str() {
            "png" | "jpeg" | "jpg" | "gif" => {}
            other => {
                return Err(format!(
                    "Unsupported image extension '.{}'; supported: png, jpeg, jpg, gif",
                    other
                ));
            }
        }

        let id = v["id"].as_str().map(|s| s.to_string());

        Ok(Some(crate::office::DocElement::Image {
            id: id.unwrap_or_else(|| format!("__new_i{}", uuid_simple())),
            position: 0,
            path: path.to_string(),
            width_emu,
            height_emu,
        }))
    }

    // ── Component block parser ───────────────────────────────────────────────────
    //
    // The design-system component blocks (cover/chapter/heading/body/bullet_list/
    // ordered_list/styled_table/callout/code/page_break) all share a common
    // shape: a JSON body that `ContentBlock` can deserialise directly, plus an
    // optional `anchor_id`/`position` pair carried alongside. The parser below
    // splits the raw `Value` into:
    //   - the `ContentBlock` payload (deletes any tool-level `anchor_id`/`position`
    //     before parsing so the inner deserialiser doesn't choke),
    //   - the positional metadata,
    //   - the rendered `RenderedDocument` (paragraphs + tables + images).
    //
    // The caller (execute) is then responsible for integrating the rendered
    // pieces into the existing document pipeline at the right anchor / order.
    //
    // Returns `Ok(None)` for `type` values that fall through to the legacy
    // paragraph/table/image parsers so the calling loop can degrade gracefully.

    fn parse_component_block(
        v: &serde_json::Value,
    ) -> Result<Option<ComponentRender>, String> {
        // Distinguish three cases:
        //   1. No `type` field at all → legacy element (let caller handle).
        //   2. `type` is a known low-level tag (paragraph/table/image) →
        //      legacy element (let caller handle).
        //   3. `type` is a known component tag → render it here.
        //   4. `type` is anything else → error (the AI was confused).
        let elem_type = match v["type"].as_str() {
            Some(t) => t,
            None => return Ok(None),
        };
        let component_type = match elem_type {
            "cover" | "chapter" | "heading" | "body" | "bullet_list" | "ordered_list"
            | "styled_table" | "callout" | "code" | "page_break" => elem_type,
            // Legacy element types — let the caller degrade.
            "paragraph" | "table" | "image" => return Ok(None),
            _ => {
                return Err(format!(
                    "Unknown element type '{}'. Valid types: paragraph, table, image, cover, \
                     chapter, heading, body, bullet_list, ordered_list, styled_table, callout, \
                     code, page_break.",
                    elem_type
                ));
            }
        };

        // Strip the tool-level positional fields before deserialising into
        // ContentBlock — the inner schema doesn't know about them, and serde
        // would reject them otherwise.
        let mut payload = v.clone();
        if let Some(obj) = payload.as_object_mut() {
            obj.remove("anchor_id");
            obj.remove("position");
            // Also strip `columns` — we handle it separately in the caller
            // so it can be injected into the expansion pass regardless of
            // whether the paragraph is low-level or a component block.
            obj.remove("columns");
        }

        // Backwards-compat: `styled_table` accepts a {header, rows} shape that
        // drops the `type` tag. We accept this convenience form (without an
        // explicit `type: 'styled_table'`) by re-adding the tag here.
        if component_type == "styled_table" {
            if let Some(obj) = payload.as_object_mut() {
                if obj.get("type").is_none() {
                    obj.insert("type".to_string(), Value::String("styled_table".to_string()));
                }
                // Rename legacy string-array rows into Vec<Vec<String>> if the
                // caller used the old format with plain strings — handled by
                // ContentTableStyle shape already.
                if let Some(headers) = obj.get("headers").cloned() {
                    if headers.is_array() {
                        let _ = headers;
                    }
                }
            }
        }

        // Backwards-compat: `callout` accepts `body` (single-line) or
        // `body_lines` (multi-line). Reflect that in default schema.
        let block: crate::office::ContentBlock = serde_json::from_value(payload)
            .map_err(|e| format!("Invalid `{}` element: {}", component_type, e))?;

        let mut tokens = crate::office::DesignTokens::default();
        let style_override = v["style"].clone();
        if let Some(style) = style_override.as_object() {
            if let Some(p_palette) = style.get("palette").and_then(|v| v.as_object()) {
                if let Some(p) = p_palette.get("primary").and_then(|v| v.as_str()) {
                    tokens.palette.primary = p.to_string();
                }
            }
        }

        let rendered = crate::office::render_blocks(&[block], &tokens);

        Ok(Some(ComponentRender {
            rendered,
            anchor_id: v["anchor_id"].as_str().map(|s| s.to_string()),
            position: v["position"].as_str().map(|s| s.to_string()),
            // Extract `columns: N` from the raw JSON. This is a hint that the
            // caller wants this paragraph rendered in N-column layout.
            // Supported on: body component (the paragraph uses the block's id).
            // For other block types (heading, callout, etc.) we ignore it silently
            // since they render to multiple paragraphs where per-paragraph cols
            // doesn't make sense.
            column_wrap: if component_type == "body" {
                v["columns"].as_u64().map(|n| {
                    // The block's id is the paragraph's id after rendering.
                    let id = v["id"].as_str()
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| format!("__new_p{}", uuid_simple()));
                    (id, n as u32)
                })
            } else {
                None
            },
        }))
    }

    /// Convert the tool's section inputs into the model `WordSection` list.
    fn convert_sections(
        inputs: &[DocSectionInput],
    ) -> Vec<crate::office::WordSection> {
        inputs
            .iter()
            .map(|s| crate::office::WordSection {
                id: s.id.clone(),
                section_type: s.section_type.clone(),
                page_size_twips: s.page_size_twips.as_ref().map(|p| crate::office::PageSize {
                    width: p.width,
                    height: p.height,
                    orient: p.orient.clone(),
                }),
                page_size_mm: s.page_size_mm.as_ref().map(|p| crate::office::PageSizeMm {
                    width: p.width,
                    height: p.height,
                    orient: p.orient.clone(),
                }),
                margins: s.margins.as_ref().map(|m| crate::office::PageMargins {
                    top: m.top,
                    right: m.right,
                    bottom: m.bottom,
                    left: m.left,
                    header: m.header,
                    footer: m.footer,
                    gutter: m.gutter,
                }),
                text_direction: s.text_direction.clone(),
                title_pg: s.title_pg.unwrap_or(false),
                cols: s.cols,
                page_num_start: s.page_num_start,
                page_num_format: s.page_num_format.clone(),
                header_refs: s
                    .header_refs
                    .as_ref()
                    .map(|refs| {
                        refs.iter()
                            .map(|r| crate::office::HeaderPartRef {
                                header_id: r.header_id.clone(),
                                kind: r.kind.clone(),
                            })
                            .collect()
                    })
                    .unwrap_or_default(),
                footer_refs: s
                    .footer_refs
                    .as_ref()
                    .map(|refs| {
                        refs.iter()
                            .map(|r| crate::office::FooterPartRef {
                                footer_id: r.footer_id.clone(),
                                kind: r.kind.clone(),
                            })
                            .collect()
                    })
                    .unwrap_or_default(),
            })
            .collect()
    }

    /// Convert the tool's header part inputs into the model `HeaderPart` list.
    fn convert_headers(
        inputs: &[DocHeaderPart],
    ) -> Vec<crate::office::HeaderPart> {
        inputs
            .iter()
            .map(|h| {
                let paragraphs = h
                    .paragraphs
                    .iter()
                    .map(|p| crate::office::WordParagraph {
                        id: p.id.clone().unwrap_or_else(|| format!("__new_p{}", uuid_simple())),
                        text: p.text.clone(),
                        style: p.style.clone(),
                        runs: p
                            .runs
                            .as_ref()
                            .map(|rvec| rvec.iter().map(|r| Self::to_font_run(r.clone())).collect()),
                        numbering: p.numbering.clone().map(crate::office::NumberingRef::from),
                        alignment: p.alignment.clone(),
                        text_direction: p.text_direction.clone(),
                    })
                    .collect();
                crate::office::HeaderPart {
                    id: h.id.clone(),
                    paragraphs,
                    tables: Vec::new(),
                    images: Vec::new(),
                }
            })
            .collect()
    }

    /// Convert the tool's footer part inputs into the model `FooterPart` list.
    fn convert_footers(
        inputs: &[DocFooterPart],
    ) -> Vec<crate::office::FooterPart> {
        inputs
            .iter()
            .map(|f| {
                let paragraphs = f
                    .paragraphs
                    .iter()
                    .map(|p| crate::office::WordParagraph {
                        id: p.id.clone().unwrap_or_else(|| format!("__new_p{}", uuid_simple())),
                        text: p.text.clone(),
                        style: p.style.clone(),
                        runs: p
                            .runs
                            .as_ref()
                            .map(|rvec| rvec.iter().map(|r| Self::to_font_run(r.clone())).collect()),
                        numbering: p.numbering.clone().map(crate::office::NumberingRef::from),
                        alignment: p.alignment.clone(),
                        text_direction: p.text_direction.clone(),
                    })
                    .collect();
                crate::office::FooterPart {
                    id: f.id.clone(),
                    paragraphs,
                    tables: Vec::new(),
                    images: Vec::new(),
                }
            })
            .collect()
    }

    pub async fn execute(&self, arguments: Value, workspace: Option<String>) -> Result<String, ToolError> {
        let params: CreateWordDocParams = serde_json::from_value(arguments).map_err(|e| {
            // serde's default "missing field `path`" message is technically
            // correct but unhelpful: the model often thinks "I already
            // passed the path last call" and gets stuck. Spell out exactly
            // what went wrong so the next retry passes path.
            let raw = e.to_string();
            let friendly = if raw.contains("missing field `path`") {
                "Missing required field `path`. The `path` field is required on every call \
                 (including append calls); the backend does not remember the path from a \
                 previous call. Please retry with the full absolute path to the .docx file."
                    .to_string()
            } else {
                format!("Invalid parameters: {}", raw)
            };
            ToolError::InvalidArguments("create_word_doc".to_string(), friendly)
        })?;

        validate_workspace_path(&params.path, &workspace)?;

        let path_obj = std::path::Path::new(&params.path);
        if path_obj.extension().and_then(|e| e.to_str()).unwrap_or("") != "docx" {
            return Err(ToolError::InvalidArguments("create_word_doc".to_string(), "Only .docx files are supported".into()));
        }

        // ── Parse and classify operations ────────────────────────────────────────

        // Collect operations from elements[]
        let mut modifies = Vec::new();
        let mut new_elements = Vec::new();
        let mut deletes = Vec::new();

        // Per-paragraph column-wrap hints. When the caller sets
        // `columns: N` on a paragraph element (low-level or body component),
        // we record the (paragraph_id, N) pair here so that after
        // `existing.modify(...)` materialises the paragraph list we can
        // inject the section-break markers that scope the column layout
        // to just that one paragraph.
        let mut column_wraps: Vec<(String, u32)> = Vec::new();

        // Component blocks (Cover / Chapter / Heading / Body / BulletList /
        // OrderedList / StyledTable / Callout / Code / PageBreak) are
        // recognised by their `type` field and routed through the design-
        // system renderer. They are append-only: each block expands into
        // a batch of paragraphs/tables that the tool appends after the
        // legacy new_elements. Anchor_id/position are recorded for
        // future use but ignored for element-level positioning.
        let mut component_renders: Vec<ComponentRender> = Vec::new();

        // Bug fix 5: Wire up params.deletes parameter
        if let Some(ref delete_ids) = params.deletes {
            deletes.extend(delete_ids.iter().cloned());
        }

        // Check if file exists to determine operation mode
        let file_exists = path_obj.exists();

        if let Some(ref elems) = params.elements {
            for v in elems {
                let is_delete = v["action"].as_str() == Some("delete");
                let has_id = v["id"].is_string();
                let has_anchor = v["anchor_id"].is_string();

                if is_delete {
                    if let Some(id) = v["id"].as_str() {
                        deletes.push(id.to_string());
                    }
                    continue;
                }

                // Route the element through the right parser. Component
                // blocks (`type: cover|chapter|heading|body|bullet_list|
                // ordered_list|styled_table|callout|code|page_break`) go
                // through the design-system renderer — never through the
                // legacy paragraph/table/image parsers.
                //
                // If `type` is set to a recognised low-level tag
                // (paragraph / table / image), fall through to the legacy
                // parser. If `type` is anything else, hand it to
                // `parse_component_block` so it can emit a clear "unknown
                // type X" error rather than silently degrading to a
                // legacy paragraph.
                let has_type = v["type"].is_string();
                let is_low_level_typed = has_type
                    && matches!(v["type"].as_str(), Some("paragraph" | "table" | "image"));
                if !is_low_level_typed && has_type {
                    let component = Self::parse_component_block(v)
                        .map_err(|e| ToolError::InvalidArguments("create_word_doc".to_string(), e))?;
                    if let Some(r) = component {
                        component_renders.push(r);
                    }
                    continue;
                }

                // Bug fix 1: Infer type from presence of header/rows fields if type is not specified
                let elem_type = v["type"].as_str().unwrap_or_else(|| {
                    if v.get("header").is_some() || v.get("rows").is_some() {
                        "table"
                    } else {
                        "paragraph"
                    }
                });
                let elem = if elem_type == "table" {
                    Self::parse_table(v)
                } else if elem_type == "image" {
                    Self::parse_image(v)
                } else {
                    Self::parse_paragraph(v)
                }
                .map_err(|e| ToolError::InvalidArguments("create_word_doc".to_string(), e))?;

                if let Some(e) = elem {
                    // Collect `columns: N` hint from the raw JSON before we
                    // consume `e` into modifies/new_elements.
                    if let Some(cols) = v["columns"].as_u64() {
                        let id = e.id();
                        column_wraps.push((id.to_string(), cols as u32));
                    }

                    // Bug fix: For new file creation, all elements go to new_elements
                    // For existing files, elements with ID are modifications
                    if file_exists && has_id && !has_anchor {
                        modifies.push(e);
                    } else {
                        // Store element with its anchor_id and position for positioned insertion
                        let anchor_id = v["anchor_id"].as_str().map(|s| s.to_string());
                        let position = v["position"].as_str().map(|s| s.to_string());
                        new_elements.push(crate::office::InsertElement {
                            element: e,
                            anchor_id,
                            position,
                        });
                    }
                }
            }
        }

        // Backward compat: convert old paragraphs/tables format
        if let Some(ref paras) = params.paragraphs {
            for p in paras {
                if p.delete_action.as_deref() == Some("delete") {
                    if let Some(ref id) = p.id {
                        deletes.push(id.clone());
                    }
                } else {
                    let paragraph_id = p.id.clone().unwrap_or_else(|| format!("__new_p{}", uuid_simple()));
                    if let Some(cols) = p.columns {
                        if cols > 1 {
                            column_wraps.push((paragraph_id.clone(), cols));
                        }
                    }
                    let elem = crate::office::DocElement::Paragraph {
                        id: paragraph_id,
                        text: p.text.clone(),
                        omit_text: false,
                        style: p.style.clone(),
                        runs: p.runs.as_ref().map(|rvec| rvec.iter().map(|r| Self::to_font_run(r.clone())).collect()),
                        numbering: p.numbering.clone().map(crate::office::NumberingRef::from),
                        alignment: p.alignment.clone(),
                        text_direction: p.text_direction.clone(),
                    };
                    if file_exists && p.id.is_some() {
                        modifies.push(elem);
                    } else {
                        let anchor_id = p.anchor_id.clone();
                        let position = p.position.clone();
                        new_elements.push(crate::office::InsertElement {
                            element: elem,
                            anchor_id,
                            position,
                        });
                    }
                }
            }
        }

        if let Some(ref tbls) = params.tables {
            for t in tbls {
                if t.delete_action.as_deref() == Some("delete") {
                    if let Some(ref id) = t.id {
                        deletes.push(id.clone());
                    }
                } else {
                    let header: Vec<crate::office::TableCell> = t
                        .header
                        .iter()
                        .map(|s| crate::office::TableCell::plain(s.clone()))
                        .collect();
                    let rows: Vec<Vec<crate::office::TableCell>> = t
                        .rows
                        .iter()
                        .map(|r| r.iter().map(|s| crate::office::TableCell::plain(s.clone())).collect())
                        .collect();
                    let elem = crate::office::DocElement::Table {
                        id: t.id.clone().unwrap_or_else(|| format!("__new_t{}", uuid_simple())),
                        position: 0,
                        header,
                        rows,
                    };
                    if file_exists && t.id.is_some() {
                        modifies.push(elem);
                    } else {
                        let anchor_id = t.anchor_id.clone();
                        let position = t.position.clone();
                        new_elements.push(crate::office::InsertElement {
                            element: elem,
                            anchor_id,
                            position,
                        });
                    }
                }
            }
        }

        // Determine if this is purely a new-file creation
        let has_operations = !modifies.is_empty()
            || !deletes.is_empty()
            || !new_elements.is_empty()
            || !component_renders.is_empty();
        // New file only if: no file exists, OR no operations requested
        let is_pure_new_file = !file_exists || !has_operations;

        // Append/deprecated mode: append_to takes precedence for backward compat
        if let Some(ref append_path) = params.append_to {
            if std::path::Path::new(append_path).exists() {
                validate_workspace_path(append_path, &workspace)?;
                let bytes = tokio::fs::read(append_path)
                    .await
                    .map_err(|e| ToolError::IoError(format!("Failed to read existing doc: {}", e)))?;
                let mut existing = crate::office::read_word_document(&bytes)
                    .map_err(|e| ToolError::ExecutionError(format!("Failed to read existing doc: {}", e)))?;

                let mut new_paras = Vec::new();
                let mut new_tables = Vec::new();
                let mut new_images = Vec::new();
                for insert_elem in new_elements {
                    match insert_elem.element {
                        crate::office::DocElement::Paragraph { id, text, style, runs, numbering, alignment, text_direction, .. } => {
                            new_paras.push(crate::office::WordParagraph { id, text, style, runs, numbering, alignment, text_direction });
                        }
                        crate::office::DocElement::Table { id, position: _, header, rows } => {
                            let mut table_rows = vec![];
                            if !header.is_empty() {
                                table_rows.push(crate::office::TableRow { cells: header });
                            }
                            for row in rows {
                                if !row.is_empty() {
                                    table_rows.push(crate::office::TableRow { cells: row });
                                }
                            }
                            new_tables.push(crate::office::WordTable { id, rows: table_rows, cell_paragraphs: Vec::new() });
                        }
                        crate::office::DocElement::Image { id, position: _, path, width_emu, height_emu } => {
                            new_images.push(crate::office::WordImage {
                                id,
                                path,
                                width_emu,
                                height_emu,
                                internal_path: None,
                            });
                        }
                    }
                }
                existing.paragraphs.extend(new_paras);
                existing.tables.extend(new_tables);
                existing.images.extend(new_images);

                // Append component blocks (cover / chapter / heading / body /
                // bullet_list / ordered_list / styled_table / callout / code /
                // page_break). Each block is a self-contained batch of
                // paragraphs + tables that we drop onto the end of the
                // document — anchor_id is recorded but currently unused
                // because the legacy modify path doesn't expose per-paragraph
                // insertion.
                for r in &component_renders {
                    existing.paragraphs.extend(r.rendered.paragraphs.iter().cloned());
                    existing.tables.extend(r.rendered.tables.iter().cloned());
                    existing.images.extend(r.rendered.images.iter().cloned());
                }

                // Collect column-wrap hints from component blocks. Body component
                // blocks carry `columns: N` via their `column_wrap` field.
                let mut all_column_wraps = column_wraps.clone();
                for r in &component_renders {
                    if let Some(hint) = &r.column_wrap {
                        all_column_wraps.push(hint.clone());
                    }
                }

                // Collect column-wrap hints from component blocks.
                let mut all_column_wraps = column_wraps.clone();
                for r in &component_renders {
                    if let Some(hint) = &r.column_wrap {
                        all_column_wraps.push(hint.clone());
                    }
                }

                // Apply user sections FIRST, before expand_paragraph_columns.
                // This ensures the baseline for column-wrap sections is set correctly.
                // The user's section (if provided) becomes the trailing section,
                // and expand_paragraph_columns will use it as the baseline to clone
                // the column-wrap sections.
                if let Some(ref sections) = params.sections {
                    if !sections.is_empty() {
                        existing.sections = Self::convert_sections(sections);
                    }
                }

                // Expand per-paragraph column hints into section-break markers.
                // This injects `__sect_break_<idx>__` paragraphs and additional
                // `WordSection` entries so only the targeted paragraphs are
                // laid out in the requested number of columns.
                // IMPORTANT: This must run AFTER user sections are applied,
                // so the column-wrap sections are based on the user's section baseline.
                if !all_column_wraps.is_empty() {
                    if let Err(e) = expand_paragraph_columns(
                        &mut existing.paragraphs,
                        &mut existing.sections,
                        &all_column_wraps,
                    ) {
                        return Err(ToolError::InvalidArguments("create_word_doc".to_string(), e));
                    }
                }

                // Validate sections[].cols usage: warn when a single section
                // carries cols>1, because without an explicit section break the
                // column layout applies to the whole document — almost certainly
                // not what the AI intended when it asked for "分栏" or "columns".
                if let Some(ref sects) = params.sections {
                    if sects.len() == 1 {
                        if let Some(cols) = sects[0].cols {
                            if cols > 1 {
                                eprintln!(
                                    "[create_word_doc] WARNING: sections[0].cols={} would make \
                                     the ENTIRE document {} columns. If you only wanted a \
                                     portion of the document in multiple columns, use \
                                     `columns` on individual paragraph elements instead of \
                                     `sections[].cols`. The document was written as-is.",
                                    cols, cols
                                );
                            }
                        }
                    }
                }
                if let Some(ref headers) = params.headers {
                    if !headers.is_empty() {
                        existing.headers = Self::convert_headers(headers);
                    }
                }
                if let Some(ref footers) = params.footers {
                    if !footers.is_empty() {
                        existing.footers = Self::convert_footers(footers);
                    }
                }

                crate::office::write_word_document_to_path(&existing, path_obj, Some(&bytes))
                    .map_err(|e| ToolError::ExecutionError(format!("Failed to write doc: {}", e)))?;
                return Ok(format!("Successfully appended content to: {}", params.path));
            }
        }

        // Progressive append mode: append new elements to existing document without reading/modifying structure
        if params.append == Some(true) && file_exists && (!new_elements.is_empty() || !component_renders.is_empty()) {
            let bytes = tokio::fs::read(&params.path)
                .await
                .map_err(|e| ToolError::IoError(format!("Failed to read existing doc: {}", e)))?;
            let mut existing = crate::office::read_word_document(&bytes)
                .map_err(|e| ToolError::ExecutionError(format!("Failed to read existing doc: {}", e)))?;

            // Build a temporary document from just the new elements, then extract its parts
            let temp_elements: Vec<crate::office::DocElement> = new_elements.iter().map(|ie| ie.element.clone()).collect();
            let temp_doc = crate::office::WordDocument::from_elements(temp_elements);
            let mut new_count = temp_doc.paragraphs.len() + temp_doc.tables.len() + temp_doc.images.len();

            existing.paragraphs.extend(temp_doc.paragraphs);
            existing.tables.extend(temp_doc.tables);
            existing.images.extend(temp_doc.images);

            // Collect column-wrap hints from component blocks.
            let mut all_column_wraps = column_wraps.clone();
            for r in &component_renders {
                if let Some(hint) = &r.column_wrap {
                    all_column_wraps.push(hint.clone());
                }
            }

            // Apply user sections FIRST, before expand_paragraph_columns.
            // This ensures the baseline for column-wrap sections is set correctly.
            if let Some(ref sections) = params.sections {
                if !sections.is_empty() {
                    existing.sections = Self::convert_sections(sections);
                }
            }

            // Expand per-paragraph column hints into section-break markers.
            // This injects `__sect_break_<idx>__` paragraphs and additional
            // `WordSection` entries so only the targeted paragraphs are
            // laid out in the requested number of columns.
            // IMPORTANT: This must run AFTER user sections are applied.
            if !all_column_wraps.is_empty() {
                if let Err(e) = expand_paragraph_columns(
                    &mut existing.paragraphs,
                    &mut existing.sections,
                    &all_column_wraps,
                ) {
                    return Err(ToolError::InvalidArguments("create_word_doc".to_string(), e));
                }
            }

            // Validate sections[].cols usage.
            if let Some(ref sects) = params.sections {
                if sects.len() == 1 {
                    if let Some(cols) = sects[0].cols {
                        if cols > 1 {
                            eprintln!(
                                "[create_word_doc] WARNING: sections[0].cols={} would make \
                                 the ENTIRE document {} columns. Use `columns` on individual \
                                 paragraph elements instead of `sections[].cols` to limit the \
                                 multi-column layout to a specific section of the document.",
                                cols, cols
                            );
                        }
                    }
                }
            }

            // Then append any component blocks (design-system styled).
            for r in &component_renders {
                new_count += r.rendered.paragraphs.len() + r.rendered.tables.len() + r.rendered.images.len();
                existing.paragraphs.extend(r.rendered.paragraphs.iter().cloned());
                existing.tables.extend(r.rendered.tables.iter().cloned());
                existing.images.extend(r.rendered.images.iter().cloned());
            }

            crate::office::write_word_document_to_path(&existing, path_obj, Some(&bytes))
                .map_err(|e| ToolError::ExecutionError(format!("Failed to append to doc: {}", e)))?;
            return Ok(format!("Successfully appended {} element(s) to: {}", new_count, params.path));
        }

        // Existing file with operations: modify/delete/insert
        if file_exists && !is_pure_new_file {
            let bytes = tokio::fs::read(&params.path)
                .await
                .map_err(|e| ToolError::IoError(format!("Failed to read existing doc: {}", e)))?;
            let mut existing = crate::office::read_word_document(&bytes)
                .map_err(|e| ToolError::ExecutionError(format!("Failed to read existing doc: {}", e)))?;

            // Bug fix: when a low-level element (paragraph/table/image) carries
            // an `id` that does NOT match any pre-existing element, the modify
            // path silently drops it (the modify_map is keyed by id and only
            // matches against existing elements). Re-route those orphans into
            // `new_elements` so they get inserted instead. Without this, callers
            // who pass an `id` for a brand-new image (instead of relying on
            // anchor_id) would see the image vanish — see the
            // `image_with_id_falls_back_to_insert` regression test.
            let existing_ids: std::collections::HashSet<String> = {
                let snapshot = existing.to_elements();
                snapshot.iter().map(|e| e.id().to_string()).collect()
            };
            let mut orphans: Vec<crate::office::InsertElement> = Vec::new();
            modifies.retain(|e| {
                if existing_ids.contains(e.id()) {
                    true
                } else {
                    // Treat the orphan as a new insertion. Carry its anchor_id /
                    // position from the original input JSON if present (none for
                    // legacy modifies[]); default to end-of-doc.
                    orphans.push(crate::office::InsertElement {
                        element: e.clone(),
                        anchor_id: None,
                        position: None,
                    });
                    false
                }
            });
            // New elements that already have an anchor_id keep their anchor;
            // append orphans after them so anchor positioning isn't disturbed.
            new_elements.extend(orphans);

            existing.modify(modifies, deletes, new_elements);

            // Append component blocks (design-system styled) on top of
            // whatever `modify` produced. Each block expands into a
            // batch of paragraphs/tables that we append to the end of
            // the document.
            for r in &component_renders {
                existing.paragraphs.extend(r.rendered.paragraphs.iter().cloned());
                existing.tables.extend(r.rendered.tables.iter().cloned());
                existing.images.extend(r.rendered.images.iter().cloned());
            }

            // Collect column-wrap hints from component blocks.
            let mut all_column_wraps = column_wraps.clone();
            for r in &component_renders {
                if let Some(hint) = &r.column_wrap {
                    all_column_wraps.push(hint.clone());
                }
            }

            // Apply user sections FIRST, before expand_paragraph_columns.
            // This ensures the baseline for column-wrap sections is set correctly.
            if let Some(ref sections) = params.sections {
                if !sections.is_empty() {
                    existing.sections = Self::convert_sections(sections);
                }
            }

            // Expand per-paragraph column hints into section-break markers.
            // This injects `__sect_break_<idx>__` paragraphs and additional
            // `WordSection` entries so only the targeted paragraphs are
            // laid out in the requested number of columns.
            // IMPORTANT: This must run AFTER user sections are applied.
            if !all_column_wraps.is_empty() {
                if let Err(e) = expand_paragraph_columns(
                    &mut existing.paragraphs,
                    &mut existing.sections,
                    &all_column_wraps,
                ) {
                    return Err(ToolError::InvalidArguments("create_word_doc".to_string(), e));
                }
            }

            // Validate sections[].cols usage.
            if let Some(ref sects) = params.sections {
                if sects.len() == 1 {
                    if let Some(cols) = sects[0].cols {
                        if cols > 1 {
                            eprintln!(
                                "[create_word_doc] WARNING: sections[0].cols={} would make \
                                 the ENTIRE document {} columns. Use `columns` on individual \
                                 paragraph elements instead of `sections[].cols` to limit the \
                                 multi-column layout to a specific section of the document.",
                                cols, cols
                            );
                        }
                    }
                }
            }
            if let Some(ref headers) = params.headers {
                if !headers.is_empty() {
                    existing.headers = Self::convert_headers(headers);
                }
            }
            if let Some(ref footers) = params.footers {
                if !footers.is_empty() {
                    existing.footers = Self::convert_footers(footers);
                }
            }

            crate::office::write_word_document_to_path(&existing, path_obj, Some(&bytes))
                .map_err(|e| ToolError::ExecutionError(format!("Failed to modify document: {}", e)))?;
            return Ok(format!("Successfully modified document: {}", params.path));
        }

        // Existing file with no operations: no-op
        if file_exists {
            return Ok(format!("Document already exists, no changes requested: {}", params.path));
        }

        // New file mode: title + new_elements
        let mut elements_for_new: Vec<crate::office::DocElement> = Vec::new();

        if let Some(ref title) = params.title {
            if !title.is_empty() {
                elements_for_new.push(crate::office::DocElement::Paragraph {
                    id: format!("__new_p{}", uuid_simple()),
                    text: title.clone(),
                    omit_text: false,
                    style: Some("Title".to_string()),
                    runs: None,
                    numbering: None,
                    alignment: Some("center".to_string()),
                    text_direction: None,
                });
            }
        }

        for insert_elem in new_elements {
            elements_for_new.push(insert_elem.element);
        }

        // ── Write document ─────────────────────────────────────────────────────────

        let mut doc = crate::office::WordDocument::from_elements(elements_for_new);

        // Append component blocks (cover / chapter / heading / body /
        // bullet_list / ordered_list / styled_table / callout / code /
        // page_break). Each block expands into a batch of paragraphs
        // + tables; we drop them onto the end of the doc in the order
        // the caller specified.
        for r in &component_renders {
            doc.paragraphs.extend(r.rendered.paragraphs.iter().cloned());
            doc.tables.extend(r.rendered.tables.iter().cloned());
            doc.images.extend(r.rendered.images.iter().cloned());
        }

        // Collect column-wrap hints from component blocks.
        let mut all_column_wraps = column_wraps.clone();
        for r in &component_renders {
            if let Some(hint) = &r.column_wrap {
                all_column_wraps.push(hint.clone());
            }
        }

        // Apply user sections FIRST, before expand_paragraph_columns.
        // This ensures the baseline for column-wrap sections is set correctly.
        if let Some(ref sections) = params.sections {
            if !sections.is_empty() {
                doc.sections = Self::convert_sections(sections);
            }
        }

        // Expand per-paragraph column hints into section-break markers.
        // This injects `__sect_break_<idx>__` paragraphs and additional
        // `WordSection` entries so only the targeted paragraphs are
        // laid out in the requested number of columns.
        // IMPORTANT: This must run AFTER user sections are applied.
        if !all_column_wraps.is_empty() {
            if let Err(e) = expand_paragraph_columns(
                &mut doc.paragraphs,
                &mut doc.sections,
                &all_column_wraps,
            ) {
                return Err(ToolError::InvalidArguments("create_word_doc".to_string(), e));
            }
        }

        // Validate sections[].cols usage.
        if let Some(ref sects) = params.sections {
            if sects.len() == 1 {
                if let Some(cols) = sects[0].cols {
                    if cols > 1 {
                        eprintln!(
                            "[create_word_doc] WARNING: sections[0].cols={} would make \
                             the ENTIRE document {} columns. Use `columns` on individual \
                             paragraph elements instead of `sections[].cols` to limit the \
                             multi-column layout to a specific section of the document.",
                            cols, cols
                        );
                    }
                }
            }
        }
        if let Some(ref headers) = params.headers {
            if !headers.is_empty() {
                doc.headers = Self::convert_headers(headers);
            }
        }
        if let Some(ref footers) = params.footers {
            if !footers.is_empty() {
                doc.footers = Self::convert_footers(footers);
            }
        }
        crate::office::write_word_document_to_path(&doc, path_obj, None)
            .map_err(|e| ToolError::ExecutionError(format!("Failed to write Word document: {}", e)))?;

        Ok(format!("Successfully created Word document: {}", params.path))
    }
}

impl Default for CreateWordDocTool {
    fn default() -> Self { Self::new() }
}

/// Tiny opaque id used by `CreateWordDocTool` to thread stable ids
/// through nested structs (and avoid pulling in the `uuid` crate just
/// for this). The previous clock + thread-local counter pattern is
/// preserved verbatim so collisions stay vanishingly rare.
fn uuid_simple() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    // `SystemTime::duration_since(UNIX_EPOCH)` only fails when the system
    // clock is set *before* 1970. Falling back to zero costs us one epoch of
    // nanosecond resolution; the value is only used to build an opaque id,
    // not as a real timestamp.
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    use std::sync::atomic::{AtomicU64, Ordering};
    thread_local! { static CNT: AtomicU64 = AtomicU64::new(0); }
    let cnt = CNT.with(|c| c.fetch_add(1, Ordering::Relaxed));
    format!("{}{}", now.as_nanos(), cnt)
}

// ── Component block bridge tests ───────────────────────────────────────────────
//
// These tests exercise the JSON schema → WordDocument path that the AI uses:
// a CreateWordDocTool call with `elements[]` carrying component block types
// (cover / chapter / heading / body / bullet_list / ordered_list /
// styled_table / callout / code / page_break) should produce a `.docx` whose
// internal structure matches what the design-system renderer produces — i.e.
// `render_blocks` is the single source of truth.

#[cfg(test)]
mod component_bridge_tests {
    use super::*;
    use crate::office::WordDocument;
    use crate::office::read_word_document;
    use serde_json::json;
    use std::path::PathBuf;

    /// Build a temp path under the OS temp dir. Each test gets its own file
    /// so they can run in parallel without colliding.
    fn tmp_path(name: &str) -> PathBuf {
        let mut p = std::env::temp_dir();
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        p.push(format!("inkuo_create_word_doc_{}_{}_{}.docx", name, std::process::id(), nanos));
        p
    }

    /// Drive the tool with a JSON payload and return the parsed WordDocument.
    async fn run_tool(payload: serde_json::Value) -> WordDocument {
        let tool = CreateWordDocTool::new();
        let path = payload["path"].as_str().unwrap().to_string();
        let result = tool
            .execute(payload.clone(), None)
            .await
            .expect("tool should succeed");
        assert!(result.starts_with("Successfully"), "tool returned: {}", result);
        let bytes = tokio::fs::read(&path).await.expect("file should exist");
        read_word_document(&bytes).expect("file should be a valid docx")
    }

    #[tokio::test]
    async fn cover_chapter_heading_chain_creates_normal_doc() {
        let path = tmp_path("cover_chain");
        let payload = json!({
            "path": path.to_string_lossy(),
            "elements": [
                {"type": "cover", "id": "cover1", "title": "My Report", "subtitle": "An inkuo demo"},
                {"type": "chapter", "id": "ch1", "title": "Chapter 1"},
                {"type": "heading", "id": "h1", "level": 2, "text": "Section 1.1"},
                {"type": "body", "id": "p1", "text": "Hello world."},
            ]
        });

        let doc = run_tool(payload).await;

        // Cover emits 3 paragraphs (title + subtitle + spacer). Chapter
        // adds 1, heading adds 1, body adds 1 — total 6.
        assert!(doc.paragraphs.len() >= 5, "got {} paragraphs", doc.paragraphs.len());
        // The cover paragraph carries the cover-title style (CoverTitle).
        let cover_seen = doc.paragraphs.iter().any(|p| {
            p.style.as_deref() == Some("CoverTitle") && p.text.contains("My Report")
        });
        assert!(cover_seen, "expected CoverTitle paragraph");
    }

    #[tokio::test]
    async fn bulleted_and_ordered_lists_get_numbering() {
        let path = tmp_path("lists");
        let payload = json!({
            "path": path.to_string_lossy(),
            "elements": [
                {"type": "bullet_list", "id_prefix": "b", "items": ["a", "b", "c"]},
                {"type": "ordered_list", "id_prefix": "o", "items": ["x", "y"]},
            ]
        });

        let doc = run_tool(payload).await;

        // 3 bullets + 2 ordered = 5 paragraphs.
        assert!(doc.paragraphs.len() >= 5, "got {} paragraphs", doc.paragraphs.len());
        let bulleted = doc.paragraphs.iter().filter(|p| {
            p.numbering.as_ref().map(|n| n.num_id == 1).unwrap_or(false)
        }).count();
        let ordered = doc.paragraphs.iter().filter(|p| {
            p.numbering.as_ref().map(|n| n.num_id == 2).unwrap_or(false)
        }).count();
        assert_eq!(bulleted, 3, "expected 3 bulleted items");
        assert_eq!(ordered, 2, "expected 2 ordered items");
    }

    #[tokio::test]
    async fn styled_table_emits_styled_marker() {
        let path = tmp_path("styled_table");
        let payload = json!({
            "path": path.to_string_lossy(),
            "elements": [
                {"type": "styled_table", "id": "t1",
                 "headers": ["col1", "col2"],
                 "rows": [["a", "b"], ["c", "d"]],
                 "style": {"header_fill": "213B32", "zebra": true}},
            ]
        });

        let doc = run_tool(payload).await;

        // The styled writer strips the `__STYLE__|...` marker row at
        // emit time (it's scaffolding used to carry the visual params
        // through the model layer). After round-trip the table should
        // contain only the real header + body cells — col1/col2 in
        // the header, a/b/c/d in the body. We verify that all four
        // pieces survived and the marker is gone (so callers can't
        // accidentally rely on scaffolding leaking through).
        assert!(!doc.tables.is_empty(), "expected at least one table");
        let joined: String = doc.tables.iter()
            .flat_map(|t| t.rows.iter())
            .flat_map(|r| r.cells.iter())
            .map(|c| c.text.clone())
            .collect::<Vec<_>>()
            .join("\n");
        for needle in ["col1", "col2", "a", "b", "c", "d"] {
            assert!(
                joined.contains(needle),
                "expected styled table to contain '{}' after round-trip; got: {}",
                needle, joined
            );
        }
        assert!(
            !joined.contains("__STYLE__|"),
            "styled writer should strip the marker row before round-trip; got: {}",
            joined
        );
    }

    #[tokio::test]
    async fn callout_block_emits_callout_marker() {
        let path = tmp_path("callout");
        let payload = json!({
            "path": path.to_string_lossy(),
            "elements": [
                {"type": "callout", "id": "cal1", "level": "warning",
                 "title": "Heads up", "body": "Be careful with this."},
            ]
        });

        let doc = run_tool(payload).await;

        // The callout's title + body paragraphs are stored in the
        // container table's `cell_paragraphs` field (round-trip-safe)
        // so the writer can re-emit them inside the shaded cell on
        // the next save. They survive the round-trip even though
        // they're not in the top-level `paragraphs` list.
        let cell_joined: String = doc.tables.iter()
            .flat_map(|t| t.cell_paragraphs.iter())
            .map(|p| p.text.clone())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            cell_joined.contains("Heads up") && cell_joined.contains("Be careful with this."),
            "expected callout title + body in cell_paragraphs; got: {}",
            cell_joined
        );
        // The writer's `classify_and_strip` removes the marker row
        // before persisting — that's correct (it's internal
        // scaffolding). The reader still knows this is a container
        // by its 1×1 shape (no marker row needed). We verify the
        // cell text round-trips: the title + body appear in the
        // first cell's flattened text, AND the cell_paragraphs
        // collection keeps the structured representation.
        let table_text: String = doc.tables.iter()
            .flat_map(|t| t.rows.iter())
            .flat_map(|r| r.cells.iter())
            .map(|c| c.text.clone())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            table_text.contains("Heads up") && table_text.contains("Be careful with this."),
            "expected callout title + body in cell text; got: {}",
            table_text
        );
        assert!(
            doc.tables.iter().any(|t| t.cell_paragraphs.len() >= 2),
            "expected at least one container table to have ≥2 cell_paragraphs; got: {:?}",
            doc.tables.iter().map(|t| t.cell_paragraphs.len()).collect::<Vec<_>>()
        );
    }

    #[tokio::test]
    async fn code_block_emits_code_marker() {
        let path = tmp_path("code");
        let payload = json!({
            "path": path.to_string_lossy(),
            "elements": [
                {"type": "code", "id": "code1",
                 "lines": ["fn main() {", "  println!(\"hi\");", "}"],
                 "language": "rust"},
            ]
        });

        let doc = run_tool(payload).await;
        // Code lines live in `cell_paragraphs` of the container table
        // so they survive the round-trip; the marker row identifies
        // the container as a code block.
        let cell_joined: String = doc.tables.iter()
            .flat_map(|t| t.cell_paragraphs.iter())
            .map(|p| p.text.clone())
            .collect::<Vec<_>>()
            .join("\n");
        for needle in ["fn main() {", "  println!(\"hi\");", "}", "rust"] {
            assert!(
                cell_joined.contains(needle),
                "expected code block to retain '{}' in cell_paragraphs; got: {}",
                needle, cell_joined
            );
        }
        // Cell text (flattened) also retains the lines.
        let table_text: String = doc.tables.iter()
            .flat_map(|t| t.rows.iter())
            .flat_map(|r| r.cells.iter())
            .map(|c| c.text.clone())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            table_text.contains("fn main() {") && table_text.contains("rust"),
            "expected code lines + language in cell text; got: {}",
            table_text
        );
        assert!(
            doc.tables.iter().any(|t| t.cell_paragraphs.len() >= 4),
            "expected the code-block table to carry ≥4 cell_paragraphs (lang + 3 lines); got: {:?}",
            doc.tables.iter().map(|t| t.cell_paragraphs.len()).collect::<Vec<_>>()
        );
    }

    #[tokio::test]
    async fn page_break_is_present() {
        // The page-break component emits a paragraph with a single empty
        // run. The current writer treats this as a no-op paragraph at the
        // XML level — the page-break behaviour for component blocks is
        // layered on top of paragraph stylings in the renderer. We verify
        // only that the page-break paragraph survived the round-trip and
        // is positioned between two body paragraphs.
        let path = tmp_path("page_break");
        let payload = json!({
            "path": path.to_string_lossy(),
            "elements": [
                {"type": "body", "id": "p1", "text": "before"},
                {"type": "page_break", "id": "pb1"},
                {"type": "body", "id": "p2", "text": "after"},
            ]
        });

        let doc = run_tool(payload).await;
        let order: Vec<_> = doc.paragraphs.iter().map(|p| p.id.clone()).collect();
        let p1 = order.iter().position(|id| id == "p1").expect("p1 exists");
        let pb = order.iter().position(|id| id == "pb1").expect("pb1 exists");
        let p2 = order.iter().position(|id| id == "p2").expect("p2 exists");
        assert!(p1 < pb && pb < p2, "expected order p1 < pb < p2, got: {:?}", order);
    }

    #[tokio::test]
    async fn unknown_component_type_returns_error() {
        let path = tmp_path("unknown_type");
        let payload = json!({
            "path": path.to_string_lossy(),
            "elements": [
                {"type": "not_a_real_type", "id": "x"},
            ]
        });

        let tool = CreateWordDocTool::new();
        let result = tool.execute(payload, None).await;
        match result {
            Err(ToolError::InvalidArguments(_, msg)) => {
                assert!(msg.contains("Unknown element type"), "got: {}", msg);
            }
            other => panic!("expected InvalidArguments, got: {:?}", other),
        }
    }

    #[tokio::test]
    async fn legacy_low_level_elements_still_work() {
        // Sanity check: the new component path doesn't break the legacy
        // paragraph/table/image flow.
        let path = tmp_path("legacy");
        let payload = json!({
            "path": path.to_string_lossy(),
            "elements": [
                {"id": "p_legacy", "text": "legacy paragraph", "style": "Heading1"},
            ]
        });

        let doc = run_tool(payload).await;
        let para = doc.paragraphs.iter()
            .find(|p| p.id == "p_legacy")
            .expect("legacy paragraph should exist");
        assert_eq!(para.text, "legacy paragraph");
        assert_eq!(para.style.as_deref(), Some("Heading1"));
    }

    #[tokio::test]
    async fn image_with_id_falls_back_to_insert() {
        // Regression: an image element with an explicit `id` that does
        // not match any pre-existing element used to be silently dropped
        // by the modify path (the modify_map is keyed by id and the
        // element vanished if no key matched). The fix re-routes the
        // orphan into `new_elements` so it lands in the doc instead.
        //
        // We seed an existing document with one body paragraph, then
        // run a modify operation that supplies an image with a fresh
        // id (no matching existing element) and no anchor. The image
        // must end up in the saved doc.
        let path = tmp_path("img_id_orphan");
        let seed = json!({
            "path": path.to_string_lossy(),
            "elements": [
                {"type": "body", "id": "seed", "text": "seed body"},
            ]
        });
        // Write the seed.
        run_tool(seed).await;

        // Build a tiny PNG (1x1 transparent) into a temp file. The
        // writer only needs the bytes + extension to be valid; the
        // dimensions we pass below are what matter for layout.
        let png_path = {
            let mut p = std::env::temp_dir();
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            p.push(format!("inkuo_orphan_{}.png", nanos));
            p
        };
        // Minimal valid 1x1 PNG (89 bytes).
        let png_bytes: &[u8] = &[
            0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A,
            0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52,
            0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01,
            0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4,
            0x89, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x44, 0x41,
            0x54, 0x78, 0x9C, 0x63, 0x00, 0x01, 0x00, 0x00,
            0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00,
            0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE,
            0x42, 0x60, 0x82,
        ];
        std::fs::write(&png_path, png_bytes).expect("write png");

        let modify = json!({
            "path": path.to_string_lossy(),
            "elements": [
                {"type": "image", "id": "img_orphan",
                 "path": png_path.to_string_lossy(),
                 "width_emu": 914400, "height_emu": 914400},
            ]
        });
        run_tool(modify).await;

        // Re-open the resulting docx and verify the image is present.
        let bytes = std::fs::read(&path).expect("read back");
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).expect("open zip");
        let mut doc_xml = String::new();
        zip.by_name("word/document.xml")
            .expect("document.xml")
            .read_to_string(&mut doc_xml)
            .expect("read");
        assert!(
            doc_xml.contains("<w:drawing>"),
            "orphan image with id must still be inserted; document.xml has no <w:drawing>"
        );
        assert!(
            doc_xml.contains("img_orphan") || doc_xml.contains("__img_pos_img_orphan"),
            "image marker or stable id should appear in document.xml"
        );
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(&png_path);
    }
}
