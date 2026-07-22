//! Markdown-flavoured plain-text rendering of a `WordDocument`.
//!
//! Both this file and `xml_parser.rs` used to live inside the
//! 4 800-line `docx/mod.rs`. Splitting the renderer out keeps
//! `xml_parser` strictly streaming-XML and lets this module be tested
//! without touching `quick_xml`.
//!
//! Public surface: the single `word_document_to_text` entry point,
//! re-exported by `docx/mod.rs` so existing
//! `crate::office::docx::word_document_to_text` import paths stay valid.

use crate::office::docx::{WordDocument, WordParagraph};

pub fn word_document_to_text(doc: &WordDocument) -> String {
    let mut output = String::new();

    // Helper: render a paragraph's runs to a markdown-flavoured string. Falls
    // back to the plain `text` field when no runs are present.
    fn render_paragraph(p: &WordParagraph) -> String {
        if let Some(ref runs) = p.runs {
            if !runs.is_empty() {
                let mut s = String::new();
                for r in runs {
                    let mut chunk = r.text.clone();
                    if r.italic { chunk = format!("*{}*", chunk); }
                    if r.bold { chunk = format!("**{}**", chunk); }
                    if r.underline { chunk = format!("__{}__", chunk); }
                    s.push_str(&chunk);
                }
                return s;
            }
        }
        p.text.clone()
    }

    if doc.tables.is_empty() {
        for para in &doc.paragraphs {
            if let Some(ref style) = para.style {
                output.push_str(&format!("[{}] ", style));
            }
            output.push_str(&render_paragraph(para));
            output.push_str("\n\n");
        }
    } else {
        let mut para_idx = 0;
        let mut rendered_table_rows: usize = 0;
        let mut in_table_block = false;

        for para in &doc.paragraphs {
            if rendered_table_rows > 0 {
                rendered_table_rows -= 1;
                if rendered_table_rows == 0 {
                    in_table_block = false;
                }
                para_idx += 1;
                continue;
            }

            if !in_table_block && !doc.tables.is_empty() && para.text.len() < 80 {
                let ahead_end = (para_idx + 5).min(doc.paragraphs.len());
                let ahead: Vec<_> = doc.paragraphs[para_idx..ahead_end]
                    .iter()
                    .filter(|p| !p.text.trim().is_empty())
                    .collect();

                if ahead.len() >= 2 {
                    let all_short = ahead.iter().all(|p| p.text.len() < 100);
                    let similar_length = ahead.len() > 1
                        && ahead.windows(2).all(|w| {
                            let diff = (w[0].text.len() as i32 - w[1].text.len() as i32).abs();
                            diff < 30
                        });

                    if all_short && similar_length {
                        output.push_str("--- Tables ---\n");
                        for tbl in &doc.tables {
                            for row in &tbl.rows {
                                let cells: Vec<String> = row.cells.iter().map(|c| c.text.clone()).collect();
                                output.push_str(&format!("| {}\n", cells.join(" | ")));
                            }
                            output.push('\n');
                            rendered_table_rows = tbl.rows.len();
                        }
                        in_table_block = true;
                        para_idx += 1;
                        continue;
                    }
                }
            }

            if !in_table_block {
                if let Some(ref style) = para.style {
                    output.push_str(&format!("[{}] ", style));
                }
                output.push_str(&render_paragraph(para));
                output.push_str("\n\n");
            }
            para_idx += 1;
        }
    }

    output.trim().to_string()
}
