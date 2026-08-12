//! Re-export surface for Word (.docx) data types.
//!
//! This module exists so future sub-module splits (`reader.rs`,
//! `writer.rs`) can both reference the canonical types without each
//! having to re-declare them. Today the canonical declarations live in
//! `mod.rs`; this module simply re-exports them so downstream code can
//! `use crate::office::docx::types::WordDocument` (or
//! `use crate::office::docx::WordDocument` via the existing re-exports)
//! without coupling to a single 4 800-line file.
//!
//! Adding new fields? Update the declaration in `mod.rs` AND keep this
//! `pub use` in sync, so the re-export surface stays a faithful
//! projection of the canonical schema.

pub use crate::office::docx::{
    DocElement, FieldRef, FontRun, InsertElement, NumberingRef, WordDocument,
    WordDocumentMeta, WordImage, WordParagraph, WordSection, WordTable,
    HeaderPart, HeaderPartRef, FooterPart, FooterPartRef,
    PageSize, PageSizeMm, PageMargins,
};
