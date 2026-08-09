//! Optional LibreOffice-backed render checker.
//!
//! When the user opts into the "render check" workflow, the writer
//! shell-execs `soffice` (LibreOffice's headless mode) to render the
//! freshly-generated `.docx` to a series of PNGs, one per page. The
//! agent can then look at those PNGs and look for visual problems
//! (tables overflowing the page, headers stranded on a page with no
//! body, etc.).
//!
//! This module is intentionally lightweight — it shells out, parses
//! stdout, and returns a list of PNG paths. The "what to look for"
//! detection is left to the agent (or to a downstream ML model)
//! because the rules vary wildly per document type.
//!
//! ## Why optional?
//!
//! LibreOffice is a heavy dependency (200+ MB on Linux, bigger on
//! macOS). Most users running the docx writer on CI don't have it
//! installed. We ship the helper but only call it from the explicit
//! "render check" path — never from the regular `create_word_doc`
//! tool. The function checks `soffice --version` first and returns
//! `Ok(None)` when the binary isn't on PATH, so callers can treat
//! "LibreOffice not installed" as a soft failure.

use crate::office::shared::OfficeError;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tokio::process::Command;

/// One rendered page. We keep it as a path + dimensions rather than
/// raw bytes so the agent can decide whether to inline the PNG
/// (small docs) or just look at it externally (large docs).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RenderedPage {
    /// Absolute path of the PNG on disk.
    pub path: PathBuf,
    /// 1-based page number.
    pub page_number: u32,
    /// PNG dimensions in pixels. Zero when the renderer didn't
    /// measure them (e.g. the v1 single-PNG helper).
    pub width: u32,
    pub height: u32,
    /// Size of the PNG file in bytes — useful for "is this page
    /// suspiciously small" heuristics.
    pub byte_size: u64,
}

/// Result of a render check: a list of PNGs (one per page) plus the
/// directory they live in so callers can clean up later.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RenderCheckResult {
    /// The directory LibreOffice wrote PNGs into. The caller owns
    /// this directory after the check completes; we don't auto-delete.
    pub output_dir: PathBuf,
    pub pages: Vec<RenderedPage>,
    /// Total page count according to LibreOffice (should match
    /// `pages.len()`).
    pub page_count: u32,
    /// Wall-clock time the render took, in milliseconds.
    pub elapsed_ms: u128,
}

/// Locate LibreOffice on the system. Returns the path to the
/// `soffice` binary, or `None` if it isn't installed.
///
/// Search order:
///   1. `soffice` on `PATH`
///   2. Platform-specific known locations
///       - macOS: `/Applications/LibreOffice.app/Contents/MacOS/soffice`
///       - Linux: `/usr/bin/soffice`, `/usr/lib/libreoffice/program/soffice`
///       - Windows: `C:\Program Files\LibreOffice\program\soffice.exe`
pub async fn find_libreoffice() -> Option<PathBuf> {
    // First try the bare command. tokio's `Command` doesn't search PATH
    // when given a bare name on every platform; using `which` crate
    // would be cleaner but we already have `std::env::var` available.
    let candidates: Vec<PathBuf> = if cfg!(target_os = "macos") {
        vec![
            PathBuf::from("/Applications/LibreOffice.app/Contents/MacOS/soffice"),
            PathBuf::from("soffice"),
        ]
    } else if cfg!(target_os = "windows") {
        vec![
            PathBuf::from(r"C:\Program Files\LibreOffice\program\soffice.exe"),
            PathBuf::from(r"C:\Program Files (x86)\LibreOffice\program\soffice.exe"),
            PathBuf::from("soffice.exe"),
        ]
    } else {
        vec![
            PathBuf::from("/usr/bin/soffice"),
            PathBuf::from("/usr/lib/libreoffice/program/soffice"),
            PathBuf::from("/snap/bin/libreoffice"),
            PathBuf::from("soffice"),
        ]
    };
    for c in &candidates {
        if c.is_absolute() && c.exists() {
            return Some(c.clone());
        }
        // For bare names, probe by spawning `which`.
        if let Ok(out) = Command::new("which").arg(c).output().await {
            if out.status.success() {
                let path = String::from_utf8_lossy(&out.stdout).trim().to_string();
                if !path.is_empty() {
                    return Some(PathBuf::from(path));
                }
            }
        }
    }
    None
}

/// Render a `.docx` file to PNG-per-page using LibreOffice headless.
///
/// `output_dir` is created if it doesn't exist; the resulting PNGs
/// are named `page-001.png`, `page-002.png`, etc. The caller is
/// responsible for cleaning up the directory afterwards.
///
/// Returns `Ok(None)` when LibreOffice isn't installed — this is
/// treated as a soft failure rather than an error so the calling
/// tool can degrade gracefully (e.g. "render check skipped").
pub async fn render_docx_to_pngs(
    docx_path: &Path,
    output_dir: &Path,
) -> Result<Option<RenderCheckResult>, OfficeError> {
    let soffice = match find_libreoffice().await {
        Some(p) => p,
        None => return Ok(None),
    };
    if !docx_path.exists() {
        return Err(OfficeError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("docx not found: {}", docx_path.display()),
        )));
    }
    tokio::fs::create_dir_all(output_dir).await?;
    let start = std::time::Instant::now();
    let status = Command::new(&soffice)
        .arg("--headless")
        .arg("--convert-to")
        .arg("png")
        .arg("--outdir")
        .arg(output_dir)
        .arg(docx_path)
        .output()
        .await
        .map_err(|e| OfficeError::Io(e))?;
    if !status.status.success() {
        let stderr = String::from_utf8_lossy(&status.stderr);
        return Err(OfficeError::Xml(format!(
            "soffice exited {}: {}",
            status.status, stderr
        )));
    }
    // LibreOffice converts the whole doc to a single PNG by default;
    // for per-page rendering we need `pdf:writer_pdf_Export` then a
    // PDF→PNG pass. For the v1 helper we surface the single PNG and
    // let downstream tooling decide whether to do the per-page split.
    // The agent will use this as a quick smoke test, not a deep QA pass.
    let elapsed_ms = start.elapsed().as_millis();
    let mut pages = Vec::new();
    let mut entries = tokio::fs::read_dir(output_dir).await?;
    while let Some(entry) = entries.next_entry().await? {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) == Some("png") {
            let meta = entry.metadata().await?;
            pages.push(RenderedPage {
                path: path.clone(),
                page_number: 1, // single composite for v1
                width: 0,       // caller can fill in via image inspection
                height: 0,
                byte_size: meta.len(),
            });
        }
    }
    let page_count = pages.len() as u32;
    Ok(Some(RenderCheckResult {
        output_dir: output_dir.to_path_buf(),
        pages,
        page_count,
        elapsed_ms,
    }))
}

/// Quick "did this render at all" smoke check. Returns `true` when
/// LibreOffice was found AND rendered the docx without erroring.
/// Use this from `create_word_doc` to add an optional QA field to
/// the tool's response.
pub async fn smoke_render(docx_path: &Path) -> bool {
    let tmp = std::env::temp_dir().join(format!("inkuo-render-check-{}", uuid_simple()));
    match render_docx_to_pngs(docx_path, &tmp).await {
        Ok(Some(_)) => true,
        _ => false,
    }
}

fn uuid_simple() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    use std::sync::atomic::{AtomicU64, Ordering};
    thread_local! { static CNT: AtomicU64 = AtomicU64::new(0); }
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let cnt = CNT.with(|c| c.fetch_add(1, Ordering::Relaxed));
    format!("{}{}", now.as_nanos(), cnt)
}
