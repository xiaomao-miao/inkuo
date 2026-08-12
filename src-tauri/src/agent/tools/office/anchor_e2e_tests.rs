//! End-to-end regression tests for the table/image anchor insertion bug.
//!
//! These tests simulate the real user workflow:
//! 1. Create a base doc with several paragraphs (including a table-title paragraph).
//! 2. Read the doc back to confirm the paragraph IDs.
//! 3. Call create_word_doc with a new table that has `anchor_id` + `position: "after"`.
//! 4. Read the doc back and verify the table landed at the expected position
//!    (after the table-title paragraph, BEFORE the "after_para" paragraph).
//!
//! Before the fix, the table was always appended to the end of the document,
//! regardless of the anchor_id. After the fix, the table lands at the anchor.
//!
//! For invalid anchor_ids, the table falls back to end-of-doc but the tool's
//! success message now includes a visible warning so callers can detect the
//! miss instead of silently losing positional intent.

use crate::agent::tools::office::CreateWordDocTool;
use crate::office::{read_word_document, DocElement, ElementId};

fn temp_path(suffix: &str) -> std::path::PathBuf {
    let mut p = std::env::temp_dir();
    let id: u64 = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos() as u64;
    p.push(format!("inkuo_anchor_e2e_{}_{}.docx", id, suffix));
    p
}

async fn run_anchor_repro(scenario: &str, table_json: serde_json::Value) {
    let path = temp_path(scenario);
    let tool = CreateWordDocTool::new();

    // Step 1: Create base doc with paragraphs including a table-title.
    tool.execute(
        serde_json::json!({
            "path": path.to_string_lossy(),
            "elements": [
                {"type": "paragraph", "id": "title", "text": "Title"},
                {"type": "paragraph", "id": "h1", "text": "Chapter 1"},
                {"type": "paragraph", "id": "body1", "text": "Body 1"},
                {"type": "paragraph", "id": "h2", "text": "Chapter 2"},
                {"type": "paragraph", "id": "body2", "text": "Body 2"},
                {"type": "paragraph", "id": "tbl_title", "text": "Table 3-1: Caption"},
                {"type": "paragraph", "id": "after_para", "text": "After paragraph."},
            ]
        }),
        None,
    )
    .await
    .expect("create base doc");

    // Step 2: Insert the table with anchor_id + position.
    let mut payload = serde_json::json!({
        "path": path.to_string_lossy(),
        "elements": [table_json]
    });
    payload["elements"][0]["anchor_id"] = serde_json::json!("tbl_title");
    payload["elements"][0]["position"] = serde_json::json!("after");

    tool.execute(payload, None)
        .await
        .expect("insert with anchor");

    // Step 3: Read back.
    let bytes = std::fs::read(&path).expect("read doc");
    let doc = read_word_document(&bytes).expect("parse doc");
    let elements = doc.to_elements();

    let tbl_title_idx = elements.iter().position(|e| e.id() == "tbl_title");
    let after_para_idx = elements.iter().position(|e| e.id() == "after_para");
    // Find the inserted table by excluding pre-existing tables (the base doc has none).
    let table_indices: Vec<usize> = elements
        .iter()
        .enumerate()
        .filter_map(|(i, e)| match e {
            DocElement::Table { .. } => Some(i),
            _ => None,
        })
        .collect();

    assert_eq!(
        table_indices.len(),
        1,
        "[{}] expected exactly one table in result",
        scenario
    );
    let table_idx = table_indices[0];

    assert!(
        tbl_title_idx.is_some(),
        "[{}] tbl_title paragraph must exist",
        scenario
    );
    assert!(
        after_para_idx.is_some(),
        "[{}] after_para paragraph must exist",
        scenario
    );

    // The table must appear AFTER tbl_title and BEFORE after_para.
    assert!(
        table_idx > tbl_title_idx.unwrap(),
        "[{}] BUG: table at #{} should appear AFTER tbl_title at #{:?}",
        scenario,
        table_idx,
        tbl_title_idx
    );
    assert!(
        table_idx < after_para_idx.unwrap(),
        "[{}] BUG: table at #{} should appear BEFORE after_para at #{:?}",
        scenario,
        table_idx,
        after_para_idx
    );

    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn e2e_lowlevel_table_with_anchor() {
    run_anchor_repro(
        "lowlevel",
        serde_json::json!({
            "type": "table",
            "header": ["A", "B", "C"],
            "rows": [["1", "2", "3"], ["4", "5", "6"]]
        }),
    )
    .await;
}

#[tokio::test]
async fn e2e_component_styled_table_with_anchor() {
    run_anchor_repro(
        "styled",
        serde_json::json!({
            "type": "styled_table",
            "id": "mystyled",
            "headers": ["H1", "H2"],
            "rows": [["x", "y"]]
        }),
    )
    .await;
}

#[tokio::test]
async fn e2e_image_with_anchor() {
    // For an image we need an actual PNG on disk. We use the runner's
    // own binary as a fake image — it doesn't matter what's in it as
    // long as the extension is correct.
    let mut fake_img = std::env::temp_dir();
    fake_img.push(format!("fake_{}.png", std::process::id()));
    std::fs::write(&fake_img, b"\x89PNG\r\n\x1a\n").expect("write fake png");

    let path = temp_path("image");
    let tool = CreateWordDocTool::new();

    tool.execute(
        serde_json::json!({
            "path": path.to_string_lossy(),
            "elements": [
                {"type": "paragraph", "id": "tbl_title", "text": "Image caption"},
                {"type": "paragraph", "id": "after_para", "text": "After."},
            ]
        }),
        None,
    )
    .await
    .expect("create base doc");

    tool.execute(
        serde_json::json!({
            "path": path.to_string_lossy(),
            "elements": [{
                "type": "image",
                "path": fake_img.to_string_lossy(),
                "width_emu": 914400,
                "height_emu": 914400,
                "anchor_id": "tbl_title",
                "position": "after"
            }]
        }),
        None,
    )
    .await
    .expect("insert image with anchor");

    let bytes = std::fs::read(&path).expect("read doc");
    let doc = read_word_document(&bytes).expect("parse doc");
    let elements = doc.to_elements();

    let after_para_idx = elements.iter().position(|e| e.id() == "after_para");
    let image_indices: Vec<usize> = elements
        .iter()
        .enumerate()
        .filter_map(|(i, e)| match e {
            DocElement::Image { .. } => Some(i),
            _ => None,
        })
        .collect();
    assert_eq!(image_indices.len(), 1);
    let image_idx = image_indices[0];
    assert!(
        image_idx < after_para_idx.unwrap(),
        "BUG: image at #{} should appear BEFORE after_para at {:?}",
        image_idx,
        after_para_idx
    );

    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(&fake_img);
}

/// Control case: no anchor_id. The table must still be inserted (just at
/// the end of the document, matching the existing pre-fix append behavior).
#[tokio::test]
async fn e2e_control_no_anchor_table_appends_to_end() {
    let path = temp_path("control");
    let tool = CreateWordDocTool::new();

    tool.execute(
        serde_json::json!({
            "path": path.to_string_lossy(),
            "elements": [
                {"type": "paragraph", "id": "title", "text": "Title"},
                {"type": "paragraph", "id": "after_para", "text": "After."},
            ]
        }),
        None,
    )
    .await
    .expect("create base doc");

    tool.execute(
        serde_json::json!({
            "path": path.to_string_lossy(),
            "elements": [{
                "type": "table",
                "header": ["A", "B"],
                "rows": [["1", "2"]]
            }]
        }),
        None,
    )
    .await
    .expect("insert without anchor");

    let bytes = std::fs::read(&path).expect("read doc");
    let doc = read_word_document(&bytes).expect("parse doc");
    let elements = doc.to_elements();

    let after_para_idx = elements.iter().position(|e| e.id() == "after_para");
    let table_indices: Vec<usize> = elements
        .iter()
        .enumerate()
        .filter_map(|(i, e)| match e {
            DocElement::Table { .. } => Some(i),
            _ => None,
        })
        .collect();
    assert_eq!(table_indices.len(), 1);
    let table_idx = table_indices[0];
    // No anchor: table appears AFTER after_para (appended at end).
    assert!(
        table_idx > after_para_idx.unwrap(),
        "control: table at #{} should appear AFTER after_para at {:?}",
        table_idx,
        after_para_idx
    );

    let _ = std::fs::remove_file(&path);
}

/// Full end-to-end reproduction of the user's bug report.
///
/// The user reported that even after fix-check tests, anchor-based
/// insertion of tables/images was being silently ignored. This test
/// exercises the exact user workflow:
///   1. Create a base doc with the table-title anchor paragraph.
///   2. Insert a NEW table with anchor_id + position: "after".
///   3. Read back and verify the table landed at the anchor (NOT at
///      the end of the document).
///
/// Before the fix, the table was always appended to the end and
/// `position` was 7 (== total element count). After the fix, the
/// table is positioned at the anchor (between tbl_title and after_para).
#[tokio::test]
async fn e2e_full_repro_lowlevel_table() {
    let path = temp_path("repro_lowlevel");
    let tool = CreateWordDocTool::new();

    // Step 1: Create base doc with table-title paragraph.
    tool.execute(
        serde_json::json!({
            "path": path.to_string_lossy(),
            "elements": [
                {"type": "paragraph", "id": "title", "text": "Title"},
                {"type": "paragraph", "id": "h1", "text": "Chapter 1"},
                {"type": "paragraph", "id": "body1", "text": "Body 1"},
                {"type": "paragraph", "id": "h2", "text": "Chapter 2"},
                {"type": "paragraph", "id": "body2", "text": "Body 2"},
                {"type": "paragraph", "id": "tbl_title", "text": "Table 3-1: Caption"},
                {"type": "paragraph", "id": "after_para", "text": "After paragraph."},
            ]
        }),
        None,
    )
    .await
    .expect("create base doc");

    // Step 2: Read back to get the real anchor id (still "tbl_title").
    let bytes = std::fs::read(&path).expect("read doc");
    let _doc = read_word_document(&bytes).expect("parse doc");

    // Step 3: Insert a NEW low-level table with anchor_id + position: "after".
    tool.execute(
        serde_json::json!({
            "path": path.to_string_lossy(),
            "elements": [{
                "type": "table",
                "header": ["A", "B", "C"],
                "rows": [["1", "2", "3"]],
                "anchor_id": "tbl_title",
                "position": "after"
            }]
        }),
        None,
    )
    .await
    .expect("insert with anchor");

    // Step 4: Read back and check the table landed at the anchor.
    let bytes2 = std::fs::read(&path).expect("read doc");
    let doc2 = read_word_document(&bytes2).expect("parse doc");
    let elements = doc2.to_elements();

    let tbl_title_idx = elements.iter().position(|e| e.id() == "tbl_title");
    let after_para_idx = elements.iter().position(|e| e.id() == "after_para");
    let table_idx = elements.iter().position(|e| matches!(e, DocElement::Table { .. }));

    assert!(tbl_title_idx.is_some(), "tbl_title must exist");
    assert!(after_para_idx.is_some(), "after_para must exist");
    assert!(table_idx.is_some(), "table must exist");

    let table_idx = table_idx.unwrap();
    let tbl_title_idx = tbl_title_idx.unwrap();
    let after_para_idx = after_para_idx.unwrap();

    // The table MUST be between tbl_title and after_para.
    assert!(
        table_idx > tbl_title_idx,
        "BUG: table at #{} should appear AFTER tbl_title at {}",
        table_idx,
        tbl_title_idx
    );
    assert!(
        table_idx < after_para_idx,
        "BUG: table at #{} should appear BEFORE after_para at {} \
         (anchor was ignored — table fell back to end)",
        table_idx,
        after_para_idx
    );

    let _ = std::fs::remove_file(&path);
}

/// Full end-to-end reproduction of the user's bug report for `styled_table`.
#[tokio::test]
async fn e2e_full_repro_styled_table() {
    let path = temp_path("repro_styled");
    let tool = CreateWordDocTool::new();

    tool.execute(
        serde_json::json!({
            "path": path.to_string_lossy(),
            "elements": [
                {"type": "paragraph", "id": "title", "text": "Title"},
                {"type": "paragraph", "id": "h1", "text": "Chapter 1"},
                {"type": "paragraph", "id": "body1", "text": "Body 1"},
                {"type": "paragraph", "id": "h2", "text": "Chapter 2"},
                {"type": "paragraph", "id": "body2", "text": "Body 2"},
                {"type": "paragraph", "id": "tbl_title", "text": "Table 3-1: Caption"},
                {"type": "paragraph", "id": "after_para", "text": "After paragraph."},
            ]
        }),
        None,
    )
    .await
    .expect("create base doc");

    tool.execute(
        serde_json::json!({
            "path": path.to_string_lossy(),
            "elements": [{
                "type": "styled_table",
                "id": "my_styled",
                "headers": ["H1", "H2"],
                "rows": [["x", "y"]],
                "anchor_id": "tbl_title",
                "position": "after"
            }]
        }),
        None,
    )
    .await
    .expect("insert styled_table with anchor");

    let bytes = std::fs::read(&path).expect("read doc");
    let doc = read_word_document(&bytes).expect("parse doc");
    let elements = doc.to_elements();

    let tbl_title_idx = elements.iter().position(|e| e.id() == "tbl_title");
    let after_para_idx = elements.iter().position(|e| e.id() == "after_para");
    let table_idx = elements.iter().position(|e| matches!(e, DocElement::Table { .. }));

    assert!(tbl_title_idx.is_some(), "tbl_title must exist");
    assert!(after_para_idx.is_some(), "after_para must exist");
    assert!(table_idx.is_some(), "table must exist");

    let table_idx = table_idx.unwrap();
    let tbl_title_idx = tbl_title_idx.unwrap();
    let after_para_idx = after_para_idx.unwrap();

    assert!(
        table_idx > tbl_title_idx,
        "BUG: styled_table at #{} should appear AFTER tbl_title at {}",
        table_idx,
        tbl_title_idx
    );
    assert!(
        table_idx < after_para_idx,
        "BUG: styled_table at #{} should appear BEFORE after_para at {} \
         (anchor was ignored — component fell back to end)",
        table_idx,
        after_para_idx
    );

    let _ = std::fs::remove_file(&path);
}

/// Bogus anchor: anchor_id points to a non-existent paragraph. The tool
/// should warn and append the table at the end (preserving the existing
/// fallback behavior rather than silently dropping the element).
#[tokio::test]
async fn e2e_bogus_anchor_falls_back_to_end() {
    let path = temp_path("bogus");
    let tool = CreateWordDocTool::new();

    tool.execute(
        serde_json::json!({
            "path": path.to_string_lossy(),
            "elements": [
                {"type": "paragraph", "id": "title", "text": "Title"},
                {"type": "paragraph", "id": "after_para", "text": "After."},
            ]
        }),
        None,
    )
    .await
    .expect("create base doc");

    tool.execute(
        serde_json::json!({
            "path": path.to_string_lossy(),
            "elements": [{
                "type": "table",
                "header": ["A", "B"],
                "rows": [["1", "2"]],
                "anchor_id": "nonexistent_anchor_999",
                "position": "after"
            }]
        }),
        None,
    )
    .await
    .expect("insert with bogus anchor");

    let bytes = std::fs::read(&path).expect("read doc");
    let doc = read_word_document(&bytes).expect("parse doc");
    let elements = doc.to_elements();

    let after_para_idx = elements.iter().position(|e| e.id() == "after_para");
    let table_indices: Vec<usize> = elements
        .iter()
        .enumerate()
        .filter_map(|(i, e)| match e {
            DocElement::Table { .. } => Some(i),
            _ => None,
        })
        .collect();
    assert_eq!(table_indices.len(), 1);
    let table_idx = table_indices[0];
    // Bogus anchor: table falls back to end (after after_para).
    assert!(
        table_idx > after_para_idx.unwrap(),
        "bogus: table at #{} should fall back to end (after after_para at {:?})",
        table_idx,
        after_para_idx
    );

    let _ = std::fs::remove_file(&path);
}

/// Bogus anchor: tool output must include a visible warning so callers can
/// discover the typo / stale-id and avoid silent data loss. Before the fix,
/// the tool returned a bare "Successfully modified document" with no hint
/// that the anchor_id had been ignored.
#[tokio::test]
async fn e2e_bogus_anchor_emits_visible_warning_in_output() {
    let path = temp_path("bogus_warn");
    let tool = CreateWordDocTool::new();

    tool.execute(
        serde_json::json!({
            "path": path.to_string_lossy(),
            "elements": [
                {"type": "paragraph", "id": "title", "text": "Title"},
                {"type": "paragraph", "id": "after_para", "text": "After."},
            ]
        }),
        None,
    )
    .await
    .expect("create base doc");

    let result = tool
        .execute(
            serde_json::json!({
                "path": path.to_string_lossy(),
                "elements": [{
                    "type": "table",
                    "header": ["A", "B"],
                    "rows": [["1", "2"]],
                    "anchor_id": "nonexistent_anchor_999",
                    "position": "after"
                }]
            }),
            None,
        )
        .await
        .expect("insert with bogus anchor");

    // The success message must mention the bogus anchor and signal that
    // something went wrong (a warning header, the bogus id, or a fallback
    // phrase). We do not pin the exact wording so the message can evolve.
    assert!(
        result.contains("nonexistent_anchor_999"),
        "tool output should mention the bogus anchor_id; got: {}",
        result
    );
    let lowered = result.to_lowercase();
    assert!(
        lowered.contains("warning") || lowered.contains("not found"),
        "tool output should signal a warning or not-found condition; got: {}",
        result
    );

    let _ = std::fs::remove_file(&path);
}