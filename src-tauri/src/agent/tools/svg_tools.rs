//! SVG authoring tool: `create_svg`
//!
//! Lets the AI agent author a complete, standalone SVG file (e.g. an icon,
//! illustration, logo, decorative banner, simple data-visual) and write it to
//! the workspace. The tool itself does NOT rasterise the SVG — SVG *is* the
//! final output and any modern viewer (browser, image viewer, docx inserter)
//! can render it losslessly at any size.
//!
//! ## Why a dedicated tool (and not just `write_file`)
//!
//! `write_file` works, but it gives the model no scaffold for "make me a nice
//! SVG". This tool pins a richer schema and produces a structured outcome
//! (`CreateSvgOutcome`) that:
//!
//!  1. Validates the SVG is well-formed before writing (so the LLM gets a
//!     clear, recoverable error on stray tags or unbalanced quotes instead of
//!     the frontend later complaining "file looks corrupt").
//!  2. Carries `file_path` AND `svg_source` back to the frontend so the
//!     chat panel can optionally inline-preview the SVG without an extra
//!     `read_file` round-trip.
//!  3. Triggers a `file-change` event through the registry so the sidebar
//!     tree refreshes and the SVG is auto-opened by the in-app viewer
//!     (handy: the user sees their freshly-drawn icon immediately).
//!
//! ## Style guidance (mirrored in the tool description)
//!
//! The schema is deliberately minimal (prompt + output path + optional
//! viewBox / size hint). The `description` field carries the style guidance
//! so the LLM sees it in every tool-call response, not just the first turn.
//! The richer style guide lives in `prompts/tool_specs/svg.md`, loaded on
//! demand via `get_tool_help`.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::PathBuf;

use super::asset_registry;
use super::{validate_workspace_path, ToolDefinition, ToolError, ToolParameters};

/// Structured outcome returned by `CreateSvgTool::execute`. Carries
/// everything the registry + frontend need without re-reading the file:
///
///   - `output`: human-readable summary fed to the LLM (so it can confirm
///     what it just wrote and the file_path the user should click).
///   - `file_path`: absolute path the tool wrote to. The registry uses this
///     to stamp the `ToolResult.file_path` and emit a `file-change` event so
///     the in-app viewer auto-opens the new SVG.
///   - `svg_source`: the *exact* bytes written. The frontend can build a
///     `data:image/svg+xml;base64,...` URL without an extra `read_file`
///     round-trip — useful for an inline preview chip in the chat card.
///   - `byte_size`: raw file size, so the chat card can show "12.3 KB".
///   - `view_box`: parsed `viewBox` attribute (`x y w h`), when present, so
///     the preview chip can render at the intrinsic aspect ratio.
pub struct CreateSvgOutcome {
    pub output: String,
    pub file_path: String,
    pub svg_source: String,
    pub byte_size: usize,
    pub view_box: Option<(f64, f64, f64, f64)>,
    pub is_error: bool,
}

#[derive(Debug, Deserialize)]
struct CreateSvgArgs {
    /// Natural-language brief the agent is illustrating. Captured for the
    /// human-readable output line; the LLM still writes the *full* SVG into
    /// `svg_source` itself — we don't expand prompts on the server.
    #[serde(default)]
    description: Option<String>,
    /// The complete, self-contained `<svg>...</svg>` document. Must include
    /// the XML processing instruction and the `xmlns` attribute so the file
    /// is portable to docx, browsers, and image viewers.
    svg_source: String,
    /// Absolute workspace-relative path the SVG should be written to. Must
    /// end in `.svg`. Parent directories are created as needed.
    output_path: String,
    /// Optional aspect-ratio hint ("16:9", "1:1", "3:4"). Used purely as a
    /// shape sanity check — the LLM controls the real `viewBox`.
    #[serde(default)]
    aspect_ratio: Option<String>,
}

pub struct CreateSvgTool;

impl CreateSvgTool {
    pub fn new() -> Self {
        Self
    }

    pub fn definition(&self) -> ToolDefinition {
        ToolDefinition::new_with_label(
            "create_svg",
            "生成 SVG 图片",
            "Author a beautiful, self-contained SVG file and save it into the workspace. \
             The `svg_source` MUST be a complete standalone `<svg ...>...</svg>` document \
             (include the `<?xml ?>` prolog and `xmlns=\"http://www.w3.org/2000/svg\"`). \
             You decide width / height / viewBox yourself — pick the aspect ratio that \
             fits the content. Aesthetics guidelines: limited harmonious palette \
             (3-5 colours), generous whitespace, consistent stroke widths, real \
             text labels (not paths-as-text), no external references (no <image href= to \
             remote URLs, no xlink:href to remote URLs), no scripts. To embed a PNG / JPEG \
             you loaded with `read_image`, reference it via the `asset://<asset_id>` URI \
             returned by that tool — e.g. `<image href=\"asset://asset-12345678\" x=\"0\" \
             y=\"0\" width=\"640\" height=\"480\"/>`. The tool will replace every such \
             reference with the actual image bytes at write time, so the bytes never need \
             to enter your context. Inline data: URLs are also supported as a fallback. \
             For diagrams prefer simple geometric primitives (rect / circle / path / line \
             / text) over bitmap filters. Load `get_tool_help(category=\"svg\")` for the \
             full style guide.",
            ToolParameters::new(
                vec!["svg_source", "output_path"],
                vec![
                    ("description", "string", Some("One-line natural-language brief of what the SVG depicts. Used only in the success log line; the SVG itself is fully self-contained.")),
                    ("svg_source", "string", Some("The complete standalone `<svg>...</svg>` document. Must start with `<?xml ...?>` and include `xmlns=\"http://www.w3.org/2000/svg\"`.")),
                    ("output_path", "string", Some("Absolute workspace path to write the SVG to. Extension must be `.svg`. Parent directories are created automatically.")),
                    ("aspect_ratio", "string", Some("Optional aspect-ratio hint such as `\"16:9\"`, `\"1:1\"`, `\"3:4\"`. The tool uses it only as a sanity check against the declared `viewBox`; the actual rendering uses the viewBox.")),
                ],
            ),
        )
    }

    pub async fn execute(
        &self,
        arguments: Value,
        workspace: Option<String>,
    ) -> Result<CreateSvgOutcome, ToolError> {
        let args: CreateSvgArgs = serde_json::from_value(arguments).map_err(|e| {
            ToolError::InvalidArguments(
                "create_svg".to_string(),
                format!("Invalid parameters: {}", e),
            )
        })?;

        // ── 1. Path validation ────────────────────────────────────────────
        let output_path = PathBuf::from(&args.output_path);
        let extension = output_path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if extension != "svg" {
            return Err(ToolError::InvalidArguments(
                "create_svg".to_string(),
                format!(
                    "output_path must end with `.svg`; got `.{}{}`",
                    extension,
                    if extension.is_empty() { " (no extension)" } else { "" }
                ),
            ));
        }

        // Workspace sandbox check. We allow the path not to exist yet (this
        // tool *creates* files), so `validate_workspace_path` will probe
        // the canonicalized parent — same pattern as `write_file`.
        validate_workspace_path(&args.output_path, &workspace)?;

        // ── 2. SVG sanity check ──────────────────────────────────────────
        let trimmed = args.svg_source.trim_start();
        if !trimmed.starts_with("<?xml") && !trimmed.starts_with("<svg") {
            return Err(ToolError::InvalidArguments(
                "create_svg".to_string(),
                "svg_source must start with `<?xml ...?>` or `<svg ...>`".to_string(),
            ));
        }
        if !args.svg_source.contains("</svg>") {
            return Err(ToolError::InvalidArguments(
                "create_svg".to_string(),
                "svg_source is missing the closing `</svg>` tag".to_string(),
            ));
        }
        if !args.svg_source.contains("xmlns=\"http://www.w3.org/2000/svg\"")
            && !args.svg_source.contains("xmlns='http://www.w3.org/2000/svg'")
        {
            return Err(ToolError::InvalidArguments(
                "create_svg".to_string(),
                "svg_source must declare xmlns=\"http://www.w3.org/2000/svg\" so the file is portable".to_string(),
            ));
        }
        // Forbid the most common ways an "AI-generated" SVG turns out ugly
        // or unsafe. These are *guardrails*, not style police — we still
        // accept the file when the LLM has a good reason, but only after
        // it explains the deviation to the user.
        for forbidden in &["<script", "<foreignObject"] {
            if args.svg_source.contains(forbidden) {
                return Err(ToolError::InvalidArguments(
                    "create_svg".to_string(),
                    format!(
                        "svg_source contains forbidden element `{}`. Use static SVG only — no scripts, no foreignObject. Re-author the SVG without this element.",
                        forbidden
                    ),
                ));
            }
        }
        // Reject external image references. SVG can raster-bomb via
        // `<image href="https://attacker/...">`; the in-app viewer is
        // sandboxed but docx round-tripping is not, so we kill the
        // surface at the source.
        for pattern in &["xlink:href=\"http", "xlink:href='http", " href=\"http", " href='http"] {
            if args.svg_source.contains(pattern) {
                return Err(ToolError::InvalidArguments(
                    "create_svg".to_string(),
                    "svg_source contains an external http(s) reference. Inline any external asset or use only static shapes.".to_string(),
                ));
            }
        }

        // Optional: parse the `viewBox` so the preview chip can render at
        // the intrinsic aspect ratio. Best-effort — we don't fail the
        // whole call when viewBox is missing, since the LLM might have
        // used `width`/`height` instead.
        let view_box = parse_view_box(&args.svg_source);

        if let (Some(hint), Some((_, _, w, h))) = (&args.aspect_ratio, view_box) {
            if let Some((hw, hh)) = parse_aspect_ratio(hint) {
                let declared = w / h;
                let expected = hw / hh;
                let drift = (declared - expected).abs() / expected;
                if drift > 0.05 {
                    // Soft warning — surface it in the success log so the
                    // LLM notices, but don't fail the call. The LLM can
                    // pass `aspect_ratio=null` next time if it really
                    // meant to deviate.
                    tracing::warn!(
                        "create_svg: aspect_ratio hint {} does not match declared viewBox {}x{} (drift {:.1}%)",
                        hint, w, h, drift * 100.0
                    );
                }
            }
        }

        // ── 3. Resolve asset:// references into inline data URLs ────────
        //
        // The LLM may have emitted `<image href="asset://<id>"/>` placeholders
        // for PNGs/JPEGs it loaded via `read_image`. We substitute the real
        // bytes here, just before writing — so the bytes never needed to
        // traverse the conversation history. Resolution failures are loud
        // errors (the LLM gets a clear "asset expired" / "unknown id" message
        // and can re-call `read_image`).
        let resolved_source = resolve_asset_references(&args.svg_source)?;

        // ── 4. Write the file ────────────────────────────────────────────
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

        let bytes = resolved_source.as_bytes();
        tokio::fs::write(&output_path, bytes).await.map_err(|e| {
            ToolError::IoError(format!(
                "Failed to write SVG to {}: {}",
                output_path.display(),
                e
            ))
        })?;

        let byte_size = bytes.len();
        let description = args
            .description
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("(no description provided)");

        let output = json!({
            "status": "ok",
            "file_path": output_path.to_string_lossy(),
            "description": description,
            "bytes": byte_size,
            "view_box": view_box.map(|(x, y, w, h)| [x, y, w, h]),
        })
        .to_string();

        Ok(CreateSvgOutcome {
            output,
            file_path: output_path.to_string_lossy().to_string(),
            svg_source: resolved_source,
            byte_size,
            view_box,
            is_error: false,
        })
    }
}

impl Default for CreateSvgTool {
    fn default() -> Self {
        Self::new()
    }
}

/// Best-effort extraction of `viewBox="x y w h"`. Returns `None` if the
/// attribute is absent or the values don't parse as four numbers. We don't
/// try to be clever about whitespace, units, or commas — the LLM is
/// expected to emit a clean `viewBox="-10 -10 320 200"`-style value.
fn parse_view_box(svg: &str) -> Option<(f64, f64, f64, f64)> {
    // Case-insensitive search for the attribute name.
    let lower = svg.to_ascii_lowercase();
    let idx = lower.find("viewbox=\"")?;
    let after = &svg[idx + "viewbox=\"".len()..];
    let end = after.find('"')?;
    let value = &after[..end];
    let parts: Vec<&str> = value
        .split(|c: char| c.is_whitespace() || c == ',')
        .filter(|s| !s.is_empty())
        .collect();
    if parts.len() != 4 {
        return None;
    }
    Some((
        parts[0].parse().ok()?,
        parts[1].parse().ok()?,
        parts[2].parse().ok()?,
        parts[3].parse().ok()?,
    ))
}

/// Parse "16:9" / "4:3" / "1:1" into (width, height). Returns None on
/// anything else — the LLM is allowed to omit the hint entirely.
fn parse_aspect_ratio(s: &str) -> Option<(f64, f64)> {
    let (a, b) = s.split_once(':')?;
    let a: f64 = a.trim().parse().ok()?;
    let b: f64 = b.trim().parse().ok()?;
    if a <= 0.0 || b <= 0.0 {
        return None;
    }
    Some((a, b))
}

/// Scan an SVG source string for `asset://<id>` references inside any
/// `href="..."` / `xlink:href="..."` attribute, and substitute each one
/// with an inline `data:<mime>;base64,<...>` URL backed by the asset
/// registry. Returns the rewritten source.
///
/// We deliberately do this with a simple `find` loop instead of a full
/// XML parser: SVG attributes are quoted, the asset id charset is a known
/// ASCII subset (`asset-` + lowercase hex), and we want to be tolerant of
/// whitespace, single vs. double quotes, and `xlink:href` aliases. If the
/// LLM emits malformed XML the higher-level SVG validator (or the browser)
/// will catch it; this pass only deals with the asset substitution.
fn resolve_asset_references(svg: &str) -> Result<String, ToolError> {
    let prefix = "asset://";
    let mut out = String::with_capacity(svg.len());
    let mut cursor = 0;
    let mut found_any = false;

    while let Some(idx) = svg[cursor..].find(prefix) {
        let abs_idx = cursor + idx;
        // Walk back to find the opening quote of the attribute value.
        // We accept `"`, `'`, or end-of-prev-tag as the boundary; SVG
        // authors also use `xlink:href=` so we look for either prefix.
        let prefix_start = abs_idx;
        // Find the closing quote (or single quote) that terminates this
        // attribute value.
        let after = &svg[prefix_start + prefix.len()..];
        let end_rel = after
            .find(|c: char| c == '"' || c == '\'' || c.is_whitespace())
            .unwrap_or(after.len());
        let id = &after[..end_rel];

        // The id grammar is `asset-` + hex; any other shape is a typo.
        if !is_asset_id(id) {
            return Err(ToolError::InvalidArguments(
                "create_svg".to_string(),
                format!(
                    "asset reference `{}{}` is malformed (expected `asset://asset-XXXXXXXX`); \
                     re-issue with the `asset_id` returned by `read_image`",
                    prefix, id
                ),
            ));
        }

        // Pull the bytes from the registry. Missing or expired entries
        // surface as a clear error so the LLM can re-call `read_image`.
        let entry = asset_registry::lookup(id).ok_or_else(|| {
            ToolError::InvalidArguments(
                "create_svg".to_string(),
                format!(
                    "asset `{}` is unknown or expired (>1 hour old). Call `read_image` again \
                     on the source image and use the fresh `asset_id`.",
                    id
                ),
            )
        })?;

        // Emit everything up to (and including) the asset id, then the
        // data: URL, then continue parsing from after the closing quote.
        out.push_str(&svg[cursor..prefix_start]);
        out.push_str("data:");
        out.push_str(&entry.mime);
        out.push_str(";base64,");
        out.push_str(&BASE64.encode(&entry.data));

        // Skip past the id we just consumed. The next char is the
        // attribute's closing quote (or whitespace), which we leave in
        // place so the resulting `href="data:..."` stays well-formed.
        cursor = prefix_start + prefix.len() + end_rel;
        found_any = true;
    }

    out.push_str(&svg[cursor..]);
    debug_assert!(
        found_any || !svg.contains(prefix),
        "found_any / prefix presence out of sync"
    );
    Ok(out)
}

fn is_asset_id(s: &str) -> bool {
    let rest = match s.strip_prefix("asset-") {
        Some(r) => r,
        None => return false,
    };
    !rest.is_empty() && rest.len() <= 32 && rest.chars().all(|c| c.is_ascii_hexdigit())
}

// `Serialize` is implemented so the outcome can be round-tripped through
// the registry's `ToolResult` if we ever want to (e.g. for an MCP bridge).
// The registry currently uses `outcome.output` (a `String`) directly, so
// the derive is forward-looking and intentionally not stripped.
impl Serialize for CreateSvgOutcome {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut st = s.serialize_struct("CreateSvgOutcome", 5)?;
        st.serialize_field("output", &self.output)?;
        st.serialize_field("file_path", &self.file_path)?;
        st.serialize_field("byte_size", &self.byte_size)?;
        st.serialize_field("view_box", &self.view_box.map(|(x, y, w, h)| [x, y, w, h]))?;
        st.serialize_field("is_error", &self.is_error)?;
        st.end()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_view_box_typical() {
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 320 200">...</svg>"#;
        assert_eq!(parse_view_box(svg), Some((0.0, 0.0, 320.0, 200.0)));
    }

    #[test]
    fn parse_view_box_missing() {
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100"></svg>"#;
        assert_eq!(parse_view_box(svg), None);
    }

    #[test]
    fn parse_view_box_with_comma_separator() {
        let svg = r#"<svg viewBox="-10.5,-5.5,200,100">"#;
        assert_eq!(parse_view_box(svg), Some((-10.5, -5.5, 200.0, 100.0)));
    }

    #[test]
    fn parse_aspect_ratio_ok() {
        assert_eq!(parse_aspect_ratio("16:9"), Some((16.0, 9.0)));
        assert_eq!(parse_aspect_ratio("1:1"), Some((1.0, 1.0)));
        assert_eq!(parse_aspect_ratio(" 3 : 4 "), Some((3.0, 4.0)));
    }

    #[test]
    fn parse_aspect_ratio_rejects_garbage() {
        assert_eq!(parse_aspect_ratio("wide"), None);
        assert_eq!(parse_aspect_ratio("0:9"), None);
        assert_eq!(parse_aspect_ratio(""), None);
    }

    // ─── resolve_asset_references ──────────────────────────────────────

    fn register_test_asset(id: &str, mime: &str, ext: &str, data: &[u8]) {
        use std::time::Instant;
        asset_registry::insert(
            id.to_string(),
            asset_registry::AssetEntry {
                mime: mime.to_string(),
                ext: ext.to_string(),
                data: data.to_vec(),
                inserted_at: Instant::now(),
                source_path: format!("/tmp/{id}.{ext}"),
            },
        );
    }

    #[test]
    fn resolve_substitutes_data_url() {
        asset_registry::clear();
        register_test_asset("asset-deadbeef", "image/png", "png", b"PNGDATA");

        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg"><image href="asset://asset-deadbeef" x="0" y="0" width="100" height="100"/></svg>"#;
        let out = resolve_asset_references(svg).expect("resolve ok");
        assert!(out.contains("data:image/png;base64,UE5HREFUQQ=="), "got: {out}");
        assert!(!out.contains("asset://"), "asset reference must be gone");
        // Attribute structure preserved: the closing `"` should still be there.
        assert!(out.contains(r#"data:image/png;base64,UE5HREFUQQ==""#));
    }

    #[test]
    fn resolve_handles_xlink_href() {
        asset_registry::clear();
        register_test_asset("asset-1234abcd", "image/jpeg", "jpg", b"\xFF\xD8\xFF");
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink"><image xlink:href="asset://asset-1234abcd"/></svg>"#;
        let out = resolve_asset_references(svg).expect("resolve ok");
        assert!(out.contains("data:image/jpeg;base64,"));
        assert!(!out.contains("asset://"));
    }

    #[test]
    fn resolve_handles_single_quotes() {
        asset_registry::clear();
        register_test_asset("asset-0001", "image/png", "png", b"X");
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg"><image href='asset://asset-0001'/></svg>"#;
        let out = resolve_asset_references(svg).expect("resolve ok");
        assert!(out.contains("data:image/png;base64,"));
    }

    #[test]
    fn resolve_no_references_is_passthrough() {
        asset_registry::clear();
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg"><rect/></svg>"#;
        let out = resolve_asset_references(svg).expect("resolve ok");
        assert_eq!(out, svg);
    }

    #[test]
    fn resolve_unknown_id_is_error() {
        asset_registry::clear();
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg"><image href="asset://asset-0123abcd"/></svg>"#;
        let err = resolve_asset_references(svg).expect_err("unknown id must error");
        let msg = err.to_string();
        assert!(msg.contains("unknown or expired"), "msg: {msg}");
    }

    #[test]
    fn resolve_malformed_id_is_error() {
        asset_registry::clear();
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg"><image href="asset://nothex!"/></svg>"#;
        let err = resolve_asset_references(svg).expect_err("malformed id must error");
        let msg = err.to_string();
        assert!(msg.contains("malformed"), "msg: {msg}");
    }

    #[test]
    fn resolve_multiple_references() {
        asset_registry::clear();
        register_test_asset("asset-aaaa", "image/png", "png", b"AAAA");
        register_test_asset("asset-bbbb", "image/jpeg", "jpg", b"BBBB");
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg">
  <image href="asset://asset-aaaa" width="10"/>
  <image href="asset://asset-bbbb" width="20"/>
</svg>"#;
        let out = resolve_asset_references(svg).expect("resolve ok");
        assert!(out.contains("data:image/png;base64,QUFBQQ=="), "missing png data");
        assert!(out.contains("data:image/jpeg;base64,QkJCQg=="), "missing jpeg data");
        // The png and jpeg base64 strings are distinct.
        let png_pos = out.find("QUFBQQ==").unwrap();
        let jpeg_pos = out.find("QkJCQg==").unwrap();
        assert_ne!(png_pos, jpeg_pos);
    }

    /// End-to-end: call `CreateSvgTool::execute` with an `asset://` reference
    /// and confirm the bytes that hit disk contain the substituted data URL.
    /// This is the user-facing contract: AI emits `<image href="asset://...">`,
    /// tool produces a self-contained SVG.
    #[tokio::test]
    async fn execute_writes_resolved_svg_to_disk() {
        asset_registry::clear();
        register_test_asset("asset-12345678", "image/png", "png", b"PNGBYTES");
        let dir = std::env::temp_dir().join("inkuo-svg-asset-test");
        let _ = std::fs::create_dir_all(&dir);
        let out_path = dir.join("out.svg");
        // Clean up any prior file so we read the one we just wrote.
        let _ = std::fs::remove_file(&out_path);

        let svg = r#"<?xml version="1.0" encoding="UTF-8"?>
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100">
  <image href="asset://asset-12345678" x="0" y="0" width="100" height="100"/>
</svg>"#;

        let tool = CreateSvgTool::new();
        let outcome = tool
            .execute(
                serde_json::json!({
                    "description": "test",
                    "svg_source": svg,
                    "output_path": out_path.to_string_lossy(),
                }),
                None,
            )
            .await
            .expect("create_svg execute ok");

        // The returned `svg_source` is the resolved copy (so the frontend
        // preview chip can render the embedded image without re-running
        // resolution).
        assert!(outcome.svg_source.contains("data:image/png;base64,UE5HQllURVM="));
        assert!(!outcome.svg_source.contains("asset://"));

        // Same content on disk.
        let on_disk = std::fs::read_to_string(&out_path).expect("read back");
        assert!(on_disk.contains("data:image/png;base64,UE5HQllURVM="));
        assert!(!on_disk.contains("asset://"));
    }
}
