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

use serde::Deserialize;
use serde_json::Value;

use super::{ToolDefinition, ToolError, ToolParameters, validate_workspace_path};

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
}

/// Same shape as `NumberingRef` but deserialized from the wire-format JSON.
#[derive(Debug, Clone, Deserialize)]
struct NumberingInput {
    num_id: u32,
    #[serde(default)]
    level: u32,
}

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
                        "Array of element objects. Paragraph: {id?, text?, style?, runs?, position?, anchor_id?, alignment?, text_direction?}. Table: {id?, header, rows, position?, anchor_id?}. Image: {type:'image', id?, path, width_emu, height_emu, anchor_id?, position?}.\n\
                         Elements with id replace existing ones; without id are appended or inserted at anchor_id+position. Use action:'delete' with id to delete.\n\
                         When modifying (id present), omit 'text' field to preserve original text. Providing 'text' field will update the paragraph text.\n\
                         Omit 'runs' to keep original formatting, or provide 'runs' array to fully replace paragraph formatting.\n\
                         runs shape: array of {text, bold?, italic?, underline?, font_size? (half-points, e.g. 24=12pt), color? (hex RGB, e.g. 'FF0000'), font_name?, highlight?, vert_align?, field?}.\n\
                         alignment: 'left' | 'right' | 'center' | 'both' | 'distribute'.\n\
                         text_direction: 'horizontal' | 'vertical' | 'verticalRightToLeft' | 'verticalLeftToRight' | 'rotate90' | 'rotate270'.\n\
                         vert_align: 'superscript' | 'subscript' on a run.\n\
                         field: {kind: 'page' | 'numpages' | 'date' | 'time' | 'author' | 'title' | 'custom', format?: '<format-string>', instr?: '<raw field instr>'} for a Word field code. When set, the run renders as a live field instead of plain text (e.g. page number, current date).\n\
                         position can be 'before' or 'after' (default) to control where new elements are inserted relative to anchor_id.\n\
                         Tables are auto-detected from header/rows fields, no need to specify type='table'.\n\
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
                         - cols: number of text columns. 1 = single column. >1 = multi-column.\n\
                         - page_num_start: starting page number (omit to continue from previous section).\n\
                         - page_num_format: 'decimal' (default) | 'upperRoman' | 'lowerRoman' | 'upperLetter' | 'lowerLetter'.\n\
                         - header_refs: array of {header_id, kind?} where kind is 'default' (default) | 'first' | 'even'.\n\
                         - footer_refs: array of {footer_id, kind?} with the same kind values.\n\
                         For multi-section docs (e.g. cover page in landscape + body in portrait vertical), list each section in order; the LAST section's sectPr is the trailing one in the body, the rest are embedded as section breaks at the end of their section's content."
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
        }
    }

    fn parse_paragraph(v: &serde_json::Value) -> Result<Option<crate::office::DocElement>, String> {
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

        // Collect operations from elements[]
        let mut modifies = Vec::new();
        let mut new_elements = Vec::new();
        let mut deletes = Vec::new();
        
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

                // Bug fix 1: Infer type from presence of header/rows fields if type is not specified
                let elem_type = v["type"].as_str().unwrap_or_else(|| {
                    if v.get("header").is_some() || v.get("rows").is_some() {
                        "table"
                    } else {
                        "paragraph"
                    }
                });
                let result = if elem_type == "table" {
                    Self::parse_table(v)
                } else if elem_type == "image" {
                    Self::parse_image(v)
                } else {
                    Self::parse_paragraph(v)
                };

                let elem = result.map_err(|e| ToolError::InvalidArguments("create_word_doc".to_string(), e))?;

                if let Some(e) = elem {
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
                    let elem = crate::office::DocElement::Paragraph {
                        id: p.id.clone().unwrap_or_else(|| format!("__new_p{}", uuid_simple())),
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
        let has_operations = !modifies.is_empty() || !deletes.is_empty() || !new_elements.is_empty();
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
                            new_tables.push(crate::office::WordTable { id, rows: table_rows });
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

                if let Some(ref sections) = params.sections {
                    if !sections.is_empty() {
                        existing.sections = Self::convert_sections(sections);
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
        if params.append == Some(true) && file_exists && !new_elements.is_empty() {
            let bytes = tokio::fs::read(&params.path)
                .await
                .map_err(|e| ToolError::IoError(format!("Failed to read existing doc: {}", e)))?;
            let mut existing = crate::office::read_word_document(&bytes)
                .map_err(|e| ToolError::ExecutionError(format!("Failed to read existing doc: {}", e)))?;

            // Build a temporary document from just the new elements, then extract its parts
            let temp_elements: Vec<crate::office::DocElement> = new_elements.iter().map(|ie| ie.element.clone()).collect();
            let temp_doc = crate::office::WordDocument::from_elements(temp_elements);
            let new_count = temp_doc.paragraphs.len() + temp_doc.tables.len() + temp_doc.images.len();

            existing.paragraphs.extend(temp_doc.paragraphs);
            existing.tables.extend(temp_doc.tables);
            existing.images.extend(temp_doc.images);

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

            existing.modify(modifies, deletes, new_elements);

            if let Some(ref sections) = params.sections {
                if !sections.is_empty() {
                    existing.sections = Self::convert_sections(sections);
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

        let mut doc = crate::office::WordDocument::from_elements(elements_for_new);
        if let Some(ref sections) = params.sections {
            if !sections.is_empty() {
                doc.sections = Self::convert_sections(sections);
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
