//! The renderer layer — bridges the high-level structured content
//! model and the existing low-level OOXML writer.
//!
//! ## Layering
//!
//! ```text
//! AI / agent    ContentBlock (JSON-like content)
//!     ↓
//! Renderer      docx::components (style application)
//!     ↓
//! Writer        docx::writer (OOXML string assembly)
//! ```
//!
//! Today the renderer just translates a [`DocumentContent`] tree into a
//! flat list of paragraphs + tables + images that the existing writer
//! can serialise. Future enhancements (cover-page generation, TOC,
//! cross-references) live here so the writer stays focused on the
//! OOXML mechanics.

use crate::office::docx::components::{
    body_paragraph, body_runs, bulleted_list, callout_block, callout_multiline, chapter_title,
    code_block, cover_title, heading, ordered_list, page_break, styled_table,
    CalloutLevel, CalloutRender, CodeBlockRender, TableStyle,
};
use crate::office::docx::design_tokens::{DesignTokens, DEFAULT_PALETTE};
use crate::office::docx::types::{FontRun, WordImage, WordParagraph, WordTable};
use serde::{Deserialize, Serialize};

/// One block of document content. Each variant maps to one component
/// in `components.rs`. The `id` field is preserved through to the
/// emitted paragraph / table so callers can `modify()` the rendered
/// document later by referencing it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    /// Cover-page title (oversized, centred).
    Cover {
        id: String,
        title: String,
        #[serde(default)]
        subtitle: Option<String>,
    },
    /// Chapter title — H1 inside a chapter section.
    Chapter {
        id: String,
        title: String,
    },
    /// Generic heading. `level`: 1 (chapter), 2 (section), 3 (subsection).
    Heading {
        id: String,
        level: u8,
        text: String,
    },
    /// Body paragraph. Plain text or rich runs (caller picks via the
    /// presence of `runs`).
    Body {
        id: String,
        #[serde(default)]
        text: Option<String>,
        #[serde(default)]
        runs: Option<Vec<RichRun>>,
    },
    /// Bulleted list.
    BulletList {
        id_prefix: String,
        items: Vec<String>,
    },
    /// Ordered list.
    OrderedList {
        id_prefix: String,
        items: Vec<String>,
    },
    /// Styled table. `style` controls the brand-level look.
    ///
    /// Note: the JSON tag is `styled_table` (not `table`) so the AI side
    /// can clearly distinguish a brand-styled table from a low-level
    /// plain table. Rendered through the same `styled_table` component.
    #[serde(rename = "styled_table")]
    Table {
        id: String,
        headers: Vec<String>,
        rows: Vec<Vec<String>>,
        #[serde(default)]
        style: Option<ContentTableStyle>,
    },
    /// Info / Warning / Important / Tip callout.
    Callout {
        id: String,
        level: CalloutLevelName,
        title: String,
        #[serde(default)]
        body: Option<String>,
        #[serde(default)]
        body_lines: Option<Vec<String>>,
    },
    /// Code block. `lines` is a list of monospace strings; `language`
    /// is shown as a small label (optional).
    Code {
        id: String,
        lines: Vec<String>,
        #[serde(default)]
        language: Option<String>,
    },
    /// Force a page break.
    PageBreak {
        id: String,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CalloutLevelName {
    Info,
    Warning,
    Important,
    Tip,
}

impl CalloutLevelName {
    fn into_level(self) -> CalloutLevel {
        match self {
            CalloutLevelName::Info => CalloutLevel::Info,
            CalloutLevelName::Warning => CalloutLevel::Warning,
            CalloutLevelName::Important => CalloutLevel::Important,
            CalloutLevelName::Tip => CalloutLevel::Tip,
        }
    }
}

impl From<CalloutLevelName> for CalloutLevel {
    fn from(level: CalloutLevelName) -> Self {
        level.into_level()
    }
}

/// Rich run input — serialised as JSON-friendly tuples. The internal
/// `FontRun` carries more fields but those are for the OOXML layer.
/// NOTE: This struct mirrors `crate::office::FontRun` for JSON compatibility
/// but uses Option<T> for all optional fields.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RichRun {
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
    #[serde(default)]
    pub highlight: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vert_align: Option<String>,  // "superscript", "subscript"
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub field: Option<crate::office::FieldRef>,
}

impl Default for RichRun {
    fn default() -> Self {
        RichRun {
            text: String::new(),
            bold: false,
            italic: false,
            underline: false,
            strikethrough: false,
            font_size: None,
            color: None,
            font_name: None,
            highlight: None,
            vert_align: None,
            field: None,
        }
    }
}

/// Style inputs the JSON caller can specify. Mirrors `components::TableStyle`
/// but uses owned `String` so it can serialise.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ContentTableStyle {
    /// Hex colour (no `#`) for the header-row background.
    #[serde(default)]
    pub header_fill: Option<String>,
    /// Hex colour for zebra-stripe rows.
    #[serde(default)]
    pub zebra_fill: Option<String>,
    /// Hex colour for cell borders.
    #[serde(default)]
    pub border_color: Option<String>,
    /// Hex colour for header-row text.
    #[serde(default)]
    pub header_text_color: Option<String>,
    /// Repeat the header row on every page the table spans.
    #[serde(default)]
    pub repeat_header: bool,
    /// Apply zebra striping to body rows.
    #[serde(default)]
    pub zebra: bool,
}

impl ContentTableStyle {
    pub(crate) fn into_table_style(self, tokens: &DesignTokens) -> TableStyle {
        TableStyle {
            header_fill: self
                .header_fill
                .or_else(|| Some(tokens.palette.primary.to_string())),
            zebra_fill: self
                .zebra_fill
                .or_else(|| Some(tokens.palette.zebra.to_string())),
            border_color: self
                .border_color
                .or_else(|| Some("DDDDDD".to_string())),
            header_text_color: self
                .header_text_color
                .or_else(|| Some(tokens.palette.text_on_primary.to_string())),
            repeat_header: self.repeat_header,
            zebra: self.zebra,
        }
    }
}

/// The rendered output: parallel lists of paragraphs and tables
/// (callouts and code-blocks are tables + paragraphs combined).
/// The caller appends them to a `WordDocument` and writes via the
/// normal `write_word_document` path.
#[derive(Debug, Default, Clone)]
pub struct RenderedDocument {
    pub paragraphs: Vec<WordParagraph>,
    pub tables: Vec<WordTable>,
    pub images: Vec<WordImage>,
}

/// Render a flat list of content blocks. Each block maps to one or
/// more paragraphs / tables / images in the output. Callouts and
/// code-blocks emit a container table that the styled writer
/// recognises (see `styled_writer.rs`).
pub fn render_blocks(blocks: &[ContentBlock], tokens: &DesignTokens) -> RenderedDocument {
    let mut out = RenderedDocument::default();
    for block in blocks {
        render_one(block, tokens, &mut out);
    }
    out
}

fn render_one(block: &ContentBlock, tokens: &DesignTokens, out: &mut RenderedDocument) {
    match block {
        ContentBlock::Cover { id, title, subtitle } => {
            out.paragraphs
                .extend(cover_title(tokens, title, subtitle.as_deref()));
            let _ = id;
        }
        ContentBlock::Chapter { id, title } => {
            out.paragraphs.extend(chapter_title(tokens, title));
            let _ = id;
        }
        ContentBlock::Heading { id, level, text } => {
            out.paragraphs.extend(heading(tokens, *level, text));
            let _ = id;
        }
        ContentBlock::Body { id, text, runs } => {
            if let Some(rs) = runs {
                out.paragraphs.extend(body_runs(tokens, id, rs));
            } else if let Some(t) = text {
                out.paragraphs.extend(body_paragraph(tokens, id, t));
            }
        }
        ContentBlock::BulletList { id_prefix, items } => {
            let strs: Vec<&str> = items.iter().map(|s| s.as_str()).collect();
            out.paragraphs
                .extend(bulleted_list(tokens, id_prefix, &strs));
        }
        ContentBlock::OrderedList { id_prefix, items } => {
            let strs: Vec<&str> = items.iter().map(|s| s.as_str()).collect();
            out.paragraphs
                .extend(ordered_list(tokens, id_prefix, &strs));
        }
        ContentBlock::Table {
            id,
            headers,
            rows,
            style,
        } => {
            let ts = style
                .clone()
                .unwrap_or_default()
                .into_table_style(tokens);
            let h: Vec<&str> = headers.iter().map(|s| s.as_str()).collect();
            let rrows: Vec<Vec<String>> = rows.clone();
            let table = styled_table(tokens, id, &h, &rrows, &ts);
            out.tables.push(table);
        }
        ContentBlock::Callout {
            id,
            level,
            title,
            body,
            body_lines,
        } => {
            let callout_level = match level {
                CalloutLevelName::Info => CalloutLevel::Info,
                CalloutLevelName::Warning => CalloutLevel::Warning,
                CalloutLevelName::Important => CalloutLevel::Important,
                CalloutLevelName::Tip => CalloutLevel::Tip,
            };
            render_callout(
                tokens,
                id,
                callout_level,
                title,
                body.as_deref(),
                body_lines.as_deref(),
                out,
            );
        }        ContentBlock::Code {
            id,
            lines,
            language,
        } => {
            render_code(tokens, id, lines, language.as_deref(), out);
        }
        ContentBlock::PageBreak { id } => {
            out.paragraphs.extend(page_break(id));
        }
    }
}

fn render_callout(
    tokens: &DesignTokens,
    id: &str,
    level: CalloutLevel,
    title: &str,
    body: Option<&str>,
    body_lines: Option<&[String]>,
    out: &mut RenderedDocument,
) {
    let rendered = if let Some(lines) = body_lines {
        let strs: Vec<&str> = lines.iter().map(|s| s.as_str()).collect();
        callout_multiline(tokens, id, level, title, &strs)
    } else if let Some(b) = body {
        callout_block(tokens, id, level, title, b)
    } else {
        return;
    };
    push_callout(out, &rendered);
}

fn render_code(
    tokens: &DesignTokens,
    id: &str,
    lines: &[String],
    language: Option<&str>,
    out: &mut RenderedDocument,
) {
    let strs: Vec<&str> = lines.iter().map(|s| s.as_str()).collect();
    let mut rendered = code_block(tokens, id, &strs);
    if let Some(lang) = language {
        // Prepend a small "lang" label line in muted text. We
        // synthesise a paragraph rather than threading another
        // parameter through the component to keep the API narrow.
        // The label is rendered at half the caption size so it reads
        // as a header strip rather than competing with the code
        // body — and we drop the leading whitespace so it lines up
        // with the rest of the code container.
        let lang_run = FontRun {
            text: lang.to_string(),
            bold: true,
            italic: false,
            underline: false,
            strikethrough: false,
            font_size: Some(tokens.fonts.caption_pt / 2),
            color: Some(tokens.palette.text_muted.to_string()),
            font_name: None,
            highlight: None,
            vert_align: None,
            field: None,
            page_break: false,
            column_break: false,
        };
        let lang_para = WordParagraph {
            id: format!("{}-lang", id),
            text: String::new(),
            style: Some("CodeBlock".to_string()),
            runs: Some(vec![lang_run]),
            numbering: None,
            alignment: Some("right".to_string()),
            text_direction: None,
            page_break: None,
        };
        rendered.paragraphs.insert(0, lang_para);
    }
    push_code(out, &rendered);
}

fn push_callout(out: &mut RenderedDocument, r: &CalloutRender) {
    // Emit a 1×1 table whose only cell contains the title + body
    // paragraphs. The single-cell table gives us a coloured
    // background and a thick left border (the "vertical bar"
    // effect readers expect).
    let cell = crate::office::shared::TableCell {
        text: String::new(),
        col_span: 1,
        row_span: 1,
    };
    let row = crate::office::shared::TableRow { cells: vec![cell] };
    // Tag the paragraphs with a style the styles.xml knows.
    let mut paragraphs = r.paragraphs.clone();
    for p in paragraphs.iter_mut() {
        if p.style.is_none() {
            p.style = Some("CalloutBody".to_string());
        }
    }
    // Build the container table with `cell_paragraphs` populated so
    // the writer can re-emit them inside the cell on subsequent saves
    // (round-trip safety). The reader also populates `cell_paragraphs`
    // for container tables via `xml_parser::parse_container_cell_paragraphs`.
    let table = WordTable {
        id: r.table_id.clone(),
        rows: vec![row],
        cell_paragraphs: paragraphs.clone(),
    };
    // Stash the visual params into the first cell using a magic
    // prefix the writer's `classify_and_strip` recognises. The
    // writer strips this prefix row before emitting real `<w:tr>` tags.
    let style_marker = format!(
        "__CALLOUT__|{}|{}",
        r.bg,
        r.accent,
    );
    let mut marker_table = table;
    marker_table.rows[0].cells[0].text = style_marker;
    // Inject a `__tbl_pos_<id>__` marker paragraph so the writer
    // finds this container in document order and emits the inner
    // paragraphs *inside* the cell. Without the marker the writer
    // would skip the callout entirely because there is no
    // `<__tbl_pos_<id>__>` paragraph pointing at it.
    let marker_para = WordParagraph {
        id: format!("__tbl_pos_{}__", r.table_id),
        text: format!("<__tbl_pos_{}__>", r.table_id),
        style: None,
        runs: None,
        numbering: None,
        alignment: None,
        text_direction: None,
        page_break: None,
    };
    out.paragraphs.push(marker_para);
    out.tables.push(marker_table);
    // Also append the inner paragraphs as body siblings so the reader
    // (which doesn't yet extract cell paragraphs back) keeps them in
    // `doc.paragraphs`. The writer's callout path consumes them via
    // `emit_callout_inner_paragraphs` (fresh-render path) when
    // `cell_paragraphs` is empty. Belt-and-braces.
    out.paragraphs.append(&mut paragraphs);
}

fn push_code(out: &mut RenderedDocument, r: &CodeBlockRender) {
    let cell = crate::office::shared::TableCell {
        text: String::new(),
        col_span: 1,
        row_span: 1,
    };
    let row = crate::office::shared::TableRow { cells: vec![cell] };
    let mut paragraphs = r.paragraphs.clone();
    let table = WordTable {
        id: r.table_id.clone(),
        rows: vec![row],
        cell_paragraphs: paragraphs.clone(),
    };
    let mut marker_table = table;
    marker_table.rows[0].cells[0].text = format!("__CODE__|{}", r.bg);
    let marker_para = WordParagraph {
        id: format!("__tbl_pos_{}__", r.table_id),
        text: format!("<__tbl_pos_{}__>", r.table_id),
        style: None,
        runs: None,
        numbering: None,
        alignment: None,
        text_direction: None,
        page_break: None,
    };
    out.paragraphs.push(marker_para);
    out.tables.push(marker_table);
    out.paragraphs.append(&mut paragraphs);
}

/// Convenience entry point: render a top-level [`DocumentContent`]
/// (which carries its own metadata) and return the rendered output.
pub fn render_document(content: &DocumentContent) -> RenderedDocument {
    let tokens = DesignTokens::default();
    render_blocks(&content.blocks, &tokens)
}

/// Top-level document content. Carries the document's metadata +
/// the list of content blocks the agent wants to emit.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentContent {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub blocks: Vec<ContentBlock>,
}

impl DocumentContent {
    /// Default-palette factory. Convenience for callers that don't
    /// care about custom palettes.
    pub fn new(blocks: Vec<ContentBlock>) -> Self {
        Self {
            title: None,
            blocks,
        }
    }
}

#[allow(dead_code)]
const _PALETTE_GUARD: PaletteGuard = PaletteGuard;
/// Compile-time anchor ensuring [`DEFAULT_PALETTE`] stays in the
/// public surface even if every call site changes.
struct PaletteGuard;

#[allow(dead_code)]
impl PaletteGuard {
    const fn new() -> Self {
        Self
    }
}

impl PaletteGuard {
    #[allow(dead_code)]
    fn _check(&self) {
        // References the constant; if it goes unused the compiler
        // will still keep it because of this const-context borrow.
        let _ = DEFAULT_PALETTE.primary;
        let _ = DesignTokens::default();
    }
}
