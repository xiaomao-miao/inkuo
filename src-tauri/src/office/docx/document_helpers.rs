//! Document-tree helpers for the `.docx` writer.
//!
//! Owns the pure-data helpers used by `read_word_document` and the
//! `Write` entry-points to introspect the model and walk preserved
//! XML/rels blobs:
//!   - `doc_has_numbering` — is there at least one paragraph with a
//!     `NumberingRef`? Drives whether `word/numbering.xml` is emitted.
//!   - `parse_image_xml` — recover `<w:drawing>` images from a
//!     preserved document/rels blob (paired with the writer's
//!     `PreservedImageRef` reuse pass).
//!   - `flush_image` / `parse_image_rels` — image manifest helpers.
//!   - `attr_value_str` — quick-xml attribute accessor used by both
//!     the reader and the writer (kept here because it's pure and
//!     has no business state coupling).
//!
//! Pulled out of `mod.rs` so the entry-points stay short — these
//! helpers collectively accounted for ~225 lines of unrelated logic
//! in the middle of the file.

use std::collections::HashMap;

use quick_xml::events::BytesStart;

use super::{WordDocument, WordImage, WordParagraph};
use crate::office::shared::{OfficeError, TableCell};

/// True when the document contains at least one paragraph with a numbering
/// reference. Used to decide whether `word/numbering.xml` should be emitted.
pub(crate) fn doc_has_numbering(doc: &WordDocument) -> bool {
    doc.paragraphs.iter().any(|p| p.numbering.is_some())
}

/// Recover existing `<w:drawing>` images from the document so the model
/// can re-emit them on the next save. Without this, appending a *new*
/// image to a docx that already embeds an older one would silently drop
/// the older image's `<w:drawing>` and its relationship — the picture
/// bytes would still be inside the zip, but Word would have no idea
/// where they belong.
///
/// Strategy:
///   1. Parse `word/_rels/document.xml.rels` once to build a
///      rId → relative-path (e.g. `media/image3.png`) lookup.
///   2. Walk `word/document.xml` for every `<a:blip
///      r:embed="rIdN"/>` element. For each, also pick up the
///      neighbouring `<wp:extent cx="..." cy="..."/>` for the EMU size
///      and scan the same enclosing paragraph for an `<inkuo:id
///      w:val="__img_pos_<img_id>__"/>` marker. The marker id is the
///      stable id the writer uses to pair this drawing with its
///      `WordImage` entry. When the marker is missing we synthesise a
///      fresh id from the rId so we still surface the picture to the
///      model.
///
/// Every recovered entry sets `internal_path = Some(...)` so the writer
/// knows to reuse the existing zip bytes and rId instead of allocating
/// a new `imageN.ext`.
pub(crate) fn parse_image_xml(
    doc_content: &str,
    rels_content: &str,
    _image_markers: &[WordParagraph],
) -> Vec<WordImage> {
    let rid_to_target = parse_image_rels(rels_content);
    if rid_to_target.is_empty() {
        return Vec::new();
    }

    let mut reader = quick_xml::Reader::from_str(doc_content);
    reader.config_mut().trim_text(false);
    let mut buf = Vec::new();

    // Per-drawing state. We reset these at the start of each
    // `<w:drawing>` element.
    let mut in_drawing = false;
    let mut blip_rid: Option<String> = None;
    let mut cx: u32 = 0;
    let mut cy: u32 = 0;

    // Per-paragraph state. The writer decorates every image-bearing
    // paragraph with a `<inkuo:id w:val="__img_pos_<img_id>__"/>`; we
    // capture it here so the recovered entry uses the same stable id
    // the writer will key off on the next save. The id may appear
    // *before* the `<w:drawing>` child element inside `<w:pPr>` (Start
    // tag) or right at the start of the paragraph (Empty tag).
    let mut current_para_id: Option<String> = None;
    let mut current_para_depth = 0usize;

    let mut images: Vec<WordImage> = Vec::new();

    loop {
        let event = reader.read_event_into(&mut buf);
        match event {
            Ok(quick_xml::events::Event::Start(ref e)) | Ok(quick_xml::events::Event::Empty(ref e)) => {
                let name = e.local_name();
                let is_empty = matches!(event, Ok(quick_xml::events::Event::Empty(_)));
                if name.as_ref() == b"p" {
                    current_para_depth += 1;
                    current_para_id = None;
                } else if name.as_ref() == b"id" && current_para_depth > 0 {
                    // inkuo:id inside the paragraph — could be the marker.
                    if let Some(v) = attr_value_str(e, b"val") {
                        if !v.is_empty() {
                            current_para_id = Some(v);
                        }
                    }
                } else if name.as_ref() == b"drawing" {
                    in_drawing = true;
                    blip_rid = None;
                    cx = 0;
                    cy = 0;
                } else if in_drawing && name.as_ref() == b"extent" {
                    if let Some(v) = attr_value_str(e, b"cx") {
                        if let Ok(n) = v.parse::<u32>() {
                            cx = n;
                        }
                    }
                    if let Some(v) = attr_value_str(e, b"cy") {
                        if let Ok(n) = v.parse::<u32>() {
                            cy = n;
                        }
                    }
                } else if in_drawing && name.as_ref() == b"blip" {
                    if let Some(v) = attr_value_str(e, b"embed") {
                        blip_rid = Some(v);
                    }
                    if is_empty {
                        // `<a:blip ... />` is usually self-closing — flush
                        // the recovery record now.
                        flush_image(&mut images, &blip_rid, cx, cy, current_para_id.as_deref(), &rid_to_target);
                        in_drawing = false;
                        blip_rid = None;
                    }
                }
            }
            Ok(quick_xml::events::Event::End(ref e)) => {
                let name = e.local_name();
                if name.as_ref() == b"drawing" {
                    // Empty / non-self-closing blip flush. `<a:blip />`
                    // cases were flushed inside the Start handler.
                    flush_image(&mut images, &blip_rid, cx, cy, current_para_id.as_deref(), &rid_to_target);
                    in_drawing = false;
                    blip_rid = None;
                } else if name.as_ref() == b"p" && current_para_depth > 0 {
                    current_para_depth -= 1;
                    if current_para_depth == 0 {
                        current_para_id = None;
                    }
                }
            }
            Ok(quick_xml::events::Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
        buf.clear();
    }

    images
}

/// Push one recovered `WordImage` onto `images` if `blip_rid` resolves
/// to a media entry we recognise. Centralises the policy so the
/// self-closing and balanced-tag code paths stay in lockstep.
pub(crate) fn flush_image(
    images: &mut Vec<WordImage>,
    blip_rid: &Option<String>,
    cx: u32,
    cy: u32,
    para_id: Option<&str>,
    rid_to_target: &std::collections::HashMap<String, String>,
) {
    let Some(rid) = blip_rid.as_deref() else {
        return;
    };
    let Some(target) = rid_to_target.get(rid) else {
        return;
    };
    let internal_path = format!("word/{}", target);
    let img_id = para_id
        .and_then(|p| p.strip_prefix("__img_pos_"))
        .and_then(|rest| rest.strip_suffix("__"))
        .map(|s| s.to_string())
        .unwrap_or_else(|| format!("_recovered_{}", rid));
    images.push(WordImage {
        id: img_id,
        path: internal_path.clone(),
        width_emu: cx,
        height_emu: cy,
        internal_path: Some(internal_path),
    });
}

/// Parse `word/_rels/document.xml.rels` and return a map from
/// `rIdN` → `media/imageN.ext` (the path is kept *relative* to `word/`
/// so the writer can prepend the prefix when needed).
pub(crate) fn parse_image_rels(rels_content: &str) -> std::collections::HashMap<String, String> {
    let mut map = std::collections::HashMap::new();
    let mut reader = quick_xml::Reader::from_str(rels_content);
    reader.config_mut().trim_text(false);
    let mut buf = Vec::new();
    loop {
        let event = reader.read_event_into(&mut buf);
        match event {
            Ok(quick_xml::events::Event::Start(ref e)) | Ok(quick_xml::events::Event::Empty(ref e)) => {
                if e.local_name().as_ref() == b"Relationship" {
                    let id = attr_value_str(e, b"Id").unwrap_or_default();
                    let target = attr_value_str(e, b"Target").unwrap_or_default();
                    let ty = attr_value_str(e, b"Type").unwrap_or_default();
                    if id.is_empty() || target.is_empty() {
                        continue;
                    }
                    // Only image relationships carry forward — styles,
                    // settings, etc. must not enter the image rels map.
                    if !ty.contains("/image") && !ty.contains("/chart") {
                        continue;
                    }
                    if !target.starts_with("media/") && !target.starts_with("/word/media/") {
                        continue;
                    }
                    let normalised = target
                        .trim_start_matches('/')
                        .strip_prefix("word/")
                        .map(|s| s.to_string())
                        .unwrap_or(target);
                    map.insert(id.to_string(), normalised);
                }
            }
            Ok(quick_xml::events::Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
        buf.clear();
    }
    map
}

/// Pull a `String` out of a quick-xml attribute. Convenience wrapper
/// over the file's existing `attr_value` (which returns a `Cow<[u8]>`)
/// for callers that already know they want an owned `String`.
pub(crate) fn attr_value_str(e: &quick_xml::events::BytesStart<'_>, attr: &[u8]) -> Option<String> {
    for a in e.attributes().with_checks(false).flatten() {
        let key = a.key.as_ref();
        // quick_xml emits `inkuo:id` (namespaced) in the doc but raw
        // `Id` / `cx` in the rels file, so match on either the full or
        // the local part of the key.
        let local = key
            .iter()
            .position(|&b| b == b':')
            .map(|i| &key[i + 1..])
            .unwrap_or(key);
        if local == attr {
            return Some(String::from_utf8_lossy(&a.value).into_owned());
        }
    }
    None
}
