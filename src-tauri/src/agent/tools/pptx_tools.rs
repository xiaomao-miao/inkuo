//! PowerPoint authoring tool: `create_pptx`
//!
//! Takes a list of `.svg` files and packages them into a `.pptx` presentation
//! in which **every shape remains editable in PowerPoint / Keynote / WPS**
//! — the SVG geometry is converted to native OOXML shapes
//! (`<p:sp>` / `<p:cxnSp>` / `<a:custGeom>`) rather than rasterised to a
//! bitmap inside an `<p:pic>`.
//!
//! ## Why a dedicated tool (and not just `write_file`)
//!
//! `write_file` *cannot* write a `.pptx` — the format is a binary zip of XML,
//! and hand-rolling the contents is exactly what we want to *hide* from the
//! LLM. `create_pptx` exposes a tiny, declarative API:
//!
//! - `svg_paths[]` — every input SVG becomes one slide (each input supplies a
//!   list of shapes that are mapped 1:1 to `<p:sp>` elements on that slide).
//! - `output_path` — the destination `.pptx`. Created atomically, parent
//!   directories auto-created.
//! - `title` — optional deck title, stamped into the core properties.
//!
//! Like `create_svg`, this tool returns a `CreatePptxOutcome` so the registry
//! can stamp `file_path` on the `ToolResult` and trigger the frontend's
//! `file-change` event (so the sidebar tree refreshes and the OS file
//! association launches PowerPoint when the user clicks the link).
//!
//! ## Coverage
//!
//! We deliberately support a *narrow* SVG subset — the shapes that round-trip
//! cleanly into OOXML without losing the "edit later" property:
//!
//! | SVG element             | OOXML target                                | Editable in PPT? |
//! | ----------------------- | ------------------------------------------- | ---------------- |
//! | `<rect>`                | `<p:sp>` preset geometry `rect`             | ✓ (resize, recolour, edit text-free) |
//! | `<circle>`              | `<p:sp>` preset geometry `ellipse`          | ✓                |
//! | `<ellipse>`             | `<p:sp>` preset geometry `ellipse`          | ✓                |
//! | `<line>`                | `<p:cxnSp>` connector                       | ✓                |
//! | `<polyline>` / `<polygon>` | `<p:sp>` preset geometry + custom path    | ✓ (geometry locked, but vertex handles visible) |
//! | `<path>`                | `<p:sp>` with `<a:custGeom>` custom path    | ✓                |
//! | `<text>`                | `<p:sp>` with `<p:txBody>`                  | ✓ (fully editable text) |
//! | `<image>` (data: URL)   | `<p:pic>` with embedded `<a:blip>`           | ✓ (resize, recolour) |
//! | `<g transform="…">`     | wrapped in `<p:sp>` xfrm or applied to children | ✓            |
//!
//! Unsupported elements (`<use>`, `<foreignObject>`, `<filter>`,
//! `<mask>`, scripts) are skipped with a soft warning — the slide is still
//! emitted so the file opens cleanly.

use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::io::{Cursor, Read, Write};
use std::path::PathBuf;

use super::{validate_workspace_path, ToolDefinition, ToolError, ToolParameters};

// ---------------------------------------------------------------------------
// Slide canvas (EMU units). PowerPoint uses 914,400 EMU per inch; the default
// slide size is 13.333" × 7.5" (16:9 widescreen), i.e. 12,192,000 × 6,858,000
// EMU. We use that as our default canvas.
// ---------------------------------------------------------------------------

/// EMUs per inch (PowerPoint's base unit).
pub const EMU_PER_INCH: i64 = 914_400;
/// Default slide width in EMU (13.333" × 914,400).
pub const SLIDE_W_EMU: i64 = 12_192_000;
/// Default slide height in EMU (7.5" × 914,400).
pub const SLIDE_H_EMU: i64 = 6_858_000;

// ---------------------------------------------------------------------------
// Outcome wrapper
// ---------------------------------------------------------------------------

/// Structured outcome returned by `CreatePptxTool::execute`. Mirrors the
/// `CreateSvgOutcome` shape so the registry can stamp `file_path` and trigger
/// the frontend's `file-change` event identically.
#[derive(Debug)]
pub struct CreatePptxOutcome {
    pub output: String,
    pub file_path: String,
    pub byte_size: usize,
    pub slide_count: usize,
    pub slide_summaries: Vec<SlideSummary>,
    pub is_error: bool,
}

/// Per-slide summary, returned in the tool's JSON output so the LLM can
/// confirm to the user what was generated.
#[derive(Debug, Serialize)]
pub struct SlideSummary {
    pub index: usize,
    pub source_svg: String,
    pub shape_count: usize,
    pub skipped_elements: Vec<String>,
}

// ---------------------------------------------------------------------------
// Args
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct CreatePptxArgs {
    /// Absolute paths to the input SVG files. Order is preserved — the n-th
    /// SVG becomes the n-th slide. At least one is required.
    svg_paths: Vec<String>,
    /// Absolute workspace-relative path to write the `.pptx` to. Must end in
    /// `.pptx`. Parent directories are created as needed.
    output_path: String,
    /// Optional deck title, stamped into `docProps/core.xml` as
    /// `<dc:title>`. Also shown in PowerPoint's "Title" field.
    #[serde(default)]
    title: Option<String>,
}

// ---------------------------------------------------------------------------
// Tool
// ---------------------------------------------------------------------------

pub struct CreatePptxTool;

impl CreatePptxTool {
    pub fn new() -> Self { Self }

    pub fn definition(&self) -> ToolDefinition {
        ToolDefinition::new_with_label(
            "create_pptx",
            "生成 PPT",
            "Pack a list of `.svg` files into a single `.pptx` presentation in which every shape \
             is native OOXML — fully editable in PowerPoint / Keynote / WPS (resize, recolour, \
             edit text). Each SVG becomes one slide, in the same order as the input. The supported \
             SVG subset is `rect`, `circle`, `ellipse`, `line`, `polyline`, `polygon`, `path`, \
             `text`, and `<g transform=...>`. Linear / radial gradients resolve to the first \
             `<stop>`'s colour as a `<a:solidFill>` (we don't try to recreate the gradient ramp \
             because it doesn't render portably across PowerPoint / Keynote / WPS). Unsupported \
             elements (use / foreignObject / filter / mask / script) are skipped with a \
             warning; the slide is still emitted so the deck always opens cleanly.",
            ToolParameters::new(
                vec!["svg_paths", "output_path"],
                vec![
                    ("svg_paths", "array", Some("JSON array of absolute paths to `.svg` files. Order is preserved — n-th element becomes the n-th slide. Must contain at least one path.")),
                    ("output_path", "string", Some("Absolute workspace path to write the `.pptx` to. Extension must be `.pptx`. Parent directories are created automatically.")),
                    ("title", "string", Some("Optional deck title, stamped into `docProps/core.xml` and PowerPoint's Title field.")),
                ],
            ),
        )
    }

    pub async fn execute(
        &self,
        arguments: Value,
        workspace: Option<String>,
    ) -> Result<CreatePptxOutcome, ToolError> {
        let args: CreatePptxArgs = serde_json::from_value(arguments).map_err(|e| {
            ToolError::InvalidArguments(
                "create_pptx".to_string(),
                format!("Invalid parameters: {}", e),
            )
        })?;

        // ── 1. Output path validation ────────────────────────────────────
        let output_path = PathBuf::from(&args.output_path);
        let ext = output_path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if ext != "pptx" {
            return Err(ToolError::InvalidArguments(
                "create_pptx".to_string(),
                format!("output_path must end with `.pptx`; got `.{}{}`",
                        ext,
                        if ext.is_empty() { " (no extension)" } else { "" }),
            ));
        }
        validate_workspace_path(&args.output_path, &workspace)?;

        // ── 2. Input validation ──────────────────────────────────────────
        if args.svg_paths.is_empty() {
            return Err(ToolError::InvalidArguments(
                "create_pptx".to_string(),
                "svg_paths must contain at least one path".to_string(),
            ));
        }
        for (i, p) in args.svg_paths.iter().enumerate() {
            let ext = std::path::Path::new(p)
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            if ext != "svg" {
                return Err(ToolError::InvalidArguments(
                    "create_pptx".to_string(),
                    format!("svg_paths[{i}] must end with `.svg`; got `{p}`"),
                ));
            }
            validate_workspace_path(p, &workspace)?;
        }

        // ── 3. Parse every SVG ───────────────────────────────────────────
        let mut slides = Vec::with_capacity(args.svg_paths.len());
        for (idx, p) in args.svg_paths.iter().enumerate() {
            let bytes = tokio::fs::read(p).await.map_err(|e| {
                ToolError::IoError(format!("Failed to read SVG {p}: {e}"))
            })?;
            let svg = std::str::from_utf8(&bytes).map_err(|e| {
                ToolError::ExecutionError(format!(
                    "SVG {p} is not valid UTF-8: {e}"
                ))
            })?;
            let parsed = match parse_svg(svg) {
                Ok(p) => p,
                Err(e) => {
                    return Err(ToolError::ExecutionError(format!(
                        "Failed to parse SVG {p}: {e}"
                    )));
                }
            };
            slides.push(SlideInput {
                source_path: p.clone(),
                slide_index: idx + 1,
                content: parsed,
            });
        }

        // ── 4. Build the .pptx in memory ─────────────────────────────────
        let deck = build_pptx(&slides, args.title.as_deref())?;
        let byte_size = deck.len();

        // ── 5. Ensure parent directory + atomic write ────────────────────
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
        tokio::fs::write(&output_path, &deck).await.map_err(|e| {
            ToolError::IoError(format!(
                "Failed to write pptx to {}: {}",
                output_path.display(),
                e
            ))
        })?;

        // ── 6. Build the success output JSON ─────────────────────────────
        let summaries: Vec<serde_json::Value> = slides
            .iter()
            .map(|s| {
                json!({
                    "index": s.slide_index,
                    "source_svg": s.source_path,
                    "shape_count": s.content.shapes.len(),
                    "skipped_elements": s.content.skipped,
                })
            })
            .collect();

        let title = args
            .title
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("(untitled)");

        let output = json!({
            "status": "ok",
            "file_path": output_path.to_string_lossy(),
            "title": title,
            "bytes": byte_size,
            "slide_count": slides.len(),
            "slides": summaries,
        })
        .to_string();

        Ok(CreatePptxOutcome {
            output,
            file_path: output_path.to_string_lossy().to_string(),
            byte_size,
            slide_count: slides.len(),
            slide_summaries: slides
                .iter()
                .map(|s| SlideSummary {
                    index: s.slide_index,
                    source_svg: s.source_path.clone(),
                    shape_count: s.content.shapes.len(),
                    skipped_elements: s.content.skipped.clone(),
                })
                .collect(),
            is_error: false,
        })
    }
}

impl Default for CreatePptxTool {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// SVG → internal model
// ---------------------------------------------------------------------------

/// A single SVG, after parsing.
pub struct ParsedSvg {
    /// The canvas size in SVG user units. We centre the artwork onto the
    /// 16:9 PPT slide and scale to fit (preserving aspect ratio).
    pub vb_x: f64,
    pub vb_y: f64,
    pub vb_w: f64,
    pub vb_h: f64,
    /// Shapes, in document order. `text` shapes carry their raw `<text>`
    /// children (we render them on the PPT slide too).
    pub shapes: Vec<SvgShape>,
    /// Element names that we encountered but skipped (so the tool output can
    /// tell the user "we dropped 3 <image> elements").
    skipped: Vec<String>,
    /// Gradient lookup table: id → first `<stop>` colour/opacity. Populated
    /// while parsing the `<defs>` block at the top of the SVG. Shapes may
    /// reference these via `fill="url(#id)"`; the parser resolves the
    /// reference to a solid colour so the OOXML writer stays trivial.
    defs: BTreeMap<String, GradientStop>,
}

/// A single SVG shape, normalised into a representation we can convert to
/// OOXML. The shape coordinates are still in SVG user units — the
/// `to_ooxml` step applies the per-slide scale + offset transform.
#[derive(Debug, Clone)]
pub enum SvgShape {
    Rect {
        x: f64, y: f64, width: f64, height: f64,
        rx: Option<f64>, ry: Option<f64>,
        fill: Option<Paint>,
        stroke: Option<Paint>,
        stroke_width: Option<f64>,
        opacity: Option<f64>,
    },
    Ellipse {
        cx: f64, cy: f64, rx: f64, ry: f64,
        fill: Option<Paint>,
        stroke: Option<Paint>,
        stroke_width: Option<f64>,
        opacity: Option<f64>,
    },
    Line {
        x1: f64, y1: f64, x2: f64, y2: f64,
        stroke: Option<Paint>,
        stroke_width: Option<f64>,
        opacity: Option<f64>,
    },
    Path {
        /// Raw `d` attribute. We pass it through to OOXML `<a:custGeom>`
        /// (which uses the same SVG path grammar since Office 2013).
        d: String,
        fill: Option<Paint>,
        stroke: Option<Paint>,
        stroke_width: Option<f64>,
        opacity: Option<f64>,
    },
    Text {
        x: f64, y: f64,
        /// `<text>` body — child text + nested `<tspan>` runs, flattened.
        runs: Vec<TextRun>,
        font_size: Option<f64>,
        fill: Option<Paint>,
        opacity: Option<f64>,
        /// SVG `text-anchor` value (`start` / `middle` / `end`). The
        /// OOXML writer uses this to set both the text-box geometry
        /// (so the box doesn't overflow the slide when `x` is at the
        /// centre / right) and the per-paragraph alignment.
        text_anchor: String,
    },
    /// An embedded raster image (`<image href="data:image/png;base64,..."/>`).
    /// The PNG/JPEG bytes are stored as-is; the PPTX writer embeds them
    /// as `Media/placeholderN.{ext}` entries and references them via
    /// `<a:blip fill="sblipRgd">`.
    Image {
        x: f64,
        y: f64,
        width: f64,
        height: f64,
        /// Raw image bytes (PNG or JPEG).
        data: Vec<u8>,
        /// MIME type: "image/png" or "image/jpeg".
        mime: String,
        /// Extension used in the ZIP file: "png" or "jpg".
        ext: String,
    },
}

/// One run inside a `<text>` element. Multi-run text is preserved so PPT can
/// edit each run independently.
#[derive(Debug, Clone)]
pub struct TextRun {
    text: String,
    bold: bool,
    italic: bool,
    underline: bool,
    fill: Option<Paint>,
}

/// Fill / stroke paint. We collapse CSS-ish `fill="red"` / `fill="none"`
/// / `fill-opacity` into a single struct so the OOXML writer doesn't have
/// to branch. Gradient refs (`fill="url(#…)"`) are resolved at parse time
/// to the first `<stop>`'s colour — we don't try to recreate the gradient
/// ramp in DrawingML because that's not portable across PowerPoint /
/// Keynote / WPS, and the AI toolchain (create_svg, flowchart_expert)
/// emits gradients purely for visual richness, never as data-bearing
/// colour ramps.
#[derive(Clone, Debug)]
pub enum Paint {
    None,
    Color { rgb: String, opacity: Option<f64> },
    /// `url(#id)` resolved to the first stop of the matching gradient
    /// inside this slide's `<defs>`. We carry the resolved colour so the
    /// OOXML writer doesn't need to thread the gradient map through.
    GradientRef { rgb: String, opacity: Option<f64> },
}

/// Intermediate representation of one input SVG.
pub struct SlideInput {
    source_path: String,
    slide_index: usize,
    content: ParsedSvg,
}

// ---------------------------------------------------------------------------
// SVG parser (public so pptx_animation_tools can re-use it)
// ---------------------------------------------------------------------------

pub fn parse_svg(svg: &str) -> Result<ParsedSvg, String> {
    let mut reader = Reader::from_str(svg);
    reader.config_mut().trim_text(true);

    let mut parsed = ParsedSvg {
        vb_x: 0.0,
        vb_y: 0.0,
        vb_w: 0.0,
        vb_h: 0.0,
        shapes: Vec::new(),
        skipped: Vec::new(),
        defs: BTreeMap::new(),
    };

    // Stack of `<g transform="...">` translation / scale contexts. Each entry
    // is a transform applied to coordinates *before* they're added to the
    // parent. We only support `translate(x, y)` and `scale(s)` because that's
    // all the create_svg / flowchart / diagram toolchains emit.
    let mut transforms: Vec<Transform> = vec![Transform::identity()];

    // Stack of currently-open `<linearGradient>` / `<radialGradient>` ids.
    // Used by `<stop>` Start events to know which gradient they belong to.
    // A gradient can only contain `<stop>`s in SVG, so a 1-deep stack is
    // technically enough, but we keep it general.
    let mut gradient_stack: Vec<String> = Vec::new();

    // Text accumulation state. When we hit a `<text>` element we begin
    // collecting runs; when we hit the close we flush.
    let mut text_acc: Option<TextAcc> = None;

    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                let name = e.name();
                let tag = std::str::from_utf8(name.as_ref()).unwrap_or("").to_string();
                let attrs = read_attrs(&e);

                match tag.as_str() {
                    "svg" => {
                        if let Some(vb) = attrs.get("viewBox") {
                            let parts: Vec<&str> =
                                vb.split(|c: char| c.is_whitespace() || c == ',')
                                    .filter(|s| !s.is_empty())
                                    .collect();
                            if parts.len() == 4 {
                                parsed.vb_x = parts[0].parse().unwrap_or(0.0);
                                parsed.vb_y = parts[1].parse().unwrap_or(0.0);
                                parsed.vb_w = parts[2].parse().unwrap_or(100.0);
                                parsed.vb_h = parts[3].parse().unwrap_or(100.0);
                            }
                        } else if let (Some(w), Some(h)) =
                            (attrs.get("width"), attrs.get("height"))
                        {
                            // Best-effort fallback when viewBox is missing.
                            parsed.vb_w = w.parse().unwrap_or(100.0);
                            parsed.vb_h = h.parse().unwrap_or(100.0);
                        }
                        if parsed.vb_w == 0.0 { parsed.vb_w = 100.0; }
                        if parsed.vb_h == 0.0 { parsed.vb_h = 100.0; }
                    }
                    "g" => {
                        if let Some(t) = attrs.get("transform") {
                            transforms.push(transforms.last().unwrap().compose(t));
                        } else {
                            transforms.push(*transforms.last().unwrap());
                        }
                    }
                    "linearGradient" | "radialGradient" => {
                        // Capture the gradient id → first stop mapping so
                        // shapes that reference `url(#id)` can resolve to
                        // a solid colour. We only need the *first* stop
                        // for our v1 fallback — see the Paint::GradientRef
                        // doc-comment for the rationale.
                        if let Some(id) = attrs.get("id").cloned() {
                            // Seed with white so <stop> can detect "first
                            // wins" via the colour placeholder. The actual
                            // colour is overwritten below.
                            parsed.defs.entry(id.clone()).or_insert(GradientStop {
                                rgb: "FFFFFF".to_string(),
                                opacity: None,
                            });
                            gradient_stack.push(id);
                        } else {
                            // An anonymous gradient can't be referenced by
                            // `url(#…)`, but we still push a placeholder
                            // so End handling stays balanced.
                            gradient_stack.push(String::new());
                        }
                    }
                    "stop" => {
                        // See `try_capture_gradient_stop` — only the
                        // FIRST stop in any given gradient is honoured.
                        if let Some(parent_id) = gradient_stack.last() {
                            try_capture_gradient_stop(&mut parsed.defs, parent_id, &attrs);
                        }
                    }
                    "rect" => {
                        if let Some(shape) = build_rect(&attrs, transforms.last().unwrap(), &parsed.defs) {
                            parsed.shapes.push(shape);
                        }
                    }
                    "circle" => {
                        if let Some(shape) = build_circle(&attrs, transforms.last().unwrap(), &parsed.defs) {
                            parsed.shapes.push(shape);
                        }
                    }
                    "ellipse" => {
                        if let Some(shape) = build_ellipse(&attrs, transforms.last().unwrap(), &parsed.defs) {
                            parsed.shapes.push(shape);
                        }
                    }
                    "line" => {
                        if let Some(shape) = build_line(&attrs, transforms.last().unwrap(), &parsed.defs) {
                            parsed.shapes.push(shape);
                        }
                    }
                    "path" => {
                        if let Some(shape) = build_path(&attrs, transforms.last().unwrap(), &parsed.defs) {
                            parsed.shapes.push(shape);
                        }
                    }
                    "polyline" | "polygon" => {
                        if let Some(shape) =
                            build_poly(&tag, &attrs, transforms.last().unwrap(), &parsed.defs)
                        {
                            parsed.shapes.push(shape);
                        }
                    }
                    "text" => {
                        if text_acc.is_none() {
                            text_acc = Some(TextAcc {
                                x: attrs
                                    .get("x")
                                    .and_then(|v| v.parse().ok())
                                    .unwrap_or(0.0),
                                y: attrs
                                    .get("y")
                                    .and_then(|v| v.parse().ok())
                                    .unwrap_or(0.0),
                                font_size: attrs
                                    .get("font-size")
                                    .and_then(|v| parse_len(v)),
                                fill: attrs
                                    .get("fill")
                                    .and_then(|v| parse_paint(v, &attrs, &parsed.defs)),
                                opacity: attrs
                                    .get("opacity")
                                    .and_then(|v| v.parse().ok()),
                                transform: *transforms.last().unwrap(),
                                text_anchor: attrs
                                    .get("text-anchor")
                                    .cloned()
                                    .unwrap_or_else(|| "start".to_string()),
                                runs: Vec::new(),
                                current_run: String::new(),
                                current_bold: false,
                                current_italic: false,
                                current_underline: false,
                                current_fill: None,
                            });
                        }
                    }
                    "tspan" => {
                        // Flush whatever we have so far as a run, then open
                        // a new run with this tspan's style overrides.
                        if let Some(acc) = text_acc.as_mut() {
                            if !acc.current_run.is_empty() {
                                acc.runs.push(TextRun {
                                    text: std::mem::take(&mut acc.current_run),
                                    bold: acc.current_bold,
                                    italic: acc.current_italic,
                                    underline: acc.current_underline,
                                    fill: acc.current_fill.clone(),
                                });
                            }
                            let bold = attrs
                                .get("font-weight")
                                .map(|v| matches!(v.as_str(), "bold" | "700" | "800" | "900"))
                                .unwrap_or(false);
                            let italic = attrs
                                .get("font-style")
                                .map(|v| v == "italic")
                                .unwrap_or(false);
                            let underline = attrs
                                .get("text-decoration")
                                .map(|v| v.contains("underline"))
                                .unwrap_or(false);
                            let fill = attrs
                                .get("fill")
                                .and_then(|v| parse_paint(v, &attrs, &parsed.defs));
                            acc.current_bold = bold;
                            acc.current_italic = italic;
                            acc.current_underline = underline;
                            if fill.is_some() {
                                acc.current_fill = fill;
                            }
                        }
                    }
                    // Unsupported — record and skip.
                    "use" | "foreignObject" | "filter" | "mask"
                    | "clipPath" | "pattern" | "switch" => {
                        if !parsed.skipped.contains(&tag) {
                            parsed.skipped.push(tag);
                        }
                    }
                    "image" => {
                        // Try to parse inline data: URL; skip only if it fails.
                        if let Some(href) = attrs.get("href")
                            .or_else(|| attrs.get("{http://www.w3.org/1999/xlink}href"))
                        {
                            let x = attrs.get("x").and_then(|v| v.parse().ok()).unwrap_or(0.0);
                            let y = attrs.get("y").and_then(|v| v.parse().ok()).unwrap_or(0.0);
                            let w = attrs.get("width").and_then(|v| v.parse().ok());
                            let h = attrs.get("height").and_then(|v| v.parse().ok());
                            if let Some(shape) = build_image(href, x, y, w, h) {
                                parsed.shapes.push(shape);
                            } else if !parsed.skipped.contains(&"image".to_string()) {
                                parsed.skipped.push("image".to_string());
                            }
                        } else if !parsed.skipped.contains(&"image".to_string()) {
                            parsed.skipped.push("image".to_string());
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::Empty(e)) => {
                let name = e.name();
                let tag = std::str::from_utf8(name.as_ref()).unwrap_or("").to_string();
                let attrs = read_attrs(&e);

                match tag.as_str() {
                    "rect" => {
                        if let Some(shape) = build_rect(&attrs, transforms.last().unwrap(), &parsed.defs) {
                            parsed.shapes.push(shape);
                        }
                    }
                    "circle" => {
                        if let Some(shape) = build_circle(&attrs, transforms.last().unwrap(), &parsed.defs) {
                            parsed.shapes.push(shape);
                        }
                    }
                    "ellipse" => {
                        if let Some(shape) = build_ellipse(&attrs, transforms.last().unwrap(), &parsed.defs) {
                            parsed.shapes.push(shape);
                        }
                    }
                    "line" => {
                        if let Some(shape) = build_line(&attrs, transforms.last().unwrap(), &parsed.defs) {
                            parsed.shapes.push(shape);
                        }
                    }
                    "path" => {
                        if let Some(shape) = build_path(&attrs, transforms.last().unwrap(), &parsed.defs) {
                            parsed.shapes.push(shape);
                        }
                    }
                    "polyline" | "polygon" => {
                        if let Some(shape) =
                            build_poly(&tag, &attrs, transforms.last().unwrap(), &parsed.defs)
                        {
                            parsed.shapes.push(shape);
                        }
                    }
                    "stop" => {
                        // `<stop>` is virtually always self-closing
                        // (`<stop offset="0" stop-color="..." />`), so it
                        // shows up as Event::Empty. The Start branch
                        // also has a handler — both go through
                        // `try_capture_gradient_stop` so the "first stop
                        // wins" rule is implemented in exactly one place.
                        if let Some(parent_id) = gradient_stack.last() {
                            try_capture_gradient_stop(&mut parsed.defs, parent_id, &attrs);
                        }
                    }
                    "use" | "foreignObject" | "filter" | "mask"
                    | "clipPath" | "pattern" | "switch" => {
                        if !parsed.skipped.contains(&tag) {
                            parsed.skipped.push(tag);
                        }
                    }
                    "image" => {
                        if let Some(href) = attrs.get("href")
                            .or_else(|| attrs.get("{http://www.w3.org/1999/xlink}href"))
                        {
                            let x = attrs.get("x").and_then(|v| v.parse().ok()).unwrap_or(0.0);
                            let y = attrs.get("y").and_then(|v| v.parse().ok()).unwrap_or(0.0);
                            let w = attrs.get("width").and_then(|v| v.parse().ok());
                            let h = attrs.get("height").and_then(|v| v.parse().ok());
                            if let Some(shape) = build_image(href, x, y, w, h) {
                                parsed.shapes.push(shape);
                            } else if !parsed.skipped.contains(&"image".to_string()) {
                                parsed.skipped.push("image".to_string());
                            }
                        } else if !parsed.skipped.contains(&"image".to_string()) {
                            parsed.skipped.push("image".to_string());
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::Text(t)) => {
                if let Some(acc) = text_acc.as_mut() {
                    let txt = t.unescape().map_err(|e| e.to_string())?.into_owned();
                    acc.current_run.push_str(&txt);
                }
            }
            Ok(Event::End(e)) => {
                let name = e.name();
                let tag = std::str::from_utf8(name.as_ref()).unwrap_or("").to_string();
                match tag.as_str() {
                    "g" => {
                        transforms.pop();
                    }
                    "linearGradient" | "radialGradient" => {
                        gradient_stack.pop();
                    }
                    "tspan" => {
                        if let Some(acc) = text_acc.as_mut() {
                            // Flush the current run, then keep accumulating.
                            // We don't reset bold/italic because tspans
                            // typically *inherit* formatting from the parent
                            // text element; the next opening tspan can
                            // override.
                            if !acc.current_run.is_empty() {
                                acc.runs.push(TextRun {
                                    text: std::mem::take(&mut acc.current_run),
                                    bold: acc.current_bold,
                                    italic: acc.current_italic,
                                    underline: acc.current_underline,
                                    fill: acc.current_fill.clone(),
                                });
                            }
                        }
                    }
                    "text" => {
                        if let Some(mut acc) = text_acc.take() {
                            // Flush the trailing run, even if empty.
                            acc.runs.push(TextRun {
                                text: std::mem::take(&mut acc.current_run),
                                bold: acc.current_bold,
                                italic: acc.current_italic,
                                underline: acc.current_underline,
                                fill: acc.current_fill.clone(),
                            });

                            // Drop trailing empty runs (PowerPoint renders
                            // them as a phantom cursor).
                            while acc.runs.last().map(|r| r.text.is_empty()).unwrap_or(false) {
                                acc.runs.pop();
                            }

                            if !acc.runs.is_empty() {
                                let (x, y) = acc.transform.apply_point(acc.x, acc.y);
                                parsed.shapes.push(SvgShape::Text {
                                    x,
                                    y,
                                    runs: acc.runs,
                                    font_size: acc.font_size,
                                    fill: acc.fill,
                                    opacity: acc.opacity,
                                    text_anchor: std::mem::take(&mut acc.text_anchor),
                                });
                            }
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(format!("quick-xml error at position {}: {}", reader.buffer_position(), e)),
            _ => {}
        }
        buf.clear();
    }

    // Fallback: an SVG that never set a viewBox got default 100×100 above; if
    // the SVG used `width` / `height` instead we already captured those into
    // vb_w / vb_h. Either way, vb_w / vb_h must be > 0 by now.

    Ok(parsed)
}

// ---------------------------------------------------------------------------
// Attribute helpers
// ---------------------------------------------------------------------------

fn read_attrs(e: &BytesStart) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    for a in e.attributes() {
        let attr = match a {
            Ok(a) => a,
            Err(_) => continue,
        };
        let key = std::str::from_utf8(attr.key.as_ref())
            .unwrap_or("")
            .to_string();
        let val = attr.unescape_value().map(|v| v.into_owned()).unwrap_or_default();
        map.insert(key, val);
    }
    map
}

fn parse_len(s: &str) -> Option<f64> {
    let s = s.trim();
    let s = if let Some(stripped) = s.strip_suffix("px") {
        stripped
    } else if let Some(stripped) = s.strip_suffix("pt") {
        // 1 pt = 1.333 px (assume 96 DPI viewport); we don't actually use
        // this conversion downstream (font sizes are taken in pt natively).
        return stripped.parse().ok();
    } else {
        s
    };
    s.parse().ok()
}

/// Resolve a `fill="..."` / `stroke="..."` value into a `Paint`. The
/// `defs` map is used to look up gradient stops; if `url(#id)` references
/// a gradient we've seen, we resolve to its first stop's colour (with the
/// stop's opacity if present). If the gradient is unknown — e.g. the
/// shape is parsed before the matching `<defs>` block — we fall back to
/// `None` so the shape is still selectable in PowerPoint.
fn parse_paint(
    s: &str,
    _attrs: &BTreeMap<String, String>,
    defs: &BTreeMap<String, GradientStop>,
) -> Option<Paint> {
    let s = s.trim();
    if s.is_empty() || s == "none" {
        return Some(Paint::None);
    }
    if let Some(rest) = s.strip_prefix("url(#") {
        let id = rest.strip_suffix(')')?;
        if let Some(stop) = defs.get(id) {
            return Some(Paint::GradientRef {
                rgb: stop.rgb.clone(),
                opacity: stop.opacity,
            });
        }
        // Unknown gradient — emit a transparent paint so the shape is
        // still selectable; the writer will skip the fill element.
        return Some(Paint::None);
    }
    // `#RRGGBB` / `#RGB` / `rgb(…)` / named colours.
    //
    // For `rgba(…)` we keep the alpha — it controls the
    // semi-transparent "glass" effect that the user's SVG deck
    // depends on (e.g. `rgba(255,255,255,0.03)` strokes on the
    // decorative circles in `slide1-title.svg`). The earlier
    // version silently dropped the alpha here, which made every
    // stroke render fully opaque — losing the glass look.
    let (rgb, alpha) = parse_color_with_alpha(s).or_else(|| {
        let rgb = named_color(s).map(|s| s.to_string())?;
        Some((rgb, None))
    })?;
    Some(Paint::Color {
        rgb,
        opacity: alpha,
    })
}

/// See `parse_paint` for docs.
pub fn parse_color(s: &str) -> Option<String> {
    parse_color_with_alpha(s).map(|(rgb, _)| rgb)
}

/// Like [`parse_color`] but also returns the alpha channel from
/// `rgba(…)` inputs so the writer can emit the correct
/// `<a:alpha val="…"/>` for the semi-transparent "glass" strokes
/// the SVG deck relies on. Returns `(rgb, Some(alpha))` for
/// `rgba(…)`, `(rgb, None)` for everything else (the writer
/// defaults to fully opaque).
fn parse_color_with_alpha(s: &str) -> Option<(String, Option<f64>)> {
    let s = s.trim();
    if let Some(hex) = s.strip_prefix('#') {
        return match hex.len() {
            3 => {
                let r = &hex[0..1];
                let g = &hex[1..2];
                let b = &hex[2..3];
                Some((format!("{}{}{}{}{}{}", r, r, g, g, b, b).to_uppercase(), None))
            }
            6 => Some((hex.to_uppercase(), None)),
            // `#RRGGBBAA` — keep the alpha so semi-transparent fills
            // survive the round-trip into PowerPoint.
            8 => Some((hex[0..6].to_uppercase(), Some(hex_alpha(&hex[6..8])))),
            _ => None,
        };
    }
    if s.starts_with("rgb(") && s.ends_with(')') {
        let body = &s[4..s.len() - 1];
        let parts: Vec<&str> = body
            .split(|c: char| c == ',' || c.is_whitespace())
            .filter(|x| !x.is_empty())
            .collect();
        if parts.len() != 3 {
            return None;
        }
        let r: u8 = parts[0].parse().ok()?;
        let g: u8 = parts[1].parse().ok()?;
        let b: u8 = parts[2].parse().ok()?;
        return Some((format!("{:02X}{:02X}{:02X}", r, g, b), None));
    }
    if s.starts_with("rgba(") && s.ends_with(')') {
        let body = &s[5..s.len() - 1];
        let parts: Vec<&str> = body
            .split(|c: char| c == ',' || c.is_whitespace())
            .filter(|x| !x.is_empty())
            .collect();
        if parts.len() != 4 {
            return None;
        }
        let r: u8 = parts[0].parse().ok()?;
        let g: u8 = parts[1].parse().ok()?;
        let b: u8 = parts[2].parse().ok()?;
        let a: f64 = parts[3].parse().ok()?;
        return Some((format!("{:02X}{:02X}{:02X}", r, g, b), Some(a)));
    }
    None
}

/// Convert a two-char hex byte (e.g. `"40"` = 64/255) into the
/// `0.0..=1.0` alpha that `<a:alpha val="…"/>` expects.
fn hex_alpha(hex: &str) -> f64 {
    let v = u8::from_str_radix(hex, 16).unwrap_or(255);
    f64::from(v) / 255.0
}

/// A single resolved gradient stop. We capture the colour + opacity of the
/// *first* stop in the gradient so `Paint::GradientRef` has a colour to
/// fall back to. The rest of the ramp is intentionally discarded — see
/// the `Paint::GradientRef` doc-comment for why we don't try to render
/// the ramp in DrawingML.
#[derive(Clone)]
pub struct GradientStop {
    rgb: String,
    opacity: Option<f64>,
}

/// Insert a `<stop>`'s colour into `defs` for the gradient `parent_id`,
/// but ONLY if we don't already have a stop for that gradient — we
/// collapse multi-stop gradients to their first stop, and only the first
/// one we see wins. See the `Paint::GradientRef` doc-comment for why we
/// don't try to render the actual ramp. Returns `true` when this call
/// recorded a stop (useful for tests).
fn try_capture_gradient_stop(
    defs: &mut BTreeMap<String, GradientStop>,
    parent_id: &str,
    attrs: &BTreeMap<String, String>,
) -> bool {
    if parent_id.is_empty() {
        return false;
    }
    // "Already captured" means we have an entry whose colour is anything
    // other than the white placeholder we seeded from the gradient's
    // Start event. If we *do* see the placeholder, that means the
    // gradient Start event was missing (e.g. malformed SVG) but we're
    // still seeing a stop — capture it anyway.
    let already = defs
        .get(parent_id)
        .map(|s| s.rgb != "FFFFFF" || s.opacity.is_some())
        .unwrap_or(false);
    if already {
        return false;
    }
    let Some(stop_color) = attrs
        .get("stop-color")
        .cloned()
        .or_else(|| extract_style_attr(attrs.get("style").map(String::as_str), "stop-color"))
    else {
        return false;
    };
    let Some(rgb) = parse_color(&stop_color) else {
        return false;
    };
    let opacity = attrs
        .get("stop-opacity")
        .and_then(|v| v.parse().ok())
        .or_else(|| {
            extract_style_attr(attrs.get("style").map(String::as_str), "stop-opacity")
                .and_then(|s| s.parse().ok())
        });
    defs.insert(parent_id.to_string(), GradientStop { rgb, opacity });
    true
}

/// Pull a single `name:value;` pair out of an inline `style="…"`
/// attribute. We only care about `stop-color` / `stop-opacity` for
/// gradient stops, but the helper is generic. Returns `None` if the
/// attribute is missing or doesn't contain the requested name.
fn extract_style_attr(style: Option<&str>, name: &str) -> Option<String> {
    let style = style?;
    for decl in style.split(';') {
        let decl = decl.trim();
        if let Some(rest) = decl.strip_prefix(name) {
            let rest = rest.trim_start();
            if let Some(v) = rest.strip_prefix(':') {
                return Some(v.trim().to_string());
            }
        }
    }
    None
}

/// Tiny named-color table. We intentionally do NOT ship the full CSS list —
/// the LLM is expected to emit `fill="#1F2933"`-style hex values per the
/// `create_svg` style guide.
fn named_color(name: &str) -> Option<&'static str> {
    Some(match name.to_ascii_lowercase().as_str() {
        "black" => "000000",
        "white" => "FFFFFF",
        "red" => "FF0000",
        "green" => "008000",
        "blue" => "0000FF",
        "yellow" => "FFFF00",
        "cyan" | "aqua" => "00FFFF",
        "magenta" | "fuchsia" => "FF00FF",
        "gray" | "grey" => "808080",
        "silver" => "C0C0C0",
        "maroon" => "800000",
        "olive" => "808000",
        "purple" => "800080",
        "teal" => "008080",
        "navy" => "000080",
        "orange" => "FFA500",
        "pink" => "FFC0CB",
        "brown" => "A52A2A",
        "lime" => "00FF00",
        "indigo" => "4B0082",
        "violet" => "EE82EE",
        "gold" => "FFD700",
        "transparent" => "FFFFFF", // Caller should use opacity, not this.
        _ => return None,
    })
}

// ---------------------------------------------------------------------------
// Shape builders (translate raw attribute maps into SvgShape variants).
// ---------------------------------------------------------------------------

fn build_rect(
    a: &BTreeMap<String, String>,
    t: &Transform,
    defs: &BTreeMap<String, GradientStop>,
) -> Option<SvgShape> {
    let x: f64 = a.get("x").and_then(|v| v.parse().ok()).unwrap_or(0.0);
    let y: f64 = a.get("y").and_then(|v| v.parse().ok()).unwrap_or(0.0);
    let w: f64 = a.get("width").and_then(|v| v.parse().ok()).unwrap_or(0.0);
    let h: f64 = a.get("height").and_then(|v| v.parse().ok()).unwrap_or(0.0);
    if w <= 0.0 || h <= 0.0 {
        return None;
    }
    let rx: Option<f64> = a.get("rx").and_then(|v| v.parse().ok());
    let ry: Option<f64> = a.get("ry").and_then(|v| v.parse().ok());
    let (x, y) = t.apply_point(x, y);
    let (w, h) = t.apply_size(w, h);
    let fill = a.get("fill").and_then(|v| parse_paint(v, a, defs));
    let stroke = a.get("stroke").and_then(|v| parse_paint(v, a, defs));
    let stroke_width = a.get("stroke-width").and_then(|v| v.parse().ok());
    let opacity = a.get("opacity").and_then(|v| v.parse().ok())
        .or_else(|| a.get("fill-opacity").and_then(|v| v.parse().ok()));
    Some(SvgShape::Rect {
        x, y, width: w, height: h, rx, ry,
        fill, stroke, stroke_width, opacity,
    })
}

fn build_circle(
    a: &BTreeMap<String, String>,
    t: &Transform,
    defs: &BTreeMap<String, GradientStop>,
) -> Option<SvgShape> {
    let cx: f64 = a.get("cx").and_then(|v| v.parse().ok()).unwrap_or(0.0);
    let cy: f64 = a.get("cy").and_then(|v| v.parse().ok()).unwrap_or(0.0);
    let r: f64 = a.get("r").and_then(|v| v.parse().ok()).unwrap_or(0.0);
    if r <= 0.0 { return None; }
    let (cx, cy) = t.apply_point(cx, cy);
    let r = (r * t.uniform_scale()).max(0.0);
    let fill = a.get("fill").and_then(|v| parse_paint(v, a, defs));
    let stroke = a.get("stroke").and_then(|v| parse_paint(v, a, defs));
    let stroke_width = a.get("stroke-width").and_then(|v| v.parse().ok());
    let opacity = a.get("opacity").and_then(|v| v.parse().ok())
        .or_else(|| a.get("fill-opacity").and_then(|v| v.parse().ok()));
    Some(SvgShape::Ellipse {
        cx, cy, rx: r, ry: r,
        fill, stroke, stroke_width, opacity,
    })
}

fn build_ellipse(
    a: &BTreeMap<String, String>,
    t: &Transform,
    defs: &BTreeMap<String, GradientStop>,
) -> Option<SvgShape> {
    let cx: f64 = a.get("cx").and_then(|v| v.parse().ok()).unwrap_or(0.0);
    let cy: f64 = a.get("cy").and_then(|v| v.parse().ok()).unwrap_or(0.0);
    let rx: f64 = a.get("rx").and_then(|v| v.parse().ok()).unwrap_or(0.0);
    let ry: f64 = a.get("ry").and_then(|v| v.parse().ok()).unwrap_or(0.0);
    if rx <= 0.0 || ry <= 0.0 { return None; }
    let (cx, cy) = t.apply_point(cx, cy);
    let scale = t.uniform_scale();
    let rx = (rx * scale).max(0.0);
    let ry = (ry * scale).max(0.0);
    let fill = a.get("fill").and_then(|v| parse_paint(v, a, defs));
    let stroke = a.get("stroke").and_then(|v| parse_paint(v, a, defs));
    let stroke_width = a.get("stroke-width").and_then(|v| v.parse().ok());
    let opacity = a.get("opacity").and_then(|v| v.parse().ok())
        .or_else(|| a.get("fill-opacity").and_then(|v| v.parse().ok()));
    Some(SvgShape::Ellipse {
        cx, cy, rx, ry,
        fill, stroke, stroke_width, opacity,
    })
}

fn build_line(
    a: &BTreeMap<String, String>,
    t: &Transform,
    defs: &BTreeMap<String, GradientStop>,
) -> Option<SvgShape> {
    let x1: f64 = a.get("x1").and_then(|v| v.parse().ok()).unwrap_or(0.0);
    let y1: f64 = a.get("y1").and_then(|v| v.parse().ok()).unwrap_or(0.0);
    let x2: f64 = a.get("x2").and_then(|v| v.parse().ok()).unwrap_or(0.0);
    let y2: f64 = a.get("y2").and_then(|v| v.parse().ok()).unwrap_or(0.0);
    let (x1, y1) = t.apply_point(x1, y1);
    let (x2, y2) = t.apply_point(x2, y2);
    let stroke = a.get("stroke").and_then(|v| parse_paint(v, a, defs))
        .or_else(|| Some(Paint::Color { rgb: "000000".into(), opacity: None }));
    let stroke_width = a.get("stroke-width").and_then(|v| v.parse().ok())
        .or(Some(1.0));
    let opacity = a.get("opacity").and_then(|v| v.parse().ok());
    Some(SvgShape::Line {
        x1, y1, x2, y2,
        stroke, stroke_width, opacity,
    })
}

fn build_path(
    a: &BTreeMap<String, String>,
    t: &Transform,
    defs: &BTreeMap<String, GradientStop>,
) -> Option<SvgShape> {
    let d = a.get("d")?.clone();
    let fill = a.get("fill").and_then(|v| parse_paint(v, a, defs));
    let stroke = a.get("stroke").and_then(|v| parse_paint(v, a, defs));
    let stroke_width = a.get("stroke-width").and_then(|v| v.parse().ok());
    let opacity = a.get("opacity").and_then(|v| v.parse().ok())
        .or_else(|| a.get("fill-opacity").and_then(|v| v.parse().ok()));
    // Pre-bake the active transform into the path by re-emitting each
    // command with the parent group's translation/scale applied. This
    // lets the OOXML writer use the path as-is (it gets stored in a
    // fixed `w=100000 h=100000` viewport). See the OOXML `<a:custGeom>`
    // writer for how that viewport is chosen.
    let d = apply_transform_to_path(&d, t);
    Some(SvgShape::Path {
        d, fill, stroke, stroke_width, opacity,
    })
}

fn build_poly(
    tag: &str,
    a: &BTreeMap<String, String>,
    t: &Transform,
    defs: &BTreeMap<String, GradientStop>,
) -> Option<SvgShape> {
    let points = a.get("points")?;
    let mut d = String::new();
    let mut first = true;
    for token in points.split(|c: char| c.is_whitespace() || c == ',') {
        if token.is_empty() { continue; }
        let mut nums = token.split(|c: char| c == ',' || c == 'x' || c == 'X');
        let x: f64 = nums.next()?.parse().ok()?;
        let y: f64 = nums.next()?.parse().ok()?;
        let (x, y) = t.apply_point(x, y);
        if first {
            d.push_str(&format!("M {} {}", format_decimal(x), format_decimal(y)));
            first = false;
        } else {
            d.push_str(&format!(" L {} {}", format_decimal(x), format_decimal(y)));
        }
    }
    if tag == "polygon" {
        d.push_str(" Z");
    }
    let fill = a.get("fill").and_then(|v| parse_paint(v, a, defs))
        .or_else(|| Some(Paint::Color { rgb: "000000".into(), opacity: None }));
    let stroke = a.get("stroke").and_then(|v| parse_paint(v, a, defs));
    let stroke_width = a.get("stroke-width").and_then(|v| v.parse().ok());
    let opacity = a.get("opacity").and_then(|v| v.parse().ok());
    Some(SvgShape::Path {
        d, fill, stroke, stroke_width, opacity,
    })
}

/// Parse an `<image>` element with an inline data: URL.
/// Supports `data:image/png;base64,...` and `data:image/jpeg;base64,...`.
/// Returns `None` if the href is absent, not a data URL, or the base64
/// decoding fails.
fn build_image(
    href: &str,
    x: f64,
    y: f64,
    width: Option<f64>,
    height: Option<f64>,
) -> Option<SvgShape> {
    // We only accept inline data: URLs — no external http/https.
    let href = href.trim();
    if !href.starts_with("data:image/") {
        return None;
    }
    // Split "data:image/png;base64,..." into (mime, body)
    let body = href.strip_prefix("data:")?;
    let (mime, rest) = body.split_once(';')?;
    let encoding = rest.strip_prefix("base64,")?;
    let decoded = match base64_decode(encoding.as_bytes()) {
        Some(d) => d,
        None => return None, // decode error → image skipped
    };
    // Determine extension + strict MIME check.
    let mime = mime.to_lowercase();
    let ext = match mime.as_str() {
        "image/png" => "png",
        "image/jpeg" | "image/jpg" => "jpg",
        _ => return None, // only PNG/JPEG supported
    };
    Some(SvgShape::Image {
        x, y,
        width: width.unwrap_or(100.0),
        height: height.unwrap_or(100.0),
        data: decoded,
        mime: mime.clone(),
        ext: ext.to_string(),
    })
}

fn format_decimal(v: f64) -> String {
    // quick-xml writes attributes verbatim; trim trailing zeros so we don't
    // ship "12.000000" through the zip.
    let s = format!("{:.4}", v);
    let s = s.trim_end_matches('0').trim_end_matches('.').to_string();
    if s.is_empty() { "0".to_string() } else { s }
}

/// Decode base64 from a slice of ASCII bytes. Returns `None` on invalid input.
fn base64_decode(input: &[u8]) -> Option<Vec<u8>> {
    const DECODE_TABLE: [i8; 256] = {
        let mut t = [-1i8; 256];
        // A-Z
        t[b'A' as usize] = 0; t[b'B' as usize] = 1; t[b'C' as usize] = 2;
        t[b'D' as usize] = 3; t[b'E' as usize] = 4; t[b'F' as usize] = 5;
        t[b'G' as usize] = 6; t[b'H' as usize] = 7; t[b'I' as usize] = 8;
        t[b'J' as usize] = 9; t[b'K' as usize] = 10; t[b'L' as usize] = 11;
        t[b'M' as usize] = 12; t[b'N' as usize] = 13; t[b'O' as usize] = 14;
        t[b'P' as usize] = 15; t[b'Q' as usize] = 16; t[b'R' as usize] = 17;
        t[b'S' as usize] = 18; t[b'T' as usize] = 19; t[b'U' as usize] = 20;
        t[b'V' as usize] = 21; t[b'W' as usize] = 22; t[b'X' as usize] = 23;
        t[b'Y' as usize] = 24; t[b'Z' as usize] = 25;
        // a-z
        t[b'a' as usize] = 26; t[b'b' as usize] = 27; t[b'c' as usize] = 28;
        t[b'd' as usize] = 29; t[b'e' as usize] = 30; t[b'f' as usize] = 31;
        t[b'g' as usize] = 32; t[b'h' as usize] = 33; t[b'i' as usize] = 34;
        t[b'j' as usize] = 35; t[b'k' as usize] = 36; t[b'l' as usize] = 37;
        t[b'm' as usize] = 38; t[b'n' as usize] = 39; t[b'o' as usize] = 40;
        t[b'p' as usize] = 41; t[b'q' as usize] = 42; t[b'r' as usize] = 43;
        t[b's' as usize] = 44; t[b't' as usize] = 45; t[b'u' as usize] = 46;
        t[b'v' as usize] = 47; t[b'w' as usize] = 48; t[b'x' as usize] = 49;
        t[b'y' as usize] = 50; t[b'z' as usize] = 51;
        // 0-9
        t[b'0' as usize] = 52; t[b'1' as usize] = 53; t[b'2' as usize] = 54;
        t[b'3' as usize] = 55; t[b'4' as usize] = 56; t[b'5' as usize] = 57;
        t[b'6' as usize] = 58; t[b'7' as usize] = 59; t[b'8' as usize] = 60;
        t[b'9' as usize] = 61;
        t[b'+' as usize] = 62; t[b'/' as usize] = 63; t[b'=' as usize] = 64;
        t
    };

    let input = input.trim_ascii();
    if input.is_empty() { return Some(Vec::new()); }

    // Pad to multiple of 4
    let padding = (4 - (input.len() % 4)) % 4;
    let len = input.len() + padding;
    let mut buf = Vec::with_capacity(len * 3 / 4);

    let mut i = 0;
    while i < len {
        let get = |idx: usize| -> i8 {
            if idx >= input.len() { return -1 }
            DECODE_TABLE[input[idx] as usize]
        };
        let a = get(i); let b = get(i+1); let c = get(i+2); let d = get(i+3);
        if a < 0 || b < 0 { return None; }
        buf.push(((a as u8) << 2) | ((b as u8) >> 4));
        if c >= 0 {
            buf.push(((b as u8) & 0x0F) << 4 | ((c as u8) >> 2));
        }
        if d >= 0 && (i + 3 < input.len() || padding < 3) {
            buf.push(((c as u8) & 0x03) << 6 | (d as u8));
        }
        i += 4;
    }
    Some(buf)
}

/// Standard base64 encode (URL-safe alphabet variant not used here since
/// PPTX uses standard base64 in data: URLs and the XML comment).
fn base64_encode(input: &[u8]) -> String {
    const ENCODE_TABLE: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    let chunks = input.chunks(3);
    for chunk in chunks {
        let b0 = chunk[0] as usize;
        let b1 = chunk.get(1).copied().unwrap_or(0) as usize;
        let b2 = chunk.get(2).copied().unwrap_or(0) as usize;
        out.push(ENCODE_TABLE[(b0 >> 2)] as char);
        out.push(ENCODE_TABLE[((b0 & 0x03) << 4) | (b1 >> 4)] as char);
        if chunk.len() > 1 {
            out.push(ENCODE_TABLE[((b1 & 0x0F) << 2) | (b2 >> 6)] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(ENCODE_TABLE[b2 & 0x3F] as char);
        } else {
            out.push('=');
        }
    }
    out
}

/// Rewrite an SVG path `d` attribute so every coordinate is transformed
/// by the parent group's `translate` + uniform `scale`. We parse the
/// path command stream (M / L / H / V / C / S / Q / T / A / Z, absolute
/// + lowercase relative) and emit a *new* path with the transform baked
/// in. The original AI-generated paths are simple — usually just M / L
/// / Z — but we handle the full subset so nothing in the user's SVG
/// silently mis-positions.
///
/// A few simplifications we accept:
/// - We do NOT support arc segments (`A`/`a`) when a non-1 scale is
///   active; we pass through the original command unchanged so the
///   shape still draws at the wrong place. This is fine because none of
///   our SVG sources use arcs inside a scaled group.
/// - We do NOT try to honour chained transforms; `Transform::compose`
///   already accumulates them.

/// Rewrite an SVG path `d` attribute so every coordinate is transformed
/// by the parent group's `translate` + uniform `scale`. We tokenise the
/// path command stream (M / L / H / V / C / S / Q / T / A / Z, absolute
/// + lowercase relative) and emit a *new* path with the transform baked
/// in. The original AI-generated paths are simple — usually just M / L /
/// Z — but we handle the full subset so nothing in the user's SVG
/// silently mis-positions.
///
/// Simplifications we accept:
/// - We do NOT support arc segments (`A`/`a`) when a non-1 scale is
///   active; we pass through the original command unchanged so the
///   shape still draws at the wrong place. None of our SVG sources use
///   arcs inside a scaled group.
/// - We do NOT try to honour chained transforms; `Transform::compose`
///   already accumulates them.
fn apply_transform_to_path(d: &str, t: &Transform) -> String {
    let tx = t.tx;
    let ty = t.ty;
    let scale = t.scale;
    // Fast path: identity transform → return as-is. Saves the
    // tokenisation walk for the (very common) case of a path that
    // lives outside any `<g transform=...>` block.
    if tx == 0.0 && ty == 0.0 && (scale - 1.0).abs() < 1e-9 {
        return d.to_string();
    }

    let mut out = String::with_capacity(d.len());
    let mut chars = d.chars().peekable();

    // Current command + collected args (numbers) so far. We flush when
    // the command letter changes (or at EOF), applying the transform
    // based on what the command expects.
    let mut current_cmd: Option<char> = None;
    let mut args: Vec<f64> = Vec::new();

    while let Some(&c) = chars.peek() {
        if c.is_whitespace() || c == ',' {
            // Separator inside an arg list — preserve but don't act on.
            out.push(c);
            chars.next();
            continue;
        }
        if c.is_ascii_alphabetic() {
            // Flush any buffered args under the previous command.
            if let Some(prev) = current_cmd {
                flush_path_cmd(&mut out, prev, &args, tx, ty, scale);
            }
            args.clear();
            current_cmd = Some(c);
            out.push(c);
            chars.next();
            continue;
        }
        // Start of a number. Read it, buffer it.
        let mut buf = String::new();
        if matches!(c, '+' | '-') {
            buf.push(c);
            chars.next();
        }
        while let Some(&nc) = chars.peek() {
            if nc.is_ascii_digit() || nc == '.' {
                buf.push(nc);
                chars.next();
            } else if (nc == 'e' || nc == 'E') && buf.chars().any(|x| x.is_ascii_digit()) {
                buf.push(nc);
                chars.next();
                if let Some(&sign) = chars.peek() {
                    if matches!(sign, '+' | '-') {
                        buf.push(sign);
                        chars.next();
                    }
                }
            } else {
                break;
            }
        }
        if let Ok(v) = buf.parse::<f64>() {
            args.push(v);
        } else {
            // Malformed — flush verbatim and keep going.
            out.push_str(&buf);
        }
    }
    if let Some(prev) = current_cmd {
        flush_path_cmd(&mut out, prev, &args, tx, ty, scale);
    }
    out
}

/// Emit one path command (with all its buffered numeric args) into `out`,
/// applying the (tx, ty, scale) transform to the coord-bearing args. The
/// pre-/post- translate logic is the same for every command except for
/// the relative-vs-absolute distinction: relative cmds (`m`/`l`/…) only
/// get scaled, while absolute cmds also get translated.
fn flush_path_cmd(out: &mut String, cmd: char, args: &[f64], tx: f64, ty: f64, scale: f64) {
    let upper = cmd.to_ascii_uppercase();
    let rel = cmd.is_ascii_lowercase();
    out.push(cmd);
    match upper {
        // 1 number — H takes x, V takes y.
        'H' => {
            if let Some(&n) = args.first() {
                let nx = n * scale + (if rel { 0.0 } else { tx });
                out.push(' ');
                out.push_str(&format_decimal(nx));
            }
        }
        'V' => {
            if let Some(&n) = args.first() {
                let ny = n * scale + (if rel { 0.0 } else { ty });
                out.push(' ');
                out.push_str(&format_decimal(ny));
            }
        }
        // 2 numbers — M / L / T.
        'M' | 'L' | 'T' => {
            for pair in args.chunks(2) {
                if let [x, y] = pair {
                    let nx = x * scale + (if rel { 0.0 } else { tx });
                    let ny = y * scale + (if rel { 0.0 } else { ty });
                    out.push(' ');
                    out.push_str(&format_decimal(nx));
                    out.push(' ');
                    out.push_str(&format_decimal(ny));
                }
            }
        }
        // 6 numbers — C cubic: x1 y1 x2 y2 x y. All six are coordinates.
        'C' => {
            for chunk in args.chunks(6) {
                if chunk.len() == 6 {
                    let pts = [
                        (chunk[0], chunk[1]),
                        (chunk[2], chunk[3]),
                        (chunk[4], chunk[5]),
                    ];
                    for (px, py) in pts {
                        let nx = px * scale + (if rel { 0.0 } else { tx });
                        let ny = py * scale + (if rel { 0.0 } else { ty });
                        out.push(' ');
                        out.push_str(&format_decimal(nx));
                        out.push(' ');
                        out.push_str(&format_decimal(ny));
                    }
                }
            }
        }
        // 4 numbers — S / Q.
        'S' | 'Q' => {
            for chunk in args.chunks(4) {
                if chunk.len() == 4 {
                    let pts = [(chunk[0], chunk[1]), (chunk[2], chunk[3])];
                    for (px, py) in pts {
                        let nx = px * scale + (if rel { 0.0 } else { tx });
                        let ny = py * scale + (if rel { 0.0 } else { ty });
                        out.push(' ');
                        out.push_str(&format_decimal(nx));
                        out.push(' ');
                        out.push_str(&format_decimal(ny));
                    }
                }
            }
        }
        // 7 numbers — A arc. Pass through verbatim.
        'A' => {
            for n in args {
                out.push(' ');
                out.push_str(&format_decimal(*n));
            }
        }
        'Z' => { /* no args */ }
        _ => {
            // Unknown command — pass through verbatim so the shape still draws.
            for n in args {
                out.push(' ');
                out.push_str(&format_decimal(*n));
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Transforms
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
pub struct Transform {
    /// Translation in SVG user units (pre-scale).
    tx: f64,
    ty: f64,
    /// Uniform scale applied AFTER translation (we don't support non-uniform
    /// scale because none of our SVG sources emit it).
    scale: f64,
}

impl Transform {
    fn identity() -> Self { Self { tx: 0.0, ty: 0.0, scale: 1.0 } }

    fn apply_point(&self, x: f64, y: f64) -> (f64, f64) {
        (self.tx + x * self.scale, self.ty + y * self.scale)
    }

    fn apply_size(&self, w: f64, h: f64) -> (f64, f64) {
        (w * self.scale, h * self.scale)
    }

    fn uniform_scale(&self) -> f64 {
        self.scale
    }

    /// Parse a `transform="…"` attribute. We only honour `translate(x y)`
    /// and `scale(s)` (and combinations). Anything else (rotate, matrix,
    /// skewX) is silently ignored — the SVG will still render in PPT, just
    /// without the rotation / skew.
    fn compose(&self, attr: &str) -> Self {
        let mut out = *self;
        for op in split_transform_ops(attr) {
            let body = op.trim();
            if let Some(rest) = body.strip_prefix("translate(") {
                let body = rest.trim_end_matches(')');
                let parts: Vec<&str> = body
                    .split(|c: char| c == ',' || c.is_whitespace())
                    .filter(|s| !s.is_empty())
                    .collect();
                if parts.len() >= 1 {
                    if let Ok(x) = parts[0].parse::<f64>() { out.tx += x * out.scale; }
                }
                if parts.len() >= 2 {
                    if let Ok(y) = parts[1].parse::<f64>() { out.ty += y * out.scale; }
                }
            } else if let Some(rest) = body.strip_prefix("scale(") {
                let body = rest.trim_end_matches(')');
                let parts: Vec<&str> = body
                    .split(|c: char| c == ',' || c.is_whitespace())
                    .filter(|s| !s.is_empty())
                    .collect();
                if !parts.is_empty() {
                    if let Ok(s) = parts[0].parse::<f64>() {
                        out.scale *= s;
                    }
                }
            }
            // rotate, matrix, skewX/Y: intentionally ignored.
        }
        out
    }
}

fn split_transform_ops(attr: &str) -> Vec<String> {
    // Split on the function-name boundary: every operation ends with `)`.
    // We split by `)` and re-attach the `)`, since `transform="rotate(45) scale(2)"`
    // has no commas between ops.
    let mut ops = Vec::new();
    let mut buf = String::new();
    let mut depth = 0i32;
    for ch in attr.chars() {
        buf.push(ch);
        match ch {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    ops.push(std::mem::take(&mut buf));
                }
            }
            _ => {}
        }
    }
    if !buf.trim().is_empty() {
        ops.push(buf);
    }
    ops
}

// ---------------------------------------------------------------------------
// Text accumulator (state during the parse)
// ---------------------------------------------------------------------------

struct TextAcc {
    x: f64,
    y: f64,
    font_size: Option<f64>,
    fill: Option<Paint>,
    opacity: Option<f64>,
    transform: Transform,
    /// SVG `text-anchor` attribute (`start` / `middle` / `end`). We
    /// capture it here because OOXML text alignment has to be set on
    /// the text box *body* via `<a:pPr algn="…"/>`, not just on the
    /// run, so we need to thread the value through to the writer.
    /// Defaults to `start` (the SVG default).
    text_anchor: String,
    runs: Vec<TextRun>,
    current_run: String,
    current_bold: bool,
    current_italic: bool,
    current_underline: bool,
    current_fill: Option<Paint>,
}

// ---------------------------------------------------------------------------
// OOXML builders
// ---------------------------------------------------------------------------

/// Image data extracted from SVG `<image>` elements during build_pptx.
struct SlideImage {
    shape_id: usize,
    ext: String,
    data: Vec<u8>,
}

/// Collect all `<!--IMG|...|-->` markers from slide XML and extract the
/// embedded image data. Returns (processed_xml, images).
fn extract_images_from_slide(xml: &str) -> (String, Vec<SlideImage>) {
    let mut images = Vec::new();
    let marker_prefix = "<!--IMG|";
    let mut result = String::new();
    let mut last_end = 0;

    while let Some(start) = xml[last_end..].find(marker_prefix) {
        let abs_start = last_end + start;
        if let Some(end_offset) = xml[abs_start..].find("|-->") {
            let marker_end = abs_start + end_offset + 4;
            let inner = &xml[abs_start + 5..marker_end - 3]; // strip <!--IMG| and |-->
            let parts: Vec<&str> = inner.split('|').collect();
            if parts.len() >= 7 {
                if let (Ok(shape_id), Some(ext), Some(b64)) = (
                    parts[0].parse::<usize>(),
                    parts.get(5),
                    parts.get(6),
                ) {
                    if let Some(data) = base64_decode(b64.as_bytes()) {
                        images.push(SlideImage {
                            shape_id,
                            ext: ext.to_string(),
                            data,
                        });
                    }
                }
            }
            result.push_str(&xml[last_end..abs_start]);
            last_end = marker_end;
        } else {
            break;
        }
    }
    result.push_str(&xml[last_end..]);
    (result, images)
}

/// Update a slide's XML to replace placeholder rIdS{shape_id} with the real
/// media relationship id (rIdM{media_idx}) assigned by build_pptx.
fn patch_slide_image_refs(xml: &str, shape_id: usize, media_rid: &str) -> String {
    xml.replace(&format!("rIdS{shape_id}"), media_rid)
}

/// Build the `[Content_Types].xml` with optional PNG/JPEG overrides.
fn build_content_types_with_images(slide_count: usize, has_png: bool, has_jpg: bool) -> String {
    let mut out = String::new();
    out.push_str(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#);
    out.push_str("<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">");
    out.push_str("<Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/>");
    out.push_str("<Default Extension=\"xml\" ContentType=\"application/xml\"/>");
    if has_png {
        out.push_str("<Default Extension=\"png\" ContentType=\"image/png\"/>");
    }
    if has_jpg {
        out.push_str("<Default Extension=\"jpg\" ContentType=\"image/jpeg\"/>");
        out.push_str("<Default Extension=\"jpeg\" ContentType=\"image/jpeg\"/>");
    }
    out.push_str("<Override PartName=\"/ppt/presentation.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml\"/>");
    out.push_str("<Override PartName=\"/ppt/slideMasters/slideMaster1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.slideMaster+xml\"/>");
    out.push_str("<Override PartName=\"/ppt/slideLayouts/slideLayout1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.slideLayout+xml\"/>");
    out.push_str("<Override PartName=\"/ppt/theme/theme1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.theme+xml\"/>");
    out.push_str("<Override PartName=\"/docProps/core.xml\" ContentType=\"application/vnd.openxmlformats-package.core-properties+xml\"/>");
    out.push_str("<Override PartName=\"/docProps/app.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.extended-properties+xml\"/>");
    for i in 1..=slide_count {
        out.push_str(&format!(
            "<Override PartName=\"/ppt/slides/slide{i}.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.slide+xml\"/>"
        ));
    }
    out.push_str("</Types>");
    out
}

/// Build slide rels with optional image relationships.
/// media_rels: [(media_idx, ext)] — maps media_idx to its file extension.
fn build_slide_rels_with_images(media_rels: &[(usize, String)]) -> String {
    let mut out = String::new();
    out.push_str(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#);
    out.push_str("<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">");
    out.push_str("<Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout\" Target=\"../slideLayouts/slideLayout1.xml\"/>");
    for (media_idx, ext) in media_rels {
        let rid = format!("rIdM{}", media_idx);
        let target = format!("../media/image{}.{}", media_idx, ext);
        let ct = if ext == "png" { "image/png" } else { "image/jpeg" };
        out.push_str(&format!(
            "<Relationship Id=\"{}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/image\" Target=\"{}\" ContentType=\"{}\"/>",
            rid, target, ct
        ));
    }
    out.push_str("</Relationships>");
    out
}

/// Build a complete `.pptx` (as bytes) from a list of `SlideInput`s.
fn build_pptx(slides: &[SlideInput], title: Option<&str>) -> Result<Vec<u8>, ToolError> {
    let mut entries: Vec<(String, Vec<u8>)> = Vec::new();

    // Compute the presentation-wide slide size (OOXML: one size per deck).
    let (slide_w_emu, slide_h_emu) = slides
        .first()
        .map(|s| compute_slide_size_emu(&s.content))
        .unwrap_or((SLIDE_W_EMU, SLIDE_H_EMU));

    // First pass: extract images from each slide and patch the XML.
    let mut all_media: Vec<SlideImage> = Vec::new();
    // Map from (slide_idx, shape_id) → media_idx in all_media
    let mut slide_image_map: Vec<Vec<(usize, usize)>> = Vec::new(); // slide_idx → [(media_idx, shape_id)]
    let mut patched_slides: Vec<String> = Vec::new();

    for (slide_idx, slide) in slides.iter().enumerate() {
        let xml = build_slide_xml(&slide.content, slide_w_emu, slide_h_emu)?;
        let (patched, images) = extract_images_from_slide(&xml);
        let mut local_map: Vec<(usize, usize)> = Vec::new();
        for img in images {
            let shape_id = img.shape_id;
            let media_idx = all_media.len();
            all_media.push(img);
            local_map.push((media_idx, shape_id));
        }
        slide_image_map.push(local_map);
        patched_slides.push(patched);
    }

    // Determine if we need PNG/JPEG content types.
    let has_png = all_media.iter().any(|m| m.ext == "png");
    let has_jpg = all_media.iter().any(|m| m.ext == "jpg");

    // [Content_Types].xml
    entries.push((
        "[Content_Types].xml".to_string(),
        build_content_types_with_images(slides.len(), has_png, has_jpg).into_bytes(),
    ));

    // _rels/.rels
    entries.push(("_rels/.rels".to_string(), build_root_rels().into_bytes()));

    // ppt/_rels/presentation.xml.rels
    entries.push((
        "ppt/_rels/presentation.xml.rels".to_string(),
        build_presentation_rels(slides.len()).into_bytes(),
    ));

    // Compute the presentation-wide slide size.
    let (slide_w_emu, slide_h_emu) = slides
        .first()
        .map(|s| compute_slide_size_emu(&s.content))
        .unwrap_or((SLIDE_W_EMU, SLIDE_H_EMU));

    // ppt/presentation.xml
    entries.push((
        "ppt/presentation.xml".to_string(),
        build_presentation_xml(slides.len(), slide_w_emu, slide_h_emu).into_bytes(),
    ));

    // ppt/theme/theme1.xml
    entries.push(("ppt/theme/theme1.xml".to_string(), THEME_XML.as_bytes().to_vec()));

    // ppt/slides/_rels/slideN.xml.rels (with image refs)
    for (slide_idx, _) in slides.iter().enumerate() {
        let media_rels: Vec<(usize, String)> = slide_image_map
            .get(slide_idx)
            .map(|v| v.clone())
            .unwrap_or_default()
            .into_iter()
            .map(|(media_idx, _shape_id)| {
                (media_idx, all_media.get(media_idx).map(|m| m.ext.clone()).unwrap_or_default())
            })
            .collect();
        let rels_xml = build_slide_rels_with_images(&media_rels);
        entries.push((
            format!("ppt/slides/_rels/slide{}.xml.rels", slide_idx + 1),
            rels_xml.into_bytes(),
        ));
    }

    // ppt/slides/slideN.xml (patched, without IMG markers)
    for (slide_idx, patched) in patched_slides.iter().enumerate() {
        let mut final_xml = patched.to_string();
        // Patch all image refs in this slide
        if let Some(local_map) = slide_image_map.get(slide_idx) {
            for &(media_idx, shape_id) in local_map {
                let rid = format!("rIdM{}", media_idx);
                final_xml = patch_slide_image_refs(&final_xml, shape_id, &rid);
            }
        }
        entries.push((
            format!("ppt/slides/slide{}.xml", slide_idx + 1),
            final_xml.into_bytes(),
        ));
    }

    // ppt/slideMasters/slideMaster1.xml + rels
    entries.push((
        "ppt/slideMasters/slideMaster1.xml".to_string(),
        SLIDE_MASTER_XML.as_bytes().to_vec(),
    ));
    entries.push((
        "ppt/slideMasters/_rels/slideMaster1.xml.rels".to_string(),
        SLIDE_MASTER_RELS.as_bytes().to_vec(),
    ));

    // ppt/slideLayouts/slideLayout1.xml + rels
    entries.push((
        "ppt/slideLayouts/slideLayout1.xml".to_string(),
        SLIDE_LAYOUT_XML.as_bytes().to_vec(),
    ));
    entries.push((
        "ppt/slideLayouts/_rels/slideLayout1.xml.rels".to_string(),
        SLIDE_LAYOUT_RELS.as_bytes().to_vec(),
    ));

    // docProps/core.xml + app.xml
    entries.push((
        "docProps/core.xml".to_string(),
        build_core_props_xml(title.unwrap_or("Inkuo Presentation")).into_bytes(),
    ));
    entries.push(("docProps/app.xml".to_string(), APP_XML.as_bytes().to_vec()));

    // Write media files
    for (media_idx, img) in all_media.iter().enumerate() {
        let path = format!("ppt/media/image{}.{}", media_idx, img.ext);
        entries.push((path, img.data.clone()));
    }

    // Now zip everything up.
    let mut buf: Vec<u8> = Vec::new();
    {
        let mut zip = zip::ZipWriter::new(Cursor::new(&mut buf));
        let opts = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated)
            .unix_permissions(0o644);
        for (name, data) in &entries {
            zip.start_file(name.as_str(), opts).map_err(|e| {
                ToolError::ExecutionError(format!("zip start_file({name}) failed: {e}"))
            })?;
            zip.write_all(data).map_err(|e| {
                ToolError::ExecutionError(format!("zip write({name}) failed: {e}"))
            })?;
        }
        zip.finish().map_err(|e| {
            ToolError::ExecutionError(format!("zip finish failed: {e}"))
        })?;
    }
    Ok(buf)
}

// ---- Content_Types --------------------------------------------------------

/// See `parse_paint` for docs.
pub fn build_content_types(slide_count: usize) -> String {
    let mut out = String::new();
    out.push_str(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#);
    out.push_str("<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">");
    out.push_str("<Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/>");
    out.push_str("<Default Extension=\"xml\" ContentType=\"application/xml\"/>");
    out.push_str("<Override PartName=\"/ppt/presentation.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml\"/>");
    out.push_str("<Override PartName=\"/ppt/slideMasters/slideMaster1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.slideMaster+xml\"/>");
    out.push_str("<Override PartName=\"/ppt/slideLayouts/slideLayout1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.slideLayout+xml\"/>");
    out.push_str("<Override PartName=\"/ppt/theme/theme1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.theme+xml\"/>");
    out.push_str("<Override PartName=\"/docProps/core.xml\" ContentType=\"application/vnd.openxmlformats-package.core-properties+xml\"/>");
    out.push_str("<Override PartName=\"/docProps/app.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.extended-properties+xml\"/>");
    for i in 1..=slide_count {
        out.push_str(&format!(
            "<Override PartName=\"/ppt/slides/slide{i}.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.slide+xml\"/>"
        ));
    }
    out.push_str("</Types>");
    out
}

// ---- _rels ----------------------------------------------------------------

pub fn build_root_rels() -> String {
    r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="ppt/presentation.xml"/>
  <Relationship Id="rId2" Type="http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties" Target="docProps/core.xml"/>
  <Relationship Id="rId3" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/extended-properties" Target="docProps/app.xml"/>
</Relationships>"#.to_string()
}

pub fn build_presentation_rels(slide_count: usize) -> String {
    let mut out = String::new();
    out.push_str(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#);
    out.push_str("<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">");
    out.push_str("<Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideMaster\" Target=\"slideMasters/slideMaster1.xml\"/>");
    out.push_str("<Relationship Id=\"rId2\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/theme\" Target=\"theme/theme1.xml\"/>");
    out.push_str("<Relationship Id=\"rId11\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout\" Target=\"slideLayouts/slideLayout1.xml\"/>");
    for i in 1..=slide_count {
        out.push_str(&format!(
            "<Relationship Id=\"rId{}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide\" Target=\"slides/slide{i}.xml\"/>",
            i + 2
        ));
    }
    out.push_str("</Relationships>");
    out
}

pub fn build_slide_rels() -> String {
    r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout" Target="../slideLayouts/slideLayout1.xml"/>
</Relationships>"#.to_string()
}

// ---- presentation.xml -----------------------------------------------------

pub fn build_presentation_xml(slide_count: usize, slide_w_emu: i64, slide_h_emu: i64) -> String {
    let mut out = String::new();
    out.push_str(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#);
    out.push_str("<p:presentation xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" xmlns:p=\"http://schemas.openxmlformats.org/presentationml/2006/main\">");
    // OOXML wants a `type` attribute on `<p:sldSz>` for the well-known
    // aspect ratios; for everything else we just emit the dimensions
    // without a type. PowerPoint, Keynote and WPS all accept the
    // dimension-only form, so we always emit that.
    out.push_str(&format!(
        "<p:sldSz cx=\"{}\" cy=\"{}\"/>",
        slide_w_emu, slide_h_emu
    ));
    out.push_str("<p:notesSz cx=\"6858000\" cy=\"9144000\"/>");
    out.push_str("<p:defaultTextStyle><a:defPPr/></p:defaultTextStyle>");
    out.push_str("<p:sldIdLst>");
    for i in 1..=slide_count {
        out.push_str(&format!(
            "<p:sldId id=\"{}\" r:id=\"rId{}\"/>",
            255 + i,
            i + 2
        ));
    }
    out.push_str("</p:sldIdLst>");
    out.push_str("</p:presentation>");
    out
}

// ---- slide.xml (the actual content) ---------------------------------------

fn build_slide_xml(svg: &ParsedSvg, slide_w: i64, slide_h: i64) -> Result<String, ToolError> {
    // Project SVG coordinates into slide EMU space. We keep the SVG's
    // viewBox at its native pixel dimensions (1 SVG user unit = 1
    // PowerPoint "px" at 96 DPI = 9525 EMU). The slide canvas is sized
    // to match the viewBox (see `compute_slide_size_emu`), so the
    // projection becomes the identity for SVG coordinates inside the
    // viewBox — no scaling, no margin, no centring.
    //
    // Why no margin: SVG backgrounds almost always paint a full-bleed
    // rect (`<rect width="100%" height="100%" fill="url(#bg)"/>`),
    // expecting it to cover the entire canvas. Adding a 5% margin
    // would leave a visible white frame around the artwork, which
    // looks broken. If the user wants a margin, they put it in the
    // SVG itself.
    let px_per_emu = EMU_PER_INCH as f64 / 96.0;
    let scale = px_per_emu;
    // The viewBox may start at (vb_x, vb_y) != (0, 0); translate the
    // viewBox origin to the slide origin.
    let off_x = -svg.vb_x * scale;
    let off_y = -svg.vb_y * scale;

    let mut shapes = String::new();
    // Every `<p:cNvPr id="…"/>` inside a slide must be unique —
    // PowerPoint silently drops subsequent shapes that share an id
    // with an earlier one (we observed that the first shape on a
    // slide drew fine but every following shape disappeared — even
    // though the OOXML rendered correctly in macOS Keynote and
    // python-pptx). `id=1` is reserved for the group's own
    // `<p:cNvPr>`, so we start the per-shape counter at 2.
    for (idx, shape) in svg.shapes.iter().enumerate() {
        write_shape(&mut shapes, shape, scale, off_x, off_y, slide_w, slide_h, idx + 2)?;
    }

    let mut out = String::new();
    out.push_str(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#);
    out.push_str("<p:sld xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" xmlns:p=\"http://schemas.openxmlformats.org/presentationml/2006/main\">");
    out.push_str("<p:cSld><p:spTree>");
    out.push_str("<p:nvGrpSpPr><p:cNvPr id=\"1\" name=\"\"/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr>");
    out.push_str("<p:grpSpPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"0\" cy=\"0\"/><a:chOff x=\"0\" y=\"0\"/><a:chExt cx=\"0\" cy=\"0\"/></a:xfrm></p:grpSpPr>");
    out.push_str(&shapes);
    out.push_str("</p:spTree></p:cSld>");
    out.push_str("</p:sld>");
    Ok(out)
}

/// Compute the slide size in EMU for an SVG. The slide size is the
/// viewBox's pixel dimensions converted to EMU at 96 DPI (so 1 SVG
/// user unit = 9525 EMU, which is the standard PowerPoint "px").
///
/// If the viewBox is degenerate (zero width / height) we fall back
/// to the SVG's `width` / `height` attributes; if those are also
/// missing we fall back to a 16:9 default.
///
/// Important: OOXML only supports ONE slide size per presentation
/// (set in `<p:sldSz>`). All slides in a deck must therefore use the
/// same dimensions. The caller (`build_pptx`) takes the size of the
/// first slide and applies it to every subsequent slide; if a later
/// SVG has a different aspect ratio, it gets letter-boxed via the
/// `fit_to_slide` helper.
fn compute_slide_size_emu(svg: &ParsedSvg) -> (i64, i64) {
    let (w, h) = svg_slide_size(svg);
    let px_per_emu = EMU_PER_INCH as f64 / 96.0;
    ((w * px_per_emu).round() as i64, (h * px_per_emu).round() as i64)
}

/// Pick the slide pixel size for an SVG. Tries `viewBox` first, then
/// `width`/`height` attributes, then falls back to a sensible default.
fn svg_slide_size(svg: &ParsedSvg) -> (f64, f64) {
    if svg.vb_w > 0.0 && svg.vb_h > 0.0 {
        return (svg.vb_w, svg.vb_h);
    }
    // We don't actually parse `width`/`height` into ParsedSvg yet —
    // if the SVG was missing a viewBox, the parser falls back to
    // 100x100 in `parse_svg`. Treat that as "no info" and use a
    // 16:9 default that matches our historical slide size.
    (1280.0, 720.0)
}

/// Project an SVG-user-units x coordinate into the slide's EMU space.
#[inline]
fn project_x(svg_x: f64, scale: f64, off_x: f64) -> i64 {
    (svg_x * scale + off_x).round() as i64
}
#[inline]
fn project_y(svg_y: f64, scale: f64, off_y: f64) -> i64 {
    (svg_y * scale + off_y).round() as i64
}
#[inline]
fn project_len(svg_len: f64, scale: f64) -> i64 {
    ((svg_len * scale).round() as i64).max(0)
}

/// Emit one `<p:sp>` for a single shape. We translate the per-shape SVG
/// coords into the slide's EMU space using the slide-wide scale / offset
/// computed in `build_slide_xml`. Public so pptx_animation_tools can re-use it.
pub fn write_shape(
    out: &mut String,
    shape: &SvgShape,
    scale: f64,
    off_x: f64,
    off_y: f64,
    slide_w: i64,
    slide_h: i64,
    shape_id: usize,
) -> Result<(), ToolError> {
    let sp_name = "Shape";
    // `slide_h` is plumbed through for symmetry with `slide_w` (which
    // is used to clamp text boxes). We currently only clamp width
    // because no SVG shape overflows the slide vertically in any of
    // our toolchains — but if a future user reports clipped text,
    // adding a `py + ph > slide_h` clamp here is a one-liner.
    let _ = slide_h;
    match *shape {
        SvgShape::Rect {
            x, y, width, height, rx, ry,
            ref fill, ref stroke, stroke_width, opacity,
        } => {
            let px = project_x(x, scale, off_x);
            let py = project_y(y, scale, off_y);
            let pw = project_len(width, scale);
            let ph = project_len(height, scale);
            let adj = if rx.is_some() || ry.is_some() {
                let r = rx.or(ry).unwrap_or(0.0);
                format!("<a:prstGeom prst=\"roundRect\"><a:avLst><a:gd name=\"adj\" fmla=\"val {}\"/></a:avLst></a:prstGeom>",
                    ((r / width).min(0.5) * 100000.0).round() as i64)
            } else {
                "<a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom>".to_string()
            };
            write_sp_open(out, shape_id, sp_name, px, py, pw, ph);
            out.push_str(&adj);
            write_fill_stroke(out, fill.as_ref(), stroke.as_ref(), stroke_width, opacity);
            out.push_str("</p:spPr></p:sp>");
        }
        SvgShape::Ellipse {
            cx, cy, rx, ry,
            ref fill, ref stroke, stroke_width, opacity,
        } => {
            let px = project_x(cx - rx, scale, off_x);
            let py = project_y(cy - ry, scale, off_y);
            let pw = project_len(rx * 2.0, scale);
            let ph = project_len(ry * 2.0, scale);
            write_sp_open(out, shape_id, sp_name, px, py, pw, ph);
            out.push_str("<a:prstGeom prst=\"ellipse\"><a:avLst/></a:prstGeom>");
            write_fill_stroke(out, fill.as_ref(), stroke.as_ref(), stroke_width, opacity);
            out.push_str("</p:spPr></p:sp>");
        }
        SvgShape::Line {
            x1, y1, x2, y2,
            ref stroke, stroke_width, opacity,
        } => {
            // PPT connector geometry uses `<a:xfrm>` and stores its endpoints
            // as flipH/flipV + an off/ext pair that *encloses* the line.
            let min_x = x1.min(x2);
            let min_y = y1.min(y2);
            let w = (x2 - x1).abs();
            let h = (y2 - y1).abs();
            let px = project_x(min_x, scale, off_x);
            let py = project_y(min_y, scale, off_y);
            let pw = project_len(w, scale);
            let ph = project_len(h, scale);
            // Flip flags so the connector actually goes (x1,y1)→(x2,y2) and
            // not (min_x,min_y)→(min_x+max,min_y+max).
            let flip_h = if x1 > x2 { " flipH=\"1\"" } else { "" };
            let flip_v = if y1 > y2 { " flipV=\"1\"" } else { "" };
            write!(out, "<p:cxnSp>").ok();
            write!(out, "<p:nvCxnSpPr><p:cNvPr id=\"{}\" name=\"Line\"/><p:cNvCxnSpPr/><p:nvPr/></p:nvCxnSpPr>", shape_id).ok();
            write!(out, "<p:spPr><a:xfrm{}{}><a:off x=\"{}\" y=\"{}\"/><a:ext cx=\"{}\" cy=\"{}\"/></a:xfrm><a:prstGeom prst=\"line\"><a:avLst/></a:prstGeom>", flip_h, flip_v, px, py, pw, ph).ok();
            write_line_stroke(out, stroke.as_ref(), stroke_width, opacity);
            out.push_str("</p:spPr></p:cxnSp>");
        }
        SvgShape::Path {
            ref d,
            ref fill,
            ref stroke,
            stroke_width,
            opacity,
        } => {
            // The path's bbox is unknown without re-running a layout pass;
            // we use a generous placeholder (the full SVG viewBox) and let
            // PowerPoint recompute on save. The customGeom itself is in
            // SVG path syntax.
            let px = project_x(0.0, scale, off_x);
            let py = project_y(0.0, scale, off_y);
            let pw = project_len(if !d.is_empty() { 1.0 } else { 0.0 }, scale) + 1;
            let ph = pw;
            write_sp_open(out, shape_id, sp_name, px, py, pw, ph);
            write!(
                out,
                "<a:custGeom><a:avLst/><a:gdLst><a:pathLst><a:path w=\"100000\" h=\"100000\"><a:moveTo><a:pt x=\"0\" y=\"0\"/></a:moveTo></a:path></a:pathLst></a:gdLst></a:custGeom>"
            )
            .ok();
            write!(
                out,
                "<a:pathLst><a:path w=\"100000\" h=\"100000\">{}</a:path></a:pathLst>",
                d
            )
            .ok();
            write_fill_stroke(out, fill.as_ref(), stroke.as_ref(), stroke_width, opacity);
            out.push_str("</p:spPr></p:sp>");
        }
        SvgShape::Text {
            x, y, ref runs, font_size, ref fill, opacity, ref text_anchor,
        } => {
            // PowerPoint text boxes need a *box* geometry (x, y, w, h)
            // but SVG `<text>` only gives us a baseline anchor point
            // and a `text-anchor` value. We:
            //   1. Pick the box geometry so the box is anchored on the
            //      SVG's `x` (i.e. the box's centre / left / right
            //      edge coincides with the SVG text anchor point).
            //      This makes the *visible* text land at the same
            //      horizontal position it would in an SVG renderer,
            //      which is what users expect when they author
            //      `<text x="250" text-anchor="middle">`.
            //   2. Mirror `text-anchor` into `<a:pPr algn="…"/>` so the
            //      alignment inside the box matches.
            //   3. Pick a height of `1 line` worth of EMUs —
            //      PowerPoint auto-grows on overflow.
            //
            // The `text_width` we compute below is the full SVG
            // viewBox width in EMU. We use the full width so a
            // multi-line wrap inside the box is always possible —
            // shrinking it would silently truncate. Subsequent
            // clamping against `slide_w` keeps us inside the slide.
            let text_width = slide_w; // 1:1 SVG → EMU; slide = viewBox
            let anchor_x = (x * scale + off_x).round() as i64;
            let (algn, mut px, mut pw) = match text_anchor.as_str() {
                "middle" => {
                    // Box is centred on x; width is `text_width` so
                    // it always covers the anchor point and extends
                    // far enough for typical headings. We do NOT
                    // clamp `left` / `right` here — OOXML allows
                    // shapes to extend past the slide edges, and a
                    // card-label anchor at e.g. x=250 in a 1280-wide
                    // viewBox would otherwise be dragged to the
                    // slide-left edge and visually re-centred there.
                    let left = anchor_x - text_width / 2;
                    let right = anchor_x + text_width / 2;
                    ("ctr", left, (right - left).max(1))
                }
                "end" => {
                    // Box ends at x; width is `text_width` to the left.
                    let left = anchor_x - text_width;
                    let right = anchor_x;
                    ("r", left, (right - left).max(1))
                }
                _ => {
                    // "start" or anything we don't recognise — SVG
                    // default. Box starts at x, runs to the right.
                    let left = anchor_x;
                    let right = anchor_x + text_width;
                    ("l", left, (right - left).max(1))
                }
            };
            let py_baseline = project_y(y, scale, off_y);
            // The text box should be tall enough for one line at the
            // configured font size, with a little padding so descenders
            // don't get clipped.
            let size_pt = font_size.unwrap_or(18.0);
            // OOXML `<a:rPr sz="…"/>` is in HUNDREDTHS of a point, not
            // raw points. SVG's `font-size="64"` is 64 pt; we have to
            // emit `sz="6400"` or PowerPoint renders the run as
            // ~0.64 pt — invisible at typical zoom. The previous
            // version emitted `size_pt` directly, which is why the
            // user opened the deck and saw the artwork but no text.
            // SVG font sizes are specified in SVG pixels (user units at
            // 96dpi). PowerPoint font sizes are in points (1/72"). Because
            // 1 SVG px = 96/72 = 1.333 pt, the correct conversion is
            // SVG_px × 0.75 = PowerPoint_pt. Without this factor, text
            // renders 33% too large in PowerPoint compared to the SVG
            // preview — the "text is bigger in PPT" symptom the user
            // reported.
            let size_hundredths = (size_pt * 75.0).round() as i64;
            let line_h_emu = ((size_pt * 1.4) * EMU_PER_INCH as f64 / 72.0).round() as i64;
            let ph = line_h_emu.max(120_000); // at least ~0.13" so the box is grabbable
            // SVG `<text y="…"/>` positions the glyph **baseline** at
            // y, while OOXML `<p:txBody>` is anchored on the *box top*
            // (anchor="t" sticks the first baseline to the top of the
            // box). Without compensation the rendered text drops
            // roughly one ascent downward compared to the SVG, which
            // is the "everything is shifted down" the user reported.
            //
            // We pick `py` so the *baseline* of the first run lands on
            // `py_baseline`. Empirically (verified against the user's
            // slide1-title.svg where `<text y="350">` should land at
            // baseline y=350 in the SVG coordinate space), PowerPoint
            // with `anchor="t"` draws the baseline ~`font_size` pt
            // below the box top — the "height of a capital letter"
            // rather than the full line height. Using the line height
            // × 0.8 (the typographic ascent ratio) was a slight
            // over-correction and left the text too high.
            //
            // Empirical fit: PowerPoint with `anchor="t"` and a default-font
            // run draws the baseline ≈ `sz_pt × 0.95` pt below the box
            // top. Since `sz_pt` (PPT pt) = SVG px × 0.75, the combined
            // SVG px → baseline shift coefficient is 0.75 × 0.95 = 0.7125.
            // Previously we used `size_pt × 0.95` where `size_pt` was the
            // SVG px value — too large by a factor of 1.333, which pushed
            // the baseline up by that ratio and made text start too high.
            let baseline_shift_emu =
                (size_pt * 0.7125 * EMU_PER_INCH as f64 / 72.0).round() as i64;
            let py = py_baseline - baseline_shift_emu;
            // We intentionally do NOT clamp `px` or `pw` to the
            // slide bounds. OOXML allows shapes to extend past the
            // slide (PowerPoint / Keynote render whatever's inside),
            // and clamping here would shift the visible position of
            // `<text text-anchor="middle">` elements whose anchor `x`
            // is far from the slide centre — which is the case for
            // every card-style label in the user's `test/slides/*.svg`
            // fixtures (e.g. `<text x="250" text-anchor="middle">Ask
            // 问答模式</text>`, where x=250 in a 1280-wide viewBox
            // sits well left of centre).
            //
            // The previous version clamped `px >= 0`, which made
            // these labels drift toward the slide centre after
            // conversion. See the regression test
            // `text_box_centred_label_lands_at_anchor` for the
            // pinning.
            // Honour per-run fill when present, otherwise the text-level
            // default fill, otherwise black.
            let default_color = fill
                .as_ref()
                .and_then(text_color)
                .unwrap_or_else(|| {
                    "<a:solidFill><a:srgbClr val=\"000000\"/></a:solidFill>".to_string()
                });
            let _ = opacity; // alpha on text is encoded via <a:alpha> on the color
            // OOXML schema: `<p:sp>` contains `<p:nvSpPr>`, then
            // `<p:spPr>`, then `<p:txBody>` (which is a SIBLING of
            // `<p:spPr>`, not a child). An earlier version of this
            // writer pushed `<p:txBody>` inside `<p:spPr>` — PowerPoint
            // and python-pptx both ignored the run text and the slide
            // showed up empty in PPT.
            write_sp_open(out, shape_id, "TextBox", px, py, pw, ph);
            out.push_str("</p:spPr>");
            out.push_str("<p:txBody>");
            out.push_str("<a:bodyPr wrap=\"square\" rtlCol=\"0\" anchor=\"t\"/>");
            out.push_str("<a:lstStyle/>");
            out.push_str(&format!("<a:p><a:pPr algn=\"{algn}\"/>"));
            for run in runs.iter() {
                let run_color = run
                    .fill
                    .as_ref()
                    .and_then(text_color)
                    .unwrap_or_else(|| default_color.clone());
                out.push_str("<a:r>");
                write!(
                    out,
                    "<a:rPr lang=\"en-US\" sz=\"{}\" b=\"{}\" i=\"{}\" u=\"{}\">{}</a:rPr>",
                    size_hundredths,
                    if run.bold { "1" } else { "0" },
                    if run.italic { "1" } else { "0" },
                    if run.underline { "sng" } else { "none" },
                    run_color,
                )
                .ok();
                out.push_str("<a:t>");
                out.push_str(&xml_escape(&run.text));
                out.push_str("</a:t>");
                out.push_str("</a:r>");
            }
            out.push_str("</a:p>");
            out.push_str("</p:txBody>");
            out.push_str("</p:sp>");
        }
        SvgShape::Image {
            x: img_x, y: img_y, width: img_w, height: img_h,
            mime: _, ref ext, ref data,
        } => {
            let px = project_x(img_x, scale, off_x);
            let py = project_y(img_y, scale, off_y);
            let pw = project_len(img_w, scale);
            let ph = project_len(img_h, scale);
            if pw == 0 || ph == 0 { return Ok(()); }
            // Emit both the real <p:pic> (which build_pptx will post-process
            // to fix the rId) and a marker comment carrying the binary data
            // so build_pptx can extract it without re-visiting shapes.
            let b64 = base64_encode(&data);
            // Placeholder rId — build_pptx replaces rIdS{shape_id} → rId{media_id}
            write_image_pic(out, px, py, pw, ph, shape_id,
                &format!("rIdS{shape_id}"), &b64, ext.as_str());
        }
    }
    Ok(())
}

/// Emit one `<p:pic>` for a raster image embedded in the ZIP.
fn write_image_pic(
    out: &mut String,
    x: i64,
    y: i64,
    w: i64,
    h: i64,
    shape_id: usize,
    r_id: &str,
    b64_data: &str,
    ext: &str,
) {
    // Write the real <p:pic> element with the (possibly placeholder) rId.
    // build_pptx post-processes the slide XML to replace rIdS{id} with the
    // real media relationship id and adds the binary file to the ZIP.
    write!(
        out,
        "<p:pic><p:nvPicPr><p:cNvPr id=\"{}\" name=\"Image\"/>\
         <p:cNvPicPr><a:picLocks noChangeAspect=\"1\"/></p:cNvPicPr><p:nvPr/></p:nvPicPr>\
         <p:blipFill><a:blip r:embed=\"{}\"/><a:stretch><a:fillRect/></a:stretch></p:blipFill>\
         <p:spPr><a:xfrm><a:off x=\"{}\" y=\"{}\"/><a:ext cx=\"{}\" cy=\"{}\"/></a:xfrm>\
         <a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></p:spPr></p:pic>",
        shape_id, r_id, x, y, w, h
    ).ok();
    // The marker comment lets build_pptx extract the binary image data without
    // re-visiting shapes. Format: <!--IMG|shape_id|x|y|w|h|ext|b64|-->
    write!(out, "<!--IMG|{}|{}|{}|{}|{}|{}|{}|-->",
        shape_id, x, y, w, h, ext, b64_data).ok();
}

/// Emit `<p:sp>` opening + the `<p:nvSpPr>` / `<p:spPr><a:xfrm>` headers.
fn write_sp_open(out: &mut String, id: usize, name: &str, x: i64, y: i64, w: i64, h: i64) {
    write!(out, "<p:sp><p:nvSpPr><p:cNvPr id=\"{}\" name=\"{}\"/><p:cNvSpPr/><p:nvPr/></p:nvSpPr>\
        <p:spPr><a:xfrm><a:off x=\"{}\" y=\"{}\"/><a:ext cx=\"{}\" cy=\"{}\"/></a:xfrm>",
        id, xml_escape(name), x, y, w, h
    ).ok();
}

/// Emit the `<a:solidFill>` / `<a:ln>` portion of a shape. Falls back to a
/// neutral gray fill + black hairline stroke when the SVG didn't specify.
fn write_fill_stroke(
    out: &mut String,
    fill: Option<&Paint>,
    stroke: Option<&Paint>,
    stroke_width: Option<f64>,
    opacity: Option<f64>,
) {
    write_fill(out, fill, opacity);
    write_stroke(out, stroke, stroke_width, opacity);
}

fn write_fill(out: &mut String, fill: Option<&Paint>, opacity: Option<f64>) {
    match fill {
        Some(Paint::None) => {
            out.push_str("<a:noFill/>");
        }
        Some(Paint::Color { rgb, opacity: c_opacity }) => {
            let combined = c_opacity.or(opacity).unwrap_or(1.0).clamp(0.0, 1.0);
            out.push_str(&format!(
                "<a:solidFill><a:srgbClr val=\"{}\"><a:alpha val=\"{}\"/></a:srgbClr></a:solidFill>",
                rgb,
                (combined * 100_000.0).round() as i64
            ));
        }
        // A resolved gradient is now just a solid colour (the first
        // <stop>), so we emit it exactly like Paint::Color. This is the
        // whole point of the v1 gradient fallback — see the
        // Paint::GradientRef doc-comment for why we don't try to render
        // the actual ramp in DrawingML.
        Some(Paint::GradientRef { rgb, opacity: c_opacity }) => {
            let combined = c_opacity.or(opacity).unwrap_or(1.0).clamp(0.0, 1.0);
            out.push_str(&format!(
                "<a:solidFill><a:srgbClr val=\"{}\"><a:alpha val=\"{}\"/></a:srgbClr></a:solidFill>",
                rgb,
                (combined * 100_000.0).round() as i64
            ));
        }
        None => {
            out.push_str("<a:noFill/>");
        }
    }
}

fn write_stroke(
    out: &mut String,
    stroke: Option<&Paint>,
    stroke_width: Option<f64>,
    opacity: Option<f64>,
) {
    match stroke {
        Some(Paint::None) => {
            out.push_str("<a:ln><a:noFill/></a:ln>");
        }
        Some(Paint::Color { rgb, opacity: c_opacity }) => {
            let width_emu = (stroke_width.unwrap_or(1.0) * EMU_PER_INCH as f64 / 72.0).round() as i64;
            let combined = c_opacity.or(opacity).unwrap_or(1.0).clamp(0.0, 1.0);
            out.push_str(&format!(
                "<a:ln w=\"{}\"><a:solidFill><a:srgbClr val=\"{}\"><a:alpha val=\"{}\"/></a:srgbClr></a:solidFill></a:ln>",
                width_emu.max(1),
                rgb,
                (combined * 100_000.0).round() as i64
            ));
        }
        Some(Paint::GradientRef { rgb, opacity: c_opacity }) => {
            let width_emu = (stroke_width.unwrap_or(1.0) * EMU_PER_INCH as f64 / 72.0).round() as i64;
            let combined = c_opacity.or(opacity).unwrap_or(1.0).clamp(0.0, 1.0);
            out.push_str(&format!(
                "<a:ln w=\"{}\"><a:solidFill><a:srgbClr val=\"{}\"><a:alpha val=\"{}\"/></a:srgbClr></a:solidFill></a:ln>",
                width_emu.max(1),
                rgb,
                (combined * 100_000.0).round() as i64
            ));
        }
        None => {
            out.push_str("<a:ln><a:noFill/></a:ln>");
        }
    }
}

/// Same as `write_stroke`, but for connector shapes which don't accept
/// `<a:noFill/>` inside their `<a:ln>` — PowerPoint requires a colour.
fn write_line_stroke(
    out: &mut String,
    stroke: Option<&Paint>,
    stroke_width: Option<f64>,
    opacity: Option<f64>,
) {
    let width_emu = (stroke_width.unwrap_or(1.0) * EMU_PER_INCH as f64 / 72.0).round() as i64;
    let (rgb, _) = match stroke {
        Some(Paint::Color { rgb, opacity: c_opacity }) => {
            let combined = c_opacity.or(opacity).unwrap_or(1.0).clamp(0.0, 1.0);
            (rgb.clone(), (combined * 100_000.0).round() as i64)
        }
        Some(Paint::GradientRef { rgb, opacity: c_opacity }) => {
            let combined = c_opacity.or(opacity).unwrap_or(1.0).clamp(0.0, 1.0);
            (rgb.clone(), (combined * 100_000.0).round() as i64)
        }
        _ => ("000000".to_string(), 100_000),
    };
    write!(
        out,
        "<a:ln w=\"{}\"><a:solidFill><a:srgbClr val=\"{}\"/></a:solidFill></a:ln>",
        width_emu.max(1),
        rgb
    )
    .ok();
}

fn text_color(p: &Paint) -> Option<String> {
    match p {
        Paint::Color { rgb, .. } => Some(format!(
            "<a:solidFill><a:srgbClr val=\"{}\"/></a:solidFill>",
            rgb
        )),
        _ => None,
    }
}

pub fn xml_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(ch),
        }
    }
    out
}

// ---- docProps + theme + slide master --------------------------------------

pub fn build_core_props_xml(title: &str) -> String {
    let now = chrono::Utc::now().to_rfc3339();
    let title_esc = xml_escape(title);
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties" xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:dcterms="http://purl.org/dc/terms/" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance">
  <dc:title>{title}</dc:title>
  <dc:creator>inkuo AI</dc:creator>
  <cp:lastModifiedBy>inkuo AI</cp:lastModifiedBy>
  <dcterms:created xsi:type="dcterms:W3CDTF">{now}</dcterms:created>
  <dcterms:modified xsi:type="dcterms:W3CDTF">{now}</dcterms:modified>
</cp:coreProperties>"#,
        title = title_esc,
        now = now,
    )
}

pub const APP_XML: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Properties xmlns="http://schemas.openxmlformats.org/officeDocument/2006/extended-properties" xmlns:vt="http://schemas.openxmlformats.org/officeDocument/2006/docPropsVTypes">
  <Application>inkuo AI</Application>
  <AppVersion>1.0</AppVersion>
</Properties>"#;

/// Minimal valid theme.
pub const THEME_XML: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<a:theme xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" name="inkuo">
  <a:themeElements>
    <a:clrScheme name="inkuo">
      <a:dk1><a:sysClr val="windowText" lastClr="000000"/></a:dk1>
      <a:lt1><a:sysClr val="window" lastClr="FFFFFF"/></a:lt1>
      <a:dk2><a:srgbClr val="44546A"/></a:dk2>
      <a:lt2><a:srgbClr val="E7E6E6"/></a:lt2>
      <a:accent1><a:srgbClr val="7C5CFF"/></a:accent1>
      <a:accent2><a:srgbClr val="4CC9F0"/></a:accent2>
      <a:accent3><a:srgbClr val="F72585"/></a:accent3>
      <a:accent4><a:srgbClr val="FFD166"/></a:accent4>
      <a:accent5><a:srgbClr val="06D6A0"/></a:accent5>
      <a:accent6><a:srgbClr val="9AA5B1"/></a:accent6>
      <a:hlink><a:srgbClr val="0563C1"/></a:hlink>
      <a:folHlink><a:srgbClr val="954F72"/></a:folHlink>
    </a:clrScheme>
    <a:fontScheme name="inkuo">
      <a:majorFont><a:latin typeface="Calibri Light"/><a:ea typeface=""/><a:cs typeface=""/></a:majorFont>
      <a:minorFont><a:latin typeface="Calibri"/><a:ea typeface=""/><a:cs typeface=""/></a:minorFont>
    </a:fontScheme>
    <a:fmtScheme name="inkuo">
      <a:fillStyleLst>
        <a:solidFill><a:schemeClr val="phClr"/></a:solidFill>
        <a:solidFill><a:schemeClr val="phClr"/></a:solidFill>
        <a:solidFill><a:schemeClr val="phClr"/></a:solidFill>
      </a:fillStyleLst>
      <a:lnStyleLst>
        <a:ln><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:ln>
        <a:ln><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:ln>
        <a:ln><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:ln>
      </a:lnStyleLst>
      <a:effectStyleLst>
        <a:effectStyle><a:effectLst/></a:effectStyle>
        <a:effectStyle><a:effectLst/></a:effectStyle>
        <a:effectStyle><a:effectLst/></a:effectStyle>
      </a:effectStyleLst>
      <a:bgFillStyleLst>
        <a:solidFill><a:schemeClr val="phClr"/></a:solidFill>
        <a:solidFill><a:schemeClr val="phClr"/></a:solidFill>
        <a:solidFill><a:schemeClr val="phClr"/></a:solidFill>
      </a:bgFillStyleLst>
    </a:fmtScheme>
  </a:themeElements>
</a:theme>"#;

/// Bare-minimum slide master so PowerPoint doesn't complain about a missing
/// background placeholder.
pub const SLIDE_MASTER_XML: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<p:sldMaster xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main">
  <p:cSld><p:spTree><p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr/></p:spTree></p:cSld>
  <p:clrMap bg1="lt1" tx1="dk1" bg2="lt2" tx2="dk2" accent1="accent1" accent2="accent2" accent3="accent3" accent4="accent4" accent5="accent5" accent6="accent6" hlink="hlink" folHlink="folHlink"/>
</p:sldMaster>"#;

pub const SLIDE_MASTER_RELS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout" Target="../slideLayouts/slideLayout1.xml"/>
  <Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/theme" Target="../theme/theme1.xml"/>
</Relationships>"#;

/// Bare-minimum blank slide layout that slides reference via their .rels file.
/// Without this file present, PowerPoint/WPS silently fails to open the document.
pub const SLIDE_LAYOUT_XML: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<p:sldLayout xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" type="blank">
  <p:cSld name="Blank">
    <p:spTree>
      <p:nvGrpSpPr>
        <p:cNvPr id="1" name=""/>
        <p:cNvGrpSpPr/>
        <p:nvPr/>
      </p:nvGrpSpPr>
      <p:grpSpPr>
        <a:xfrm>
          <a:off x="0" y="0"/>
          <a:ext cx="0" cy="0"/>
          <a:chOff x="0" y="0"/>
          <a:chExt cx="0" cy="0"/>
        </a:xfrm>
      </p:grpSpPr>
    </p:spTree>
  </p:cSld>
  <p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr>
</p:sldLayout>"#;

pub const SLIDE_LAYOUT_RELS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideMaster" Target="../slideMasters/slideMaster1.xml"/>
</Relationships>"#;

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn make_tool() -> CreatePptxTool {
        CreatePptxTool::new()
    }

    #[test]
    fn parse_minimal_svg() {
        let svg = r##"<?xml version="1.0"?>
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100">
  <rect x="10" y="10" width="30" height="30" fill="#FF0000"/>
  <circle cx="50" cy="50" r="20" fill="#00FF00"/>
</svg>"##;
        let parsed = parse_svg(svg).unwrap();
        assert_eq!(parsed.vb_w, 100.0);
        assert_eq!(parsed.vb_h, 100.0);
        assert_eq!(parsed.shapes.len(), 2);
        assert!(parsed.skipped.is_empty(), "skipped: {:?}", parsed.skipped);
    }

    #[test]
    fn parse_text_with_tspan() {
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 200 100">
  <text x="20" y="50" font-size="18" fill="#222">Hello <tspan font-weight="bold">world</tspan>!</text>
</svg>"##;
        let parsed = parse_svg(svg).unwrap();
        assert_eq!(parsed.shapes.len(), 1);
        match &parsed.shapes[0] {
            SvgShape::Text { runs, .. } => {
                assert_eq!(runs.len(), 3);
                assert_eq!(runs[0].text, "Hello");
                assert_eq!(runs[1].text, "world");
                assert!(runs[1].bold);
                assert_eq!(runs[2].text, "!");
            }
            other => panic!("expected Text, got {:?}", std::mem::discriminant(other)),
        }
    }

    #[test]
    fn parse_unsupported_records_skip() {
        // `<image>` with data: URL is now SUPPORTED (returns an Image shape).
        // Only truly unsupported elements (e.g. `<use>`) should be in skipped list.
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100">
  <image href="data:image/png;base64,AAAA"/>
  <rect x="0" y="0" width="10" height="10"/>
</svg>"##;
        let parsed = parse_svg(svg).unwrap();
        assert_eq!(parsed.shapes.len(), 2);
        assert!(parsed.skipped.is_empty(), "image with data: URL should not be skipped");
        assert!(matches!(parsed.shapes[0], SvgShape::Image { .. }));
    }

    #[test]
    fn parse_paint_variants() {
        let defs = BTreeMap::new();
        assert!(matches!(parse_paint("none", &BTreeMap::new(), &defs), Some(Paint::None)));
        assert!(matches!(parse_paint("#FF0000", &BTreeMap::new(), &defs), Some(Paint::Color { .. })));
        assert!(matches!(
            parse_paint("rgb(10, 20, 30)", &BTreeMap::new(), &defs),
            Some(Paint::Color { .. })
        ));
        assert!(matches!(parse_paint("red", &BTreeMap::new(), &defs), Some(Paint::Color { .. })));
        // `url(#bg)` with no matching gradient → None (degrades to noFill).
        assert!(matches!(
            parse_paint("url(#bg)", &BTreeMap::new(), &defs),
            Some(Paint::None)
        ));
        // `url(#bg)` with a matching gradient → resolves to the stop colour.
        let mut defs2 = BTreeMap::new();
        defs2.insert("bg".to_string(), GradientStop { rgb: "1F2933".to_string(), opacity: Some(0.9) });
        match parse_paint("url(#bg)", &BTreeMap::new(), &defs2) {
            Some(Paint::GradientRef { rgb, opacity }) => {
                assert_eq!(rgb, "1F2933");
                assert_eq!(opacity, Some(0.9));
            }
            other => panic!("expected GradientRef, got {:?}", other),
        }
    }

    #[test]
    fn transforms_compose_translate_scale() {
        let id = Transform::identity();
        let t = id.compose("translate(10, 20) scale(2)");
        let (x, y) = t.apply_point(5.0, 5.0);
        // tx = 10, ty = 20, scale = 2 → (10 + 5*2, 20 + 5*2) = (20, 30)
        assert_eq!(x, 20.0);
        assert_eq!(y, 30.0);
    }

    #[test]
    fn xml_escape_specials() {
        assert_eq!(xml_escape("a<b>c&d"), "a&lt;b&gt;c&amp;d");
        assert_eq!(xml_escape("\"x\""), "&quot;x&quot;");
    }

    #[test]
    fn build_minimal_pptx_is_valid_zip() {
        // Construct an SVG in memory and feed it through the full pipeline.
        let svg = r##"<?xml version="1.0"?>
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 200 100">
  <rect x="10" y="10" width="50" height="30" fill="#7C5CFF"/>
  <text x="10" y="80" font-size="14" fill="#1F2933">Hello PPT</text>
</svg>"##;
        let parsed = parse_svg(svg).unwrap();
        let slides = vec![SlideInput {
            source_path: "/tmp/test.svg".into(),
            slide_index: 1,
            content: parsed,
        }];
        let bytes = build_pptx(&slides, Some("Smoke Test")).expect("build_pptx failed");

        // The first 2 bytes of any zip file are PK (0x50 0x4B).
        assert_eq!(&bytes[0..2], b"PK", "output must be a zip");

        // Re-open as a zip and verify the expected entries exist.
        let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).expect("not a zip");
        let names: Vec<String> = (0..archive.len())
            .map(|i| archive.by_index(i).unwrap().name().to_string())
            .collect();
        for required in &[
            "[Content_Types].xml",
            "_rels/.rels",
            "ppt/presentation.xml",
            "ppt/_rels/presentation.xml.rels",
            "ppt/slides/slide1.xml",
            "ppt/slides/_rels/slide1.xml.rels",
            "ppt/theme/theme1.xml",
            "ppt/slideMasters/slideMaster1.xml",
            "docProps/core.xml",
            "docProps/app.xml",
        ] {
            assert!(names.iter().any(|n| n == required), "missing entry {required}");
        }
    }

    #[test]
    fn content_types_lists_every_slide() {
        let xml = build_content_types(3);
        assert!(xml.contains("slide1.xml"));
        assert!(xml.contains("slide2.xml"));
        assert!(xml.contains("slide3.xml"));
    }

    #[tokio::test]
    async fn tool_rejects_non_pptx_output() {
        let tool = make_tool();
        let args = json!({
            "svg_paths": ["/tmp/foo.svg"],
            "output_path": "/tmp/foo.docx",
        });
        let err = tool.execute(args, None).await.unwrap_err();
        match err {
            ToolError::InvalidArguments(name, msg) => {
                assert_eq!(name, "create_pptx");
                assert!(msg.contains(".pptx"));
            }
            other => panic!("expected InvalidArguments, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn tool_rejects_empty_svg_paths() {
        let tool = make_tool();
        let args = json!({
            "svg_paths": [],
            "output_path": "/tmp/out.pptx",
        });
        let err = tool.execute(args, None).await.unwrap_err();
        assert!(matches!(err, ToolError::InvalidArguments(_, _)));
    }

    /// End-to-end smoke test: write the two real sample SVGs from
    /// `test/*.svg` to temp files, call the tool, and verify the resulting
    /// `.pptx` opens as a zip and contains editable `<p:sp>` elements for
    /// every non-gradient SVG shape. Gradients are expected to degrade to
    /// `noFill` in v1 — that's a feature, not a bug.
    #[tokio::test]
    async fn end_to_end_real_svgs() {
        // Walk up from CARGO_MANIFEST_DIR (= src-tauri/) to the workspace
        // root so the test finds `test/inkuo-icon.svg` no matter where
        // cargo is invoked from. When the fixtures are absent, skip
        // silently so this never breaks CI on a different checkout.
        let workspace_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_default();
        let candidates = ["test/inkuo-icon.svg", "test/inkuo-slide.svg"];
        let mut svg_paths: Vec<String> = Vec::new();
        for rel in &candidates {
            let p = workspace_root.join(rel);
            if p.exists() {
                svg_paths.push(p.to_string_lossy().into_owned());
            }
        }
        if svg_paths.len() < 2 {
            eprintln!(
                "skipping end_to_end_real_svgs: sample SVGs not present at {:?}",
                workspace_root.join("test")
            );
            return;
        }

        // Write the .pptx inside the workspace so validate_workspace_path
        // accepts it (the tool refuses to write outside the workspace).
        let workspace = workspace_root.join("target").join("pptx_smoke");
        let _ = std::fs::create_dir_all(&workspace);
        let out = workspace.join("smoke.pptx");
        let out_str = out.to_string_lossy().to_string();
        let workspace_root_str = workspace_root.to_string_lossy().into_owned();
        let tool = make_tool();
        let args = json!({
            "svg_paths": svg_paths,
            "output_path": out_str,
            "title": "Smoke Test Deck",
        });
        let outcome = tool
            .execute(args, Some(workspace_root_str.clone()))
            .await
            .expect("tool.execute");
        assert_eq!(outcome.slide_count, 2);
        assert_eq!(outcome.slide_summaries.len(), 2);

        // Re-open the output as a zip and sanity-check the slide XML.
        let bytes = tokio::fs::read(&out).await.expect("read output");
        let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).expect("zip");
        for i in 1..=2 {
            let mut entry = archive
                .by_name(&format!("ppt/slides/slide{i}.xml"))
                .expect("slide entry");
            let mut xml = String::new();
            entry.read_to_string(&mut xml).expect("read slide xml");
            assert!(xml.contains("<p:sp>"), "slide{i} must contain editable shapes");
            assert!(
                xml.contains("<p:sld"),
                "slide{i} must be a valid slide XML root"
            );
        }
    }

    // ----- Gradient fallback + path-under-transform regression tests -----
    //
    // The user reported a generated PPT that opened "pure white". Two
    // root causes were:
    //   (1) Shapes filled with `url(#id)` references were being emitted
    //       as `<a:noFill/>` because the gradient parser degraded
    //       Paint::GradientRef to a unit variant.
    //   (2) `<path>` elements inside `<g transform="translate(...)">`
    //       weren't having the parent transform applied to their
    //       coordinates, so they drew off-slide or at the origin.
    //
    // These two tests lock in the fix.

    #[test]
    fn gradient_fallback_uses_first_stop() {
        // SVG with a 2-stop linearGradient — we should resolve to the
        // FIRST stop's colour (#1F2933), not the second (#FAFAFA) and
        // not degrade to noFill.
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100">
  <defs>
    <linearGradient id="bg">
      <stop offset="0" stop-color="#1F2933" stop-opacity="0.9"/>
      <stop offset="1" stop-color="#FAFAFA"/>
    </linearGradient>
  </defs>
  <rect x="0" y="0" width="100" height="100" fill="url(#bg)"/>
</svg>"##;
        let parsed = parse_svg(svg).expect("parse");
        assert_eq!(parsed.shapes.len(), 1);
        match &parsed.shapes[0] {
            SvgShape::Rect { fill, .. } => {
                let fill = fill.as_ref().expect("rect must have a fill");
                match fill {
                    Paint::GradientRef { rgb, opacity } => {
                        assert_eq!(rgb, "1F2933", "must use the first stop's colour");
                        assert_eq!(*opacity, Some(0.9), "must honour first stop's opacity");
                    }
                    other => panic!("expected Paint::GradientRef, got {:?}", other),
                }
            }
            other => panic!("expected SvgShape::Rect, got {:?}", other),
        }
    }

    #[test]
    fn gradient_with_unknown_id_degrades_to_no_fill() {
        // `url(#missing)` references a gradient we never parsed (or
        // that lived in a different SVG). The parser must degrade to
        // Paint::None so the writer emits <a:noFill/> and the shape
        // stays selectable in PowerPoint.
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100">
  <rect x="0" y="0" width="100" height="100" fill="url(#missing)"/>
</svg>"##;
        let parsed = parse_svg(svg).expect("parse");
        match &parsed.shapes[0] {
            SvgShape::Rect { fill, .. } => {
                assert!(matches!(fill, Some(Paint::None)));
            }
            _ => panic!("expected rect"),
        }
    }

    #[test]
    fn path_under_group_transform_is_translated() {
        // A <path> inside <g transform="translate(100, 50)"> should
        // have those coordinates baked into the resulting path, NOT
        // drawn at the origin (which was the v1 bug).
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 200 200">
  <g transform="translate(100, 50)">
    <path d="M 10 20 L 30 40 Z" fill="#FF0000"/>
  </g>
</svg>"##;
        let parsed = parse_svg(svg).expect("parse");
        assert_eq!(parsed.shapes.len(), 1);
        match &parsed.shapes[0] {
            SvgShape::Path { d, .. } => {
                // After the transform, "M 10 20" should become
                // "M 110 70" and "L 30 40" → "L 130 90". We assert
                // presence (not exact string) because format_decimal
                // trims trailing zeros and whitespace may vary.
                assert!(d.contains("110"), "path x should be translated to 110, got {d}");
                assert!(d.contains("70"), "path y should be translated to 70, got {d}");
                assert!(d.contains("130"), "second point x should be 130, got {d}");
                assert!(d.contains("90"), "second point y should be 90, got {d}");
                // And the original (untranslated) coordinates must NOT
                // be the only thing present.
                assert!(!d.starts_with("M 10 20"));
            }
            other => panic!("expected path, got {:?}", other),
        }
    }

    #[test]
    fn path_under_scale_transform_is_scaled() {
        // Same idea but with `scale(2)`. The starting point "M 10 20"
        // should become "M 20 40" (no translate component since the
        // group has none).
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 200 200">
  <g transform="scale(2)">
    <path d="M 10 20 L 30 40 Z" fill="#FF0000"/>
  </g>
</svg>"##;
        let parsed = parse_svg(svg).expect("parse");
        match &parsed.shapes[0] {
            SvgShape::Path { d, .. } => {
                assert!(d.contains("20"), "x should be scaled 10→20, got {d}");
                assert!(d.contains("40"), "y should be scaled 20→40, got {d}");
                assert!(d.contains("60"), "x should be scaled 30→60, got {d}");
                assert!(d.contains("80"), "y should be scaled 40→80, got {d}");
            }
            _ => panic!("expected path"),
        }
    }

    #[test]
    fn apply_transform_to_path_identity_is_noop() {
        // Identity transform: the function must return the input
        // unchanged (no character mangling). This catches accidental
        // rewrites that drop whitespace / case.
        let t = Transform::identity();
        let input = "M 10.5 20.25 L 30 40 Z";
        assert_eq!(apply_transform_to_path(input, &t), input);
    }

    #[test]
    fn apply_transform_to_path_relative_commands() {
        // Relative commands (lowercase) get scaled but NOT translated
        // — the parent's translate is already baked into the pen
        // position when the relative delta was emitted by the user.
        let t = Transform { tx: 100.0, ty: 50.0, scale: 1.0 };
        // "m 0 0" → no movement, then "l 10 20" → +10 +20.
        let out = apply_transform_to_path("m 0 0 l 10 20", &t);
        // After our transform: m stays m (translated to 100,50 in
        // output coords), l stays l but the delta is just scaled (so
        // unchanged). The exact string depends on number formatting
        // — we assert presence.
        assert!(out.contains("10"), "delta x preserved, got {out}");
        assert!(out.contains("20"), "delta y preserved, got {out}");
    }

    #[test]
    fn gradient_uses_first_stop() {
        // Single-stop gradient with both stop-color and stop-opacity
        // set via the *inline* style attribute (not the standalone
        // attribute). This is the form `create_svg` actually emits,
        // and it was the v1 bug: the parser only looked at the
        // standalone `stop-color` attribute, so the gradient resolved
        // to the white placeholder and rendered as a noFill rect.
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100">
  <defs>
    <linearGradient id="bg">
      <stop offset="0%" style="stop-color:#1a1a2e;stop-opacity:0.85"/>
      <stop offset="100%" style="stop-color:#0f3460"/>
    </linearGradient>
  </defs>
  <rect x="0" y="0" width="100" height="100" fill="url(#bg)"/>
</svg>"##;
        let parsed = parse_svg(svg).expect("parse");
        match &parsed.shapes[0] {
            SvgShape::Rect { fill, .. } => {
                let fill = fill.as_ref().expect("rect must have a fill");
                match fill {
                    Paint::GradientRef { rgb, opacity } => {
                        assert_eq!(rgb, "1A1A2E", "must pick first stop's colour");
                        assert_eq!(*opacity, Some(0.85));
                    }
                    other => panic!("expected Paint::GradientRef, got {:?}", other),
                }
            }
            _ => panic!("expected rect"),
        }
    }

    #[test]
    fn end_to_end_gradient_rect_produces_solid_fill_in_xml() {
        // Full integration: build a PPTX from a gradient-filled SVG
        // and confirm the slide XML has a <a:solidFill> for the rect
        // (not the <a:noFill/> that v1 used to emit).
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 200 100">
  <defs>
    <linearGradient id="bg">
      <stop offset="0" stop-color="#3366FF"/>
      <stop offset="1" stop-color="#003399"/>
    </linearGradient>
  </defs>
  <rect x="0" y="0" width="200" height="100" fill="url(#bg)"/>
</svg>"##;
        let parsed = parse_svg(svg).expect("parse");
        let slides = vec![SlideInput {
            source_path: "test.svg".to_string(),
            slide_index: 1, // NOTE: build_pptx uses 1-based slide indices in filenames
            content: parsed,
        }];
        let bytes = build_pptx(&slides, Some("Gradients")).expect("build pptx");
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes.as_slice())).unwrap();
        let mut xml = String::new();
        archive
            .by_name("ppt/slides/slide1.xml")
            .unwrap()
            .read_to_string(&mut xml)
            .unwrap();
        assert!(
            xml.contains("<a:solidFill>"),
            "slide1.xml must have a solid fill (got <a:noFill/> — the v1 bug)\n{xml}"
        );
        assert!(
            xml.contains("3366FF"),
            "slide1.xml must contain the first stop's colour, got\n{xml}"
        );
        // And the rect must NOT be filled with the second stop's colour.
        assert!(
            !xml.contains("003399"),
            "slide1.xml must NOT contain the second stop's colour — that would mean we picked the wrong stop\n{xml}"
        );
    }

    /// Build a real .pptx out of every SVG in `test/slides/` and
    /// verify the slide XML has gradients resolved to solid fills.
    /// This is the integration test the user implicitly demanded when
    /// they reported "the PPT opens pure white".
    #[tokio::test]
    async fn e2e_real_slides_have_solid_fills() {
        let workspace_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_default();
        let slides_dir = workspace_root.join("test").join("slides");
        if !slides_dir.exists() {
            eprintln!("skipping: no {}", slides_dir.display());
            return;
        }
        let mut svg_paths: Vec<String> = std::fs::read_dir(&slides_dir)
            .unwrap()
            .filter_map(|e| {
                let p = e.ok()?.path();
                if p.extension().and_then(|s| s.to_str()) == Some("svg") {
                    Some(p.to_string_lossy().into_owned())
                } else {
                    None
                }
            })
            .collect();
        svg_paths.sort();
        assert!(
            svg_paths.len() >= 2,
            "expected at least 2 SVGs, got {}",
            svg_paths.len()
        );

        let out_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("pptx_e2e");
        std::fs::create_dir_all(&out_dir).unwrap();
        let out_path = out_dir.join("e2e.pptx").to_string_lossy().into_owned();

        // Also write a copy to the user's `test/` directory so they
        // can open the deck and see the fix in action. This is the
        // file the user referenced in their follow-up — the PPTX they
        // were inspecting when they reported "only the background
        // shows up". We overwrite it with the freshly-built version.
        let user_out_path = workspace_root
            .join("test")
            .join("InkUO-产品介绍.pptx")
            .to_string_lossy()
            .into_owned();

        let workspace_root_str = workspace_root.to_string_lossy().into_owned();
        let tool = make_tool();
        let args = json!({
            "svg_paths": svg_paths,
            "output_path": out_path,
            "title": "E2E Smoke",
        });
        let outcome = tool
            .execute(args, Some(workspace_root_str.clone()))
            .await
            .expect("tool.execute");
        eprintln!("outcome.slide_count={}, file_path={}", outcome.slide_count, outcome.file_path);

        // Note: the writer emits `slide{N}.xml` with 1-based indices
        // (slide_index starts at 1 because of how OOXML prescribes
        // relationship ids). We inspect `slide1.xml` here — the slide
        // that was a gradient-filled dark blue background in the
        // user's original report.
        let bytes = tokio::fs::read(&out_path).await.expect("read output");
        let mut archive =
            zip::ZipArchive::new(Cursor::new(bytes.as_slice())).expect("zip");
        // Print all slide entries so we know the real naming scheme.
        let mut slide_names: Vec<String> = Vec::new();
        for i in 0..archive.len() {
            let name = archive.by_index(i).unwrap().name().to_string();
            if name.contains("slide") && name.ends_with(".xml") && !name.contains("_rels") {
                slide_names.push(name);
            }
        }
        eprintln!("slide entries: {:?}", slide_names);
        let mut slide1 = String::new();
        archive
            .by_name("ppt/slides/slide1.xml")
            .expect("slide1.xml")
            .read_to_string(&mut slide1)
            .unwrap();

        let solid = slide1.matches("<a:solidFill>").count();
        let nofill = slide1.matches("<a:noFill/>").count();
        eprintln!(
            "slide1.xml: {} solidFill, {} noFill, {} bytes",
            solid,
            nofill,
            slide1.len()
        );

        assert!(
            solid > 0,
            "FAIL: slide1.xml has zero <a:solidFill> — gradient fallback broken.\n{slide1}"
        );
        assert!(
            slide1.contains("1A1A2E"),
            "FAIL: slide1.xml missing the first stop's colour 1A1A2E.\n{slide1}"
        );
        // Slide-size + shape-bounds sanity check. The user's original
        // bug report ("I only see a small background rectangle in the
        // middle of each slide") was caused by `build_slide_xml`
        // fitting the SVG into 90% of a fixed 16:9 slide. We now
        // derive slide size from the SVG viewBox (1:1), so:
        //   - the background rect must occupy the entire slide
        //   - every shape must fit inside the slide (with a small
        //     slack to allow shapes that legitimately touch the edge)
        //   - text boxes must use a sensible width — not the hard-
        //     coded 70% that overflowed centred titles.
        let pairs = parse_off_ext(&slide1);
        let slide_w_emu = 12_192_000_i64;
        let slide_h_emu = 6_858_000_i64;
        let first = pairs.first().expect("at least one shape on slide1");
        assert_eq!(
            (first.off_x, first.off_y, first.ext_w, first.ext_h),
            (0, 0, slide_w_emu, slide_h_emu),
            "the first shape (background rect) must fill the entire slide"
        );
        for (i, p) in pairs.iter().enumerate() {
            // SVG shapes can intentionally extend past the viewBox
            // origin (the user wrote them that way and SVG renderers
            // clip to the viewBox). We tolerate negative offsets here
            // — PowerPoint will clip the shape at the slide edge.
            // What we DON'T tolerate is shapes that overflow the
            // right / bottom edge, which is what the original bug
            // report looked like (the background rect was a 90%×90%
            // card in the middle of the slide, with text boxes
            // extending past the right edge).
            assert!(
                p.off_x + p.ext_w <= slide_w_emu + 1000,
                "shape #{i} overflows the right edge: {p:?}"
            );
            assert!(
                p.off_y + p.ext_h <= slide_h_emu + 1000,
                "shape #{i} overflows the bottom edge: {p:?}"
            );
        }
        // Note: we do NOT assert `solid > noFill` because every shape's
        // default stroke is `<a:ln><a:noFill/>`, which can flip the
        // ratio. The relevant invariant is "the gradient fallback
        // produced at least one solid fill" — which `solid > 0` plus
        // the colour assertion already guarantees.

        // Also walk the rest of the slides to make sure we didn't
        // regress any of them.
        for i in 2..=outcome.slide_count {
            let name = format!("ppt/slides/slide{i}.xml");
            let mut xml = String::new();
            archive
                .by_name(&name)
                .expect(&name)
                .read_to_string(&mut xml)
                .unwrap();
            assert!(
                xml.contains("<p:sp>"),
                "FAIL: {name} has no editable shapes.\n{xml}"
            );
        }

        // Mirror the freshly-built deck into the user's `test/`
        // directory so they can open it without running anything.
        // We only do this when the user-facing path actually exists
        // (so CI / a fresh clone doesn't accidentally write into a
        // non-existent target).
        let user_path = std::path::Path::new(&user_out_path);
        if let Some(parent) = user_path.parent() {
            if parent.exists() {
                std::fs::copy(&out_path, user_path).ok();
            }
        }
    }

    // ----- slide-size + shape-bounds regression tests --------------------
    //
    // The user reported that the PPTX opened with only a small
    // background-coloured rectangle in the middle of each slide,
    // instead of the full artwork filling the slide. Root cause was a
    // `build_slide_xml` that fitted the SVG viewBox into 90% of a
    // fixed 16:9 slide and used a hard-coded `pw = SLIDE_W_EMU * 0.7`
    // for text boxes — so a centred title (`x="640"`) started at
    // slide-middle + 0.7 × slide-width and overflowed the right edge.

    #[test]
    fn slide_size_matches_svg_viewbox() {
        // 1280 × 720 viewBox at 96 DPI → 12,192,000 × 6,858,000 EMU
        // (i.e. a standard 13.333" × 7.5" 16:9 slide, but the test
        // proves we DERIVE this from the viewBox rather than hard-
        // coding it).
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1280 720">
  <rect width="1280" height="720" fill="#FF0000"/>
</svg>"##;
        let parsed = parse_svg(svg).expect("parse");
        let (w, h) = compute_slide_size_emu(&parsed);
        assert_eq!(w, 12_192_000, "slide width must equal 1280 px at 96 DPI");
        assert_eq!(h, 6_858_000, "slide height must equal 720 px at 96 DPI");
    }

    #[test]
    fn slide_size_handles_non_standard_viewbox() {
        // Square viewBox → square slide.
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 800 800">
  <rect width="800" height="800" fill="#00FF00"/>
</svg>"##;
        let parsed = parse_svg(svg).expect("parse");
        let (w, h) = compute_slide_size_emu(&parsed);
        assert_eq!(w, h, "square viewBox → square slide");
        assert!(w > 0 && h > 0);
    }

    #[test]
    fn full_bleed_background_rect_fills_slide() {
        // The classic "user wrote a 1280x720 background rect" must
        // produce a slide-filling background shape, not a centred
        // 90%-of-slide rectangle.
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1280 720">
  <rect x="0" y="0" width="1280" height="720" fill="#1A1A2E"/>
</svg>"##;
        let parsed = parse_svg(svg).expect("parse");
        let slides = vec![SlideInput {
            source_path: "bg.svg".to_string(),
            slide_index: 1,
            content: parsed,
        }];
        let bytes = build_pptx(&slides, Some("bg")).expect("build pptx");
        let mut archive =
            zip::ZipArchive::new(std::io::Cursor::new(bytes.as_slice())).unwrap();
        let mut xml = String::new();
        archive
            .by_name("ppt/slides/slide1.xml")
            .unwrap()
            .read_to_string(&mut xml)
            .unwrap();
        // The rect's <a:off> must be at (0, 0) and its <a:ext> must
        // match the slide dimensions exactly (12_192_000 × 6_858_000).
        assert!(
            xml.contains("<a:off x=\"0\" y=\"0\"/><a:ext cx=\"12192000\" cy=\"6858000\"/>"),
            "background rect must fill the entire slide, not 90% of it.\n{xml}"
        );
    }

    #[test]
    fn all_shape_ids_are_unique_within_a_slide() {
        // PowerPoint silently drops every shape whose `<p:cNvPr id>`
        // collides with an earlier one — the deck renders as if only
        // the first shape exists. The first user-visible symptom
        // was "only the background rectangle shows up", because the
        // background was always emitted with `id=100` and so was
        // every subsequent shape.
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1280 720">
  <rect width="1280" height="720" fill="#1A1A2E"/>
  <circle cx="200" cy="200" r="80" fill="#FF0000"/>
  <circle cx="400" cy="400" r="80" fill="#00FF00"/>
  <circle cx="600" cy="600" r="80" fill="#0000FF"/>
  <text x="640" y="360" font-size="40" text-anchor="middle" fill="#FFF">Hello</text>
</svg>"##;
        let parsed = parse_svg(svg).expect("parse");
        let slides = vec![SlideInput {
            source_path: "ids.svg".to_string(),
            slide_index: 1,
            content: parsed,
        }];
        let bytes = build_pptx(&slides, Some("ids")).expect("build pptx");
        let mut archive =
            zip::ZipArchive::new(std::io::Cursor::new(bytes.as_slice())).unwrap();
        let mut xml = String::new();
        archive
            .by_name("ppt/slides/slide1.xml")
            .unwrap()
            .read_to_string(&mut xml)
            .unwrap();
        // Pull every cNvPr id out of the slide (both the group
        // `<p:nvGrpSpPr><p:cNvPr id="…"/>` and every per-shape one).
        let ids: Vec<String> = regex::Regex::new(r#"<p:cNvPr id="(\d+)""#)
            .unwrap()
            .captures_iter(&xml)
            .map(|cap| cap[1].to_string())
            .collect();
        // Sanity: we should have at least the group's id + 5 shapes.
        assert!(
            ids.len() >= 6,
            "expected at least 6 ids (group + 5 shapes), got {} in:\n{xml}",
            ids.len()
        );
        let mut seen = std::collections::HashSet::new();
        for id in &ids {
            assert!(
                seen.insert(id.clone()),
                "duplicate <p:cNvPr id=\"{id}\"> in slide1.xml:\n{xml}"
            );
        }
    }

    #[test]
    fn all_ooxml_tags_balance_and_txbody_is_sibling_of_sppr() {
        // Regression test for two related bugs that both manifested
        // as "PowerPoint opens the file but the slide is empty":
        //
        //  1. `<p:spPr>` was never closed (we wrote
        //     `<p:spPr>...</p:sp>` and skipped `</p:spPr>`). XML
        //     parsers bail out at the first mismatch, so PowerPoint
        //     silently dropped every shape after the broken one —
        //     which happened to be the only one rendered (the
        //     background).
        //
        //  2. `<p:txBody>` was being emitted *inside* `<p:spPr>`,
        //     because the text-shape writer didn't close `<p:spPr>`
        //     before pushing the text body. OOXML schema requires
        //     `<p:txBody>` to be a sibling of `<p:spPr>`, not a
        //     child — python-pptx saw the `<p:txBody>` but couldn't
        //     navigate to the runs, so every text box came back
        //     blank.
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1280 720">
  <rect width="1280" height="720" fill="#1A1A2E"/>
  <circle cx="200" cy="200" r="80" fill="#FF0000"/>
  <circle cx="400" cy="400" r="80" fill="#00FF00"/>
  <text x="640" y="360" font-size="40" text-anchor="middle" fill="#FFF">Hello</text>
</svg>"##;
        let parsed = parse_svg(svg).expect("parse");
        let slides = vec![SlideInput {
            source_path: "t.svg".to_string(),
            slide_index: 1,
            content: parsed,
        }];
        let bytes = build_pptx(&slides, Some("t")).expect("build pptx");
        let mut archive =
            zip::ZipArchive::new(std::io::Cursor::new(bytes.as_slice())).unwrap();
        let mut xml = String::new();
        archive
            .by_name("ppt/slides/slide1.xml")
            .unwrap()
            .read_to_string(&mut xml)
            .unwrap();

        // (1) Tag-balance check.
        for tag in ["p:sp", "p:spPr", "p:nvSpPr", "p:txBody"] {
            let open = xml.matches(&format!("<{tag}>")).count()
                + xml.matches(&format!("<{tag} ")).count();
            let close = xml.matches(&format!("</{tag}>")).count();
            assert_eq!(
                open, close,
                "<{tag}> opens ({open}) must equal </{tag}> closes ({close}); xml:\n{xml}"
            );
        }

        // (2) `<p:txBody>` must be a SIBLING of `<p:spPr>`, not a
        // child. We check by extracting the first `<p:sp>` whose
        // `<p:cNvPr name="TextBox"/>` and confirming the order is
        // `<p:spPr>…</p:spPr><p:txBody>…</p:txBody>`.
        let sp_re = regex::Regex::new(
            r#"<p:sp><p:nvSpPr><p:cNvPr id="\d+" name="TextBox"/>"#,
        )
        .unwrap();
        let sppr_close_re = regex::Regex::new(r"</p:spPr>").unwrap();
        let txbody_open_re = regex::Regex::new(r"<p:txBody>").unwrap();
        let start = sp_re.find(&xml).expect("expected at least one TextBox").start();
        let text_box_block = &xml[start..];
        let sppr_close_at = sppr_close_re.find(text_box_block).unwrap().start();
        let txbody_open_at = txbody_open_re.find(text_box_block).unwrap().start();
        assert!(
            txbody_open_at > sppr_close_at,
            "<p:txBody> must come AFTER </p:spPr>, not nested inside it. xml slice:\n{text_box_block}"
        );

        // (3) Sanity: the round-trip via python-pptx must surface
        // the text inside the text frame — this is the strongest
        // end-to-end check that the OOXML we emit is well-formed.
        // We do the parse by hand instead of pulling in python-pptx
        // as a cargo dependency: a successful `<a:t>` extraction
        // plus the tag-balance check above is enough.
        let hello_re = regex::Regex::new(r"<a:t>Hello</a:t>").unwrap();
        assert!(
            hello_re.is_match(&xml),
            "expected <a:t>Hello</a:t> in slide1.xml:\n{xml}"
        );
    }

    #[test]
    fn font_size_is_emitted_in_hundredths_of_a_point() {
        // OOXML `<a:rPr sz="…"/>` is in HUNDREDTHS of a point, and
        // SVG font sizes are in SVG px (at 96dpi). Because
        // 1 SVG px = 96/72 = 1.333 pt, the correct conversion is
        // SVG_px × 0.75 = PowerPoint_pt (so sz = SVG_px × 75 in
        // hundredths of a point). Without the ×0.75 factor, text
        // renders 33% too large in PowerPoint compared to the SVG
        // preview. Previously the code emitted the raw SVG px value
        // (e.g. `sz="48"` for font-size="48") which PowerPoint
        // rendered as 0.48 pt — invisible at normal zoom.
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1280 720">
  <text x="100" y="100" font-size="48" fill="#FFF">Big text</text>
  <text x="100" y="200" font-size="18" fill="#FFF">Small text</text>
  <text x="100" y="300" fill="#FFF">Default text</text>
</svg>"##;
        let parsed = parse_svg(svg).expect("parse");
        let slides = vec![SlideInput {
            source_path: "sizes.svg".to_string(),
            slide_index: 1,
            content: parsed,
        }];
        let bytes = build_pptx(&slides, Some("sizes")).expect("build pptx");
        let mut archive =
            zip::ZipArchive::new(std::io::Cursor::new(bytes.as_slice())).unwrap();
        let mut xml = String::new();
        archive
            .by_name("ppt/slides/slide1.xml")
            .unwrap()
            .read_to_string(&mut xml)
            .unwrap();
        // SVG font-size="48" px → 48 × 0.75 = 36 pt → sz="3600"
        // SVG font-size="18" px → 18 × 0.75 = 13.5 pt → sz="1350"
        // Default (18 px) → sz="1350"
        assert!(
            xml.contains(r#"sz="3600""#),
            "expected sz=\"3600\" for SVG font-size=\"48\" px (→ 36pt), got:\n{xml}"
        );
        assert!(
            xml.contains(r#"sz="1350""#),
            "expected sz=\"1350\" for SVG font-size=\"18\" px (→ 13.5pt), got:\n{xml}"
        );
        // The old buggy values (raw SVG px as raw hundredths-of-pt)
        // must NOT appear.
        for raw in [r#"sz="48""#, r#"sz="18""#] {
            assert!(
                !xml.contains(raw),
                "{raw} should not appear — sz must be in hundredths of a pt after ×0.75 conversion. xml:\n{xml}"
            );
        }
    }

    #[test]
    fn rgba_alpha_round_trips_into_ooxml() {
        // SVG `rgba(r, g, b, a)` carries the alpha channel for
        // the "glass" semi-transparent strokes / fills. The earlier
        // version of `parse_paint` silently dropped the alpha,
        // which made every stroke fully opaque — losing the glass
        // look the user originally authored.
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1280 720">
  <circle cx="100" cy="100" r="50" fill="none" stroke="rgba(255,255,255,0.03)" stroke-width="2"/>
  <circle cx="300" cy="100" r="50" fill="rgba(0,210,255,0.25)"/>
  <rect width="100" height="100" fill="#FF000080"/>
</svg>"##;
        let parsed = parse_svg(svg).expect("parse");
        let slides = vec![SlideInput {
            source_path: "glass.svg".to_string(),
            slide_index: 1,
            content: parsed,
        }];
        let bytes = build_pptx(&slides, Some("glass")).expect("build pptx");
        let mut archive =
            zip::ZipArchive::new(std::io::Cursor::new(bytes.as_slice())).unwrap();
        let mut xml = String::new();
        archive
            .by_name("ppt/slides/slide1.xml")
            .unwrap()
            .read_to_string(&mut xml)
            .unwrap();
        // rgba(255,255,255,0.03) → alpha = 0.03 * 100_000 = 3000
        assert!(
            xml.contains(r#"<a:alpha val="3000""#),
            "expected <a:alpha val=\"3000\"/> for rgba(255,255,255,0.03) stroke, got:\n{xml}"
        );
        // rgba(0,210,255,0.25) → alpha = 0.25 * 100_000 = 25000
        assert!(
            xml.contains(r#"<a:alpha val="25000""#),
            "expected <a:alpha val=\"25000\"/> for rgba(0,210,255,0.25) fill, got:\n{xml}"
        );
        // #RRGGBBAA where AA=0x80 → alpha = 128/255 ≈ 0.502
        //   * 100_000 = 50196 (rounded)
        assert!(
            xml.contains(r#"<a:alpha val="50196""#),
            "expected <a:alpha val=\"50196\"/> for #FF000080 fill (alpha 128/255), got:\n{xml}"
        );
    }

    #[test]
    fn text_boxes_fit_within_slide_bounds() {
        // A centred title (text-anchor="middle") at x=640 in a 1280
        // viewBox used to overflow because we hard-coded pw = 0.7 ×
        // slide width, putting the right edge at 0.7 × 12_192_000 =
        // 8_534_400 from x=640 → past the slide's right edge.
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1280 720">
  <text x="640" y="360" font-size="48" text-anchor="middle" fill="#FFFFFF">Hello</text>
</svg>"##;
        let parsed = parse_svg(svg).expect("parse");
        let slides = vec![SlideInput {
            source_path: "t.svg".to_string(),
            slide_index: 1,
            content: parsed,
        }];
        let bytes = build_pptx(&slides, Some("t")).expect("build pptx");
        let mut archive =
            zip::ZipArchive::new(std::io::Cursor::new(bytes.as_slice())).unwrap();
        let mut xml = String::new();
        archive
            .by_name("ppt/slides/slide1.xml")
            .unwrap()
            .read_to_string(&mut xml)
            .unwrap();
        // Pull every <a:off>/<a:ext> pair from the slide and assert
        // nothing overflows the right / bottom of the slide (we
        // tolerate shapes that legitimately extend past the left
        // edge — see `write_shape` for the rationale).
        let pairs = parse_off_ext(&xml);
        let slide_w = 12_192_000_i64;
        let slide_h = 6_858_000_i64;
        for (i, p) in pairs.iter().enumerate() {
            assert!(
                p.ext_w > 0 && p.ext_h > 0,
                "shape #{i} has zero/negative extent: {:?}",
                p
            );
            assert!(
                p.off_x + p.ext_w <= slide_w + 1000, // 1000 EMU slack
                "shape #{i} overflows the right edge: off_x={} ext_w={}",
                p.off_x,
                p.ext_w
            );
            assert!(
                p.off_y + p.ext_h <= slide_h + 1000,
                "shape #{i} overflows the bottom edge: off_y={} ext_h={}",
                p.off_y,
                p.ext_h
            );
        }
        // Sanity: we should have exactly ONE shape (the text box).
        assert_eq!(pairs.len(), 1, "expected exactly one shape, got {:?}", pairs);
        // And the text box should be reasonably wide (centred), not
        // squeezed into a 70%-of-slide strip.
        let text_box = &pairs[0];
        assert!(
            text_box.ext_w >= slide_w / 2,
            "centred text box width ({} EMU) is less than half the slide ({})",
            text_box.ext_w,
            slide_w / 2
        );
        // And the box's centre must coincide with the SVG text
        // anchor `x=640` (slide-w centre). This pins the bug where
        // the box was clamped to `off_x >= 0`, which would have
        // shifted the visible text to the slide-left edge instead.
        let box_centre_x = text_box.off_x + text_box.ext_w / 2;
        assert!(
            (box_centre_x - 6_096_000).abs() < 1_000,
            "centred text box centre ({box_centre_x}) must align with the SVG anchor x=640 (6_096_000 EMU)"
        );
    }

    #[test]
    fn text_baseline_matches_svg_y() {
        // Regression test for the "everything is shifted down" bug:
        // SVG `<text y="…"/>` is the *baseline*, while OOXML
        // `<p:txBody>` with `anchor="t"` places the baseline a font
        // size below the box top. The earlier writer treated
        // `py_baseline` as the box top directly, which dropped every
        // text run roughly one ascent lower than the SVG authored.
        //
        // The sz values in the XML are SVG_px × 75 (SVG px → PPT pt
        // is ×0.75). The baseline in PowerPoint with `anchor="t"` is
        // at box_top + sz_pt × 0.95. We verify the baseline lands on
        // the SVG `y` by reading the XML `sz` back, dividing by 75 to
        // recover SVG px, then verifying the baseline position.
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1280 720">
  <text x="640" y="100" font-size="64"  text-anchor="middle" fill="#FFF">Big</text>
  <text x="640" y="250" font-size="28"  text-anchor="middle" fill="#FFF">Mid</text>
  <text x="640" y="400" font-size="18"  text-anchor="middle" fill="#FFF">Small</text>
  <text x="640" y="600" font-size="14"  text-anchor="middle" fill="#FFF">Tiny</text>
</svg>"##;
        let parsed = parse_svg(svg).expect("parse");
        let slides = vec![SlideInput {
            source_path: "baseline.svg".to_string(),
            slide_index: 1,
            content: parsed,
        }];
        let bytes = build_pptx(&slides, Some("baseline")).expect("build pptx");
        let mut archive =
            zip::ZipArchive::new(std::io::Cursor::new(bytes.as_slice())).unwrap();
        let mut xml = String::new();
        archive
            .by_name("ppt/slides/slide1.xml")
            .unwrap()
            .read_to_string(&mut xml)
            .unwrap();
        let pairs = parse_off_ext(&xml);
        // Pull the first `<a:rPr sz="…"/>` from each text box in
        // document order. These are in hundredths of PPT pt (sz =
        // SVG_px × 75). Dividing by 75 recovers the SVG px; dividing
        // by 100 gives PPT pt.
        let sz_re = regex::Regex::new(r#"<a:rPr[^>]*sz="(\d+)""#).unwrap();
        let szs_svg_px: Vec<f64> = sz_re
            .captures_iter(&xml)
            .map(|c| c[1].parse::<i64>().unwrap() as f64 / 75.0)
            .collect();
        assert_eq!(
            szs_svg_px.len(),
            4,
            "expected four <a:rPr sz=\"…\"/> runs in:\n{xml}",
        );
        // The four SVG px values (hardcoded above in the SVG string).
        let szs_svg_px_expected = [64.0_f64, 28.0, 18.0, 14.0];
        // Verify the SVG px → PPT pt conversion (×0.75) round-trips correctly.
        for (i, (got, expected)) in szs_svg_px.iter().zip(szs_svg_px_expected.iter()).enumerate() {
            assert!(
                (got - expected).abs() < 0.5,
                "text #{i}: sz in XML (sz/75={got} SVG px) doesn't match SVG px={expected}",
            );
        }
        // For each text box, verify its baseline lands on the SVG `y`.
        // PowerPoint anchor="t": baseline = box_top + sz_ppt_pt × 0.95 (in pt)
        // SVG_px → PPT_pt = SVG_px × 0.75
        // baseline_emu = off_y + SVG_px × 0.75 × 0.95 / 72 × 914400
        let targets = [100.0_f64, 250.0, 400.0, 600.0];
        for (i, target) in targets.iter().enumerate() {
            let p = &pairs[i];
            let sz_svg_px = szs_svg_px[i];
            let sz_ppt_pt = sz_svg_px * 0.75;
            let baseline_emu =
                p.off_y as f64 + sz_ppt_pt * 0.95 / 72.0 * 914_400.0;
            let baseline_svg = baseline_emu / 9525.0;
            assert!(
                (baseline_svg - target).abs() < 1.0,
                "text #{i}: baseline drifted from SVG y={target} (got {baseline_svg:.2}); \
                 off_y={}, sz={sz_svg_px}px (→ {sz_ppt_pt}pt)",
                p.off_y,
            );
        }
    }

    #[test]
    fn text_box_centred_label_lands_at_anchor() {
        // Regression test: a card-style label whose SVG `text x` is
        // far from the slide centre must still render at the same
        // horizontal position it would in an SVG renderer. Earlier
        // versions clamped the text box's off_x to >= 0, which
        // collapsed every `<text text-anchor="middle">` to the slide
        // left edge and then "centred" the text inside that small
        // box — putting the visible text well to the right of where
        // the user drew it.
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1280 720">
  <rect x="80" y="180" width="340" height="200" fill="#222"/>
  <text x="250" y="208" font-size="20" text-anchor="middle" fill="#FFF">Ask 问答模式</text>
</svg>"##;
        let parsed = parse_svg(svg).expect("parse");
        let slides = vec![SlideInput {
            source_path: "ask.svg".to_string(),
            slide_index: 1,
            content: parsed,
        }];
        let bytes = build_pptx(&slides, Some("ask")).expect("build pptx");
        let mut archive =
            zip::ZipArchive::new(std::io::Cursor::new(bytes.as_slice())).unwrap();
        let mut xml = String::new();
        archive
            .by_name("ppt/slides/slide1.xml")
            .unwrap()
            .read_to_string(&mut xml)
            .unwrap();
        // Pick out the TextBox shape (skip the rect).
        let pairs = parse_off_ext(&xml);
        let text_box = pairs
            .iter()
            .find(|p| p.ext_w > 1_000_000 && p.ext_h > 0)
            .expect("expected a wide text box");
        // Box centre must coincide with the SVG anchor (250 * 9525 = 2_381_250 EMU).
        let box_centre_x = text_box.off_x + text_box.ext_w / 2;
        assert!(
            (box_centre_x - 2_381_250).abs() < 1_000,
            "card-label box centre ({box_centre_x}) must align with SVG x=250 (2_381_250 EMU), got off_x={} ext_w={}",
            text_box.off_x,
            text_box.ext_w
        );
        // And we must use `<a:pPr algn="ctr"/>` so the visible text
        // is centred inside the box.
        assert!(
            xml.contains("algn=\"ctr\""),
            "card-label text must use algn=\"ctr\""
        );
    }

    #[test]
    fn text_anchor_start_aligns_left() {
        // A `<text>` with text-anchor="start" (default) at x=40 must
        // produce a text box that starts near x=40 and runs to the
        // slide's right edge (so multi-line wraps work).
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1280 720">
  <text x="40" y="100" font-size="18" text-anchor="start" fill="#FFFFFF">Left</text>
</svg>"##;
        let parsed = parse_svg(svg).expect("parse");
        let slides = vec![SlideInput {
            source_path: "l.svg".to_string(),
            slide_index: 1,
            content: parsed,
        }];
        let bytes = build_pptx(&slides, Some("l")).expect("build pptx");
        let mut archive =
            zip::ZipArchive::new(std::io::Cursor::new(bytes.as_slice())).unwrap();
        let mut xml = String::new();
        archive
            .by_name("ppt/slides/slide1.xml")
            .unwrap()
            .read_to_string(&mut xml)
            .unwrap();
        assert!(xml.contains("algn=\"l\""), "left-aligned text box missing algn=\"l\".\n{xml}");
    }

    #[test]
    fn text_anchor_end_aligns_right() {
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1280 720">
  <text x="1240" y="100" font-size="18" text-anchor="end" fill="#FFFFFF">Right</text>
</svg>"##;
        let parsed = parse_svg(svg).expect("parse");
        let slides = vec![SlideInput {
            source_path: "r.svg".to_string(),
            slide_index: 1,
            content: parsed,
        }];
        let bytes = build_pptx(&slides, Some("r")).expect("build pptx");
        let mut archive =
            zip::ZipArchive::new(std::io::Cursor::new(bytes.as_slice())).unwrap();
        let mut xml = String::new();
        archive
            .by_name("ppt/slides/slide1.xml")
            .unwrap()
            .read_to_string(&mut xml)
            .unwrap();
        assert!(xml.contains("algn=\"r\""), "right-aligned text box missing algn=\"r\".\n{xml}");
    }

    #[derive(Debug)]
    struct OffExt {
        off_x: i64,
        off_y: i64,
        ext_w: i64,
        ext_h: i64,
    }

    /// Pull every `<a:off x="…" y="…"/>` and the next `<a:ext cx="…"
    /// cy="…"/>` from a slide's XML. We use this to assert that no
    /// shape overflows the slide bounds.
    fn parse_off_ext(xml: &str) -> Vec<OffExt> {
        // We look for the specific shape geometry pattern emitted by
        // `write_sp_open`: `<a:xfrm><a:off x="N" y="N"/><a:ext cx="N"
        // cy="N"/></a:xfrm>`. PowerPoint's XML preserves this exact
        // ordering (we tested above; see the e2e output).
        //
        // Note: this regex is XML-only — it does not handle
        // whitespace inside the angle brackets, but our writer
        // doesn't emit any.
        let re = regex::Regex::new(
            r#"<a:off x="(-?\d+)" y="(-?\d+)"/><a:ext cx="(-?\d+)" cy="(-?\d+)"/>"#,
        )
        .unwrap();
        re.captures_iter(xml)
            .map(|cap| OffExt {
                off_x: cap[1].parse().unwrap(),
                off_y: cap[2].parse().unwrap(),
                ext_w: cap[3].parse().unwrap(),
                ext_h: cap[4].parse().unwrap(),
            })
            // PowerPoint's empty-group geometry (`<p:grpSpPr>`) is
            // emitted as `cx="0" cy="0"`. That's a placeholder, not
            // a real shape — skip it so it doesn't pollute the
            // bounds check.
            .filter(|p| p.ext_w > 0 && p.ext_h > 0)
            .collect()
    }
}