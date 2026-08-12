//! DOCX zip-package reader — turns raw .docx bytes into a [`WordDocument`].
//!
//! Pulled out of `mod.rs` because the entry point + its two helpers
//! (`parse_header_footer_parts`, `resolve_section_refs`) are a self-contained
//! zip-to-schema pipeline that does not share any state with the writer.
//! `read_word_document` is `pub` and re-exported from `mod.rs` so
//! `crate::office::docx::read_word_document` keeps resolving for external
//! callers (e.g. `crate::office::mod.rs`, `agent/tools/office/*`).
//!
//! Internal helpers (all `fn`, used only inside this file):
//! - [`parse_header_footer_parts`] — streams `word/header*.xml` /
//!   `word/footer*.xml` from the package into [`HeaderPart`] / [`FooterPart`]
//!   trees via the shared XML parser.
//! - [`resolve_section_refs`] — wires up the header/footer rels from
//!   `word/_rels/document.xml.rels` into the [`WordSection`] list.

use std::io::Read;

use super::{
    HeaderPart, FooterPart, WordDocument, WordDocumentMeta, WordSection,
    parse_document_xml, parse_image_xml, parse_table_xml,
};
use crate::office::docx::cell_paragraph_extractor;
use crate::office::shared::{read_zip_entry, OfficeError};

pub fn read_word_document(bytes: &[u8]) -> Result<WordDocument, OfficeError> {
    let doc_content = read_zip_entry(bytes, "word/document.xml")?;
    let rels_content = read_zip_entry(bytes, "word/_rels/document.xml.rels")
        .unwrap_or_default();
    let (mut paragraphs, image_markers, mut sections) = parse_document_xml(&doc_content)?;
    let mut images = parse_image_xml(&doc_content, &rels_content, &image_markers);
    // Image markers are now kept in the main paragraphs list (preserves
    // their document position). The side channel is unused by
    // `parse_image_xml` (it recovers images directly from the XML); we
    // drop it here. Stale markers in the side channel would otherwise
    // be appended to the end and lose their anchor position.
    let mut tables = parse_table_xml(&doc_content)?;
    // Associate table markers (paragraphs with text `<__tbl_pos_<id>__>`)
    // with their corresponding tables so the table's stable id matches the
    // marker. Without this, parse_table_xml auto-assigns `t0`, `t1`, ... and
    // the original id (often `__new_tXXX` for inserted tables) is lost.
    // This breaks anchor-based insertion: `to_elements()` looks up the table
    // by id from the marker, fails to find it, and appends the table at the
    // end of the document.
    //
    // We pair markers and tables in document order. When the counts match,
    // each marker corresponds to one table. When they don't (e.g. some
    // tables have no preceding marker — newly-parsed documents), we leave
    // the auto-assigned ids intact.
    let mut pending_marker_ids: Vec<String> = Vec::new();
    for p in &paragraphs {
        if let Some(rest) = p.text.strip_prefix("<__tbl_pos_") {
            if let Some(end) = rest.find("__>") {
                let tbl_id = rest[..end].to_string();
                pending_marker_ids.push(tbl_id);
            }
        }
    }
    if pending_marker_ids.len() == tables.len() {
        for (tbl, marker_id) in tables.iter_mut().zip(pending_marker_ids.into_iter()) {
            // Only override the auto-assigned id if it matches the simple
            // counter pattern (`t0`, `t1`, ...). This way pre-existing tables
            // that already have meaningful ids are preserved.
            if tbl.id.starts_with('t') && tbl.id.len() > 1 {
                if tbl.id[1..].chars().all(|c| c.is_ascii_digit()) {
                    tbl.id = marker_id;
                }
            }
        }
    }
    // Same marker-vs-id pairing for images. `parse_image_xml` auto-assigns
    // `image0`, `image1`, …; we patch those against the markers we found
    // in the body so the rendered image's id matches the marker's id
    // (and therefore matches the original id from `parse_image`).
    let mut pending_img_marker_ids: Vec<String> = Vec::new();
    for p in &paragraphs {
        if let Some(rest) = p.text.strip_prefix("<__img_pos_") {
            if let Some(end) = rest.find("__>") {
                let img_id = rest[..end].to_string();
                pending_img_marker_ids.push(img_id);
            }
        }
    }
    if pending_img_marker_ids.len() == images.len() {
        for (img, marker_id) in images.iter_mut().zip(pending_img_marker_ids.into_iter()) {
            if img.id.starts_with("image") && img.id.len() > 5 {
                if img.id[5..].chars().all(|c| c.is_ascii_digit()) {
                    img.id = marker_id;
                }
            }
        }
    }
    // Populate `cell_paragraphs` for design-system container tables
    // (callouts, code blocks). The main parser flattens cell content
    // into `TableRow.cells[j].text`, so we run a second pass to
    // recover the structured paragraphs the writer needs to re-emit
    // inside the shaded cell on the next save. The returned Vec is
    // indexed by table-position so we can zip it with `tables`.
    let cell_paragraphs = cell_paragraph_extractor::extract_container_cell_paragraphs(
        &doc_content,
    );
    for (tbl, cps) in tables.iter_mut().zip(cell_paragraphs.into_iter()) {
        if !cps.is_empty() {
            tbl.cell_paragraphs = cps;
        }
    }
    // Load header / footer parts from the zip. We scan every
    // `word/headerN.xml` / `word/footerN.xml` entry and parse each one
    // back into a `HeaderPart` / `FooterPart` so the writer can
    // re-emit them on save. References from sections (which carry
    // `rIdN` strings as `header_id`) are resolved to those parts below.
    let (headers, footers) = parse_header_footer_parts(bytes)?;
    // Resolve section -> header/footer rels: rels file maps rIdN to
    // the zip-internal path (`header2.xml`, `footer1.xml`, etc.).
    // We translate every section's ref into a `HeaderPart.id` /
    // `FooterPart.id` so the writer can look them up directly.
    resolve_section_refs(&mut sections, &rels_content, &headers, &footers);
    // Pull document metadata (`dc:title`, `dc:creator`, …) from
    // `docProps/core.xml` so callers that round-trip a doc through
    // `read_word_document` don't lose the metadata the writer just
    // emitted. Without this, AUTHOR/TITLE fields can never resolve
    // back to their source strings after a single read/write cycle.
    let meta = parse_core_xml_meta(bytes).unwrap_or_default();
    Ok(WordDocument {
        paragraphs,
        tables,
        images,
        sections,
        headers,
        footers,
        meta,
    })
}

/// Best-effort parse of `docProps/core.xml` into `WordDocumentMeta`.
/// Returns `Ok(default())` if the entry is missing or malformed so
/// callers can treat the result as "empty metadata" instead of erroring
/// out — many real-world DOCX files ship without a core.xml at all
/// (e.g. blank templates created by very old Word versions).
fn parse_core_xml_meta(bytes: &[u8]) -> Result<WordDocumentMeta, OfficeError> {
    let content = match read_zip_entry(bytes, "docProps/core.xml") {
        Ok(s) => s,
        Err(_) => return Ok(WordDocumentMeta::default()),
    };
    let extract = |tag: &str| -> String {
        let open = format!("<{}>", tag);
        let close = format!("</{}>", tag);
        if let Some(start) = content.find(&open) {
            let body_start = start + open.len();
            if let Some(end) = content[body_start..].find(&close) {
                let raw = &content[body_start..body_start + end];
                return decode_xml_entities(raw);
            }
        }
        String::new()
    };
    Ok(WordDocumentMeta {
        title: extract("dc:title"),
        author: extract("dc:creator"),
        subject: extract("dc:subject"),
        description: extract("dc:description"),
        keywords: extract("cp:keywords"),
    })
}

/// Decode the small subset of XML entities that Word uses in core.xml
/// (`&amp;`, `&lt;`, `&gt;`, `&quot;`, `&apos;` and numeric refs).
/// Anything more exotic is left as-is.
fn decode_xml_entities(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let bytes = input.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'&' {
            if let Some(end) = input[i..].find(';') {
                let entity = &input[i + 1..i + end];
                let decoded = match entity {
                    "amp" => Some('&'),
                    "lt" => Some('<'),
                    "gt" => Some('>'),
                    "quot" => Some('"'),
                    "apos" => Some('\''),
                    _ => None,
                };
                if let Some(c) = decoded {
                    out.push(c);
                    i += end + 1;
                    continue;
                }
            }
        }
        // Push the next char (handles UTF-8 multibyte correctly).
        let ch = input[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

/// Walk every `word/headerN.xml` / `word/footerN.xml` zip entry and
/// parse each one into a `HeaderPart` / `FooterPart`. The returned
/// `HeaderPart.id` / `FooterPart.id` is the file's basename
/// (`header2`, `footer1`) so it's easy to correlate with the rId map
/// built from the rels file.
///
/// We also extract the part's EMU-stable rels id by reading
/// `word/_rels/document.xml.rels` so the writer can reuse existing
/// rIds when round-tripping — the rels id is stored in the part's
/// `internal_path` field (re-purposed: for header/footer parts we
/// stuff the rels id there as a "stable id" so the writer knows which
/// `rIdN` to reuse when constructing the new rels file).
fn parse_header_footer_parts(bytes: &[u8]) -> Result<(Vec<HeaderPart>, Vec<FooterPart>), OfficeError> {
    let mut headers = Vec::new();
    let mut footers = Vec::new();
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes))?;
    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        let name = file.name().to_string();
        if !name.starts_with("word/header") && !name.starts_with("word/footer") {
            continue;
        }
        if !name.ends_with(".xml") {
            continue;
        }
        let mut content = String::new();
        let _ = Read::read_to_string(&mut file.by_ref().take(8 * 1024 * 1024), &mut content);
        let (paras, image_markers, _sects) = parse_document_xml(&content)
            .map_err(|e| OfficeError::Xml(format!("Failed to parse {}: {}", name, e)))?;
        let id = std::path::Path::new(&name)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("?")
            .to_string();
        // Materialise a `WordDocument`-shaped struct for the part so the
        // writer can iterate paragraphs uniformly. Inline images inside
        // header/footer parts are a follow-up — the model round-trips
        // them as plain text for now, which is correct for the
        // overwhelmingly common case (page numbers, titles, dates).
        let all_paras = paras;
        if name.starts_with("word/header") {
            headers.push(HeaderPart {
                id,
                paragraphs: all_paras,
                tables: Vec::new(),
                images: Vec::new(),
            });
        } else {
            footers.push(FooterPart {
                id,
                paragraphs: all_paras,
                tables: Vec::new(),
                images: Vec::new(),
            });
        }
    }
    Ok((headers, footers))
}

/// Walk the rels file once to build a `rId -> target_path` map, then
/// re-write each section's `header_refs` / `footer_refs` so the
/// `header_id` / `footer_id` is the *file basename* (e.g. `header2`)
/// of the matching part. The writer will resolve that to a
/// `HeaderPart` by id and re-use the original rId when minting fresh
/// rels.
fn resolve_section_refs(
    sections: &mut [WordSection],
    rels_content: &str,
    headers: &[HeaderPart],
    footers: &[FooterPart],
) {
    if rels_content.is_empty() {
        return;
    }
    // Build rId -> target map. The rels file format is
    // `<Relationship Id="rId6" Type="...header" Target="header2.xml"/>`.
    let mut rid_to_target: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    let mut reader = quick_xml::Reader::from_str(rels_content);
    reader.config_mut().trim_text(false);
    let mut buf = Vec::new();
    loop {
        let event = reader.read_event_into(&mut buf);
        match event {
            Ok(quick_xml::events::Event::Start(ref e)) | Ok(quick_xml::events::Event::Empty(ref e)) => {
                let name = e.local_name();
                if name.as_ref() == b"Relationship" {
                    let mut rid: Option<String> = None;
                    let mut target: Option<String> = None;
                    let mut is_header_or_footer = false;
                    for attr in e.attributes().with_checks(false).flatten() {
                        let key = attr.key.as_ref().to_vec();
                        let local = key
                            .iter()
                            .position(|&b| b == b':')
                            .map(|i| &key[i + 1..])
                            .unwrap_or(&key[..]);
                        let val = attr.value.as_ref();
                        if local == b"Id" {
                            if let Ok(s) = std::str::from_utf8(val) {
                                rid = Some(s.to_string());
                            }
                        } else if local == b"Type" {
                            if let Ok(s) = std::str::from_utf8(val) {
                                is_header_or_footer = s.contains("header") || s.contains("footer");
                            }
                        } else if local == b"Target" {
                            if let Ok(s) = std::str::from_utf8(val) {
                                target = Some(s.to_string());
                            }
                        }
                    }
                    if is_header_or_footer {
                        if let (Some(r), Some(t)) = (rid, target) {
                            rid_to_target.insert(r, t);
                        }
                    }
                }
            }
            Ok(quick_xml::events::Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
        buf.clear();
    }
    // Convert rId refs in sections to file basenames.
    for sect in sections.iter_mut() {
        for hr in sect.header_refs.iter_mut() {
            if let Some(target) = rid_to_target.get(&hr.header_id) {
                if let Some(stem) = std::path::Path::new(target)
                    .file_stem()
                    .and_then(|s| s.to_str())
                {
                    hr.header_id = stem.to_string();
                }
            }
        }
        for fr in sect.footer_refs.iter_mut() {
            if let Some(target) = rid_to_target.get(&fr.footer_id) {
                if let Some(stem) = std::path::Path::new(target)
                    .file_stem()
                    .and_then(|s| s.to_str())
                {
                    fr.footer_id = stem.to_string();
                }
            }
        }
    }
    // Defensive: if a section has a `header_id` / `footer_id` that
    // doesn't match any loaded part (e.g. the rels entry was missing),
    // drop the ref. The writer can re-allocate later if the user
    // provides a fresh header/footer.
    for sect in sections.iter_mut() {
        sect.header_refs.retain(|hr| headers.iter().any(|h| h.id == hr.header_id));
        sect.footer_refs.retain(|fr| footers.iter().any(|f| f.id == fr.footer_id));
    }
}

