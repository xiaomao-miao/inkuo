//! Plain-text rendering for the legacy flat [`ExcelWorkbook`] / [`ExcelSheet`]
//! API.
//!
//! Cell-to-string conversion and `excel_workbook_to_text` lived inside the
//! 3 400-line `xlsx/mod.rs`. Both are pure string assembly over the
//! existing types and have no zip / parser state, so we split them into a
//! dedicated file and re-export the function from `mod.rs` so the public
//! path `crate::office::xlsx::excel_workbook_to_text` keeps working.

use calamine::Data;

use crate::office::xlsx::ExcelWorkbook;

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
                    sheet
                        .rows
                        .iter()
                        .map(|row| row.get(col).map(|s| s.len()).unwrap_or(0))
                        .max()
                        .unwrap_or(0)
                        .max(8)
                })
                .collect();

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
