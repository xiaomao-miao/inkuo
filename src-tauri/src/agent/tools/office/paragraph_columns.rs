//! Helpers for the `create_word_doc` tool that don't fit naturally
//! into the generic writer/renderer pipeline.
//!
//! Today this module hosts [`expand_paragraph_columns`], which
//! translates the tool's per-paragraph `columns` hint into ordered OOXML
//! section-break markers around balanced paragraph runs. Word's native
//! data model is section-centric: `<w:cols w:num="N"/>` lives inside
//! `<w:sectPr>` and applies to every paragraph in the section. There
//! is no paragraph-level column primitive, so a partial-column region
//! must be bracketed by `continuous` section breaks whose middle
//! section carries the desired `cols`.
//!
//! The writer understands the `__sect_break_<idx>__` marker convention.
//! This helper therefore:
//!
//!   1. Validates and locates every target id.
//!   2. Recovers the document's existing physical section order.
//!   3. Coalesces adjacent targets with the same count into one section.
//!   4. Rebuilds sequential markers and sections while enforcing
//!      `section_breaks.len() + 1 == total_sections`.

// NOTE: The tests in `#[cfg(test)]` below need `tokio::test` (async) and
// `zip::ZipArchive`; those crates are already in Cargo.toml's `[dependencies]`
// so their re-exported paths (`crate::office::...`, `crate::agent::...`) are
// available without extra imports here. The one test-only import needed is
// `zip::ZipArchive` for reading the raw docx bytes back.

use std::collections::HashMap;

use crate::office::{PageSize, WordParagraph, WordSection};

/// A4 page size in twips (1 inch = 1440 twips, A4 = 210mm × 297mm)
const A4_WIDTH_TWIPS: u32 = 11906;
const A4_HEIGHT_TWIPS: u32 = 16838;

/// Overlay column layouts on the paragraphs named by `targets`.
///
/// Adjacent targets with the same column count are deliberately coalesced into
/// one section. A multi-column section is the unit Word balances across its
/// columns; wrapping each paragraph independently leaves every short paragraph
/// with an empty column and creates the staggered layout this helper exists to
/// avoid.
///
/// Paragraphs not found by id raise a clear error so the AI gets
/// useful feedback ("you asked to column-wrap paragraph 'p1' but no
/// paragraph with that id exists in the document"). This catches the
/// common AI failure mode of mistyping ids or referencing a paragraph
/// that was deleted in the same call.
///
/// The function also normalises the document's section representation. Writer
/// markers and `sections` must obey one invariant: `markers + 1 == sections`.
/// Older versions appended three sections for every two markers, causing the
/// writer to ignore the markers and fall back to an even paragraph split. We
/// rebuild both collections in physical document order so this invariant is
/// true after every successful call, including when repairing an existing file.
pub fn expand_paragraph_columns(
    paragraphs: &mut Vec<WordParagraph>,
    sections: &mut Vec<WordSection>,
    targets: &[(String, u32)],
) -> Result<(), String> {
    if targets.is_empty() {
        return Ok(());
    }

    // Filter, validate, and reject ambiguous duplicate hints.
    let mut effective: HashMap<String, u32> = HashMap::with_capacity(targets.len());
    for (id, cols) in targets {
        if *cols == 1 {
            // cols=1 is the default; no wrap needed.
            continue;
        }
        if *cols == 0 || *cols > 9 {
            return Err(format!(
                "Invalid `columns` value {} for paragraph '{}': must be 1..=9 \
                 (1 = single column, the writer ignores cols=1; 2..=9 are the \
                 multi-column layouts Word supports).",
                cols, id
            ));
        }
        if let Some(previous) = effective.insert(id.clone(), *cols) {
            if previous != *cols {
                return Err(format!(
                    "Conflicting `columns` values for paragraph '{}': {} and {}.",
                    id, previous, cols
                ));
            }
        }
    }
    if effective.is_empty() {
        return Ok(());
    }

    // Validate ids before mutating either collection. Section-break markers are
    // implementation details and can never be valid user targets.
    let mut matched: HashMap<&str, usize> = HashMap::new();
    for paragraph in paragraphs
        .iter()
        .filter(|p| section_break_index(p).is_none())
    {
        if effective.contains_key(&paragraph.id) {
            *matched.entry(paragraph.id.as_str()).or_insert(0) += 1;
        }
    }
    let mut remaining: Vec<&str> = effective
        .keys()
        .filter(|id| !matched.contains_key(id.as_str()))
        .map(String::as_str)
        .collect();
    remaining.sort_unstable();
    if !remaining.is_empty() {
        return Err(format!(
            "`columns` hint requested for paragraph id(s) that don't exist in the \
             document: {}. Make sure the paragraph id is correct and that the \
             paragraph hasn't been deleted earlier in the same call.",
            remaining.join(", ")
        ));
    }
    if let Some((id, count)) = matched.iter().find(|(_, count)| **count > 1) {
        return Err(format!(
            "`columns` hint is ambiguous because paragraph id '{}' occurs {} times.",
            id, count
        ));
    }

    let baseline = sections
        .last()
        .cloned()
        .unwrap_or_else(WordSection::default);
    let source_paragraphs = std::mem::take(paragraphs);
    let source_sections = std::mem::take(sections);
    let mut base_segments =
        split_existing_sections(source_paragraphs, &source_sections, &baseline)?;

    // Split each original section into ordinary / multi-column runs. Runs never
    // cross an original section boundary. Adjacent targets with the same count
    // stay in the same run and therefore balance across the same Word section.
    let mut output_segments: Vec<(Vec<WordParagraph>, WordSection)> = Vec::new();
    for (segment_paragraphs, base_section) in base_segments.drain(..) {
        if segment_paragraphs.is_empty() {
            output_segments.push((Vec::new(), base_section));
            continue;
        }

        let mut runs: Vec<(Vec<WordParagraph>, Option<u32>)> = Vec::new();
        for paragraph in segment_paragraphs {
            let requested = effective.get(&paragraph.id).copied();
            if runs.last().map(|(_, cols)| *cols) == Some(requested) {
                runs.last_mut().expect("run exists").0.push(paragraph);
            } else {
                runs.push((vec![paragraph], requested));
            }
        }

        let run_count = runs.len();
        for (run_index, (run_paragraphs, requested)) in runs.into_iter().enumerate() {
            let mut section = base_section.clone();
            if let Some(cols) = requested {
                section.cols = Some(cols);
                section.id = format!("__col_wrap_{}__", run_paragraphs[0].id);
                // Force A4 page size for multi-column sections to ensure proper rendering.
                // Column sections can inherit non-A4 sizes from the base section,
                // which may cause rendering issues in Word.
                section.page_size_twips = Some(PageSize {
                    width: A4_WIDTH_TWIPS,
                    height: A4_HEIGHT_TWIPS,
                    orient: Some("portrait".to_string()),
                });
                section.page_size_mm = None;
            }
            // A boundary introduced inside an existing section must not force a
            // page break. The last run keeps the original section's break type.
            if run_index + 1 < run_count {
                section.section_type = Some("continuous".to_string());
            }
            output_segments.push((run_paragraphs, section));
        }
    }

    if output_segments.is_empty() {
        output_segments.push((Vec::new(), baseline));
    }

    let segment_count = output_segments.len();
    for (section_index, (segment_paragraphs, section)) in output_segments.into_iter().enumerate() {
        sections.push(section);
        paragraphs.extend(segment_paragraphs);
        if section_index + 1 < segment_count {
            paragraphs.push(make_section_break_marker(section_index, "", "boundary"));
        }
    }

    debug_assert_eq!(
        paragraphs
            .iter()
            .filter(|p| section_break_index(p).is_some())
            .count()
            + 1,
        sections.len()
    );
    Ok(())
}

/// Convert the writer's existing physical section representation into ordered
/// paragraph/property segments. Explicit markers take priority. When callers
/// supplied multiple sections without markers, mirror the writer's legacy even
/// split once and then make the result explicit.
fn split_existing_sections(
    source_paragraphs: Vec<WordParagraph>,
    source_sections: &[WordSection],
    baseline: &WordSection,
) -> Result<Vec<(Vec<WordParagraph>, WordSection)>, String> {
    let has_markers = source_paragraphs
        .iter()
        .any(|p| section_break_index(p).is_some());
    if has_markers {
        let mut segments = Vec::new();
        let mut current = Vec::new();
        for paragraph in source_paragraphs {
            if let Some(section_index) = section_break_index(&paragraph) {
                let section = source_sections.get(section_index).cloned().ok_or_else(|| {
                    format!(
                        "Section-break marker references missing section index {} ({} sections exist).",
                        section_index,
                        source_sections.len()
                    )
                })?;
                segments.push((std::mem::take(&mut current), section));
            } else {
                current.push(paragraph);
            }
        }
        segments.push((current, baseline.clone()));
        return Ok(segments);
    }

    if source_sections.len() <= 1 {
        return Ok(vec![(source_paragraphs, baseline.clone())]);
    }

    let section_count = source_sections.len();
    let total = source_paragraphs.len();
    let base = total / section_count;
    let mut distributed: Vec<Vec<WordParagraph>> = vec![Vec::new(); section_count];
    let mut section_index = 0usize;
    let mut in_current = 0usize;
    for (paragraph_index, paragraph) in source_paragraphs.into_iter().enumerate() {
        distributed[section_index].push(paragraph);
        in_current += 1;
        if section_index + 1 < section_count
            && in_current >= base
            && (paragraph_index + 1) + (section_count - section_index - 1) <= total
        {
            section_index += 1;
            in_current = 0;
        }
    }

    Ok(distributed
        .into_iter()
        .zip(source_sections.iter().cloned())
        .collect())
}

fn section_break_index(paragraph: &WordParagraph) -> Option<usize> {
    paragraph
        .id
        .strip_prefix("__sect_break_")
        .and_then(|rest| rest.strip_suffix("__"))
        .and_then(|value| value.parse::<usize>().ok())
}

/// Build a synthetic `<w:sectPr>`-carrying paragraph whose id matches
/// the writer's `__sect_break_<idx>__` convention. The writer embeds
/// the sectPr of section `idx` inside the paragraph's `<w:pPr>`, which
/// is exactly what we want for an in-paragraph section break.
///
/// The writer parses the marker by stripping the literal `__sect_break_`
/// prefix and the literal `__` suffix; anything in between must be a
/// single non-negative integer (see `writer::section_break_section_idx`).
/// We therefore MUST emit the id in the exact form `__sect_break_<idx>__`
/// — there is no room for additional metadata. Per-wrap uniqueness is
/// guaranteed by passing a distinct `section_idx` for each marker we
/// emit, never reusing one.
fn make_section_break_marker(section_idx: usize, _target_id: &str, _role: &str) -> WordParagraph {
    WordParagraph {
        id: format!("__sect_break_{}__", section_idx),
        text: String::new(),
        style: None,
        runs: None,
        numbering: None,
        alignment: None,
        text_direction: None,
        page_break: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::office::WordDocument;
    use std::io::Read;

    fn temp_workspace() -> Option<String> {
        Some(std::env::temp_dir().to_string_lossy().into_owned())
    }

    fn make_paragraph(id: &str, text: &str) -> WordParagraph {
        WordParagraph {
            id: id.to_string(),
            text: text.to_string(),
            style: None,
            runs: None,
            numbering: None,
            alignment: None,
            text_direction: None,
            page_break: None,
        }
    }

    #[test]
    fn wraps_single_paragraph() {
        let mut doc = WordDocument::default();
        doc.paragraphs = vec![
            make_paragraph("a", "A"),
            make_paragraph("target", "T"),
            make_paragraph("z", "Z"),
        ];
        // Use the default WordSection::default() as the trailing section.
        doc.sections = vec![WordSection::default()];

        expand_paragraph_columns(&mut doc.paragraphs, &mut doc.sections, &[("target".into(), 2)])
            .expect("wrap should succeed");

        // After expansion: a, marker1, target, marker2, z.
        assert_eq!(doc.paragraphs.len(), 5);
        assert_eq!(doc.paragraphs[0].id, "a");
        assert!(doc.paragraphs[1].id.starts_with("__sect_break_"));
        assert_eq!(doc.paragraphs[2].id, "target");
        assert!(doc.paragraphs[3].id.starts_with("__sect_break_"));
        assert_eq!(doc.paragraphs[4].id, "z");

        // The two marker ids must reference two distinct sections,
        // and the wrap section must be between them.
        let mid_marker_idx = doc.paragraphs[1]
            .id
            .strip_prefix("__sect_break_")
            .and_then(|s| s.split("__").next())
            .and_then(|s| s.parse::<usize>().ok())
            .expect("first marker should parse");
        let end_marker_idx = doc.paragraphs[3]
            .id
            .strip_prefix("__sect_break_")
            .and_then(|s| s.split("__").next())
            .and_then(|s| s.parse::<usize>().ok())
            .expect("second marker should parse");
        assert_eq!(mid_marker_idx, 0, "first marker closes the leading section");
        assert_eq!(end_marker_idx, 1, "second marker closes the column section");

        // Sections: [baseline before, two-column run, baseline after].
        assert_eq!(doc.sections.len(), 3);
        assert_eq!(doc.sections[1].cols, Some(2));
        assert_eq!(
            doc.sections[1].section_type,
            Some("continuous".to_string()),
            "wrap section must be continuous so it doesn't force a page break"
        );
        assert_eq!(
            doc.paragraphs
                .iter()
                .filter(|p| section_break_index(p).is_some())
                .count()
                + 1,
            doc.sections.len(),
            "writer marker invariant must hold"
        );
    }

    #[test]
    fn cols_one_is_no_op() {
        let mut doc = WordDocument::default();
        doc.paragraphs = vec![make_paragraph("a", "A")];
        doc.sections = vec![WordSection::default()];

        expand_paragraph_columns(&mut doc.paragraphs, &mut doc.sections, &[("a".into(), 1)])
            .expect("cols=1 should be a no-op");
        assert_eq!(doc.paragraphs.len(), 1);
        assert_eq!(doc.sections.len(), 1);
    }

    #[test]
    fn rejects_invalid_columns() {
        let mut doc = WordDocument::default();
        doc.paragraphs = vec![make_paragraph("a", "A")];
        doc.sections = vec![WordSection::default()];

        let r = expand_paragraph_columns(
            &mut doc.paragraphs,
            &mut doc.sections,
            &[("a".into(), 0)],
        );
        assert!(r.is_err(), "cols=0 must be rejected");

        let r = expand_paragraph_columns(
            &mut doc.paragraphs,
            &mut doc.sections,
            &[("a".into(), 15)],
        );
        assert!(r.is_err(), "cols=15 must be rejected");
    }

    #[test]
    fn rejects_missing_paragraph_id() {
        let mut doc = WordDocument::default();
        doc.paragraphs = vec![make_paragraph("a", "A")];
        doc.sections = vec![WordSection::default()];

        let r = expand_paragraph_columns(
            &mut doc.paragraphs,
            &mut doc.sections,
            &[("nonexistent".into(), 2)],
        );
        assert!(r.is_err(), "unknown paragraph id must be rejected");
        assert!(
            r.unwrap_err().contains("nonexistent"),
            "error must name the missing id"
        );
    }

    #[test]
    fn wraps_multiple_paragraphs_in_order() {
        let mut doc = WordDocument::default();
        doc.paragraphs = vec![
            make_paragraph("a", "A"),
            make_paragraph("first", "F"),
            make_paragraph("middle", "M"),
            make_paragraph("last", "L"),
            make_paragraph("z", "Z"),
        ];
        doc.sections = vec![WordSection::default()];

        expand_paragraph_columns(
            &mut doc.paragraphs,
            &mut doc.sections,
            &[("first".into(), 2), ("last".into(), 3)],
        )
        .expect("two wraps should succeed");

        // Expect: a, m1, first, m2, middle, m3, last, m4, z
        assert_eq!(doc.paragraphs.len(), 9);
        let ids: Vec<&str> = doc.paragraphs.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(ids[0], "a");
        assert!(ids[1].starts_with("__sect_break_"));
        assert_eq!(ids[2], "first");
        assert!(ids[3].starts_with("__sect_break_"));
        assert_eq!(ids[4], "middle");
        assert!(ids[5].starts_with("__sect_break_"));
        assert_eq!(ids[6], "last");
        assert!(ids[7].starts_with("__sect_break_"));
        assert_eq!(ids[8], "z");

        // Sections: baseline, first wrap, baseline, last wrap, baseline.
        assert_eq!(doc.sections.len(), 5);
        assert_eq!(doc.sections[1].cols, Some(2));
        assert_eq!(doc.sections[3].cols, Some(3));
    }

    #[test]
    fn adjacent_paragraphs_with_same_columns_share_one_section() {
        let mut doc = WordDocument::default();
        doc.paragraphs = vec![
            make_paragraph("a", "A"),
            make_paragraph("first", "F"),
            make_paragraph("second", "S"),
            make_paragraph("z", "Z"),
        ];
        doc.sections = vec![WordSection::default()];

        expand_paragraph_columns(
            &mut doc.paragraphs,
            &mut doc.sections,
            &[("first".into(), 2), ("second".into(), 2)],
        )
        .expect("adjacent paragraphs should share a column section");

        assert_eq!(doc.sections.len(), 3);
        assert_eq!(doc.sections.iter().filter(|s| s.cols == Some(2)).count(), 1);
        assert_eq!(doc.paragraphs.len(), 6);
        assert_eq!(doc.paragraphs[2].id, "first");
        assert_eq!(doc.paragraphs[3].id, "second");
    }

    #[test]
    fn no_targets_is_no_op() {
        let mut doc = WordDocument::default();
        doc.paragraphs = vec![make_paragraph("a", "A")];
        doc.sections = vec![WordSection::default()];

        expand_paragraph_columns(&mut doc.paragraphs, &mut doc.sections, &[])
            .expect("empty targets should be a no-op");
        assert_eq!(doc.paragraphs.len(), 1);
        assert_eq!(doc.sections.len(), 1);
    }

    #[test]
    fn empty_sections_uses_default_baseline() {
        let mut doc = WordDocument::default();
        doc.paragraphs = vec![make_paragraph("a", "A")];
        // No sections at all.
        doc.sections.clear();

        expand_paragraph_columns(&mut doc.paragraphs, &mut doc.sections, &[("a".into(), 2)])
            .expect("wrap with no existing sections should fall back to default");
        assert_eq!(doc.paragraphs.len(), 1);
        assert_eq!(doc.sections.len(), 1);
        assert_eq!(doc.sections[0].cols, Some(2));
    }

    /// End-to-end check: build the writer's actual document XML and
    /// assert that (a) the wrap section emits `<w:cols w:num="2"/>`,
    /// (b) the trailing section emits `<w:cols w:space="720"/>` (the
    /// single-column marker), and (c) the marker ids are exactly the
    /// `__sect_break_<idx>__` shape the writer recognises.
    #[test]
    fn wraps_via_writer_emits_cols_xml() {
        use crate::office::build_document_xml;
        let mut doc = WordDocument::default();
        doc.paragraphs = vec![
            make_paragraph("a", "pre"),
            make_paragraph("target", "two-column body"),
            make_paragraph("z", "post"),
        ];
        doc.sections = vec![WordSection::default()];

        expand_paragraph_columns(&mut doc.paragraphs, &mut doc.sections, &[("target".into(), 2)])
            .expect("wrap should succeed");

        let xml = build_document_xml(&doc);
        assert_eq!(
            xml.matches("<w:sectPr>").count(),
            doc.sections.len(),
            "writer should emit exactly one sectPr per physical section"
        );
        assert!(
            xml.contains(r#"<w:cols w:num="2""#),
            "wrap section must emit <w:cols w:num=\"2\"/>; got: {}",
            xml
        );
        assert!(
            xml.contains(r#"<w:cols w:space="720""#),
            "at least one single-column marker must appear; got: {}",
            xml
        );
        // The target paragraph must NOT be eaten by the section-break
        // handling (its text should still be in the body).
        assert!(
            xml.contains("two-column body"),
            "target paragraph text must survive the wrap; got: {}",
            xml
        );
    }

    /// End-to-end integration test: drive the CreateWordDocTool with
    /// a `columns: 2` hint on a paragraph and verify the resulting
    /// docx emits `<w:cols w:num="2"/>` for the wrap section while
    /// the rest of the document stays single-column. This simulates
    /// the full path (tool parse → expand → write → read XML).
    #[tokio::test]
    async fn tool_columns_hint_produces_two_column_section() {
        use crate::agent::tools::office::CreateWordDocTool;
        use crate::office::read_word_document;
        use std::path::PathBuf;

        let mut path = std::env::temp_dir();
        path.push(format!(
            "inkuo_cols_test_{}.docx",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));

        let tool = CreateWordDocTool::new();
        let payload = serde_json::json!({
            "path": path.to_string_lossy(),
            "elements": [
                {"type": "paragraph", "id": "pre", "text": "Before the column section"},
                {"type": "paragraph", "id": "col_target", "text": "This paragraph should be in two columns", "columns": 2},
                {"type": "paragraph", "id": "post", "text": "After the column section"},
            ]
        });

        tool.execute(payload, temp_workspace())
            .await
            .expect("tool should succeed with columns hint");

        // Read the file back and inspect the XML.
        let bytes = std::fs::read(&path).expect("file should exist");
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(&bytes)).expect("valid zip");
        let mut doc_xml = String::new();
        zip.by_name("word/document.xml")
            .expect("document.xml")
            .read_to_string(&mut doc_xml)
            .expect("readable");

        // The two-column section must appear somewhere.
        assert!(
            doc_xml.contains(r#"<w:cols w:num="2""#),
            "expected <w:cols w:num=\"2\"/> for the wrap section; got: {}",
            doc_xml
        );

        // The "pre" and "post" paragraphs should survive intact.
        assert!(
            doc_xml.contains("Before the column section"),
            "pre paragraph should survive; got: {}",
            doc_xml
        );
        assert!(
            doc_xml.contains("After the column section"),
            "post paragraph should survive; got: {}",
            doc_xml
        );

        // Clean up.
        let _ = std::fs::remove_file(&path);
    }

    /// Verify that `columns` on a `body` component block also produces the
    /// two-column wrap.
    #[tokio::test]
    async fn body_component_columns_hint_works() {
        use crate::agent::tools::office::CreateWordDocTool;

        let mut path = std::env::temp_dir();
        path.push(format!(
            "inkuo_body_cols_test_{}.docx",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));

        let tool = CreateWordDocTool::new();
        let payload = serde_json::json!({
            "path": path.to_string_lossy(),
            "elements": [
                {"type": "body", "id": "before_body", "text": "Before body block"},
                {"type": "body", "id": "col_body", "text": "This body paragraph should be in three columns", "columns": 3},
                {"type": "body", "id": "after_body", "text": "After body block"},
            ]
        });

        tool.execute(payload, temp_workspace())
            .await
            .expect("tool should succeed with body columns hint");

        let bytes = std::fs::read(&path).expect("file should exist");
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(&bytes)).expect("valid zip");
        let mut doc_xml = String::new();
        zip.by_name("word/document.xml")
            .expect("document.xml")
            .read_to_string(&mut doc_xml)
            .expect("readable");

        assert!(
            doc_xml.contains(r#"<w:cols w:num="3""#),
            "expected <w:cols w:num=\"3\"/> for the body columns wrap; got: {}",
            doc_xml
        );

        let _ = std::fs::remove_file(&path);
    }

    /// Regression for the visible failure: adjacent body paragraphs with the
    /// same `columns` value must share one balanced physical section.
    #[tokio::test]
    async fn adjacent_body_columns_emit_one_balanced_section() {
        use crate::agent::tools::office::CreateWordDocTool;

        let mut path = std::env::temp_dir();
        path.push(format!(
            "inkuo_adjacent_body_cols_test_{}.docx",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));

        let payload = serde_json::json!({
            "path": path.to_string_lossy(),
            "elements": [
                {"type": "body", "id": "intro", "text": "Full-width introduction"},
                {"type": "body", "id": "column_left", "text": "First column paragraph", "columns": 2},
                {"type": "body", "id": "column_right", "text": "Second column paragraph", "columns": 2},
                {"type": "body", "id": "outro", "text": "Full-width conclusion"},
            ]
        });

        CreateWordDocTool::new()
            .execute(payload, temp_workspace())
            .await
            .expect("adjacent body column hints should succeed");

        let bytes = std::fs::read(&path).expect("file should exist");
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(&bytes)).expect("valid zip");
        let mut doc_xml = String::new();
        zip.by_name("word/document.xml")
            .expect("document.xml")
            .read_to_string(&mut doc_xml)
            .expect("readable");

        assert_eq!(
            doc_xml.matches(r#"<w:cols w:num="2""#).count(),
            1,
            "adjacent paragraphs must share exactly one two-column section"
        );
        assert_eq!(
            doc_xml.matches("<w:sectPr>").count(),
            3,
            "full-width / two-column / full-width requires exactly three sections"
        );

        let _ = std::fs::remove_file(&path);
    }

    /// Regression test: when both `sections[]` and `columns` are provided,
    /// the column wrap should work correctly without being overwritten by
    /// user-provided sections.
    ///
    /// Bug: In the original code, expand_paragraph_columns was called BEFORE
    /// convert_sections, which meant the column-wrap sections were
    /// completely overwritten by user sections. This test verifies the fix.
    #[tokio::test]
    async fn columns_with_sections_both_provided() {
        use crate::agent::tools::office::CreateWordDocTool;

        let mut path = std::env::temp_dir();
        path.push(format!(
            "inkuo_cols_sects_test_{}.docx",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));

        let tool = CreateWordDocTool::new();

        // Test 1: sections[] with columns - the key regression case
        let payload = serde_json::json!({
            "path": path.to_string_lossy(),
            "elements": [
                {"type": "paragraph", "id": "pre", "text": "Before column section"},
                {"type": "paragraph", "id": "col_target", "text": "This should be in two columns", "columns": 2},
                {"type": "paragraph", "id": "post", "text": "After column section"},
            ],
            "sections": [
                {"id": "main", "cols": 1}
            ]
        });

        tool.execute(payload, temp_workspace())
            .await
            .expect("tool should succeed with both sections and columns");

        let bytes = std::fs::read(&path).expect("file should exist");
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(&bytes)).expect("valid zip");
        let mut doc_xml = String::new();
        zip.by_name("word/document.xml")
            .expect("document.xml")
            .read_to_string(&mut doc_xml)
            .expect("readable");

        // The two-column section MUST appear - this was the bug!
        assert!(
            doc_xml.contains(r#"<w:cols w:num="2""#),
            "BUG: columns hint was overwritten by sections[]. cols=2 must appear; got: {}",
            doc_xml
        );

        // Content should survive
        assert!(
            doc_xml.contains("Before column section"),
            "pre paragraph should survive; got: {}",
            doc_xml
        );
        assert!(
            doc_xml.contains("This should be in two columns"),
            "target paragraph should survive; got: {}",
            doc_xml
        );
        assert!(
            doc_xml.contains("After column section"),
            "post paragraph should survive; got: {}",
            doc_xml
        );

        let _ = std::fs::remove_file(&path);
    }

    /// Regression test: columns with append mode
    #[tokio::test]
    async fn columns_with_append_mode() {
        use crate::agent::tools::office::CreateWordDocTool;

        let mut path = std::env::temp_dir();
        path.push(format!(
            "inkuo_cols_append_test_{}.docx",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));

        let tool = CreateWordDocTool::new();

        // Create initial document
        let initial = serde_json::json!({
            "path": path.to_string_lossy(),
            "elements": [
                {"type": "paragraph", "id": "init", "text": "Initial paragraph"},
            ]
        });
        tool.execute(initial, temp_workspace()).await.expect("initial creation should succeed");

        // Append with columns
        let append = serde_json::json!({
            "path": path.to_string_lossy(),
            "append": true,
            "elements": [
                {"type": "paragraph", "id": "col_target", "text": "Column section text", "columns": 2},
            ]
        });
        tool.execute(append, temp_workspace()).await.expect("append with columns should succeed");

        let bytes = std::fs::read(&path).expect("file should exist");
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(&bytes)).expect("valid zip");
        let mut doc_xml = String::new();
        zip.by_name("word/document.xml")
            .expect("document.xml")
            .read_to_string(&mut doc_xml)
            .expect("readable");

        // The two-column section must appear
        assert!(
            doc_xml.contains(r#"<w:cols w:num="2""#),
            "columns in append mode should work; got: {}",
            doc_xml
        );

        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn body_columns_work_in_progressive_append_mode() {
        use crate::agent::tools::office::CreateWordDocTool;

        let mut path = std::env::temp_dir();
        path.push(format!(
            "inkuo_body_cols_append_test_{}.docx",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));

        let tool = CreateWordDocTool::new();
        tool.execute(
            serde_json::json!({
                "path": path.to_string_lossy(),
                "elements": [{"type": "body", "id": "initial", "text": "Initial"}]
            }),
            temp_workspace(),
        )
        .await
        .expect("initial creation should succeed");

        tool.execute(
            serde_json::json!({
                "path": path.to_string_lossy(),
                "append": true,
                "elements": [
                    {"type": "body", "id": "append_left", "text": "Left", "columns": 2},
                    {"type": "body", "id": "append_right", "text": "Right", "columns": 2}
                ]
            }),
            temp_workspace(),
        )
        .await
        .expect("component paragraphs must be appended before column ids are resolved");

        let bytes = std::fs::read(&path).expect("file should exist");
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(&bytes)).expect("valid zip");
        let mut doc_xml = String::new();
        zip.by_name("word/document.xml")
            .expect("document.xml")
            .read_to_string(&mut doc_xml)
            .expect("readable");
        assert_eq!(doc_xml.matches(r#"<w:cols w:num="2""#).count(), 1);
        assert!(doc_xml.contains("Left"));
        assert!(doc_xml.contains("Right"));

        let _ = std::fs::remove_file(&path);
    }
}
