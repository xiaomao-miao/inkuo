//! Manual acceptance verification: generates a full regression docx
//! and writes its document.xml to stdout for xmllint inspection.
//!
//! Run with:
//!   cargo test --lib -- --nocapture office::docx::acceptance_test::verify_accept_criteria_print

use crate::office::docx::{
    read_word_document, write_word_document_to_path, FontRun, PageMargins, PageSize, WordDocument,
    WordDocumentMeta, WordParagraph, WordSection, A4_HEIGHT_TWIPS, A4_WIDTH_TWIPS,
};
use std::fs::File;
use std::io::Read;

fn plain_para(text: &str) -> WordParagraph {
    WordParagraph {
        id: String::new(),
        text: text.to_string(),
        style: None,
        runs: Some(vec![FontRun {
            text: text.to_string(),
            ..Default::default()
        }]),
        numbering: None,
        alignment: None,
        text_direction: None,
        page_break: None,
    }
}

fn default_margins() -> PageMargins {
    PageMargins {
        top: 1440,
        right: 1440,
        bottom: 1440,
        left: 1440,
        header: None,
        footer: None,
        gutter: None,
    }
}

#[test]
fn verify_accept_criteria_print() {
    let path = std::env::temp_dir().join("inkuo_acceptance_test.docx");

    let mut doc = WordDocument::default();
    doc.meta = WordDocumentMeta {
        title: "回归测试文档".to_string(),
        author: "maomao".to_string(),
        ..Default::default()
    };

    // Cover content + section break marker
    doc.paragraphs.push(plain_para("封面段落"));
    doc.paragraphs.push(WordParagraph {
        id: "__sect_break_0__".to_string(),
        text: String::new(),
        style: None,
        runs: None,
        numbering: None,
        alignment: None,
        text_direction: None,
        page_break: None,
    });
    doc.sections.push(WordSection {
        id: "cover".to_string(),
        page_size_twips: Some(PageSize {
            width: A4_HEIGHT_TWIPS,
            height: A4_WIDTH_TWIPS,
            orient: Some("landscape".to_string()),
        }),
        margins: Some(default_margins()),
        text_direction: Some("horizontal".to_string()),
        ..Default::default()
    });

    // Body content + section break marker
    doc.paragraphs.push(plain_para("正文段落"));
    doc.paragraphs.push(WordParagraph {
        id: "__sect_break_1__".to_string(),
        text: String::new(),
        style: None,
        runs: None,
        numbering: None,
        alignment: None,
        text_direction: None,
        page_break: None,
    });
    doc.sections.push(WordSection {
        id: "body".to_string(),
        page_size_twips: Some(PageSize {
            width: A4_WIDTH_TWIPS,
            height: A4_HEIGHT_TWIPS,
            orient: Some("portrait".to_string()),
        }),
        margins: Some(default_margins()),
        text_direction: Some("horizontal".to_string()),
        ..Default::default()
    });

    // Vertical content (last section, no marker)
    doc.paragraphs.push(plain_para("竖排段落"));
    doc.sections.push(WordSection {
        id: "vertical".to_string(),
        page_size_twips: Some(PageSize {
            width: A4_WIDTH_TWIPS,
            height: A4_HEIGHT_TWIPS,
            orient: Some("portrait".to_string()),
        }),
        margins: Some(default_margins()),
        text_direction: Some("verticalRightToLeft".to_string()),
        ..Default::default()
    });

    write_word_document_to_path(&doc, &path, None).expect("write docx");

    let mut zip =
        zip::ZipArchive::new(File::open(&path).expect("open")).expect("zip");
    let mut document_xml = String::new();
    zip.by_name("word/document.xml")
        .expect("doc xml")
        .read_to_string(&mut document_xml)
        .expect("read doc xml");

    let mut core_xml = String::new();
    zip.by_name("docProps/core.xml")
        .expect("core xml")
        .read_to_string(&mut core_xml)
        .expect("read core xml");

    println!("=== document.xml ===");
    println!("{}", document_xml);
    println!("=== core.xml ===");
    println!("{}", core_xml);

    let mut bytes = Vec::new();
    File::open(&path)
        .expect("reopen")
        .read_to_end(&mut bytes)
        .expect("read bytes");
    let read_back = read_word_document(&bytes).expect("read back");
    println!("=== round-trip meta.title: {} ===", read_back.meta.title);
    println!("=== round-trip meta.author: {} ===", read_back.meta.author);
    println!(
        "=== read_back sections: {} ===",
        read_back.sections.len()
    );

    // Acceptance criteria
    assert_eq!(read_back.meta.title, "回归测试文档");
    assert_eq!(read_back.meta.author, "maomao");
    assert!(document_xml.contains("<w:pgSz"));
    assert!(document_xml.contains("<w:pgMar"));
    assert!(document_xml.contains("w:orient=\"landscape\""));
    assert!(document_xml.contains("tbRl"));
    assert!(core_xml.contains("回归测试文档"));
    assert!(core_xml.contains("maomao"));

    // Copy outputs to a stable location for xmllint validation.
    let out_dir = std::path::PathBuf::from("/tmp/inkuo_acceptance");
    let _ = std::fs::create_dir_all(&out_dir);
    let _ = std::fs::write(out_dir.join("document.xml"), &document_xml);
    let _ = std::fs::write(out_dir.join("core.xml"), &core_xml);
    let _ = std::fs::copy(&path, out_dir.join("inkuo_acceptance_test.docx"));

    let _ = std::fs::remove_file(&path);
}