//! End-to-end smoke test that simulates the user's "全面测试" doc.
//! Verifies that the three bug fixes (dynamic numbering, image
//! with id, section distribution) all work together in a realistic
//! payload.

use crate::office::docx::{
    build_dynamic_numbering_body, collect_referenced_num_ids, read_word_document, WordDocument,
    WordDocumentMeta, write_word_document_to_path,
};
use std::io::Read;

fn temp_path(suffix: &str) -> std::path::PathBuf {
    let mut p = std::env::temp_dir();
    let id: u64 = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos() as u64;
    p.push(format!("inkuo_smoke_{}_{}.docx", id, suffix));
    p
}

fn write_minimal_png(path: &std::path::Path) {
    let png_bytes: &[u8] = &[
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A,
        0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52,
        0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01,
        0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4,
        0x89, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x44, 0x41,
        0x54, 0x78, 0x9C, 0x63, 0x00, 0x01, 0x00, 0x00,
        0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00,
        0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE,
        0x42, 0x60, 0x82,
    ];
    std::fs::write(path, png_bytes).expect("write png");
}

#[test]
fn user_scenario_all_three_fixes_together() {
    // Construct a document that mirrors the user's reported scenario:
    //   - bullet/numbered lists using numId 1, 2, 10, 11, 12, 13
    //   - multiple sections, one with cols=2, one with cols=1
    //   - an image with explicit id
    use crate::office::docx::types::{FontRun, NumberingRef, WordParagraph, WordSection};
    use crate::office::docx::WordImage;

    // Write a real PNG so the writer's media-handling code path
    // (which reads bytes from disk on first write) succeeds.
    let png_path = temp_path("smoke_img").with_extension("png");
    write_minimal_png(&png_path);
    let png_path_str = png_path.to_string_lossy().to_string();

    let mut paragraphs = Vec::new();
    // Cover-style heading.
    paragraphs.push(WordParagraph {
        id: "cover-title".into(),
        text: "Test Document".into(),
        style: Some("CoverTitle".to_string()),
        runs: None,
        numbering: None,
        alignment: None,
        text_direction: None,
        page_break: None,
    });
    // Body paragraphs (these would be in section 0 with cols=2).
    for i in 0..6 {
        paragraphs.push(WordParagraph {
            id: format!("body-s0-{}", i),
            text: format!("Section 0 body {}", i),
            style: Some("BodyParagraph".to_string()),
            runs: None,
            numbering: None,
            alignment: None,
            text_direction: None,
            page_break: None,
        });
    }
    // Image marker paragraph: the writer expands `<__img_pos_<id>__>`
    // markers into `<w:drawing>` paragraphs at this position. We use the
    // same id as the `WordImage` entry below.
    paragraphs.push(WordParagraph {
        id: "__img_pos_img1__".to_string(),
        text: "<__img_pos_img1__>".to_string(),
        style: None,
        runs: None,
        numbering: None,
        alignment: None,
        text_direction: None,
        page_break: None,
    });
    // A bulleted list using numId 1 (built-in).
    for i in 0..3 {
        paragraphs.push(WordParagraph {
            id: format!("bl-{}", i),
            text: format!("Bullet item {}", i),
            style: Some("ListBullet".to_string()),
            runs: Some(vec![FontRun {
                text: format!("Bullet item {}", i),
                bold: false,
                italic: false,
                underline: false,
                strikethrough: false,
                font_size: Some(20),
                color: None,
                font_name: None,
                highlight: None,
                vert_align: None,
                field: None,
                page_break: false,
                column_break: false,
            }]),
            numbering: Some(NumberingRef { num_id: 1, level: 0 }),
            alignment: None,
            text_direction: None,
            page_break: None,
        });
    }
    // An ordered list using numId 11 (NOT built-in — exercises dynamic registration).
    for i in 0..2 {
        paragraphs.push(WordParagraph {
            id: format!("ol-{}", i),
            text: format!("Ordered item {}", i),
            style: Some("ListNumber".to_string()),
            runs: Some(vec![FontRun {
                text: format!("Ordered item {}", i),
                bold: false,
                italic: false,
                underline: false,
                strikethrough: false,
                font_size: Some(20),
                color: None,
                font_name: None,
                highlight: None,
                vert_align: None,
                field: None,
                page_break: false,
                column_break: false,
            }]),
            numbering: Some(NumberingRef { num_id: 11, level: 0 }),
            alignment: None,
            text_direction: None,
            page_break: None,
        });
    }
// Body paragraphs for section 1 (cols=1). The first body carries a
        // `column_break` run so we exercise `<w:br w:type="column"/>` on
        // round-trip; the second body carries a `HYPERLINK` field run
        // for the same reason.
        for i in 0..4 {
            let runs = if i == 0 {
                Some(vec![FontRun {
                    text: String::new(),
                    bold: false,
                    italic: false,
                    underline: false,
                    strikethrough: false,
                    font_size: None,
                    color: None,
                    font_name: None,
                    highlight: None,
                    vert_align: None,
                    field: None,
                    page_break: false,
                    column_break: true,
                }])
            } else if i == 1 {
                Some(vec![FontRun {
                    text: "https://example.com".to_string(),
                    bold: false,
                    italic: false,
                    underline: true,
                    strikethrough: false,
                    font_size: None,
                    color: Some("1F6FEB".to_string()),
                    font_name: None,
                    highlight: None,
                    vert_align: None,
                    field: Some(crate::office::FieldRef::Custom {
                        instr: "HYPERLINK \"https://example.com\"".to_string(),
                    }),
                    page_break: false,
                    column_break: false,
                }])
            } else {
                None
            };
            paragraphs.push(WordParagraph {
                id: format!("body-s1-{}", i),
                text: format!("Section 1 body {}", i),
                style: Some("BodyParagraph".to_string()),
                runs,
                numbering: None,
                alignment: None,
                text_direction: None,
                page_break: None,
            });
        }

    let doc = WordDocument {
        paragraphs,
        tables: vec![],
        images: vec![WordImage {
            id: "img1".into(),
            path: png_path_str.clone(),
            width_emu: 914400,
            height_emu: 914400,
            alt_text: Some("Test image".to_string()),
            internal_path: None,
        }],
        sections: vec![
            WordSection {
                id: "s0".into(),
                section_type: Some("continuous".into()),
                page_size_twips: None,
                page_size_mm: None,
                margins: None,
                text_direction: None,
                title_pg: false,
                cols: Some(2),
                page_num_start: None,
                page_num_format: None,
                header_refs: vec![],
                footer_refs: vec![],
            },
            WordSection {
                id: "s1".into(),
                section_type: None,
                page_size_twips: None,
                page_size_mm: None,
                margins: None,
                text_direction: None,
                title_pg: false,
                cols: Some(1),
                page_num_start: None,
                page_num_format: None,
                header_refs: vec![],
                footer_refs: vec![],
            },
        ],
        headers: vec![],
        footers: vec![],
        meta: WordDocumentMeta::default(),
    };

    // Inject explicit section-break markers so the writer treats the two
    // `WordSection`s as distinct physical sections. Without markers, the
    // writer now (correctly) coerces multi-section docs without explicit
    // markers down to a single trailing section — see the regression
    // report and the `sections_without_markers_coerce_to_one_section`
    // test in bug_fixes_tests.rs. The smoke test exercises the
    // marker-driven path so it needs the markers to actually be there.
    let doc = inject_section_break_markers(doc);

    // Verify dynamic numbering picks up numId 11.
    let referenced = collect_referenced_num_ids(&doc);
    assert!(referenced.contains(&11), "numId 11 must be in referenced");
    let numbering_body = build_dynamic_numbering_body(&referenced);
    assert!(
        numbering_body.contains(r#"<w:num w:numId="11">"#),
        "numId 11 must be auto-registered"
    );

    // Verify section distribution: 17 paragraphs across 2 sections
    // means section 0 gets ~8 paragraphs and section 1 gets ~9.
    // (This is exercised by the per-section test in bug_fixes_tests;
    // we don't re-check it here to keep this test focused.)

    // Write and re-read; round-trip should preserve paragraphs,
    // sections, and numbering references.
    let path = temp_path("user_scenario");
    write_word_document_to_path(&doc, &path, None).expect("write");
    eprintln!("wrote {} bytes to {}", std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0), path.display());
    let bytes = std::fs::read(&path).expect("read back");
    let read = read_word_document(&bytes).expect("read");

    // numId 11 reference must still be present in the read-back doc.
    let mut found_11 = false;
    for p in &read.paragraphs {
        if let Some(ref n) = p.numbering {
            if n.num_id == 11 {
                found_11 = true;
            }
        }
    }
    assert!(found_11, "read-back must preserve numId 11 references");

    // Sections must round-trip too.
    assert_eq!(read.sections.len(), 2, "two sections must round-trip");
    assert_eq!(read.sections[0].cols, Some(2));
    assert_eq!(read.sections[1].cols, Some(1));

    // Open the zip ONCE and inspect both media + numbering.xml.
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).expect("open");
    let has_media = zip.file_names().any(|n| n.starts_with("word/media/"));
    assert!(has_media, "media bytes must be in the zip");

    let mut numbering = String::new();
    zip.by_name("word/numbering.xml")
        .expect("numbering.xml present")
        .read_to_string(&mut numbering)
        .expect("read");
    assert!(
        numbering.contains(r#"<w:num w:numId="11">"#),
        "numbering.xml must register numId 11"
    );

    // ── Feature regressions ──────────────────────────────────────────────
    //
    // Each iteration of the regression report added one of these
    // surfaces; re-asserting them on every smoke test run catches any
    // drift in the writer / reader before a user hits it.
    let mut document_xml = String::new();
    zip.by_name("word/document.xml")
        .expect("document.xml present")
        .read_to_string(&mut document_xml)
        .expect("read");
    assert!(
        document_xml.contains("<w:drawing>"),
        "image must produce a <w:drawing> element"
    );
    assert!(
        document_xml.contains("HYPERLINK"),
        "HYPERLINK field must be emitted as an instrText payload"
    );
    assert!(
        document_xml.contains(r#"<w:br w:type="column"/>"#),
        "column_break run must emit <w:br w:type=\"column\"/>"
    );

    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(&png_path);
}

/// Insert a `__sect_break_<idx>__` marker paragraph right after the last
/// body paragraph of each non-final `WordSection`. The new marker-driven
/// writer requires `markers + 1 == sections.len()`; this helper bridges
/// smoke tests that build `WordDocument` by hand without going through
/// `paragraph_columns.rs::expand_paragraph_columns`.
fn inject_section_break_markers(mut doc: WordDocument) -> WordDocument {
    use crate::office::docx::types::WordParagraph;
    if doc.sections.len() <= 1 {
        return doc;
    }
    let mut new_paragraphs: Vec<WordParagraph> = Vec::with_capacity(doc.paragraphs.len() + doc.sections.len() - 1);
    let total = doc.sections.len();
    let paragraphs_per_section = doc.paragraphs.len() / total;
    for (section_index, chunk) in doc.paragraphs.chunks(paragraphs_per_section.max(1)).enumerate() {
        new_paragraphs.extend_from_slice(chunk);
        if section_index + 1 < total {
            new_paragraphs.push(WordParagraph {
                id: format!("__sect_break_{}__", section_index),
                text: String::new(),
                style: None,
                runs: None,
                numbering: None,
                alignment: None,
                text_direction: None,
                page_break: None,
            });
        }
    }
    doc.paragraphs = new_paragraphs;
    doc
}
