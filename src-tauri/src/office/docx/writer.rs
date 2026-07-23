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
    // For N sections and M paragraphs without markers we put all M
    // paragraphs into the last section and emit empty (placeholder)
    // sectPrs at body-level for the rest. With markers we honour
    // them precisely.
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
    }

    // Iterate over paragraphs directly - markers contain position info
    for (idx, para) in doc.paragraphs.iter().enumerate() {
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
                    // Also output the table immediately after
                    xml.push_str(&build_table_xml(&tbl.id, &tbl.rows));
                    tables_emitted.insert(tbl_id);
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
                continue;
            }
        }

        // Regular paragraph - output as normal
        xml.push_str("\n    <w:p>");
        // Build paragraph properties: style (if any) + numbering (if any) + alignment + text direction + stable ID.
        // For the *last* paragraph of a non-final section we also embed
        // that section's `<w:sectPr>` here (the OOXML idiom for an
        // in-paragraph section break).
        let sect_idx = para_section_idx[idx];
        let is_last_para_of_section = if sect_idx + 1 < total_sections {
            // The next paragraph is the section-break marker for THIS
            // section, or this is the very last paragraph in the
            // section's range. We check the marker at idx+1.
            idx + 1 < doc.paragraphs.len()
                && section_break_section_idx(&doc.paragraphs[idx + 1]) == Some(sect_idx)
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
    }

    // Output any tables that weren't emitted via markers (orphaned tables)
    for tbl in &doc.tables {
        if !tables_emitted.contains(tbl.id.as_str()) {
            xml.push_str(&build_table_xml(&tbl.id, &tbl.rows));
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

pub(crate) fn build_table_xml(_table_id: &str, rows: &[TableRow]) -> String {
    let mut xml = String::new();
    xml.push_str("\n    <w:tbl>");
    xml.push_str("\n      <w:tblPr>");
    xml.push_str("<w:tblStyle w:val=\"TableGrid\"/>");
    xml.push_str("<w:tblW w:type=\"auto\" w:w=\"0\"/>");
    xml.push_str("<w:tblBorders>");
    xml.push_str("<w:top w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"auto\"/>");
    xml.push_str("<w:left w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"auto\"/>");
    xml.push_str("<w:bottom w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"auto\"/>");
    xml.push_str("<w:right w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"auto\"/>");
    xml.push_str("<w:insideH w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"auto\"/>");
    xml.push_str("<w:insideV w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"auto\"/>");
    xml.push_str("</w:tblBorders>");
    xml.push_str("</w:tblPr>");

    let render_row = |xml: &mut String, cells: &[TableCell]| {
        xml.push_str("\n        <w:tr>");
        for cell in cells {
            let col_span = cell.col_span.max(1);
            let row_span = cell.row_span.max(1);
            xml.push_str("<w:tc><w:tcPr>");
            if col_span > 1 {
                xml.push_str(&format!("<w:gridSpan w:val=\"{}\"/>", col_span));
            }
            if row_span > 1 {
                xml.push_str("<w:vMerge w:val=\"restart\"/>");
            }
            xml.push_str("</w:tcPr><w:p>");
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
    };

    // Render all rows (first row is header, rest are body)
    for row in rows {
        if !row.cells.is_empty() {
            render_row(&mut xml, &row.cells);
        }
    }

    xml.push_str("\n    </w:tbl>");
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
