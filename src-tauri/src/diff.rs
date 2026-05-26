//! Diff engine module
//! 
//! Computes text diffs and maps them to document hunks
//! for visualization in the editor.

use serde::{Deserialize, Serialize};
use similar::{ChangeTag, TextDiff};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffResult {
    pub hunks: Vec<DiffHunk>,
    pub summary: DiffSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffHunk {
    pub id: String,
    pub old_range: HunkRange,
    pub new_range: HunkRange,
    pub changes: Vec<DiffChange>,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HunkRange {
    pub start_line: usize,
    pub end_line: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffChange {
    pub tag: ChangeType,
    pub old_line: Option<usize>,
    pub new_line: Option<usize>,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ChangeType {
    Delete,
    Insert,
    Equal,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffSummary {
    pub added_lines: usize,
    pub deleted_lines: usize,
    pub unchanged_lines: usize,
    pub description: String,
}

/// Diff summary for a specific file (used in streaming payload)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileDiffSummary {
    pub file_name: String,
    pub added_lines: usize,
    pub deleted_lines: usize,
    pub hunks: Vec<StreamDiffHunk>,
}

/// Stream-specific diff hunk (flattened ranges for JSON serialization)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamDiffHunk {
    pub id: String,
    pub old_start: usize,
    pub old_lines: usize,
    pub new_start: usize,
    pub new_lines: usize,
    pub changes: Vec<StreamDiffChange>,
}

/// Stream-specific diff change (string tag instead of enum)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamDiffChange {
    pub tag: String, // "delete", "insert", "equal"
    pub old_line: Option<usize>,
    pub new_line: Option<usize>,
    pub content: String,
}

pub fn compute_diff(old_text: &str, new_text: &str) -> DiffResult {
    let diff = TextDiff::from_lines(old_text, new_text);
    
    let mut hunks = Vec::new();
    let mut current_hunk: Option<DiffHunk> = None;
    
    let mut old_line = 0usize;
    let mut new_line = 0usize;
    let mut added_lines = 0usize;
    let mut deleted_lines = 0usize;
    let mut unchanged_lines = 0usize;
    
    for change in diff.iter_all_changes() {
        let change_type = match change.tag() {
            ChangeTag::Delete => {
                deleted_lines += 1;
                old_line += 1;
                ChangeType::Delete
            }
            ChangeTag::Insert => {
                added_lines += 1;
                new_line += 1;
                ChangeType::Insert
            }
            ChangeTag::Equal => {
                unchanged_lines += 1;
                old_line += 1;
                new_line += 1;
                ChangeType::Equal
            }
        };
        
        let diff_change = DiffChange {
            tag: change_type,
            old_line: if change.tag() != ChangeTag::Insert { Some(old_line) } else { None },
            new_line: if change.tag() != ChangeTag::Delete { Some(new_line) } else { None },
            content: change.value().to_string(),
        };
        
        // Start new hunk after context break
        if change.tag() != ChangeTag::Equal && current_hunk.is_none() {
            current_hunk = Some(DiffHunk {
                id: uuid::Uuid::new_v4().to_string(),
                old_range: HunkRange { start_line: old_line, end_line: old_line },
                new_range: HunkRange { start_line: new_line, end_line: new_line },
                changes: Vec::new(),
                summary: String::new(),
            });
        }
        
        if let Some(ref mut hunk) = current_hunk {
            hunk.changes.push(diff_change);
            hunk.old_range.end_line = old_line;
            hunk.new_range.end_line = new_line;
            
            // Finalize hunk after some context
            if change.tag() == ChangeTag::Equal && hunk.changes.len() > 6 {
                hunk.summary = generate_hunk_summary(&hunk.changes);
                hunks.push(hunk.clone());
                current_hunk = None;
            }
        }
    }
    
    // Finalize last hunk
    if let Some(mut hunk) = current_hunk {
        hunk.summary = generate_hunk_summary(&hunk.changes);
        hunks.push(hunk);
    }
    
    let description = generate_diff_description(added_lines, deleted_lines);
    
    DiffResult {
        hunks,
        summary: DiffSummary {
            added_lines,
            deleted_lines,
            unchanged_lines,
            description,
        },
    }
}

fn generate_hunk_summary(changes: &[DiffChange]) -> String {
    let added = changes.iter().filter(|c| matches!(c.tag, ChangeType::Insert)).count();
    let deleted = changes.iter().filter(|c| matches!(c.tag, ChangeType::Delete)).count();
    
    match (added, deleted) {
        (a, 0) if a > 0 => format!("新增 {} 行", a),
        (0, d) if d > 0 => format!("删除 {} 行", d),
        (a, d) => format!("修改：新增 {} 行，删除 {} 行", a, d),
    }
}

fn generate_diff_description(added: usize, deleted: usize) -> String {
    if added == 0 && deleted == 0 {
        "内容无变化".to_string()
    } else if deleted == 0 {
        format!("新增 {} 行", added)
    } else if added == 0 {
        format!("删除 {} 行", deleted)
    } else {
        format!("修改：新增 {} 行，删除 {} 行", added, deleted)
    }
}
