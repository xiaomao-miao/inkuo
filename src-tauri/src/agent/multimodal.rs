//! Provider-neutral multimodal message inputs.
//!
//! The frontend deals in either workspace-local image paths (the normal
//! screenshot/visual-QA path) or an already captured base64 payload.  This
//! module validates and bounds both forms once, then the agent loop renders
//! the same [`ImageAttachment`] as either OpenAI content parts or Ollama's
//! `images` array.  Keeping provider adaptation here prevents every caller
//! from growing its own subtly-incompatible image payload.

use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use serde::{Deserialize, Serialize};
use std::path::Path;

pub(crate) const MAX_IMAGE_COUNT: usize = 8;
const MAX_IMAGE_BYTES: usize = 12 * 1024 * 1024;
pub(crate) const MAX_TOTAL_IMAGE_BYTES: usize = 32 * 1024 * 1024;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ImageDetail {
    Auto,
    Low,
    High,
}

impl Default for ImageDetail {
    fn default() -> Self {
        Self::Auto
    }
}

impl ImageDetail {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Low => "low",
            Self::High => "high",
        }
    }
}

/// Wire shape accepted by `ai_agent_stream`.
///
/// Exactly one of `path` and `data_base64` must be present.  `data_base64`
/// may be either bare base64 or a `data:image/...;base64,...` URL.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FrontendImageAttachment {
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default, alias = "data")]
    pub data_base64: Option<String>,
    #[serde(default, alias = "mime")]
    pub mime_type: Option<String>,
    #[serde(default)]
    pub detail: ImageDetail,
    #[serde(default)]
    pub name: Option<String>,
}

/// Validated image carried inside an agent conversation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageAttachment {
    pub mime_type: String,
    pub data_base64: String,
    pub detail: ImageDetail,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Decoded byte count, retained so a tool batch can be bounded without
    /// decoding base64 a second time. Legacy serialized messages default to
    /// zero and are conservatively estimated from base64 length.
    #[serde(default)]
    pub byte_len: usize,
}

/// Image queued by an agent tool for the *next* model request.
///
/// The executor only enqueues these after it has appended every tool result
/// for the assistant's tool-call batch. This preserves the Chat Completions
/// protocol ordering (`assistant(tool_calls)` → all `tool` messages → one
/// multimodal `user` inspection message).
#[derive(Debug, Clone)]
pub struct VisualInspectionInput {
    pub source_tool_call_id: String,
    pub asset_id: String,
    pub attachment: ImageAttachment,
}

impl ImageAttachment {
    pub fn as_data_url(&self) -> String {
        format!("data:{};base64,{}", self.mime_type, self.data_base64)
    }

    pub fn decoded_len(&self) -> usize {
        if self.byte_len > 0 {
            self.byte_len
        } else {
            self.data_base64.len().saturating_mul(3) / 4
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum MultimodalError {
    #[error("too many image attachments ({count}); maximum is {max}")]
    TooManyImages { count: usize, max: usize },
    #[error("image attachment #{index} must provide exactly one of path or dataBase64")]
    InvalidSource { index: usize },
    #[error("image attachment #{index} is outside the workspace: {reason}")]
    OutsideWorkspace { index: usize, reason: String },
    #[error("failed to read image attachment #{index}: {reason}")]
    ReadFailed { index: usize, reason: String },
    #[error("image attachment #{index} has unsupported MIME type '{mime}'")]
    UnsupportedMime { index: usize, mime: String },
    #[error("image attachment #{index} is not valid base64: {reason}")]
    InvalidBase64 { index: usize, reason: String },
    #[error("image attachment #{index} is {size} bytes; maximum is {max}")]
    ImageTooLarge {
        index: usize,
        size: usize,
        max: usize,
    },
    #[error("image attachments total {size} bytes; maximum is {max}")]
    TotalTooLarge { size: usize, max: usize },
    #[error("image attachment #{index} content does not match MIME type '{mime}'")]
    MimeMismatch { index: usize, mime: String },
    #[error("visual inspection asset '{0}' is missing or expired")]
    MissingAsset(String),
    #[error("visual inspection asset '{asset_id}' uses unsupported MIME type '{mime}'")]
    UnsupportedAssetMime { asset_id: String, mime: String },
    #[error("invalid visual asset manifest in tool output: {0}")]
    InvalidVisualAssetManifest(String),
}

fn mime_from_path(path: &Path) -> Option<&'static str> {
    match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "webp" => Some("image/webp"),
        "gif" => Some("image/gif"),
        _ => None,
    }
}

fn normalise_mime(raw: &str) -> Option<&'static str> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "image/png" => Some("image/png"),
        "image/jpeg" | "image/jpg" => Some("image/jpeg"),
        "image/webp" => Some("image/webp"),
        "image/gif" => Some("image/gif"),
        _ => None,
    }
}

fn magic_matches(mime: &str, bytes: &[u8]) -> bool {
    match mime {
        "image/png" => bytes.starts_with(b"\x89PNG\r\n\x1a\n"),
        "image/jpeg" => bytes.starts_with(&[0xff, 0xd8, 0xff]),
        "image/gif" => bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a"),
        "image/webp" => bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP",
        _ => false,
    }
}

fn split_data_url(value: &str) -> (Option<&str>, &str) {
    let Some(rest) = value.strip_prefix("data:") else {
        return (None, value);
    };
    let Some((header, payload)) = rest.split_once(',') else {
        return (None, value);
    };
    let Some(mime) = header.strip_suffix(";base64") else {
        return (None, value);
    };
    (Some(mime), payload)
}

/// Resolve and validate a group of image inputs.
///
/// Local paths are workspace-bounded with the same canonical/symlink-safe
/// validator used by agent file tools.  The returned payload is canonical
/// base64 with a verified MIME magic header.
pub fn resolve_image_attachments(
    inputs: Vec<FrontendImageAttachment>,
    workspace: &Option<String>,
) -> Result<Vec<ImageAttachment>, MultimodalError> {
    if inputs.len() > MAX_IMAGE_COUNT {
        return Err(MultimodalError::TooManyImages {
            count: inputs.len(),
            max: MAX_IMAGE_COUNT,
        });
    }

    let mut result = Vec::with_capacity(inputs.len());
    let mut total_bytes = 0usize;

    for (zero_index, input) in inputs.into_iter().enumerate() {
        let index = zero_index + 1;
        let has_path = input.path.as_ref().is_some_and(|p| !p.trim().is_empty());
        let has_data = input
            .data_base64
            .as_ref()
            .is_some_and(|d| !d.trim().is_empty());
        if has_path == has_data {
            return Err(MultimodalError::InvalidSource { index });
        }

        let (bytes, inferred_mime, fallback_name) = if let Some(path) = input.path.as_ref() {
            // Frontends naturally send workspace-relative screenshot paths.
            // Resolve those against the declared workspace before invoking
            // the canonical/symlink-safe boundary validator; resolving them
            // against the process CWD would reject a legitimate attachment
            // whenever the app was launched from another directory.
            let requested = Path::new(path);
            let resolved = if requested.is_absolute() {
                requested.to_path_buf()
            } else if let Some(root) = workspace.as_ref() {
                Path::new(root).join(requested)
            } else {
                return Err(MultimodalError::OutsideWorkspace {
                    index,
                    reason: "a workspace is required for a relative image path".to_string(),
                });
            };
            // Validate and read the same canonical target. Besides making
            // relative paths independent of process CWD, this ensures a
            // workspace symlink cannot redirect the actual read outside the
            // boundary after we checked only its lexical spelling.
            let canonical =
                std::fs::canonicalize(&resolved).map_err(|error| MultimodalError::ReadFailed {
                    index,
                    reason: error.to_string(),
                })?;
            let canonical_string = canonical.to_string_lossy().to_string();
            crate::security::validate_workspace_path(&canonical_string, workspace).map_err(
                |error| MultimodalError::OutsideWorkspace {
                    index,
                    reason: error.to_string(),
                },
            )?;
            let bytes = std::fs::read(&canonical).map_err(|error| MultimodalError::ReadFailed {
                index,
                reason: error.to_string(),
            })?;
            let inferred = mime_from_path(&canonical).map(str::to_string);
            let name = canonical
                .file_name()
                .map(|n| n.to_string_lossy().to_string());
            (bytes, inferred, name)
        } else {
            let raw = input.data_base64.as_deref().unwrap_or_default().trim();
            let (data_url_mime, payload) = split_data_url(raw);
            let compact: String = payload.chars().filter(|c| !c.is_whitespace()).collect();
            let bytes = BASE64.decode(compact.as_bytes()).map_err(|error| {
                MultimodalError::InvalidBase64 {
                    index,
                    reason: error.to_string(),
                }
            })?;
            (bytes, data_url_mime.map(str::to_string), None)
        };

        if bytes.len() > MAX_IMAGE_BYTES {
            return Err(MultimodalError::ImageTooLarge {
                index,
                size: bytes.len(),
                max: MAX_IMAGE_BYTES,
            });
        }
        total_bytes = total_bytes.saturating_add(bytes.len());
        if total_bytes > MAX_TOTAL_IMAGE_BYTES {
            return Err(MultimodalError::TotalTooLarge {
                size: total_bytes,
                max: MAX_TOTAL_IMAGE_BYTES,
            });
        }

        let mime_raw = input
            .mime_type
            .as_deref()
            .map(str::to_string)
            .or(inferred_mime)
            .unwrap_or_default();
        let mime = normalise_mime(&mime_raw).ok_or_else(|| MultimodalError::UnsupportedMime {
            index,
            mime: mime_raw.clone(),
        })?;
        if !magic_matches(mime, &bytes) {
            return Err(MultimodalError::MimeMismatch {
                index,
                mime: mime.to_string(),
            });
        }

        let byte_len = bytes.len();
        result.push(ImageAttachment {
            mime_type: mime.to_string(),
            data_base64: BASE64.encode(bytes),
            detail: input.detail,
            name: input.name.or(fallback_name),
            byte_len,
        });
    }

    Ok(result)
}

/// Resolve multiple logical message groups under one request-wide budget.
///
/// A group normally corresponds to one historical user message, with the
/// final group representing the current turn. Flattening before resolution
/// is intentional: applying the limits independently to each message would
/// allow a long history to upload an unbounded number of pixels in a single
/// provider request.
pub fn resolve_image_attachment_groups(
    groups: Vec<Vec<FrontendImageAttachment>>,
    workspace: &Option<String>,
) -> Result<Vec<Vec<ImageAttachment>>, MultimodalError> {
    let lengths: Vec<usize> = groups.iter().map(Vec::len).collect();
    let count = lengths
        .iter()
        .try_fold(0usize, |total, len| total.checked_add(*len))
        .unwrap_or(usize::MAX);
    if count > MAX_IMAGE_COUNT {
        return Err(MultimodalError::TooManyImages {
            count,
            max: MAX_IMAGE_COUNT,
        });
    }

    let flattened = groups.into_iter().flatten().collect();
    let mut resolved = resolve_image_attachments(flattened, workspace)?.into_iter();
    let resolved_groups = lengths
        .into_iter()
        .map(|len| resolved.by_ref().take(len).collect())
        .collect();
    debug_assert!(resolved.next().is_none());
    Ok(resolved_groups)
}

/// Validate every active image that will be serialized into one provider
/// request, irrespective of which conversation message owns it.
pub fn validate_image_request_budget<'a>(
    images: impl IntoIterator<Item = &'a ImageAttachment>,
) -> Result<(), MultimodalError> {
    let mut count = 0usize;
    let mut total_bytes = 0usize;
    for image in images {
        count = count.saturating_add(1);
        if count > MAX_IMAGE_COUNT {
            return Err(MultimodalError::TooManyImages {
                count,
                max: MAX_IMAGE_COUNT,
            });
        }
        let size = image.decoded_len();
        if size > MAX_IMAGE_BYTES {
            return Err(MultimodalError::ImageTooLarge {
                index: count,
                size,
                max: MAX_IMAGE_BYTES,
            });
        }
        total_bytes = total_bytes.saturating_add(size);
        if total_bytes > MAX_TOTAL_IMAGE_BYTES {
            return Err(MultimodalError::TotalTooLarge {
                size: total_bytes,
                max: MAX_TOTAL_IMAGE_BYTES,
            });
        }
    }
    Ok(())
}

/// Bridge a `read_image` tool result into a provider-neutral visual input.
/// This is the tool-loop counterpart to `resolve_image_attachments`: the
/// asset registry keeps pixels out of textual tool results, then this bridge
/// restores the bytes directly into the following multimodal API message.
pub fn visual_inspection_from_asset(
    source_tool_call_id: impl Into<String>,
    asset_id: &str,
    workspace: Option<&str>,
) -> Result<VisualInspectionInput, MultimodalError> {
    let asset = super::tools::asset_registry::lookup_for_workspace(asset_id, workspace)
        .ok_or_else(|| MultimodalError::MissingAsset(asset_id.to_string()))?;
    let mime =
        normalise_mime(&asset.mime).ok_or_else(|| MultimodalError::UnsupportedAssetMime {
            asset_id: asset_id.to_string(),
            mime: asset.mime.clone(),
        })?;
    if asset.data.len() > MAX_IMAGE_BYTES {
        return Err(MultimodalError::ImageTooLarge {
            index: 1,
            size: asset.data.len(),
            max: MAX_IMAGE_BYTES,
        });
    }
    if !magic_matches(mime, &asset.data) {
        return Err(MultimodalError::MimeMismatch {
            index: 1,
            mime: mime.to_string(),
        });
    }
    let name = Path::new(&asset.source_path)
        .file_name()
        .map(|name| name.to_string_lossy().to_string());
    let byte_len = asset.data.len();
    Ok(VisualInspectionInput {
        source_tool_call_id: source_tool_call_id.into(),
        asset_id: asset_id.to_string(),
        attachment: ImageAttachment {
            mime_type: mime.to_string(),
            data_base64: BASE64.encode(asset.data),
            detail: ImageDetail::High,
            name,
            byte_len,
        },
    })
}

/// Resolve the standard visual-output contract emitted by tools.
///
/// A single-image producer such as `read_image` returns top-level
/// `asset_id`; renderers return `visual_assets: [{ asset_id, ... }]`. Tool
/// Only those explicitly trusted producer names are parsed; every other tool
/// produces an empty list even if its output mimics the JSON shape. A trusted
/// producer with malformed/missing fields fails clearly so the agent cannot
/// mistake metadata for a completed visual check.
pub fn visual_inspections_from_tool_output(
    source_tool_call_id: &str,
    source_tool_name: &str,
    output: &str,
    workspace: Option<&str>,
) -> Result<Vec<VisualInspectionInput>, MultimodalError> {
    // Check producer capability before parsing. Ordinary file/search results
    // can be large and are untrusted text; parsing all of them as JSON would
    // waste time and let an unrelated tool mimic an asset manifest.
    if !matches!(source_tool_name, "read_image" | "render_office_preview") {
        return Ok(Vec::new());
    }
    let value: serde_json::Value = serde_json::from_str(output).map_err(|error| {
        MultimodalError::InvalidVisualAssetManifest(format!(
            "{} returned invalid JSON: {}",
            source_tool_name, error
        ))
    })?;

    let mut asset_ids = Vec::new();
    if source_tool_name == "render_office_preview" {
        let Some(assets) = value.get("visual_assets") else {
            return Err(MultimodalError::InvalidVisualAssetManifest(
                "render_office_preview returned no visual_assets array".to_string(),
            ));
        };
        let assets = assets.as_array().ok_or_else(|| {
            MultimodalError::InvalidVisualAssetManifest(
                "visual_assets must be an array".to_string(),
            )
        })?;
        for (index, asset) in assets.iter().enumerate() {
            let asset_id = asset
                .get("asset_id")
                .and_then(serde_json::Value::as_str)
                .filter(|id| !id.trim().is_empty())
                .ok_or_else(|| {
                    MultimodalError::InvalidVisualAssetManifest(format!(
                        "visual_assets[{}].asset_id must be a non-empty string",
                        index
                    ))
                })?;
            asset_ids.push(asset_id.to_string());
        }
    } else if source_tool_name == "read_image" {
        let Some(asset_id) = value.get("asset_id") else {
            return Err(MultimodalError::InvalidVisualAssetManifest(
                "read_image returned no asset_id".to_string(),
            ));
        };
        let asset_id = asset_id
            .as_str()
            .filter(|id| !id.trim().is_empty())
            .ok_or_else(|| {
                MultimodalError::InvalidVisualAssetManifest(
                    "asset_id must be a non-empty string".to_string(),
                )
            })?;
        asset_ids.push(asset_id.to_string());
    }

    if asset_ids.len() > MAX_IMAGE_COUNT {
        return Err(MultimodalError::TooManyImages {
            count: asset_ids.len(),
            max: MAX_IMAGE_COUNT,
        });
    }

    let mut inspections = Vec::with_capacity(asset_ids.len());
    for asset_id in asset_ids {
        let input = visual_inspection_from_asset(source_tool_call_id, &asset_id, workspace)?;
        push_visual_inspection_bounded(&mut inspections, input)?;
    }
    Ok(inspections)
}

/// Add a tool-produced inspection image while enforcing the same count and
/// decoded-byte ceilings as direct frontend attachments.
pub fn push_visual_inspection_bounded(
    batch: &mut Vec<VisualInspectionInput>,
    input: VisualInspectionInput,
) -> Result<(), MultimodalError> {
    if batch.len() >= MAX_IMAGE_COUNT {
        return Err(MultimodalError::TooManyImages {
            count: batch.len() + 1,
            max: MAX_IMAGE_COUNT,
        });
    }
    let current_bytes: usize = batch.iter().map(|item| item.attachment.decoded_len()).sum();
    let next_bytes = current_bytes.saturating_add(input.attachment.decoded_len());
    if next_bytes > MAX_TOTAL_IMAGE_BYTES {
        return Err(MultimodalError::TotalTooLarge {
            size: next_bytes,
            max: MAX_TOTAL_IMAGE_BYTES,
        });
    }
    batch.push(input);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // 1x1 transparent PNG.
    const PNG_1PX: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=";

    fn frontend_png(name: &str) -> FrontendImageAttachment {
        FrontendImageAttachment {
            path: None,
            data_base64: Some(PNG_1PX.to_string()),
            mime_type: Some("image/png".to_string()),
            detail: ImageDetail::Auto,
            name: Some(name.to_string()),
        }
    }

    #[test]
    fn accepts_bare_base64_and_builds_data_url() {
        let images = resolve_image_attachments(
            vec![FrontendImageAttachment {
                path: None,
                data_base64: Some(PNG_1PX.to_string()),
                mime_type: Some("image/png".to_string()),
                detail: ImageDetail::High,
                name: Some("preview.png".to_string()),
            }],
            &None,
        )
        .unwrap();
        assert_eq!(images.len(), 1);
        assert!(images[0]
            .as_data_url()
            .starts_with("data:image/png;base64,"));
        assert_eq!(images[0].detail, ImageDetail::High);
    }

    #[test]
    fn rejects_mime_spoofing() {
        let error = resolve_image_attachments(
            vec![FrontendImageAttachment {
                path: None,
                data_base64: Some(PNG_1PX.to_string()),
                mime_type: Some("image/jpeg".to_string()),
                detail: ImageDetail::Auto,
                name: None,
            }],
            &None,
        )
        .unwrap_err();
        assert!(matches!(error, MultimodalError::MimeMismatch { .. }));
    }

    #[test]
    fn requires_exactly_one_source() {
        let error = resolve_image_attachments(
            vec![FrontendImageAttachment {
                path: None,
                data_base64: None,
                mime_type: None,
                detail: ImageDetail::Auto,
                name: None,
            }],
            &None,
        )
        .unwrap_err();
        assert!(matches!(error, MultimodalError::InvalidSource { .. }));
    }

    #[test]
    fn request_groups_share_one_image_count_limit() {
        let historical_one = (0..4)
            .map(|index| frontend_png(&format!("history-a-{index}.png")))
            .collect();
        let historical_two = (0..4)
            .map(|index| frontend_png(&format!("history-b-{index}.png")))
            .collect();
        let current = vec![frontend_png("current.png")];

        let error =
            resolve_image_attachment_groups(vec![historical_one, historical_two, current], &None)
                .unwrap_err();
        assert!(matches!(
            error,
            MultimodalError::TooManyImages { count: 9, max: 8 }
        ));
    }

    #[test]
    fn request_budget_combines_history_and_current_bytes() {
        let attachment = |name: &str, byte_len: usize| ImageAttachment {
            mime_type: "image/png".to_string(),
            data_base64: String::new(),
            detail: ImageDetail::Auto,
            name: Some(name.to_string()),
            byte_len,
        };
        let historical = [
            attachment("history-a.png", 12 * 1024 * 1024),
            attachment("history-b.png", 12 * 1024 * 1024),
        ];
        let current = [attachment("current.png", 8 * 1024 * 1024 + 1)];

        let error =
            validate_image_request_budget(historical.iter().chain(current.iter())).unwrap_err();
        assert!(matches!(error, MultimodalError::TotalTooLarge { .. }));
    }

    fn temp_workspace(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "inkuo_multimodal_{}_{}",
            name,
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn resolves_relative_and_absolute_workspace_paths() {
        let dir = temp_workspace("paths");
        let path = dir.join("preview.png");
        std::fs::write(&path, BASE64.decode(PNG_1PX).unwrap()).unwrap();
        let workspace = Some(dir.to_string_lossy().to_string());

        for candidate in [
            "preview.png".to_string(),
            path.to_string_lossy().to_string(),
        ] {
            let images = resolve_image_attachments(
                vec![FrontendImageAttachment {
                    path: Some(candidate),
                    data_base64: None,
                    mime_type: None,
                    detail: ImageDetail::Auto,
                    name: None,
                }],
                &workspace,
            )
            .unwrap();
            assert_eq!(images[0].mime_type, "image/png");
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn rejects_absolute_path_outside_workspace() {
        let inside = temp_workspace("inside");
        let outside = temp_workspace("outside");
        let path = outside.join("preview.png");
        std::fs::write(&path, BASE64.decode(PNG_1PX).unwrap()).unwrap();
        let error = resolve_image_attachments(
            vec![FrontendImageAttachment {
                path: Some(path.to_string_lossy().to_string()),
                data_base64: None,
                mime_type: None,
                detail: ImageDetail::Auto,
                name: None,
            }],
            &Some(inside.to_string_lossy().to_string()),
        )
        .unwrap_err();
        assert!(matches!(error, MultimodalError::OutsideWorkspace { .. }));
        let _ = std::fs::remove_dir_all(inside);
        let _ = std::fs::remove_dir_all(outside);
    }

    #[test]
    fn rejects_relative_path_when_no_workspace_is_declared() {
        let error = resolve_image_attachments(
            vec![FrontendImageAttachment {
                path: Some("preview.png".to_string()),
                data_base64: None,
                mime_type: None,
                detail: ImageDetail::Auto,
                name: None,
            }],
            &None,
        )
        .unwrap_err();
        assert!(matches!(error, MultimodalError::OutsideWorkspace { .. }));
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlink_that_escapes_workspace() {
        use std::os::unix::fs::symlink;
        let inside = temp_workspace("symlink_inside");
        let outside = temp_workspace("symlink_outside");
        let target = outside.join("preview.png");
        let link = inside.join("linked.png");
        std::fs::write(&target, BASE64.decode(PNG_1PX).unwrap()).unwrap();
        symlink(&target, &link).unwrap();
        let error = resolve_image_attachments(
            vec![FrontendImageAttachment {
                path: Some("linked.png".to_string()),
                data_base64: None,
                mime_type: None,
                detail: ImageDetail::Auto,
                name: None,
            }],
            &Some(inside.to_string_lossy().to_string()),
        )
        .unwrap_err();
        assert!(matches!(error, MultimodalError::OutsideWorkspace { .. }));
        let _ = std::fs::remove_dir_all(inside);
        let _ = std::fs::remove_dir_all(outside);
    }

    #[test]
    fn bridges_asset_registry_pixels_for_next_model_turn() {
        use std::time::Instant;
        let _registry_guard = super::super::tools::asset_registry::test_registry_guard();
        super::super::tools::asset_registry::clear();
        let workspace = temp_workspace("asset_owner");
        let other_workspace = temp_workspace("asset_other");
        let canonical_workspace = std::fs::canonicalize(&workspace).unwrap();
        let id = super::super::tools::asset_registry::fresh_id();
        super::super::tools::asset_registry::insert(
            id.clone(),
            super::super::tools::asset_registry::AssetEntry {
                mime: "image/png".to_string(),
                ext: "png".to_string(),
                data: BASE64.decode(PNG_1PX).unwrap(),
                inserted_at: Instant::now(),
                source_path: workspace.join("preview.png").to_string_lossy().to_string(),
                workspace_root: canonical_workspace.to_string_lossy().to_string(),
            },
        );
        let inspection =
            visual_inspection_from_asset("call-1", &id, Some(workspace.to_string_lossy().as_ref()))
                .unwrap();
        assert_eq!(inspection.source_tool_call_id, "call-1");
        assert_eq!(inspection.attachment.mime_type, "image/png");
        assert!(inspection.attachment.as_data_url().contains(";base64,"));
        let render_output = serde_json::json!({
            "visual_assets": [{"asset_id": id, "page_number": 1}]
        })
        .to_string();
        let rendered = visual_inspections_from_tool_output(
            "render-call",
            "render_office_preview",
            &render_output,
            Some(workspace.to_string_lossy().as_ref()),
        )
        .unwrap();
        assert_eq!(rendered.len(), 1);
        assert_eq!(rendered[0].source_tool_call_id, "render-call");
        assert!(visual_inspections_from_tool_output(
            "forged-call",
            "read_file",
            &render_output,
            Some(workspace.to_string_lossy().as_ref()),
        )
        .unwrap()
        .is_empty());
        assert!(matches!(
            visual_inspection_from_asset(
                "call-2",
                &id,
                Some(other_workspace.to_string_lossy().as_ref()),
            ),
            Err(MultimodalError::MissingAsset(_))
        ));
        super::super::tools::asset_registry::clear();
        let _ = std::fs::remove_dir_all(workspace);
        let _ = std::fs::remove_dir_all(other_workspace);
    }

    #[test]
    fn office_renderer_manifest_requires_a_bounded_asset_array() {
        assert!(matches!(
            visual_inspections_from_tool_output(
                "call",
                "render_office_preview",
                "not-json",
                Some("/workspace"),
            ),
            Err(MultimodalError::InvalidVisualAssetManifest(_))
        ));
        assert!(matches!(
            visual_inspections_from_tool_output(
                "call",
                "render_office_preview",
                r#"{"visual_assets":"asset-id"}"#,
                Some("/workspace"),
            ),
            Err(MultimodalError::InvalidVisualAssetManifest(_))
        ));
        let too_many = serde_json::json!({
            "visual_assets": (0..9)
                .map(|index| serde_json::json!({"asset_id": format!("asset-{index}")}))
                .collect::<Vec<_>>()
        })
        .to_string();
        assert!(matches!(
            visual_inspections_from_tool_output(
                "call",
                "render_office_preview",
                &too_many,
                Some("/workspace"),
            ),
            Err(MultimodalError::TooManyImages { count: 9, max: 8 })
        ));
    }

    #[test]
    fn visual_batch_enforces_count_limit() {
        let attachment = ImageAttachment {
            mime_type: "image/png".to_string(),
            data_base64: PNG_1PX.to_string(),
            detail: ImageDetail::High,
            name: None,
            byte_len: 1,
        };
        let mut batch = Vec::new();
        for index in 0..MAX_IMAGE_COUNT {
            push_visual_inspection_bounded(
                &mut batch,
                VisualInspectionInput {
                    source_tool_call_id: format!("call-{index}"),
                    asset_id: format!("asset-{index}"),
                    attachment: attachment.clone(),
                },
            )
            .unwrap();
        }
        let error = push_visual_inspection_bounded(
            &mut batch,
            VisualInspectionInput {
                source_tool_call_id: "call-over".to_string(),
                asset_id: "asset-over".to_string(),
                attachment,
            },
        )
        .unwrap_err();
        assert!(matches!(error, MultimodalError::TooManyImages { .. }));
    }

    #[test]
    fn visual_batch_enforces_total_decoded_byte_limit() {
        let mut batch = vec![VisualInspectionInput {
            source_tool_call_id: "call-1".to_string(),
            asset_id: "asset-1".to_string(),
            attachment: ImageAttachment {
                mime_type: "image/png".to_string(),
                data_base64: String::new(),
                detail: ImageDetail::High,
                name: None,
                byte_len: MAX_TOTAL_IMAGE_BYTES,
            },
        }];
        let error = push_visual_inspection_bounded(
            &mut batch,
            VisualInspectionInput {
                source_tool_call_id: "call-2".to_string(),
                asset_id: "asset-2".to_string(),
                attachment: ImageAttachment {
                    mime_type: "image/png".to_string(),
                    data_base64: String::new(),
                    detail: ImageDetail::High,
                    name: None,
                    byte_len: 1,
                },
            },
        )
        .unwrap_err();
        assert!(matches!(error, MultimodalError::TotalTooLarge { .. }));
    }
}
