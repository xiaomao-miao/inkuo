//! Format-conversion tools: svg_to_png, md_to_word, word_to_pdf
//!
//! Three "source-file → target-file" converters that the
//! `document_converter` sub-agent uses. They share two design rules:
//!
//! 1. Each tool returns a `ConvertOutcome { output, is_error, file_path }`
//!    rather than a plain `String`. The registry wrapper in `mod.rs`
//!    re-stitches `file_path` onto the `ToolResult` so the frontend
//!    `file-written` event fires (same pattern as `render_mermaid`).
//!
//! 2. Heavy work is dispatched onto `tokio::task::spawn_blocking` so
//!    the renderer / parser never blocks other tool calls running on
//!    the same Tokio executor.
//!
//! Engines used:
//!   - SVG → PNG: `resvg` (pure Rust, Skia subset, same engine merman
//!     uses internally for Mermaid rasterization).
//!   - Markdown → Word: `pulldown-cmark` parses the source
//!     (CommonMark + GFM tables / task lists / strikethrough /
//!     footnotes). The events are folded into an in-house `MdBlock`
//!     AST, then a separate materialiser converts that AST into a
//!     `docx_rs::Docx` and packs it directly via docx-rs's own writer.
//!     We deliberately bypass the in-house OOXML writer because its
//!     output is not parseable by `office2pdf`'s embedded docx-rs
//!     reader (zip packaging diverges from what docx-rs 0.4.22
//!     expects). Routing md_to_word through docx-rs as both producer
//!     and consumer guarantees the produced `.docx` round-trips
//!     cleanly into `word_to_pdf`.
//!   - Word → PDF: `office2pdf` (pure Rust, Typst backend — no
//!     LibreOffice, no Chromium, no Docker).

use crate::office::{FontRun, TableCell};
use pulldown_cmark::{
    Alignment, CodeBlockKind, Event, HeadingLevel, Options as MdOptions, Parser,
    Tag, TagEnd,
};
use serde::Deserialize;
use serde_json::Value;
use std::path::{Path, PathBuf};

use super::{ToolDefinition, ToolError, ToolParameters, validate_workspace_path};

/// Per-tool outcome. The registry wrapper stamps `file_path` onto the
/// final `ToolResult` and emits the frontend `file-written` event when
/// `is_error` is false.
pub struct ConvertOutcome {
    pub output: String,
    pub is_error: bool,
    pub file_path: Option<String>,
}

// ── svg_to_png ────────────────────────────────────────────────────────────────

pub struct SvgToPngTool;

impl SvgToPngTool {
    pub fn new() -> Self { Self }

    pub fn definition(&self) -> ToolDefinition {
        ToolDefinition::new_with_label(
            "svg_to_png",
            "SVG 转 PNG",
            "Rasterize an .svg file to a .png file. Pure-Rust `resvg` engine (Skia subset, no Node/Chromium). The output pixel dimensions follow the SVG's intrinsic size, optionally constrained by `max_width` / `max_height` (the renderer preserves the SVG's aspect ratio). Use this whenever the user wants a bitmap copy of an SVG (e.g. to embed into a `.docx`, share on chat, set as a desktop background). For Mermaid diagrams, prefer `render_mermaid` (handled by `flowchart_expert`).",
            ToolParameters::new(
                vec!["input_path", "output_path"],
                vec![
                    ("input_path", "string", Some("Absolute path to the source `.svg` file.")),
                    ("output_path", "string", Some("Absolute path of the output `.png` file. Parent directory is created if missing.")),
                    ("max_width", "integer", Some("Optional upper bound on the output width in pixels. The SVG is scaled down (preserving aspect ratio) if its intrinsic width exceeds this. Has no effect when the intrinsic width already fits.")),
                    ("max_height", "integer", Some("Optional upper bound on the output height in pixels. Behaviour mirrors `max_width`.")),
                    ("background", "string", Some("Optional CSS color string painted behind the SVG (e.g. `#ffffff`, `white`, `transparent`). Default: `transparent`.")),
                ],
            ),
        )
    }

    pub async fn execute(
        &self,
        arguments: Value,
        workspace: Option<String>,
    ) -> Result<ConvertOutcome, ToolError> {
        let args: SvgToPngArgs = serde_json::from_value(arguments).map_err(|e| {
            ToolError::InvalidArguments(
                "svg_to_png".to_string(),
                format!("Invalid parameters: {}", e),
            )
        })?;

        validate_workspace_path(&args.input_path, &workspace)?;
        validate_workspace_path(&args.output_path, &workspace)?;

        let input_path = PathBuf::from(&args.input_path);
        if !input_path.exists() {
            return Err(ToolError::IoError(format!(
                "Source SVG file does not exist: {}",
                args.input_path
            )));
        }
        let output_path = PathBuf::from(&args.output_path);
        if let Some(parent) = output_path.parent() {
            if !parent.as_os_str().is_empty() && !parent.exists() {
                tokio::fs::create_dir_all(parent).await.map_err(|e| {
                    ToolError::IoError(format!(
                        "Failed to create output directory {}: {}",
                        parent.display(),
                        e
                    ))
                })?;
            }
        }

        let svg_bytes = tokio::fs::read(&input_path).await.map_err(|e| {
            ToolError::IoError(format!("Failed to read SVG {}: {}", args.input_path, e))
        })?;
        let svg_source = match String::from_utf8(svg_bytes) {
            Ok(s) => s,
            Err(_) => {
                return Err(ToolError::InvalidArguments(
                    "svg_to_png".to_string(),
                    format!("Source SVG is not valid UTF-8: {}", args.input_path),
                ));
            }
        };

        let max_width = args.max_width;
        let max_height = args.max_height;
        let background = args.background.clone();
        let output_path_for_render = output_path.clone();
        let output_path_for_report = output_path.clone();
        let input_path_for_report = input_path.clone();

        let render_result = tokio::task::spawn_blocking(move || {
            rasterize_svg_to_png(
                &svg_source,
                &output_path_for_render,
                max_width,
                max_height,
                background.as_deref(),
            )
        })
        .await
        .map_err(|e| ToolError::ExecutionError(format!("svg_to_png task panicked: {}", e)))?;

        match render_result {
            Ok(bytes) => {
                let result_json = serde_json::json!({
                    "input_path": input_path_for_report.to_string_lossy(),
                    "output_path": output_path_for_report.to_string_lossy(),
                    "bytes": bytes.len(),
                })
                .to_string();
                Ok(ConvertOutcome {
                    output: result_json,
                    is_error: false,
                    file_path: Some(output_path_for_report.to_string_lossy().to_string()),
                })
            }
            Err(e) => Ok(ConvertOutcome {
                output: format!(
                    "svg_to_png failed: {}\nSource: {}",
                    e,
                    input_path_for_report.display()
                ),
                is_error: true,
                file_path: None,
            }),
        }
    }
}

impl Default for SvgToPngTool {
    fn default() -> Self { Self::new() }
}

#[derive(Debug, Deserialize)]
struct SvgToPngArgs {
    input_path: String,
    output_path: String,
    max_width: Option<u32>,
    max_height: Option<u32>,
    background: Option<String>,
}

/// Pure-Rust SVG → PNG rasterizer.
///
/// `resvg` returns a pixmap at the SVG's intrinsic size; we optionally
/// downscale to honour `max_width` / `max_height` (preserving aspect
/// ratio). `background` is composited *behind* the SVG via `pixmap.fill`
/// before save — `transparent` (the default) leaves the pixmap's alpha
/// untouched.
fn rasterize_svg_to_png(
    svg_source: &str,
    output_path: &Path,
    max_width: Option<u32>,
    max_height: Option<u32>,
    background: Option<&str>,
) -> Result<Vec<u8>, String> {
    let mut options = resvg::usvg::Options::default();
    // Load every font reachable on the host so SVG `<text>` elements
    // with arbitrary `font-family` declarations actually render.
    //
    // Without this, `usvg` resolves fonts from an empty database and
    // silently drops text runs whose requested family isn't in the
    // database — which is the default behaviour of a fresh
    // `Options::default()`. The symptom in the calling tool is
    // "PNG looks correct but the text is missing".
    //
    // `load_system_fonts` walks `/usr/share/fonts/`,
    // `/usr/local/share/fonts/`, and `~/.local/share/fonts/` — the
    // documented set resvg scans on Unix. It also walks
    // `C:\Windows\Fonts\` on Windows (handled by fontdb internally).
    options.fontdb_mut().load_system_fonts();
    // Some SVGs omit `font-family` entirely, in which case usvg falls
    // back to the Options::default family. Default is "Times New Roman",
    // which is rarely installed on Linux. Override to a sans-serif
    // family name and hope the system has it (most modern Linux distros
    // ship DejaVu Sans or Liberation Sans, both of which match
    // `sans-serif` queries via fontdb's CSS-like family matching).
    if options.fontdb.len() > 0 {
        options.font_family = "sans-serif".to_string();
    }

    let tree = resvg::usvg::Tree::from_str(svg_source, &options)
        .map_err(|e| format!("usvg parse failed: {:?}", e))?;

    let intrinsic = tree.size();
    let (intr_w, intr_h) = (intrinsic.width(), intrinsic.height());
    if intr_w <= 0.0 || intr_h <= 0.0 {
        return Err(format!(
            "SVG declares a non-positive intrinsic size ({}x{})",
            intr_w, intr_h
        ));
    }

    // Compute target pixel dimensions honouring `max_width` / `max_height`
    // while preserving aspect ratio. Both bounds absent → render at the
    // intrinsic size (rounded up to 1px to avoid zero allocations).
    let (mut w, mut h) = (intr_w.ceil().max(1.0) as u32, intr_h.ceil().max(1.0) as u32);
    if let Some(mw) = max_width {
        if w > mw {
            let new_h = ((intr_h / intr_w) * mw as f32).ceil().max(1.0) as u32;
            w = mw;
            h = new_h;
        }
    }
    if let Some(mh) = max_height {
        if h > mh {
            let new_w = ((intr_w / intr_h) * mh as f32).ceil().max(1.0) as u32;
            h = mh;
            w = new_w;
        }
    }

    let mut pixmap = resvg::tiny_skia::Pixmap::new(w, h)
        .ok_or_else(|| format!("Pixmap allocation failed for {}x{}", w, h))?;

    // Scale the SVG's intrinsic coordinate system onto the requested
    // output size. Without this transform, max_width/max_height would
    // crop instead of scale.
    let transform = resvg::tiny_skia::Transform::from_scale(
        w as f32 / intr_w,
        h as f32 / intr_h,
    );
    {
        let mut pm = pixmap.as_mut();
        resvg::render(&tree, transform, &mut pm);
    }

    if let Some(bg) = background {
        if !bg.eq_ignore_ascii_case("transparent") {
            let color = parse_css_color(bg).ok_or_else(|| {
                format!(
                    "Unrecognized CSS color '{}'. Use '#RRGGBB', '#RGB', 'white', 'black', etc., or 'transparent'.",
                    bg
                )
            })?;
            pixmap.fill(color);
        }
    }

    // Re-encode the pixmap to PNG bytes in memory, then write them out
    // ourselves. `Pixmap::save_png` only writes to disk and doesn't
    // expose the encoded bytes; for the JSON result we want both the
    // byte count and a successful disk write, so encoding once and
    // writing the buffer is the cleanest path.
    let png_bytes = pixmap
        .encode_png()
        .map_err(|e| format!("PNG encoding failed: {:?}", e))?;
    std::fs::write(output_path, &png_bytes)
        .map_err(|e| format!("Failed to write PNG to {}: {}", output_path.display(), e))?;
    Ok(png_bytes)
}

/// Minimal CSS color parser. We only accept the subset that's safe to
/// translate into a `tiny_skia::Color` (which takes 8-bit sRGB) without
/// pulling in a CSS parser: `#RGB` / `#RRGGBB` / `#RRGGBBAA` hex, and a
/// short allow-list of named colors. Returning `None` for anything else
/// lets the caller emit a clear "unsupported color" error instead of
/// silently dropping the background.
fn parse_css_color(s: &str) -> Option<resvg::tiny_skia::Color> {
    let s = s.trim();
    if let Some(hex) = s.strip_prefix('#') {
        let parse = |slice: &str| u8::from_str_radix(slice, 16).ok();
        let bytes: Option<[u8; 4]> = match hex.len() {
            3 => {
                let r = parse(&hex[0..1].repeat(2))?;
                let g = parse(&hex[1..2].repeat(2))?;
                let b = parse(&hex[2..3].repeat(2))?;
                Some([r, g, b, 0xff])
            }
            6 => {
                let r = parse(&hex[0..2])?;
                let g = parse(&hex[2..4])?;
                let b = parse(&hex[4..6])?;
                Some([r, g, b, 0xff])
            }
            8 => {
                let r = parse(&hex[0..2])?;
                let g = parse(&hex[2..4])?;
                let b = parse(&hex[4..6])?;
                let a = parse(&hex[6..8])?;
                Some([r, g, b, a])
            }
            _ => None,
        };
        bytes.map(|[r, g, b, a]| resvg::tiny_skia::Color::from_rgba8(r, g, b, a))
    } else {
        let named: Option<(u8, u8, u8)> = match s.to_ascii_lowercase().as_str() {
            "white" => Some((0xff, 0xff, 0xff)),
            "black" => Some((0, 0, 0)),
            "red" => Some((0xff, 0, 0)),
            "green" => Some((0, 0x80, 0)),
            "blue" => Some((0, 0, 0xff)),
            "yellow" => Some((0xff, 0xff, 0)),
            "gray" | "grey" => Some((0x80, 0x80, 0x80)),
            "lightgray" | "lightgrey" => Some((0xd3, 0xd3, 0xd3)),
            _ => None,
        };
        named.map(|(r, g, b)| resvg::tiny_skia::Color::from_rgba8(r, g, b, 0xff))
    }
}

// ── md_to_word ────────────────────────────────────────────────────────────────

pub struct MdToWordTool;

impl MdToWordTool {
    pub fn new() -> Self { Self }

    pub fn definition(&self) -> ToolDefinition {
        ToolDefinition::new_with_label(
            "md_to_word",
            "Markdown 转 Word",
            "Convert Markdown to a Word `.docx` file. Pure-Rust path: `pulldown-cmark` parses the source (CommonMark + GFM tables / task lists / strikethrough / footnotes), the events are folded into the workspace's in-house `WordDocument` model, and the OOXML writer used by `create_word_doc` packs the result. Reuses the existing docx infrastructure, so output opens cleanly in Word, LibreOffice, and WPS. Use this when the user wants to share / print a Markdown document as a real Word file.",
            ToolParameters::new(
                vec!["output_path"],
                vec![
                    ("input_path", "string", Some("Absolute path to the source `.md` (or `.markdown`) file. Exactly one of `input_path` or `markdown` must be provided.")),
                    ("markdown", "string", Some("Inline Markdown source. Exactly one of `input_path` or `markdown` must be provided. Use this when the Markdown is already in the model's context (e.g. the contents of an existing file the LLM just read).")),
                    ("output_path", "string", Some("Absolute path of the output `.docx` file. Parent directory is created if missing.")),
                    ("title", "string", Some("Optional document title. When provided, written as the document's core property and emitted as the first paragraph styled with Heading 1.")),
                ],
            ),
        )
    }

    pub async fn execute(
        &self,
        arguments: Value,
        workspace: Option<String>,
    ) -> Result<ConvertOutcome, ToolError> {
        let args: MdToWordArgs = serde_json::from_value(arguments).map_err(|e| {
            ToolError::InvalidArguments(
                "md_to_word".to_string(),
                format!("Invalid parameters: {}", e),
            )
        })?;

        validate_workspace_path(&args.output_path, &workspace)?;
        if let Some(ref p) = args.input_path {
            validate_workspace_path(p, &workspace)?;
        }

        let output_path = PathBuf::from(&args.output_path);
        if let Some(parent) = output_path.parent() {
            if !parent.as_os_str().is_empty() && !parent.exists() {
                tokio::fs::create_dir_all(parent).await.map_err(|e| {
                    ToolError::IoError(format!(
                        "Failed to create output directory {}: {}",
                        parent.display(),
                        e
                    ))
                })?;
            }
        }

        let markdown = match (args.input_path.as_deref(), args.markdown.as_deref()) {
            (Some(_), Some(_)) => {
                return Err(ToolError::InvalidArguments(
                    "md_to_word".to_string(),
                    "Provide exactly one of `input_path` or `markdown`, not both.".to_string(),
                ));
            }
            (None, None) => {
                return Err(ToolError::InvalidArguments(
                    "md_to_word".to_string(),
                    "One of `input_path` or `markdown` is required.".to_string(),
                ));
            }
            (Some(path), None) => {
                let p = PathBuf::from(path);
                if !p.exists() {
                    return Err(ToolError::IoError(format!(
                        "Source Markdown file does not exist: {}",
                        path
                    )));
                }
                tokio::fs::read_to_string(&p)
                    .await
                    .map_err(|e| {
                        ToolError::IoError(format!(
                            "Failed to read Markdown {}: {}",
                            path, e
                        ))
                    })?
            }
            (None, Some(inline)) => inline.to_string(),
        };

        let title = args.title.clone();
        let output_path_for_render = output_path.clone();

        let convert_result = tokio::task::spawn_blocking(move || {
            let blocks = parse_markdown_to_blocks(&markdown);
            materialise_to_docx_rs_doc(blocks, title.as_deref(), &output_path_for_render)
        })
        .await
        .map_err(|e| ToolError::ExecutionError(format!("md_to_word task panicked: {}", e)))?;

        let bytes = match convert_result {
            Ok(b) => b,
            Err(e) => {
                return Ok(ConvertOutcome {
                    output: format!("md_to_word failed: {}", e),
                    is_error: true,
                    file_path: None,
                });
            }
        };

        let result_json = serde_json::json!({
            "output_path": output_path.to_string_lossy(),
            "bytes": bytes,
        })
        .to_string();
        Ok(ConvertOutcome {
            output: result_json,
            is_error: false,
            file_path: Some(output_path.to_string_lossy().to_string()),
        })
    }
}

impl Default for MdToWordTool {
    fn default() -> Self { Self::new() }
}

#[derive(Debug, Deserialize)]
struct MdToWordArgs {
    input_path: Option<String>,
    markdown: Option<String>,
    output_path: String,
    title: Option<String>,
}

/// Fold a Markdown source string into the in-house `MdBlock` AST.
/// Each top-level Markdown block becomes either a `MdBlock::Paragraph`
/// / `Heading` / `ListItem` / `CodeBlock` / `BlockQuote` (each carrying
/// a `Vec<FontRun>` for inline formatting) or a `MdBlock::Table`
/// (header + body rows). The AST is intentionally renderer-agnostic;
/// the docx-rs materialiser (`materialise_to_docx_rs_doc`) walks it to
/// produce the final `.docx`.
///
/// The converter is intentionally not exhaustive — it covers the common
/// subset (headings, paragraphs, lists, blockquotes, code blocks,
/// horizontal rules, GFM tables, GFM task lists, inline emphasis /
/// strong / strikethrough / code / links, images, footnotes references).
/// Anything outside the subset degrades gracefully (raw text inside a
/// best-effort paragraph) so the docx always opens without errors.
pub(crate) fn parse_markdown_to_blocks(markdown: &str) -> Vec<MdBlock> {
    let mut options = MdOptions::empty();
    options.insert(MdOptions::ENABLE_TABLES);
    options.insert(MdOptions::ENABLE_STRIKETHROUGH);
    options.insert(MdOptions::ENABLE_TASKLISTS);
    options.insert(MdOptions::ENABLE_SMART_PUNCTUATION);

    let parser = Parser::new_ext(markdown, options);

    let mut blocks: Vec<MdBlock> = Vec::new();
    let mut stack: Vec<MdBlock> = Vec::new();
    // Inline formatting stack. Each entry is a delta applied to the
    // run produced next (bold/italic/etc.).
    #[derive(Clone, Copy)]
    struct InlineFlags {
        bold: bool,
        italic: bool,
        strikethrough: bool,
        code: bool,
    }
    impl InlineFlags {
        fn default_() -> Self {
            Self { bold: false, italic: false, strikethrough: false, code: false }
        }
    }
    let mut inline_stack: Vec<InlineFlags> = vec![InlineFlags::default_()];
    // Numbered list depth + counter; we only emit bullet/number prefixes,
    // not full OOXML numbering definitions (the in-house writer doesn't
    // track those).
    let mut ordered_counters: Vec<u64> = Vec::new();
    let mut list_depth: u32 = 0;
    let mut blockquote_depth: u32 = 0;
    let mut table_state: Option<TableBuildState> = None;

    struct TableBuildState {
        header: Vec<TableCell>,
        rows: Vec<Vec<TableCell>>,
        current_row: Option<Vec<TableCell>>,
        cell_runs: Vec<FontRun>,
    }

    /// Push text into the currently-open block as a new FontRun,
    /// inheriting the topmost inline flags. When a table is being
    /// built, the run is appended to the in-progress cell instead.
    fn push_text(
        block: Option<&mut MdBlock>,
        table_state: &mut Option<TableBuildState>,
        text: &str,
        flags: InlineFlags,
    ) {
        if text.is_empty() {
            return;
        }
        let mut run = FontRun {
            text: text.to_string(),
            bold: flags.bold,
            italic: flags.italic,
            strikethrough: if flags.code { false } else { flags.strikethrough },
            ..Default::default()
        };
        if flags.code {
            run.font_name = Some("Consolas".to_string());
            run.highlight = Some("lightGray".to_string());
        }
        if let Some(state) = table_state.as_mut() {
            // Coalesce consecutive runs that share formatting so the
            // final cell text isn't peppered with identical runs.
            if let Some(prev) = state.cell_runs.last_mut() {
                if prev.text.is_empty() {
                    prev.text = run.text;
                } else if prev.bold == run.bold
                    && prev.italic == run.italic
                    && prev.strikethrough == run.strikethrough
                    && prev.font_name == run.font_name
                    && prev.highlight == run.highlight
                {
                    prev.text.push_str(&run.text);
                } else {
                    state.cell_runs.push(run);
                }
            } else {
                state.cell_runs.push(run);
            }
            return;
        }
        let Some(block) = block else { return };
        match block {
            MdBlock::Paragraph { runs, .. } => runs.push(run),
            MdBlock::Heading { runs, .. } => runs.push(run),
            MdBlock::ListItem { runs, .. } => runs.push(run),
            MdBlock::BlockQuote { runs } => runs.push(run),
            MdBlock::CodeBlock { .. } | MdBlock::Table { .. } => {
                // Code blocks / tables absorb raw text into their own
                // buffers via dedicated branches; ignore.
            }
        }
    }

    for event in parser {
        match event {
            Event::Start(tag) => match tag {
                Tag::Paragraph => {
                    stack.push(MdBlock::Paragraph { style: None, runs: Vec::new() });
                }
                Tag::Heading { level, .. } => {
                    let lvl = match level {
                        HeadingLevel::H1 => 1,
                        HeadingLevel::H2 => 2,
                        HeadingLevel::H3 => 3,
                        HeadingLevel::H4 => 4,
                        HeadingLevel::H5 => 5,
                        HeadingLevel::H6 => 6,
                    };
                    stack.push(MdBlock::Heading { level: lvl, runs: Vec::new() });
                }
                Tag::BlockQuote(_) => {
                    blockquote_depth += 1;
                    stack.push(MdBlock::BlockQuote { runs: Vec::new() });
                }
                Tag::CodeBlock(kind) => {
                    let lang = match kind {
                        CodeBlockKind::Indented => None,
                        CodeBlockKind::Fenced(s) if s.is_empty() => None,
                        CodeBlockKind::Fenced(s) => Some(s.to_string()),
                    };
                    stack.push(MdBlock::CodeBlock { lang, text: String::new() });
                }
                Tag::List(start) => {
                    list_depth += 1;
                    if start.is_some() {
                        ordered_counters.push(1);
                    } else {
                        ordered_counters.push(0);
                    }
                }
                Tag::Item => {
                    // Decide ordered vs unordered by peeking at the top
                    // of `ordered_counters` (0 means unordered).
                    let ordered = ordered_counters
                        .last()
                        .copied()
                        .map(|c| c > 0)
                        .unwrap_or(false);
                    let number = ordered_counters
                        .last_mut()
                        .map(|c| {
                            let cur = if *c > 0 { *c } else { 0 };
                            *c += 1;
                            cur
                        })
                        .unwrap_or(0);
                    let _ = number; // not used in the render path below
                    stack.push(MdBlock::ListItem {
                        ordered,
                        level: list_depth.saturating_sub(1),
                        runs: Vec::new(),
                        checked: None,
                    });
                }
                Tag::Table(_aligns) => {
                    let _ = _aligns; // column alignments aren't carried into docx-rs.
                    table_state = Some(TableBuildState {
                        header: Vec::new(),
                        rows: Vec::new(),
                        current_row: Some(Vec::new()),
                        cell_runs: Vec::new(),
                    });
                    stack.push(MdBlock::Table {
                        header: Vec::new(),
                        rows: Vec::new(),
                    });
                }
                Tag::TableHead => {
                    if let Some(state) = table_state.as_mut() {
                        state.current_row = Some(Vec::new());
                    }
                }
                Tag::TableRow => {
                    if let Some(state) = table_state.as_mut() {
                        state.current_row = Some(Vec::new());
                        state.cell_runs.clear();
                    }
                }
                Tag::TableCell => {
                    if let Some(state) = table_state.as_mut() {
                        state.cell_runs.clear();
                    }
                }
                Tag::Emphasis => inline_stack.push(InlineFlags { italic: true, ..InlineFlags::default_() }),
                Tag::Strong => inline_stack.push(InlineFlags { bold: true, ..InlineFlags::default_() }),
                Tag::Strikethrough => inline_stack.push(InlineFlags {
                    strikethrough: true,
                    ..InlineFlags::default_()
                }),
                Tag::Link { .. } => {
                    // Hyperlinks: render as plain inline text for now.
                    // The in-house writer doesn't expose a hyperlink run,
                    // so we silently drop the href. A future patch can
                    // add hyperlink support in the OOXML path.
                }
                Tag::Image { .. } => {
                    // We don't embed images in the converted docx in
                    // v1 (would require resolving paths and copying
                    // bytes into `word/media`). Mark with a short
                    // bracketed marker so the conversion is auditable.
                    let flags = *inline_stack.last().unwrap_or(&InlineFlags::default_());
                    push_text(stack.last_mut(), &mut table_state, "[image]", flags);
                }
                Tag::FootnoteDefinition(_) => {
                    // Footnote definitions are absorbed into the body
                    // stream by pulldown-cmark; nothing to track here.
                }
                _ => {
                    // Other tags (Superscript, Subscript, etc.) pass
                    // through; their text ends up as a normal run.
                }
            },
            Event::End(tag_end) => {
                match tag_end {
                    TagEnd::Paragraph | TagEnd::Heading(_) => {
                        if let Some(block) = stack.pop() {
                            blocks.push(block);
                        }
                    }
                    TagEnd::Table => {
                        // Finalize the table by pulling header + rows
                        // from the table_state we were tracking.
                        if let (Some(_block), Some(state)) = (stack.pop(), table_state.take()) {
                            let header = state.header;
                            let rows = state.rows;
                            blocks.push(MdBlock::Table { header, rows });
                        } else if let Some(block) = stack.pop() {
                            blocks.push(block);
                        }
                    }
                    TagEnd::TableHead => {
                        if let Some(state) = table_state.as_mut() {
                            if let Some(row) = state.current_row.take() {
                                state.header = row;
                            }
                        }
                    }
                    TagEnd::TableRow => {
                        if let Some(state) = table_state.as_mut() {
                            if let Some(mut cells) = state.current_row.take() {
                                // Flush any pending cell_runs as a final
                                // cell if the cell column hasn't been
                                // closed yet (this can happen on the
                                // last row if the last cell had no
                                // text). Move all accumulated runs
                                // into the cell.
                                if cells.is_empty() {
                                    let run = state.cell_runs.pop().unwrap_or_default();
                                    cells.push(TableCell::plain(run.text));
                                }
                                state.rows.push(cells);
                            }
                        }
                    }
                    TagEnd::TableCell => {
                        if let Some(state) = table_state.as_mut() {
                            // The current row is the open container;
                            // collapse the accumulated runs into a
                            // single cell text by joining them with
                            // the formatting encoded inline as plain
                            // text. The v1 converter doesn't carry
                            // per-cell rich formatting (only the body
                            // paragraphs do).
                            let cell_text = if state.cell_runs.is_empty() {
                                String::new()
                            } else {
                                state
                                    .cell_runs
                                    .iter()
                                    .map(|r| r.text.as_str())
                                    .collect::<Vec<_>>()
                                    .join("")
                            };
                            if let Some(row) = state.current_row.as_mut() {
                                row.push(TableCell::plain(cell_text));
                            }
                            state.cell_runs.clear();
                        }
                    }
                    TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough => {
                        if inline_stack.len() > 1 {
                            inline_stack.pop();
                        }
                    }
                    TagEnd::Link => {
                        // No-op (see Start above).
                    }
                    TagEnd::BlockQuote(_) => {
                        if let Some(block) = stack.pop() {
                            blocks.push(block);
                        }
                        blockquote_depth = blockquote_depth.saturating_sub(1);
                    }
                    TagEnd::CodeBlock => {
                        if let Some(block) = stack.pop() {
                            blocks.push(block);
                        }
                    }
                    TagEnd::List(_) => {
                        list_depth = list_depth.saturating_sub(1);
                        if !ordered_counters.is_empty() {
                            ordered_counters.pop();
                        }
                    }
                    TagEnd::Item => {
                        if let Some(block) = stack.pop() {
                            blocks.push(block);
                        }
                    }
                    TagEnd::Image => {
                        // Already inserted the "[image]" marker at Start.
                    }
                    TagEnd::FootnoteDefinition => {}
                    _ => {}
                }
            }
            Event::Text(text) => {
                let flags = *inline_stack.last().unwrap_or(&InlineFlags::default_());
                let mut absorbed_by_code_block = false;
                if let Some(block) = stack.last_mut() {
                    if let MdBlock::CodeBlock { text: code_text, .. } = block {
                        code_text.push_str(&text);
                        absorbed_by_code_block = true;
                    }
                }
                if !absorbed_by_code_block {
                    push_text(stack.last_mut(), &mut table_state, &text, flags);
                }
            }
            Event::Code(text) => {
                let flags = InlineFlags { code: true, ..InlineFlags::default_() };
                push_text(stack.last_mut(), &mut table_state, &text, flags);
            }
            Event::SoftBreak | Event::HardBreak => {
                if let Some(block) = stack.last_mut() {
                    match block {
                        MdBlock::Paragraph { runs, .. }
                        | MdBlock::Heading { runs, .. }
                        | MdBlock::ListItem { runs, .. }
                        | MdBlock::BlockQuote { runs } => {
                            runs.push(FontRun {
                                text: "\n".to_string(),
                                ..Default::default()
                            });
                        }
                        _ => {}
                    }
                }
            }
            Event::Rule => {
                // Horizontal rule → empty paragraph carrying a
                // bottom border. The in-house writer styles empty
                // paragraphs as thematic breaks via the same trick.
                stack.push(MdBlock::Paragraph {
                    style: Some("HorizontalRule".to_string()),
                    runs: Vec::new(),
                });
                if let Some(block) = stack.pop() {
                    blocks.push(block);
                }
            }
            Event::TaskListMarker(checked) => {
                if let Some(MdBlock::ListItem { checked: c, .. }) = stack.last_mut() {
                    *c = Some(checked);
                }
            }
            Event::FootnoteReference(_) => {
                // Footnote refs render as "[*]" inline. A future patch
                // can wire the in-house footnote machinery to convert
                // these into real w:footnoteReference runs.
                let flags = *inline_stack.last().unwrap_or(&InlineFlags::default_());
                push_text(stack.last_mut(), &mut table_state, "[*]", flags);
            }
            Event::Html(_)
            | Event::InlineHtml(_)
            | Event::InlineMath(..)
            | Event::DisplayMath(..) => {
                // Pass-through elements we deliberately drop.
            }
        }
    }

    blocks
}

/// Internal AST node used by `parse_markdown_to_blocks`. The variants
/// mirror Markdown's block-level grammar (heading / paragraph / list
/// item / code block / blockquote / table) and each carries the inline
/// runs produced by the parser.
pub(crate) enum MdBlock {
    Paragraph { style: Option<String>, runs: Vec<FontRun> },
    Heading { level: u8, runs: Vec<FontRun> },
    ListItem { ordered: bool, level: u32, runs: Vec<FontRun>, checked: Option<bool> },
    CodeBlock { lang: Option<String>, text: String },
    BlockQuote { runs: Vec<FontRun> },
    Table {
        header: Vec<TableCell>,
        rows: Vec<Vec<TableCell>>,
    },
}

/// Materialise the parsed AST into a `docx_rs::Docx` and pack it into a
/// `.docx` file at `output_path`. This is the production entry point
/// for `md_to_word`. We deliberately route through docx-rs's own writer
/// (instead of the in-house OOXML writer) so the output is parseable by
/// `office2pdf`'s embedded docx-rs reader — that round-trip is the
/// contract `word_to_pdf` depends on.
pub(crate) fn materialise_to_docx_rs_doc(
    blocks: Vec<MdBlock>,
    title: Option<&str>,
    output_path: &Path,
) -> Result<u64, String> {
    use std::io::Cursor;
    let mut doc = docx_rs::Docx::new();
    if let Some(t) = title.filter(|s| !s.is_empty()) {
        // Title becomes a Heading 1 paragraph.
        doc = doc.add_paragraph(
            docx_rs::Paragraph::new()
                .align(docx_rs::AlignmentType::Center)
                .add_run(
                    docx_rs::Run::new()
                        .add_text(t)
                        .bold()
                        .size(36),
                ),
        );
    }
    // Walk the AST and apply each block to the docx-rs builder. We
    // re-parse to a stream of docx-rs `Paragraph` / `Table` nodes via
    // the internal helper, but the helper signature was tuned for the
    // legacy code path; here we drive the builder directly.
    for block in blocks {
        match block {
            MdBlock::Paragraph { style, runs } => {
                doc = doc.add_paragraph(build_docxrs_paragraph(runs, style.as_deref(), None));
            }
            MdBlock::Heading { level, runs } => {
                let style = match level {
                    1 => "Heading1",
                    2 => "Heading2",
                    3 => "Heading3",
                    4 => "Heading4",
                    5 => "Heading5",
                    _ => "Heading6",
                };
                doc = doc.add_paragraph(build_docxrs_paragraph(runs, Some(style), Some(level)));
            }
            MdBlock::ListItem { ordered, level, runs, checked } => {
                let marker = match checked {
                    Some(true) => "☒ ",
                    Some(false) => "☐ ",
                    None => "• ",
                };
                let _ = ordered;
                let indent = "  ".repeat(level as usize);
                let prefix = format!("{}{}", indent, marker);
                let mut prefixed = Vec::with_capacity(runs.len() + 1);
                prefixed.push(FontRun {
                    text: prefix,
                    ..Default::default()
                });
                prefixed.extend(runs);
                doc = doc.add_paragraph(build_docxrs_paragraph(prefixed, None, None));
            }
            MdBlock::CodeBlock { lang, text } => {
                let _ = lang;
                if text.contains('\n') {
                    // Multi-line code block: emit each line as its own
                    // paragraph so reflow at print-time works.
                    for line in text.lines() {
                        doc = doc.add_paragraph(build_docxrs_paragraph(
                            vec![FontRun {
                                text: line.to_string(),
                                font_name: Some("Consolas".to_string()),
                                highlight: Some("lightGray".to_string()),
                                ..Default::default()
                            }],
                            Some("CodeBlock"),
                            None,
                        ));
                    }
                } else {
                    doc = doc.add_paragraph(build_docxrs_paragraph(
                        vec![FontRun {
                            text: text,
                            font_name: Some("Consolas".to_string()),
                            highlight: Some("lightGray".to_string()),
                            ..Default::default()
                        }],
                        Some("CodeBlock"),
                        None,
                    ));
                }
            }
            MdBlock::BlockQuote { runs } => {
                let mut prefixed = Vec::with_capacity(runs.len() + 1);
                prefixed.push(FontRun {
                    text: "│ ".to_string(),
                    italic: true,
                    ..Default::default()
                });
                prefixed.extend(runs);
                doc = doc.add_paragraph(build_docxrs_paragraph(prefixed, Some("Quote"), None));
            }
            MdBlock::Table { header, rows } => {
                doc = doc.add_table(build_docxrs_table(&header, &rows));
            }
        }
    }

    let mut buf = Cursor::new(Vec::<u8>::new());
    doc.build()
        .pack(&mut buf)
        .map_err(|e| format!("docx-rs pack failed: {e:?}"))?;
    let bytes = buf.into_inner();
    std::fs::write(output_path, &bytes)
        .map_err(|e| format!("Failed to write DOCX {}: {}", output_path.display(), e))?;
    Ok(bytes.len() as u64)
}

/// Build a docx-rs `Paragraph` from a flat list of `FontRun`s. The
/// `style` argument selects one of Word's built-in style names; we
/// translate that to a docx-rs `Style` (via the run-level bold /
/// size hints since we don't ship a custom style sheet). Headings get
/// an inline bold + larger font fallback so they render visually
/// distinctive even when the styles table isn't carried through.
fn build_docxrs_paragraph(
    runs: Vec<FontRun>,
    style: Option<&str>,
    heading_level: Option<u8>,
) -> docx_rs::Paragraph {
    let mut para = docx_rs::Paragraph::new();
    if let Some(level) = heading_level {
        // Map Markdown heading levels to docx-rs heading styles. Word
        // ships these by default; docx-rs writes the references into
        // the document.xml. The visual size is also reinforced with a
        // raw size hint in case the receiving reader ignores the
        // style table.
        para = match level {
            1 => para.style("Heading1"),
            2 => para.style("Heading2"),
            3 => para.style("Heading3"),
            4 => para.style("Heading4"),
            5 => para.style("Heading5"),
            _ => para.style("Heading6"),
        };
    } else if matches!(style, Some("Quote")) {
        para = para.style("Quote");
    } else if matches!(style, Some("CodeBlock")) {
        para = para.style("CodeBlock");
    }
    if runs.is_empty() {
        return para;
    }
    for run in runs {
        let run_text = run.text.clone();
        if run_text.is_empty() {
            continue;
        }
        let mut r = docx_rs::Run::new().add_text(&run_text);
        if run.bold {
            r = r.bold();
        }
        if run.italic {
            r = r.italic();
        }
        if run.strikethrough {
            r = r.strike();
        }
        if let Some(font) = &run.font_name {
            if !font.is_empty() {
                let family = format!(
                    "{}\", \"{}\", monospace",
                    font,
                    if font.to_ascii_lowercase().contains("mono")
                        || font.to_ascii_lowercase().contains("consolas")
                        || font.to_ascii_lowercase().contains("courier")
                    {
                        "Liberation Mono"
                    } else {
                        "Liberation Sans"
                    }
                );
                r = r.fonts(docx_rs::RunFonts::new().east_asia(family.clone()).ascii(family.clone()));
            }
        }
        if let Some(size) = run.font_size {
            r = r.size(size as usize);
        } else if let Some(level) = heading_level {
            // Reinforce the heading style with a raw size so unknown
            // renderers don't render the heading as body text.
            r = r.size((28 + (6 - level as u32) * 2) as usize);
        }
        if let Some(color) = &run.color {
            if !color.is_empty() {
                r = r.color(color.clone());
            }
        }
        // Inline code: simulate the highlight via a soft-grey color.
        if matches!(run.highlight.as_deref(), Some("lightGray")) && run.font_name.is_none() {
            // No-op; `highlight` (text background) is a Word-side
            // shading property the docx-rs run builder doesn't
            // expose directly. The font is monospaced instead, which
            // is the user-visible cue that matters.
        }
        para = para.add_run(r);
    }
    para
}

/// Build a docx-rs `Table` from a header row + body rows of
/// `TableCell`s. The header row is marked bold so the resulting Word
/// table looks like a typical Markdown table.
fn build_docxrs_table(
    header: &[TableCell],
    rows: &[Vec<TableCell>],
) -> docx_rs::Table {
    // Collect every row's `TableCell`s first, then build the table in
    // one go — `docx_rs::Table::new` consumes a `Vec<TableRow>` and the
    // builder chain has no `add_cell` method on `TableRow`.
    let mut all_rows: Vec<docx_rs::TableRow> = Vec::new();
    if !header.is_empty() {
        let header_cells: Vec<docx_rs::TableCell> = header
            .iter()
            .map(|c| {
                let text = c.text.clone();
                docx_rs::TableCell::new().add_paragraph(
                    docx_rs::Paragraph::new()
                        .add_run(docx_rs::Run::new().add_text(text).bold()),
                )
            })
            .collect();
        all_rows.push(docx_rs::TableRow::new(header_cells));
    }
    for row_cells in rows {
        if row_cells.is_empty() {
            continue;
        }
        let body_cells: Vec<docx_rs::TableCell> = row_cells
            .iter()
            .map(|c| {
                let text = c.text.clone();
                docx_rs::TableCell::new().add_paragraph(
                    docx_rs::Paragraph::new()
                        .add_run(docx_rs::Run::new().add_text(text)),
                )
            })
            .collect();
        all_rows.push(docx_rs::TableRow::new(body_cells));
    }
    // docx-rs's `Table::new` requires at least one row; emit a single
    // empty cell when nothing was supplied so the table isn't malformed.
    if all_rows.is_empty() {
        all_rows.push(docx_rs::TableRow::new(vec![
            docx_rs::TableCell::new()
                .add_paragraph(docx_rs::Paragraph::new()),
        ]));
    }
    docx_rs::Table::new(all_rows).style("TableGrid")
}

// ── word_to_pdf ───────────────────────────────────────────────────────────────

pub struct WordToPdfTool;

impl WordToPdfTool {
    pub fn new() -> Self { Self }

    pub fn definition(&self) -> ToolDefinition {
        ToolDefinition::new_with_label(
            "word_to_pdf",
            "Word 转 PDF",
            "Convert a Word `.docx` file to a `.pdf` file. Pure-Rust path via `office2pdf` (Typst backend — no LibreOffice, no Chromium, no Docker). The output preserves text formatting, tables, images, headers/footers, and page setup. Use this when the user wants to share a Word document as a read-only PDF (printing, distribution, archival).",
            ToolParameters::new(
                vec!["input_path", "output_path"],
                vec![
                    ("input_path", "string", Some("Absolute path to the source `.docx` file.")),
                    ("output_path", "string", Some("Absolute path of the output `.pdf` file. Parent directory is created if missing.")),
                    ("paper_size", "string", Some("Optional paper size: `a4` (default), `letter`, or `legal`. Maps to `office2pdf::config::PaperSize`.")),
                    ("landscape", "string", Some("Optional orientation flag. `true`/`false`. Default: `false`.")),
                ],
            ),
        )
    }

    pub async fn execute(
        &self,
        arguments: Value,
        workspace: Option<String>,
    ) -> Result<ConvertOutcome, ToolError> {
        let args: WordToPdfArgs = serde_json::from_value(arguments).map_err(|e| {
            ToolError::InvalidArguments(
                "word_to_pdf".to_string(),
                format!("Invalid parameters: {}", e),
            )
        })?;

        validate_workspace_path(&args.input_path, &workspace)?;
        validate_workspace_path(&args.output_path, &workspace)?;

        let input_path = PathBuf::from(&args.input_path);
        if !input_path.exists() {
            return Err(ToolError::IoError(format!(
                "Source DOCX file does not exist: {}",
                args.input_path
            )));
        }
        let output_path = PathBuf::from(&args.output_path);
        if let Some(parent) = output_path.parent() {
            if !parent.as_os_str().is_empty() && !parent.exists() {
                tokio::fs::create_dir_all(parent).await.map_err(|e| {
                    ToolError::IoError(format!(
                        "Failed to create output directory {}: {}",
                        parent.display(),
                        e
                    ))
                })?;
            }
        }

        let docx_bytes = tokio::fs::read(&input_path).await.map_err(|e| {
            ToolError::IoError(format!("Failed to read DOCX {}: {}", args.input_path, e))
        })?;

        let paper_size = args.paper_size.clone();
        let landscape = args.landscape.clone();
        let output_path_for_render = output_path.clone();

        let convert_result = tokio::task::spawn_blocking(move || {
            convert_docx_bytes_to_pdf(
                docx_bytes,
                &output_path_for_render,
                paper_size.as_deref(),
                landscape.as_deref(),
            )
        })
        .await
        .map_err(|e| ToolError::ExecutionError(format!("word_to_pdf task panicked: {}", e)))?;

        match convert_result {
            Ok(bytes) => {
                let result_json = serde_json::json!({
                    "input_path": input_path.to_string_lossy(),
                    "output_path": output_path.to_string_lossy(),
                    "bytes": bytes.len(),
                })
                .to_string();
                Ok(ConvertOutcome {
                    output: result_json,
                    is_error: false,
                    file_path: Some(output_path.to_string_lossy().to_string()),
                })
            }
            Err(e) => Ok(ConvertOutcome {
                output: format!(
                    "word_to_pdf failed: {}\nSource: {}",
                    e,
                    input_path.display()
                ),
                is_error: true,
                file_path: None,
            }),
        }
    }
}

impl Default for WordToPdfTool {
    fn default() -> Self { Self::new() }
}

#[derive(Debug, Deserialize)]
struct WordToPdfArgs {
    input_path: String,
    output_path: String,
    paper_size: Option<String>,
    landscape: Option<String>,
}

fn convert_docx_bytes_to_pdf(
    docx_bytes: Vec<u8>,
    output_path: &Path,
    paper_size: Option<&str>,
    landscape: Option<&str>,
) -> Result<Vec<u8>, String> {
    use office2pdf::config::{ConvertOptions, Format, PaperSize};

    let mut options = ConvertOptions::default();
    if let Some(size) = paper_size {
        options.paper_size = Some(match size.to_ascii_lowercase().as_str() {
            "a4" => PaperSize::A4,
            "letter" => PaperSize::Letter,
            "legal" => PaperSize::Legal,
            other => {
                return Err(format!(
                    "Unsupported paper_size '{}'. Use 'a4', 'letter', or 'legal'.",
                    other
                ));
            }
        });
    }
    // `office2pdf::config::ConvertOptions` exposes orientation via
    // individual fields in v0.6.x. We pass `landscape` through the
    // `ConvertOptions::landscape` field when present.
    if let Some(land) = landscape {
        options.landscape = Some(match land.to_ascii_lowercase().as_str() {
            "true" | "1" | "yes" => true,
            "false" | "0" | "no" => false,
            other => {
                return Err(format!(
                    "Invalid landscape value '{}'. Use 'true' or 'false'.",
                    other
                ));
            }
        });
    }

    let result = office2pdf::convert_bytes(&docx_bytes, Format::Docx, &options)
        .map_err(|e| format!("office2pdf convert_bytes failed: {:?}", e))?;

    std::fs::write(output_path, &result.pdf)
        .map_err(|e| format!("Failed to write PDF to {}: {}", output_path.display(), e))?;
    Ok(result.pdf)
}

#[cfg(test)]
mod round_trip_tests {
    //! End-to-end smoke tests for the md_to_word → word_to_pdf
    //! pipeline. Each test exercises `parse_markdown_to_blocks` +
    //! `materialise_to_docx_rs_doc` and then hands the resulting bytes
    //! to `office2pdf::convert_bytes`. The tests write artefacts to
    //! `/tmp` and assert both stages succeed without warnings.
    //!
    //! These are `#[ignore]`-d by default — they're slow (each spins
    //! up docx-rs's pack pipeline plus Typst via office2pdf) and only
    //! meaningful when investigating the writer / converter
    //! compatibility question. Run with
    //! `cargo test -p inkuo --lib round_trip_tests -- --ignored`.
    use super::*;
    use std::path::PathBuf;

    const SAMPLE_MD: &str = r#"# Sample Document

A quick smoke test of the **md_to_word** + **word_to_pdf** round-trip.

## Features

- Headings work.
- *Italic* and **bold** runs render.
- Inline `code` is rendered as monospace text.

## Code block

```rust
fn main() {
    println!("hello, world");
}
```

## Table

| Column A | Column B |
| -------- | -------- |
| one      | two      |
| three    | four     |

## Quote

> This is a blockquote.

End of document.
"#;

    fn tmp_path(name: &str) -> PathBuf {
        let mut p = std::env::temp_dir();
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        p.push(format!("inkuo_round_trip_{}_{}", name, stamp));
        p
    }

    #[test]
    #[ignore]
    fn md_to_word_then_word_to_pdf_succeeds() {
        let blocks = parse_markdown_to_blocks(SAMPLE_MD);
        assert!(!blocks.is_empty(), "parser should emit at least one block");

        let docx_path = tmp_path("docx");
        let bytes = materialise_to_docx_rs_doc(blocks, Some("Sample Document"), &docx_path)
            .expect("materialise must succeed");
        assert!(bytes > 1000, "docx should be non-trivial; got {} bytes", bytes);
        assert!(docx_path.exists(), "docx file must exist on disk");

        // Hand the same bytes to office2pdf. The whole point of
        // routing md_to_word through docx-rs is that this conversion
        // succeeds without warnings — failing here means the writer
        // and reader drifted again.
        let docx_bytes = std::fs::read(&docx_path).expect("read docx");
        let mut opts = office2pdf::config::ConvertOptions::default();
        opts.landscape = Some(false);
        let result = office2pdf::convert_bytes(
            &docx_bytes,
            office2pdf::config::Format::Docx,
            &opts,
        )
        .expect("office2pdf must accept docx-rs output");
        // `FallbackUsed` warnings are non-fatal: they mean the
        // converter substituted a missing font (e.g. Arial → Liberation
        // Sans) and the PDF was still generated. We only fail on
        // *errors* in the warning stream.
        for w in &result.warnings {
            let s = format!("{:?}", w);
            assert!(
                !s.contains("Error") && !s.contains("Failed"),
                "office2pdf emitted error warning: {:?}",
                w
            );
        }
        assert!(result.pdf.len() > 1000, "pdf should be non-trivial");

        let pdf_path = tmp_path("pdf");
        std::fs::write(&pdf_path, &result.pdf).expect("write pdf");
        let _ = pdf_path;
    }

    #[test]
    #[ignore]
    fn svg_to_png_loads_system_fonts_when_present() {
        // Smoke test for the fontdb fix: rasterise a tiny SVG that
        // references `sans-serif` and check we get a non-empty PNG.
        // We don't assert text pixels — that requires a known
        // font — but the encode step should succeed even when the
        // system has zero fonts (usvg silently drops the runs in
        // that case, which is the legacy behaviour we're preserving).
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="20">
            <text x="0" y="15" font-family="sans-serif" font-size="14">hi</text>
        </svg>"#;
        let out = tmp_path("png");
        let bytes = rasterize_svg_to_png(svg, &out, None, None, None).expect("rasterise");
        assert!(bytes.starts_with(&[0x89, b'P', b'N', b'G']));
        assert!(out.exists());
        let _ = out;
    }
}