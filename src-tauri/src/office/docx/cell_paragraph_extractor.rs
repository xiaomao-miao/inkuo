//! Targeted cell-paragraph extractor for design-system container
//! tables.
//!
//! The main `parse_document_xml` parser drops paragraphs that live
//! inside `<w:tc>` cells (the cell text is captured by
//! `parse_table_xml` and lives on `WordTable.rows[i].cells[j].text`).
//! That's the right behaviour for ordinary data tables but it loses
//! important structure for callout / code-block containers, where
//! the cell holds actual paragraphs (with styles, runs, and field
//! codes) that the writer needs to re-emit inside the shaded cell
//! on the next save.
//!
//! This module adds a small second pass over `document.xml`. It walks
//! the XML twice: the first pass counts `<w:tr>` and `<w:tc>` inside
//! every depth-1 `<w:tbl>` so we know which tables are 1×1 container
//! shapes; the second pass extracts `<w:p>` content for those tables
//! only. Ordinary data tables contribute empty `Vec`s so the caller
//! can zip the result with `parse_table_xml`'s output by position.
//!
//! The extractor is deliberately smaller than the full
//! `parse_document_xml`: it only handles the paragraph-level
//! properties the writer needs to round-trip (text, runs, style,
//! alignment, text direction, numbering). Field codes are surfaced
//! as plain text via the cached-result representation; the writer
//! re-emits any field code from its `FontRun.field` slot on the
//! next pass via the main parser path.

use crate::office::docx::{FontRun, NumberingRef, WordParagraph};

/// Walk `document.xml` and return one `Vec<WordParagraph>` per table
/// in document order. Empty entries mean "this isn't a container
/// table; ignore it".
pub(crate) fn extract_container_cell_paragraphs(
    content: &str,
) -> Vec<Vec<WordParagraph>> {
    // First pass: identify container tables by counting rows/cells.
    let containers = find_container_tables(content);
    // Second pass: only walk paragraphs inside those tables.
    walk_container_paragraphs(content, &containers)
}

/// For each `<w:tbl>` in document order, returns `true` iff it's a
/// 1×1 container (exactly one `<w:tr>` and exactly one `<w:tc>`).
/// Other tables (data tables with multiple cells / rows) are not
/// containers.
fn find_container_tables(content: &str) -> Vec<bool> {
    let mut out: Vec<bool> = Vec::new();
    let mut reader = quick_xml::Reader::from_str(content);
    reader.config_mut().trim_text(false);
    let mut buf = Vec::new();

    let mut tbl_depth: usize = 0;
    let mut tr_count: usize = 0;
    let mut tc_count: usize = 0;

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(quick_xml::events::Event::Start(ref e)) => {
                let name = e.local_name();
                match name.as_ref() {
                    b"tbl" if tbl_depth == 0 => {
                        tbl_depth = 1;
                        tr_count = 0;
                        tc_count = 0;
                    }
                    b"tr" if tbl_depth == 1 => {
                        tr_count += 1;
                        tc_count = 0;
                    }
                    b"tc" if tbl_depth == 1 && tr_count == 1 => {
                        // We only care about the first row; a 1×1
                        // container always has tc_count = 1.
                        tc_count += 1;
                    }
                    _ => {}
                }
            }
            Ok(quick_xml::events::Event::End(ref e)) => {
                let name = e.local_name();
                if name.as_ref() == b"tbl" && tbl_depth == 1 {
                    out.push(tr_count == 1 && tc_count == 1);
                    tbl_depth = 0;
                }
            }
            Ok(quick_xml::events::Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
        buf.clear();
    }
    out
}

/// Walk `document.xml` and collect `<w:p>` paragraphs for each
/// container table identified by `find_container_tables`. The
/// returned Vec is aligned with the input `containers` by position.
fn walk_container_paragraphs(
    content: &str,
    containers: &[bool],
) -> Vec<Vec<WordParagraph>> {
    let mut out: Vec<Vec<WordParagraph>> = vec![Vec::new(); containers.len()];
    let mut reader = quick_xml::Reader::from_str(content);
    reader.config_mut().trim_text(false);
    let mut buf = Vec::new();

    let mut tbl_depth: usize = 0;
    let mut tbl_index: usize = 0;
    let mut p_depth: usize = 0;
    let mut in_target: bool = false;

    // Per-paragraph state.
    let mut cur_text = String::new();
    let mut cur_style: Option<String> = None;
    let mut cur_runs: Vec<FontRun> = Vec::new();
    let mut cur_stable_id: Option<String> = None;
    let mut cur_alignment: Option<String> = None;
    let mut cur_text_direction: Option<String> = None;
    let mut cur_numbering: Option<NumberingRef> = None;
    let mut cur_saw_run = false;
    let mut cur_run_text = String::new();
    let mut cur_run_format = SimpleRunFormat::default();
    let mut cur_run_vert_align: Option<String> = None;
    let mut cur_run_field: Option<crate::office::FieldRef> = None;
    let mut fld_state: u8 = 0;
    let mut in_ppr = false;
    let mut in_numpr = false;
    let mut pending_num_id: Option<u32> = None;
    let mut pending_ilvl: Option<u32> = None;
    let mut in_run = false;
    let mut in_run_props = false;
    let mut para_counter: usize = 0;

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(quick_xml::events::Event::Start(ref e)) => {
                let name = e.local_name();
                match name.as_ref() {
                    b"tbl" if tbl_depth == 0 => {
                        tbl_depth = 1;
                        in_target = containers
                            .get(tbl_index)
                            .copied()
                            .unwrap_or(false);
                    }
                    b"tbl" => {
                        tbl_depth += 1;
                    }
                    b"p" if tbl_depth == 1 && in_target => {
                        p_depth += 1;
                        if p_depth == 1 {
                            cur_text.clear();
                            cur_style = None;
                            cur_runs.clear();
                            cur_stable_id = None;
                            cur_alignment = None;
                            cur_text_direction = None;
                            cur_numbering = None;
                            cur_saw_run = false;
                            fld_state = 0;
                        }
                    }
                    b"p" if tbl_depth == 0 => {
                        p_depth += 1;
                    }
                    b"r" if in_target && p_depth > 0 && tbl_depth == 1 => {
                        in_run = true;
                        in_run_props = false;
                        cur_run_text.clear();
                        cur_run_format = SimpleRunFormat::default();
                        cur_run_vert_align = None;
                        cur_run_field = None;
                        cur_saw_run = true;
                    }
                    b"rPr" if in_run => {
                        in_run_props = true;
                    }
                    b"pPr" if in_target && p_depth > 0 && tbl_depth == 1 => {
                        in_ppr = true;
                    }
                    b"numPr" if in_ppr => {
                        in_numpr = true;
                    }
                    b"pStyle" if in_ppr => {
                        for attr in e.attributes().with_checks(false).flatten() {
                            if attr.key.local_name().as_ref() == b"val" {
                                if let Ok(s) = std::str::from_utf8(&attr.value) {
                                    cur_style = Some(s.to_string());
                                }
                            }
                        }
                    }
                    b"jc" if in_ppr => {
                        for attr in e.attributes().with_checks(false).flatten() {
                            if attr.key.local_name().as_ref() == b"val" {
                                if let Ok(s) = std::str::from_utf8(&attr.value) {
                                    cur_alignment = Some(s.to_string());
                                }
                            }
                        }
                    }
                    b"textDirection" if in_ppr => {
                        for attr in e.attributes().with_checks(false).flatten() {
                            if attr.key.local_name().as_ref() == b"val" {
                                if let Ok(s) = std::str::from_utf8(&attr.value) {
                                    cur_text_direction = Some(s.to_string());
                                }
                            }
                        }
                    }
                    b"id" if in_ppr => {
                        for attr in e.attributes().with_checks(false).flatten() {
                            if attr.key.local_name().as_ref() == b"val" {
                                if let Ok(s) = std::str::from_utf8(&attr.value) {
                                    cur_stable_id = Some(s.to_string());
                                }
                            }
                        }
                    }
                    b"ilvl" if in_numpr => {
                        for attr in e.attributes().with_checks(false).flatten() {
                            if attr.key.local_name().as_ref() == b"val" {
                                if let Ok(s) = std::str::from_utf8(&attr.value) {
                                    pending_ilvl = s.parse().ok();
                                }
                            }
                        }
                    }
                    b"numId" if in_numpr => {
                        for attr in e.attributes().with_checks(false).flatten() {
                            if attr.key.local_name().as_ref() == b"val" {
                                if let Ok(s) = std::str::from_utf8(&attr.value) {
                                    pending_num_id = s.parse().ok();
                                }
                            }
                        }
                    }
                    b"b" if in_run_props => cur_run_format.bold = true,
                    b"i" if in_run_props => cur_run_format.italic = true,
                    b"u" if in_run_props => cur_run_format.underline = true,
                    b"strike" if in_run_props => cur_run_format.strikethrough = true,
                    b"color" if in_run_props => {
                        for attr in e.attributes().with_checks(false).flatten() {
                            if attr.key.local_name().as_ref() == b"val" {
                                if let Ok(s) = std::str::from_utf8(&attr.value) {
                                    cur_run_format.color = Some(s.to_string());
                                }
                            }
                        }
                    }
                    b"sz" if in_run_props => {
                        for attr in e.attributes().with_checks(false).flatten() {
                            if attr.key.local_name().as_ref() == b"val" {
                                if let Ok(s) = std::str::from_utf8(&attr.value) {
                                    cur_run_format.font_size = s.parse().ok();
                                }
                            }
                        }
                    }
                    b"rFonts" if in_run_props => {
                        for attr in e.attributes().with_checks(false).flatten() {
                            if attr.key.local_name().as_ref() == b"ascii" {
                                if let Ok(s) = std::str::from_utf8(&attr.value) {
                                    cur_run_format.font_name = Some(s.to_string());
                                }
                            }
                        }
                    }
                    b"highlight" if in_run_props => {
                        for attr in e.attributes().with_checks(false).flatten() {
                            if attr.key.local_name().as_ref() == b"val" {
                                if let Ok(s) = std::str::from_utf8(&attr.value) {
                                    cur_run_format.highlight = Some(s.to_string());
                                }
                            }
                        }
                    }
                    b"vertAlign" if in_run_props => {
                        for attr in e.attributes().with_checks(false).flatten() {
                            if attr.key.local_name().as_ref() == b"val" {
                                if let Ok(s) = std::str::from_utf8(&attr.value) {
                                    cur_run_vert_align = Some(s.to_string());
                                }
                            }
                        }
                    }
                    b"fldChar" if in_run => {
                        for attr in e.attributes().with_checks(false).flatten() {
                            if attr.key.local_name().as_ref() == b"fldCharType" {
                                if let Ok(s) = std::str::from_utf8(&attr.value) {
                                    fld_state = match s {
                                        "begin" => 1,
                                        "separate" => 2,
                                        _ => 0,
                                    };
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
            Ok(quick_xml::events::Event::Empty(ref e)) => {
                let name = e.local_name();
                if in_ppr {
                    match name.as_ref() {
                        b"pStyle" => {
                            for attr in e.attributes().with_checks(false).flatten() {
                                if attr.key.local_name().as_ref() == b"val" {
                                    if let Ok(s) = std::str::from_utf8(&attr.value) {
                                        cur_style = Some(s.to_string());
                                    }
                                }
                            }
                        }
                        b"jc" => {
                            for attr in e.attributes().with_checks(false).flatten() {
                                if attr.key.local_name().as_ref() == b"val" {
                                    if let Ok(s) = std::str::from_utf8(&attr.value) {
                                        cur_alignment = Some(s.to_string());
                                    }
                                }
                            }
                        }
                        b"textDirection" => {
                            for attr in e.attributes().with_checks(false).flatten() {
                                if attr.key.local_name().as_ref() == b"val" {
                                    if let Ok(s) = std::str::from_utf8(&attr.value) {
                                        cur_text_direction = Some(s.to_string());
                                    }
                                }
                            }
                        }
                        b"id" => {
                            for attr in e.attributes().with_checks(false).flatten() {
                                if attr.key.local_name().as_ref() == b"val" {
                                    if let Ok(s) = std::str::from_utf8(&attr.value) {
                                        cur_stable_id = Some(s.to_string());
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                }
                if in_numpr {
                    match name.as_ref() {
                        b"ilvl" => {
                            for attr in e.attributes().with_checks(false).flatten() {
                                if attr.key.local_name().as_ref() == b"val" {
                                    if let Ok(s) = std::str::from_utf8(&attr.value) {
                                        pending_ilvl = s.parse().ok();
                                    }
                                }
                            }
                        }
                        b"numId" => {
                            for attr in e.attributes().with_checks(false).flatten() {
                                if attr.key.local_name().as_ref() == b"val" {
                                    if let Ok(s) = std::str::from_utf8(&attr.value) {
                                        pending_num_id = s.parse().ok();
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            Ok(quick_xml::events::Event::Text(ref t)) => {
                if in_target && tbl_depth == 1 && p_depth > 0 {
                    if let Ok(s) = t.unescape() {
                        if fld_state != 1 {
                            cur_text.push_str(&s);
                        }
                    }
                }
            }
            Ok(quick_xml::events::Event::End(ref e)) => {
                let name = e.local_name();
                match name.as_ref() {
                    b"tbl" => {
                        if tbl_depth == 1 {
                            tbl_index += 1;
                            in_target = false;
                        }
                        tbl_depth = tbl_depth.saturating_sub(1);
                    }
                    b"p" => {
                        if p_depth > 0 {
                            if in_target && tbl_depth == 1 {
                                let has_format = cur_runs.iter().any(|r| {
                                    r.bold || r.italic || r.underline || r.strikethrough
                                        || r.font_size.is_some() || r.color.is_some()
                                        || r.font_name.is_some() || r.highlight.is_some()
                                });
                                let keep = !cur_text.is_empty()
                                    || cur_style.is_some()
                                    || cur_numbering.is_some()
                                    || has_format
                                    || cur_saw_run;
                                if keep {
                                    let id = if let Some(sid) = cur_stable_id.clone() {
                                        sid
                                    } else {
                                        let id = format!("p{}", para_counter);
                                        para_counter += 1;
                                        id
                                    };
                                    let runs_opt = if cur_runs.is_empty() {
                                        None
                                    } else {
                                        Some(cur_runs.clone())
                                    };
                                    if let (Some(nid), Some(ilvl)) = (pending_num_id, pending_ilvl) {
                                        cur_numbering = Some(NumberingRef { num_id: nid, level: ilvl });
                                    }
                                    if let Some(bucket) = out.get_mut(tbl_index) {
                                        bucket.push(WordParagraph {
                                            id,
                                            text: cur_text.clone(),
                                            style: cur_style.clone(),
                                            runs: runs_opt,
                                            numbering: cur_numbering.clone(),
                                            alignment: cur_alignment.clone(),
                                            text_direction: cur_text_direction.clone(),
                                        });
                                    }
                                }
                            }
                        }
                        p_depth = p_depth.saturating_sub(1);
                    }
                    b"r" => {
                        if in_run {
                            in_run = false;
                            in_run_props = false;
                            let has_format = cur_run_format.bold
                                || cur_run_format.italic
                                || cur_run_format.underline
                                || cur_run_format.strikethrough
                                || cur_run_format.font_size.is_some()
                                || cur_run_format.color.is_some()
                                || cur_run_format.font_name.is_some()
                                || cur_run_format.highlight.is_some();
                            if !cur_run_text.is_empty() || has_format || cur_run_field.is_some() {
                                cur_runs.push(FontRun {
                                    text: std::mem::take(&mut cur_run_text),
                                    bold: cur_run_format.bold,
                                    italic: cur_run_format.italic,
                                    underline: cur_run_format.underline,
                                    strikethrough: cur_run_format.strikethrough,
                                    font_size: cur_run_format.font_size,
                                    color: cur_run_format.color.clone(),
                                    font_name: cur_run_format.font_name.clone(),
                                    highlight: cur_run_format.highlight.clone(),
                                    vert_align: cur_run_vert_align.take(),
                                    field: cur_run_field.take(),
                                    page_break: false,
                                });
                            } else {
                                cur_run_vert_align = None;
                                cur_run_field = None;
                            }
                        }
                    }
                    b"rPr" => {
                        in_run_props = false;
                    }
                    b"pPr" => {
                        in_ppr = false;
                        in_numpr = false;
                        pending_num_id = None;
                        pending_ilvl = None;
                    }
                    b"numPr" => {
                        in_numpr = false;
                    }
                    _ => {}
                }
            }
            Ok(quick_xml::events::Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
        buf.clear();
    }
    out
}

#[derive(Default, Clone)]
struct SimpleRunFormat {
    bold: bool,
    italic: bool,
    underline: bool,
    strikethrough: bool,
    font_size: Option<u32>,
    color: Option<String>,
    font_name: Option<String>,
    highlight: Option<String>,
}