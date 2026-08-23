//! Office document parsing module
//!
//! Provides utilities to read and write Office documents (Word .docx and Excel .xlsx)

mod docx;
pub mod render_check;
pub mod shared;
mod xlsx;

// Re-export the document XML builder so other in-crate callers (e.g.
// `agent::tools::office::paragraph_columns` tests) can drive the
// writer without needing `pub mod docx`. The function is the same one
// `write_word_document_to_path` uses internally.
#[cfg(test)]
pub(crate) use docx::writer::build_document_xml;

pub use docx::{
    read_word_document, word_document_to_text, write_word_document_to_path, DocElement, ElementId,
    FieldRef, FontRun, FooterPart, FooterPartRef, HeaderPart, HeaderPartRef, InsertElement,
    NumberingRef, PageMargins, PageSize, PageSizeMm, WordDocument, WordDocumentMeta, WordImage,
    WordParagraph, WordSection, WordTable,
};
pub use shared::{OfficeError, TableCell, TableRow};
// Re-export the brand new design-system surface so callers can
// `use crate::office::{DesignTokens, ContentBlock, render_blocks}`.
// These are pure additions; existing call sites that only use the
// above types don't need to change.
pub use docx::components::{CalloutLevel, CalloutRender, CodeBlockRender, TableStyle};
pub use docx::design_tokens::{DesignTokens, FontScale, Palette, Spacing, DEFAULT_PALETTE};
pub use docx::renderer::{
    render_blocks, render_document, CalloutLevelName, ContentBlock, ContentTableStyle,
    DocumentContent, RenderedDocument, RichRun,
};
pub use docx::styled_pipeline::{write_sample_document, write_styled_docx, RenderStats};
pub use docx::styled_styles::EXTENDED_STYLES_XML;
pub use docx::styled_writer::{
    build_callout_close_xml, build_callout_container_xml, build_code_block_container_xml,
    build_styled_table_xml, classify_and_strip, page_break_run_xml, TableKind,
    CALLOUT_MARKER_PREFIX, CODE_MARKER_PREFIX, STYLE_MARKER_PREFIX,
};
pub use render_check::{
    find_libreoffice, render_docx_to_pngs, render_office_page_window_to_pngs,
    render_office_to_pngs, smoke_render, RenderCheckResult, RenderedPage,
};
pub use xlsx::{
    cell_address, create_xlsx_workbook, excel_workbook_to_text, incremental_write_xlsx,
    parse_cell_address, read_excel_workbook, read_xlsx_structured, write_excel_document, Cell,
    CellModification, CellValue, ExcelOperation, MergedRange, XlsxSheet, XlsxWorkbook,
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
            let doc: docx::WordDocument =
                serde_json::from_str(json_content).map_err(|e| OfficeError::Json(e.to_string()))?;
            docx::write_word_document_to_path(&doc, path, None)?;
            Ok(())
        }
        _ => Err(OfficeError::UnsupportedFileType(extension)),
    }
}
