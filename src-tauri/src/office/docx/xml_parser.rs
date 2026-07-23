//! Streaming XML parser for `word/document.xml`.
//!
//! This file used to live inside `mod.rs` (the canonical ~4 800-line
//! document module). The biggest responsibility in the file — turning the
//! OOXML `<w:document>` stream into `WordParagraph` + `WordSection`
//! records — was 1 000+ lines on its own, with its own helper types
//! (`RunFormat`, attribute appliers, field-instruction parsing). Splitting
//! the parser out means future changes to font / highlight / field
//! handling touch one focused file instead of scrolling past the
//! type definitions and writer helpers in `mod.rs`.
//!
//! Public surface limited to what `mod.rs` itself needs:
//!
//! - [`parse_document_xml`] is consumed only inside `read_word_document`
//!   and the header/footer rescan path; we expose it `pub(crate)`.
//! - Everything else (the `RunFormat` struct, the per-attribute appliers,
//!   the field-instruction parser) stays private to this module.

use crate::office::shared::OfficeError;
use crate::office::docx::{
    FieldRef, FontRun, FooterPartRef, HeaderPartRef, NumberingRef, PageMargins, PageSize,
    WordParagraph, WordSection,
};

/// Convenience: same as the file's `attr_value` (which returns a `Cow<[u8]>`)
/// for callers that already know they want an owned `String`. Pulled into the
/// parser module because every call site lives inside `parse_document_xml`;
/// the writer doesn't need it.

// ── Attribute / run helpers ─────────────────────────────────────────────────────

fn attr_value_str(e: &quick_xml::events::BytesStart<'_>, attr: &[u8]) -> Option<String> {
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

#[derive(Debug, Clone, Default)]
struct RunFormat {
    bold: bool,
    italic: bool,
    underline: bool,
    strikethrough: bool,
    font_size: Option<u32>,
    color: Option<String>,
    font_name: Option<String>,
    highlight: Option<String>,
}

/// Apply a single `<w:rPr>` attribute to a `RunFormat`.
/// `tag` is the local element name (e.g. "b", "i", "u", "color", "sz").
/// When `tag` is "color" or "rFonts" or "sz" / "szCs", `attr_val` carries the attribute.
fn apply_run_attr(fmt: &mut RunFormat, tag: &[u8], attr_val: Option<&[u8]>) {
    match tag {
        b"b" | b"bCs" => fmt.bold = true,
        b"i" | b"iCs" => fmt.italic = true,
        b"u" => fmt.underline = true,
        b"strike" => fmt.strikethrough = true,
        b"highlight" => {
            if let Some(v) = attr_val {
                if let Ok(s) = std::str::from_utf8(v) {
                    if !s.is_empty() {
                        fmt.highlight = Some(s.to_string());
                    }
                }
            }
        }
        b"color" => {
            if let Some(v) = attr_val {
                if let Ok(s) = std::str::from_utf8(v) {
                    // Strip leading '#' if present so output is plain hex.
                    let s = s.trim_start_matches('#');
                    if !s.is_empty() {
                        fmt.color = Some(s.to_string());
                    }
                }
            }
        }
        b"sz" | b"szCs" => {
            if let Some(v) = attr_val {
                if let Ok(s) = std::str::from_utf8(v) {
                    if let Ok(n) = s.parse::<u32>() {
                        fmt.font_size = Some(n);
                    }
                }
            }
        }
        b"rFonts" => {
            // ascii / hAnsi / cs are all valid carriers of the font name.
            if let Some(v) = attr_val {
                if let Ok(s) = std::str::from_utf8(v) {
                    if !s.is_empty() {
                        fmt.font_name = Some(s.to_string());
                    }
                }
            }
        }
        _ => {}
    }
}

/// Walk attributes of an `<w:rPr>` (or any other) start event and apply them to `fmt`.
/// Recognized attributes map to their element siblings — this is the standard OOXML
/// compact form: `<w:b w:val="true"/>` instead of `<w:b><w:val .../></w:b>`.
fn apply_run_attrs_from_event(fmt: &mut RunFormat, e: &quick_xml::events::BytesStart) {
    for attr in e.attributes().with_checks(false).flatten() {
        let key = attr.key.as_ref().to_vec();
        let local = key
            .iter()
            .position(|&b| b == b':')
            .map(|i| &key[i + 1..])
            .unwrap_or(&key[..]);
        let val = attr.value.as_ref();
        apply_run_attr(fmt, local, Some(val));
    }
    // `w:val="false"` / `w:val="0"` should explicitly disable the flag.
    if let Some(val_attr) = e.attributes().with_checks(false).flatten().find(|a| {
        let k = a.key.as_ref();
        k.ends_with(b":val") || k == b"val"
    }) {
        let v = val_attr.value.as_ref();
        let is_off = v == b"false" || v == b"0" || v == b"off";
        if is_off {
            let key = val_attr.key.as_ref().to_vec();
            let local = key
                .iter()
                .position(|&b| b == b':')
                .map(|i| &key[i + 1..])
                .unwrap_or(&key[..]);
            match local {
                b"b" | b"bCs" => fmt.bold = false,
                b"i" | b"iCs" => fmt.italic = false,
                b"u" => fmt.underline = false,
                b"strike" => fmt.strikethrough = false,
                _ => {}
            }
        }
    }
}

fn parse_run_attrs_from_nested(e: &quick_xml::events::BytesStart, fmt: &mut RunFormat) {
    apply_run_attrs_from_event(fmt, e);
}

/// Extract the "val" attribute from a `<w:color w:val="...">` / `<w:sz w:val="...">` / `<w:rFonts w:ascii="...">` etc.
fn attr_value<'a>(e: &'a quick_xml::events::BytesStart, name: &[u8]) -> Option<std::borrow::Cow<'a, [u8]>> {
    for attr in e.attributes().with_checks(false).flatten() {
        let key = attr.key.as_ref().to_vec();
        let local = key
            .iter()
            .position(|&b| b == b':')
            .map(|i| &key[i + 1..])
            .unwrap_or(&key[..]);
        if local == name {
            return Some(std::borrow::Cow::Owned(attr.value.into_owned()));
        }
    }
    None
}

/// Normalise OOXML's `w:textDirection` vocabulary into the friendlier set
/// the writer/tool accept on input. OOXML uses `lrTb`, `tbRl`, `btLr`,
/// `lrTbV`, `tbRlV`, `btLrV`; we keep the directional form and pass
/// through any value the AI sends verbatim — both vocabularies are
/// accepted on the write side.
fn normalise_text_direction(v: &str) -> String {
    match v {
        "lrTb" => "horizontal".to_string(),
        "tbRl" => "verticalRightToLeft".to_string(),
        "btLr" => "verticalRightToLeft".to_string(),
        "lrTbV" => "rotate90".to_string(),
        "tbRlV" => "vertical".to_string(),
        "btLrV" => "verticalLeftToRight".to_string(),
        other => other.to_string(),
    }
}

/// Parse the raw `<w:instrText>` payload of a Word field code into a
/// `FieldRef`. We recognise a small set of well-known field names by
/// exact match; anything else falls back to `FieldRef::Custom` so the
/// model can still surface / round-trip arbitrary fields.
///
/// The instr text Word emits looks like ` PAGE \\* MERGEFORMAT ` —
/// with leading / trailing whitespace and a `\* MERGEFORMAT` switch
/// that's safe to ignore. We strip both before matching.
fn parse_field_instr(instr: &str) -> Option<FieldRef> {
    let trimmed = instr.trim();
    // Strip well-known formatting switches (`\* ...`). Real documents
    // add them to mark fields as auto-recalculating; we don't model
    // them and they don't change the field's meaning.
    let cleaned = trimmed
        .split_whitespace()
        .take_while(|tok| !tok.starts_with("\\*"))
        .collect::<Vec<_>>()
        .join(" ");
    let head = cleaned.split_whitespace().next().unwrap_or("").to_string();
    let rest = cleaned.get(head.len()..).unwrap_or("").trim().to_string();
    match head.to_uppercase().as_str() {
        "PAGE" => Some(FieldRef::Page),
        "NUMPAGES" => Some(FieldRef::NumPages),
        "SECTIONPAGES" => Some(FieldRef::SectionPages),
        "SECTION" => Some(FieldRef::Section),
        "DATE" => Some(FieldRef::Date { format: parse_date_format(&rest) }),
        "TIME" => Some(FieldRef::Time { format: parse_date_format(&rest) }),
        "AUTHOR" => Some(FieldRef::Author),
        "TITLE" => Some(FieldRef::Title),
        "FILENAME" => {
            // `FILENAME \p` means "path only, no extension". Default is
            // filename with extension.
            let with_ext = !rest.split_whitespace().any(|t| t == "\\p");
            Some(FieldRef::Filename { with_ext })
        }
        "" => None,
        other => Some(FieldRef::Custom { instr: other.to_string() + " " + &rest }),
    }
}

/// Extract a date/time format pattern from a `\@ "..."` or `\@ ...`
/// switch. Returns `None` when the field has no explicit format (Word
/// then uses the locale default).
fn parse_date_format(rest: &str) -> Option<String> {
    // Word stores it as `\@ "yyyy-MM-dd"` or `\@ yyyy-MM-dd`.
    let mut parts = rest.split_whitespace();
    while let Some(tok) = parts.next() {
        if tok == "\\@" {
            let next = parts.next()?.to_string();
            return Some(next.trim_matches('"').to_string());
        }
    }
    None
}

// ── Main parser ──────────────────────────────────────────────────────────────────

pub(crate) fn parse_document_xml(content: &str) -> Result<(Vec<WordParagraph>, Vec<WordParagraph>, Vec<WordSection>), OfficeError> {
    let mut paragraphs = Vec::new();
    let mut reader = quick_xml::Reader::from_str(content);
    reader.config_mut().trim_text(false);

    let mut buf = Vec::new();

    // ── Top-level state ────────────────────────────────────────────────────
    let mut para_depth = 0usize;
    let mut tbl_cell_depth = 0usize;
    let mut para_counter = 0usize;

    // ── Per-paragraph state (reset on each <w:p>) ──────────────────────────
    let mut current_text = String::new();
    let mut current_style: Option<String> = None;
    let mut current_runs: Vec<FontRun> = Vec::new();
    let mut current_numbering: Option<NumberingRef> = None;
    let mut current_stable_id: Option<String> = None;
    let mut in_numpr = false;
    let mut pending_num_id: Option<u32> = None;
    let mut pending_ilvl: Option<u32> = None;
    let mut is_table_marker = false;  // Tracks if current paragraph is a table position marker
    let mut is_image_marker = false;  // Tracks if current paragraph is an image position marker
    // NEW: paragraph-level alignment and text direction. Captured from
    // `<w:jc>` and `<w:textDirection>` inside `<w:pPr>`.
    let mut current_alignment: Option<String> = None;
    let mut current_text_direction: Option<String> = None;
    // NEW: set while we're inside a paragraph's `<w:pPr>` (so we know
    // `<w:jc>`/`<w:textDirection>` belong to this paragraph's properties
    // and not to some other context).
    let mut in_ppr = false;
    // NEW: section-break tracking. Sections can be declared at the body
    // level (`<w:body><w:sectPr>…</w:sectPr></w:body>`) — that's the
    // trailing section — or inline inside a paragraph's `<w:pPr>`
    // (`<w:p><w:pPr><w:sectPr>…</w:sectPr></w:pPr></w:p>`) which is a
    // "next-page section break" inside the body. We collect each one and
    // pair it with header/footer rels after the loop.
    let mut pending_sectpr: Option<WordSection> = None;
    let mut pending_sectpr_id_counter: u32 = 0;

    // ── Image marker state ──────────────────────────────────────────────────
    // Image-bearing paragraphs are emitted as `<w:p><w:pPr><inkuo:id
    // w:val="__img_pos_<img_id>__"/></w:pPr><w:r>...drawing...</w:r></w:p>`
    // by the writer. On read we want to round-trip them back into a
    // `WordParagraph` (id = `__img_pos_<img_id>__`, text = same shape) so
    // the writer can re-emit the drawing next time. We also stash the
    // marker paragraphs separately so `parse_image_xml` can pull the
    // image id out without the marker being double-counted as a regular
    // paragraph.
    let mut image_markers: Vec<WordParagraph> = Vec::new();

    // ── Per-run state (reset on each <w:r>) ────────────────────────────────
    let mut in_run = false;
    let mut in_run_props = false;
    let mut current_run_text = String::new();
    let mut current_run_format = RunFormat::default();
    // NEW: per-run vertAlign and field. `vert_align` comes from
    // `<w:vertAlign w:val="superscript"/>` inside `<w:rPr>`. `field` comes
    // from a `<w:fldChar>` / `<w:instrText>` triplet and is committed when
    // the run closes.
    let mut current_run_vert_align: Option<String> = None;
    let mut current_run_field: Option<FieldRef> = None;
    // NEW: `<w:fldChar>` state machine. fldCharType can be `begin`,
    // `separate`, or `end`. We accumulate `<w:instrText>` between begin
    // and separate, then treat the run text between separate and end as
    // the cached field result.
    let mut fld_state: u8 = 0; // 0 = no field, 1 = between begin & separate, 2 = between separate & end
    let mut fld_instr_buf: String = String::new();
    let mut fld_cached_text: String = String::new();

    // Track whether this paragraph actually saw any run (even an empty one).
    // We use this to decide whether to keep the paragraph even if it ended up
    // textless — see "preserve empty paragraphs" below.
    let mut paragraph_saw_run = false;

    // Collect every section we encounter. We finalise into the
    // `WordDocument.sections` field after the loop.
    let mut sections: Vec<WordSection> = Vec::new();
    // Track which header/footer rIds each section references. The mapping
    // rid -> (kind, target_path) is built by `read_word_document` from
    // `word/_rels/document.xml.rels`. For now we just collect the refs.
    let mut pending_section_header_refs: Vec<HeaderPartRef> = Vec::new();
    let mut pending_section_footer_refs: Vec<FooterPartRef> = Vec::new();
    // We also collect the in-progress `WordSection` being built.
    let mut in_sectpr = false;
    // Tracks whether we're currently between `<w:body>` and the matching
    // `</w:body>`. Used to recognise the body-level trailing
    // `<w:sectPr>` (which lives directly under `<w:body>`, NOT under any
    // `<w:pPr>`). Without this flag, body-level section properties are
    // silently dropped during read-back — `WordDocument.sections` ends
    // up empty even though `document.xml` had a perfectly valid
    // `<w:body><w:sectPr>…</w:sectPr></w:body>`.
    let mut in_body = false;

    loop {
        let event = reader.read_event_into(&mut buf);
        match event {
            Ok(quick_xml::events::Event::Start(ref e)) => {
                let name = e.local_name();
                if name.as_ref() == b"tc" {
                    tbl_cell_depth += 1;
                } else if name.as_ref() == b"body" {
                    in_body = true;
                } else if name.as_ref() == b"p" {
                    para_depth += 1;
                    current_text.clear();
                    current_style = None;
                    current_runs.clear();
                    current_numbering = None;
                    current_stable_id = None;
                    is_table_marker = false;  // Reset marker detection for new paragraph
                    is_image_marker = false; // Reset image-marker flag for new paragraph
                    in_numpr = false;
                    pending_num_id = None;
                    pending_ilvl = None;
                    paragraph_saw_run = false;
                    current_alignment = None;
                    current_text_direction = None;
                    in_ppr = false;
                    current_run_vert_align = None;
                    current_run_field = None;
                } else if name.as_ref() == b"r" && tbl_cell_depth == 0 {
                    // Only top-level runs count toward the paragraph's `runs` list.
                    in_run = true;
                    in_run_props = false;
                    current_run_text.clear();
                    current_run_format = RunFormat::default();
                } else if name.as_ref() == b"rPr" && in_run {
                    in_run_props = true;
                    // `<w:rPr>` itself can carry attributes (compact form).
                    parse_run_attrs_from_nested(e, &mut current_run_format);
                } else if in_run_props {
                    // `<w:b/>`, `<w:color w:val="..."/>`, `<w:sz w:val="24"/>` etc.
                    // Use the "compact" attributes path for val-bearing tags.
                    let val = attr_value(e, b"val");
                    let ascii = attr_value(e, b"ascii");
                    let hansi = attr_value(e, b"hAnsi");
                    let cs = attr_value(e, b"cs");
                    apply_run_attr(&mut current_run_format, name.as_ref(), val.as_deref());
                    if let Some(v) = ascii.or(hansi).or(cs) {
                        apply_run_attr(&mut current_run_format, b"rFonts", Some(v.as_ref()));
                    }
                } else if name.as_ref() == b"t" && in_run {
                    if let Ok(quick_xml::events::Event::Text(t)) = reader.read_event_into(&mut buf) {
                        current_run_text.push_str(&t.unescape().unwrap_or_default());
                    }
                } else if name.as_ref() == b"pStyle" {
                    if let Some(v) = attr_value_str(e, b"val") {
                        if let Ok(s) = std::str::from_utf8(v.as_ref()) {
                            if !s.is_empty() {
                                current_style = Some(s.to_string());
                            }
                        }
                    }
                    // Some writers emit `<w:pStyle>Heading1</w:pStyle>` (text body).
                    if let Ok(quick_xml::events::Event::Text(t)) = reader.read_event_into(&mut buf) {
                        let val = t.unescape().unwrap_or_default();
                        if !val.is_empty() {
                            current_style = Some(val.to_string());
                        }
                    }
                } else if name.as_ref() == b"numPr" {
                    in_numpr = true;
                    pending_num_id = None;
                    pending_ilvl = None;
                } else if in_numpr && name.as_ref() == b"numId" {
                    if let Some(v) = attr_value_str(e, b"val") {
                        if let Ok(s) = std::str::from_utf8(v.as_ref()) {
                            if let Ok(n) = s.parse::<u32>() {
                                pending_num_id = Some(n);
                            }
                        }
                    }
                } else if in_numpr && name.as_ref() == b"ilvl" {
                    if let Some(v) = attr_value_str(e, b"val") {
                        if let Ok(s) = std::str::from_utf8(v.as_ref()) {
                            if let Ok(n) = s.parse::<u32>() {
                                pending_ilvl = Some(n);
                            }
                        }
                    }
                } else if name.as_ref() == b"id" && para_depth > 0 && tbl_cell_depth == 0 {
                    // Read stable ID from custom inkuo:id element
                    // Also detect table markers (format: __tbl_pos_<table_id>__)
                    if let Some(v) = attr_value_str(e, b"val") {
                        if let Ok(s) = std::str::from_utf8(v.as_ref()) {
                            if !s.is_empty() {
                                if s.starts_with("__tbl_pos_") && s.ends_with("__") {
                                    // This is a table position marker
                                    current_stable_id = Some(s.to_string());
                                    is_table_marker = true;
                                } else if s.starts_with("__img_pos_") && s.ends_with("__") {
                                    // This is an image position marker — the writer
                                    // emits `<inkuo:id w:val="__img_pos_<img_id>__"/>`
                                    // and the actual `<w:drawing>` block in the same
                                    // paragraph. Round-tripping requires capturing the
                                    // id here so the marker paragraph can be re-emitted
                                    // and `parse_image_xml` can pair it with the
                                    // `<a:blip>` rId it finds deeper in the XML.
                                    current_stable_id = Some(s.to_string());
                                    is_image_marker = true;
                                } else {
                                    current_stable_id = Some(s.to_string());
                                }
                            }
                        }
                    }
                } else if name.as_ref() == b"pPr" {
                    // Entering a paragraph-properties block. The `jc`,
                    // `textDirection`, and `sectPr` children belong to
                    // the current paragraph.
                    in_ppr = true;
                    // Reset section-tracking state — a new pPr starts
                    // a fresh in-paragraph sectPr if one is present.
                    pending_sectpr = None;
                } else if in_ppr && name.as_ref() == b"jc" {
                    // `<w:jc w:val="center"/>` — paragraph alignment.
                    if let Some(v) = attr_value_str(e, b"val") {
                        if !v.is_empty() {
                            current_alignment = Some(v);
                        }
                    }
                } else if in_ppr && name.as_ref() == b"textDirection" {
                    // `<w:textDirection w:val="btLr"/>` — paragraph text
                    // direction. We normalise to the same vocabulary the
                    // writer accepts on input.
                    if let Some(v) = attr_value_str(e, b"val") {
                        if !v.is_empty() {
                            current_text_direction = Some(normalise_text_direction(&v));
                        }
                    }
                } else if (in_ppr || in_body) && name.as_ref() == b"sectPr" {
                    // Either an in-paragraph section break (next page /
                    // continuous / column — when `in_ppr` is set) or the
                    // body-level trailing `<w:sectPr>` (when `in_body`
                    // is set and we're outside any `<w:p>` / `<w:pPr>`).
                    // The Start handler must accept BOTH forms: a sectPr
                    // that appears directly under `<w:body>` (the trailing
                    // section) was previously silently dropped because
                    // `in_ppr` was false, which made read-back return
                    // `sections: []` even though document.xml had a
                    // perfectly valid `<w:body><w:sectPr>…</w:sectPr>`.
                    in_sectpr = true;
                    pending_sectpr_id_counter += 1;
                    pending_sectpr = Some(WordSection {
                        id: format!("section-{}", pending_sectpr_id_counter),
                        ..WordSection::default()
                    });
                } else if in_sectpr && name.as_ref() == b"pgSz" {
                    if let Some(ref mut sect) = pending_sectpr {
                        let width = attr_value_str(e, b"w")
                            .and_then(|v| v.parse::<u32>().ok());
                        let height = attr_value_str(e, b"h")
                            .and_then(|v| v.parse::<u32>().ok());
                        let orient = attr_value_str(e, b"orient")
                            .or_else(|| attr_value_str(e, b"o"))
                            .filter(|v| !v.is_empty());
                        if width.is_some() || height.is_some() || orient.is_some() {
                            let existing = sect.page_size_twips
                                .clone()
                                .unwrap_or(PageSize {
                                    width: 11906,
                                    height: 16838,
                                    orient: Some("portrait".to_string()),
                                });
                            sect.page_size_twips = Some(PageSize {
                                width: width.unwrap_or(existing.width),
                                height: height.unwrap_or(existing.height),
                                orient: orient.or(existing.orient),
                            });
                        }
                    }
                } else if in_sectpr && name.as_ref() == b"pgMar" {
                    if let Some(ref mut sect) = pending_sectpr {
                        let parse_opt = |key: &[u8], default: u32| -> u32 {
                            attr_value_str(e, key)
                                .and_then(|v| v.parse::<u32>().ok())
                                .unwrap_or(default)
                        };
                        let top = parse_opt(b"top", 1440);
                        let right = parse_opt(b"right", 1440);
                        let bottom = parse_opt(b"bottom", 1440);
                        let left = parse_opt(b"left", 1440);
                        let header = parse_opt(b"header", 720);
                        let footer = parse_opt(b"footer", 720);
                        let gutter = parse_opt(b"gutter", 0);
                        sect.margins = Some(PageMargins {
                            top, right, bottom, left,
                            header: Some(header),
                            footer: Some(footer),
                            gutter: Some(gutter),
                        });
                    }
                } else if in_sectpr && name.as_ref() == b"textDirection" {
                    if let Some(v) = attr_value_str(e, b"val") {
                        if !v.is_empty() {
                            if let Some(ref mut sect) = pending_sectpr {
                                sect.text_direction = Some(normalise_text_direction(&v));
                            }
                        }
                    }
                } else if in_sectpr && name.as_ref() == b"titlePg" {
                    if let Some(ref mut sect) = pending_sectpr {
                        sect.title_pg = true;
                    }
                } else if in_sectpr && name.as_ref() == b"cols" {
                    if let Some(ref mut sect) = pending_sectpr {
                        if let Some(v) = attr_value_str(e, b"num") {
                            if let Ok(n) = v.parse::<u32>() {
                                sect.cols = Some(n.max(1));
                            }
                        }
                    }
                } else if in_sectpr && name.as_ref() == b"pgNumType" {
                    if let Some(ref mut sect) = pending_sectpr {
                        if let Some(v) = attr_value_str(e, b"start") {
                            if let Ok(n) = v.parse::<u32>() {
                                sect.page_num_start = Some(n);
                            }
                        }
                        if let Some(v) = attr_value_str(e, b"fmt") {
                            if !v.is_empty() {
                                sect.page_num_format = Some(v);
                            }
                        }
                    }
                } else if in_sectpr && name.as_ref() == b"type" {
                    // `<w:type w:val="continuous"/>` etc.
                    if let Some(v) = attr_value_str(e, b"val") {
                        if !v.is_empty() {
                            if let Some(ref mut sect) = pending_sectpr {
                                sect.section_type = Some(v);
                            }
                        }
                    }
                } else if in_sectpr && name.as_ref() == b"headerReference" {
                    if let Some(rid) = attr_value_str(e, b"id") {
                        if !rid.is_empty() {
                            let kind = attr_value_str(e, b"type")
                                .unwrap_or_else(|| "default".to_string());
                            pending_section_header_refs.push(HeaderPartRef {
                                header_id: rid,
                                kind: Some(kind),
                            });
                        }
                    }
                } else if in_sectpr && name.as_ref() == b"footerReference" {
                    if let Some(rid) = attr_value_str(e, b"id") {
                        if !rid.is_empty() {
                            let kind = attr_value_str(e, b"type")
                                .unwrap_or_else(|| "default".to_string());
                            pending_section_footer_refs.push(FooterPartRef {
                                footer_id: rid,
                                kind: Some(kind),
                            });
                        }
                    }
                } else if in_run_props && name.as_ref() == b"vertAlign" {
                    if let Some(v) = attr_value_str(e, b"val") {
                        if !v.is_empty() {
                            current_run_vert_align = Some(v);
                        }
                    }
                } else if in_run && name.as_ref() == b"fldChar" {
                    // Field-code state machine: begin (1) -> separate (2) -> end (0).
                    if let Some(v) = attr_value_str(e, b"fldCharType") {
                        match v.as_str() {
                            "begin" => {
                                fld_state = 1;
                                fld_instr_buf.clear();
                                fld_cached_text.clear();
                            }
                            "separate" => {
                                // The text after this point (until end) is
                                // the cached field result.
                                if fld_state == 1 {
                                    fld_state = 2;
                                }
                            }
                            "end" => {
                                // Commit a run carrying the field.
                                if fld_state >= 1 {
                                    let instr = std::mem::take(&mut fld_instr_buf);
                                    let cached = std::mem::take(&mut fld_cached_text);
                                    let field = parse_field_instr(&instr);
                                    // Field runs still carry formatting, so we
                                    // push them as FontRuns. The visible text
                                    // is the cached result (e.g. "1" for PAGE).
                                    current_run_text = cached;
                                    current_run_field = field;
                                }
                                fld_state = 0;
                            }
                            _ => {}
                        }
                    }
                } else if in_run && name.as_ref() == b"instrText" {
                    if fld_state == 1 {
                        if let Ok(quick_xml::events::Event::Text(t)) = reader.read_event_into(&mut buf) {
                            fld_instr_buf.push_str(&t.unescape().unwrap_or_default());
                        }
                    }
                }
            }
            Ok(quick_xml::events::Event::Empty(ref e)) => {
                let name = e.local_name();
                if in_sectpr && name.as_ref() == b"headerReference" {
                    // Self-closing `<w:headerReference r:id="rIdN" w:type="default"/>`
                    // — the writer emits header/footer references as
                    // self-closing tags, so quick_xml surfaces them as
                    // `Empty` events rather than `Start`+`End`. Without
                    // this branch the reference is silently dropped
                    // during read-back, which means a doc that round-
                    // trips through `read_word_document` + `write_word_
                    // document` loses its header/footer wiring (the
                    // rIds get rewritten but the section ends up with
                    // an empty `header_refs` vec).
                    if let Some(rid) = attr_value_str(e, b"id") {
                        if !rid.is_empty() {
                            let kind = attr_value_str(e, b"type")
                                .unwrap_or_else(|| "default".to_string());
                            pending_section_header_refs.push(HeaderPartRef {
                                header_id: rid,
                                kind: Some(kind),
                            });
                        }
                    }
                } else if in_sectpr && name.as_ref() == b"footerReference" {
                    if let Some(rid) = attr_value_str(e, b"id") {
                        if !rid.is_empty() {
                            let kind = attr_value_str(e, b"type")
                                .unwrap_or_else(|| "default".to_string());
                            pending_section_footer_refs.push(FooterPartRef {
                                footer_id: rid,
                                kind: Some(kind),
                            });
                        }
                    }
                } else if in_sectpr && name.as_ref() == b"pgSz" {
                    if let Some(ref mut sect) = pending_sectpr {
                        let width = attr_value_str(e, b"w")
                            .and_then(|v| v.parse::<u32>().ok());
                        let height = attr_value_str(e, b"h")
                            .and_then(|v| v.parse::<u32>().ok());
                        let orient = attr_value_str(e, b"orient")
                            .or_else(|| attr_value_str(e, b"o"))
                            .filter(|v| !v.is_empty());
                        if width.is_some() || height.is_some() || orient.is_some() {
                            let existing = sect.page_size_twips.clone().unwrap_or(PageSize {
                                width: 11906,
                                height: 16838,
                                orient: Some("portrait".to_string()),
                            });
                            sect.page_size_twips = Some(PageSize {
                                width: width.unwrap_or(existing.width),
                                height: height.unwrap_or(existing.height),
                                orient: orient.or(existing.orient),
                            });
                        }
                    }
                } else if in_sectpr && name.as_ref() == b"pgMar" {
                    if let Some(ref mut sect) = pending_sectpr {
                        let parse_opt = |key: &[u8], default: u32| -> u32 {
                            attr_value_str(e, key)
                                .and_then(|v| v.parse::<u32>().ok())
                                .unwrap_or(default)
                        };
                        let top = parse_opt(b"top", 1440);
                        let right = parse_opt(b"right", 1440);
                        let bottom = parse_opt(b"bottom", 1440);
                        let left = parse_opt(b"left", 1440);
                        let header = parse_opt(b"header", 720);
                        let footer = parse_opt(b"footer", 720);
                        let gutter = parse_opt(b"gutter", 0);
                        sect.margins = Some(PageMargins {
                            top, right, bottom, left,
                            header: Some(header),
                            footer: Some(footer),
                            gutter: Some(gutter),
                        });
                    }
                } else if in_sectpr && name.as_ref() == b"textDirection" {
                    if let Some(v) = attr_value_str(e, b"val") {
                        if !v.is_empty() {
                            if let Some(ref mut sect) = pending_sectpr {
                                sect.text_direction = Some(normalise_text_direction(&v));
                            }
                        }
                    }
                } else if in_sectpr && name.as_ref() == b"titlePg" {
                    if let Some(ref mut sect) = pending_sectpr {
                        sect.title_pg = true;
                    }
                } else if in_sectpr && name.as_ref() == b"cols" {
                    if let Some(ref mut sect) = pending_sectpr {
                        if let Some(v) = attr_value_str(e, b"num") {
                            if let Ok(n) = v.parse::<u32>() {
                                sect.cols = Some(n.max(1));
                            }
                        }
                    }
                } else if in_sectpr && name.as_ref() == b"pgNumType" {
                    if let Some(ref mut sect) = pending_sectpr {
                        if let Some(v) = attr_value_str(e, b"start") {
                            if let Ok(n) = v.parse::<u32>() {
                                sect.page_num_start = Some(n);
                            }
                        }
                        if let Some(v) = attr_value_str(e, b"fmt") {
                            if !v.is_empty() {
                                sect.page_num_format = Some(v);
                            }
                        }
                    }
                } else if in_sectpr && name.as_ref() == b"type" {
                    if let Some(v) = attr_value_str(e, b"val") {
                        if !v.is_empty() {
                            if let Some(ref mut sect) = pending_sectpr {
                                sect.section_type = Some(v);
                            }
                        }
                    }
                } else if name.as_ref() == b"p" {
                    para_depth = para_depth.saturating_sub(0);
                    let id = if let Some(stable_id) = current_stable_id.clone() {
                        stable_id
                    } else {
                        let id = format!("p{}", para_counter);
                        para_counter += 1;
                        id
                    };
                    // Keep if has style OR is a table marker (has inkuo:id)
                    if current_style.is_some() || is_table_marker {
                        let text = if is_table_marker {
                            if let Some(stable_id) = &current_stable_id {
                                if let Some(rest) = stable_id.strip_prefix("__tbl_pos_") {
                                    if let Some(table_id) = rest.strip_suffix("__") {
                                        format!("<__tbl_pos_{}__>", table_id)
                                    } else {
                                        stable_id.clone()
                                    }
                                } else {
                                    stable_id.clone()
                                }
                            } else {
                                String::new()
                            }
                        } else {
                            String::new()
                        };
                        paragraphs.push(WordParagraph {
                            id,
                            text,
                            style: None,
                            runs: None,
                            numbering: None,
                            alignment: None,
                            text_direction: None,
                        });
                    }
                } else if name.as_ref() == b"r" && tbl_cell_depth == 0 && para_depth > 0 {
                    // Self-closing run — typically `<w:r><w:br/></w:r>` for line breaks.
                    // We model this by pushing an empty run with whatever format was set.
                    paragraph_saw_run = true;
                    if let Some(style) = current_style.clone() {
                        // ignore runs for paragraphs that haven't been started yet
                        let _ = style;
                    }
                } else if name.as_ref() == b"pStyle" {
                    if let Some(v) = attr_value_str(e, b"val") {
                        if let Ok(s) = std::str::from_utf8(v.as_ref()) {
                            if !s.is_empty() {
                                current_style = Some(s.to_string());
                            }
                        }
                    }
                } else if in_run_props {
                    // Self-closing run-property children like `<w:strike/>`,
                    // `<w:b/>`, `<w:color w:val="..."/>` come through here.
                    let val = attr_value(e, b"val");
                    let ascii = attr_value(e, b"ascii");
                    let hansi = attr_value(e, b"hAnsi");
                    let cs = attr_value(e, b"cs");
                    apply_run_attr(&mut current_run_format, name.as_ref(), val.as_deref());
                    if let Some(v) = ascii.or(hansi).or(cs) {
                        apply_run_attr(&mut current_run_format, b"rFonts", Some(v.as_ref()));
                    }
                } else if in_numpr && name.as_ref() == b"numId" {
                    // numId is typically a self-closing element like `<w:numId w:val="2"/>`.
                    if let Some(v) = attr_value_str(e, b"val") {
                        if let Ok(s) = std::str::from_utf8(v.as_ref()) {
                            if let Ok(n) = s.parse::<u32>() {
                                pending_num_id = Some(n);
                            }
                        }
                    }
                } else if in_numpr && name.as_ref() == b"ilvl" {
                    if let Some(v) = attr_value_str(e, b"val") {
                        if let Ok(s) = std::str::from_utf8(v.as_ref()) {
                            if let Ok(n) = s.parse::<u32>() {
                                pending_ilvl = Some(n);
                            }
                        }
                    }
                } else if name.as_ref() == b"id" && tbl_cell_depth == 0 {
                    // Read stable ID from custom inkuo:id element (empty tag)
                    // This can fire even when para_depth is 0 for self-closing tags
                    if let Some(v) = attr_value_str(e, b"val") {
                        if let Ok(s) = std::str::from_utf8(v.as_ref()) {
                            if !s.is_empty() {
                                if s.starts_with("__tbl_pos_") && s.ends_with("__") {
                                    current_stable_id = Some(s.to_string());
                                    is_table_marker = true;
                                } else if s.starts_with("__img_pos_") && s.ends_with("__") {
                                    current_stable_id = Some(s.to_string());
                                    is_image_marker = true;
                                } else if para_depth > 0 {
                                    current_stable_id = Some(s.to_string());
                                }
                            }
                        }
                    }
                } else if name.as_ref() == b"numPr" {
                    // Self-closing `<w:numPr/>` — empty list (no numId); still
                    // flip the in_numpr flag off in case more events follow.
                    in_numpr = false;
                }
            }
            Ok(quick_xml::events::Event::End(ref e)) => {
                let name = e.local_name();
                if name.as_ref() == b"tc" {
                    tbl_cell_depth = tbl_cell_depth.saturating_sub(1);
                } else if name.as_ref() == b"body" {
                    in_body = false;
                } else if name.as_ref() == b"rPr" {
                    in_run_props = false;
                } else if name.as_ref() == b"numPr" {
                    // Commit the numbering reference when numPr closes.
                    if let Some(num_id) = pending_num_id {
                        current_numbering = Some(NumberingRef {
                            num_id,
                            level: pending_ilvl.unwrap_or(0),
                        });
                    }
                    in_numpr = false;
                } else if name.as_ref() == b"pPr" {
                    // Closing the paragraph-properties block. If a sectPr
                    // is pending, attach it to the current paragraph's
                    // section list. For an in-paragraph sectPr, this
                    // means the paragraph closes out the *previous*
                    // section (a "next-page break" idiom); the trailing
                    // body-level sectPr will be the final section.
                    if let Some(mut sect) = pending_sectpr.take() {
                        // Drain accumulated header/footer refs into the
                        // section. They're collected across multiple
                        // sectPrs in document order so we pop them in
                        // arrival order — usually one header_ref / one
                        // footer_ref per sectPr.
                        sect.header_refs = std::mem::take(&mut pending_section_header_refs);
                        sect.footer_refs = std::mem::take(&mut pending_section_footer_refs);
                        sections.push(sect);
                    }
                    in_ppr = false;
                } else if name.as_ref() == b"sectPr" {
                    // Closing a sectPr. If we never saw a pPr (e.g. this
                    // is the body-level trailing sectPr), commit the
                    // accumulated refs now.
                    if let Some(mut sect) = pending_sectpr.take() {
                        sect.header_refs = std::mem::take(&mut pending_section_header_refs);
                        sect.footer_refs = std::mem::take(&mut pending_section_footer_refs);
                        sections.push(sect);
                    }
                    in_sectpr = false;
                } else if name.as_ref() == b"r" {
                    in_run = false;
                    in_run_props = false;
                    if tbl_cell_depth == 0 && para_depth > 0 {
                        paragraph_saw_run = true;
                        // Commit this run only if it produced text OR has a format
                        // flag the AI should know about. Empty runs with no flags
                        // are skipped — they would just bloat the response.
                        let has_format = current_run_format.bold
                            || current_run_format.italic
                            || current_run_format.underline
                            || current_run_format.strikethrough
                            || current_run_format.font_size.is_some()
                            || current_run_format.color.is_some()
                            || current_run_format.font_name.is_some()
                            || current_run_format.highlight.is_some()
                            || current_run_vert_align.is_some();
                        if !current_run_text.is_empty() || has_format || current_run_field.is_some() {
                            // Field runs always have a "field" payload but
                            // the visible text is the cached result (e.g. "1"
                            // for PAGE) that Word displays until the user
                            // presses F9. We treat this as a normal run for
                            // round-trip purposes; the writer recognises the
                            // `field` and re-emits a fldChar triplet on save.
                            if current_run_field.is_none() {
                                current_text.push_str(&current_run_text);
                            } else if !current_run_text.is_empty() {
                                // Append the cached field result to the
                                // paragraph's plain text view so search
                                // and "find" still see something useful.
                                current_text.push_str(&current_run_text);
                            }
                            current_runs.push(FontRun {
                                text: std::mem::take(&mut current_run_text),
                                bold: current_run_format.bold,
                                italic: current_run_format.italic,
                                underline: current_run_format.underline,
                                strikethrough: current_run_format.strikethrough,
                                font_size: current_run_format.font_size,
                                color: current_run_format.color.clone(),
                                font_name: current_run_format.font_name.clone(),
                                highlight: current_run_format.highlight.clone(),
                                vert_align: current_run_vert_align.take(),
                                field: current_run_field.take(),
                            });
                        } else {
                            // Discard any per-run transient state so it
                            // doesn't leak into the next run.
                            current_run_vert_align = None;
                            current_run_field = None;
                        }
                    }
                } else if name.as_ref() == b"p" {
                    para_depth = para_depth.saturating_sub(1);
                    if para_depth == 0 && tbl_cell_depth == 0 {
                        // Always preserve the paragraph's slot in the document.
                        // We only skip it if it had zero text AND zero runs AND no
                        // style — i.e. it was a totally empty paragraph that carries
                        // no information at all. Such paragraphs are usually
                        // artefacts of trailing whitespace and dropping them is safe.
                        let has_format = current_runs.iter().any(|r| {
                            r.bold || r.italic || r.underline || r.strikethrough
                                || r.font_size.is_some() || r.color.is_some() || r.font_name.is_some()
                                || r.highlight.is_some()
                        });
                        // Keep if: has content, or style, or formatting, or is a
                        // table or image marker. Image markers carry no text and
                        // produce no runs themselves (the run lives in a sub-parse
                        // of `<w:drawing>` in `parse_image_xml`), but we still
                        // want them present so the writer can re-emit the
                        // `<w:drawing>` on the next save.
                        let keep = !current_text.is_empty()
                            || current_style.is_some()
                            || current_numbering.is_some()
                            || has_format
                            || paragraph_saw_run
                            || is_table_marker
                            || is_image_marker;
                        if keep {
                            // Use stable ID if available, otherwise generate sequential ID
                            // For table markers, use the special marker text format
                            let id = if let Some(stable_id) = current_stable_id.clone() {
                                stable_id
                            } else {
                                let id = format!("p{}", para_counter);
                                para_counter += 1;
                                id
                            };
                            let runs_opt = if current_runs.is_empty() { None } else { Some(current_runs.clone()) };
                            // Markers carry a synthetic text that the writer's
                            // build_document_xml() recognises when splicing the
                            // `<w:tbl>` or `<w:drawing>` element back in.
                            let text = if is_table_marker {
                                // Extract table ID from marker format __tbl_pos_<table_id>__
                                if let Some(stable_id) = &current_stable_id {
                                    if let Some(rest) = stable_id.strip_prefix("__tbl_pos_") {
                                        if let Some(table_id) = rest.strip_suffix("__") {
                                            format!("<__tbl_pos_{}__>", table_id)
                                        } else {
                                            stable_id.clone()
                                        }
                                    } else {
                                        stable_id.clone()
                                    }
                                } else {
                                    current_text.clone()
                                }
                            } else if is_image_marker {
                                // Mirror the writer's marker text so the next
                                // write can splice the `<w:drawing>` back in
                                // via `image_map.get(img_id)`.
                                if let Some(stable_id) = &current_stable_id {
                                    if let Some(rest) = stable_id.strip_prefix("__img_pos_") {
                                        if let Some(img_id) = rest.strip_suffix("__") {
                                            format!("<__img_pos_{}__>", img_id)
                                        } else {
                                            stable_id.clone()
                                        }
                                    } else {
                                        stable_id.clone()
                                    }
                                } else {
                                    current_text.clone()
                                }
                            } else {
                                current_text.trim().to_string()
                            };
                            let para = WordParagraph {
                                id,
                                text,
                                style: current_style.clone(),
                                runs: runs_opt,
                                numbering: current_numbering.clone(),
                                alignment: current_alignment.take(),
                                text_direction: current_text_direction.take(),
                            };
                            // Image markers go to the side channel so the
                            // caller (read_word_document) can pair them
                            // with the WordImage entries we recover in
                            // parse_image_xml.
                            if is_image_marker {
                                image_markers.push(para);
                            } else {
                                paragraphs.push(para);
                            }
                        }
                    }
                }
            }
            Ok(quick_xml::events::Event::Eof) => break,
            Err(e) => return Err(OfficeError::Xml(format!("XML parse error: {}", e))),
            _ => {}
        }
        buf.clear();
    }

    Ok((paragraphs, image_markers, sections))
}

