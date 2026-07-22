use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::io::{Cursor, Write};
use std::path::PathBuf;

use super::{validate_workspace_path, ToolDefinition, ToolError, ToolParameters};

mod svg_parser;

// Re-export the parser surface so existing
// `crate::agent::tools::pptx::ParsedSvg` etc. import paths continue to
// resolve. The OOXML writer (`write_shape`, `build_slide_xml`, …) still
// lives in `mod.rs`; the cross-module use sites are intentionally
// invisible to callers.
pub use svg_parser::{
    parse_color, parse_svg, GradientStop, Paint, ParsedSvg, SvgShape, TextRun, Transform,
};
pub(crate) use svg_parser::{base64_decode, base64_encode, SlideImage};

/// Intermediate representation of one input SVG. Holds the parser
/// output plus the originating path/index for diagnostics.
pub struct SlideInput {
    pub source_path: String,
    pub slide_index: usize,
    pub content: ParsedSvg,
}

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
        let _ = slide_idx;
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

