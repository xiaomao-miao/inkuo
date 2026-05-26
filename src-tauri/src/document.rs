//! Document parsing and serialization module
//! 
//! Handles:
//! - Markdown parsing (via pulldown-cmark)
//! - Word (.docx) and Excel (.xlsx) internal representation
//! - Block-level document model

use serde::{Deserialize, Serialize};
use sha2::Digest;
use std::path::Path;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum DocumentError {
    #[error("Failed to read file: {0}")]
    ReadError(String),
    #[error("Failed to parse document: {0}")]
    ParseError(String),
    #[error("Unsupported format: {0}")]
    UnsupportedFormat(String),
    #[error("Failed to write file: {0}")]
    WriteError(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Document {
    pub id: String,
    pub path: String,
    pub doc_type: DocumentType,
    pub title: String,
    pub blocks: Vec<Block>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DocumentType {
    Markdown,
    Word,
    Excel,
    PlainText,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Block {
    pub id: String,
    pub kind: BlockKind,
    pub range: Range,
    pub text: String,
    pub metadata: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum BlockKind {
    Paragraph,
    Heading { level: u8 },
    CodeBlock { lang: Option<String> },
    List { ordered: bool },
    ListItem,
    Table,
    TableRow,
    TableCell,
    Blockquote,
    HorizontalRule,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Range {
    pub start_line: usize,
    pub start_col: usize,
    pub end_line: usize,
    pub end_col: usize,
}

impl Document {
    pub fn from_markdown(content: &str, path: &str) -> Result<Self, DocumentError> {
        let mut blocks = Vec::new();
        let mut current_block = String::new();
        let mut line_count = 0;
        
        for line in content.lines() {
            line_count += 1;
            let trimmed = line.trim();
            
            // Check for heading
            if trimmed.starts_with('#') {
                let level = trimmed.find(|c: char| !c.is_whitespace() && c != '#')
                    .unwrap_or(0);
                if level > 0 && level <= 6 {
                    blocks.push(Block {
                        id: uuid::Uuid::new_v4().to_string(),
                        kind: BlockKind::Heading { level: level as u8 },
                        range: Range {
                            start_line: line_count,
                            start_col: 0,
                            end_line: line_count,
                            end_col: line.len(),
                        },
                        text: trimmed.trim_start_matches(|c: char| c == '#' || c.is_whitespace()).to_string(),
                        metadata: serde_json::json!({}),
                    });
                    continue;
                }
            }
            
            // Check for code block
            if trimmed.starts_with("```") {
                if !current_block.is_empty() {
                    blocks.push(Block {
                        id: uuid::Uuid::new_v4().to_string(),
                        kind: BlockKind::Paragraph,
                        range: Range {
                            start_line: line_count - current_block.lines().count(),
                            start_col: 0,
                            end_line: line_count - 1,
                            end_col: current_block.lines().last().map(|l| l.len()).unwrap_or(0),
                        },
                        text: current_block.clone(),
                        metadata: serde_json::json!({}),
                    });
                    current_block.clear();
                }
                let lang = trimmed.trim_start_matches("```").to_string();
                blocks.push(Block {
                    id: uuid::Uuid::new_v4().to_string(),
                    kind: BlockKind::CodeBlock { lang: if lang.is_empty() { None } else { Some(lang) } },
                    range: Range {
                        start_line: line_count,
                        start_col: 0,
                        end_line: line_count,
                        end_col: line.len(),
                    },
                    text: trimmed.to_string(),
                    metadata: serde_json::json!({}),
                });
                continue;
            }
            
            current_block.push_str(line);
            current_block.push('\n');
        }
        
        // Handle remaining content
        if !current_block.is_empty() {
            blocks.push(Block {
                id: uuid::Uuid::new_v4().to_string(),
                kind: BlockKind::Paragraph,
                range: Range {
                    start_line: line_count - current_block.lines().count() + 1,
                    start_col: 0,
                    end_line: line_count,
                    end_col: current_block.lines().last().map(|l| l.len()).unwrap_or(0),
                },
                text: current_block,
                metadata: serde_json::json!({}),
            });
        }
        
        let hash = format!("{:x}", sha2::Sha256::digest(content.as_bytes()));
        let path_obj = Path::new(path);
        let title = path_obj.file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Untitled")
            .to_string();
        
        Ok(Document {
            id: uuid::Uuid::new_v4().to_string(),
            path: path.to_string(),
            doc_type: DocumentType::Markdown,
            title,
            blocks,
            updated_at: chrono::Utc::now(),
            hash,
        })
    }
    
    pub fn to_markdown(&self) -> String {
        self.blocks.iter().map(|block| {
            match &block.kind {
                BlockKind::Heading { level } => {
                    format!("{} {}", "#".repeat(*level as usize), block.text)
                }
                BlockKind::CodeBlock { lang } => {
                    match lang {
                        Some(l) => format!("```{}\n{}\n```", l, block.text),
                        None => format!("```\n{}\n```", block.text),
                    }
                }
                _ => block.text.clone(),
            }
        }).collect::<Vec<_>>().join("\n\n")
    }
}

pub fn detect_document_type(path: &str) -> DocumentType {
    let path = Path::new(path);
    match path.extension().and_then(|e| e.to_str()) {
        Some("md") | Some("markdown") => DocumentType::Markdown,
        Some("docx") | Some("doc") => DocumentType::Word,
        Some("xlsx") | Some("xls") => DocumentType::Excel,
        Some("txt") => DocumentType::PlainText,
        _ => DocumentType::PlainText,
    }
}
