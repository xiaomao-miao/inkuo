//! Office document parsing module
//!
//! Provides utilities to read Office documents (Word .docx and Excel .xlsx)
//! in a text-friendly format that can be understood by the AI agent.

use calamine::{Data, Reader, Xlsx};
use serde::{Deserialize, Serialize};
use std::io::{Cursor, Read, Write as IoWrite};
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
pub struct WordParagraph {
    pub text: String,
    pub style: Option<String>,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WordTable {
    pub rows: Vec<TableRow>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WordDocument {
    pub paragraphs: Vec<WordParagraph>,
    pub tables: Vec<WordTable>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExcelSheet {
    pub name: String,
    pub headers: Vec<String>,
    pub rows: Vec<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExcelWorkbook {
    pub sheets: Vec<ExcelSheet>,
}

fn read_zip_entry(zip_data: &[u8], name: &str) -> Result<String, OfficeError> {
    let mut archive = ZipArchive::new(Cursor::new(zip_data))?;
    let mut file = archive.by_name(name)?;
    let mut content = String::new();
    file.read_to_string(&mut content)?;
    Ok(content)
}

fn parse_document_xml(content: &str) -> Result<Vec<WordParagraph>, OfficeError> {
    let mut paragraphs = Vec::new();
    let mut reader = quick_xml::Reader::from_str(content);
    reader.config_mut().trim_text(false);

    let mut buf = Vec::new();
    let mut in_para = false;
    let mut current_text = String::new();
    let mut current_style: Option<String> = None;
    let mut para_depth = 0;

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(quick_xml::events::Event::Start(ref e)) | Ok(quick_xml::events::Event::Empty(ref e)) => {
                let name = e.local_name();
                if name.as_ref() == b"p" {
                    in_para = true;
                    para_depth += 1;
                    current_text.clear();
                    current_style = None;
                } else if name.as_ref() == b"t" && in_para {
                    if let Ok(quick_xml::events::Event::Text(t)) = reader.read_event_into(&mut buf) {
                        current_text.push_str(&t.unescape().unwrap_or_default());
                    }
                } else if name.as_ref() == b"pStyle" {
                    if let Ok(quick_xml::events::Event::Text(t)) = reader.read_event_into(&mut buf) {
                        current_style = Some(t.unescape().unwrap_or_default().to_string());
                    }
                }
            }
            Ok(quick_xml::events::Event::End(ref e)) => {
                let name = e.local_name();
                if name.as_ref() == b"p" {
                    para_depth -= 1;
                    if para_depth == 0 {
                        in_para = false;
                        let text = current_text.trim().to_string();
                        if !text.is_empty() {
                            paragraphs.push(WordParagraph {
                                text,
                                style: current_style.clone(),
                            });
                        }
                    }
                }
            }
            Ok(quick_xml::events::Event::Eof) => break,
            Err(e) => return Err(OfficeError::Xml(format!("XML parse error: {}", e))),
            _ => {}
        }
        buf.clear();
    }

    Ok(paragraphs)
}

fn parse_table_xml(content: &str) -> Result<Vec<WordTable>, OfficeError> {
    let mut tables = Vec::new();
    let mut reader = quick_xml::Reader::from_str(content);
    reader.config_mut().trim_text(false);

    let mut buf = Vec::new();
    let mut in_table = false;
    let mut current_table: Option<WordTable> = None;
    let mut current_row: Option<Vec<TableCell>> = None;
    let mut current_cell_text = String::new();
    let mut cell_col_span: usize = 1;
    let mut cell_row_span: usize = 1;
    let mut table_depth = 0;
    let mut row_depth = 0;
    let mut cell_depth = 0;

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(quick_xml::events::Event::Start(ref e)) => {
                let name = e.local_name();
                match name.as_ref() {
                    b"tbl" => {
                        in_table = true;
                        table_depth += 1;
                        current_table = Some(WordTable { rows: Vec::new() });
                    }
                    b"tr" => {
                        row_depth += 1;
                        current_row = Some(Vec::new());
                    }
                    b"tc" => {
                        cell_depth += 1;
                        current_cell_text.clear();
                        cell_col_span = 1;
                        cell_row_span = 1;
                    }
                    b"t" if cell_depth > 0 => {
                        if let Ok(quick_xml::events::Event::Text(t)) = reader.read_event_into(&mut buf) {
                            current_cell_text.push_str(&t.unescape().unwrap_or_default());
                        }
                    }
                    b"gridSpan" => {
                        if let Ok(quick_xml::events::Event::Text(t)) = reader.read_event_into(&mut buf) {
                            if let Ok(n) = t.unescape().unwrap_or_default().parse::<usize>() {
                                cell_col_span = n;
                            }
                        }
                    }
                    _ => {}
                }
            }
            Ok(quick_xml::events::Event::Empty(ref e)) => {
                let name = e.local_name();
                if name.as_ref() == b"gridSpan" {
                    for attr in e.attributes().with_checks(false) {
                        if let Ok(attr) = attr {
                            if attr.key.as_ref() == b"val" {
                                let val = std::str::from_utf8(&attr.value).unwrap_or("1");
                                if let Ok(n) = val.parse::<usize>() {
                                    cell_col_span = n;
                                }
                            }
                        }
                    }
                }
            }
            Ok(quick_xml::events::Event::End(ref e)) => {
                let name = e.local_name();
                match name.as_ref() {
                    b"tc" => {
                        cell_depth -= 1;
                        if cell_depth == 0 {
                            if let Some(ref mut row) = current_row {
                                row.push(TableCell {
                                    text: current_cell_text.trim().to_string(),
                                    col_span: cell_col_span,
                                    row_span: cell_row_span,
                                });
                            }
                        }
                    }
                    b"tr" => {
                        row_depth -= 1;
                        if row_depth == 0 {
                            if let Some(row) = current_row.take() {
                                if let Some(ref mut tbl) = current_table {
                                    tbl.rows.push(TableRow { cells: row });
                                }
                            }
                        }
                    }
                    b"tbl" => {
                        table_depth -= 1;
                        if table_depth == 0 {
                            in_table = false;
                            if let Some(table) = current_table.take() {
                                if !table.rows.is_empty() {
                                    tables.push(table);
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
            Ok(quick_xml::events::Event::Eof) => break,
            Err(e) => return Err(OfficeError::Xml(format!("XML parse error: {}", e))),
            _ => {}
        }
        buf.clear();
    }

    Ok(tables)
}

pub fn read_word_document(bytes: &[u8]) -> Result<WordDocument, OfficeError> {
    let doc_content = read_zip_entry(bytes, "word/document.xml")?;
    let paragraphs = parse_document_xml(&doc_content)?;
    let tables = parse_table_xml(&doc_content)?;
    Ok(WordDocument { paragraphs, tables })
}

pub fn word_document_to_text(doc: &WordDocument) -> String {
    let mut output = String::new();

    if doc.tables.is_empty() {
        for para in &doc.paragraphs {
            if let Some(ref style) = para.style {
                output.push_str(&format!("[{}] ", style));
            }
            output.push_str(&para.text);
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
                output.push_str(&para.text);
                output.push_str("\n\n");
            }
            para_idx += 1;
        }
    }

    output.trim().to_string()
}

pub fn read_excel_workbook(bytes: &[u8]) -> Result<ExcelWorkbook, OfficeError> {
    let cursor = Cursor::new(bytes.to_vec());
    let mut workbook: Xlsx<Cursor<Vec<u8>>> = calamine::open_workbook_from_rs(cursor)
        .map_err(|e: calamine::XlsxError| OfficeError::Excel(e.to_string()))?;

    let sheet_names = workbook.sheet_names();
    let mut sheets = Vec::new();

    for name in sheet_names {
        if let Ok(range) = workbook.worksheet_range(name.as_str()) {
            let mut headers = Vec::new();
            let mut rows: Vec<Vec<String>> = Vec::new();

            for (row_idx, row) in range.rows().enumerate() {
                let row_data: Vec<String> = row.iter().map(|c| cell_to_string(c)).collect();

                if row_idx == 0 && !row_data.is_empty() && !row_data.iter().all(|s| s.is_empty()) {
                    headers = row_data.clone();
                    rows.push(row_data);
                } else {
                    rows.push(row_data);
                }
            }

            sheets.push(ExcelSheet {
                name: name.to_string(),
                headers,
                rows,
            });
        }
    }

    Ok(ExcelWorkbook { sheets })
}

fn cell_to_string(cell: &Data) -> String {
    match cell {
        Data::Int(n) => n.to_string(),
        Data::Float(f) => {
            if f.fract() == 0.0 {
                format!("{:.0}", f)
            } else {
                format!("{}", f)
            }
        }
        Data::String(ref s) => s.clone(),
        Data::Bool(b) => b.to_string(),
        Data::DateTime(ref dt) => format!("{:.0}", dt.as_f64()),
        Data::DateTimeIso(ref s) => s.clone(),
        Data::DurationIso(ref s) => s.clone(),
        Data::Error(ref e) => format!("#ERR:{:?}", e),
        Data::Empty => String::new(),
    }
}

pub fn excel_workbook_to_text(workbook: &ExcelWorkbook) -> String {
    let mut output = String::new();

    for sheet in &workbook.sheets {
        output.push_str(&format!("=== Sheet: {} ===\n\n", sheet.name));

        if sheet.rows.is_empty() {
            output.push_str("(empty sheet)\n");
        } else {
            let max_cols = sheet.rows.iter().map(|r| r.len()).max().unwrap_or(0);
            let col_widths: Vec<usize> = (0..max_cols)
                .map(|col| {
                    sheet.rows
                        .iter()
                        .map(|row| row.get(col).map(|s| s.len()).unwrap_or(0))
                        .max()
                        .unwrap_or(0)
                        .max(8)
                })
                .collect();

            if !sheet.headers.is_empty() {
                let header_row: Vec<String> = sheet
                    .headers
                    .iter()
                    .enumerate()
                    .map(|(i, h)| {
                        let w = col_widths.get(i).copied().unwrap_or(8);
                        format!("{:w$}", h, w = w)
                    })
                    .collect();
                output.push_str(&header_row.join(" | "));
                output.push('\n');
                output.push_str(&col_widths.iter().map(|w| "-".repeat(*w)).collect::<Vec<_>>().join("-+-"));
                output.push('\n');
            }

            for row in sheet.rows.iter().take(100) {
                let row_text: Vec<String> = row
                    .iter()
                    .enumerate()
                    .map(|(i, cell)| {
                        let w = col_widths.get(i).copied().unwrap_or(8);
                        format!("{:w$}", cell, w = w)
                    })
                    .collect();
                output.push_str(&row_text.join(" | "));
                output.push('\n');
            }

            if sheet.rows.len() > 100 {
                output.push_str(&format!("\n... ({} more rows)\n", sheet.rows.len() - 100));
            }
        }

        output.push('\n');
    }

    output.trim().to_string()
}

#[derive(Debug, Clone)]
pub enum OfficeFileType {
    Word(WordDocument),
    Excel(ExcelWorkbook),
}

pub fn read_office_file(path: &std::path::Path) -> Result<(OfficeFileType, String), OfficeError> {
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

// ─── Write Functions ────────────────────────────────────────────────────────────

/// Write an ExcelWorkbook to a .xlsx file using rust_xlsxwriter.
pub fn write_excel_workbook(workbook: &ExcelWorkbook, output_path: &std::path::Path) -> Result<(), OfficeError> {
    use rust_xlsxwriter::*;

    let mut xl_workbook = Workbook::new();

    for sheet in &workbook.sheets {
        let worksheet = xl_workbook.add_worksheet();
        worksheet.set_name(&sheet.name).map_err(|e| OfficeError::Excel(e.to_string()))?;

        for (row_idx, row) in sheet.rows.iter().enumerate() {
            for (col_idx, cell) in row.iter().enumerate() {
                // Try to parse as number first
                if let Ok(num) = cell.parse::<f64>() {
                    worksheet.write(row_idx as u32, col_idx as u16, num)
                        .map_err(|e| OfficeError::Excel(e.to_string()))?;
                } else {
                    worksheet.write(row_idx as u32, col_idx as u16, cell.as_str())
                        .map_err(|e| OfficeError::Excel(e.to_string()))?;
                }
            }
        }
    }

    xl_workbook.save(output_path)
        .map_err(|e| OfficeError::Io(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())))?;

    Ok(())
}

/// Write a WordDocument to a .docx file by constructing a minimal ZIP archive.
pub fn write_word_document(doc: &WordDocument, output_path: &std::path::Path) -> Result<(), OfficeError> {
    use std::io::Cursor;

    let mut buf = Vec::new();
    {
        let mut zip = zip::ZipWriter::new(Cursor::new(&mut buf));

        let opts = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated)
            .unix_permissions(0o644);

        // [Content_Types].xml
        zip.start_file("[Content_Types].xml", opts)?;
        zip.write_all(CONTENT_TYPES_XML.as_bytes())?;

        // _rels/.rels
        zip.start_file("_rels/.rels", opts)?;
        zip.write_all(RELS_XML.as_bytes())?;

        // word/_rels/document.xml.rels
        zip.start_file("word/_rels/document.xml.rels", opts)?;
        zip.write_all(WORD_RELS_XML.as_bytes())?;

        // word/document.xml
        let doc_xml = build_document_xml(doc);
        zip.start_file("word/document.xml", opts)?;
        zip.write_all(doc_xml.as_bytes())?;

        // word/styles.xml (minimal)
        zip.start_file("word/styles.xml", opts)?;
        zip.write_all(STYLES_XML.as_bytes())?;

        // word/settings.xml (minimal)
        zip.start_file("word/settings.xml", opts)?;
        zip.write_all(SETTINGS_XML.as_bytes())?;

        // word/fontTable.xml
        zip.start_file("word/fontTable.xml", opts)?;
        zip.write_all(FONT_TABLE_XML.as_bytes())?;

        // word/settings.xml -> word/theme/theme1.xml (minimal theme)
        zip.start_file("word/theme/theme1.xml", opts)?;
        zip.write_all(THEME_XML.as_bytes())?;

        // word/settings.xml -> [Content_Types].xml for theme
        // (we embed theme inline in content types)

        zip.finish()?;
    }

    std::fs::write(output_path, &buf)?;
    Ok(())
}

fn build_document_xml(doc: &WordDocument) -> String {
    let mut xml = String::from(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"
            xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
  <w:body>"#
    );

    for para in &doc.paragraphs {
        xml.push_str("\n    <w:p>");
        if let Some(ref style) = para.style {
            xml.push_str(&format!("<w:pPr><w:pStyle w:val=\"{}\"/></w:pPr>", escape_xml(style)));
        }
        for chunk in para.text.split('\n') {
            if !chunk.is_empty() {
                xml.push_str(&format!(
                    "<w:r><w:t xml:space=\"preserve\">{}</w:t></w:r>",
                    escape_xml(chunk)
                ));
            }
            xml.push_str("<w:r><w:br/></w:r>");
        }
        xml.push_str("</w:p>");
    }

    for table in &doc.tables {
        xml.push_str("\n    <w:tbl>");
        for row in &table.rows {
            xml.push_str("\n      <w:tr>");
            for cell in &row.cells {
                xml.push_str("<w:tc><w:tcPr>");
                if cell.col_span > 1 {
                    xml.push_str(&format!("<w:gridSpan w:val=\"{}\"/>", cell.col_span));
                }
                xml.push_str("</w:tcPr><w:p>");
                for chunk in cell.text.split('\n') {
                    if !chunk.is_empty() {
                        xml.push_str(&format!(
                            "<w:r><w:t xml:space=\"preserve\">{}</w:t></w:r>",
                            escape_xml(chunk)
                        ));
                    }
                    xml.push_str("<w:r><w:br/></w:r>");
                }
                xml.push_str("</w:p></w:tc>");
            }
            xml.push_str("</w:tr>");
        }
        xml.push_str("\n    </w:tbl>");
    }

    xml.push_str("\n  </w:body>\n</w:document>");
    xml
}

fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

/// Unified write function — auto-detects format from extension.
pub fn write_office_file(
    path: &std::path::Path,
    json_content: &str,
) -> Result<(), OfficeError> {
    let extension = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    match extension.as_str() {
        "xlsx" => {
            let workbook: ExcelWorkbook = serde_json::from_str(json_content)
                .map_err(|e| OfficeError::Excel(e.to_string()))?;
            write_excel_workbook(&workbook, path)?;
            Ok(())
        }
        "docx" => {
            let doc: WordDocument = serde_json::from_str(json_content)
                .map_err(|e| OfficeError::Xml(e.to_string()))?;
            write_word_document(&doc, path)?;
            Ok(())
        }
        _ => Err(OfficeError::UnsupportedFileType(extension)),
    }
}

// ─── Minimal OOXML boilerplate ─────────────────────────────────────────────────

const CONTENT_TYPES_XML: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
  <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
  <Default Extension="xml" ContentType="application/xml"/>
  <Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>
  <Override PartName="/word/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml"/>
  <Override PartName="/word/settings.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.settings+xml"/>
  <Override PartName="/word/fontTable.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.fontTable+xml"/>
  <Override PartName="/word/theme/theme1.xml" ContentType="application/vnd.openxmlformats-officedocument.theme+xml"/>
</Types>"#;

const RELS_XML: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/>
</Relationships>"#;

const WORD_RELS_XML: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/>
  <Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/settings" Target="settings.xml"/>
  <Relationship Id="rId3" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/fontTable" Target="fontTable.xml"/>
  <Relationship Id="rId4" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/theme" Target="theme/theme1.xml"/>
</Relationships>"#;

const STYLES_XML: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:docDefaults>
    <w:rPrDefault>
      <w:rPr>
        <w:rFonts w:ascii="Calibri" w:hAnsi="Calibri" w:cs="Times New Roman"/>
        <w:sz w:val="22"/>
        <w:szCs w:val="22"/>
      </w:rPr>
    </w:rPrDefault>
  </w:docDefaults>
</w:styles>"#;

const SETTINGS_XML: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:settings xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:defaultTabStop w:val="720"/>
</w:settings>"#;

const FONT_TABLE_XML: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:fonts xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:font w:name="Calibri">
    <w:panose1 w:val="020F0502020204030204"/>
    <w:charset w:val="00"/>
    <w:family w:val="swiss"/>
    <w:pitch w:val="variable"/>
  </w:font>
  <w:font w:name="Times New Roman">
    <w:panose1 w:val="02020603050405020304"/>
    <w:charset w:val="00"/>
    <w:family w:val="roman"/>
    <w:pitch w:val="variable"/>
  </w:font>
</w:fonts>"#;

const THEME_XML: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<a:theme xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" name="Office Theme">
  <a:themeElements>
    <a:clrScheme name="Office">
      <a:dk1><a:sysClr val="windowText" lastClr="000000"/></a:dk1>
      <a:lt1><a:sysClr val="window" lastClr="FFFFFF"/></a:lt1>
      <a:dk2><a:srgbClr val="1F497D"/></a:dk2>
      <a:lt2><a:srgbClr val="EEECE1"/></a:lt2>
      <a:accent1><a:srgbClr val="4F81BD"/></a:accent1>
      <a:accent2><a:srgbClr val="C0504D"/></a:accent2>
      <a:accent3><a:srgbClr val="9BBB59"/></a:accent3>
      <a:accent4><a:srgbClr val="8064A2"/></a:accent4>
      <a:accent5><a:srgbClr val="4BACC6"/></a:accent5>
      <a:accent6><a:srgbClr val="F79646"/></a:accent6>
      <a:hlink><a:srgbClr val="0000FF"/></a:hlink>
      <a:folHlink><a:srgbClr val="800080"/></a:folHlink>
    </a:clrScheme>
    <a:fontScheme name="Office">
      <a:majorFont><a:latin typeface="Calibri"/><a:ea typeface=""/><a:cs typeface="Times New Roman"/></a:majorFont>
      <a:minorFont><a:latin typeface="Calibri"/><a:ea typeface=""/><a:cs typeface="Times New Roman"/></a:minorFont>
    </a:fontScheme>
    <a:fmtScheme name="Office">
      <a:fillStyleLst>
        <a:solidFill><a:schemeClr val="phClr"/></a:solidFill>
        <a:gradFill rotWithShape="1"><a:gsLst><a:gs pos="0"><a:schemeClr val="phClr"><a:tint val="65000"/></a:schemeClr></a:gs><a:gs pos="100000"><a:schemeClr val="phClr"><a:shade val="99000"/></a:schemeClr></a:gs></a:gsLst><a:lin ang="5400000" scaled="0"/></a:gradFill>
      </a:fillStyleLst>
      <a:lnStyleLst>
        <a:ln w="9525" cap="flat" cmpd="sng" algn="ctr"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:prstDash val="solid"/></a:ln>
        <a:ln w="25400" cap="flat" cmpd="sng" algn="ctr"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:prstDash val="solid"/></a:ln>
        <a:ln w="38100" cap="flat" cmpd="sng" algn="ctr"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:prstDash val="solid"/></a:ln>
      </a:lnStyleLst>
      <a:effectStyleLst>
        <a:effectStyle><a:effectLst/></a:effectStyle>
        <a:effectStyle><a:effectLst/></a:effectStyle>
        <a:effectStyle><a:effectLst/></a:effectStyle>
      </a:effectStyleLst>
      <a:bgFillStyleLst>
        <a:solidFill><a:schemeClr val="phClr"/></a:solidFill>
        <a:gradFill rotWithShape="1"><a:gsLst><a:gs pos="0"><a:schemeClr val="phClr"><a:tint val="95000"/></a:schemeClr></a:gs><a:gs pos="100000"><a:schemeClr val="phClr"><a:shade val="85000"/></a:schemeClr></a:gs></a:gsLst><a:lin ang="5400000" scaled="0"/></a:gradFill>
      </a:bgFillStyleLst>
    </a:fmtScheme>
  </a:themeElements>
  <a:objectDefaults/>
  <a:extraClrSchemeLst/>
</a:theme>"#;
