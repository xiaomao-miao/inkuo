//! Plain-text rendering for the structured [`XlsxWorkbook`] / [`XlsxSheet`]
//! API.
//!
//! The full XLSX surface (formulas, merged ranges, styles, etc.) used to be
//! rendered inside the same 3 400-line `xlsx/mod.rs`. Splitting the renderer
//! out keeps `mod.rs` focused on zip I/O and streaming XML, and lets this
//! module be tested without touching `quick_xml` or `zip`.
//!
//! Public surface: the single `xlsx_workbook_to_text` entry point,
//! re-exported by `mod.rs` so existing
//! `crate::office::xlsx::xlsx_workbook_to_text` import paths keep working.

use crate::office::xlsx::XlsxWorkbook;

pub fn xlsx_workbook_to_text(workbook: &XlsxWorkbook) -> String {
    let mut output = String::new();
    for sheet in &workbook.sheets {
        output.push_str(&format!(
            "=== Sheet: {} ({}x{}) ===\n\n",
            sheet.name, sheet.max_row, sheet.max_col
        ));

        if sheet.cells.is_empty() {
            output.push_str("(empty sheet)\n\n");
            continue;
        }

        let mut grid: Vec<Vec<String>> =
            vec![vec![String::new(); sheet.max_col.max(1)]; sheet.max_row.max(1)];
        for cell in &sheet.cells {
            let display = if let Some(f) = &cell.formula {
                format!("={}", f)
            } else {
                cell.value.as_string_for_display()
            };
            if cell.row < grid.len() && cell.col < (grid.get(0).map(|r| r.len()).unwrap_or(0)) {
                grid[cell.row][cell.col] = display;
            }
        }

        let col_widths: Vec<usize> = (0..sheet.max_col.max(1))
            .map(|c| {
                sheet
                    .cells
                    .iter()
                    .filter(|cell| cell.col == c)
                    .map(|cell| {
                        let v = if let Some(f) = &cell.formula {
                            format!("={}", f)
                        } else {
                            cell.value.as_string_for_display()
                        };
                        v.chars().count().max(8)
                    })
                    .max()
                    .unwrap_or(8)
            })
            .collect();

        for row in grid.iter() {
            let cells: Vec<String> = row
                .iter()
                .enumerate()
                .map(|(i, c)| {
                    let w = col_widths.get(i).copied().unwrap_or(8);
                    format!("{:w$}", c, w = w)
                })
                .collect();
            output.push_str(&cells.join(" | "));
            output.push('\n');
        }

        let styled: Vec<String> = sheet
            .cells
            .iter()
            .filter_map(|cell| {
                cell.style.as_ref().and_then(|s| {
                    if s.number_format != "General" {
                        Some(format!("{}={}", cell.address(), s.number_format))
                    } else {
                        None
                    }
                })
            })
            .collect();
        if !styled.is_empty() {
            output.push_str(&format!("\nFormats: {}\n", styled.join(", ")));
        }
        if !sheet.merged_cells.is_empty() {
            let merged_addrs: Vec<String> =
                sheet.merged_cells.iter().map(|m| m.address()).collect();
            output.push_str(&format!("Merged: {}\n", merged_addrs.join(", ")));
        }
        output.push('\n');
    }
    output.trim().to_string()
}
