//! PowerPoint animation authoring tools
//!
//! Provides:
//! 1. **`CreatePptxAnimationTool`** — generates a `.pptx` from SVG files with full animation
//!    and transition support.
//! 2. **`AddAnimationTool`** — adds animations to an existing `.pptx` file.
//!
//! ## Animation taxonomy
//!
//! | Category   | Effect             | OOXML element         |
//! | ---------- | ------------------ | --------------------- |
//! | Entrance   | fadeIn, flyIn, zoom | `<p:anim>` opacity/x |
//! | Emphasis   | pulse, spin         | `<p:animScale>`       |
//! | Exit       | fadeOut, flyOut     | `<p:anim>` opacity/x  |
//! | Set        | toggle, show, hide  | `<p:set>`             |
//! | Transitions | fade, push, wipe  | `<p:transition>`      |

use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::io::{Cursor, Read, Write};
use std::path::PathBuf;
use zip;

use super::{validate_workspace_path, ToolDefinition, ToolError, ToolParameters};
use crate::agent::tools::pptx::{
    build_content_types, build_core_props_xml, build_presentation_rels,
    build_presentation_xml, build_root_rels, build_slide_rels, parse_svg,
    write_shape, xml_escape, EMU_PER_INCH, SLIDE_H_EMU, SLIDE_W_EMU,
    APP_XML, SLIDE_LAYOUT_RELS, SLIDE_LAYOUT_XML, SLIDE_MASTER_RELS, SLIDE_MASTER_XML, THEME_XML,
    ParsedSvg as PptxParsedSvg,
};

// ===========================================================================
// Public outcome / args / tool structs
// ===========================================================================

#[derive(Debug)]
pub struct CreatePptxAnimationOutcome {
    pub output: String,
    pub file_path: String,
    pub byte_size: usize,
    pub slide_count: usize,
    pub animation_count: usize,
    pub is_error: bool,
}

mod animation_xml;

// Re-export the OOXML animation/transition XML builders from
// `animation_xml.rs`. The `mod.rs` keeps the tool `impl` blocks and
// the SVG animation parser; these helpers are pure string templating
// with no I/O, so they sit naturally next to each other.
pub(crate) use animation_xml::{
    build_timing_xml, build_transition_xml, Anim, EmphasisStyle, FlyDir, Trigger,
};

impl Serialize for CreatePptxAnimationOutcome {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&json!({
            "output": self.output,
            "file_path": self.file_path,
            "byte_size": self.byte_size,
            "slide_count": self.slide_count,
            "animation_count": self.animation_count,
            "is_error": self.is_error,
        }).to_string())
    }
}

#[derive(Debug, Deserialize)]
struct CreatePptxAnimationArgs {
    svg_paths: Vec<String>,
    output_path: String,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    slide_animations: Option<Vec<SlideAnimationSpec>>,
    #[serde(default)]
    transition: Option<TransitionEffect>,
    #[serde(default = "default_speed")]
    transition_speed: String,
}

fn default_speed() -> String { "med".to_string() }

#[derive(Debug, Deserialize)]
struct SlideAnimationSpec {
    slide_index: usize,
    animations: Vec<AnimationSpec>,
    #[serde(default)]
    transition: Option<TransitionEffect>,
    #[serde(default)]
    transition_speed: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct AnimationSpec {
    /// Shape selector: index (0-based), "@first", "@last", "@all", or name pattern.
    #[serde(default)]
    shape: Option<String>,
    #[serde(default = "default_effect")]
    effect: String,
    #[serde(default = "default_dur")]
    duration_ms: u32,
    #[serde(default)]
    delay_ms: u32,
    /// "onclick" | "afterprev" | "withprev"
    #[serde(default)]
    trigger: String,
    /// For fly-in/fly-out: "l" | "r" | "t" | "b" | "tl" | "tr" | "bl" | "br"
    #[serde(default)]
    direction: String,
    /// For zoom entrance: starting scale (0.0 = invisible, 1.0 = normal)
    #[serde(default)]
    zoom_scale: Option<f64>,
}

fn default_effect() -> String { "fadeIn".to_string() }
fn default_dur() -> u32 { 500 }

#[derive(Debug, Clone, Deserialize, Serialize)]
pub(crate) struct TransitionEffect {
    #[serde(default = "default_trans")]
    transition_type: String,
    /// Direction for directional transitions: "l" | "r" | "t" | "b"
    #[serde(default)]
    direction: String,
    /// Color for fadeThroughColor: hex RGB
    #[serde(default)]
    color: Option<String>,
}

fn default_trans() -> String { "fade".to_string() }

#[derive(Debug, Deserialize)]
struct AddAnimationArgs {
    input_pptx: String,
    output_pptx: String,
    slides: Vec<SlideAnimationSpec>,
    #[serde(default)]
    transition: Option<TransitionEffect>,
    #[serde(default = "default_speed")]
    transition_speed: String,
}


// ===========================================================================
// OOXML XML builders
// ===========================================================================

#[derive(Debug)]
struct SvgAnim {
    target_id: Option<String>,
    attr: String,
    from: Option<String>,
    to: String,
    dur_ms: Option<u32>,
    begin_ms: Option<u32>,
}

fn parse_svg_animations(svg: &str) -> Vec<SvgAnim> {
    let mut out = Vec::new();
    let mut reader = Reader::from_str(svg);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                let name = e.name();
                let tag = std::str::from_utf8(name.as_ref()).unwrap_or("");
                if tag == "animate" || tag == "set" {
                    let attrs = read_attrs_qxml(&e);
                    if let Some(a) = parse_svg_anim_tag(&attrs) {
                        out.push(a);
                    }
                }
            }
            Ok(Event::Empty(e)) => {
                let name = e.name();
                let tag = std::str::from_utf8(name.as_ref()).unwrap_or("");
                if tag == "animate" || tag == "set" {
                    let attrs = read_attrs_qxml(&e);
                    if let Some(a) = parse_svg_anim_tag(&attrs) {
                        out.push(a);
                    }
                }
            }
            Ok(Event::Eof) => break,
            _ => {}
        }
        buf.clear();
    }
    out
}

fn read_attrs_qxml(e: &BytesStart) -> BTreeMap<String, String> {
    let mut m = BTreeMap::new();
    for a in e.attributes() {
        if let Ok(a) = a {
            let k = std::str::from_utf8(a.key.as_ref()).unwrap_or("").to_string();
            let v = a.unescape_value().map(|v| v.into_owned()).unwrap_or_default();
            m.insert(k, v);
        }
    }
    m
}

fn parse_svg_anim_tag(attrs: &BTreeMap<String, String>) -> Option<SvgAnim> {
    let attr = attrs.get("attributeName")?.clone();
    let to = attrs.get("to")?.clone();
    let from = attrs.get("from").cloned();
    let dur_ms = attrs.get("dur").and_then(|v| parse_svg_dur(v));
    let begin_ms = attrs.get("begin").and_then(|v| parse_svg_dur(v));
    Some(SvgAnim {
        target_id: attrs.get("href")
            .or_else(|| attrs.get("xlink:href"))
            .cloned(),
        attr,
        from,
        to,
        dur_ms,
        begin_ms,
    })
}

fn parse_svg_dur(s: &str) -> Option<u32> {
    let s = s.trim();
    let n: f64 = if let Some(r) = s.strip_suffix("ms") {
        r.parse().ok()?
    } else if let Some(r) = s.strip_suffix('s') {
        r.parse::<f64>().ok()? * 1000.0
    } else if let Some(r) = s.strip_suffix("min") {
        r.parse::<f64>().ok()? * 60000.0
    } else {
        s.parse().ok()?
    };
    Some(n.round() as u32)
}

fn svg_anim_to_ooxml(a: &SvgAnim, sid: usize) -> Option<Anim> {
    let dur = a.dur_ms.unwrap_or(500);
    let delay = a.begin_ms.unwrap_or(0);

    match a.attr.to_lowercase().as_str() {
        "opacity" => {
            let from_v: f64 = a.from.as_ref()?.parse().ok()?;
            let to_v: f64 = a.to.parse().ok()?;
            if from_v < to_v {
                Some(Anim::Fade { sid, dur, delay, trigger: Trigger::OnClick })
            } else {
                Some(Anim::ExitFade { sid, dur, delay, trigger: Trigger::OnClick })
            }
        }
        "visibility" | "display" | "fill-opacity" => {
            Some(Anim::Set {
                sid,
                delay,
                trigger: Trigger::OnClick,
                property: format!("style.{}", a.attr),
                value: a.to.clone(),
            })
        }
        _ => {
            // Generic opacity fade as fallback
            Some(Anim::Fade { sid, dur, delay, trigger: Trigger::OnClick })
        }
    }
}

// ===========================================================================
// Spec → Anim converter
// ===========================================================================

fn resolve_shape_ids(spec: &AnimationSpec, count: usize) -> Vec<usize> {
    let sel = spec.shape.as_deref().unwrap_or("@all");

    if sel == "@all" {
        return (2..=count + 1).collect();
    }
    if sel == "@first" { return vec![2]; }
    if sel == "@last" { return vec![count + 1]; }

    // Try as 0-based index
    if let Ok(idx) = sel.parse::<usize>() {
        if idx < count {
            return vec![idx + 2];
        }
    }

    Vec::new() // no match
}

fn spec_to_anim(spec: &AnimationSpec, sid: usize) -> Anim {
    let trigger = Trigger::from_str(&spec.trigger);
    let dur = if spec.duration_ms == 0 { 500 } else { spec.duration_ms };
    let delay = spec.delay_ms;

    match spec.effect.to_lowercase().as_str() {
        "fadein" | "fade" | "appear" => Anim::Fade { sid, dur, delay, trigger },
        "flyin" | "fly_in" | "fly-in" | "wipe" | "slide" => Anim::FlyIn {
            sid, dur, delay, trigger,
            dir: FlyDir::from_str(&spec.direction),
        },
        "zoom" | "zoomin" | "zoom_in" | "scale" | "grow" => Anim::Zoom {
            sid, dur, delay, trigger,
            from_scale: spec.zoom_scale.unwrap_or(0.0),
        },
        "bounce" => Anim::Zoom { sid, dur, delay, trigger, from_scale: 0.5 },
        "spin" | "rotate" => Anim::Emphasis {
            sid, dur, delay, trigger,
            style: EmphasisStyle::Spin,
        },
        "pulse" | "emphasize" => Anim::Emphasis {
            sid, dur, delay, trigger,
            style: EmphasisStyle::Pulse,
        },
        "fadeout" | "fade_out" | "exit" => Anim::ExitFade { sid, dur, delay, trigger },
        "flyout" | "fly_out" => Anim::ExitFlyOut {
            sid, dur, delay, trigger,
            dir: FlyDir::from_str(&spec.direction),
        },
        "toggle" | "set" => Anim::Set {
            sid, delay, trigger,
            property: "style.visibility".to_string(),
            value: if spec.effect.to_lowercase() == "hide" { "hidden".to_string() } else { "visible".to_string() },
        },
        _ => Anim::Fade { sid, dur, delay, trigger },
    }
}

// ===========================================================================
// Slide XML with animation
// ===========================================================================

fn build_slide_xml_with_anim(
    svg: &PptxParsedSvg,
    slide_w: i64,
    slide_h: i64,
    anims: &[Anim],
    transition: Option<&TransitionEffect>,
    trans_speed: &str,
) -> Result<String, ToolError> {
    let px_per_emu = EMU_PER_INCH as f64 / 96.0;
    let scale = px_per_emu;
    let off_x = -svg.vb_x * scale;
    let off_y = -svg.vb_y * scale;

    let mut shapes = String::new();
    for (idx, shape) in svg.shapes.iter().enumerate() {
        write_shape(
            &mut shapes, shape, scale, off_x, off_y, slide_w, slide_h, idx + 2,
        )?;
    }

    let mut out = String::new();
    out.push_str(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#);
    out.push_str("<p:sld xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" xmlns:p=\"http://schemas.openxmlformats.org/presentationml/2006/main\">");
    out.push_str("<p:cSld><p:spTree>");
    out.push_str("<p:nvGrpSpPr><p:cNvPr id=\"1\" name=\"\"/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr>");
    out.push_str("<p:grpSpPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"0\" cy=\"0\"/><a:chOff x=\"0\" y=\"0\"/><a:chExt cx=\"0\" cy=\"0\"/></a:xfrm></p:grpSpPr>");
    out.push_str(&shapes);
    out.push_str("</p:spTree></p:cSld>");

    if let Some(te) = transition {
        out.push_str(&build_transition_xml(te, trans_speed));
    }

    if !anims.is_empty() {
        out.push_str(&build_timing_xml(anims));
    }

    out.push_str("</p:sld>");
    Ok(out)
}

// ===========================================================================
// PPTX builder
// ===========================================================================

struct SlideWithAnim {
    source_path: String,
    svg: PptxParsedSvg,
    anims: Vec<Anim>,
    transition: Option<TransitionEffect>,
    trans_speed: String,
}

fn build_pptx_anim(slides: &[SlideWithAnim], title: Option<&str>) -> Result<Vec<u8>, ToolError> {
    let mut entries: Vec<(String, Vec<u8>)> = Vec::new();

    entries.push(("[Content_Types].xml".into(), build_content_types(slides.len()).into_bytes()));
    entries.push(("_rels/.rels".into(), build_root_rels().into_bytes()));
    entries.push(("ppt/_rels/presentation.xml.rels".into(), build_presentation_rels(slides.len()).into_bytes()));

    let (sw, sh) = slides.first().map(|s| {
        let px = EMU_PER_INCH as f64 / 96.0;
        ((s.svg.vb_w * px).round() as i64, (s.svg.vb_h * px).round() as i64)
    }).unwrap_or((SLIDE_W_EMU, SLIDE_H_EMU));

    entries.push(("ppt/presentation.xml".into(), build_presentation_xml(slides.len(), sw, sh).into_bytes()));
    entries.push(("ppt/theme/theme1.xml".into(), THEME_XML.as_bytes().to_vec()));

    for i in 1..=slides.len() {
        entries.push((format!("ppt/slides/_rels/slide{i}.xml.rels"), build_slide_rels().into_bytes()));
    }

    for (i, slide) in slides.iter().enumerate() {
        let xml = build_slide_xml_with_anim(&slide.svg, sw, sh, &slide.anims, slide.transition.as_ref(), &slide.trans_speed)?;
        entries.push((format!("ppt/slides/slide{}.xml", i + 1), xml.into_bytes()));
    }

    entries.push(("ppt/slideMasters/slideMaster1.xml".into(), SLIDE_MASTER_XML.as_bytes().to_vec()));
    entries.push(("ppt/slideMasters/_rels/slideMaster1.xml.rels".into(), SLIDE_MASTER_RELS.as_bytes().to_vec()));
    entries.push(("ppt/slideLayouts/slideLayout1.xml".into(), SLIDE_LAYOUT_XML.as_bytes().to_vec()));
    entries.push(("ppt/slideLayouts/_rels/slideLayout1.xml.rels".into(), SLIDE_LAYOUT_RELS.as_bytes().to_vec()));
    entries.push(("docProps/core.xml".into(), build_core_props_xml(title.unwrap_or("Animated Presentation")).into_bytes()));
    entries.push(("docProps/app.xml".into(), APP_XML.as_bytes().to_vec()));

    let mut buf = Vec::new();
    let mut zip = zip::ZipWriter::new(Cursor::new(&mut buf));
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .unix_permissions(0o644);
    for (name, data) in &entries {
        zip.start_file(name.as_str(), opts)
            .map_err(|e| ToolError::ExecutionError(format!("zip start_file: {e}")))?;
        zip.write_all(data)
            .map_err(|e| ToolError::ExecutionError(format!("zip write: {e}")))?;
    }
    zip.finish().map_err(|e| ToolError::ExecutionError(format!("zip finish: {e}")))?;
    Ok(buf)
}

// ===========================================================================
// Tool implementations
// ===========================================================================

pub struct CreatePptxAnimationTool;

impl CreatePptxAnimationTool {
    pub fn new() -> Self { Self }

    pub fn definition(&self) -> ToolDefinition {
        ToolDefinition::new_with_label(
            "create_pptx_animation",
            "生成带动画的 PPT",
            "Pack `.svg` files into a `.pptx` with full animation support. Each SVG becomes one slide. \
             SVG `<animate>` tags are auto-converted. Use `slide_animations` for declarative per-slide \
             animation specs. Supports: entrance (fadeIn, flyIn, zoom), emphasis (pulse, spin), \
             exit (fadeOut, flyOut), and 20+ slide transitions (fade, push, wipe, cover, zoom, etc.).",
            ToolParameters::new(
                vec!["svg_paths", "output_path"],
                vec![
                    ("svg_paths", "array", Some("JSON array of absolute paths to `.svg` files.")),
                    ("output_path", "string", Some("Absolute workspace path ending in `.pptx`.")),
                    ("title", "string", Some("Optional deck title.")),
                    ("slide_animations", "array", Some("Per-slide animation specs: [{ slide_index, animations: [{ shape, effect, duration_ms, delay_ms, trigger, direction, zoom_scale }], transition, transition_speed }]")),
                    ("transition", "object", Some("Default slide transition: { transition_type, direction, color, speed }")),
                    ("transition_speed", "string", Some("\"slow\", \"med\" (default), or \"fast\".")),
                ],
            ),
        )
    }

    pub async fn execute(
        &self,
        arguments: Value,
        workspace: Option<String>,
    ) -> Result<CreatePptxAnimationOutcome, ToolError> {
        let args: CreatePptxAnimationArgs = serde_json::from_value(arguments)
            .map_err(|e| ToolError::InvalidArguments("create_pptx_animation".into(), e.to_string()))?;

        let out_path = PathBuf::from(&args.output_path);
        if out_path.extension().and_then(|e| e.to_str()).unwrap_or("") != "pptx" {
            return Err(ToolError::InvalidArguments("create_pptx_animation".into(), "output_path must end with .pptx".into()));
        }
        validate_workspace_path(&args.output_path, &workspace)?;

        if args.svg_paths.is_empty() {
            return Err(ToolError::InvalidArguments("create_pptx_animation".into(), "svg_paths is empty".into()));
        }

        let mut slide_inputs = Vec::new();
        let mut total_anim = 0usize;

        for (i, path) in args.svg_paths.iter().enumerate() {
            let content = tokio::fs::read_to_string(path).await
                .map_err(|e| ToolError::IoError(format!("Read {}: {e}", path)))?;

            let svg = parse_svg(&content)
                .map_err(|e| ToolError::ExecutionError(format!("SVG parse error in {path}: {e}")))?;

            let shape_count = svg.shapes.len();

            // Animations from SVG <animate> tags
            let mut anims: Vec<Anim> = Vec::new();
            for sa in parse_svg_animations(&content) {
                if let Some(a) = svg_anim_to_ooxml(&sa, 2) {
                    anims.push(a);
                    total_anim += 1;
                }
            }

            // Animations from declarative spec
            if let Some(ref specs) = args.slide_animations {
                if let Some(slide_spec) = specs.iter().find(|s| s.slide_index == i) {
                    for spec in &slide_spec.animations {
                        for sid in resolve_shape_ids(spec, shape_count) {
                            anims.push(spec_to_anim(spec, sid));
                            total_anim += 1;
                        }
                    }
                }
            }

            let transition = args.slide_animations
                .as_ref()
                .and_then(|ss| ss.iter().find(|s| s.slide_index == i))
                .and_then(|s| s.transition.clone())
                .or_else(|| args.transition.clone());

            let trans_speed = args.slide_animations
                .as_ref()
                .and_then(|ss| ss.iter().find(|s| s.slide_index == i))
                .and_then(|s| s.transition_speed.clone())
                .unwrap_or_else(|| args.transition_speed.clone());

            slide_inputs.push(SlideWithAnim {
                source_path: path.clone(),
                svg,
                anims,
                transition,
                trans_speed,
            });
        }

        let bytes = build_pptx_anim(&slide_inputs, args.title.as_deref())?;

        if let Some(parent) = out_path.parent() {
            tokio::fs::create_dir_all(parent).await
                .map_err(|e| ToolError::IoError(format!("mkdir: {e}")))?;
        }
        tokio::fs::write(&out_path, &bytes).await
            .map_err(|e| ToolError::IoError(format!("write: {e}")))?;

        Ok(CreatePptxAnimationOutcome {
            output: format!("Created animated PPTX: {} slides, {} animations, {} bytes",
                slide_inputs.len(), total_anim, bytes.len()),
            file_path: args.output_path,
            byte_size: bytes.len(),
            slide_count: slide_inputs.len(),
            animation_count: total_anim,
            is_error: false,
        })
    }
}

// ===========================================================================
// AddAnimationTool
// ===========================================================================

pub struct AddAnimationTool;

impl AddAnimationTool {
    pub fn new() -> Self { Self }

    pub fn definition(&self) -> ToolDefinition {
        ToolDefinition::new_with_label(
            "add_pptx_animation",
            "给PPT添加动画",
            "Add animations to an existing `.pptx`. Reads the PPTX, injects `<p:timing>` and \
             `<p:transition>` elements into specified slides, and writes the result. Supports \
             entrance (fadeIn, flyIn, zoom), emphasis (pulse, spin), exit (fadeOut, flyOut), \
             and transitions (fade, push, wipe, cover, zoom, etc.).",
            ToolParameters::new(
                vec!["input_pptx", "output_pptx", "slides"],
                vec![
                    ("input_pptx", "string", Some("Absolute path to the existing `.pptx` file.")),
                    ("output_pptx", "string", Some("Absolute path for the output `.pptx`.")),
                    ("slides", "array", Some("Per-slide animation specs: [{ slide_index, animations: [{ shape, effect, duration_ms, delay_ms, trigger, direction }], transition, transition_speed }]")),
                    ("transition", "object", Some("Default transition for all slides.")),
                    ("transition_speed", "string", Some("\"slow\", \"med\" (default), or \"fast\".")),
                ],
            ),
        )
    }

    pub async fn execute(&self, arguments: Value, workspace: Option<String>) -> Result<String, ToolError> {
        let args: AddAnimationArgs = serde_json::from_value(arguments)
            .map_err(|e| ToolError::InvalidArguments("add_pptx_animation".into(), e.to_string()))?;

        validate_workspace_path(&args.input_pptx, &workspace)?;
        validate_workspace_path(&args.output_pptx, &workspace)?;

        let out_path = PathBuf::from(&args.output_pptx);
        if out_path.extension().and_then(|e| e.to_str()).unwrap_or("") != "pptx" {
            return Err(ToolError::InvalidArguments("add_pptx_animation".into(), "output_pptx must end with .pptx".into()));
        }

        let input_bytes = tokio::fs::read(&args.input_pptx).await
            .map_err(|e| ToolError::IoError(format!("Read: {e}")))?;

        let mut input_zip = zip::ZipArchive::new(Cursor::new(&input_bytes))
            .map_err(|e| ToolError::ExecutionError(format!("Invalid PPTX: {e}")))?;

        let total_slides = input_zip.len();
        let mut output_entries: Vec<(String, Vec<u8>)> = Vec::new();
        let mut anim_count = 0usize;

        for i in 0..input_zip.len() {
            let mut entry = input_zip.by_index(i)
                .map_err(|e| ToolError::ExecutionError(format!("Read entry {i}: {e}")))?;
            let name = entry.name().to_string();
            let mut data = Vec::new();
            entry.read_to_end(&mut data)
                .map_err(|e| ToolError::IoError(format!("Read {name}: {e}")))?;

            let is_slide = name.starts_with("ppt/slides/slide")
                && name.ends_with(".xml")
                && !name.contains("_rels");

            if is_slide {
                let slide_idx = extract_slide_index(&name);
                let slide_spec = args.slides.iter().find(|s| s.slide_index == slide_idx);

                let trans_spec = slide_spec.and_then(|s| s.transition.as_ref())
                    .or(args.transition.as_ref());
                let trans_speed = slide_spec
                    .and_then(|s| s.transition_speed.clone())
                    .unwrap_or_else(|| args.transition_speed.clone());

                // Count shapes
                let shape_count = count_shapes(&data).max(1);

                let mut anims: Vec<Anim> = Vec::new();
                if let Some(spec) = slide_spec {
                    for a_spec in &spec.animations {
                        for sid in resolve_shape_ids(a_spec, shape_count) {
                            anims.push(spec_to_anim(a_spec, sid));
                            anim_count += 1;
                        }
                    }
                }

                if !anims.is_empty() || trans_spec.is_some() {
                    data = inject_animations(&data, &anims, trans_spec, &trans_speed)?;
                }
            }

            output_entries.push((name, data));
        }

        // Write output
        let mut buf = Vec::new();
        {
            let mut zip = zip::ZipWriter::new(Cursor::new(&mut buf));
            let opts = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated)
                .unix_permissions(0o644);
            for (name, data) in &output_entries {
                zip.start_file(name.as_str(), opts)
                    .map_err(|e| ToolError::ExecutionError(format!("zip: {e}")))?;
                zip.write_all(data)
                    .map_err(|e| ToolError::ExecutionError(format!("zip: {e}")))?;
            }
            zip.finish().map_err(|e| ToolError::ExecutionError(format!("zip: {e}")))?;
        }

        if let Some(parent) = out_path.parent() {
            tokio::fs::create_dir_all(parent).await
                .map_err(|e| ToolError::IoError(format!("mkdir: {e}")))?;
        }
        tokio::fs::write(&out_path, &buf).await
            .map_err(|e| ToolError::IoError(format!("write: {e}")))?;

        Ok(json!({
            "output": format!("Added {} animations to {} slides. Output: {}", anim_count, args.slides.len(), args.output_pptx),
            "file_path": args.output_pptx,
            "byte_size": buf.len(),
            "animation_count": anim_count,
            "slide_count": total_slides,
            "is_error": false
        }).to_string())
    }
}

fn extract_slide_index(name: &str) -> usize {
    let re = Regex::new(r"slide(\d+)\.xml$").unwrap();
    re.captures(name)
        .and_then(|c| c.get(1))
        .and_then(|m| m.as_str().parse().ok())
        .unwrap_or(0)
}

/// Count <p:sp> and <p:cxnSp> elements in raw slide XML bytes.
fn count_shapes(data: &[u8]) -> usize {
    let s = String::from_utf8_lossy(data);
    s.matches("<p:sp>").count() + s.matches("<p:cxnSp>").count()
}

/// Inject `<p:timing>` and `<p:transition>` into a raw slide XML byte vector.
fn inject_animations(
    data: &[u8],
    anims: &[Anim],
    transition: Option<&TransitionEffect>,
    trans_speed: &str,
) -> Result<Vec<u8>, ToolError> {
    let s = String::from_utf8_lossy(data).into_owned();

    // Remove existing timing/transition
    let re_timing = Regex::new(r"<p:timing>.*?</p:timing>").unwrap();
    let re_trans = Regex::new(r"<p:transition[^>]*>.*?</p:transition>").unwrap();
    let s = re_timing.replace_all(&s, "");
    let s = re_trans.replace_all(&s, "");

    let trans_xml = transition.map(|t| build_transition_xml(t, trans_speed)).unwrap_or_default();
    let timing_xml = if !anims.is_empty() { build_timing_xml(anims) } else { String::new() };

    if let Some(pos) = s.find("</p:sld>") {
        let replacement = format!("{}{}{}</p:sld>", trans_xml, timing_xml, "</p:sld>");
        let mut result = s[..pos].to_string();
        result.push_str(&replacement);
        Ok(result.into_bytes())
    } else {
        Ok(s.into_owned().into_bytes())
    }
}

