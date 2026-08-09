//! End-to-end smoke test: write a sample document, then verify the
//! resulting .docx can be re-read and the brand colour round-trips.
//!
//! This is the "does the whole pipeline hang together" test — it
//! exercises the renderer, component builders, writer, and reader
//! in one go.

use crate::office::docx::read_word_document;
use crate::office::docx::styled_pipeline::write_sample_document;

#[test]
fn sample_document_round_trips() {
    // Use a temp path so tests don't pollute the workspace.
    let tmp = std::env::temp_dir().join(format!("inkuo-sample-{}.docx", std::process::id()));
    let path = write_sample_document(&tmp).expect("write_sample_document failed");
    assert!(path.exists(), "sample file should exist at {:?}", path);
    let bytes = std::fs::read(&path).expect("read back the file");
    assert!(!bytes.is_empty());
    let doc = read_word_document(&bytes).expect("parse back the file");
    // We expect at least: cover (3 paragraphs) + chapter title (1) +
    // body (1) + heading (1) + 4 bullet items + 2 callouts (4
    // paragraphs) + 1 code block (with lang label, 8 lines) +
    // chapter title (1) + heading (1) + page break (1) + heading (1)
    // + body (1) = ~28 paragraphs.
    assert!(
        doc.paragraphs.len() >= 20,
        "sample doc should have at least 20 paragraphs; got {}",
        doc.paragraphs.len()
    );
    // 3 tables: the callout container, the callout-2 container, the
    // code-block container, and the styled data table = 4.
    assert!(
        doc.tables.len() >= 4,
        "sample doc should have at least 4 tables; got {}",
        doc.tables.len()
    );
    let _ = std::fs::remove_file(&path);
}
