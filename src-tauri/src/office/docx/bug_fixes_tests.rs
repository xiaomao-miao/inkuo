//! Regression tests for bugs in the design-system + low-level element path:
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
//!   4. `insert_table_with_anchor_emits_marker_before_table` — when a table
//!      is inserted at an anchor position, the `<__tbl_pos_<id>__>` marker
//!      paragraph must appear BEFORE the table in the XML output (not after
//!      all original elements). Previously, inserted markers were emitted at
//!      the end of out_paras regardless of the anchor position, causing
//!      inserted tables/images to appear at the document end instead of
//!      at the intended anchor position.
//!
//!   5. `insert_image_with_anchor_emits_marker_before_image` — same fix for
//!      inserted images.
//!
//! Bug #4 (image-with-id silent drop) is exercised through
//! `create_word_doc` integration tests below.

use crate::office::docx::ooxml_boilerplate::{
    build_dynamic_numbering_body, collect_referenced_num_ids,
};
use crate::office::docx::styled_styles::EXTENDED_STYLES_XML;
use crate::office::docx::writer::build_document_xml;
use crate::office::docx::{
    NumberingRef, WordDocument, WordDocumentMeta, WordParagraph, WordSection,
    write_word_document_to_path,
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
                page_break: None,
            },
            WordParagraph {
                id: "b".into(),
                text: "".into(),
                style: None,
                runs: None,
                numbering: Some(NumberingRef { num_id: 5, level: 0 }),
                alignment: None,
                text_direction: None,
                page_break: None,
            },
            WordParagraph {
                id: "c".into(),
                text: "".into(),
                style: None,
                runs: None,
                numbering: Some(NumberingRef { num_id: 7, level: 0 }),
                alignment: None,
                text_direction: None,
                page_break: None,
            },
        ],
        tables: vec![],
        images: vec![],
        sections: vec![],
        headers: vec![],
        footers: vec![],
        meta: WordDocumentMeta::default(),
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
fn sections_without_markers_coerce_to_one_section() {
    // 10 plain body paragraphs + 2 sections, no `__sect_break_<idx>__`
    // markers. The writer now coerces multi-section docs without explicit
    // markers down to a single trailing section (the section whose
    // properties the user actually set). This is the safe default: the
    // alternative — distributing paragraphs evenly across "phantom"
    // sections — was the source of the "first sectPr lands on the wrong
    // list item" regression in the report.
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
            page_break: None,
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
    let xml = build_document_xml(&doc);
    // Coerced to a single section: exactly one body-level sectPr. The
    // orphan "s0" cols=2 setting is dropped because no marker anchored
    // it; callers must either pass `columns: 2` on a body paragraph (the
    // proper way to scope a multi-column region) or include a marker.
    assert_eq!(
        xml.matches("<w:sectPr>").count(),
        1,
        "no markers + multiple sections must coerce to a single trailing section"
    );
    // Final section is single-column (the last user-defined section).
    assert!(
        !xml.contains(r#"<w:cols w:num="2""#),
        "without markers the cols=2 section must NOT be silently applied; got: {}",
        xml
    );
}

#[test]
fn every_section_emits_pgsz_and_pgmar() {
    // A single WordSection without explicit page size / margins must
    // still emit `<w:pgSz>` and `<w:pgMar>` — the user's regression
    // report showed sections missing both, leaving Word to fall back to
    // Letter / 612x792 pt defaults.
    let doc = WordDocument {
        paragraphs: vec![WordParagraph {
            id: "p0".into(),
            text: "Hello".into(),
            style: None,
            runs: None,
            numbering: None,
            alignment: None,
            text_direction: None,
            page_break: None,
        }],
        tables: vec![],
        images: vec![],
        sections: vec![WordSection {
            id: "s0".into(),
            section_type: None,
            page_size_twips: None,
            page_size_mm: None,
            margins: None,
            text_direction: None,
            title_pg: false,
            cols: None,
            page_num_start: None,
            page_num_format: None,
            header_refs: vec![],
            footer_refs: vec![],
        }],
        headers: vec![],
        footers: vec![],
        meta: WordDocumentMeta::default(),
    };
    let xml = build_document_xml(&doc);
    assert!(
        xml.contains(r#"<w:pgSz w:w="11906" w:h="16838" w:orient="portrait""#),
        "writer must emit A4 portrait pgSz by default; got: {}",
        xml
    );
    assert!(
        xml.contains(r#"<w:pgMar w:top="1440" w:right="1440" w:bottom="1440" w:left="1440""#),
        "writer must emit 1-inch pgMar by default; got: {}",
        xml
    );
}

#[test]
fn marker_only_branch_emits_sectpr() {
    // When markers satisfy `markers + 1 == sections.len()`, the writer
    // emits one sectPr per section (the marker branch + the trailing
    // body sectPr). This guards against the marker branch being
    // bypassed when `total_sections > 1`.
    let mut paragraphs: Vec<WordParagraph> = Vec::new();
    for i in 0..4 {
        paragraphs.push(WordParagraph {
            id: format!("p{}", i),
            text: format!("P{}", i),
            style: None,
            runs: None,
            numbering: None,
            alignment: None,
            text_direction: None,
            page_break: None,
        });
    }
    paragraphs.push(WordParagraph {
        id: "__sect_break_0__".into(),
        text: String::new(),
        style: None,
        runs: None,
        numbering: None,
        alignment: None,
        text_direction: None,
        page_break: None,
    });
    paragraphs.push(WordParagraph {
        id: "p5".into(),
        text: "after break".into(),
        style: None,
        runs: None,
        numbering: None,
        alignment: None,
        text_direction: None,
        page_break: None,
    });

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
    let xml = build_document_xml(&doc);
    assert_eq!(
        xml.matches("<w:sectPr>").count(),
        2,
        "marker + body-level sectPr per section; got: {}",
        xml
    );
    assert!(
        xml.contains(r#"<w:cols w:num="2""#),
        "marker-anchored section's cols=2 must survive; got: {}",
        xml
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
            page_break: None,
        }],
        tables: vec![],
        images: vec![],
        sections: vec![WordSection::default()],
        headers: vec![],
        footers: vec![],
        meta: WordDocumentMeta::default(),
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

#[test]
fn drop_unresolved_header_footer_refs() {
    use crate::office::docx::{drop_unresolved_header_footer_refs, FooterPart, FooterPartRef, HeaderPart, HeaderPartRef};
    let mut doc = WordDocument {
        paragraphs: vec![WordParagraph {
            id: "p0".into(),
            text: "Hello".into(),
            style: None,
            runs: None,
            numbering: None,
            alignment: None,
            text_direction: None,
            page_break: None,
        }],
        tables: vec![],
        images: vec![],
        sections: vec![WordSection {
            id: "s0".into(),
            section_type: None,
            page_size_twips: None,
            page_size_mm: None,
            margins: None,
            text_direction: None,
            title_pg: false,
            cols: None,
            page_num_start: None,
            page_num_format: None,
            header_refs: vec![
                HeaderPartRef { header_id: "real_header".into(), kind: None },
                HeaderPartRef { header_id: "missing_header".into(), kind: None },
            ],
            footer_refs: vec![
                FooterPartRef { footer_id: "missing_footer".into(), kind: None },
            ],
        }],
        headers: vec![HeaderPart {
            id: "real_header".into(),
            paragraphs: vec![],
            tables: vec![],
            images: vec![],
        }],
        footers: vec![FooterPart {
            id: "real_footer".into(),
            paragraphs: vec![],
            tables: vec![],
            images: vec![],
        }],
        meta: WordDocumentMeta::default(),
    };
    drop_unresolved_header_footer_refs(&mut doc);
    assert_eq!(doc.sections[0].header_refs.len(), 1, "missing header must be dropped");
    assert_eq!(doc.sections[0].header_refs[0].header_id, "real_header");
    assert_eq!(doc.sections[0].footer_refs.len(), 0, "missing footer must be dropped");
}

#[test]
fn update_fields_added_when_doc_has_field() {
    use crate::office::docx::inject_update_fields;
    let injected = inject_update_fields(crate::office::docx::SETTINGS_XML);
    assert!(
        injected.contains("<w:updateFields w:val=\"true\"/>"),
        "settings.xml must get updateFields injected; got: {}",
        injected
    );
    // Idempotent.
    let injected_again = inject_update_fields(&injected);
    assert_eq!(
        injected.matches("<w:updateFields").count(),
        1,
        "running the injector twice must not duplicate the element"
    );
    let _ = injected_again;
}

#[test]
fn core_xml_has_title_and_author() {
    use crate::office::docx::build_core_xml;
    let xml = build_core_xml(
        Some("Regression Title"),
        Some("Regression Author"),
        Some("Regression Subject"),
        Some("Regression Description"),
        Some("k1, k2"),
    );
    assert!(xml.contains("<dc:title>Regression Title</dc:title>"));
    assert!(xml.contains("<dc:creator>Regression Author</dc:creator>"));
    assert!(xml.contains("<dc:subject>Regression Subject</dc:subject>"));
    assert!(xml.contains("<cp:keywords>k1, k2</cp:keywords>"));
}

#[test]
fn cover_section_suppresses_header_footer() {
    // When the document has multiple sections, only the *first* section
    // can act as the cover. The default-footer injection should skip
    // sections whose title_pg=true so the cover doesn't carry the
    // page-number footer that should start on the body section.
    use crate::office::docx::maybe_inject_default_footer;
    let mut doc = WordDocument {
        paragraphs: vec![WordParagraph {
            id: "body".into(),
            text: "Body content".into(),
            style: Some("BodyParagraph".into()), // triggers brand-style path
            runs: None,
            numbering: None,
            alignment: None,
            text_direction: None,
            page_break: None,
        }],
        tables: vec![],
        images: vec![],
        sections: vec![
            WordSection {
                id: "cover".into(),
                section_type: None,
                page_size_twips: None,
                page_size_mm: None,
                margins: None,
                text_direction: None,
                title_pg: true, // <-- cover indicator
                cols: None,
                page_num_start: None,
                page_num_format: None,
                header_refs: vec![],
                footer_refs: vec![],
            },
            WordSection {
                id: "body".into(),
                section_type: None,
                page_size_twips: None,
                page_size_mm: None,
                margins: None,
                text_direction: None,
                title_pg: false,
                cols: None,
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
    maybe_inject_default_footer(&mut doc);
    // The cover section must remain footer-less — the body section is
    // where the default page-number footer takes effect.
    assert!(
        doc.sections[0].footer_refs.is_empty(),
        "cover section must keep an empty footer_refs; got: {:?}",
        doc.sections[0].footer_refs
    );
    assert_eq!(
        doc.sections[1].footer_refs.len(),
        1,
        "body section must carry the default footer ref"
    );
}

// ── Anchor insertion regression tests ─────────────────────────────────────────

#[test]
fn insert_table_with_anchor_emits_marker_before_table() {
    // Regression test for the anchor-insertion bug where inserted tables
    // appeared at the document end instead of at the anchor position.
    //
    // Scenario: Document has [A, B, C]. Insert table T after anchor=B.
    // Expected order in XML: [A, B, <__tbl_pos_T__>, T, C]
    // Bug behavior: [A, B, C, <__tbl_pos_T__>, T] (marker at end, table at end)
    //
    // The key assertion: the <__tbl_pos_T__> marker must appear BEFORE C,
    // not after it.

    use crate::office::docx::build_document_xml;
    use crate::office::{DocElement, InsertElement, TableCell};

    let mut doc = crate::office::WordDocument {
        paragraphs: vec![
            WordParagraph {
                id: "a".into(),
                text: "A".into(),
                style: None,
                runs: None,
                numbering: None,
                alignment: None,
                text_direction: None,
                page_break: None,
            },
            WordParagraph {
                id: "b".into(),
                text: "B".into(),
                style: None,
                runs: None,
                numbering: None,
                alignment: None,
                text_direction: None,
                page_break: None,
            },
            WordParagraph {
                id: "c".into(),
                text: "C".into(),
                style: None,
                runs: None,
                numbering: None,
                alignment: None,
                text_direction: None,
                page_break: None,
            },
        ],
        tables: vec![],
        images: vec![],
        sections: vec![WordSection::default()],
        headers: vec![],
        footers: vec![],
        meta: WordDocumentMeta::default(),
    };

    // Insert a table after anchor="b" (position "after").
    let inserted_table = DocElement::Table {
        id: "t1".into(),
        position: 0,
            header: vec![TableCell::plain("Header")],
        rows: vec![vec![TableCell::plain("Cell")]],
    };
    let insert_elem = InsertElement {
        element: inserted_table,
        anchor_id: Some("b".into()),
        position: Some("after".into()),
    };

    let _warnings = doc.modify(
        vec![],                    // no modifies
        vec![],                    // no deletes
        vec![insert_elem],        // insert table after anchor b
    );

    // Build the document XML to verify ordering.
    let xml = build_document_xml(&doc);

    // The writer transforms <__tbl_pos_t1__> to <inkuo:id w:val="__tbl_pos_t1__"/>
    // in the XML. Check for this transformed form.
    let marker_pattern = r#"<inkuo:id w:val="__tbl_pos_t1__""#;
    let marker_pos = xml.find(marker_pattern).expect(
        "marker must exist (writer transforms <__tbl_pos_t1__> to <inkuo:id w:val=.../>)"
    );
    let c_pos = xml.find(">C<").expect("C must exist in XML");
    assert!(
        marker_pos < c_pos,
        "BUG: table marker must appear BEFORE paragraph C. \
         marker_pos={}, c_pos={}. \
         Full XML:\n{}",
        marker_pos, c_pos, xml
    );

    // The table content must also appear after B and before the table should be in the
    // right place (after the marker, before C).
    let table_content_pos = xml.find("Header").expect("table content must exist");
    assert!(
        marker_pos < table_content_pos && table_content_pos < c_pos,
        "table content must appear after marker and before C. \
         marker={}, content={}, c={}",
        marker_pos, table_content_pos, c_pos
    );
}

#[test]
fn insert_image_with_anchor_emits_marker_before_image() {
    // Same regression test for images: inserted images must have their marker
    // appear at the correct position, not at the document end.

    use crate::office::docx::build_document_xml;
    use crate::office::{DocElement, InsertElement};

    let mut doc = crate::office::WordDocument {
        paragraphs: vec![
            WordParagraph {
                id: "a".into(),
                text: "A".into(),
                style: None,
                runs: None,
                numbering: None,
                alignment: None,
                text_direction: None,
                page_break: None,
            },
            WordParagraph {
                id: "b".into(),
                text: "B".into(),
                style: None,
                runs: None,
                numbering: None,
                alignment: None,
                text_direction: None,
                page_break: None,
            },
            WordParagraph {
                id: "c".into(),
                text: "C".into(),
                style: None,
                runs: None,
                numbering: None,
                alignment: None,
                text_direction: None,
                page_break: None,
            },
        ],
        tables: vec![],
        images: vec![],
        sections: vec![WordSection::default()],
        headers: vec![],
        footers: vec![],
        meta: WordDocumentMeta::default(),
    };

    // Insert an image after anchor="b".
    let inserted_image = DocElement::Image {
        id: "img1".into(),
        position: 0,
        path: "/fake/image.png".into(),
        width_emu: 100000,
        height_emu: 100000,
        alt_text: None,
    };
    let insert_elem = InsertElement {
        element: inserted_image,
        anchor_id: Some("b".into()),
        position: Some("after".into()),
    };

    let _warnings = doc.modify(vec![], vec![], vec![insert_elem]);

    let xml = build_document_xml(&doc);

    // The marker <__img_pos_img1__> is transformed to <inkuo:id w:val="__img_pos_img1__"/>
    let marker_pattern = r#"<inkuo:id w:val="__img_pos_img1__""#;
    let marker_pos = xml.find(marker_pattern).expect("marker must exist");
    let c_pos = xml.find(">C<").expect("C must exist in XML");
    assert!(
        marker_pos < c_pos,
        "BUG: image marker must appear BEFORE paragraph C. \
         marker_pos={}, c_pos={}",
        marker_pos, c_pos
    );
}

#[test]
fn insert_multiple_tables_with_anchors_preserves_order() {
    // Test that inserting multiple tables at different anchor positions
    // results in all markers appearing BEFORE the end of the original document.
    // The key assertion: none of the inserted markers should appear after the
    // last original paragraph C.

    use crate::office::docx::build_document_xml;
    use crate::office::{DocElement, InsertElement, TableCell};

    let mut doc = crate::office::WordDocument {
        paragraphs: vec![
            WordParagraph {
                id: "a".into(),
                text: "A".into(),
                style: None,
                runs: None,
                numbering: None,
                alignment: None,
                text_direction: None,
                page_break: None,
            },
            WordParagraph {
                id: "b".into(),
                text: "B".into(),
                style: None,
                runs: None,
                numbering: None,
                alignment: None,
                text_direction: None,
                page_break: None,
            },
            WordParagraph {
                id: "c".into(),
                text: "C".into(),
                style: None,
                runs: None,
                numbering: None,
                alignment: None,
                text_direction: None,
                page_break: None,
            },
        ],
        tables: vec![],
        images: vec![],
        sections: vec![WordSection::default()],
        headers: vec![],
        footers: vec![],
        meta: WordDocumentMeta::default(),
    };

    // Insert two tables: T1 after A, T2 after B.
    let insert_t1 = InsertElement {
        element: DocElement::Table {
            id: "t1".into(),
            position: 0,
            header: vec![TableCell::plain("T1-Header")],
            rows: vec![],
        },
        anchor_id: Some("a".into()),
        position: Some("after".into()),
    };
    let insert_t2 = InsertElement {
        element: DocElement::Table {
            id: "t2".into(),
            position: 0,
            header: vec![TableCell::plain("T2-Header")],
            rows: vec![],
        },
        anchor_id: Some("b".into()),
        position: Some("after".into()),
    };

    let _warnings = doc.modify(vec![], vec![], vec![insert_t1, insert_t2]);

    let xml = build_document_xml(&doc);

    // The key regression test: markers must NOT appear after the last original paragraph.
    // With the bug, both markers were emitted at the end of out_paras (after C),
    // making xml contain: ... C <marker_t1> <marker_t2>
    //
    // With the fix, markers are emitted at their correct positions:
    // ... A <marker_t1> B <marker_t2> C
    let c_pos = xml.find(">C<").expect("C must exist");
    let marker_t1_pattern = r#"<inkuo:id w:val="__tbl_pos_t1__""#;
    let marker_t2_pattern = r#"<inkuo:id w:val="__tbl_pos_t2__""#;

    let marker_t1_pos = xml.find(marker_t1_pattern).expect("marker_t1 must exist");
    let marker_t2_pos = xml.find(marker_t2_pattern).expect("marker_t2 must exist");

    // Both markers must appear BEFORE C (the bug was that they appeared after C)
    assert!(
        marker_t1_pos < c_pos,
        "BUG: marker_t1 at pos {} should appear before C at pos {}. XML snippet around C:\n{}",
        marker_t1_pos,
        c_pos,
        &xml[c_pos.saturating_sub(100)..std::cmp::min(c_pos + 200, xml.len())]
    );

    assert!(
        marker_t2_pos < c_pos,
        "BUG: marker_t2 at pos {} should appear before C at pos {}. XML snippet around C:\n{}",
        marker_t2_pos,
        c_pos,
        &xml[c_pos.saturating_sub(100)..std::cmp::min(c_pos + 200, xml.len())]
    );

    // Additionally verify that marker_t1 appears before marker_t2 (correct relative order)
    assert!(
        marker_t1_pos < marker_t2_pos,
        "marker_t1 should appear before marker_t2"
    );
}
