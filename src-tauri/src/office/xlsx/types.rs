//! Re-export surface for Excel (.xlsx) data types.
//!
//! Same pattern as `office::docx::types` — this is the canonical
//! downstream surface for the structured workbook model. The legacy
//! 2D-grid types (`ExcelWorkbook` / `ExcelSheet`) are exposed too because
//! the editor's writer path still uses them for the back-compat
//! "read_to_string → write_excel_workbook" round-trip.

pub use crate::office::xlsx::{
    Cell, CellModification, CellStyle, CellValue, ExcelOperation, ExcelWorkbook, ExcelSheet,
    MergedRange, XlsxWorkbook, XlsxSheet,
};
