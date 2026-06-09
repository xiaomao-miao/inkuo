//! Document parsing and serialization module
//! 
//! Handles:
//! - Markdown parsing (via pulldown-cmark)
//! - Word (.docx) and Excel (.xlsx) internal representation
//! - Block-level document model

use pulldown_cmark::{Event, Parser, Tag, TagEnd, CodeBlockKind, HeadingLevel};
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

        let parser = Parser::new_ext(content, pulldown_cmark::Options::all())
            .into_offset_iter();

        let mut code_block_lang: Option<String> = None;
        let mut code_block_buffer: Option<(String, usize)> = None;

        for (event, range) in parser {
            let range = range;
            match event {
                Event::Start(Tag::Heading { level, .. }) => {
                    let text = content[range.clone()].trim().to_string();
                    let line_info = offset_to_line_col(content, range.start);
                    blocks.push(Block {
                        id: uuid::Uuid::new_v4().to_string(),
                        kind: BlockKind::Heading {
                            level: match level {
                                HeadingLevel::H1 => 1,
                                HeadingLevel::H2 => 2,
                                HeadingLevel::H3 => 3,
                                HeadingLevel::H4 => 4,
                                HeadingLevel::H5 => 5,
                                HeadingLevel::H6 => 6,
                            },
                        },
                        range: Range {
                            start_line: line_info.0,
                            start_col: line_info.1,
                            end_line: line_info.0,
                            end_col: line_info.2,
                        },
                        text,
                        metadata: serde_json::json!({}),
                    });
                }
                Event::Start(Tag::CodeBlock(kind)) => {
                    if let Some((buf, start)) = code_block_buffer.take() {
                        blocks.push(pending_paragraph_block(&buf, content, start));
                    }
                    code_block_lang = match kind {
                        CodeBlockKind::Fenced(lang) => Some(lang.to_string()),
                        CodeBlockKind::Indented => None,
                    };
                    code_block_buffer = Some((String::new(), range.start));
                }
                Event::End(TagEnd::CodeBlock) => {
                    if let Some((buf, start)) = code_block_buffer.take() {
                        let end_offset = range.end;
                        let line_info = offset_to_line_col(content, start);
                        let end_line_info = offset_to_line_col(content, end_offset);
                        blocks.push(Block {
                            id: uuid::Uuid::new_v4().to_string(),
                            kind: BlockKind::CodeBlock { lang: code_block_lang.take() },
                            range: Range {
                                start_line: line_info.0,
                                start_col: line_info.1,
                                end_line: end_line_info.0,
                                end_col: end_line_info.2,
                            },
                            text: buf,
                            metadata: serde_json::json!({}),
                        });
                    }
                }
                Event::Start(Tag::Paragraph) | Event::End(TagEnd::Paragraph) => {}
                Event::Start(Tag::BlockQuote(_)) => {
                    let text = content[range.clone()].trim().to_string();
                    blocks.push(Block {
                        id: uuid::Uuid::new_v4().to_string(),
                        kind: BlockKind::Blockquote,
                        range: range_to_range(content, range.clone()),
                        text,
                        metadata: serde_json::json!({}),
                    });
                }
                Event::Start(Tag::List(ordered)) => {
                    let text = content[range.clone()].trim().to_string();
                    blocks.push(Block {
                        id: uuid::Uuid::new_v4().to_string(),
                        kind: BlockKind::List { ordered: ordered.is_some() },
                        range: range_to_range(content, range.clone()),
                        text,
                        metadata: serde_json::json!({}),
                    });
                }
                Event::Start(Tag::Item) => {}
                Event::End(TagEnd::Item) => {
                    let text = content[range.clone()].trim().to_string();
                    if !text.is_empty() {
                        blocks.push(Block {
                            id: uuid::Uuid::new_v4().to_string(),
                            kind: BlockKind::ListItem,
                            range: range_to_range(content, range.clone()),
                            text,
                            metadata: serde_json::json!({}),
                        });
                    }
                }
                Event::Start(Tag::Table(_)) | Event::End(TagEnd::Table) |
                Event::Start(Tag::TableHead) | Event::End(TagEnd::TableHead) |
                Event::Start(Tag::TableRow) | Event::End(TagEnd::TableRow) |
                Event::Start(Tag::TableCell) | Event::End(TagEnd::TableCell) => {
                    if matches!(event, Event::End(TagEnd::Table)) {
                        let text = content[range.clone()].trim().to_string();
                        blocks.push(Block {
                            id: uuid::Uuid::new_v4().to_string(),
                            kind: BlockKind::Table,
                            range: range_to_range(content, range.clone()),
                            text,
                            metadata: serde_json::json!({}),
                        });
                    }
                }
                Event::Rule => {
                    blocks.push(Block {
                        id: uuid::Uuid::new_v4().to_string(),
                        kind: BlockKind::HorizontalRule,
                        range: range_to_range(content, range.clone()),
                        text: "---".to_string(),
                        metadata: serde_json::json!({}),
                    });
                }
                _ => {}
            }
        }

        if let Some((buf, start)) = code_block_buffer.take() {
            blocks.push(pending_paragraph_block(&buf, content, start));
        }

        // Second pass: extract paragraphs (their content spans across events)
        let parser2 = Parser::new_ext(content, pulldown_cmark::Options::all())
            .into_offset_iter();
        let mut para_stack: Vec<std::ops::Range<usize>> = Vec::new();
        for (event, range) in parser2 {
            match event {
                Event::Start(Tag::Paragraph) => {
                    para_stack.push(range);
                }
                Event::End(TagEnd::Paragraph) => {
                    if let Some(start) = para_stack.pop() {
                        let text = content[start.start..range.end].trim().to_string();
                        if !text.is_empty() {
                            let line_info = offset_to_line_col(content, start.start);
                            blocks.push(Block {
                                id: uuid::Uuid::new_v4().to_string(),
                                kind: BlockKind::Paragraph,
                                range: Range {
                                    start_line: line_info.0,
                                    start_col: line_info.1,
                                    end_line: offset_to_line_col(content, range.end).0,
                                    end_col: line_info.2,
                                },
                                text,
                                metadata: serde_json::json!({}),
                            });
                        }
                    }
                }
                _ => {}
            }
        }

        let hash = format!("{:x}", sha2::Sha256::digest(content.as_bytes()));
        let path_obj = Path::new(path);
        let title = path_obj
            .file_stem()
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

fn offset_to_line_col(content: &str, offset: usize) -> (usize, usize, usize) {
    let mut line = 1;
    let mut col = 1;
    for (i, c) in content.char_indices() {
        if i >= offset {
            break;
        }
        if c == '\n' {
            line += 1;
            col = 1;
        } else {
            col += 1;
        }
    }
    (line, 1, col)
}

fn range_to_range(content: &str, range: std::ops::Range<usize>) -> Range {
    let start_line = offset_to_line_col(content, range.start).0;
    let end_line = offset_to_line_col(content, range.end.saturating_sub(1)).0;
    Range {
        start_line,
        start_col: 1,
        end_line,
        end_col: 1,
    }
}

fn pending_paragraph_block(buf: &str, content: &str, start: usize) -> Block {
    let line_info = offset_to_line_col(content, start);
    Block {
        id: uuid::Uuid::new_v4().to_string(),
        kind: BlockKind::Paragraph,
        range: Range {
            start_line: line_info.0,
            start_col: 1,
            end_line: line_info.0,
            end_col: line_info.2,
        },
        text: buf.trim().to_string(),
        metadata: serde_json::json!({}),
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
