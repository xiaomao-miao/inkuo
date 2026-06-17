//! Office document parsing module
//!
//! Provides utilities to read and write Office documents (Word .docx and Excel .xlsx)

pub mod shared;
mod docx;
mod xlsx;

pub use shared::{OfficeError, TableCell, TableRow, read_all_zip_entries};
pub use docx::{WordDocument, WordParagraph, WordTable, FontRun, DocElement, NumberingRef, read_word_document, word_document_to_text, write_word_document_to_path};
pub use xlsx::{ExcelWorkbook, read_excel_workbook, excel_workbook_to_text};

use std::path::Path;

#[derive(Debug, Clone)]
pub enum OfficeFileType {
    Word(docx::WordDocument),
    Excel(xlsx::ExcelWorkbook),
}

/// Structured elements for Word documents (paragraphs + tables with IDs).
pub struct WordElements {
    pub elements: Vec<DocElement>,
}

pub fn read_office_file(path: &Path) -> Result<(OfficeFileType, String), OfficeError> {
    let bytes = std::fs::read(path)?;

    let extension = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    match extension.as_str() {
        "docx" => {
            let doc = read_word_document(&bytes)?;
            let text = word_document_to_text(&doc);
            Ok((OfficeFileType::Word(doc), text))
        }
        "xlsx" => {
            let workbook = read_excel_workbook(&bytes)?;
            let text = excel_workbook_to_text(&workbook);
            Ok((OfficeFileType::Excel(workbook), text))
        }
        _ => Err(OfficeError::UnsupportedFileType(extension)),
    }
}

pub fn word_document_to_elements(doc: &docx::WordDocument) -> Vec<DocElement> {
    doc.to_elements()
}

pub fn write_office_file(path: &Path, json_content: &str) -> Result<(), OfficeError> {
    let extension = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    match extension.as_str() {
        "xlsx" => {
            let workbook: xlsx::ExcelWorkbook = serde_json::from_str(json_content)
                .map_err(|e| OfficeError::Excel(e.to_string()))?;
            xlsx::write_excel_workbook(&workbook, path)?;
            Ok(())
        }
        "docx" => {
            let doc: docx::WordDocument = serde_json::from_str(json_content)
                .map_err(|e| OfficeError::Json(e.to_string()))?;
            docx::write_word_document_to_path(&doc, path, None)?;
            Ok(())
        }
        _ => Err(OfficeError::UnsupportedFileType(extension)),
    }
}
