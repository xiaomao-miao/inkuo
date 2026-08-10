//! End-to-end smoke test that simulates the user's "全面测试" doc.
//! Verifies that the three bug fixes (dynamic numbering, image
//! with id, section distribution) all work together in a realistic
//! payload.

use crate::office::docx::{
    build_dynamic_numbering_body, collect_referenced_num_ids, read_word_document, WordDocument,
    write_word_document_to_path,
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
        });
    }
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
            }]),
            numbering: Some(NumberingRef { num_id: 1, level: 0 }),
            alignment: None,
            text_direction: None,
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
            }]),
            numbering: Some(NumberingRef { num_id: 11, level: 0 }),
            alignment: None,
            text_direction: None,
        });
    }
    // Body paragraphs for section 1 (cols=1).
    for i in 0..4 {
        paragraphs.push(WordParagraph {
            id: format!("body-s1-{}", i),
            text: format!("Section 1 body {}", i),
            style: Some("BodyParagraph".to_string()),
            runs: None,
            numbering: None,
            alignment: None,
            text_direction: None,
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
    };

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

    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(&png_path);
}
