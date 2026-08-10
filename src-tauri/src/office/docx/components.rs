//! Component-level builders for Word document XML.
//!
//! This module is the **second layer** in the three-layer rendering
//! pipeline. Components take a [`DesignTokens`] (the visual language)
//! plus some data, and return a list of [`WordParagraph`]s / [`WordTable`]s
//! that the existing writer can splat into `document.xml`.
//!
//! ## Why component builders?
//!
//! The existing writer is "directly-mapped": a `WordParagraph` becomes a
//! `<w:p>`, a `FontRun` becomes a `<w:r>`. That's great for raw
//! fidelity but it makes brand consistency painful — every chapter has
//! to remember to set the right colour, the right border, the right
//! padding. Components encode "this is what an info callout looks
//! like" as a single function call so the brand can never drift.
//!
//! ## Component inventory
//!
//! | Component       | Function              | Returns            |
//! |-----------------|-----------------------|--------------------|
//! | Cover title     | `cover_title`         | `Vec<WordParagraph>` |
//! | Chapter title   | `chapter_title`       | `Vec<WordParagraph>` |
//! | Heading (H1/H2/H3) | `heading`         | `Vec<WordParagraph>` |
//! | Body paragraph  | `body_paragraph`      | `Vec<WordParagraph>` |
//! | Bulleted list   | `bulleted_list`       | `Vec<WordParagraph>` |
//! | Ordered list    | `ordered_list`        | `Vec<WordParagraph>` |
//! | Styled table    | `styled_table`        | `WordTable`        |
//! | Info callout    | `info_callout`        | `Vec<WordParagraph>` |
//! | Warning callout | `warning_callout`     | `Vec<WordParagraph>` |
//! | Important callout | `important_callout` | `Vec<WordParagraph>` |
//! | Tip callout     | `tip_callout`         | `Vec<WordParagraph>` |
//! | Code block      | `code_block`          | `Vec<WordParagraph>` |
//! | Page break      | `page_break`          | `Vec<WordParagraph>` |
//!
//! Every function takes `&DesignTokens` as its first argument so unit
//! tests can swap palettes without re-implementing the components.

use crate::office::docx::design_tokens::DesignTokens;
use crate::office::shared::{TableCell, TableRow};

use super::{
    FontRun, NumberingRef, WordImage, WordParagraph, WordTable,
};

// ─── Helpers ───────────────────────────────────────────────────────────────────

/// Build a single text run with the given formatting.
///
/// `color` and `font_name` accept any string-like value. We coerce
/// to `&str` at call time so the `FontRun` can hold owned `String`s
/// (which is what the rest of the writer expects).
pub(crate) fn run(
    text: impl Into<String>,
    bold: bool,
    italic: bool,
    color: Option<&str>,
    size_hp: Option<u32>,
    font_name: Option<&str>,
) -> FontRun {
    FontRun {
        text: text.into(),
        bold,
        italic,
        underline: false,
        strikethrough: false,
        font_size: size_hp,
        color: color.map(|s| s.to_string()),
        font_name: font_name.map(|s| s.to_string()),
        highlight: None,
        vert_align: None,
        field: None,
        page_break: false,
    }
}

/// Same as [`run`] but takes `String` colour / font strings directly,
/// so callers that already have a `String` (e.g. from
/// `DesignTokens::palette`) don't have to call `.as_str()` themselves.
#[allow(dead_code)]
pub(crate) fn run_owned_color(
    text: impl Into<String>,
    bold: bool,
    italic: bool,
    color: Option<String>,
    size_hp: Option<u32>,
    font_name: Option<String>,
) -> FontRun {
    FontRun {
        text: text.into(),
        bold,
        italic,
        underline: false,
        strikethrough: false,
        font_size: size_hp,
        color,
        font_name,
        highlight: None,
        vert_align: None,
        field: None,
        page_break: false,
    }
}

/// Build a paragraph carrying one inline run.
pub(crate) fn para_with_run(id: &str, r: FontRun, style: Option<&str>) -> WordParagraph {
    WordParagraph {
        id: id.to_string(),
        text: String::new(),
        style: style.map(|s| s.to_string()),
        runs: Some(vec![r]),
        numbering: None,
        alignment: None,
        text_direction: None,
    }
}

/// Build a paragraph carrying plain text (no runs).
pub(crate) fn para_with_text(id: &str, text: &str, style: Option<&str>) -> WordParagraph {
    WordParagraph {
        id: id.to_string(),
        text: text.to_string(),
        style: style.map(|s| s.to_string()),
        runs: None,
        numbering: None,
        alignment: None,
        text_direction: None,
    }
}

/// Build a blank spacer paragraph (used between components for
/// breathing room). Has empty text and no style so it inherits Normal.
pub(crate) fn blank_paragraph(id: &str) -> WordParagraph {
    WordParagraph {
        id: id.to_string(),
        text: String::new(),
        style: None,
        runs: None,
        numbering: None,
        alignment: None,
        text_direction: None,
    }
}

// ─── Cover & titles ───────────────────────────────────────────────────────────

/// Build the cover title block. The visual goal: a single, centered,
/// oversized title that fills the upper third of the cover page. We
/// emit two paragraphs — the title itself, plus a small "subtitle"
/// underneath (or a blank spacer if no subtitle).
///
/// The caller is responsible for adding the section break that splits
/// the cover page from the main body.
pub fn cover_title(tokens: &DesignTokens, title: &str, subtitle: Option<&str>) -> Vec<WordParagraph> {
    let primary: &str = tokens.palette.primary.as_str();
    let title_run = run(
        title,
        true,
        false,
        Some(primary),
        Some(tokens.fonts.cover_title_pt * 2), // half-points
        None,
    );
    let mut out = vec![para_with_run("cover-title", title_run, Some("CoverTitle"))];

    if let Some(sub) = subtitle {
        let sub_run = run(
            sub,
            false,
            true,
            Some(tokens.palette.text_muted.as_str()),
            Some(tokens.fonts.h3_pt),
            None,
        );
        out.push(para_with_run("cover-subtitle", sub_run, Some("CoverSubtitle")));
    }

    // Three blank paragraphs of breathing room before chapter content
    // begins. Could be done with `<w:spacing/>` instead but a few
    // blank paragraphs are easier to debug ("oh, there's an empty
    // paragraph hiding here").
    out.push(blank_paragraph("cover-spacer-1"));
    out.push(blank_paragraph("cover-spacer-2"));
    out
}

/// Build a chapter title — the H1 inside a chapter section. Sized
/// between the cover title and the body H2 so the visual ladder is
/// obvious.
pub fn chapter_title(tokens: &DesignTokens, text: &str) -> Vec<WordParagraph> {
    let r = run(
        text,
        true,
        false,
        Some(tokens.palette.primary.as_str()),
        Some(tokens.fonts.h1_pt),
        None,
    );
    vec![para_with_run("chapter-title", r, Some("ChapterTitle"))]
}

// ─── Headings ─────────────────────────────────────────────────────────────────

/// Build a heading at a given level. Levels: 1 (chapter), 2 (section),
/// 3 (subsection). Level 0 falls back to level 1 to keep the function
/// total.
pub fn heading(tokens: &DesignTokens, level: u8, text: &str) -> Vec<WordParagraph> {
    let (size_hp, color, style_id) = match level {
        1 => (
            tokens.fonts.h1_pt,
            tokens.palette.primary.as_str(),
            "ChapterTitle",
        ),
        2 => (
            tokens.fonts.h2_pt,
            tokens.palette.secondary.as_str(),
            "SectionTitle",
        ),
        3 | _ => (
            tokens.fonts.h3_pt,
            tokens.palette.secondary.as_str(),
            "SubsectionTitle",
        ),
    };
    let r = run(text, true, false, Some(color), Some(size_hp), None);
    vec![para_with_run(&format!("heading-l{}", level), r, Some(style_id))]
}

// ─── Body ──────────────────────────────────────────────────────────────────────

/// Build a body paragraph. The text is wrapped in a single run with
/// body colour + size so paragraphs from a Markdown import look
/// consistent without having to apply a style.
pub fn body_paragraph(tokens: &DesignTokens, id: &str, text: &str) -> Vec<WordParagraph> {
    let r = run(
        text,
        false,
        false,
        Some(tokens.palette.text.as_str()),
        Some(tokens.fonts.body_pt),
        None,
    );
    vec![para_with_run(id, r, Some("BodyParagraph"))]
}

/// Build a body paragraph with rich runs. Each tuple is
/// `(text, bold, italic)`. Colour and size come from the design
/// tokens.
pub fn body_runs(
    tokens: &DesignTokens,
    id: &str,
    runs: &[(String, bool, bool)],
) -> Vec<WordParagraph> {
    let text_color = tokens.palette.text.clone();
    let font_size = tokens.fonts.body_pt;
    let runs: Vec<FontRun> = runs
        .iter()
        .map(|(t, b, i)| {
            FontRun {
                text: t.clone(),
                bold: *b,
                italic: *i,
                underline: false,
                strikethrough: false,
                font_size: Some(font_size),
                color: Some(text_color.clone()),
                font_name: None,
                highlight: None,
                vert_align: None,
                field: None,
                page_break: false,
            }
        })
        .collect();
    vec![WordParagraph {
        id: id.to_string(),
        text: String::new(),
        style: Some("BodyParagraph".to_string()),
        runs: Some(runs),
        numbering: None,
        alignment: None,
        text_direction: None,
    }]
}

// ─── Lists ────────────────────────────────────────────────────────────────────

/// Build a bulleted list. Each item becomes a paragraph with a
/// `NumberingRef` pointing at numId 1 (the built-in bullet abstract
/// numbering from `NUMBERING_XML`). The caller passes `&[&str]` for
/// the simplest case — for items with mixed formatting, use
/// [`list_items`] directly.
pub fn bulleted_list(tokens: &DesignTokens, id_prefix: &str, items: &[&str]) -> Vec<WordParagraph> {
    let text_color = tokens.palette.text.clone();
    let font_size = tokens.fonts.body_pt;
    let paragraphs: Vec<WordParagraph> = items
        .iter()
        .enumerate()
        .map(|(i, text)| {
            let r = FontRun {
                text: text.to_string(),
                bold: false,
                italic: false,
                underline: false,
                strikethrough: false,
                font_size: Some(font_size),
                color: Some(text_color.clone()),
                font_name: None,
                highlight: None,
                vert_align: None,
                field: None,
                page_break: false,
            };
            WordParagraph {
                id: format!("{}-{}", id_prefix, i),
                text: String::new(),
                style: Some("ListBullet".to_string()),
                runs: Some(vec![r]),
                numbering: Some(NumberingRef { num_id: 1, level: 0 }),
                alignment: None,
                text_direction: None,
            }
        })
        .collect();
    paragraphs
}

/// Build an ordered (decimal-numbered) list. numId 2 is the built-in
/// decimal numbering from `NUMBERING_XML`.
pub fn ordered_list(tokens: &DesignTokens, id_prefix: &str, items: &[&str]) -> Vec<WordParagraph> {
    let text_color = tokens.palette.text.clone();
    let font_size = tokens.fonts.body_pt;
    let paragraphs: Vec<WordParagraph> = items
        .iter()
        .enumerate()
        .map(|(i, text)| {
            let r = FontRun {
                text: text.to_string(),
                bold: false,
                italic: false,
                underline: false,
                strikethrough: false,
                font_size: Some(font_size),
                color: Some(text_color.clone()),
                font_name: None,
                highlight: None,
                vert_align: None,
                field: None,
                page_break: false,
            };
            WordParagraph {
                id: format!("{}-{}", id_prefix, i),
                text: String::new(),
                style: Some("ListNumber".to_string()),
                runs: Some(vec![r]),
                numbering: Some(NumberingRef { num_id: 2, level: 0 }),
                alignment: None,
                text_direction: None,
            }
        })
        .collect();
    paragraphs
}

// ─── Tables ───────────────────────────────────────────────────────────────────

/// Visual style for a [`styled_table`]. All options default to "off"
/// so the caller only opts in to the features they want.
#[derive(Debug, Clone, Default)]
pub struct TableStyle {
    /// Hex colour (no `#`) for the header-row background. `None`
    /// leaves the row with the style's default fill.
    pub header_fill: Option<String>,
    /// Hex colour (no `#`) for the zebra-stripe background. Applied
    /// to every even-indexed body row.
    pub zebra_fill: Option<String>,
    /// Hex colour (no `#`) for the row borders. `None` keeps the
    /// style default.
    pub border_color: Option<String>,
    /// Hex colour (no `#`) for the header-row text. `None` keeps
    /// white text (good against a dark `header_fill`).
    pub header_text_color: Option<String>,
    /// When `true`, the header row repeats at the top of every page
    /// the table spans. Critical for multi-page tables — without it
    /// readers have to flip back to page 1 to remember what each
    /// column means.
    pub repeat_header: bool,
    /// When `true`, body rows use zebra striping. Composes with
    /// `zebra_fill`; ignored if `zebra_fill` is `None`.
    pub zebra: bool,
}

/// Build a styled table. The signature differs from the existing
/// raw `WordTable` builder in two important ways:
///   1. The caller passes plain strings (`headers`, `rows`), so the
///      Python-style `add_table(headers=[…], rows=…, zebra=True)`
///      ergonomics carry over.
///   2. The [`TableStyle`] struct encodes the brand-level visual
///      decisions in one place.
///
/// Internally we return a [`WordTable`] so the existing writer can
/// splice it in alongside the model-built tables.
pub fn styled_table(
    tokens: &DesignTokens,
    id: &str,
    headers: &[&str],
    rows: &[Vec<String>],
    style: &TableStyle,
) -> WordTable {
    let header_color = style
        .header_text_color
        .clone()
        .unwrap_or_else(|| tokens.palette.text_on_primary.to_string());

    // Build header row.
    let header_cells: Vec<TableCell> = headers
        .iter()
        .map(|h| TableCell::plain(h.to_string()))
        .collect();
    let header_row = TableRow { cells: header_cells };

    // Build body rows.
    let body_rows: Vec<TableRow> = rows
        .iter()
        .map(|r| TableRow {
            cells: r.iter().map(|c| TableCell::plain(c.clone())).collect(),
        })
        .collect();

    // We don't yet emit per-cell <w:shd> from this layer — that's
    // handled by `emit_table_xml` below which uses the auxiliary
    // `header_fill` / `zebra_fill` / `border_color` fields we attach
    // to the table through `aux_table_style`. See the writer glue
    // in `super::styled_writer`.
    let mut table = WordTable {
        id: id.to_string(),
        rows: vec![header_row],
        cell_paragraphs: Vec::new(),
    };
    for row in body_rows {
        table.rows.push(row);
    }
    // Stash style metadata alongside the table id so the writer
    // can pull it out at emit time. We piggy-back on the existing
    // `rows: Vec<TableRow>` rather than adding a new field, so this
    // module compiles against the current `WordTable` definition.
    table.rows.insert(
        0,
        TableRow {
            cells: encode_style_to_row(style, &header_color),
        },
    );
    table
}

/// Encode the [`TableStyle`] into the table's first row as a
/// pseudo-cell whose text is a magic prefix the writer recognises.
/// This is a deliberate, minimal-surgery workaround for the fact
/// that `WordTable` doesn't yet carry an aux-style slot. The writer
/// strips the prefix row before emitting real `<w:tr>` tags.
fn encode_style_to_row(style: &TableStyle, header_text_color: &str) -> Vec<TableCell> {
    // Encode: "__STYLE__|<header_fill>|<zebra_fill>|<border_color>|<repeat_header>|<zebra>|<header_text>"
    let payload = format!(
        "__STYLE__|{}|{}|{}|{}|{}|{}",
        style.header_fill.clone().unwrap_or_default(),
        style.zebra_fill.clone().unwrap_or_default(),
        style.border_color.clone().unwrap_or_default(),
        if style.repeat_header { "1" } else { "0" },
        if style.zebra { "1" } else { "0" },
        header_text_color,
    );
    vec![TableCell::plain(payload)]
}

/// Parse the auxiliary style row that [`styled_table`] injects.
/// Returns `None` for tables that don't carry one (i.e. tables
/// built directly from `WordTable { … }`). The writer is
/// responsible for skipping this row when emitting real `<w:tr>`s.
pub(crate) fn decode_table_style(rows: &[TableRow]) -> Option<(TableStyle, String)> {
    let first = rows.first()?;
    let first_cell_text = first.cells.first()?.text.as_str();
    if !first_cell_text.starts_with("__STYLE__|") {
        return None;
    }
    let parts: Vec<&str> = first_cell_text.split('|').collect();
    if parts.len() < 7 {
        return None;
    }
    let style = TableStyle {
        header_fill: empty_to_none(parts[1]),
        zebra_fill: empty_to_none(parts[2]),
        border_color: empty_to_none(parts[3]),
        header_text_color: empty_to_none(parts[6]),
        repeat_header: parts[4] == "1",
        zebra: parts[5] == "1",
    };
    Some((style, parts[6].to_string()))
}

fn empty_to_none(s: &str) -> Option<String> {
    if s.is_empty() { None } else { Some(s.to_string()) }
}

// ─── Callouts ────────────────────────────────────────────────────────────────

/// Render a single-line callout. Visually we use a one-row, one-column
/// table whose left border is the callout's accent colour — this gives
/// the classic "highlighted sidebar" look that reads cleanly in Word
/// and renders identically in LibreOffice / Pages.
///
/// Returns paragraphs + a marker table for the writer to splice in.
/// The caller should append the result to the document's paragraphs
/// and tables lists in order.
pub fn callout_block(
    tokens: &DesignTokens,
    id: &str,
    level: CalloutLevel,
    title: &str,
    body: &str,
) -> CalloutRender {
    let (bg, accent, label) = level_meta(level, tokens);
    let accent_ref: &str = accent.as_str();
    let text_color = tokens.palette.text.clone();
    let title_run = run(
        format!("{} {}", label, title),
        true,
        false,
        Some(accent_ref),
        Some(tokens.fonts.body_strong_pt),
        None,
    );
    let body_run = run(
        body,
        false,
        false,
        Some(text_color.as_str()),
        Some(tokens.fonts.body_pt),
        None,
    );
    let p_title = WordParagraph {
        id: format!("{}-title", id),
        text: String::new(),
        style: Some("CalloutBody".to_string()),
        runs: Some(vec![title_run]),
        numbering: None,
        alignment: None,
        text_direction: None,
    };
    let p_body = WordParagraph {
        id: format!("{}-body", id),
        text: String::new(),
        style: Some("CalloutBody".to_string()),
        runs: Some(vec![body_run]),
        numbering: None,
        alignment: None,
        text_direction: None,
    };
    CalloutRender {
        paragraphs: vec![p_title, p_body],
        table_id: format!("{}-box", id),
        bg,
        accent,
    }
}

/// Variant of [`callout_block`] for multi-line bodies. Splits `body`
/// on `\n` and renders each line as its own paragraph inside the
/// callout's color frame.
pub fn callout_multiline(
    tokens: &DesignTokens,
    id: &str,
    level: CalloutLevel,
    title: &str,
    body_lines: &[&str],
) -> CalloutRender {
    let (bg, accent, label) = level_meta(level, tokens);
    let accent_ref: &str = accent.as_str();
    let text_color = tokens.palette.text.clone();
    let title_run = run(
        format!("{} {}", label, title),
        true,
        false,
        Some(accent_ref),
        Some(tokens.fonts.body_strong_pt),
        None,
    );
    let p_title = WordParagraph {
        id: format!("{}-title", id),
        text: String::new(),
        style: Some("CalloutBody".to_string()),
        runs: Some(vec![title_run]),
        numbering: None,
        alignment: None,
        text_direction: None,
    };
    let mut paragraphs = vec![p_title];
    for (i, line) in body_lines.iter().enumerate() {
        let r = run(
            *line,
            false,
            false,
            Some(text_color.as_str()),
            Some(tokens.fonts.body_pt),
            None,
        );
        paragraphs.push(WordParagraph {
            id: format!("{}-body-{}", id, i),
            text: String::new(),
            style: Some("CalloutBody".to_string()),
            runs: Some(vec![r]),
            numbering: None,
            alignment: None,
            text_direction: None,
        });
    }
    CalloutRender {
        paragraphs,
        table_id: format!("{}-box", id),
        bg,
        accent,
    }
}

/// Visual style for the four callout flavours. Mirrors the design
/// doc's colour map.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CalloutLevel {
    Info,
    Warning,
    Important,
    Tip,
}

fn level_meta(level: CalloutLevel, t: &DesignTokens) -> (String, String, &'static str) {
    match level {
        CalloutLevel::Info => (
            t.palette.callout_info_bg.to_string(),
            t.palette.secondary.to_string(),
            "💡 Info",
        ),
        CalloutLevel::Warning => (
            t.palette.callout_warning_bg.to_string(),
            t.palette.accent.to_string(),
            "⚠ Warning",
        ),
        CalloutLevel::Important => (
            t.palette.callout_important_bg.to_string(),
            "C0504D".to_string(),
            "❗ Important",
        ),
        CalloutLevel::Tip => (
            t.palette.callout_tip_bg.to_string(),
            t.palette.primary.to_string(),
            "✓ Tip",
        ),
    }
}

/// Concrete callout render result. The caller appends
/// `paragraphs` to the document body and `table_id` / `bg` / `accent`
/// to its tables list — see `super::styled_writer::emit_callout` for
/// the glue that emits the coloured-bordered container table.
#[derive(Debug, Clone)]
pub struct CalloutRender {
    pub paragraphs: Vec<WordParagraph>,
    pub table_id: String,
    pub bg: String,
    pub accent: String,
}

// ─── Code block ───────────────────────────────────────────────────────────────

/// Build a code block. Returns paragraphs (one per line) plus the
/// background colour so the writer can wrap them in a single-cell
/// table to get the shaded background.
pub fn code_block(
    tokens: &DesignTokens,
    id: &str,
    lines: &[&str],
) -> CodeBlockRender {
    let text_color = tokens.palette.text.clone();
    let font_size = tokens.fonts.body_pt;
    let paragraphs: Vec<WordParagraph> = lines
        .iter()
        .enumerate()
        .map(|(i, line)| {
            let r = FontRun {
                text: line.to_string(),
                bold: false,
                italic: false,
                underline: false,
                strikethrough: false,
                font_size: Some(font_size),
                color: Some(text_color.clone()),
                // Monospace fallback chain. Word will pick the first
                // one that's installed on the reader's machine.
                font_name: Some("Consolas, Menlo, 'DejaVu Sans Mono', monospace".to_string()),
                highlight: None,
                vert_align: None,
                field: None,
                page_break: false,
            };
            WordParagraph {
                id: format!("{}-{}", id, i),
                text: String::new(),
                style: Some("CodeBlock".to_string()),
                runs: Some(vec![r]),
                numbering: None,
                alignment: None,
                text_direction: None,
            }
        })
        .collect();
    CodeBlockRender {
        paragraphs,
        table_id: format!("{}-box", id),
        bg: tokens.palette.code_bg.clone(),
    }
}

/// Concrete code-block render result.
#[derive(Debug, Clone)]
pub struct CodeBlockRender {
    pub paragraphs: Vec<WordParagraph>,
    pub table_id: String,
    pub bg: String,
}

// ─── Page break ───────────────────────────────────────────────────────────────

/// Build a paragraph carrying a hard page break. Useful for
/// forcing a chapter to start on a fresh page without setting up a
/// new section.
pub fn page_break(id: &str) -> Vec<WordParagraph> {
    vec![WordParagraph {
        id: id.to_string(),
        text: String::new(),
        style: None,
        runs: Some(vec![FontRun {
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
            page_break: true,
        }]),
        numbering: None,
        alignment: None,
        text_direction: None,
    }]
}

#[allow(dead_code)]
fn _phantom(_t: &DesignTokens, _img: &WordImage) {
    // Anchor used to keep the WordImage import alive while we don't
    // yet emit images from the component layer. Remove once image
    // support is wired through.
    let _ = _t;
    let _ = _img;
}
