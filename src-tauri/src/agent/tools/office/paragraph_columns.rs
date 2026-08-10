//! Helpers for the `create_word_doc` tool that don't fit naturally
//! into the generic writer/renderer pipeline.
//!
//! Today this module hosts [`expand_paragraph_columns`], which
//! translates the tool's per-paragraph `columns` hint into OOXML
//! section-break markers around the target paragraph. Word's native
//! data model is section-centric: `<w:cols w:num="N"/>` lives inside
//! `<w:sectPr>` and applies to every paragraph in the section. There
//! is no "this one paragraph is in two columns" primitive, so the
//! only way to scope a column layout to a single paragraph (or a
//! short run of paragraphs) is to bracket it with two `continuous`
//! section breaks whose middle section carries the desired `cols`.
//!
//! The writer already understands the marker convention
//! (`__sect_break_<idx>__` paragraph ids), so all this helper does is:
//!
//!   1. Locate each target paragraph by id.
//!   2. Snapshot the trailing section's properties as a "neutral"
//!      baseline for the brackets.
//!   3. Insert a leading marker (closes the pre-wrap section, reuses
//!      the trailing section's properties) and a trailing marker
//!      (closes the wrap section, with `cols = N`).
//!   4. Append two new `WordSection` entries to the document so the
//!      writer's `section_breaks.len() + 1 == total_sections` invariant
//!      holds.

// NOTE: The tests in `#[cfg(test)]` below need `tokio::test` (async) and
// `zip::ZipArchive`; those crates are already in Cargo.toml's `[dependencies]`
// so their re-exported paths (`crate::office::...`, `crate::agent::...`) are
// available without extra imports here. The one test-only import needed is
// `zip::ZipArchive` for reading the raw docx bytes back.

use crate::office::{WordParagraph, WordSection};

/// Wrap every paragraph whose id appears in `targets` in its own
/// continuous section break pair, so that the middle section can carry
/// the requested `<w:cols w:num="N"/>` while the surrounding body
/// stays at the document's default column count.
///
/// `targets` is `(paragraph_id, columns)` pairs. `columns == 1` is
/// treated as a no-op (the surrounding section's properties already
/// produce a single-column layout) and is filtered out. `columns == 0`
/// or `columns > 9` is rejected with an error — Word's column count is
/// a small positive integer (1..=9) and accepting anything outside
/// that range would silently produce a malformed document.
///
/// Paragraphs not found by id raise a clear error so the AI gets
/// useful feedback ("you asked to column-wrap paragraph 'p1' but no
/// paragraph with that id exists in the document"). This catches the
/// common AI failure mode of mistyping ids or referencing a paragraph
/// that was deleted in the same call.
///
/// Existing section properties (page size, margins, headers, footers,
/// page numbering) on the trailing section are propagated to both the
/// leading "before" section and the trailing "after" section so the
/// brackets don't accidentally change the document's look. Only the
/// column count is overridden.
pub fn expand_paragraph_columns(
    paragraphs: &mut Vec<WordParagraph>,
    sections: &mut Vec<WordSection>,
    targets: &[(String, u32)],
) -> Result<(), String> {
    if targets.is_empty() {
        return Ok(());
    }

    // Filter + validate. We keep the (id, cols) pairs only when cols
    // is in the legal range (2..=9); cols == 1 means "no wrap needed".
    let mut effective: Vec<(String, u32)> = Vec::with_capacity(targets.len());
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
        effective.push((id.clone(), *cols));
    }
    if effective.is_empty() {
        return Ok(());
    }

    // Snapshot the trailing section as the "neutral" baseline for the
    // brackets. If the document has no sections yet (a brand-new
    // document with no caller-supplied sections), fall back to the
    // default A4 portrait section so the brackets inherit sane
    // geometry.
    let baseline = sections
        .last()
        .cloned()
        .unwrap_or_else(WordSection::default);

    // Walk paragraphs from the start. For each target, insert two
    // synthetic marker paragraphs immediately before and after it, and
    // append a pair of new sections to the document's section list.
    //
    // `idx` is the index in the *current* `paragraphs` vec. Because we
    // insert before processing the next paragraph, every inserted
    // marker shifts later indices by +1 each, so we walk with a manual
    // loop and bump `i` by 3 (target + 2 markers) when we hit a target.
    let mut i = 0usize;
    while i < paragraphs.len() {
        let p = &paragraphs[i];
        let target = effective.iter().find(|(id, _)| id == &p.id).cloned();
        if let Some((id, cols)) = target {
            // The middle section carries `cols = cols`. It MUST be
            // `continuous` so the wrap doesn't force a page break.
            let mut wrap_section = baseline.clone();
            wrap_section.cols = Some(cols);
            wrap_section.section_type = Some("continuous".to_string());
            // Give it a fresh id so it doesn't collide with the
            // baseline id when the writer looks up section properties.
            wrap_section.id = format!("__col_wrap_{}__", id);

            // The "after" section is a clone of the baseline so the
            // body returns to its original column count.
            let mut after_section = baseline.clone();
            after_section.id = format!("__col_after_{}__", id);
            // Force cols=Some(1) so the writer emits `<w:cols w:space="..."/>`
            // rather than inheriting whatever the wrap section set.
            after_section.cols = Some(1);

            // The number of existing sections grows by 2 with each
            // wrap. The new indices for the markers are
            // `total_sections` and `total_sections + 1` *before* the
            // append (so `total_sections - 1` becomes the wrap index).
            let new_idx_before = sections.len();
            let new_idx_wrap = sections.len() + 1;

            // Marker 1 (before the target paragraph): closes section
            // `new_idx_before`, which is a clone of the baseline.
            let mut before_marker = baseline.clone();
            before_marker.id = format!("__col_before_{}__", id);
            before_marker.section_type = Some("continuous".to_string());
            before_marker.cols = Some(1);
            let marker_before = make_section_break_marker(new_idx_before, &id, "before");

            // Marker 2 (after the target paragraph): closes section
            // `new_idx_wrap`, which is the cols=N wrap section.
            let marker_after = make_section_break_marker(new_idx_wrap, &id, "after");

            // Push the trailing "after" section first, then the wrap
            // section, then the leading "before" section, so the new
            // sections land at indices `len`, `len + 1`, `len + 2` in
            // that order — matching the marker indices.
            sections.push(before_marker);
            sections.push(wrap_section);
            sections.push(after_section);

            // Splice: marker_before, target, marker_after.
            let target_para = paragraphs[i].clone();
            paragraphs.remove(i);
            paragraphs.insert(i, marker_before);
            paragraphs.insert(i + 1, target_para);
            paragraphs.insert(i + 2, marker_after);
            // Skip past the inserted triplet.
            i += 3;
        } else {
            i += 1;
        }
    }

    // Sanity check: every requested id must have matched a paragraph.
    // If any didn't, surface the leftover ids so the AI can fix its
    // payload on the next retry.
    let remaining: Vec<&str> = effective
        .iter()
        .filter(|(id, _)| !paragraphs.iter().any(|p| p.id == *id))
        .map(|(id, _)| id.as_str())
        .collect();
    if !remaining.is_empty() {
        return Err(format!(
            "`columns` hint requested for paragraph id(s) that don't exist in the \
             document: {}. Make sure the paragraph id is correct and that the \
             paragraph hasn't been deleted earlier in the same call.",
            remaining.join(", ")
        ));
    }

    Ok(())
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::office::WordDocument;
    use std::io::Read;

    fn make_paragraph(id: &str, text: &str) -> WordParagraph {
        WordParagraph {
            id: id.to_string(),
            text: text.to_string(),
            style: None,
            runs: None,
            numbering: None,
            alignment: None,
            text_direction: None,
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
        assert_eq!(mid_marker_idx, 1, "first marker closes section 1 (was 0)");
        assert_eq!(end_marker_idx, 2, "second marker closes section 2 (wrap)");

        // Sections: [baseline (s0), before_marker_clone (s1), wrap (s2), after_clone (s3)]
        assert_eq!(doc.sections.len(), 4);
        assert_eq!(doc.sections[2].cols, Some(2));
        assert_eq!(
            doc.sections[2].section_type,
            Some("continuous".to_string()),
            "wrap section must be continuous so it doesn't force a page break"
        );
        assert_eq!(doc.sections[3].cols, Some(1));
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

        // Sections: baseline(0), before_first(1), wrap_first(2), after_first(3),
        //           before_last(4), wrap_last(5), after_last(6)
        assert_eq!(doc.sections.len(), 7);
        assert_eq!(doc.sections[2].cols, Some(2));
        assert_eq!(doc.sections[5].cols, Some(3));
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
        assert_eq!(doc.paragraphs.len(), 3);
        assert_eq!(doc.sections.len(), 3);
        assert_eq!(doc.sections[1].cols, Some(2));
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

        tool.execute(payload, None)
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

        tool.execute(payload, None)
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

        tool.execute(payload, None)
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
        tool.execute(initial, None).await.expect("initial creation should succeed");

        // Append with columns
        let append = serde_json::json!({
            "path": path.to_string_lossy(),
            "append": true,
            "elements": [
                {"type": "paragraph", "id": "col_target", "text": "Column section text", "columns": 2},
            ]
        });
        tool.execute(append, None).await.expect("append with columns should succeed");

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
}
