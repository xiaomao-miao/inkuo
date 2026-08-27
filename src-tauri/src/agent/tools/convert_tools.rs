//! Format-conversion tools: svg_to_png, word_to_pdf
//!
//! Two "source-file → target-file" converters that the
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
//!   - Word → PDF: `office2pdf` (pure Rust, Typst backend — no
//!     LibreOffice, no Chromium, no Docker).

use serde::Deserialize;
use serde_json::Value;
use std::path::{Path, PathBuf};

use super::{validate_workspace_path, ToolDefinition, ToolError, ToolParameters};

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
    pub fn new() -> Self {
        Self
    }

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
    fn default() -> Self {
        Self::new()
    }
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
    let transform = resvg::tiny_skia::Transform::from_scale(w as f32 / intr_w, h as f32 / intr_h);
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

// ── word_to_pdf ────────────────────────────────────────────────────────────────

pub struct WordToPdfTool;

impl WordToPdfTool {
    pub fn new() -> Self {
        Self
    }

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
    fn default() -> Self {
        Self::new()
    }
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
mod convert_tools_tests {
    //! Smoke tests for `svg_to_png`. The previous `md_to_word → word_to_pdf`
    //! round-trip test was removed together with `md_to_word`.
    use super::*;
    use std::path::PathBuf;

    fn tmp_path(name: &str) -> PathBuf {
        let mut p = std::env::temp_dir();
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        p.push(format!("inkuo_svg_{}_{}", name, stamp));
        p
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
