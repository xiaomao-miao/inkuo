//! SVG parser for the PPTX tool.
//!
//! Converts an SVG source string into a `ParsedSvg` value:
//!   - canonicalises every shape to the SVG user-space coordinates of its
//!     `<svg viewBox="…">`,
//!   - collapses gradients (`fill="url(#…)"`) to their first stop colour,
//!     so the OOXML writer stays trivial,
//!   - preserves `<text>` runs verbatim (multi-run text edits per-run in
//!     PPT).
//!
//! Used to live inside `pptx/mod.rs`; pulled out because the parser
//! needs no awareness of the OOXML writing path, and keeping it isolated
//! makes the ~1300 lines of SVG rules easy to skim independently of the
//! PPTX writer.

use std::collections::BTreeMap;

use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;

/// Return a reference to the innermost open `<g transform="…">` context,
/// or identity if the stack is empty.
///
/// Used in place of `transforms.last().unwrap()` everywhere in the SVG
/// walk: a malformed `<g>` close can underflow the stack and a stray
/// shape outside any group used to panic the parser. Treating "no open
/// group" as identity is a safe fallback — every shape builder applies
/// its own group transform on top, so the resulting OOXML is still
/// well-formed.
#[inline]
fn current_transform<'a>(transforms: &'a [Transform]) -> &'a Transform {
    static IDENTITY_FALLBACK: std::sync::OnceLock<Transform> = std::sync::OnceLock::new();
    transforms
        .last()
        .unwrap_or_else(|| IDENTITY_FALLBACK.get_or_init(Transform::identity))
}

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
    pub(crate) skipped: Vec<String>,
    /// Gradient lookup table: id → first `<stop>` colour/opacity. Populated
    /// while parsing the `<defs>` block at the top of the SVG. Shapes may
    /// reference these via `fill="url(#id)"`; the parser resolves the
    /// reference to a solid colour so the OOXML writer stays trivial.
    pub(crate) defs: BTreeMap<String, GradientStop>,
}

/// A single SVG shape, normalised into a representation we can convert to
/// OOXML. The shape coordinates are still in SVG user units — the
/// `to_ooxml` step applies the per-slide scale + offset transform.
#[derive(Debug, Clone)]
pub enum SvgShape {
    Rect {
        x: f64,
        y: f64,
        width: f64,
        height: f64,
        rx: Option<f64>,
        ry: Option<f64>,
        fill: Option<Paint>,
        stroke: Option<Paint>,
        stroke_width: Option<f64>,
        opacity: Option<f64>,
    },
    Ellipse {
        cx: f64,
        cy: f64,
        rx: f64,
        ry: f64,
        fill: Option<Paint>,
        stroke: Option<Paint>,
        stroke_width: Option<f64>,
        opacity: Option<f64>,
    },
    Line {
        x1: f64,
        y1: f64,
        x2: f64,
        y2: f64,
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
        x: f64,
        y: f64,
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
    pub(crate) text: String,
    pub(crate) bold: bool,
    pub(crate) italic: bool,
    pub(crate) underline: bool,
    pub(crate) fill: Option<Paint>,
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
    Color {
        rgb: String,
        opacity: Option<f64>,
    },
    /// `url(#id)` resolved to the first stop of the matching gradient
    /// inside this slide's `<defs>`. We carry the resolved colour so the
    /// OOXML writer doesn't need to thread the gradient map through.
    GradientRef {
        rgb: String,
        opacity: Option<f64>,
    },
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
                            let parts: Vec<&str> = vb
                                .split(|c: char| c.is_whitespace() || c == ',')
                                .filter(|s| !s.is_empty())
                                .collect();
                            if parts.len() == 4 {
                                parsed.vb_x = parts[0].parse().unwrap_or(0.0);
                                parsed.vb_y = parts[1].parse().unwrap_or(0.0);
                                parsed.vb_w = parts[2].parse().unwrap_or(100.0);
                                parsed.vb_h = parts[3].parse().unwrap_or(100.0);
                            }
                        } else if let (Some(w), Some(h)) = (attrs.get("width"), attrs.get("height"))
                        {
                            // Best-effort fallback when viewBox is missing.
                            parsed.vb_w = w.parse().unwrap_or(100.0);
                            parsed.vb_h = h.parse().unwrap_or(100.0);
                        }
                        if parsed.vb_w == 0.0 {
                            parsed.vb_w = 100.0;
                        }
                        if parsed.vb_h == 0.0 {
                            parsed.vb_h = 100.0;
                        }
                    }
                    "g" => {
                        if let Some(t) = attrs.get("transform") {
                            transforms.push(current_transform(&transforms).compose(t));
                        } else {
                            transforms.push(*current_transform(&transforms));
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
                        if let Some(shape) =
                            build_rect(&attrs, current_transform(&transforms), &parsed.defs)
                        {
                            parsed.shapes.push(shape);
                        }
                    }
                    "circle" => {
                        if let Some(shape) =
                            build_circle(&attrs, current_transform(&transforms), &parsed.defs)
                        {
                            parsed.shapes.push(shape);
                        }
                    }
                    "ellipse" => {
                        if let Some(shape) =
                            build_ellipse(&attrs, current_transform(&transforms), &parsed.defs)
                        {
                            parsed.shapes.push(shape);
                        }
                    }
                    "line" => {
                        if let Some(shape) =
                            build_line(&attrs, current_transform(&transforms), &parsed.defs)
                        {
                            parsed.shapes.push(shape);
                        }
                    }
                    "path" => {
                        if let Some(shape) =
                            build_path(&attrs, current_transform(&transforms), &parsed.defs)
                        {
                            parsed.shapes.push(shape);
                        }
                    }
                    "polyline" | "polygon" => {
                        if let Some(shape) =
                            build_poly(&tag, &attrs, current_transform(&transforms), &parsed.defs)
                        {
                            parsed.shapes.push(shape);
                        }
                    }
                    "text" => {
                        if text_acc.is_none() {
                            text_acc = Some(TextAcc {
                                x: attrs.get("x").and_then(|v| v.parse().ok()).unwrap_or(0.0),
                                y: attrs.get("y").and_then(|v| v.parse().ok()).unwrap_or(0.0),
                                font_size: attrs.get("font-size").and_then(|v| parse_len(v)),
                                fill: attrs
                                    .get("fill")
                                    .and_then(|v| parse_paint(v, &attrs, &parsed.defs)),
                                opacity: attrs.get("opacity").and_then(|v| v.parse().ok()),
                                transform: *current_transform(&transforms),
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
                    "use" | "foreignObject" | "filter" | "mask" | "clipPath" | "pattern"
                    | "switch" => {
                        if !parsed.skipped.contains(&tag) {
                            parsed.skipped.push(tag);
                        }
                    }
                    "image" => {
                        // Try to parse inline data: URL; skip only if it fails.
                        if let Some(href) = attrs
                            .get("href")
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
                        if let Some(shape) =
                            build_rect(&attrs, current_transform(&transforms), &parsed.defs)
                        {
                            parsed.shapes.push(shape);
                        }
                    }
                    "circle" => {
                        if let Some(shape) =
                            build_circle(&attrs, current_transform(&transforms), &parsed.defs)
                        {
                            parsed.shapes.push(shape);
                        }
                    }
                    "ellipse" => {
                        if let Some(shape) =
                            build_ellipse(&attrs, current_transform(&transforms), &parsed.defs)
                        {
                            parsed.shapes.push(shape);
                        }
                    }
                    "line" => {
                        if let Some(shape) =
                            build_line(&attrs, current_transform(&transforms), &parsed.defs)
                        {
                            parsed.shapes.push(shape);
                        }
                    }
                    "path" => {
                        if let Some(shape) =
                            build_path(&attrs, current_transform(&transforms), &parsed.defs)
                        {
                            parsed.shapes.push(shape);
                        }
                    }
                    "polyline" | "polygon" => {
                        if let Some(shape) =
                            build_poly(&tag, &attrs, current_transform(&transforms), &parsed.defs)
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
                    "use" | "foreignObject" | "filter" | "mask" | "clipPath" | "pattern"
                    | "switch" => {
                        if !parsed.skipped.contains(&tag) {
                            parsed.skipped.push(tag);
                        }
                    }
                    "image" => {
                        if let Some(href) = attrs
                            .get("href")
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
            Err(e) => {
                return Err(format!(
                    "quick-xml error at position {}: {}",
                    reader.buffer_position(),
                    e
                ))
            }
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
        let val = attr
            .unescape_value()
            .map(|v| v.into_owned())
            .unwrap_or_default();
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
                Some((
                    format!("{}{}{}{}{}{}", r, r, g, g, b, b).to_uppercase(),
                    None,
                ))
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
    let opacity = a
        .get("opacity")
        .and_then(|v| v.parse().ok())
        .or_else(|| a.get("fill-opacity").and_then(|v| v.parse().ok()));
    Some(SvgShape::Rect {
        x,
        y,
        width: w,
        height: h,
        rx,
        ry,
        fill,
        stroke,
        stroke_width,
        opacity,
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
    if r <= 0.0 {
        return None;
    }
    let (cx, cy) = t.apply_point(cx, cy);
    let r = (r * t.uniform_scale()).max(0.0);
    let fill = a.get("fill").and_then(|v| parse_paint(v, a, defs));
    let stroke = a.get("stroke").and_then(|v| parse_paint(v, a, defs));
    let stroke_width = a.get("stroke-width").and_then(|v| v.parse().ok());
    let opacity = a
        .get("opacity")
        .and_then(|v| v.parse().ok())
        .or_else(|| a.get("fill-opacity").and_then(|v| v.parse().ok()));
    Some(SvgShape::Ellipse {
        cx,
        cy,
        rx: r,
        ry: r,
        fill,
        stroke,
        stroke_width,
        opacity,
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
    if rx <= 0.0 || ry <= 0.0 {
        return None;
    }
    let (cx, cy) = t.apply_point(cx, cy);
    let scale = t.uniform_scale();
    let rx = (rx * scale).max(0.0);
    let ry = (ry * scale).max(0.0);
    let fill = a.get("fill").and_then(|v| parse_paint(v, a, defs));
    let stroke = a.get("stroke").and_then(|v| parse_paint(v, a, defs));
    let stroke_width = a.get("stroke-width").and_then(|v| v.parse().ok());
    let opacity = a
        .get("opacity")
        .and_then(|v| v.parse().ok())
        .or_else(|| a.get("fill-opacity").and_then(|v| v.parse().ok()));
    Some(SvgShape::Ellipse {
        cx,
        cy,
        rx,
        ry,
        fill,
        stroke,
        stroke_width,
        opacity,
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
    let stroke = a
        .get("stroke")
        .and_then(|v| parse_paint(v, a, defs))
        .or_else(|| {
            Some(Paint::Color {
                rgb: "000000".into(),
                opacity: None,
            })
        });
    let stroke_width = a
        .get("stroke-width")
        .and_then(|v| v.parse().ok())
        .or(Some(1.0));
    let opacity = a.get("opacity").and_then(|v| v.parse().ok());
    Some(SvgShape::Line {
        x1,
        y1,
        x2,
        y2,
        stroke,
        stroke_width,
        opacity,
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
    let opacity = a
        .get("opacity")
        .and_then(|v| v.parse().ok())
        .or_else(|| a.get("fill-opacity").and_then(|v| v.parse().ok()));
    // Pre-bake the active transform into the path by re-emitting each
    // command with the parent group's translation/scale applied. This
    // lets the OOXML writer use the path as-is (it gets stored in a
    // fixed `w=100000 h=100000` viewport). See the OOXML `<a:custGeom>`
    // writer for how that viewport is chosen.
    let d = apply_transform_to_path(&d, t);
    Some(SvgShape::Path {
        d,
        fill,
        stroke,
        stroke_width,
        opacity,
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
        if token.is_empty() {
            continue;
        }
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
    let fill = a
        .get("fill")
        .and_then(|v| parse_paint(v, a, defs))
        .or_else(|| {
            Some(Paint::Color {
                rgb: "000000".into(),
                opacity: None,
            })
        });
    let stroke = a.get("stroke").and_then(|v| parse_paint(v, a, defs));
    let stroke_width = a.get("stroke-width").and_then(|v| v.parse().ok());
    let opacity = a.get("opacity").and_then(|v| v.parse().ok());
    Some(SvgShape::Path {
        d,
        fill,
        stroke,
        stroke_width,
        opacity,
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
        x,
        y,
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
    if s.is_empty() {
        "0".to_string()
    } else {
        s
    }
}

/// Decode base64 from a slice of ASCII bytes. Returns `None` on invalid input.
pub(crate) fn base64_decode(input: &[u8]) -> Option<Vec<u8>> {
    const DECODE_TABLE: [i8; 256] = {
        let mut t = [-1i8; 256];
        // A-Z
        t[b'A' as usize] = 0;
        t[b'B' as usize] = 1;
        t[b'C' as usize] = 2;
        t[b'D' as usize] = 3;
        t[b'E' as usize] = 4;
        t[b'F' as usize] = 5;
        t[b'G' as usize] = 6;
        t[b'H' as usize] = 7;
        t[b'I' as usize] = 8;
        t[b'J' as usize] = 9;
        t[b'K' as usize] = 10;
        t[b'L' as usize] = 11;
        t[b'M' as usize] = 12;
        t[b'N' as usize] = 13;
        t[b'O' as usize] = 14;
        t[b'P' as usize] = 15;
        t[b'Q' as usize] = 16;
        t[b'R' as usize] = 17;
        t[b'S' as usize] = 18;
        t[b'T' as usize] = 19;
        t[b'U' as usize] = 20;
        t[b'V' as usize] = 21;
        t[b'W' as usize] = 22;
        t[b'X' as usize] = 23;
        t[b'Y' as usize] = 24;
        t[b'Z' as usize] = 25;
        // a-z
        t[b'a' as usize] = 26;
        t[b'b' as usize] = 27;
        t[b'c' as usize] = 28;
        t[b'd' as usize] = 29;
        t[b'e' as usize] = 30;
        t[b'f' as usize] = 31;
        t[b'g' as usize] = 32;
        t[b'h' as usize] = 33;
        t[b'i' as usize] = 34;
        t[b'j' as usize] = 35;
        t[b'k' as usize] = 36;
        t[b'l' as usize] = 37;
        t[b'm' as usize] = 38;
        t[b'n' as usize] = 39;
        t[b'o' as usize] = 40;
        t[b'p' as usize] = 41;
        t[b'q' as usize] = 42;
        t[b'r' as usize] = 43;
        t[b's' as usize] = 44;
        t[b't' as usize] = 45;
        t[b'u' as usize] = 46;
        t[b'v' as usize] = 47;
        t[b'w' as usize] = 48;
        t[b'x' as usize] = 49;
        t[b'y' as usize] = 50;
        t[b'z' as usize] = 51;
        // 0-9
        t[b'0' as usize] = 52;
        t[b'1' as usize] = 53;
        t[b'2' as usize] = 54;
        t[b'3' as usize] = 55;
        t[b'4' as usize] = 56;
        t[b'5' as usize] = 57;
        t[b'6' as usize] = 58;
        t[b'7' as usize] = 59;
        t[b'8' as usize] = 60;
        t[b'9' as usize] = 61;
        t[b'+' as usize] = 62;
        t[b'/' as usize] = 63;
        t[b'=' as usize] = 64;
        t
    };

    let input = input.trim_ascii();
    if input.is_empty() {
        return Some(Vec::new());
    }

    // Pad to multiple of 4
    let padding = (4 - (input.len() % 4)) % 4;
    let len = input.len() + padding;
    let mut buf = Vec::with_capacity(len * 3 / 4);

    let mut i = 0;
    while i < len {
        let get = |idx: usize| -> i8 {
            if idx >= input.len() {
                return -1;
            }
            DECODE_TABLE[input[idx] as usize]
        };
        let a = get(i);
        let b = get(i + 1);
        let c = get(i + 2);
        let d = get(i + 3);
        if a < 0 || b < 0 {
            return None;
        }
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
pub(crate) fn base64_encode(input: &[u8]) -> String {
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
    fn identity() -> Self {
        Self {
            tx: 0.0,
            ty: 0.0,
            scale: 1.0,
        }
    }

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
                    if let Ok(x) = parts[0].parse::<f64>() {
                        out.tx += x * out.scale;
                    }
                }
                if parts.len() >= 2 {
                    if let Ok(y) = parts[1].parse::<f64>() {
                        out.ty += y * out.scale;
                    }
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
pub(crate) struct SlideImage {
    pub(crate) shape_id: usize,
    pub(crate) ext: String,
    pub(crate) data: Vec<u8>,
}
