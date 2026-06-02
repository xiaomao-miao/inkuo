//! Shared types and utilities for office document handling

use serde::{Deserialize, Serialize};
use std::io::{Read, Write as IoWrite};
use thiserror::Error;
use zip::ZipArchive;

#[derive(Error, Debug)]
pub enum OfficeError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Zip error: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("XML error: {0}")]
    Xml(String),
    #[error("Excel error: {0}")]
    Excel(String),
    #[error("Unsupported file type: {0}")]
    UnsupportedFileType(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableCell {
    pub text: String,
    pub col_span: usize,
    pub row_span: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableRow {
    pub cells: Vec<TableCell>,
}

pub fn read_zip_entry(zip_data: &[u8], name: &str) -> Result<String, OfficeError> {
    let mut archive = ZipArchive::new(std::io::Cursor::new(zip_data))?;
    let mut file = archive.by_name(name)?;
    let mut content = String::new();
    file.read_to_string(&mut content)?;
    Ok(content)
}
