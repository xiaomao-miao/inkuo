//! Shared types and utilities for office document handling

use serde::{Deserialize, Serialize};
use std::io::Read;
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
    #[error("JSON error: {0}")]
    Json(String),
    #[error("Unsupported file type: {0}")]
    UnsupportedFileType(String),
}

/// A single cell in a word/Excel-style table. When round-tripping through
/// the JSON wire format (e.g. `DocElement::Table`), the cell may be either a
/// bare string (the common case for a 1×1 cell) or an object with explicit
/// `col_span`/`row_span` for merged cells. `Deserialize` accepts both forms.
#[derive(Debug, Clone, Serialize)]
pub struct TableCell {
    pub text: String,
    pub col_span: usize,
    pub row_span: usize,
}

impl TableCell {
    pub fn plain(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            col_span: 1,
            row_span: 1,
        }
    }
}

impl<'de> serde::Deserialize<'de> for TableCell {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Either {
            Str(String),
            Full {
                #[serde(default)]
                text: String,
                #[serde(default = "default_one")]
                col_span: usize,
                #[serde(default = "default_one")]
                row_span: usize,
            },
        }

        fn default_one() -> usize {
            1
        }

        match Either::deserialize(deserializer)? {
            Either::Str(text) => Ok(TableCell {
                text,
                col_span: 1,
                row_span: 1,
            }),
            Either::Full {
                text,
                col_span,
                row_span,
            } => Ok(TableCell {
                text,
                col_span: col_span.max(1),
                row_span: row_span.max(1),
            }),
        }
    }
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

/// Read all entries from a docx zip and return them as a map of path -> raw bytes.
/// Used to preserve the original document's boilerplate (styles, settings, fonts, etc.)
/// when modifying only the document.xml body.
pub fn read_all_zip_entries(zip_data: &[u8]) -> Result<std::collections::HashMap<String, Vec<u8>>, OfficeError> {
    let mut archive = ZipArchive::new(std::io::Cursor::new(zip_data))?;
    let mut entries = std::collections::HashMap::new();
    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        let name = file.name().to_string();
        let mut content = Vec::new();
        file.read_to_end(&mut content)?;
        entries.insert(name, content);
    }
    Ok(entries)
}
