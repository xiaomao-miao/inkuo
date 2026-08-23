//! Render Word/PowerPoint files into real page pixels for multimodal QA.
//!
//! The renderer is deliberately a first-class tool instead of prompt-only
//! advice: a specialist calls it after writing a `.docx`/`.pptx`, this module
//! registers the selected PNG pages as workspace-owned assets, and the agent
//! loop sends those assets to the very next provider request after all tool
//! results in the batch have been appended.

use super::{asset_registry, validate_workspace_path, ToolDefinition, ToolError, ToolParameters};
use crate::office::{render_office_page_window_to_pngs, RenderedPage};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::time::Instant;

const MAX_PREVIEW_PAGES: usize = 8;
const MAX_PREVIEW_PAGE_BYTES: usize = 12 * 1024 * 1024;
const MAX_PREVIEW_TOTAL_BYTES: usize = 32 * 1024 * 1024;
const MAX_OFFICE_INPUT_BYTES: u64 = 64 * 1024 * 1024;

pub struct RenderOfficePreviewTool;

impl RenderOfficePreviewTool {
    pub fn definition(&self) -> ToolDefinition {
        ToolDefinition::new_with_label(
            "render_office_preview",
            "视觉检查 Office 文件",
            "Render a workspace .docx or .pptx into actual PNG page/slide pixels with the app's configured renderer and queue them for the next multimodal model iteration. Use after creating or modifying the file; structural inspection is not a substitute. If no renderer is configured, fail clearly and never ask the user to install dependencies. Inspect at most 8 pages per call; use start_page for later batches.",
            ToolParameters::new(
                vec!["path"],
                vec![
                    (
                        "path",
                        "string",
                        Some("Absolute path to a workspace .docx or .pptx file."),
                    ),
                    (
                        "start_page",
                        "integer",
                        Some("Optional 1-based first page/slide (default 1)."),
                    ),
                    (
                        "max_pages",
                        "integer",
                        Some("Optional number of pages/slides to inspect (1-8, default 8)."),
                    ),
                ],
            ),
        )
    }

    pub async fn execute(
        &self,
        arguments: Value,
        workspace: Option<String>,
    ) -> Result<String, ToolError> {
        let workspace_root = workspace
            .as_deref()
            .filter(|path| !path.trim().is_empty())
            .ok_or_else(|| {
                ToolError::PathValidationError(
                    "render_office_preview requires a non-empty active workspace".to_string(),
                )
            })?;
        let canonical_workspace = std::fs::canonicalize(workspace_root).map_err(|error| {
            ToolError::PathValidationError(format!(
                "Workspace path does not exist: {} ({})",
                workspace_root, error
            ))
        })?;
        let workspace_boundary = Some(canonical_workspace.to_string_lossy().to_string());

        let raw_path = arguments
            .get("path")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                ToolError::InvalidArguments(
                    "render_office_preview".to_string(),
                    "path must be a string".to_string(),
                )
            })?;
        let requested_path = Path::new(raw_path);
        let resolved_path = if requested_path.is_absolute() {
            requested_path.to_path_buf()
        } else {
            canonical_workspace.join(requested_path)
        };
        let canonical_path = std::fs::canonicalize(&resolved_path).map_err(|error| {
            ToolError::PathValidationError(format!(
                "Office file '{}' cannot be resolved: {}",
                raw_path, error
            ))
        })?;
        validate_workspace_path(&canonical_path.to_string_lossy(), &workspace_boundary)?;
        validate_office_extension(&canonical_path)?;
        let input_metadata = std::fs::metadata(&canonical_path)
            .map_err(|error| ToolError::IoError(error.to_string()))?;
        if !input_metadata.is_file() || input_metadata.len() > MAX_OFFICE_INPUT_BYTES {
            return Err(ToolError::ExecutionError(format!(
                "Office preview input must be a regular file no larger than {} bytes",
                MAX_OFFICE_INPUT_BYTES
            )));
        }

        let start_page =
            bounded_page_argument(&arguments, "start_page", 1, u32::MAX as u64)? as u32;
        let max_pages = bounded_page_argument(
            &arguments,
            "max_pages",
            MAX_PREVIEW_PAGES as u64,
            MAX_PREVIEW_PAGES as u64,
        )? as usize;

        let output_dir =
            std::env::temp_dir().join(format!("inkuo-office-preview-{}", uuid::Uuid::new_v4()));
        let outcome = async {
            // Render one extra sentinel page so `has_more_pages` can be
            // reported without rasterizing an unbounded document/deck.
            let rendered = render_office_page_window_to_pngs(
                &canonical_path,
                &output_dir,
                start_page,
                max_pages.saturating_add(1),
            )
                .await
                .map_err(|error| ToolError::ExecutionError(error.to_string()))?
                .ok_or_else(|| {
                    ToolError::ExecutionError(
                        "No Office preview renderer is configured for this build/runtime. No visual verification was performed; do not ask the user to install a dependency."
                            .to_string(),
                    )
                })?;

            let has_more_pages = rendered.pages.len() > max_pages;
            let selected: Vec<RenderedPage> = rendered
                .pages
                .iter()
                .filter(|page| page.page_number >= start_page)
                .take(max_pages)
                .cloned()
                .collect();
            if selected.is_empty() {
                return Err(ToolError::InvalidArguments(
                    "render_office_preview".to_string(),
                    format!(
                        "the renderer returned no page/slide at requested start_page {}",
                        start_page
                    ),
                ));
            }

            let mut page_payloads = Vec::with_capacity(selected.len());
            let mut total_bytes = 0usize;
            for page in selected {
                let bytes = tokio::fs::read(&page.path)
                    .await
                    .map_err(|error| ToolError::IoError(error.to_string()))?;
                validate_png_payload(page.page_number, &bytes, &mut total_bytes)?;
                page_payloads.push((page, bytes));
            }

            let mut visual_assets = Vec::with_capacity(page_payloads.len());
            for (page, bytes) in page_payloads {
                let asset_id = asset_registry::insert(
                    asset_registry::fresh_id(),
                    asset_registry::AssetEntry {
                        mime: "image/png".to_string(),
                        ext: "png".to_string(),
                        data: bytes,
                        inserted_at: Instant::now(),
                        source_path: format!(
                            "{}#page={}",
                            canonical_path.display(),
                            page.page_number
                        ),
                        workspace_root: canonical_workspace.to_string_lossy().to_string(),
                    },
                );
                visual_assets.push(json!({
                    "asset_id": asset_id,
                    "asset_ref": asset_registry::reference(&asset_id),
                    "page_number": page.page_number,
                    "width": page.width,
                    "height": page.height,
                    "size_bytes": page.byte_size,
                }));
            }

            let selected_count = visual_assets.len() as u32;
            let last_page = start_page.saturating_add(selected_count.saturating_sub(1));
            let next_start_page = has_more_pages.then_some(last_page.saturating_add(1));
            serde_json::to_string(&json!({
                "source_path": canonical_path,
                "selected_start_page": start_page,
                "selected_page_count": selected_count,
                "has_more_pages": has_more_pages,
                "next_start_page": next_start_page,
                "visual_assets": visual_assets,
                "visual_inspection_queued": true,
                "instruction": "The actual page pixels will be attached to the next model iteration. Inspect clipping, overlap, legibility, hierarchy, alignment, spacing, contrast, and cross-page consistency before claiming visual verification.",
            }))
            .map_err(|error| ToolError::ExecutionError(error.to_string()))
        }
        .await;

        // Pixel bytes have moved into the bounded asset registry; temporary
        // render files are no longer needed and never enter the workspace.
        let _ = tokio::fs::remove_dir_all(&output_dir).await;
        outcome
    }
}

fn validate_office_extension(path: &Path) -> Result<(), ToolError> {
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if matches!(extension.as_str(), "docx" | "pptx") {
        Ok(())
    } else {
        Err(ToolError::InvalidArguments(
            "render_office_preview".to_string(),
            "path must identify a .docx or .pptx file".to_string(),
        ))
    }
}

fn bounded_page_argument(
    arguments: &Value,
    name: &str,
    default: u64,
    maximum: u64,
) -> Result<u64, ToolError> {
    let value = arguments
        .get(name)
        .and_then(Value::as_u64)
        .unwrap_or(default);
    if value == 0 || value > maximum {
        return Err(ToolError::InvalidArguments(
            "render_office_preview".to_string(),
            format!("{} must be an integer from 1 to {}", name, maximum),
        ));
    }
    Ok(value)
}

fn validate_png_payload(
    page_number: u32,
    bytes: &[u8],
    total_bytes: &mut usize,
) -> Result<(), ToolError> {
    if !bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Err(ToolError::ExecutionError(format!(
            "rendered page {} is not a valid PNG",
            page_number
        )));
    }
    if bytes.len() > MAX_PREVIEW_PAGE_BYTES {
        return Err(ToolError::ExecutionError(format!(
            "rendered page {} is {} bytes; per-page visual limit is {} bytes",
            page_number,
            bytes.len(),
            MAX_PREVIEW_PAGE_BYTES
        )));
    }
    *total_bytes = total_bytes.saturating_add(bytes.len());
    if *total_bytes > MAX_PREVIEW_TOTAL_BYTES {
        return Err(ToolError::ExecutionError(format!(
            "selected rendered pages total {} bytes; visual batch limit is {} bytes",
            *total_bytes, MAX_PREVIEW_TOTAL_BYTES
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_word_and_powerpoint_only() {
        assert!(validate_office_extension(Path::new("paper.docx")).is_ok());
        assert!(validate_office_extension(Path::new("deck.PPTX")).is_ok());
        assert!(validate_office_extension(Path::new("sheet.xlsx")).is_err());
    }

    #[test]
    fn page_arguments_are_strictly_bounded() {
        assert_eq!(
            bounded_page_argument(&json!({}), "max_pages", 8, 8).unwrap(),
            8
        );
        assert!(bounded_page_argument(&json!({"max_pages": 0}), "max_pages", 8, 8).is_err());
        assert!(bounded_page_argument(&json!({"max_pages": 9}), "max_pages", 8, 8).is_err());
    }

    #[test]
    fn png_payload_budget_is_enforced_before_registry_insertion() {
        let mut total = MAX_PREVIEW_TOTAL_BYTES;
        let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
        png.push(0);
        assert!(validate_png_payload(1, &png, &mut total).is_err());
    }
}
