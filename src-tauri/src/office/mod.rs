//! Office document parsing module
//!
//! Provides utilities to read and write Office documents (Word .docx and Excel .xlsx)

pub mod shared;
pub mod render_check;
mod docx;
mod xlsx;

// Re-export the document XML builder so other in-crate callers (e.g.
// `agent::tools::office::paragraph_columns` tests) can drive the
// writer without needing `pub mod docx`. The function is the same one
// `write_word_document_to_path` uses internally.
#[cfg(test)]
pub(crate) use docx::writer::build_document_xml;

pub use shared::{OfficeError, TableCell, TableRow};
pub use docx::{ElementId, WordDocument, WordParagraph, WordTable, WordImage, FontRun, FieldRef, DocElement, InsertElement, NumberingRef, WordSection, PageSize, PageSizeMm, PageMargins, HeaderPart, FooterPart, HeaderPartRef, FooterPartRef, read_word_document, word_document_to_text, write_word_document_to_path};
// Re-export the brand new design-system surface so callers can
// `use crate::office::{DesignTokens, ContentBlock, render_blocks}`.
// These are pure additions; existing call sites that only use the
// above types don't need to change.
pub use docx::design_tokens::{DesignTokens, FontScale, Palette, Spacing, DEFAULT_PALETTE};
pub use docx::components::{CalloutLevel, CalloutRender, CodeBlockRender, TableStyle};
pub use docx::renderer::{
    render_blocks, render_document, ContentBlock, ContentTableStyle, DocumentContent, RichRun,
    CalloutLevelName, RenderedDocument,
};
pub use docx::styled_writer::{
    build_callout_close_xml, build_callout_container_xml, build_code_block_container_xml,
    build_styled_table_xml, classify_and_strip, page_break_run_xml, TableKind,
    CALLOUT_MARKER_PREFIX, CODE_MARKER_PREFIX, STYLE_MARKER_PREFIX,
};
pub use docx::styled_styles::EXTENDED_STYLES_XML;
pub use docx::styled_pipeline::{write_styled_docx, write_sample_document, RenderStats};
pub use render_check::{
    find_libreoffice, render_docx_to_pngs, smoke_render, RenderCheckResult, RenderedPage,
};
pub use xlsx::{
    read_excel_workbook, excel_workbook_to_text,
    XlsxWorkbook, XlsxSheet, Cell, CellValue, MergedRange,
    CellModification, read_xlsx_structured,
    incremental_write_xlsx, create_xlsx_workbook, write_excel_document,
    cell_address, parse_cell_address,
    ExcelOperation,
};

use std::path::Path;

#[derive(Debug, Clone)]
pub enum OfficeFileType {
    Word(docx::WordDocument),
    Excel(xlsx::ExcelWorkbook),
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
