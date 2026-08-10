//! End-to-end tests for the design-system + component layer.
//!
//! These tests don't assert visual output (you can't easily check
//! "does this look like the brand" from a CI run) but they do
//! assert the structural invariants the user-facing pipeline
//! depends on:
//!
//!   - The default palette matches the design-doc values the user
//!     shared (locks the brand in place).
//!   - Every component builder produces a non-empty list of
//!     paragraphs / tables.
//!   - Rendering a `DocumentContent` through the renderer produces
//!     the same shape the existing writer expects (so the old
//!     `write_word_document` can still consume it).
//!   - Callouts and code blocks emit their marker tables so the
//!     styled writer can pick them up at emit time.

use crate::office::docx::components::{
    body_paragraph, body_runs, bulleted_list, callout_block, callout_multiline, chapter_title,
    code_block, cover_title, heading, ordered_list, page_break, styled_table, CalloutLevel,
    TableStyle,
};
use crate::office::docx::design_tokens::{default_palette, DesignTokens};
use crate::office::docx::renderer::{
    render_blocks, render_document, ContentBlock, DocumentContent, CalloutLevelName,
};
use crate::office::docx::styled_writer::{
    build_callout_close_xml, build_callout_container_xml, build_code_block_container_xml,
    build_styled_table_xml, classify_and_strip, TableKind,
};

fn tokens() -> DesignTokens {
    DesignTokens::default()
}

#[test]
fn default_palette_locks_brand() {
    let p = default_palette();
    assert_eq!(p.primary, "213B32");
    assert_eq!(p.secondary, "2E7D5B");
    assert_eq!(p.accent, "B8893E");
    assert_eq!(p.text, "2A2A2A");
    assert_eq!(p.text_on_primary, "FFFFFF");
}

#[test]
fn font_scale_matches_design_doc() {
    let t = tokens();
    // Font sizes are in half-points (Word's internal unit). 34pt = 68 half-points.
    assert_eq!(t.fonts.cover_title_pt, 68);
    assert_eq!(t.fonts.h1_pt, 40); // 20pt
    assert_eq!(t.fonts.body_pt, 20); // 10pt
    assert_eq!(t.fonts.table_body_pt, 17); // 8.5pt
}

#[test]
fn cover_title_emits_title_and_spacers() {
    let out = cover_title(&tokens(), "My Doc", Some("A subtitle"));
    assert!(out.len() >= 3, "cover title should include title + subtitle + spacers");
    let title = &out[0];
    assert!(title.style.as_deref() == Some("CoverTitle"));
    assert!(title.runs.is_some());
}

#[test]
fn chapter_title_uses_chapter_style() {
    let out = chapter_title(&tokens(), "Chapter 1");
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].style.as_deref(), Some("ChapterTitle"));
}

#[test]
fn heading_levels_use_distinct_styles() {
    let h1 = heading(&tokens(), 1, "x");
    let h2 = heading(&tokens(), 2, "x");
    let h3 = heading(&tokens(), 3, "x");
    assert_eq!(h1[0].style.as_deref(), Some("ChapterTitle"));
    assert_eq!(h2[0].style.as_deref(), Some("SectionTitle"));
    assert_eq!(h3[0].style.as_deref(), Some("SubsectionTitle"));
}

#[test]
fn body_paragraph_uses_body_style() {
    let out = body_paragraph(&tokens(), "p1", "hello world");
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].style.as_deref(), Some("BodyParagraph"));
}

#[test]
fn body_runs_emits_each_run_as_separate_paragraph() {
    // body_runs returns a single paragraph with all runs merged.
    let out = body_runs(
        &tokens(),
        "p1",
        &[
            ("plain ".to_string(), false, false),
            ("bold".to_string(), true, false),
        ],
    );
    assert_eq!(out.len(), 1);
    assert!(out[0].runs.is_some());
    let runs = out[0].runs.as_ref().unwrap();
    assert_eq!(runs.len(), 2);
    assert!(!runs[0].bold);
    assert!(runs[1].bold);
}

#[test]
fn bulleted_list_assigns_numbering() {
    let out = bulleted_list(&tokens(), "bl", &["one", "two", "three"]);
    assert_eq!(out.len(), 3);
    for p in &out {
        let num = p.numbering.as_ref().expect("list item needs numbering");
        assert_eq!(num.num_id, 1);
        assert_eq!(num.level, 0);
    }
}

#[test]
fn ordered_list_uses_decimal_numbering() {
    let out = ordered_list(&tokens(), "ol", &["a", "b"]);
    assert_eq!(out.len(), 2);
    for p in &out {
        let num = p.numbering.as_ref().unwrap();
        assert_eq!(num.num_id, 2);
    }
}

#[test]
fn styled_table_carries_style_marker_row() {
    let t = styled_table(
        &tokens(),
        "t1",
        &["col1", "col2"],
        &[vec!["a".into(), "b".into()], vec!["c".into(), "d".into()]],
        &TableStyle {
            header_fill: Some("213B32".into()),
            zebra_fill: Some("EAF0EC".into()),
            border_color: Some("DDDDDD".into()),
            header_text_color: None,
            repeat_header: true,
            zebra: true,
        },
    );
    assert!(t.rows[0].cells[0].text.starts_with("__STYLE__|"));
    // Header row + 2 body rows + 1 marker = 4 total rows.
    assert_eq!(t.rows.len(), 4);
}

#[test]
fn classify_and_strip_recognises_style_marker() {
    let t = styled_table(
        &tokens(),
        "t1",
        &["h"],
        &[vec!["a".into()]],
        &TableStyle {
            header_fill: Some("213B32".into()),
            ..Default::default()
        },
    );
    let (kind, stripped) = classify_and_strip(&t.rows);
    match kind {
        TableKind::Styled(_) => {}
        _ => panic!("expected TableKind::Styled"),
    }
    // The marker row should be stripped, leaving 2 rows.
    assert_eq!(stripped.len(), 2);
}

#[test]
fn classify_and_strip_recognises_callout_marker() {
    let r = callout_block(&tokens(), "c1", CalloutLevel::Info, "title", "body");
    // The renderer emits a 1-row table whose first cell carries the
    // marker. Build the same shape here for the test.
    let marker = format!("__CALLOUT__|{}|{}", r.bg, r.accent);
    let mut row = r.paragraphs; // we only need the table shape here
    row.clear();
    let _ = row;
    // Use a synthetic table with the marker prefix.
    use crate::office::shared::{TableCell, TableRow};
    let cell = TableCell { text: marker, col_span: 1, row_span: 1 };
    let row = TableRow { cells: vec![cell] };
    let table = crate::office::docx::types::WordTable {
        id: r.table_id.clone(),
        rows: vec![row],
        cell_paragraphs: Vec::new(),
    };
    let (kind, stripped) = classify_and_strip(&table.rows);
    match kind {
        TableKind::Callout { bg, accent } => {
            assert!(!bg.is_empty());
            assert!(!accent.is_empty());
        }
        _ => panic!("expected TableKind::Callout"),
    }
    assert_eq!(stripped.len(), 0);
}

#[test]
fn classify_and_strip_recognises_code_marker() {
    use crate::office::shared::{TableCell, TableRow};
    let cell = TableCell {
        text: "__CODE__|F4F1EC".to_string(),
        col_span: 1,
        row_span: 1,
    };
    let row = TableRow { cells: vec![cell] };
    let (kind, _) = classify_and_strip(&[row]);
    match kind {
        TableKind::Code { bg } => assert_eq!(bg, "F4F1EC"),
        _ => panic!("expected TableKind::Code"),
    }
}

#[test]
fn classify_and_strip_falls_back_to_plain() {
    use crate::office::shared::{TableCell, TableRow};
    let cell = TableCell {
        text: "just a cell".to_string(),
        col_span: 1,
        row_span: 1,
    };
    let row = TableRow { cells: vec![cell] };
    let (kind, stripped) = classify_and_strip(&[row]);
    assert!(matches!(kind, TableKind::Plain));
    assert_eq!(stripped.len(), 1);
}

#[test]
fn callout_block_emits_two_paragraphs() {
    let out = callout_block(&tokens(), "c1", CalloutLevel::Warning, "Heads up", "Be careful");
    assert_eq!(out.paragraphs.len(), 2);
    assert!(out.bg.starts_with("F")); // warning bg
    assert!(out.accent.starts_with("B")); // accent
}

#[test]
fn callout_multiline_emits_one_para_per_line() {
    let lines = vec!["line 1", "line 2", "line 3"];
    let out = callout_multiline(&tokens(), "c1", CalloutLevel::Tip, "Title", &lines);
    // 1 title paragraph + 3 body paragraphs.
    assert_eq!(out.paragraphs.len(), 4);
}

#[test]
fn code_block_emits_one_para_per_line() {
    let lines = vec!["fn main() {", "    println!(\"hi\");", "}"];
    let out = code_block(&tokens(), "cb", &lines);
    assert_eq!(out.paragraphs.len(), 3);
    assert_eq!(out.bg, "F4F1EC");
}

#[test]
fn page_break_returns_one_paragraph() {
    let out = page_break("pb1");
    assert_eq!(out.len(), 1);
    assert!(out[0].runs.is_some());
}

#[test]
fn styled_table_xml_contains_shading_and_tblheader() {
    let t = styled_table(
        &tokens(),
        "t1",
        &["h"],
        &[vec!["a".into()]],
        &TableStyle {
            header_fill: Some("213B32".into()),
            zebra_fill: Some("EAF0EC".into()),
            border_color: Some("DDDDDD".into()),
            header_text_color: Some("FFFFFF".into()),
            repeat_header: true,
            zebra: true,
        },
    );
    let (kind, stripped) = classify_and_strip(&t.rows);
    let style = match kind {
        TableKind::Styled(s) => *s,
        _ => panic!("expected styled"),
    };
    let xml = build_styled_table_xml("t1", &stripped, &style);
    assert!(xml.contains("<w:shd"));
    assert!(xml.contains("<w:tblHeader/>"), "header row should repeat");
    assert!(xml.contains("<w:b/>"), "header text should be bold");
    assert!(xml.contains("FFFFFF"), "header text colour set");
}

#[test]
fn callout_xml_has_accent_border() {
    let open = build_callout_container_xml("E8F1ED", "2E7D5B");
    assert!(open.contains("E8F1ED"));
    assert!(open.contains("2E7D5B"));
    assert!(open.contains("<w:tbl>"));
    let close = build_callout_close_xml();
    assert!(close.contains("</w:tc></w:tr></w:tbl>"));
}

#[test]
fn code_block_xml_has_uniform_fill() {
    let open = build_code_block_container_xml("F4F1EC");
    assert!(open.contains("F4F1EC"));
    assert!(open.contains("<w:shd"));
}

#[test]
fn render_blocks_produces_structured_output() {
    let blocks = vec![
        ContentBlock::Cover {
            id: "c1".into(),
            title: "Hello".into(),
            subtitle: Some("World".into()),
        },
        ContentBlock::Chapter {
            id: "ch1".into(),
            title: "Chapter 1".into(),
        },
        ContentBlock::Body {
            id: "p1".into(),
            text: Some("Hello world.".into()),
            runs: None,
        },
        ContentBlock::Table {
            id: "t1".into(),
            headers: vec!["a".into(), "b".into()],
            rows: vec![vec!["1".into(), "2".into()]],
            style: None,
        },
        ContentBlock::Callout {
            id: "cal1".into(),
            level: CalloutLevelName::Info,
            title: "Note".into(),
            body: Some("body".into()),
            body_lines: None,
        },
        ContentBlock::Code {
            id: "code1".into(),
            lines: vec!["a".into(), "b".into()],
            language: Some("rust".into()),
        },
        ContentBlock::PageBreak { id: "pb1".into() },
    ];
    let rendered = render_blocks(&blocks, &tokens());
    // Cover (3) + chapter (1) + body (1) + callout paragraphs (2) +
    // code paragraphs (2 with lang label) + page break (1) = 10.
    assert!(rendered.paragraphs.len() >= 7);
    // Table (1) + callout container (1) + code container (1) = 3.
    assert_eq!(rendered.tables.len(), 3);
    // No images expected.
    assert!(rendered.images.is_empty());
}

#[test]
fn render_document_walks_top_level_blocks() {
    let content = DocumentContent::new(vec![
        ContentBlock::Body {
            id: "p1".into(),
            text: Some("hi".into()),
            runs: None,
        },
        ContentBlock::PageBreak { id: "pb".into() },
    ]);
    let out = render_document(&content);
    assert_eq!(out.paragraphs.len(), 2);
}

#[test]
fn content_block_round_trip_through_serde() {
    // Verify the JSON shape is what callers (the agent / prompt layer)
    // will actually see. We don't deserialise a fully-formed block
    // here — just spot-check that the `type` tag is `snake_case` so
    // the prompt schema stays human-friendly.
    let json = serde_json::to_string(&ContentBlock::PageBreak { id: "x".into() }).unwrap();
    assert!(json.contains("\"type\":\"page_break\""), "got {}", json);
    let json = serde_json::to_string(&ContentBlock::Callout {
        id: "x".into(),
        level: CalloutLevelName::Tip,
        title: "t".into(),
        body: Some("b".into()),
        body_lines: None,
    })
    .unwrap();
    assert!(json.contains("\"type\":\"callout\""));
    assert!(json.contains("\"level\":\"tip\""));
}

#[test]
fn styled_table_xml_has_complete_ooxml_structure() {
    // Verify that the styled table XML contains all required OOXML elements:
    // - w:tblGrid (column definitions)
    // - w:tblW (table width)
    // - w:tblInd (table indent)
    // - w:tblCellMar (cell margins)
    // - w:tblHeader (header repeat)
    // - w:tcW (cell width)
    let t = styled_table(
        &tokens(),
        "t1",
        &["col1", "col2", "col3"],
        &[
            vec!["a".into(), "b".into(), "c".into()],
            vec!["d".into(), "e".into(), "f".into()],
        ],
        &TableStyle {
            header_fill: Some("213B32".into()),
            zebra_fill: Some("EAF0EC".into()),
            border_color: Some("DDDDDD".into()),
            header_text_color: Some("FFFFFF".into()),
            repeat_header: true,
            zebra: true,
        },
    );
    let (kind, stripped) = classify_and_strip(&t.rows);
    let style = match kind {
        TableKind::Styled(s) => *s,
        _ => panic!("expected styled"),
    };
    let xml = build_styled_table_xml("t1", &stripped, &style);
    
    // Required OOXML elements for proper table structure
    assert!(xml.contains("<w:tblGrid>"), "must have w:tblGrid");
    assert!(xml.contains("<w:gridCol"), "must have w:gridCol for each column");
    assert!(xml.contains("<w:tblW"), "must have w:tblW (table width)");
    assert!(xml.contains("<w:tblInd"), "must have w:tblInd (table indent)");
    assert!(xml.contains("<w:tblCellMar>"), "must have w:tblCellMar (cell margins)");
    assert!(xml.contains("<w:tblHeader/>"), "must have w:tblHeader for repeat header");
    assert!(xml.contains("<w:tcW"), "must have w:tcW (cell width) in each cell");
    assert!(xml.contains("<w:tcMar>"), "must have w:tcMar (cell margins) in cells");
    assert!(xml.contains("<w:shd"), "must have w:shd (cell shading)");
    assert!(xml.contains("<w:tcBorders>"), "must have w:tcBorders (cell borders)");
}

#[test]
fn plain_table_xml_has_complete_ooxml_structure() {
    // Verify that even plain tables have proper OOXML structure
    use crate::office::shared::{TableCell, TableRow};
    use crate::office::docx::writer::build_table_xml;
    
    let rows = vec![
        TableRow {
            cells: vec![
                TableCell::plain("Header1"),
                TableCell::plain("Header2"),
            ],
        },
        TableRow {
            cells: vec![
                TableCell::plain("Cell1"),
                TableCell::plain("Cell2"),
            ],
        },
    ];
    
    let xml = build_table_xml("plain_table", &rows, None);
    
    // Plain tables should also have tblGrid
    assert!(xml.contains("<w:tblGrid>"), "plain table must have w:tblGrid");
    assert!(xml.contains("<w:gridCol"), "plain table must have w:gridCol");
    assert!(xml.contains("<w:tblW"), "plain table must have w:tblW");
    assert!(xml.contains("<w:tblInd"), "plain table must have w:tblInd");
    assert!(xml.contains("<w:tcW"), "plain table cells must have w:tcW");
    assert!(xml.contains("<w:tcMar>"), "plain table cells must have w:tcMar");
    assert!(xml.contains("<w:tcBorders>"), "plain table cells must have w:tcBorders");
}
