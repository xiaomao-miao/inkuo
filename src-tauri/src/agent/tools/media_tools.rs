//! Media tools: read_image, read_pdf
//!
//! These tools let the AI agent consume binary workspace files (PNG / JPG
//! / WebP / GIF / SVG, and PDF) without going through the
//! UTF-8-only `read_file`. They mirror the small surface of the
//! frontend's `read_file_for_viewer` command but expose the result
//! in an LLM-friendly format:
//!
//!   - `read_image` returns the image as a base64 data URL plus the
//!     file metadata. The agent runtime can attach the data URL as a
//!     multimodal `image_url` content part for vision-capable models.
//!   - `read_pdf` extracts the embedded text page-by-page (best-effort)
//!     so the model can read long PDFs without needing 50 MB of base64
//!     in the message. Binary extraction uses `pdf-extract` to keep the
//!     dependency surface small (pure-Rust, no native bindings).

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

use super::{ToolDefinition, ToolError, ToolParameters, validate_workspace_path};

/// Maximum image payload (bytes) the agent will load. Larger images
/// are rejected with a clear error to avoid ballooning context windows.
const MAX_IMAGE_BYTES: u64 = 20 * 1024 * 1024; // 20 MB
/// Maximum PDF size the agent will attempt to text-extract. Anything
/// larger is rejected to keep tool latency bounded.
const MAX_PDF_BYTES: u64 = 100 * 1024 * 1024; // 100 MB

fn image_mime_for(path: &Path) -> Option<&'static str> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    Some(match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        "ico" => "image/x-icon",
        "avif" => "image/avif",
        "tif" | "tiff" => "image/tiff",
        "svg" => "image/svg+xml",
        _ => return None,
    })
}

fn pdf_magic_ok(bytes: &[u8]) -> bool {
    bytes.starts_with(b"%PDF-")
}

// ─── ReadImage ───────────────────────────────────────────────────────────────

pub struct ReadImageTool;

impl ReadImageTool {
    pub fn new() -> Self { Self }
    pub fn definition(&self) -> ToolDefinition {
        ToolDefinition::new_with_label(
            "read_image",
            "读取图片",
            "Read an image file (PNG / JPG / GIF / WebP / BMP / SVG) from \
             the workspace and return a base64 data URL plus size and MIME \
             type. The agent runtime typically attaches the data URL as a \
             multimodal `image_url` content part for vision-capable models. \
             Use this for visual context — UI screenshots, diagrams, photos.",
            ToolParameters::new(
                vec!["path"],
                vec![
                    ("path", "string", Some("Absolute path to the image file to read")),
                ],
            ),
        )
    }
    pub async fn execute(&self, arguments: Value, workspace: Option<String>) -> Result<String, ToolError> {
        let path = arguments["path"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidArguments("read_image".to_string(), "path must be a string".into()))?;

        validate_workspace_path(path, &workspace)?;

        let path_buf = PathBuf::from(path);
        let mime = image_mime_for(&path_buf).ok_or_else(|| {
            ToolError::InvalidArguments(
                "read_image".to_string(),
                "unsupported image extension (expected png/jpg/gif/webp/bmp/ico/avif/tif/svg)".into(),
            )
        })?;

        let metadata = std::fs::metadata(&path)
            .map_err(|e| ToolError::IoError(format!("Failed to stat {}: {}", path, e)))?;

        if metadata.len() > MAX_IMAGE_BYTES {
            return Err(ToolError::ExecutionError(format!(
                "image too large: {} bytes (limit {})",
                metadata.len(),
                MAX_IMAGE_BYTES
            )));
        }

        let bytes = std::fs::read(&path)
            .map_err(|e| ToolError::IoError(format!("Failed to read {}: {}", path, e)))?;

        let data_base64 = BASE64.encode(&bytes);
        let data_url = format!("data:{};base64,{}", mime, data_base64);

        Ok(json!({
            "path": path,
            "name": path_buf
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default(),
            "size": metadata.len(),
            "mime": mime,
            "data_url": data_url,
            "note": "Attach data_url as an image_url content part for multimodal models."
        })
        .to_string())
    }
}

impl Default for ReadImageTool {
    fn default() -> Self { Self::new() }
}

// ─── ReadPdf ─────────────────────────────────────────────────────────────────

pub struct ReadPdfTool;

impl ReadPdfTool {
    pub fn new() -> Self { Self }
    pub fn definition(&self) -> ToolDefinition {
        ToolDefinition::new_with_label(
            "read_pdf",
            "读取 PDF",
            "Read a PDF file from the workspace and return its text content \
             page-by-page as a structured JSON object (best-effort extraction \
             of embedded text — scanned PDFs without an OCR layer return empty \
             pages and should be processed by `read_image` instead). Use this \
             for long documents that exceed what an LLM can consume as an \
             image attachment.",
            ToolParameters::new(
                vec!["path"],
                vec![
                    ("path", "string", Some("Absolute path to the .pdf file to read")),
                    ("max_pages", "integer", Some("Optional cap on the number of pages to extract (default: all)")),
                ],
            ),
        )
    }
    pub async fn execute(&self, arguments: Value, workspace: Option<String>) -> Result<String, ToolError> {
        let path = arguments["path"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidArguments("read_pdf".to_string(), "path must be a string".into()))?;

        validate_workspace_path(path, &workspace)?;

        let max_pages = arguments["max_pages"].as_u64().map(|v| v as usize);

        let metadata = std::fs::metadata(&path)
            .map_err(|e| ToolError::IoError(format!("Failed to stat {}: {}", path, e)))?;

        if metadata.len() > MAX_PDF_BYTES {
            return Err(ToolError::ExecutionError(format!(
                "pdf too large: {} bytes (limit {})",
                metadata.len(),
                MAX_PDF_BYTES
            )));
        }

        let bytes = std::fs::read(&path)
            .map_err(|e| ToolError::IoError(format!("Failed to read {}: {}", path, e)))?;

        if !pdf_magic_ok(&bytes) {
            return Err(ToolError::ExecutionError(
                "file does not look like a PDF (missing %PDF- header)".into(),
            ));
        }

        // We extract text via `pdf-extract`, which is already in the
        // dependency tree (it powers the snapshot text preview path).
        // The crate is deliberately not declared in this file's
        // Cargo.toml deps — we rely on its presence in `Cargo.toml`.
        // If the dependency is later removed, the agent's PDF reads
        // will fail with a clear error, which is preferable to a
        // silent read of zero-byte content.
        let text = match pdf_extract::extract_text_from_mem(&bytes) {
            Ok(t) => t,
            Err(e) => {
                return Err(ToolError::ExecutionError(format!(
                    "failed to extract text from PDF: {} (the PDF may be image-based; try `read_image` for OCR)",
                    e
                )));
            }
        };

        // Paginate by form-feed character (pdf-extract splits pages by
        // \x0C — see its documentation).
        let pages: Vec<String> = text
            .split('\x0C')
            .map(|p| p.trim_end_matches('\r').to_string())
            .filter(|p| !p.is_empty())
            .collect();

        let total = pages.len();
        let limited: Vec<String> = if let Some(max) = max_pages {
            pages.into_iter().take(max).collect()
        } else {
            pages
        };

        Ok(json!({
            "path": path,
            "size": metadata.len(),
            "page_count": total,
            "pages": limited,
            "truncated": max_pages.map(|m| total > m).unwrap_or(false),
        })
        .to_string())
    }
}

impl Default for ReadPdfTool {
    fn default() -> Self { Self::new() }
}
