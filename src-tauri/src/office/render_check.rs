//! Optional configured-renderer-backed Office preview checker.
//!
//! When the user opts into the "render check" workflow, the writer
//! shell-execs `soffice` (LibreOffice's headless mode) to render the
//! freshly-generated `.docx` or `.pptx` to a series of PNGs. The
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
//! A renderer may be unavailable in a particular build/runtime. This module
//! only calls a configured/system renderer from an explicit preview path; it
//! never asks the user to install a dependency at runtime. Discovery returns
//! `Ok(None)` when the required programs are unavailable, so callers can
//! report truthfully that visual verification was not performed.

use crate::office::shared::OfficeError;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::process::Command;

const RENDER_COMMAND_TIMEOUT: Duration = Duration::from_secs(90);
const PROBE_COMMAND_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_RENDER_OUTPUT_PAGES: usize = 256;
const MAX_RENDER_OUTPUT_BYTES: u64 = 128 * 1024 * 1024;
const MAX_INTERMEDIATE_PDF_BYTES: u64 = 128 * 1024 * 1024;
const MAX_OFFICE_INPUT_BYTES: u64 = 64 * 1024 * 1024;

async fn command_output_with_timeout(
    command: &mut Command,
    label: &str,
    timeout: Duration,
) -> Result<std::process::Output, OfficeError> {
    command.kill_on_drop(true);
    tokio::time::timeout(timeout, command.output())
        .await
        .map_err(|_| OfficeError::Xml(format!("{} timed out after {} seconds", label, timeout.as_secs())))?
        .map_err(OfficeError::Io)
}

/// Build a local file URL without adding another URL dependency. LibreOffice
/// requires `UserInstallation` to be a URL (not a native path). Each non-URL-
/// safe UTF-8 byte is percent-encoded while path separators and Windows drive
/// colons remain readable.
fn local_file_url(path: &Path) -> String {
    use std::fmt::Write;

    let native = path.to_string_lossy().replace('\\', "/");
    let mut encoded = String::with_capacity(native.len());
    for byte in native.bytes() {
        if byte.is_ascii_alphanumeric()
            || matches!(byte, b'-' | b'.' | b'_' | b'~' | b'/' | b':')
        {
            encoded.push(byte as char);
        } else {
            let _ = write!(&mut encoded, "%{:02X}", byte);
        }
    }
    if encoded.starts_with('/') {
        format!("file://{}", encoded)
    } else {
        format!("file:///{}", encoded)
    }
}

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
    /// Number of pages/slides rendered in this result. For a bounded window
    /// this is not the total page count of the source document.
    pub page_count: u32,
    /// Wall-clock time the render took, in milliseconds.
    pub elapsed_ms: u128,
}

/// Locate LibreOffice on the system. Returns the path to the
/// `soffice` binary, or `None` if it isn't installed.
///
/// Search order:
///   1. `soffice` on `PATH` (resolved by walking `PATH` ourselves
///      rather than shelling out to `which` — fewer subprocesses)
///   2. Platform-specific known locations
///       - macOS: `/Applications/LibreOffice.app/Contents/MacOS/soffice`
///       - Linux: `/usr/bin/soffice`, `/usr/lib/libreoffice/program/soffice`
///       - Windows: `C:\Program Files\LibreOffice\program\soffice.exe`
pub async fn find_libreoffice() -> Option<PathBuf> {
    let bare_names: &[&str] = if cfg!(target_os = "windows") {
        &["soffice.exe"]
    } else {
        &["soffice"]
    };
    // Probe `PATH` directly first. tokio's `Command` does walk PATH
    // when given a bare name; we sanity-check by trying to spawn
    // `soffice --version` once before assuming it works.
    for name in bare_names {
        let mut command = Command::new(name);
        command.arg("--version");
        if let Ok(out) = command_output_with_timeout(&mut command, "soffice probe", PROBE_COMMAND_TIMEOUT).await {
            if out.status.success() || !out.stdout.is_empty() {
                return Some(PathBuf::from(name));
            }
        }
    }
    // Platform-specific fallbacks for cases where `soffice` isn't on
    // PATH but is installed in a known location.
    let candidates: Vec<PathBuf> = if cfg!(target_os = "macos") {
        vec![PathBuf::from(
            "/Applications/LibreOffice.app/Contents/MacOS/soffice",
        )]
    } else if cfg!(target_os = "windows") {
        vec![
            PathBuf::from(r"C:\Program Files\LibreOffice\program\soffice.exe"),
            PathBuf::from(r"C:\Program Files (x86)\LibreOffice\program\soffice.exe"),
        ]
    } else {
        vec![
            PathBuf::from("/usr/bin/soffice"),
            PathBuf::from("/usr/lib/libreoffice/program/soffice"),
            PathBuf::from("/snap/bin/libreoffice"),
        ]
    };
    for c in &candidates {
        if c.exists() {
            return Some(c.clone());
        }
    }
    None
}

/// Locate `pdftoppm` (Poppler) for the second stage of per-page
/// rendering. Returns the path to the binary or `None` if missing.
/// On Linux / macOS we walk `PATH`; on Windows we also check
/// `C:\Program Files\poppler*\Library\bin\pdftoppm.exe`.
pub async fn find_pdftoppm() -> Option<PathBuf> {
    let name = if cfg!(target_os = "windows") {
        "pdftoppm.exe"
    } else {
        "pdftoppm"
    };
    let mut probe = Command::new(name);
    probe.arg("-v");
    if let Ok(out) = command_output_with_timeout(&mut probe, "pdftoppm probe", PROBE_COMMAND_TIMEOUT).await {
        if out.status.success() || !out.stderr.is_empty() {
            return Some(PathBuf::from(name));
        }
    }
    if cfg!(target_os = "windows") {
        // Common install location for Poppler-on-Windows.
        let candidates = [
            r"C:\Program Files\poppler-23.11.0\Library\bin\pdftoppm.exe",
            r"C:\Program Files\poppler\Library\bin\pdftoppm.exe",
        ];
        for c in &candidates {
            let p = PathBuf::from(c);
            if p.exists() {
                return Some(p);
            }
        }
    }
    None
}

/// Render a `.docx` or `.pptx` file to PNG-per-page using LibreOffice
/// headless and Poppler's `pdftoppm`.
///
/// Pipeline:
///   1. `soffice --headless --convert-to pdf --outdir <tmp> <docx>`
///   2. `pdftoppm -r 144 -png <pdf> <tmp>/page` → `page-001.png`,
///      `page-002.png`, ...
///
/// `output_dir` is created if it doesn't exist. PNGs are named
/// `page-NNN.png` (3-digit zero-padded) so they sort naturally in
/// the file explorer. The caller is responsible for cleaning up the
/// directory afterwards.
///
/// Returns `Ok(None)` when LibreOffice isn't installed — this is
/// treated as a soft failure rather than an error so the calling
/// tool can degrade gracefully (e.g. "render check skipped").
pub async fn render_office_to_pngs(
    office_path: &Path,
    output_dir: &Path,
) -> Result<Option<RenderCheckResult>, OfficeError> {
    render_office_to_pngs_internal(
        office_path,
        output_dir,
        Some((1, MAX_RENDER_OUTPUT_PAGES)),
    )
    .await
}

/// Render only one bounded 1-based page/slide window. This is the public
/// entry point used by multimodal QA so a malicious or accidentally huge
/// Office file cannot rasterize thousands of pages into the temp directory.
pub async fn render_office_page_window_to_pngs(
    office_path: &Path,
    output_dir: &Path,
    start_page: u32,
    max_pages: usize,
) -> Result<Option<RenderCheckResult>, OfficeError> {
    if start_page == 0 || max_pages == 0 || max_pages > 16 {
        return Err(OfficeError::Xml(
            "render page window must start at page >= 1 and contain 1-16 pages".to_string(),
        ));
    }
    render_office_to_pngs_internal(
        office_path,
        output_dir,
        Some((start_page, max_pages)),
    )
    .await
}

async fn render_office_to_pngs_internal(
    office_path: &Path,
    output_dir: &Path,
    page_window: Option<(u32, usize)>,
) -> Result<Option<RenderCheckResult>, OfficeError> {
    let input_metadata = tokio::fs::metadata(office_path).await.map_err(|error| {
        OfficeError::Io(std::io::Error::new(
            error.kind(),
            format!("Office file cannot be read at {}: {}", office_path.display(), error),
        ))
    })?;
    if !input_metadata.is_file() {
        return Err(OfficeError::Xml(format!(
            "Office preview input is not a regular file: {}",
            office_path.display()
        )));
    }
    if input_metadata.len() > MAX_OFFICE_INPUT_BYTES {
        return Err(OfficeError::Xml(format!(
            "Office preview input is {} bytes; input limit is {} bytes",
            input_metadata.len(),
            MAX_OFFICE_INPUT_BYTES
        )));
    }

    let soffice = match find_libreoffice().await {
        Some(p) => p,
        None => return Ok(None),
    };
    let pdftoppm = match find_pdftoppm().await {
        Some(p) => p,
        None => return Ok(None),
    };
    tokio::fs::create_dir_all(output_dir).await?;
    let start = std::time::Instant::now();

    // Stage 1: docx → pdf. We use a separate staging sub-directory so
    // the PDF doesn't get confused with the per-page PNGs the caller
    // asked for. The PDF's basename is derived from the docx's stem
    // (soffice picks the same stem) and lives next to the PNGs in
    // `output_dir`; callers who want to clean up can just `rm -rf` the
    // whole dir.
    // A private profile prevents concurrent app sessions from contending on
    // LibreOffice's global user-profile lock and works in packaged/sandboxed
    // environments where the process home is not writable.
    let libreoffice_profile = output_dir.join(".libreoffice-profile");
    tokio::fs::create_dir_all(&libreoffice_profile).await?;
    let libreoffice_profile_url_path = if libreoffice_profile.is_absolute() {
        libreoffice_profile.clone()
    } else {
        std::env::current_dir()?.join(&libreoffice_profile)
    };
    let mut pdf_command = Command::new(&soffice);
    pdf_command
        .arg(format!(
            "-env:UserInstallation={}",
            local_file_url(&libreoffice_profile_url_path)
        ))
        .arg("--headless")
        .arg("--convert-to")
        .arg("pdf")
        .arg("--outdir")
        .arg(output_dir)
        .arg(office_path);
    let pdf_status = command_output_with_timeout(
        &mut pdf_command,
        "Office-to-PDF conversion",
        RENDER_COMMAND_TIMEOUT,
    )
    .await;
    let _ = tokio::fs::remove_dir_all(&libreoffice_profile).await;
    let pdf_status = pdf_status?;
    if !pdf_status.status.success() {
        let stderr = String::from_utf8_lossy(&pdf_status.stderr);
        return Err(OfficeError::Xml(format!(
            "soffice exited {}: {}",
            pdf_status.status, stderr
        )));
    }
    // Locate the freshly-written PDF. soffice names it after the
    // docx's basename plus `.pdf`.
    let stem = office_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("doc");
    let pdf_path = output_dir.join(format!("{}.pdf", stem));
    if !pdf_path.exists() {
        return Err(OfficeError::Xml(format!(
            "soffice did not produce expected PDF at {}",
            pdf_path.display()
        )));
    }
    let pdf_size = tokio::fs::metadata(&pdf_path).await?.len();
    if pdf_size > MAX_INTERMEDIATE_PDF_BYTES {
        return Err(OfficeError::Xml(format!(
            "intermediate PDF is {} bytes; render limit is {} bytes",
            pdf_size, MAX_INTERMEDIATE_PDF_BYTES
        )));
    }

    // Stage 2: pdf → page-NNN.png. We pass the PDF's full path and
    // a `page` stem; pdftoppm appends `-NNN.png` itself, so the
    // resulting files are `page-1.png`, `page-2.png`, etc. We then
    // rename them to zero-padded `page-001.png` for ergonomic
    // sorting.
    let page_prefix = output_dir.join("page");
    let mut png_command = Command::new(&pdftoppm);
    png_command.arg("-r").arg("144").arg("-png");
    if let Some((start_page, max_pages)) = page_window {
        let end_page = start_page.saturating_add(max_pages.saturating_sub(1) as u32);
        png_command
            .arg("-f")
            .arg(start_page.to_string())
            .arg("-l")
            .arg(end_page.to_string());
    }
    png_command.arg(&pdf_path).arg(&page_prefix);
    let png_status = command_output_with_timeout(
        &mut png_command,
        "PDF-to-PNG conversion",
        RENDER_COMMAND_TIMEOUT,
    )
    .await?;
    if !png_status.status.success() {
        let stderr = String::from_utf8_lossy(&png_status.stderr);
        return Err(OfficeError::Xml(format!(
            "pdftoppm exited {}: {}",
            png_status.status, stderr
        )));
    }
    // Rename `page-1.png` → `page-001.png` so they sort lexically.
    // Poppler numbers pages starting at 1; we discover the actual
    // count from the directory listing rather than parsing pdftoppm
    // stdout.
    let mut entries = tokio::fs::read_dir(output_dir).await?;
    let mut page_files: Vec<PathBuf> = Vec::new();
    while let Some(entry) = entries.next_entry().await? {
        let p = entry.path();
        if let Some(fname) = p.file_name().and_then(|s| s.to_str()) {
            if fname.starts_with("page-") && fname.ends_with(".png") && !fname.contains("__")
            // not our zero-padded rename target
            {
                page_files.push(p);
            }
        }
    }
    page_files.sort_by_key(|path| {
        path.file_stem()
            .and_then(|stem| stem.to_str())
            .and_then(|stem| stem.strip_prefix("page-"))
            .and_then(|number| number.parse::<u32>().ok())
            .unwrap_or(u32::MAX)
    });
    let output_page_limit = page_window
        .map(|(_, max_pages)| max_pages)
        .unwrap_or(MAX_RENDER_OUTPUT_PAGES);
    if page_files.len() > output_page_limit {
        return Err(OfficeError::Xml(format!(
            "renderer produced {} pages; output limit is {}",
            page_files.len(), output_page_limit
        )));
    }
    let mut pages: Vec<RenderedPage> = Vec::with_capacity(page_files.len());
    let mut output_bytes = 0u64;
    for p in &page_files {
        let num = rendered_page_number(p).ok_or_else(|| {
            OfficeError::Xml(format!("unexpected rendered page filename: {}", p.display()))
        })?;
        let new_name = format!("page-{:03}.png", num);
        let new_path = output_dir.join(&new_name);
        // Best-effort rename; if the target already exists skip.
        if !new_path.exists() {
            let _ = tokio::fs::rename(p, &new_path).await;
        }
        let final_path = if new_path.exists() {
            new_path
        } else {
            p.clone()
        };
        let meta = tokio::fs::metadata(&final_path).await?;
        output_bytes = output_bytes.saturating_add(meta.len());
        if output_bytes > MAX_RENDER_OUTPUT_BYTES {
            return Err(OfficeError::Xml(format!(
                "rendered PNG output exceeds {} bytes",
                MAX_RENDER_OUTPUT_BYTES
            )));
        }
        // Try to read PNG dimensions. PNG header is 8 bytes signature
        // + IHDR chunk (4 length + 4 type + 13 data). Width / height
        // are at offsets 16 and 20 (big-endian u32).
        let (width, height) = read_png_dimensions(&final_path).unwrap_or((0, 0));
        pages.push(RenderedPage {
            path: final_path,
            page_number: num,
            width,
            height,
            byte_size: meta.len(),
        });
    }
    let page_count = pages.len() as u32;
    let elapsed_ms = start.elapsed().as_millis();
    // Best-effort: clean up the intermediate PDF — callers asked for
    // PNGs, not PDFs. We don't fail if rm doesn't work.
    let _ = tokio::fs::remove_file(&pdf_path).await;
    Ok(Some(RenderCheckResult {
        output_dir: output_dir.to_path_buf(),
        pages,
        page_count,
        elapsed_ms,
    }))
}

fn rendered_page_number(path: &Path) -> Option<u32> {
    path.file_stem()?
        .to_str()?
        .strip_prefix("page-")?
        .parse()
        .ok()
}

/// Backwards-compatible Word-specific wrapper retained for existing writer
/// and test call sites. New visual-inspection flows should use
/// [`render_office_to_pngs`] so their `.pptx` support is explicit.
pub async fn render_docx_to_pngs(
    docx_path: &Path,
    output_dir: &Path,
) -> Result<Option<RenderCheckResult>, OfficeError> {
    render_office_to_pngs(docx_path, output_dir).await
}

/// Read width / height from a PNG's IHDR chunk. Returns `None` for
/// files that aren't valid PNGs (or whose IHDR is too short).
fn read_png_dimensions(path: &Path) -> Option<(u32, u32)> {
    use std::io::Read;

    // Dimensions live in the first 24 bytes. Avoid loading a multi-megabyte
    // rendered page a second time just to inspect its IHDR.
    let mut bytes = [0u8; 24];
    std::fs::File::open(path).ok()?.read_exact(&mut bytes).ok()?;
    // PNG signature is 8 bytes; IHDR is the first chunk and starts at
    // offset 8 with a 4-byte length prefix and 4-byte type ("IHDR").
    // Width and height follow at offsets 16 and 20 (big-endian u32).
    if &bytes[0..8] != b"\x89PNG\r\n\x1a\n" {
        return None;
    }
    if &bytes[12..16] != b"IHDR" {
        return None;
    }
    let w = u32::from_be_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]);
    let h = u32::from_be_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]);
    Some((w, h))
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
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};
    thread_local! { static CNT: AtomicU64 = AtomicU64::new(0); }
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let cnt = CNT.with(|c| c.fetch_add(1, Ordering::Relaxed));
    format!("{}{}", now.as_nanos(), cnt)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::office::write_sample_document;
    use std::path::PathBuf;

    #[test]
    fn rendered_page_numbers_are_parsed_without_lexical_sorting() {
        assert_eq!(rendered_page_number(Path::new("page-2.png")), Some(2));
        assert_eq!(rendered_page_number(Path::new("page-010.png")), Some(10));
        assert_eq!(rendered_page_number(Path::new("preview-1.png")), None);
    }

    #[test]
    fn libreoffice_profile_path_is_a_percent_encoded_file_url() {
        assert_eq!(
            local_file_url(Path::new("/tmp/preview profile/测试")),
            "file:///tmp/preview%20profile/%E6%B5%8B%E8%AF%95"
        );
        assert_eq!(
            local_file_url(Path::new(r"C:\Preview Folder\profile")),
            "file:///C:/Preview%20Folder/profile"
        );
    }

    #[tokio::test]
    async fn page_window_is_rejected_before_any_external_renderer_probe() {
        let missing = Path::new("does-not-need-to-exist.docx");
        let output = Path::new("unused-output");
        assert!(render_office_page_window_to_pngs(missing, output, 0, 1)
            .await
            .is_err());
        assert!(render_office_page_window_to_pngs(missing, output, 1, 17)
            .await
            .is_err());
    }

    #[tokio::test]
    async fn oversized_input_is_rejected_before_any_external_renderer_probe() {
        let tmp_dir = std::env::temp_dir().join(format!(
            "inkuo-render-size-limit-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&tmp_dir).expect("create size-limit test directory");
        let input = tmp_dir.join("oversized.docx");
        let file = std::fs::File::create(&input).expect("create sparse Office input");
        file.set_len(MAX_OFFICE_INPUT_BYTES + 1)
            .expect("extend sparse Office input");

        let error = render_office_to_pngs(&input, &tmp_dir.join("preview"))
            .await
            .expect_err("oversized Office input must be rejected");
        assert!(error.to_string().contains("input limit"));
        let _ = std::fs::remove_dir_all(tmp_dir);
    }

    /// End-to-end: write the bundled sample document, render it to
    /// per-page PNGs, and verify that we got at least one page with
    /// reasonable dimensions. Skips silently if LibreOffice /
    /// `pdftoppm` aren't installed (the soft-failure path).
    #[tokio::test]
    async fn smoke_render_sample_document() {
        let tmp_dir = std::env::temp_dir().join(format!(
            "inkuo-render-check-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        let _ = tokio::fs::remove_dir_all(&tmp_dir).await;
        tokio::fs::create_dir_all(&tmp_dir)
            .await
            .expect("create tmp dir");
        let docx_path = tmp_dir.join("sample.docx");
        write_sample_document(&docx_path).expect("write_sample_document");
        let png_dir = tmp_dir.join("png");
        let result = render_docx_to_pngs(&docx_path, &png_dir)
            .await
            .expect("render_docx_to_pngs");
        if result.is_none() {
            // LibreOffice or pdftoppm not installed — skip.
            eprintln!("LibreOffice or pdftoppm not installed; skipping render-check assertion.");
            let _ = tokio::fs::remove_dir_all(&tmp_dir).await;
            return;
        }
        let result = result.unwrap();
        assert!(
            result.page_count >= 1,
            "expected at least 1 rendered page; got {}",
            result.page_count
        );
        for page in &result.pages {
            assert!(
                page.byte_size > 100,
                "page {} suspiciously small",
                page.page_number
            );
            // The sample should produce pages with reasonable
            // dimensions at 144 DPI. A4 portrait at 144 DPI is
            // approximately 1190x1684 pixels. We use a wide lower
            // bound so the test is robust to DPI changes.
            if page.width > 0 && page.height > 0 {
                assert!(
                    page.width >= 600 && page.height >= 600,
                    "page {} dimensions {}x{} look too small",
                    page.page_number,
                    page.width,
                    page.height
                );
            }
        }
        let _ = tokio::fs::remove_dir_all(&tmp_dir).await;
    }

    /// `find_libreoffice` should locate at least one valid candidate
    /// on a developer machine (we don't fail when it can't — the
    /// function returns `Option` precisely because LibreOffice is
    /// optional). Just exercise the path.
    #[tokio::test]
    async fn find_libreoffice_returns_a_path_when_installed() {
        // We don't assert `Some` because the test environment might
        // genuinely lack LibreOffice; we just assert the call
        // doesn't panic and returns a clean `Option`.
        let result = find_libreoffice().await;
        match result {
            Some(p) => {
                let pb: PathBuf = p;
                eprintln!("found soffice at {}", pb.display());
            }
            None => eprintln!("soffice not found (test env OK)"),
        }
    }
}
