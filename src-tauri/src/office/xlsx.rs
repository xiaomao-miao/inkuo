//! Excel (.xlsx) workbook reading and writing

use std::io::Cursor;

use calamine::{Data, Reader, Xlsx};

use super::shared::OfficeError;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ExcelSheet {
    pub name: String,
    pub headers: Vec<String>,
    pub rows: Vec<Vec<String>>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ExcelWorkbook {
    pub sheets: Vec<ExcelSheet>,
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

pub fn cell_to_string(cell: &Data) -> String {
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

pub fn write_excel_workbook(workbook: &ExcelWorkbook, output_path: &std::path::Path) -> Result<(), OfficeError> {
    use rust_xlsxwriter::*;

    let mut xl_workbook = Workbook::new();

    for sheet in &workbook.sheets {
        let worksheet = xl_workbook.add_worksheet();
        worksheet.set_name(&sheet.name).map_err(|e| OfficeError::Excel(e.to_string()))?;

        for (row_idx, row) in sheet.rows.iter().enumerate() {
            for (col_idx, cell) in row.iter().enumerate() {
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
