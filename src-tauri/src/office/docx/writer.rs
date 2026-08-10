//! OOXML document-tree builders for `.docx` writers.
//!
//! Owns the pure XML-templating half of the writer:
//!   - `build_run_xml` / `build_run_rpr_xml` / `build_field_run_xml`
//!   - `field_instr_text` — `PAGE`, `NUMPAGES`, `DATE`, `TOC` field codes
//!   - `build_document_xml` — the full `<w:document>` tree
//!   - `build_paragraph_ppr_xml` / `build_body_sectpr_xml` /
//!     `build_sectpr_xml` — paragraph + section properties
//!   - `emit_text_direction` / `collect_section_breaks` /
//!     `section_break_section_idx` — section helpers
//!   - `build_table_xml` / `build_image_drawing_xml` /
//!     `stable_id_to_docpr_id` — table + image drawing XML
//!   - `escape_xml` — entity escaping
//!
//! These used to live in `mod.rs` (~640 lines of dense XML strings).
//! They have no I/O, no ZIP involvement — they're the parts that the
//! `write_word_document` orchestrator just splices into the package
//! payload.
//!
//! Pulled out of `mod.rs` so the orchestrator + image-preservation code
//! stays focused on the package assembly, not on string templating.

use crate::office::shared::{TableCell, TableRow};
use super::styled_writer::{
    build_callout_close_xml, build_callout_container_xml, build_code_block_container_xml,
    build_styled_table_xml, classify_and_strip, TableKind,
};
use super::components::TableStyle;
use super::{
    FieldRef, FontRun, HeaderPart, HeaderPartRef, PageSize, PageSizeMm, WordDocument, WordImage,
    WordParagraph, WordSection, WordTable,
};

pub fn build_run_xml(run: &FontRun) -> String {
    // Field runs render as a `<w:fldChar>` triplet plus a cached result
    // run. We don't emit the cached result inside the field (Word
    // refreshes it on F9), so the structure is: begin run, instrText
    // run, separate run, cached-text run, end run. All five share the
    // same formatting (bold, font, color, etc.) as the parent run.
    if let Some(ref field) = run.field {
        return build_field_run_xml(run, field);
    }

    // Page break: emit just the break element with no text content.
    if run.page_break {
        return "<w:r><w:br w:type=\"page\"/></w:r>".to_string();
    }

    let mut xml = String::from("<w:r>");
    let rpr = build_run_rpr_xml(run);
    if !rpr.is_empty() {
        xml.push_str("<w:rPr>");
        xml.push_str(&rpr);
        xml.push_str("</w:rPr>");
    }

    xml.push_str(&format!(
        "<w:t xml:space=\"preserve\">{}</w:t></w:r>",
        escape_xml(&run.text)
    ));
    xml
}

/// Render the `<w:rPr>` (run-property) XML for a `FontRun`, including
/// the new `vert_align` field. Used by both the regular run builder
/// and the field-run builder (fields inherit the run's formatting on
/// every run of the fldChar triplet).
pub fn build_run_rpr_xml(run: &FontRun) -> String {
    let mut rpr = String::new();

    if run.bold { rpr.push_str("<w:b/>"); }
    if run.italic { rpr.push_str("<w:i/>"); }
    if run.underline { rpr.push_str("<w:u w:val=\"single\"/>"); }
    if run.strikethrough { rpr.push_str("<w:strike/>"); }
    if let Some(ref highlight) = run.highlight {
        if !highlight.is_empty() {
            rpr.push_str(&format!("<w:highlight w:val=\"{}\"/>", escape_xml(highlight)));
        }
    }
    if let Some(ref color) = run.color {
        if !color.is_empty() {
            rpr.push_str(&format!("<w:color w:val=\"{}\"/>", escape_xml(color)));
        }
    }
    if let Some(size) = run.font_size {
        rpr.push_str(&format!("<w:sz w:val=\"{}\"/>", size));
        rpr.push_str(&format!("<w:szCs w:val=\"{}\"/>", size));
    }
    if let Some(ref font) = run.font_name {
        if !font.is_empty() {
            rpr.push_str(&format!("<w:rFonts w:ascii=\"{}\" w:hAnsi=\"{}\"/>", escape_xml(font), escape_xml(font)));
        }
    }
    if let Some(ref va) = run.vert_align {
        if !va.is_empty() {
            rpr.push_str(&format!("<w:vertAlign w:val=\"{}\"/>", escape_xml(va)));
        }
    }
    rpr
}

/// Render a Word field-code run as a 5-run fldChar triplet. The
/// `cached_text` (the visible value Word shows before F9 refresh) is
/// taken from `run.text`.
///
/// Word's required structure:
/// ```xml
/// <w:r><w:fldChar w:fldCharType="begin"/></w:r>
/// <w:r><w:instrText xml:space="preserve"> PAGE </w:instrText></w:r>
/// <w:r><w:fldChar w:fldCharType="separate"/></w:r>
/// <w:r><w:t>1</w:t></w:r>            ← cached result
/// <w:r><w:fldChar w:fldCharType="end"/></w:r>
/// ```
///
/// All five runs share the run's formatting. The `instr` text is
/// reconstructed from the `FieldRef` variant; see `field_instr_text`
/// for the inverse (parse-side) helper.
pub(crate) fn build_field_run_xml(run: &FontRun, field: &FieldRef) -> String {
    let rpr = build_run_rpr_xml(run);
    let rpr_open = if rpr.is_empty() {
        String::new()
    } else {
        format!("<w:rPr>{}</w:rPr>", rpr)
    };
    let instr = field_instr_text(field);
    let cached = if run.text.is_empty() { "".to_string() } else { run.text.clone() };
    format!(
        concat!(
            "<w:r>{rpr}<w:fldChar w:fldCharType=\"begin\"/></w:r>",
            "<w:r>{rpr}<w:instrText xml:space=\"preserve\">{instr}</w:instrText></w:r>",
            "<w:r>{rpr}<w:fldChar w:fldCharType=\"separate\"/></w:r>",
            "<w:r>{rpr}<w:t xml:space=\"preserve\">{cached}</w:t></w:r>",
            "<w:r>{rpr}<w:fldChar w:fldCharType=\"end\"/></w:r>"
        ),
        rpr = rpr_open,
        instr = escape_xml(&instr),
        cached = escape_xml(&cached),
    )
}

/// Reconstruct the raw `<w:instrText>` payload for a `FieldRef`. This
/// is the inverse of `parse_field_instr` and must stay in sync with
/// it. The leading space and trailing space match Word's own emit
/// style (` PAGE `) so the field is recognised on round-trip.

// ── Field instructions ─────────────────────────────────────────────────────────

pub fn field_instr_text(field: &FieldRef) -> String {
    match field {
        FieldRef::Page => " PAGE ".to_string(),
        FieldRef::NumPages => " NUMPAGES ".to_string(),
        FieldRef::SectionPages => " SECTIONPAGES ".to_string(),
        FieldRef::Section => " SECTION ".to_string(),
        FieldRef::Date { format: Some(f) } => format!(" DATE \\@ \"{}\" ", f),
        FieldRef::Date { format: None } => " DATE ".to_string(),
        FieldRef::Time { format: Some(f) } => format!(" TIME \\@ \"{}\" ", f),
        FieldRef::Time { format: None } => " TIME ".to_string(),
        FieldRef::Author => " AUTHOR ".to_string(),
        FieldRef::Title => " TITLE ".to_string(),
        FieldRef::Filename { with_ext: true } => " FILENAME ".to_string(),
        FieldRef::Filename { with_ext: false } => " FILENAME \\p ".to_string(),
        FieldRef::Custom { instr } => format!(" {} ", instr),
    }
}

// ── Document builder ─────────────────────────────────────────────────────────────

pub fn build_document_xml(doc: &WordDocument) -> String {
    let mut xml = String::from(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"
            xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"
            xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing"
            xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"
            xmlns:pic="http://schemas.openxmlformats.org/drawingml/2006/picture"
            xmlns:inkuo="http://inkuo.app/wordprocessingml/2026/main">
  <w:body>"#
    );

    // Build a map of table id -> table for O(1) lookup.
    let table_map: std::collections::HashMap<&str, &WordTable> =
        doc.tables.iter().map(|t| (t.id.as_str(), t)).collect();

    // Build a map of image id -> image for O(1) lookup. The image's stable id
    // is matched against `<__img_pos_<id>__>` marker paragraphs.
    let image_map: std::collections::HashMap<&str, &WordImage> =
        doc.images.iter().map(|i| (i.id.as_str(), i)).collect();

    // Track which tables have been emitted via markers to avoid double-emission
    let mut tables_emitted: std::collections::HashSet<&str> = std::collections::HashSet::new();

    // Resolve sections: the model carries an ordered list. We treat every
    // section except the last as "next-page break" and embed its `<w:sectPr>`
    // inside the `<w:pPr>` of the last paragraph of *that* section. The
    // final section's `<w:sectPr>` goes right before `</w:body>`.
    //
    // Without per-paragraph section assignment info we use a simple
    // position-based split: section 0 covers the first N0 paragraphs,
    // section 1 covers the next N1, etc. We default to "all paragraphs
    // belong to the last (and only) section" — the writer emits a
    // single body-level sectPr in that case. When there are multiple
    // sections we need to know how many paragraphs each one owns; the
    // tool layer is responsible for telling us via
    // `WordSection.id`-tagged marker paragraphs (one per section break).
    //
    // We look for marker paragraphs whose `id` is `__sect_break_<id>__`
    // and use those as section boundaries. Anything between two markers
    // (or before the first / after the last) belongs to the
    // corresponding section. A document with no markers and N sections
    // is treated as "all paragraphs in the last section, N-1 empty
    // leading sections" — pragmatic and matches what an unmodified
    // existing document looks like on read.
    let section_breaks: Vec<usize> = collect_section_breaks(&doc.paragraphs, doc.sections.len());
    let _total_sections = doc.sections.len().max(1);
    let sections: Vec<WordSection> = if doc.sections.is_empty() {
        vec![WordSection::default()]
    } else {
        doc.sections.clone()
    };
    // Make `total_sections` agree with `sections` (one section, one
    // element).
    let total_sections = sections.len();

    // Walk paragraphs, assigning each to a section index. The last
    // section is the trailing one (no marker; sectPr is body-level).
    // For N sections and M paragraphs:
    //   - With explicit `__sect_break_<idx>__` markers, we honour them
    //     precisely (one section per marker span).
    //   - Without markers, all paragraphs would land in the last
    //     section by default — which means any `cols > 1` on an
    //     earlier section silently applies to the whole document. The
    //     caller likely meant "split the doc across sections". Distribute
    //     paragraphs evenly across all sections so multi-column /
    //     cover-page settings apply only to the part they were meant
    //     for. See the `sections_without_markers_distribute_paragraphs`
    //     regression test.
    let mut para_section_idx: Vec<usize> = vec![total_sections - 1; doc.paragraphs.len()];
    if total_sections > 1 && section_breaks.len() + 1 == total_sections {
        let mut current_section = 0;
        for (i, p) in doc.paragraphs.iter().enumerate() {
            para_section_idx[i] = current_section;
            if section_breaks.contains(&i) {
                current_section = current_section.saturating_add(1).min(total_sections - 1);
            }
            let _ = p;
        }
    } else if total_sections > 1 {
        // Even split: each non-final section gets `total / N` paragraphs;
        // the last section absorbs any remainder. This way `cols: 2`
        // on a single Section entry doesn't silently span the whole
        // body — the section's properties apply to its share only.
        let total_paras = doc.paragraphs.len();
        let base = total_paras / total_sections;
        let mut idx_section = 0usize;
        let mut paragraphs_in_current = 0usize;
        for i in 0..total_paras {
            para_section_idx[i] = idx_section;
            paragraphs_in_current += 1;
            // Move to the next section when this one has had its share,
            // but always leave at least one paragraph for the final
            // (body-level) section.
            if idx_section + 1 < total_sections
                && paragraphs_in_current >= base
                && (i + 1) + (total_sections - idx_section - 1) <= total_paras
            {
                idx_section += 1;
                paragraphs_in_current = 0;
            }
        }
    }

    // Iterate over paragraphs directly - markers contain position info
    // We use a manual loop with `i` so we can advance past the inner
    // paragraphs of callout / code containers when we consume them
    // inside the cell.
    let mut idx = 0usize;
    while idx < doc.paragraphs.len() {
        let para = &doc.paragraphs[idx];
        // Check if this is a table position marker
        if let Some(rest) = para.text.strip_prefix("<__tbl_pos_") {
            if let Some(end) = rest.find("__>") {
                let tbl_id = &rest[..end];
                if let Some(tbl) = table_map.get(tbl_id) {
                    // Output a marker paragraph with empty content but marker ID pattern
                    // The parser will detect this via the inkuo:id pattern
                    xml.push_str("\n    <w:p>");
                    xml.push_str(&format!("<w:pPr><inkuo:id w:val=\"__tbl_pos_{}__\"/></w:pPr>", escape_xml(tbl_id)));
                    xml.push_str("</w:p>");
                    // Route the table through the styled writer when its first
                    // row carries a recognised marker (`__STYLE__|`,
                    // `__CALLOUT__|`, `__CODE__|`). The renderer in
                    // `components.rs` injects those markers; the styled
                    // builder in `styled_writer.rs` knows how to render the
                    // coloured fill / accent border / zebra striping.
                    // Falls back to the plain `build_table_xml` for tables
                    // built from low-level `DocElement::Table` payloads.
                    let (kind, body_rows) = classify_and_strip(&tbl.rows);
                    match kind {
                        TableKind::Styled(style) => {
                            xml.push_str(&build_styled_table_xml(&tbl.id, &body_rows, &style));
                        }
                        TableKind::Callout { bg, accent } => {
                            xml.push_str(&build_callout_container_xml(&bg, &accent));
                            // Round-trip path takes priority: the
                            // reader parked the inner paragraphs on
                            // `tbl.cell_paragraphs` and that's the
                            // authoritative source. Fresh-render path
                            // (when no cell paragraphs were recovered)
                            // falls back to consuming the body-level
                            // paragraphs that follow the marker.
                            if tbl.cell_paragraphs.is_empty() {
                                let consumed = emit_callout_inner_paragraphs(
                                    &mut xml, doc, idx + 1,
                                );
                                idx += consumed;
                            } else {
                                emit_inner_paragraphs(
                                    &mut xml,
                                    &tbl.cell_paragraphs,
                                );
                                // Skip past the body siblings that
                                // the renderer kept around for the
                                // fresh-render path.
                                let consumed = count_callout_inner_paragraphs(doc, idx + 1);
                                idx += consumed;
                            }
                            xml.push_str(&build_callout_close_xml());
                            tables_emitted.insert(tbl_id);
                            idx += 1;
                            continue;
                        }
                        TableKind::Code { bg } => {
                            xml.push_str(&build_code_block_container_xml(&bg));
                            if tbl.cell_paragraphs.is_empty() {
                                let consumed = emit_callout_inner_paragraphs(
                                    &mut xml, doc, idx + 1,
                                );
                                idx += consumed;
                            } else {
                                emit_inner_paragraphs(
                                    &mut xml,
                                    &tbl.cell_paragraphs,
                                );
                                let consumed = count_callout_inner_paragraphs(doc, idx + 1);
                                idx += consumed;
                            }
                            xml.push_str(&build_callout_close_xml());
                            tables_emitted.insert(tbl_id);
                            idx += 1;
                            continue;
                        }
                        TableKind::Plain => {
                            xml.push_str(&build_table_xml(&tbl.id, &tbl.rows, None));
                        }
                    }
                    tables_emitted.insert(tbl_id);
                    idx += 1;
                    continue;
                }
            }
        }

        // Image position marker: emit a paragraph that carries the inline
        // `<w:drawing>` run. The image's internal `word/media/imageN.ext`
        // path was resolved by the writer; the `rId` is looked up from
        // the rels table the writer builds up alongside this XML. We use
        // a placeholder `rId` here (`rIdImg<index>`) and rewrite it in
        // `write_word_document` after the rels are finalised.
        if let Some(rest) = para.text.strip_prefix("<__img_pos_") {
            if let Some(end) = rest.find("__>") {
                let img_id = &rest[..end];
                if let Some(img) = image_map.get(img_id) {
                    xml.push_str("\n    <w:p>");
                    xml.push_str(&format!(
                        "<w:pPr><inkuo:id w:val=\"__img_pos_{}__\"/></w:pPr>",
                        escape_xml(img_id)
                    ));
                    xml.push_str(&build_image_drawing_xml(img));
                    xml.push_str("</w:p>");
                    idx += 1;
                    continue;
                }
            }
        }

        // Section-break marker: a synthetic paragraph the model emits
        // to mark "this is the last paragraph of section N". We
        // promote it to a regular paragraph carrying the in-paragraph
        // `<w:sectPr>` of the closing section.
        if let Some(break_section_idx) = section_break_section_idx(para) {
            if break_section_idx < total_sections {
                xml.push_str("\n    <w:p>");
                let sect = &sections[break_section_idx];
                xml.push_str(&build_paragraph_ppr_xml(para, Some(sect)));
                xml.push_str("</w:p>");
                idx += 1;
                continue;
            }
        }

        // Regular paragraph - output as normal
        xml.push_str("\n    <w:p>");
        // Build paragraph properties: style (if any) + numbering (if any) + alignment + text direction + stable ID.
        // For the *last* paragraph of a non-final section we also embed
        // that section's `<w:sectPr>` here (the OOXML idiom for an
        // in-paragraph section break). Two ways a paragraph counts as
        // the last in its section:
        //   1. The next paragraph is a section-break marker for this
        //      section (the explicit `__sect_break_<idx>__` path).
        //   2. The next paragraph belongs to a *different* section per
        //      `para_section_idx` (the auto-distributed path used when
        //      sections are provided without markers).
        let sect_idx = para_section_idx[idx];
        let next_sect_idx = if idx + 1 < doc.paragraphs.len() {
            para_section_idx[idx + 1]
        } else {
            // Past-the-end: this paragraph is the last in the doc and
            // belongs to whichever section it's in. Section-emission
            // for the final section happens via the body-level sectPr
            // below; we don't embed anything here.
            total_sections
        };
        let is_last_para_of_section = if sect_idx + 1 < total_sections {
            idx + 1 < doc.paragraphs.len()
                && (section_break_section_idx(&doc.paragraphs[idx + 1]) == Some(sect_idx)
                    || next_sect_idx != sect_idx)
        } else {
            false
        };
        let embedded_sectpr = if is_last_para_of_section {
            Some(&sections[sect_idx])
        } else {
            None
        };
        xml.push_str(&build_paragraph_ppr_xml(para, embedded_sectpr));

        // Output paragraph content
        if let Some(ref run_list) = para.runs {
            for run in run_list {
                xml.push_str(&build_run_xml(run));
            }
        } else if !para.text.is_empty() {
            for chunk in para.text.split('\n') {
                if !chunk.is_empty() {
                    xml.push_str(&format!(
                        "<w:r><w:t xml:space=\"preserve\">{}</w:t></w:r>",
                        escape_xml(chunk)
                    ));
                }
                xml.push_str("<w:r><w:br/></w:r>");
            }
        }
        xml.push_str("</w:p>");
        idx += 1;
    }

    // Output any tables that weren't emitted via markers (orphaned tables)
    for tbl in &doc.tables {
        if !tables_emitted.contains(tbl.id.as_str()) {
            let (kind, body_rows) = classify_and_strip(&tbl.rows);
            match kind {
                TableKind::Styled(style) => {
                    xml.push_str(&build_styled_table_xml(&tbl.id, &body_rows, &style));
                }
                TableKind::Callout { bg, accent } => {
                    xml.push_str(&build_callout_container_xml(&bg, &accent));
                    xml.push_str(&build_callout_close_xml());
                }
                TableKind::Code { bg } => {
                    xml.push_str(&build_code_block_container_xml(&bg));
                    xml.push_str(&build_callout_close_xml());
                }
                TableKind::Plain => {
                    xml.push_str(&build_table_xml(&tbl.id, &tbl.rows, None));
                }
            }
        }
    }

    // Trailing body-level `<w:sectPr>` for the final section. If the
    // document has no sections the writer still emits a default A4
    // portrait sectPr so Word can open the file without complaint.
    let final_section = sections.last().cloned().unwrap_or_else(WordSection::default);
    xml.push_str(&build_body_sectpr_xml(&final_section));

    xml.push_str("\n  </w:body>\n</w:document>");
    xml
}

/// Build the `<w:pPr>` block for a paragraph, optionally with an
/// in-paragraph `<w:sectPr>` (used for section breaks). The `sect`
/// argument is `Some(_)` when this paragraph closes out a
/// non-trailing section.
pub(crate) fn build_paragraph_ppr_xml(para: &WordParagraph, sect: Option<&WordSection>) -> String {
    let has_alignment = para.alignment.is_some();
    let has_text_direction = para.text_direction.is_some();
    let has_sectpr = sect.is_some();
    let has_ppr = para.style.is_some()
        || para.numbering.is_some()
        || !para.id.is_empty()
        || has_alignment
        || has_text_direction
        || has_sectpr;
    if !has_ppr {
        return String::new();
    }
    let mut xml = String::from("<w:pPr>");
    if let Some(ref s) = para.style {
        xml.push_str(&format!("<w:pStyle w:val=\"{}\"/>", escape_xml(s)));
    }
    if let Some(ref num) = para.numbering {
        xml.push_str("<w:numPr>");
        xml.push_str(&format!("<w:ilvl w:val=\"{}\"/>", num.level));
        xml.push_str(&format!("<w:numId w:val=\"{}\"/>", num.num_id));
        xml.push_str("</w:numPr>");
    }
    if let Some(ref a) = para.alignment {
        if !a.is_empty() {
            xml.push_str(&format!("<w:jc w:val=\"{}\"/>", escape_xml(a)));
        }
    }
    if let Some(ref td) = para.text_direction {
        if !td.is_empty() {
            let v = emit_text_direction(td);
            xml.push_str(&format!("<w:textDirection w:val=\"{}\"/>", v));
        }
    }
    if let Some(s) = sect {
        xml.push_str(&build_sectpr_xml(s));
    }
    if !para.id.is_empty() {
        xml.push_str(&format!("<inkuo:id w:val=\"{}\"/>", escape_xml(&para.id)));
    }
    xml.push_str("</w:pPr>");
    xml
}

/// Build a body-level `<w:sectPr>` block (the trailing section
/// properties for the entire document). Header / footer references
/// carry a placeholder rId of the form `rIdHeaderPlaceholder_<partid>`
/// / `rIdFooterPlaceholder_<partid>`. The writer's
/// `substitute_header_footer_placeholders` pass rewrites them to the
/// real rIds once they're allocated.
pub(crate) fn build_body_sectpr_xml(sect: &WordSection) -> String {
    format!("\n    {}", build_sectpr_xml(sect))
}

/// Build a `<w:sectPr>` block (the inner contents — no surrounding
/// whitespace). Used both at body level and inside `<w:pPr>`.
pub(crate) fn build_sectpr_xml(sect: &WordSection) -> String {
    let mut xml = String::from("<w:sectPr>");
    // Header / footer references MUST come first inside sectPr (Word
    // is strict about element ordering). The placeholder rIds are
    // rewritten by the writer after rId allocation.
    for hr in &sect.header_refs {
        let kind = hr.kind.as_deref().unwrap_or("default");
        xml.push_str(&format!(
            "<w:headerReference r:id=\"rIdHeaderPlaceholder_{}\" w:type=\"{}\"/>",
            escape_xml(&hr.header_id),
            escape_xml(kind),
        ));
    }
    for fr in &sect.footer_refs {
        let kind = fr.kind.as_deref().unwrap_or("default");
        xml.push_str(&format!(
            "<w:footerReference r:id=\"rIdFooterPlaceholder_{}\" w:type=\"{}\"/>",
            escape_xml(&fr.footer_id),
            escape_xml(kind),
        ));
    }
    if let Some(ref st) = sect.section_type {
        if !st.is_empty() {
            xml.push_str(&format!("<w:type w:val=\"{}\"/>", escape_xml(st)));
        }
    }
    if let Some(ref ps) = sect.page_size_twips {
        let mut attrs = format!("w:w=\"{}\" w:h=\"{}\"", ps.width, ps.height);
        if let Some(ref o) = ps.orient {
            if !o.is_empty() {
                attrs.push_str(&format!(" w:orient=\"{}\"", escape_xml(o)));
            }
        }
        xml.push_str(&format!("<w:pgSz {}/>", attrs));
    }
    if let Some(ref m) = sect.margins {
        let mut attrs = format!(
            "w:top=\"{}\" w:right=\"{}\" w:bottom=\"{}\" w:left=\"{}\"",
            m.top, m.right, m.bottom, m.left
        );
        if let Some(h) = m.header {
            attrs.push_str(&format!(" w:header=\"{}\"", h));
        }
        if let Some(f) = m.footer {
            attrs.push_str(&format!(" w:footer=\"{}\"", f));
        }
        if let Some(g) = m.gutter {
            attrs.push_str(&format!(" w:gutter=\"{}\"", g));
        }
        xml.push_str(&format!("<w:pgMar {}/>", attrs));
    }
    // Section-level text direction (rare but valid). Writer writes
    // the OOXML vocabulary (`btLr` etc.) regardless of the input name
    // we use on the model side.
    if let Some(ref td) = sect.text_direction {
        if !td.is_empty() {
            let v = emit_text_direction(td);
            xml.push_str(&format!("<w:textDirection w:val=\"{}\"/>", v));
        }
    }
    if let Some(n) = sect.cols {
        if n > 1 {
            xml.push_str(&format!("<w:cols w:num=\"{}\" w:space=\"720\"/>", n));
        } else {
            xml.push_str("<w:cols w:space=\"720\"/>");
        }
    }
    if sect.title_pg {
        xml.push_str("<w:titlePg/>");
    }
    if let Some(start) = sect.page_num_start {
        let mut attrs = format!("w:start=\"{}\"", start);
        if let Some(ref fmt) = sect.page_num_format {
            if !fmt.is_empty() {
                attrs.push_str(&format!(" w:fmt=\"{}\"", escape_xml(fmt)));
            }
        }
        xml.push_str(&format!("<w:pgNumType {}/>", attrs));
    } else if let Some(ref fmt) = sect.page_num_format {
        if !fmt.is_empty() {
            xml.push_str(&format!("<w:pgNumType w:fmt=\"{}\"/>", escape_xml(fmt)));
        }
    }
    xml.push_str("</w:sectPr>");
    xml
}

/// Inverse of `normalise_text_direction` — turn the friendly
/// vocabulary the model uses back into OOXML's `w:textDirection`
/// attribute values. Unknown values pass through verbatim (Word
/// accepts `lrTb`, `tbRl`, `btLr`, `lrTbV`, `tbRlV`, `btLrV`).
pub(crate) fn emit_text_direction(v: &str) -> &str {
    match v {
        "horizontal" => "lrTb",
        "vertical" => "tbRlV",
        "verticalRightToLeft" => "btLr",
        "verticalLeftToRight" => "btLrV",
        "rotate90" => "lrTbV",
        "rotate270" => "lrTb",
        other => other,
    }
}

// ── Section-break helpers ────────────────────────────────────────────────────────

/// Find section-break marker paragraphs in the body. A marker is any
/// paragraph whose `id` looks like `__sect_break_<idx>__`. Returns
/// the index of each marker so the caller can compute which section
/// a given paragraph belongs to.
fn collect_section_breaks(paragraphs: &[WordParagraph], section_count: usize) -> Vec<usize> {
    if section_count <= 1 {
        return Vec::new();
    }
    let mut breaks = Vec::new();
    for (i, p) in paragraphs.iter().enumerate() {
        if section_break_section_idx(p).is_some() {
            breaks.push(i);
        }
    }
    breaks
}

/// If `p` is a section-break marker, return the section index it
/// closes (zero-based). Returns `None` for regular paragraphs.
fn section_break_section_idx(p: &WordParagraph) -> Option<usize> {
    if let Some(rest) = p.id.strip_prefix("__sect_break_") {
        if let Some(idx_str) = rest.strip_suffix("__") {
            if let Ok(n) = idx_str.parse::<usize>() {
                return Some(n);
            }
        }
    }
    None
}

/// Walk the paragraph list and emit each callout/code-block's inner
/// paragraphs (title + body) right after its marker paragraph. The
/// paragraph iterator in `build_document_xml` calls this whenever it
/// hits a callout or code-block `<__tbl_pos_<id>__>` marker. We
/// re-interpret the following paragraphs as inner-cell paragraphs
/// (rather than as body siblings) until we hit the next marker.
///
/// This is the second half of the `push_callout` / `push_code`
/// pipeline: the renderer emits `[marker_para, container_table,
/// inner_para_1, inner_para_2, ...]` into the doc, and the writer
/// flattens that into `container_table { inner_para_1; inner_para_2; ... }`
/// so the cell actually contains the title + body text.
///
/// Implementation detail: we look ahead in `doc.paragraphs` starting
/// at `idx + 1` (the paragraph immediately after the current marker
/// in the for-loop) and pull everything until the next
/// `__tbl_pos_` / `__img_pos_` / `__sect_break_` marker. We emit each
/// pulled paragraph's body inline so the callout / code cell ends up
/// with the right content. Stable ids are preserved on each inner
/// paragraph so subsequent reads can target them.
fn emit_callout_inner_paragraphs(
    xml: &mut String,
    doc: &WordDocument,
    start_idx: usize,
) -> usize {
    let mut consumed = 0usize;
    for (i, p) in doc.paragraphs.iter().enumerate().skip(start_idx) {
        // Stop at any other marker — they're not inner-cell content.
        if p.text.starts_with("<__tbl_pos_")
            || p.text.starts_with("<__img_pos_")
            || p.id.starts_with("__sect_break_")
        {
            break;
        }
        emit_inner_paragraph(xml, p);
        consumed += 1;
    }
    consumed
}

/// Emit a list of `WordParagraph` as a stream of `<w:p>...</w:p>`
/// XML inside an already-open `<w:tc>`. This is the round-trip path
/// for callout / code-block containers: the reader parked the inner
/// paragraphs on `WordTable::cell_paragraphs` so they survive the
/// round-trip even though the body-level `WordDocument::paragraphs`
/// list only carries the table marker.
fn emit_inner_paragraphs(xml: &mut String, paragraphs: &[WordParagraph]) {
    for p in paragraphs {
        emit_inner_paragraph(xml, p);
    }
}

/// Count the body-level paragraphs that follow the callout marker
/// (the same set `emit_callout_inner_paragraphs` would consume).
/// Used to advance `idx` past them when the writer is taking the
/// round-trip path (`cell_paragraphs` is set) so the body's
/// paragraph list isn't re-emitted as siblings.
fn count_callout_inner_paragraphs(doc: &WordDocument, start_idx: usize) -> usize {
    let mut counted = 0usize;
    for (_i, p) in doc.paragraphs.iter().enumerate().skip(start_idx) {
        if p.text.starts_with("<__tbl_pos_")
            || p.text.starts_with("<__img_pos_")
            || p.id.starts_with("__sect_break_")
        {
            break;
        }
        counted += 1;
    }
    counted
}

/// Emit a single paragraph as raw `<w:p>...</w:p>` inside an
/// already-open `<w:tc>` (callout / code-block cell). This skips the
/// section-tracking machinery used by the main loop because the
/// inner cell content never carries section properties.
fn emit_inner_paragraph(xml: &mut String, para: &WordParagraph) {
    xml.push_str("\n      <w:p>");
    let has_ppr = para.style.is_some()
        || para.numbering.is_some()
        || para.alignment.is_some()
        || para.text_direction.is_some()
        || !para.id.is_empty();
    if has_ppr {
        xml.push_str("<w:pPr>");
        if let Some(ref s) = para.style {
            xml.push_str(&format!("<w:pStyle w:val=\"{}\"/>", escape_xml(s)));
        }
        if let Some(ref num) = para.numbering {
            xml.push_str("<w:numPr>");
            xml.push_str(&format!("<w:ilvl w:val=\"{}\"/>", num.level));
            xml.push_str(&format!("<w:numId w:val=\"{}\"/>", num.num_id));
            xml.push_str("</w:numPr>");
        }
        if let Some(ref a) = para.alignment {
            if !a.is_empty() {
                xml.push_str(&format!("<w:jc w:val=\"{}\"/>", escape_xml(a)));
            }
        }
        if let Some(ref td) = para.text_direction {
            if !td.is_empty() {
                let v = emit_text_direction(td);
                xml.push_str(&format!("<w:textDirection w:val=\"{}\"/>", v));
            }
        }
        if !para.id.is_empty() {
            xml.push_str(&format!("<inkuo:id w:val=\"{}\"/>", escape_xml(&para.id)));
        }
        xml.push_str("</w:pPr>");
    }
    if let Some(ref run_list) = para.runs {
        for run in run_list {
            xml.push_str(&build_run_xml(run));
        }
    } else if !para.text.is_empty() {
        for chunk in para.text.split('\n') {
            if !chunk.is_empty() {
                xml.push_str(&format!(
                    "<w:r><w:t xml:space=\"preserve\">{}</w:t></w:r>",
                    escape_xml(chunk)
                ));
            }
            xml.push_str("<w:r><w:br/></w:r>");
        }
    }
    xml.push_str("</w:p>");
}

pub(crate) fn build_table_xml(table_id: &str, rows: &[TableRow], style: Option<&TableStyle>) -> String {
    let mut xml = String::new();
    xml.push_str("\n    <w:tbl>");
    xml.push_str("\n      <w:tblPr>");
    xml.push_str("<w:tblStyle w:val=\"TableGrid\"/>");
    
    // Table width: use auto with 0 (Word will auto-size), but emit explicit w:w
    // for better compatibility with parsers that require it
    xml.push_str("<w:tblW w:type=\"auto\" w:w=\"0\"/>");
    
    // Table indent: default to 0, can be overridden by style
    xml.push_str("<w:tblInd w:type=\"dxa\" w:w=\"0\"/>");
    
    // Table layout: fixed ensures consistent rendering
    xml.push_str("<w:tblLayout w:type=\"fixed\"/>");
    
    // Default table look for compatibility
    xml.push_str("<w:tblLook w:firstColumn=\"1\" w:firstRow=\"1\" w:lastColumn=\"0\" w:lastRow=\"0\" w:noHBand=\"0\" w:noVBand=\"1\" w:val=\"04A0\"/>");
    
    // Table borders
    xml.push_str("<w:tblBorders>");
    xml.push_str("<w:top w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"auto\"/>");
    xml.push_str("<w:left w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"auto\"/>");
    xml.push_str("<w:bottom w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"auto\"/>");
    xml.push_str("<w:right w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"auto\"/>");
    xml.push_str("<w:insideH w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"auto\"/>");
    xml.push_str("<w:insideV w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"auto\"/>");
    xml.push_str("</w:tblBorders>");
    
    xml.push_str("</w:tblPr>");

    // Calculate column count and build tblGrid
    let col_count = rows.first().map(|r| r.cells.len()).unwrap_or(0);
    if col_count > 0 {
        xml.push_str("\n      <w:tblGrid>");
        // Default column width: divide available width evenly
        // Using 1440 twips (1 inch) as default column width
        let default_col_width = 1440u32;
        for _ in 0..col_count {
            xml.push_str(&format!("<w:gridCol w:w=\"{}\"/>", default_col_width));
        }
        xml.push_str("</w:tblGrid>");
    }

    // Render rows
    for (row_idx, row) in rows.iter().enumerate() {
        let is_header_row = row_idx == 0;
        xml.push_str("\n        <w:tr>");
        
        // Row properties: mark first row as header for repeat if requested
        let has_header_repeat = style.map(|s| s.repeat_header && is_header_row).unwrap_or(false);
        let has_row_props = has_header_repeat;
        
        if has_row_props {
            xml.push_str("<w:trPr>");
            if has_header_repeat {
                xml.push_str("<w:tblHeader/>");
            }
            xml.push_str("</w:trPr>");
        }
        
        for cell in &row.cells {
            let col_span = cell.col_span.max(1);
            let row_span = cell.row_span.max(1);
            
            xml.push_str("<w:tc><w:tcPr>");
            
            // Grid span for merged cells
            if col_span > 1 {
                xml.push_str(&format!("<w:gridSpan w:val=\"{}\"/>", col_span));
            }
            
            // Vertical merge for row-spanning cells
            if row_span > 1 {
                xml.push_str("<w:vMerge w:val=\"restart\"/>");
            }
            
            // Cell width: distribute evenly across col_span
            let cell_width = 1440u32 * col_span as u32;
            xml.push_str(&format!("<w:tcW w:type=\"dxa\" w:w=\"{}\"/>", cell_width));
            
            // Cell margins (inner padding) for comfortable reading
            xml.push_str("<w:tcMar>");
            xml.push_str("<w:top w:w=\"100\" w:type=\"dxa\"/>");
            xml.push_str("<w:left w:w=\"120\" w:type=\"dxa\"/>");
            xml.push_str("<w:bottom w:w=\"100\" w:type=\"dxa\"/>");
            xml.push_str("<w:right w:w=\"120\" w:type=\"dxa\"/>");
            xml.push_str("</w:tcMar>");
            
            // Cell vertical alignment
            xml.push_str("<w:vAlign w:val=\"center\"/>");
            
            // Cell borders
            xml.push_str("<w:tcBorders>");
            xml.push_str("<w:top w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"auto\"/>");
            xml.push_str("<w:left w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"auto\"/>");
            xml.push_str("<w:bottom w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"auto\"/>");
            xml.push_str("<w:right w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"auto\"/>");
            xml.push_str("<w:insideH w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"auto\"/>");
            xml.push_str("<w:insideV w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"auto\"/>");
            xml.push_str("</w:tcBorders>");
            
            xml.push_str("</w:tcPr><w:p>");
            
            // Render cell text with line breaks preserved
            let lines: Vec<&str> = cell.text.split('\n').collect();
            for (chunk_idx, chunk) in lines.iter().enumerate() {
                if !chunk.is_empty() {
                    xml.push_str(&format!(
                        "<w:r><w:t xml:space=\"preserve\">{}</w:t></w:r>",
                        escape_xml(chunk)
                    ));
                }
                if chunk_idx < lines.len().saturating_sub(1) {
                    xml.push_str("<w:r><w:br/></w:r>");
                }
            }
            
            xml.push_str("</w:p></w:tc>");
        }
        xml.push_str("</w:tr>");
    }

    xml.push_str("\n    </w:tbl>");
    let _ = table_id;
    xml
}

/// Render an inline picture run as a `<w:drawing>` element.
///
/// `r:embed="rIdImgPlaceholder"` is rewritten by `write_word_document` after
/// the writer knows the final rels id (which may shift when the original
/// document already used `rId1`, `rId2`, ... for its own styles / numbering
/// / hyperlinks). The placeholder is deliberately unique so a missed
/// rewrite is impossible to miss in QA.
pub(crate) fn build_image_drawing_xml(img: &WordImage) -> String {
    let cx = img.width_emu;
    let cy = img.height_emu;
    // Image element name (the `pic:name` is a cosmetic label — Word picks
    // it up from the picture's own embedded metadata; we use the stable id
    // so debug dumps correlate with `WordDocument.images`).
    let name = escape_xml(&img.id);
    format!(
        concat!(
            "<w:r><w:drawing>",
            "<wp:inline distT=\"0\" distB=\"0\" distL=\"0\" distR=\"0\">",
            "<wp:extent cx=\"{cx}\" cy=\"{cy}\"/>",
            "<wp:effectExtent l=\"0\" t=\"0\" r=\"0\" b=\"0\"/>",
            "<wp:docPr id=\"{docpr_id}\" name=\"{name}\"/>",
            "<wp:cNvGraphicFramePr>",
            "<a:graphicFrameLocks xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" noChangeAspect=\"1\"/>",
            "</wp:cNvGraphicFramePr>",
            "<a:graphic xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\">",
            "<a:graphicData uri=\"http://schemas.openxmlformats.org/drawingml/2006/picture\">",
            "<pic:pic xmlns:pic=\"http://schemas.openxmlformats.org/drawingml/2006/picture\">",
            "<pic:nvPicPr>",
            "<pic:cNvPr id=\"{docpr_id}\" name=\"{name}\"/>",
            "<pic:cNvPicPr/>",
            "</pic:nvPicPr>",
            "<pic:blipFill>",
            "<a:blip xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" r:embed=\"rIdImgPlaceholder\"/>",
            "<a:stretch><a:fillRect/></a:stretch>",
            "</pic:blipFill>",
            "<pic:spPr>",
            "<a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"{cx}\" cy=\"{cy}\"/></a:xfrm>",
            "<a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom>",
            "</pic:spPr>",
            "</pic:pic>",
            "</a:graphicData>",
            "</a:graphic>",
            "</wp:inline>",
            "</w:drawing></w:r>"
        ),
        cx = cx,
        cy = cy,
        name = name,
        // `docPr id` and `pic:cNvPr id` are namespace-local identifiers. We
        // derive them from the image's stable id (stable, non-negative) by
        // hashing. For v1 we just use the count of leading hex chars of the
        // id as a quick-and-deterministic number; collisions are harmless
        // because Word treats the id as opaque within a document.
        docpr_id = stable_id_to_docpr_id(&img.id),
    )
}

/// Deterministically derive a positive integer id (1..i32::MAX) from a
/// string stable id. Used as `wp:docPr id` and `pic:cNvPr id` for inline
/// pictures. Stable across writes, so the docx diff stays clean.
pub(crate) fn stable_id_to_docpr_id(id: &str) -> u32 {
    // FNV-1a 32-bit; output fits in u32.
    let mut hash: u32 = 2_166_136_261u32;
    for b in id.as_bytes() {
        hash ^= *b as u32;
        hash = hash.wrapping_mul(16_777_619u32);
    }
    // Avoid the 0 sentinel — Word occasionally treats 0 as "no id".
    hash.max(1)
}

pub fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
