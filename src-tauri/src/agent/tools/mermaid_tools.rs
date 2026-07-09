//! Mermaid rendering tools: render_mermaid
//!
//! Uses the `merman` crate (a headless, parity-focused re-implementation of
//! Mermaid.js in pure Rust). We target mermaid@11.15.0 and rasterize through
//! merman's resvg-safe pipeline, so the PNG a sub-agent hands to the docx
//! inserter matches the SVG the AI chat panel renders client-side.
//!
//! No Node.js, no Chromium, no puppeteer download — `Engine::new()` is
//! ~milliseconds cold and ~15 MB resident. The renderer is shared across
//! calls (Arc-cloned into spawn_blocking) so the per-render cost after the
//! first diagram is the layout + resvg rasterization itself.

use merman::render::raster::{render_png_sync, RasterFitBox, RasterOptions, RasterSizeLimit};
use merman::render::{render_svg_sync, LayoutOptions, SvgRenderOptions};
use merman::{Engine, ParseOptions};
use serde::Deserialize;
use serde_json::Value;
use std::path::PathBuf;

use super::{validate_workspace_path as shared_validate_workspace_path, ToolDefinition, ToolError, ToolParameters};

/// Shared renderer. `Engine` and the option structs are cheap to clone
/// (Arc-counted internally), so we just keep one and clone per call —
/// no need for an `Arc` wrapper.
pub struct RenderMermaidTool {
    engine: Engine,
    layout: LayoutOptions,
    svg: SvgRenderOptions,
    parse: ParseOptions,
}

impl RenderMermaidTool {
    pub fn new() -> Self {
        Self {
            engine: Engine::new(),
            // Mermaid-like text measurer backed by vendored font metrics —
            // matches what the browser mermaid.js uses by default.
            layout: LayoutOptions::headless_svg_defaults(),
            svg: SvgRenderOptions::default(),
            parse: ParseOptions::default(),
        }
    }

    pub fn definition(&self) -> ToolDefinition {
        ToolDefinition::new_with_label(
            "render_mermaid",
            "渲染 Mermaid",
            "Render Mermaid diagram source code to an image file (PNG / SVG / PDF). Pure-Rust headless renderer (mermaid@11.15.0 parity); no Node.js or Chromium required. Output extension determines format: .png / .svg / .pdf.",
            ToolParameters::new(
                vec!["mermaid_code", "output_path"],
                vec![
                    ("mermaid_code", "string", Some("Raw Mermaid source code (no code fence)")),
                    ("output_path", "string", Some("Absolute path of the output image file (.png / .svg / .pdf). Parent directory is created if missing.")),
                    ("width", "integer", Some("Output width in pixels (PNG only). Default: 1200")),
                    ("height", "integer", Some("Output height in pixels (PNG only). Default: 800")),
                    ("theme", "string", Some("Reserved for future per-theme parity; today the renderer uses Mermaid's default theme. Default: 'default'")),
                    ("background", "string", Some("CSS background color. Default: 'white'")),
                ],
            ),
        )
    }

    /// Returns `(json_output, file_path)` so the registry wrapper can
    /// stamp `file_path` onto the `ToolResult` and trigger the frontend's
    /// `file-written` event.
    pub async fn execute(
        &self,
        arguments: Value,
        workspace: Option<String>,
    ) -> Result<RenderOutcome, ToolError> {
        let args: RenderMermaidArgs = serde_json::from_value(arguments)
            .map_err(|e| ToolError::InvalidArguments(
                "render_mermaid".to_string(),
                format!("Invalid parameters: {}", e),
            ))?;

        shared_validate_workspace_path(&args.output_path, &workspace)?;

        let output_path = PathBuf::from(&args.output_path);
        let extension = output_path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if !matches!(extension.as_str(), "png" | "svg" | "pdf") {
            return Err(ToolError::InvalidArguments(
                "render_mermaid".to_string(),
                format!(
                    "Unsupported output extension '.{}'; must be one of png, svg, pdf",
                    extension
                ),
            ));
        }

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

        let width = args.width.unwrap_or(1200).max(64);
        let height = args.height.unwrap_or(800).max(64);
        let background = args.background.clone().unwrap_or_else(|| "white".to_string());
        let mermaid_code = args.mermaid_code.clone();

        // The renderer is CPU-bound and can be slow on complex diagrams —
        // off the render off the async runtime so it never blocks other
        // tool calls running on the same Tokio executor. `Engine` and
        // the option structs are cheap to clone (Arc-counted internally).
        let engine = self.engine.clone();
        let layout = self.layout.clone();
        let svg = self.svg.clone();
        let parse = self.parse.clone();
        let output_path_for_render = output_path.clone();
        let extension_for_render = extension.clone();

        let render_result = tokio::task::spawn_blocking(move || {
            render_to_file(
                &engine,
                &layout,
                &svg,
                &parse,
                &mermaid_code,
                &output_path_for_render,
                &extension_for_render,
                width,
                height,
                &background,
            )
        })
        .await
        .map_err(|e| ToolError::ExecutionError(format!(
            "Mermaid render task panicked: {}",
            e
        )))?;

        match render_result {
            Ok(bytes) => {
                tokio::fs::write(&output_path, &bytes).await.map_err(|e| {
                    ToolError::IoError(format!(
                        "Failed to write output to {}: {}",
                        output_path.display(),
                        e
                    ))
                })?;
                let result_json = serde_json::json!({
                    "output_path": output_path.to_string_lossy(),
                    "bytes": bytes.len(),
                    "format": extension,
                })
                .to_string();
                Ok(RenderOutcome {
                    output: result_json,
                    is_error: false,
                    file_path: Some(output_path.to_string_lossy().to_string()),
                })
            }
            Err(e) => Ok(RenderOutcome {
                output: format!(
                    "Mermaid render failed: {}\n\nSource:\n{}",
                    e,
                    truncate(&args.mermaid_code, 1000),
                ),
                is_error: true,
                file_path: None,
            }),
        }
    }
}

impl Default for RenderMermaidTool {
    fn default() -> Self { Self::new() }
}

/// Tuple-equivalent of `ToolResult` but pre-`file_path` stitching. The
/// registry wrapper in `mod.rs` converts this into a real `ToolResult`
/// using `ToolResult::success` / `ToolResult::error`.
pub struct RenderOutcome {
    pub output: String,
    pub is_error: bool,
    pub file_path: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RenderMermaidArgs {
    mermaid_code: String,
    output_path: String,
    width: Option<u32>,
    height: Option<u32>,
    /// Currently unused — merman applies Mermaid's default theme. Kept on
    /// the tool signature for forward compatibility with a future
    /// per-theme parity pass.
    #[allow(dead_code)]
    theme: Option<String>,
    background: Option<String>,
}

fn render_to_file(
    engine: &Engine,
    layout: &LayoutOptions,
    svg: &SvgRenderOptions,
    parse: &ParseOptions,
    code: &str,
    _output_path: &std::path::Path,
    extension: &str,
    width: u32,
    height: u32,
    background: &str,
) -> Result<Vec<u8>, String> {
    match extension {
        "png" => render_png(engine, layout, svg, parse, code, width, height, background),
        "svg" => render_svg(engine, layout, svg, parse, code).map(|s| s.into_bytes()),
        "pdf" => Err("PDF output is not yet wired up to the Rust renderer; use .png or .svg".to_string()),
        other => Err(format!("Unsupported extension: .{}", other)),
    }
}

fn render_png(
    engine: &Engine,
    layout: &LayoutOptions,
    svg: &SvgRenderOptions,
    parse: &ParseOptions,
    code: &str,
    width: u32,
    height: u32,
    background: &str,
) -> Result<Vec<u8>, String> {
    let raster = RasterOptions {
        // 2x scale → crisp output on hi-DPI docx rendering.
        scale: 2.0,
        background: Some(background.to_string()),
        jpeg_quality: 90,
        fit_to: Some(RasterFitBox {
            width: Some(width),
            height: Some(height),
        }),
        size_limit: RasterSizeLimit::default(),
    };

    let png = render_png_sync(engine, code, parse.clone(), layout, svg, &raster)
        .map_err(|e| format!("{:?}", e))?
        .ok_or_else(|| "mermaid: diagram type not detected (no output produced)".to_string())?;

    if png.is_empty() {
        return Err("mermaid: renderer produced an empty PNG buffer".to_string());
    }

    Ok(png)
}

fn render_svg(
    engine: &Engine,
    layout: &LayoutOptions,
    svg: &SvgRenderOptions,
    parse: &ParseOptions,
    code: &str,
) -> Result<String, String> {
    let svg = render_svg_sync(engine, code, parse.clone(), layout, svg)
        .map_err(|e| format!("{:?}", e))?
        .ok_or_else(|| "mermaid: diagram type not detected (no output produced)".to_string())?;

    Ok(svg)
}

fn truncate(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        s.to_string()
    } else {
        let mut end = max_bytes;
        while end > 0 && !s.is_char_boundary(end) {
            end -= 1;
        }
        let mut out = String::with_capacity(end + 16);
        out.push_str(&s[..end]);
        out.push_str("\n...[truncated]");
        out
    }
}
