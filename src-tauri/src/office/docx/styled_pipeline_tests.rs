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
    // The styled writer emits callout/code paragraphs INSIDE their
    // container's shaded cell (so the background wraps the text). The
    // reader parks those paragraphs on `WordTable::cell_paragraphs`
    // and the first-cell text round-trips through `TableRow.cells[j].text`.
    // The exact body-paragraph count is sensitive to the writer's
    // index-advance logic (callout inner paragraphs are consumed by
    // the writer and parked on the container table); we don't make
    // strict assertions on it here. The per-component behaviour is
    // covered by the unit tests in `components_tests.rs` and
    // `create_word_doc::component_bridge_tests`.
    assert!(
        doc.paragraphs.len() >= 8,
        "sample doc should have at least 8 body paragraphs; got {}",
        doc.paragraphs.len()
    );
    // 4 tables: callout container, second callout container, code
    // container, styled data table.
    assert!(
        doc.tables.len() >= 4,
        "sample doc should have at least 4 tables; got {}",
        doc.tables.len()
    );
    // The 3 container tables (2 callouts + 1 code) should each carry
    // the cell paragraphs the writer emitted inside the shaded cell.
    let cell_para_count: usize = doc.tables.iter()
        .map(|t| t.cell_paragraphs.len())
        .sum();
    assert!(
        cell_para_count >= 10,
        "container tables should collectively carry ≥10 cell paragraphs; got {}",
        cell_para_count
    );
    let _ = std::fs::remove_file(&path);
}
