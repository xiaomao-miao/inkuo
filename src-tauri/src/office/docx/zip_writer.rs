//! ZIP-package state machinery for the `.docx` writer.
//!
//! Owns the bookkeeping types and helpers used to splice
//! pre-existing `.docx` packages with new content:
//!   - `ImageWritePlan` / `HeaderFooterWritePlan` / `PreservedImageRef` —
//!     per-asset writer state collected while scanning the preserved zip
//!   - `HeaderFooterPart` — header/footer payload routing
//!   - `scan_preserved_zip_for_image_state` / `scan_preserved_hf_state` —
//!     cursors over the existing `[Content_Types].xml` and `.rels` files
//!   - `substitute_image_placeholders` / `substitute_hf_placeholders` —
//!     the inverse pass that rewrites the freshly-built document XML
//!     to point at the new rels ids we minted
//!   - `append_image_overrides` / `append_image_relationships` /
//!     `append_hf_overrides` / `append_hf_relationships` — incremental
//!     XML appenders for `[Content_Types].xml` and `word/_rels/document.xml.rels`
//!   - `build_header_footer_xml` — header/footer XML payload
//!   - `find_next_relationship_id` / `extract_media_target` /
//!     `extract_hf_target` / `parse_hf_basename_index` — tiny string
//!     parsers used by the scanners
//!
//! Pulled out of `mod.rs` because none of these functions touch the
//! outer `WordDocument` API — they only see XML strings + byte buffers —
//! and pulling them out shrinks the orchestrator file by ~580 lines
//! while keeping the write_word_document entry-point readable.

use std::collections::HashMap;
use std::io::Read;

use super::{HeaderPart, WordDocument};
use crate::office::shared::OfficeError;
use crate::office::docx::{build_run_xml, escape_xml, FooterPart};

/// One image's worth of writer bookkeeping: bytes, target filename inside
/// the zip, content-type, and the rels id we minted for it.
pub(crate) struct ImageWritePlan {
    pub(crate) bytes: Vec<u8>,
    /// e.g. `word/media/image1.png`.
    pub(crate) internal_path: String,
    /// e.g. `image1.png` — used for the `<Override PartName=...>` path.
    pub(crate) internal_basename: String,
    /// e.g. `image/png`.
    pub(crate) content_type: String,
    /// e.g. `rId6`.
    pub(crate) rid: String,
}

/// One header (or footer) part's worth of writer bookkeeping.
pub(crate) struct HeaderFooterWritePlan {
    /// The header or footer part from the model.
    pub(crate) part: HeaderFooterPart,
    /// e.g. `header1` or `footer2`.
    pub(crate) basename: String,
    /// e.g. `word/header1.xml`.
    pub(crate) internal_path: String,
    /// e.g. `rId6`.
    pub(crate) rid: String,
    /// The user-supplied `HeaderPart.id` (or `FooterPart.id`). This is the
    /// key the `substitute_hf_placeholders` pass uses to find
    /// `rIdHeaderPlaceholder_<id>` / `rIdFooterPlaceholder_<id>` in
    /// `document.xml` and swap in the real rId.
    pub(crate) part_id: String,
    /// Whether this is a header (false = footer).
    pub(crate) is_header: bool,
}

/// Either a header or a footer part, stored in the write plan.
#[derive(Debug, Clone)]
pub(crate) enum HeaderFooterPart {
    Header(HeaderPart),
    Footer(FooterPart),
}

/// Image reference already present in a preserved `.docx`. Re-used on
/// rewrite so the existing rId stays stable and the corresponding media
/// file (still inside the preserved zip) is what the writer points at.
#[derive(Debug, Clone)]
pub(crate) struct PreservedImageRef {
    /// e.g. `rId6`.
    pub(crate) rid: String,
    /// e.g. `media/image1.png` — target path from the preserved rels.
    pub(crate) target: String,
}

/// Look at the original `.docx` (if any) and figure out:
///  - the highest `imageN.ext` index already in `word/media/`
///  - the highest `rId` already used in `word/_rels/document.xml.rels`
///  - the list of image relationships (`Target` paths under `media/`)
///    already declared by the preserved rels file. The writer uses this
///    so when an existing image is round-tripped (`WordImage` with
///    `internal_path` set) it can reuse the original rId instead of
///    allocating a brand new one. Without this each append would alias
///    the preserved image's rId to a freshly allocated imageN.ext and
///    silently orphan every previously-present `<w:drawing>` because
///    their rIds no longer resolve to the right media file.
///
/// The first two numbers are then bumped by 1 in the caller to allocate
/// fresh, non-colliding values for *new* images. Returns
/// `(0, 6, vec![])` for a fresh document with no `preserve_from`.
pub(crate) fn scan_preserved_zip_for_image_state(
    preserve_from: Option<&[u8]>,
) -> Result<(u32, u32, Vec<PreservedImageRef>), OfficeError> {
    let mut max_media_index: u32 = 0;
    let mut max_rid: u32 = 5; // matches WORD_RELS_XML: rId1..rId5
    let mut preserved: Vec<PreservedImageRef> = Vec::new();
    let mut rels_xml: Option<String> = None;
    if let Some(bytes) = preserve_from {
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes))?;
        for i in 0..archive.len() {
            let mut file = archive.by_index(i)?;
            let name = file.name().to_string();
            if let Some(rest) = name.strip_prefix("word/media/image") {
                // rest looks like "12.png" — pull the integer prefix.
                let digits: String = rest
                    .chars()
                    .take_while(|c| c.is_ascii_digit())
                    .collect();
                if !digits.is_empty() {
                    if let Ok(n) = digits.parse::<u32>() {
                        if n > max_media_index {
                            max_media_index = n;
                        }
                    }
                }
            } else if name == "word/_rels/document.xml.rels" {
                // Read up to 1 MB of rels xml (the rels file is always
                // tiny — a few KB). We use `read_to_end` via a small
                // take() shim because ZipFile isn't `io::Read` by value
                // — we have to go through `read`.
                let mut s = String::new();
                let mut limited = file.by_ref().take(1 << 20);
                let _ = std::io::Read::read_to_string(&mut limited, &mut s);
                rels_xml = Some(s);
            }
        }
    }

    if let Some(s) = rels_xml.as_deref() {
        // Cheap rId scanner: pull every `Id="rId<digits>"`.
        // We scan the byte slice once, looking for the 4-byte
        // pattern `Id="` and then verifying the next 3 bytes are
        // `rId`. This is much more robust than sliding over the
        // raw `rId"` because attribute value boundaries vary.
        let bytes = s.as_bytes();
        let mut idx = 0;
        while idx + 8 < bytes.len() {
            if &bytes[idx..idx + 4] == b"Id=\""
                && &bytes[idx + 4..idx + 7] == b"rId"
            {
                let mut j = idx + 7;
                let mut digits = String::new();
                while j < bytes.len() && bytes[j].is_ascii_digit() {
                    digits.push(bytes[j] as char);
                    j += 1;
                }
                if !digits.is_empty() {
                    let id_str = format!("rId{}", digits);
                    if let Ok(n) = digits.parse::<u32>() {
                        if n > max_rid {
                            max_rid = n;
                        }
                    }
                    // Hunt for the sibling `Target=` attribute on the
                    // same Relationship element. We scan backwards/forwards
                    // for the nearest `Target="media/..."` substring.
                    let after_id = j;
                    // Coarse scan — find a `Target="media/<…>"` substring
                    // anywhere after the current rId but before the next
                    // `Id=` (or end of buffer).
                    let next_id = find_next_relationship_id(s, after_id);
                    let window = &s[after_id..next_id];
                    let target = extract_media_target(window);
                    if let Some(t) = target {
                        preserved.push(PreservedImageRef {
                            rid: id_str,
                            target: t,
                        });
                    }
                    idx = next_id;
                } else {
                    idx += 1;
                }
            } else {
                idx += 1;
            }
        }
    }

    Ok((max_media_index, max_rid, preserved))
}

/// Find the byte offset of the next `<… Id="…` after `from` in the
/// rels buffer, or `s.len()` when none. Used to bound the `Target=`
/// search window for one relationship at a time.
pub(crate) fn find_next_relationship_id(s: &str, from: usize) -> usize {
    let bytes = s.as_bytes();
    let mut idx = from;
    while idx + 4 < bytes.len() {
        if &bytes[idx..idx + 4] == b"Id=\"" {
            return idx;
        }
        idx += 1;
    }
    bytes.len()
}

/// Within a single `<Relationship …/>` (or `<Relationship …></Relationship>`)
/// substring, pull out the value of `Target="media/…"`. Returns `None`
/// when the relationship is not an image (`Target` doesn't start with
/// `media/`) — non-image relationships are noise from this scanner's
/// point of view.
pub(crate) fn extract_media_target(window: &str) -> Option<String> {
    let bytes = window.as_bytes();
    let needle = b"Target=\"";
    let mut idx = 0;
    while idx + needle.len() < bytes.len() {
        if &bytes[idx..idx + needle.len()] == needle {
            let start = idx + needle.len();
            if let Some(end_rel) = window[start..].find('"') {
                let target = &window[start..start + end_rel];
                let normalised = target
                    .trim_start_matches('/')
                    .strip_prefix("word/")
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| target.to_string());
                if normalised.starts_with("media/") {
                    return Some(normalised);
                }
                return None;
            }
            return None;
        }
        idx += 1;
    }
    None
}

/// Replace every `rIdImgPlaceholder` in `doc_xml` with the corresponding
/// real rels id from `image_writes`. The placeholder is intentionally
/// unique; missing rewrites show up as obvious `rIdImgPlaceholder`
/// strings in the resulting docx and would fail Word's strict rels check.
pub(crate) fn substitute_image_placeholders(doc_xml: &str, image_writes: &[ImageWritePlan]) -> String {
    let mut out = String::with_capacity(doc_xml.len());
    let mut idx = 0;
    let placeholder = "rIdImgPlaceholder";
    for plan in image_writes {
        // No-op for empty plans — the placeholder is only injected when an
        // image was actually emitted by `build_document_xml`.
        if let Some(found) = doc_xml[idx..].find(placeholder) {
            let abs = idx + found;
            out.push_str(&doc_xml[idx..abs]);
            out.push_str(&plan.rid);
            idx = abs + placeholder.len();
        } else {
            // No more placeholders to rewrite; copy the rest verbatim.
            out.push_str(&doc_xml[idx..]);
            return out;
        }
    }
    out.push_str(&doc_xml[idx..]);
    out
}

/// Append an `<Override>` row for each image's media entry. The
/// `Default Extension="png"` rows already in the base `CONTENT_TYPES_XML`
/// cover most cases, but a brand-new `.docx` (no preserved zip) should
/// still declare Overrides explicitly so Word's "missing part" check
/// doesn't reject the package.
pub(crate) fn append_image_overrides(base: &str, image_writes: &[ImageWritePlan]) -> String {
    if image_writes.is_empty() {
        return base.to_string();
    }
    // De-duplicate on `/word/media/<basename>`. Round-tripped images can
    // land in `image_writes` twice (once via `internal_path` reuse, once
    // via a fresh `DocElement::Image` sharing the same source path) —
    // emitting the same Override twice would corrupt the resulting
    // `[Content_Types].xml`.
    let mut emitted: std::collections::HashSet<String> = std::collections::HashSet::new();
    // The base ends with `</Types>`. Inject overrides right before that.
    let close = "</Types>";
    let Some(pos) = base.rfind(close) else {
        return base.to_string();
    };
    let mut out = String::with_capacity(base.len() + image_writes.len() * 128);
    out.push_str(&base[..pos]);
    for plan in image_writes {
        if !emitted.insert(plan.internal_basename.clone()) {
            continue;
        }
        out.push_str(&format!(
            "  <Override PartName=\"/word/media/{}\" ContentType=\"{}\"/>\n",
            escape_xml(&plan.internal_basename),
            escape_xml(&plan.content_type),
        ));
    }
    out.push_str(&base[pos..]);
    out
}

/// Append an `<Relationship>` row for each image's media entry. Targets
/// are relative to `word/document.xml` (i.e. just `media/image1.png`),
/// and ids are the `rid` we minted in the planning phase.
///
/// When round-tripping an existing image we re-use the original rId, so
/// the same Relationship gets re-emitted if it shows up twice in
/// `image_writes` (one entry from `internal_path` reuse, one from a fresh
/// `DocElement::Image`). We de-duplicate on the rId so the resulting
/// rels file stays valid.
pub(crate) fn append_image_relationships(base: &str, image_writes: &[ImageWritePlan]) -> String {
    if image_writes.is_empty() {
        return base.to_string();
    }
    let close = "</Relationships>";
    let Some(pos) = base.rfind(close) else {
        return base.to_string();
    };
    let mut out = String::with_capacity(base.len() + image_writes.len() * 192);
    let mut emitted_ids: std::collections::HashSet<String> = std::collections::HashSet::new();
    out.push_str(&base[..pos]);
    for plan in image_writes {
        if !emitted_ids.insert(plan.rid.clone()) {
            continue;
        }
        out.push_str(&format!(
            "  <Relationship Id=\"{}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/image\" Target=\"media/{}\"/>\n",
            escape_xml(&plan.rid),
            escape_xml(&plan.internal_basename),
        ));
    }
    out.push_str(&base[pos..]);
    out
}

// ─── Header / Footer part writing ─────────────────────────────────────────────

/// Build the XML content for a header or footer part. This is a
/// stripped-down `<w:hdr>` / `<w:ftr>` document — no `<w:body>` wrapper,
/// just the part root with paragraphs inside. Images are not supported
/// inside header/footer parts in v1 (the model still emits them as
/// plain text).
pub(crate) fn build_header_footer_xml(part: &HeaderFooterPart) -> String {
    let (paragraphs, is_header) = match part {
        HeaderFooterPart::Header(h) => (&h.paragraphs, true),
        HeaderFooterPart::Footer(f) => (&f.paragraphs, false),
    };
    let tag = if is_header { "hdr" } else { "ftr" };
    let mut xml = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:{tag} xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"
             xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"
             xmlns:inkuo="http://inkuo.app/wordprocessingml/2026/main">"#,
    );
    for para in paragraphs {
        if para.text.starts_with("<__tbl_pos_") || para.text.starts_with("<__img_pos_") {
            // Skip table/image markers inside header/footer parts — they
            // can't contain inline drawings and the body handles tables.
            continue;
        }
        xml.push_str("\n  <w:p>");
        // pPr
        let has_style = para.style.is_some();
        let has_alignment = para.alignment.is_some();
        let has_id = !para.id.is_empty();
        if has_style || has_alignment || has_id {
            xml.push_str("<w:pPr>");
            if let Some(ref s) = para.style {
                xml.push_str(&format!("<w:pStyle w:val=\"{}\"/>", escape_xml(s)));
            }
            if let Some(ref a) = para.alignment {
                if !a.is_empty() {
                    xml.push_str(&format!("<w:jc w:val=\"{}\"/>", escape_xml(a)));
                }
            }
            if has_id {
                xml.push_str(&format!("<inkuo:id w:val=\"{}\"/>", escape_xml(&para.id)));
            }
            xml.push_str("</w:pPr>");
        }
        // Runs
        if let Some(ref runs) = para.runs {
            for run in runs {
                xml.push_str(&build_run_xml(run));
            }
        } else if !para.text.is_empty() {
            xml.push_str(&format!(
                "<w:r><w:t xml:space=\"preserve\">{}</w:t></w:r>",
                escape_xml(&para.text)
            ));
        }
        xml.push_str("</w:p>");
    }
    xml.push_str(&format!("\n</w:{tag}>"));
    xml
}

/// Append `<Override>` rows for header and footer parts to
/// `[Content_Types].xml`.
pub(crate) fn append_hf_overrides(base: &str, plans: &[HeaderFooterWritePlan]) -> String {
    if plans.is_empty() {
        return base.to_string();
    }
    let close = "</Types>";
    let Some(pos) = base.rfind(close) else {
        return base.to_string();
    };
    let mut out = String::with_capacity(base.len() + plans.len() * 128);
    out.push_str(&base[..pos]);
    let tag = |plan: &HeaderFooterWritePlan| if plan.is_header { "header" } else { "footer" };
    for plan in plans {
        out.push_str(&format!(
            "  <Override PartName=\"/word/{}.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.{}+xml\"/>\n",
            escape_xml(&plan.basename),
            escape_xml(tag(plan)),
        ));
    }
    out.push_str(&base[pos..]);
    out
}

/// Append `<Relationship>` rows for header and footer parts to
/// `word/_rels/document.xml.rels`. The `Target` MUST include the
/// `.xml` extension — the corresponding zip entry is stored at
/// `word/<basename>.xml`, and OOXML requires `Target` to be the
/// part-relative path WITH the extension. Word/WPS silently drop
/// the relationship when the Target doesn't resolve, which is what
/// was producing "页眉页脚好像没有生效".
pub(crate) fn append_hf_relationships(base: &str, plans: &[HeaderFooterWritePlan]) -> String {
    if plans.is_empty() {
        return base.to_string();
    }
    let close = "</Relationships>";
    let Some(pos) = base.rfind(close) else {
        return base.to_string();
    };
    let mut out = String::with_capacity(base.len() + plans.len() * 256);
    out.push_str(&base[..pos]);
    for plan in plans {
        let rel_type = if plan.is_header {
            "http://schemas.openxmlformats.org/officeDocument/2006/relationships/header"
        } else {
            "http://schemas.openxmlformats.org/officeDocument/2006/relationships/footer"
        };
        out.push_str(&format!(
            "  <Relationship Id=\"{}\" Type=\"{}\" Target=\"{}.xml\"/>\n",
            escape_xml(&plan.rid),
            rel_type,
            escape_xml(&plan.basename),
        ));
    }
    out.push_str(&base[pos..]);
    out
}

/// Substitute the `rIdHeaderPlaceholder_<partid>` and
/// `rIdFooterPlaceholder_<partid>` tokens in doc_xml with the real
/// rIds minted by the writer. The mapping comes from `HeaderFooterWritePlan`.
pub(crate) fn substitute_hf_placeholders(
    doc_xml: &str,
    plans: &[HeaderFooterWritePlan],
) -> String {
    if plans.is_empty() {
        return doc_xml.to_string();
    }
    let mut out = doc_xml.to_string();
    for plan in plans {
        // Match the exact placeholder string emitted by `build_sectpr_xml`,
        // which uses `escape_xml(&hr.header_id)` (or footer_id) verbatim.
        // The writer substitutes the placeholders at the very end of write,
        // so we use a plain string replace on the same escaped form here.
        let placeholder = if plan.is_header {
            format!("rIdHeaderPlaceholder_{}", escape_xml(&plan.part_id))
        } else {
            format!("rIdFooterPlaceholder_{}", escape_xml(&plan.part_id))
        };
        out = out.replace(&placeholder, &plan.rid);
    }
    out
}

/// Scan the preserved zip's rels file for existing header / footer
/// relationships so we can reuse their rIds on round-trip. Returns
/// `(max_rid, preserved_hf_refs)` where each ref is a tuple of
/// `(rid, basename)` for the preserved relationship.
pub(crate) fn scan_preserved_hf_state(
    preserve_from: Option<&[u8]>,
) -> Result<(u32, u32, u32, Vec<(String, String)>), OfficeError> {
    let mut max_rid: u32 = 5;
    let mut max_header_index: u32 = 0;
    let mut max_footer_index: u32 = 0;
    let mut preserved: Vec<(String, String)> = Vec::new();
    if let Some(bytes) = preserve_from {
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes))?;
        let mut rels_xml: Option<String> = None;
        for i in 0..archive.len() {
            let mut file = archive.by_index(i)?;
            let name = file.name().to_string();
            if name == "word/_rels/document.xml.rels" {
                let mut s = String::new();
                let mut limited = (&mut file).take(1 << 20);
                let _ = std::io::Read::read_to_string(&mut limited, &mut s);
                rels_xml = Some(s);
            }
        }
        if let Some(s) = rels_xml {
            let bytes = s.as_bytes();
            let mut idx = 0;
            while idx + 8 < bytes.len() {
                if &bytes[idx..idx + 4] == b"Id=\"" && &bytes[idx + 4..idx + 7] == b"rId" {
                    let mut j = idx + 7;
                    let mut digits = String::new();
                    while j < bytes.len() && bytes[j].is_ascii_digit() {
                        digits.push(bytes[j] as char);
                        j += 1;
                    }
                    let next_id = find_next_relationship_id(&s, j);
                    let advance = if digits.is_empty() { idx + 1 } else { next_id };
                    if !digits.is_empty() {
                        if let Ok(n) = digits.parse::<u32>() {
                            if n > max_rid {
                                max_rid = n;
                            }
                        }
                        let window = &s[j..next_id];
                        if let Some((rid_str, target)) = extract_hf_target(window) {
                            // Strip the `.xml` extension so we can use the
                            // bare basename as a key for the planning
                            // lookup and also so we can recognise the
                            // numeric index (e.g. `header3` → 3).
                            let bare = target
                                .strip_suffix(".xml")
                                .unwrap_or(&target)
                                .to_string();
                            if let Some(n) = parse_hf_basename_index(&bare, true) {
                                if n > max_header_index {
                                    max_header_index = n;
                                }
                            }
                            if let Some(n) = parse_hf_basename_index(&bare, false) {
                                if n > max_footer_index {
                                    max_footer_index = n;
                                }
                            }
                            preserved.push((rid_str, bare));
                        }
                    }
                    idx = advance;
                } else {
                    idx += 1;
                }
            }
        }
    }
    Ok((max_rid, max_header_index, max_footer_index, preserved))
}

/// Given a basename like `header3` or `footer2`, return the trailing
/// integer index. `expect_header` picks which prefix to look for; the
/// other prefix is ignored so we don't accidentally double-count a
/// `header1` as a `footer1`.
pub(crate) fn parse_hf_basename_index(basename: &str, expect_header: bool) -> Option<u32> {
    let prefix = if expect_header { "header" } else { "footer" };
    let rest = basename.strip_prefix(prefix)?;
    rest.parse::<u32>().ok()
}

/// Within a Relationship element, extract the Target if it's a header/footer.
pub(crate) fn extract_hf_target(window: &str) -> Option<(String, String)> {
    // Look for Type containing "header" or "footer" and a Target attribute.
    let mut rid: Option<String> = None;
    let mut target: Option<String> = None;
    let mut is_hf = false;
    let bytes = window.as_bytes();
    let mut i = 0;
    while i + 8 < bytes.len() {
        if &bytes[i..i + 6] == b"Id=\"" {
            let mut j = i + 6;
            let mut v = Vec::new();
            while j < bytes.len() && bytes[j] != b'"' {
                v.push(bytes[j]);
                j += 1;
            }
            if let Ok(s) = String::from_utf8(v) {
                rid = Some(s);
            }
            i = j;
        } else if &bytes[i..i + 6] == b"Type=\"" {
            let mut j = i + 6;
            let mut v = Vec::new();
            while j < bytes.len() && bytes[j] != b'"' {
                v.push(bytes[j]);
                j += 1;
            }
            if let Ok(s) = String::from_utf8(v) {
                is_hf = s.contains("header") || s.contains("footer");
            }
            i = j;
        } else if &bytes[i..i + 8] == b"Target=\"" {
            let mut j = i + 8;
            let mut v = Vec::new();
            while j < bytes.len() && bytes[j] != b'"' {
                v.push(bytes[j]);
                j += 1;
            }
            if let Ok(s) = String::from_utf8(v) {
                target = Some(s);
            }
            i = j;
        } else {
            i += 1;
        }
    }
    if is_hf {
        rid.zip(target)
    } else {
        None
    }
}

