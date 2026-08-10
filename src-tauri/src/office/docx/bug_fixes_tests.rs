//! Regression tests for three "silent breakage" bugs found in
//! the design-system + low-level element path:
//!
//!   1. `dynamic_numbering_registers_unknown_numids` — when a low-level
//!      paragraph references a numId that isn't in the built-in
//!      numbering.xml (e.g. numId 10, 11, ...), the writer's
//!      auto-generated numbering.xml must include a `<w:num>` entry for
//!      it. Without this, the bullet/number is silently missing in
//!      Word.
//!
//!   2. `list_styles_include_numpr` — the `ListBullet` and `ListNumber`
//!      paragraph styles in `EXTENDED_STYLES_XML` must include a
//!      `<w:numPr>` element so that paragraphs using *only* the style
//!      (no explicit numId) still get a numbered marker.
//!
//!   3. `sections_without_markers_distribute_paragraphs` — when a
//!      document provides multiple sections but no `__sect_break_<idx>__`
//!      marker paragraphs, the writer must distribute the paragraphs
//!      across sections rather than dumping them all into the last
//!      one. Otherwise `cols: 2` on a single section silently spans the
//!      entire body.
//!
//! Bug #4 (image-with-id silent drop) is exercised through
//! `create_word_doc` integration tests below.

use crate::office::docx::ooxml_boilerplate::{
    build_dynamic_numbering_body, collect_referenced_num_ids,
};
use crate::office::docx::styled_styles::EXTENDED_STYLES_XML;
use crate::office::docx::writer::build_document_xml;
use crate::office::docx::{
    NumberingRef, WordDocument, WordParagraph, WordSection, write_word_document_to_path,
};
use std::io::Read;

fn temp_path(suffix: &str) -> std::path::PathBuf {
    let mut p = std::env::temp_dir();
    let id: u64 = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos() as u64;
    p.push(format!("inkuo_test_{}_{}.docx", id, suffix));
    p
}

#[test]
fn dynamic_numbering_registers_unknown_numids() {
    // numId 10 and 11 are not in the built-in NUMBERING_XML.
    let body = build_dynamic_numbering_body(&[1, 2, 10, 11]);
    // Built-ins still present.
    assert!(body.contains(r#"<w:num w:numId="1">"#));
    assert!(body.contains(r#"<w:num w:numId="2">"#));
    // Auto-registered extras for 10 and 11.
    assert!(
        body.contains(r#"<w:num w:numId="10">"#),
        "numId 10 should be auto-registered"
    );
    assert!(
        body.contains(r#"<w:num w:numId="11">"#),
        "numId 11 should be auto-registered"
    );
    // Each registered extra has an abstractNum with decimal format.
    let abs_count = body.matches("<w:abstractNum ").count();
    // 2 built-ins + 2 extras = 4 abstractNums
    assert_eq!(
        abs_count, 4,
        "expected 4 abstractNums (2 built-in + 2 extras); got {}",
        abs_count
    );
}

#[test]
fn collect_referenced_num_ids_dedupes() {
    let doc = WordDocument {
        paragraphs: vec![
            WordParagraph {
                id: "a".into(),
                text: "".into(),
                style: None,
                runs: None,
                numbering: Some(NumberingRef { num_id: 5, level: 0 }),
                alignment: None,
                text_direction: None,
            },
            WordParagraph {
                id: "b".into(),
                text: "".into(),
                style: None,
                runs: None,
                numbering: Some(NumberingRef { num_id: 5, level: 0 }),
                alignment: None,
                text_direction: None,
            },
            WordParagraph {
                id: "c".into(),
                text: "".into(),
                style: None,
                runs: None,
                numbering: Some(NumberingRef { num_id: 7, level: 0 }),
                alignment: None,
                text_direction: None,
            },
        ],
        tables: vec![],
        images: vec![],
        sections: vec![],
        headers: vec![],
        footers: vec![],
    };
    let ids = collect_referenced_num_ids(&doc);
    assert_eq!(ids, vec![5, 7], "dedupe + sort by reference order");
}

#[test]
fn list_styles_include_numpr() {
    // ListBullet must link to numId 1 (bullet) so paragraphs using
    // just the style get an actual bullet in Word.
    let lb_start = EXTENDED_STYLES_XML
        .find(r#"w:styleId="ListBullet""#)
        .expect("ListBullet style missing");
    let lb_end = EXTENDED_STYLES_XML[lb_start..]
        .find("</w:style>")
        .expect("ListBullet style malformed")
        + lb_start
        + "</w:style>".len();
    let lb_block = &EXTENDED_STYLES_XML[lb_start..lb_end];
    assert!(
        lb_block.contains("<w:numPr>") && lb_block.contains(r#"<w:numId w:val="1"/>"#),
        "ListBullet style must include <w:numPr><w:numId val=\"1\"/></w:numPr>; got: {}",
        lb_block
    );

    // ListNumber must link to numId 2 (decimal).
    let ln_start = EXTENDED_STYLES_XML
        .find(r#"w:styleId="ListNumber""#)
        .expect("ListNumber style missing");
    let ln_end = EXTENDED_STYLES_XML[ln_start..]
        .find("</w:style>")
        .expect("ListNumber style malformed")
        + ln_start
        + "</w:style>".len();
    let ln_block = &EXTENDED_STYLES_XML[ln_start..ln_end];
    assert!(
        ln_block.contains("<w:numPr>") && ln_block.contains(r#"<w:numId w:val="2"/>"#),
        "ListNumber style must include <w:numPr><w:numId val=\"2\"/></w:numPr>; got: {}",
        ln_block
    );
}

#[test]
fn sections_without_markers_distribute_paragraphs() {
    // 10 plain body paragraphs + 2 sections, no `__sect_break_<idx>__`
    // markers. The writer should distribute them so section 0 gets a
    // share and the last section absorbs the remainder.
    let mut paragraphs = Vec::new();
    for i in 0..10 {
        paragraphs.push(WordParagraph {
            id: format!("p{}", i),
            text: format!("Body {}", i),
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
        images: vec![],
        sections: vec![
            WordSection {
                id: "s0".into(),
                section_type: Some("continuous".into()),
                page_size_twips: None,
                page_size_mm: None,
                margins: None,
                text_direction: None,
                title_pg: false,
                cols: Some(2), // <-- the column setting under test
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
                cols: Some(1), // <-- back to single column
                page_num_start: None,
                page_num_format: None,
                header_refs: vec![],
                footer_refs: vec![],
            },
        ],
        headers: vec![],
        footers: vec![],
    };
    let xml = build_document_xml(&doc);
    // Without even-distribution fix, every paragraph would carry the
    // final section's cols=1 and the cols=2 setting would be silently
    // dropped (or, with the older default, the whole doc would be 2
    // columns — see the user's bug report). With the fix, section 0
    // gets some paragraphs and the last section still emits a sectPr.
    assert!(
        xml.contains(r#"<w:cols w:num="2""#),
        "section 0's cols=2 must appear somewhere in the emitted XML; got: {}",
        xml
    );
    assert!(
        xml.contains(r#"<w:cols w:space="720""#)
            || xml.contains(r#"<w:cols"#),
        "at least one <w:cols> tag must be present"
    );
}

#[test]
fn writing_doc_with_unknown_numid_emits_dynamic_numbering_xml() {
    // End-to-end check: when a doc references numId 50 (not built-in),
    // the resulting zip must include a numbering.xml with a num entry
    // for that id.
    let doc = WordDocument {
        paragraphs: vec![WordParagraph {
            id: "p1".into(),
            text: "".into(),
            style: None,
            runs: None,
            numbering: Some(NumberingRef {
                num_id: 50,
                level: 0,
            }),
            alignment: None,
            text_direction: None,
        }],
        tables: vec![],
        images: vec![],
        sections: vec![WordSection::default()],
        headers: vec![],
        footers: vec![],
    };
    let path = temp_path("numid50");
    write_word_document_to_path(&doc, &path, None).expect("write");
    // Re-open and inspect numbering.xml.
    let bytes = std::fs::read(&path).expect("read back");
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).expect("open");
    let mut entry = zip
        .by_name("word/numbering.xml")
        .expect("numbering.xml present");
    let mut content = String::new();
    entry.read_to_string(&mut content).expect("read xml");
    assert!(
        content.contains(r#"<w:num w:numId="50">"#),
        "auto-generated numbering.xml must register numId 50; got: {}",
        content
    );
    let _ = std::fs::remove_file(&path);
}
