//! XLSX zip-package writer — turns an [`XlsxWorkbook`] into an OOXML package.
//!
//! Pulled out of `mod.rs` because the package-writing logic is ~370 lines of
//! verbatim string templating, and it's only loosely related to the mod's
//! other responsibility (text-based incremental rewriting). Splitting it
//! leaves `mod.rs` focused on the parsed types, the streaming reader, and
//! the conservative cell-by-cell writer.
//!
//! Public surface (`pub(crate)` so `mod.rs` can re-export from one place):
//! - [`create_xlsx_workbook`] — build a brand-new xlsx package from scratch.
//! - [`write_excel_document`] — round-trip an existing xlsx, replacing only
//!   the worksheet / styles / theme / rels entries that depend on the new
//!   workbook state and copying every other entry verbatim.
//!
//! Internal helpers (all `fn`, used only inside this file):
//! - [`parse_sheet_name_to_path_map`] — looks up `<sheet name="..."> -> worksheets/sheetN.xml`
//!   for `write_excel_document`'s preserved-entry bookkeeping.
//! - [`build_workbook_styles`] / [`build_sheet_xml`] / [`build_cell_xml`] /
//!   [`escape_xml_attr`] — XML string constructors used by both writers.

use std::collections::HashMap;
use std::io::{Read, Seek};

use super::{cell_address, Cell, CellValue, SheetStyleKey, XlsxSheet, XlsxWorkbook};
use crate::office::shared::OfficeError;
use crate::office::xlsx::ooxml_boilerplate::MINIMAL_THEME_XML;

use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter, ZipArchive};

// ============================================================================
// Top-level writers
// ============================================================================

/// Enable verbose logging in the structured writer (mirrors `DEBUG_XLSX` in
/// `mod.rs`). Local copy because pulling in a `const` from a `pub(crate)`
/// module adds friction for no benefit.
const DEBUG_XLSX: bool = true;

/// Read a single ZIP entry as a UTF-8 string. Returns the empty string if the
/// entry is missing (matches the historical mod.rs behaviour).
fn read_entry<R: Read + Seek>(
    archive: &mut ZipArchive<R>,
    name: &str,
) -> Result<String, OfficeError> {
    let mut file = archive.by_name(name)?;
    let mut s = String::new();
    file.read_to_string(&mut s)?;
    Ok(s)
}

fn escape_xml_text(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

// ─── Workbook creation (from scratch) ────────────────────────────────────────

/// Create a new xlsx file from a [`XlsxWorkbook`] specification. Builds a
/// minimal but valid OOXML package from scratch:
/// - `[Content_Types].xml` registers the workbook part.
/// - `xl/workbook.xml` declares the sheets.
/// - `xl/_rels/workbook.xml.rels` maps sheet rIds to file paths.
/// - `xl/sharedStrings.xml` is emitted only if at least one sheet uses
///   shared-string references; otherwise cells use inline strings.
/// - `xl/styles.xml` contains the minimum font/fill/border entries plus
///   the cellXfs entries referenced by cells.
/// - `xl/worksheets/sheetN.xml` contains the actual cell data.
///
/// String-typed cells are written as inline strings (`<is><t>...</t></is>`)
/// so we never need to maintain a shared string pool. Numeric and date cells
/// are written as `<c><v>numeric</v></c>`. This keeps the emitted xlsx
/// dependency-free (no separate sst update step) and means the round-trip
/// parser we already have can re-read what we wrote.
pub fn create_xlsx_workbook(
    workbook: &XlsxWorkbook,
    output_path: &std::path::Path,
) -> Result<(), OfficeError> {
    use std::io::Write as _;

    if workbook.sheets.is_empty() {
        return Err(OfficeError::Excel("cannot create workbook with zero sheets".to_string()));
    }

    let file = std::fs::File::create(output_path)?;
    let mut zip = zip::ZipWriter::new(file);
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .unix_permissions(0o644);

    let n_sheets = workbook.sheets.len();

    // 1. [Content_Types].xml — must enumerate EVERY part with an Override.
    let mut content_types = String::from(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
<Default Extension="xml" ContentType="application/xml"/>"#,
    );
    for i in 0..n_sheets {
        content_types.push_str(&format!(
            "<Override PartName=\"/xl/worksheets/sheet{}.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml\"/>",
            i + 1
        ));
    }
    content_types.push_str(
        "<Override PartName=\"/xl/workbook.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml\"/>\
<Override PartName=\"/xl/styles.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml\"/>\
<Override PartName=\"/xl/theme/theme1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.theme+xml\"/>\
<Override PartName=\"/docProps/core.xml\" ContentType=\"application/vnd.openxmlformats-package.core-properties+xml\"/>\
<Override PartName=\"/docProps/app.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.extended-properties+xml\"/>\
</Types>",
    );
    zip.start_file("[Content_Types].xml", opts)?;
    zip.write_all(content_types.as_bytes())?;

    // 2. _rels/.rels — top-level relationships, mapping the package to its
    //    main document part. Excel REQUIRES this to find xl/workbook.xml.
    let top_rels = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/>
<Relationship Id="rId2" Type="http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties" Target="docProps/core.xml"/>
<Relationship Id="rId3" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/extended-properties" Target="docProps/app.xml"/>
</Relationships>"#;
    zip.start_file("_rels/.rels", opts)?;
    zip.write_all(top_rels.as_bytes())?;

    // 3. docProps/core.xml — minimal core properties.
    let core_props = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties" xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:dcterms="http://purl.org/dc/terms/" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance">
<dc:creator>inkuo</dc:creator>
<cp:lastModifiedBy>inkuo</cp:lastModifiedBy>
<dcterms:created xsi:type="dcterms:W3CDTF">2024-01-01T00:00:00Z</dcterms:created>
<dcterms:modified xsi:type="dcterms:W3CDTF">2024-01-01T00:00:00Z</dcterms:modified>
</cp:coreProperties>"#;
    zip.start_file("docProps/core.xml", opts)?;
    zip.write_all(core_props.as_bytes())?;

    // 4. docProps/app.xml — minimal extended properties.
    let app_props = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Properties xmlns="http://schemas.openxmlformats.org/officeDocument/2006/extended-properties">
<Application>inkuo</Application>
<DocSecurity>0</DocSecurity>
<ScaleCrop>false</ScaleCrop>
<LinksUpToDate>false</LinksUpToDate>
<SharedDoc>false</SharedDoc>
<HyperlinksChanged>false</HyperlinksChanged>
<AppVersion>16.0000</AppVersion>
</Properties>"#;
    zip.start_file("docProps/app.xml", opts)?;
    zip.write_all(app_props.as_bytes())?;

    // 5. xl/workbook.xml — declare each sheet and reference the relationships
    //    namespace. The `r:` prefix is declared on the root <workbook> element
    //    and used by the <sheet> children for r:id="...". The `state="visible"`
    //    attribute is required by the spec; some readers reject sheets without it.
    let mut workbook_xml = String::from(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
<workbookPr/><bookViews><workbookView activeTab="0" firstSheet="0" showHorizontalScroll="1" showVerticalScroll="1" showSheetTabs="1" tabRatio="600" windowHeight="10000" windowWidth="20000"/></bookViews>
<sheets>"#,
    );
    for (i, sheet) in workbook.sheets.iter().enumerate() {
        let sheet_id = (i + 1) as u32;
        let rid = format!("rId{}", i + 1);
        let state = if sheet.state.is_empty() { "visible" } else { &sheet.state };
        workbook_xml.push_str(&format!(
            "<sheet name=\"{}\" sheetId=\"{}\" state=\"{}\" r:id=\"{}\"/>",
            escape_xml_attr(&sheet.name),
            sheet_id,
            escape_xml_attr(state),
            rid
        ));
    }
    workbook_xml.push_str("</sheets><calcPr calcId=\"124519\"/></workbook>");
    zip.start_file("xl/workbook.xml", opts)?;
    zip.write_all(workbook_xml.as_bytes())?;

    // 6. xl/_rels/workbook.xml.rels — maps each sheet rId to its worksheet
    //    file, and adds relationships for the theme and styles.
    let mut rels_xml = String::from(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#,
    );
    for i in 0..n_sheets {
        rels_xml.push_str(&format!(
            "<Relationship Id=\"rId{}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet\" Target=\"worksheets/sheet{}.xml\"/>",
            i + 1,
            i + 1
        ));
    }
    rels_xml.push_str(&format!(
        "<Relationship Id=\"rId{}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles\" Target=\"styles.xml\"/>",
        n_sheets + 1
    ));
    rels_xml.push_str(&format!(
        "<Relationship Id=\"rId{}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/theme\" Target=\"theme/theme1.xml\"/>",
        n_sheets + 2
    ));
    rels_xml.push_str("</Relationships>");
    zip.start_file("xl/_rels/workbook.xml.rels", opts)?;
    zip.write_all(rels_xml.as_bytes())?;

    // 7. xl/styles.xml — rebuilt from actual used styles.
    let (styles_xml, all_style_map) = build_workbook_styles(workbook);
    zip.start_file("xl/styles.xml", opts)?;
    zip.write_all(styles_xml.as_bytes())?;

    // 8. xl/theme/theme1.xml — a minimal Office theme. Excel doesn't strictly
    //    require this, but readers that load the relationship from workbook.xml
    //    WILL try to fetch it. Without a theme file, the file fails to open.
    zip.start_file("xl/theme/theme1.xml", opts)?;
    zip.write_all(MINIMAL_THEME_XML.as_bytes())?;

    // 9. xl/worksheets/sheetN.xml — one per sheet.
    for (i, sheet) in workbook.sheets.iter().enumerate() {
        let sheet_xml = build_sheet_xml(sheet, &all_style_map[i]);
        let path = format!("xl/worksheets/sheet{}.xml", i + 1);
        zip.start_file(&path, opts)?;
        zip.write_all(sheet_xml.as_bytes())?;
    }

    zip.finish()?;
    Ok(())
}

/// Write an [`XlsxWorkbook`] to a file, preserving all original ZIP entries that
/// are not being regenerated.
///
/// This is the structured equivalent of the old string-based `incremental_write_xlsx`.
/// If `original_bytes` is `Some`, we copy every entry from the original zip and
/// only overwrite `xl/worksheets/sheet*.xml` (and `xl/styles.xml` if modified).
/// If `original_bytes` is `None`, we fall back to `create_xlsx_workbook` behavior
/// (generate everything from scratch).
pub fn write_excel_document(
    workbook: &XlsxWorkbook,
    original_bytes: Option<&[u8]>,
    output_path: &std::path::Path,
) -> Result<(), OfficeError> {
    use std::io::{Read, Write as _};

    if workbook.sheets.is_empty() {
        return Err(OfficeError::Excel("cannot write workbook with zero sheets".to_string()));
    }

    // If no original bytes, delegate entirely to create_xlsx_workbook.
    let Some(bytes) = original_bytes else {
        return create_xlsx_workbook(workbook, output_path);
    };

    // Collect original ZIP entries we'll copy verbatim (everything except sheet XMLs).
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes.to_vec()))?;
    let mut preserved_entries: Vec<(String, Vec<u8>)> = Vec::new();

    // Read workbook.xml + rels to get sheet name -> path mapping.
    let wb_xml = read_entry(&mut archive, "xl/workbook.xml").unwrap_or_default();
    let wb_rels = read_entry(&mut archive, "xl/_rels/workbook.xml.rels").unwrap_or_default();
    let _name_to_path: std::collections::HashMap<String, String> =
        parse_sheet_name_to_path_map(&wb_xml, &wb_rels)
            .unwrap_or_default();

    // Collect entries to preserve (everything except xl/worksheets/ and entries we'll regenerate below).
    // Skip: [Content_Types].xml, _rels/.rels, docProps/*.xml, xl/workbook.xml,
    // xl/_rels/workbook.xml.rels, xl/styles.xml, xl/theme/theme1.xml
    // (those are regenerated in steps 2-9 below to reflect new state).
    let regenerated: std::collections::HashSet<&'static str> = [
        "[Content_Types].xml",
        "_rels/.rels",
        "docProps/core.xml",
        "docProps/app.xml",
        "xl/workbook.xml",
        "xl/_rels/workbook.xml.rels",
        "xl/styles.xml",
        "xl/theme/theme1.xml",
    ].into_iter().collect();

    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;
        let name = entry.name().to_string();
        if !name.starts_with("xl/worksheets/") && !regenerated.contains(name.as_str()) {
            let mut buf = Vec::new();
            entry.read_to_end(&mut buf)?;
            preserved_entries.push((name, buf));
        }
    }
    drop(archive);

    // Open the output file and write the new ZIP.
    let file = std::fs::File::create(output_path)?;
    let mut zip = zip::ZipWriter::new(file);
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .unix_permissions(0o644);

    // 1. Copy preserved entries.
    for (name, buf) in preserved_entries {
        zip.start_file(&name, opts)?;
        zip.write_all(&buf)?;
    }

    // 2. [Content_Types].xml — regenerated to list all sheets.
    let n_sheets = workbook.sheets.len();
    let mut content_types = String::from(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
<Default Extension="xml" ContentType="application/xml"/>
<Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>
<Override PartName="/xl/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml"/>
<Override PartName="/xl/theme/theme1.xml" ContentType="application/vnd.openxmlformats-officedocument.theme+xml"/>
<Override PartName="/docProps/core.xml" ContentType="application/vnd.openxmlformats-package.core-properties+xml"/>
<Override PartName="/docProps/app.xml" ContentType="application/vnd.openxmlformats-officedocument.extended-properties+xml"/>"#,
    );
    for i in 1..=n_sheets {
        content_types.push_str(&format!(
            "<Override PartName=\"/xl/worksheets/sheet{}.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml\"/>",
            i
        ));
    }
    content_types.push_str("</Types>");
    zip.start_file("[Content_Types].xml", opts)?;
    zip.write_all(content_types.as_bytes())?;

    // 3. _rels/.rels
    let top_rels = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/>
<Relationship Id="rId2" Type="http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties" Target="docProps/core.xml"/>
<Relationship Id="rId3" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/extended-properties" Target="docProps/app.xml"/>
</Relationships>"#;
    zip.start_file("_rels/.rels", opts)?;
    zip.write_all(top_rels.as_bytes())?;

    // 4. docProps/core.xml
    let core_props = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties" xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:dcterms="http://purl.org/dc/terms/" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance">
<dc:creator>inkuo</dc:creator>
<cp:lastModifiedBy>inkuo</cp:lastModifiedBy>
<dcterms:created xsi:type="dcterms:W3CDTF">2024-01-01T00:00:00Z</dcterms:created>
<dcterms:modified xsi:type="dcterms:W3CDTF">2024-01-01T00:00:00Z</dcterms:modified>
</cp:coreProperties>"#;
    zip.start_file("docProps/core.xml", opts)?;
    zip.write_all(core_props.as_bytes())?;

    // 5. docProps/app.xml
    let app_props = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Properties xmlns="http://schemas.openxmlformats.org/officeDocument/2006/extended-properties">
<Application>inkuo</Application>
<DocSecurity>0</DocSecurity>
<ScaleCrop>false</ScaleCrop>
<LinksUpToDate>false</LinksUpToDate>
<SharedDoc>false</SharedDoc>
<HyperlinksChanged>false</HyperlinksChanged>
<AppVersion>16.0000</AppVersion>
</Properties>"#;
    zip.start_file("docProps/app.xml", opts)?;
    zip.write_all(app_props.as_bytes())?;

    // 6. xl/workbook.xml — regenerated to match new sheet names/order.
    let mut workbook_xml = String::from(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
<workbookPr/><bookViews><workbookView activeTab="0" firstSheet="0" showHorizontalScroll="1" showVerticalScroll="1" showSheetTabs="1" tabRatio="600" windowHeight="10000" windowWidth="20000"/></bookViews>
<sheets>"#,
    );
    for (i, sheet) in workbook.sheets.iter().enumerate() {
        let sheet_id = (i + 1) as u32;
        let rid = format!("rId{}", i + 1);
        let state = if sheet.state.is_empty() { "visible" } else { &sheet.state };
        workbook_xml.push_str(&format!(
            "<sheet name=\"{}\" sheetId=\"{}\" state=\"{}\" r:id=\"{}\"/>",
            escape_xml_attr(&sheet.name),
            sheet_id,
            escape_xml_attr(state),
            rid
        ));
    }
    workbook_xml.push_str("</sheets><calcPr calcId=\"124519\"/></workbook>");
    zip.start_file("xl/workbook.xml", opts)?;
    zip.write_all(workbook_xml.as_bytes())?;

    // 7. xl/_rels/workbook.xml.rels — regenerated.
    let mut rels_xml = String::from(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#,
    );
    for i in 0..n_sheets {
        rels_xml.push_str(&format!(
            "<Relationship Id=\"rId{}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet\" Target=\"worksheets/sheet{}.xml\"/>",
            i + 1,
            i + 1
        ));
    }
    rels_xml.push_str(&format!(
        "<Relationship Id=\"rId{}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles\" Target=\"styles.xml\"/>",
        n_sheets + 1
    ));
    rels_xml.push_str(&format!(
        "<Relationship Id=\"rId{}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/theme\" Target=\"theme/theme1.xml\"/>",
        n_sheets + 2
    ));
    rels_xml.push_str("</Relationships>");
    zip.start_file("xl/_rels/workbook.xml.rels", opts)?;
    zip.write_all(rels_xml.as_bytes())?;

    // 8. xl/styles.xml — rebuilt from actual used styles.
    let (styles_xml, all_style_map) = build_workbook_styles(workbook);
    zip.start_file("xl/styles.xml", opts)?;
    zip.write_all(styles_xml.as_bytes())?;

    // 9. xl/theme/theme1.xml
    zip.start_file("xl/theme/theme1.xml", opts)?;
    zip.write_all(MINIMAL_THEME_XML.as_bytes())?;

    // 10. xl/worksheets/sheetN.xml — write each sheet's structured XML.
    for (i, sheet) in workbook.sheets.iter().enumerate() {
        let sheet_xml = build_sheet_xml(sheet, &all_style_map[i]);
        let path = format!("xl/worksheets/sheet{}.xml", i + 1);
        zip.start_file(&path, opts)?;
        zip.write_all(sheet_xml.as_bytes())?;
    }

    zip.finish()?;
    Ok(())
}



// ============================================================================
// XML builders used by both writers
// ============================================================================

pub(crate) fn build_styles_xml(used_styles: &[(SheetStyleKey, usize)]) -> String {
    let mut num_fmts: std::collections::BTreeMap<String, u32> = std::collections::BTreeMap::new();
    let mut next_num_fmt = 164u32;
    let mut fonts: Vec<(SheetStyleKey, usize)> = Vec::new();
    // FIX: Use tuple key to properly deduplicate fonts with different bold/italic
    let mut font_index: std::collections::HashMap<(Option<String>, Option<u32>, Option<String>, bool, bool), usize> = std::collections::HashMap::new();
    let mut fills: Vec<(Option<String>, Option<String>, usize)> = Vec::new();
    let mut fill_index: std::collections::HashMap<(Option<String>, Option<String>), usize> = std::collections::HashMap::new();

    let _default_font_idx = *font_index.entry((None, None, None, false, false)).or_insert_with(|| {
        let idx = fonts.len();
        fonts.push((SheetStyleKey::default(), idx));
        idx
    });
    let _default_fill_idx = *fill_index.entry((None, None)).or_insert_with(|| {
        let idx = fills.len();
        fills.push((None, None, idx));
        idx
    });
    // Don't pre-seed numFmts with an empty key: numFmtId 0 is reserved by the
    // spec and writing `<numFmt numFmtId="0" formatCode=""/>` confuses readers.
    // The default ("General") numFmtId is always 0 and never needs declaring.

    // FIX: Add numFmtId to xfs tuple
    let mut xfs: Vec<(usize, usize, u32, bool, bool)> = Vec::new();

    for (key, _) in used_styles.iter() {
        // FIX: Use full font info tuple as key instead of just font_name
        let font_key = (key.font_name.clone(), key.font_size, key.font_color.clone(), key.font_bold, key.font_italic);
        let font_idx = *font_index.entry(font_key).or_insert_with(|| {
            let idx = fonts.len();
            fonts.push((key.clone(), idx));
            idx
        });
        let fill_idx = *fill_index.entry((key.fill_fg_color.clone(), key.fill_bg_color.clone())).or_insert_with(|| {
            let idx = fills.len();
            fills.push((key.fill_fg_color.clone(), key.fill_bg_color.clone(), idx));
            idx
        });
        if !key.number_format.is_empty() {
            num_fmts.entry(key.number_format.clone()).or_insert_with(|| {
                let id = next_num_fmt;
                next_num_fmt += 1;
                id
            });
        }
        // FIX: Calculate numFmtId properly
        let num_fmt_id = if key.number_format.is_empty() {
            0
        } else {
            *num_fmts.get(&key.number_format).unwrap_or(&0)
        };
        xfs.push((font_idx, fill_idx, num_fmt_id, key.font_bold, key.font_italic));
    }

    let mut xml = String::from(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#);
    xml.push_str("\n<styleSheet xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\">\n");
    if !num_fmts.is_empty() {
        xml.push_str("<numFmts count=\"");
        xml.push_str(&num_fmts.len().to_string());
        xml.push_str("\">");
        for (fmt, id) in &num_fmts {
            xml.push_str("<numFmt numFmtId=\"");
            xml.push_str(&id.to_string());
            xml.push_str("\" formatCode=\"");
            xml.push_str(&escape_xml_attr(fmt));
            xml.push_str("\"/>");
        }
        xml.push_str("</numFmts>\n");
    } else {
        xml.push_str("<numFmts count=\"0\"/>\n");
    }

    xml.push_str("<fonts count=\"");
    xml.push_str(&(fonts.len() + 1).to_string());
    xml.push_str("\">");
    xml.push_str("<font><name val=\"Calibri\"/><family val=\"2\"/><color theme=\"1\"/><sz val=\"11\"/><scheme val=\"minor\"/></font>");
    for (style, _) in &fonts {
        xml.push_str("<font>");
        xml.push_str("<name val=\"");
        xml.push_str(&escape_xml_attr(style.font_name.as_deref().unwrap_or("Calibri")));
        xml.push_str("\"/>");
        xml.push_str("<family val=\"2\"/>");
        if let Some(color) = &style.font_color {
            xml.push_str("<color rgb=\"");
            xml.push_str(&escape_xml_attr(color));
            xml.push_str("\"/>");
        } else {
            xml.push_str("<color theme=\"1\"/>");
        }
        xml.push_str("<sz val=\"");
        xml.push_str(&style.font_size.unwrap_or(11).to_string());
        xml.push_str("\"/>");
        if style.font_bold { xml.push_str("<b/>"); }
        if style.font_italic { xml.push_str("<i/>"); }
        xml.push_str("<scheme val=\"minor\"/>");
        xml.push_str("</font>");
    }
    xml.push_str("</fonts>\n");

    xml.push_str("<fills count=\"");
    xml.push_str(&(fills.len() + 2).to_string());
    xml.push_str("\">");
    xml.push_str("<fill><patternFill/></fill>");
    xml.push_str("<fill><patternFill patternType=\"gray125\"/></fill>");
    for (fg, bg, _) in &fills {
        xml.push_str("<fill><patternFill patternType=\"solid\">");
        if let Some(color) = fg {
            xml.push_str("<fgColor rgb=\"");
            xml.push_str(&escape_xml_attr(color));
            xml.push_str("\"/>");
        }
        if let Some(color) = bg {
            xml.push_str("<bgColor rgb=\"");
            xml.push_str(&escape_xml_attr(color));
            xml.push_str("\"/>");
        }
        xml.push_str("</patternFill></fill>");
    }
    xml.push_str("</fills>\n");

    xml.push_str("<borders count=\"1\"><border><left/><right/><top/><bottom/><diagonal/></border></borders>\n");
    xml.push_str("<cellStyleXfs count=\"1\"><xf numFmtId=\"0\" fontId=\"0\" fillId=\"0\" borderId=\"0\"/></cellStyleXfs>\n");
    xml.push_str("<cellXfs count=\"");
    xml.push_str(&(xfs.len() + 1).to_string());
    xml.push_str("\">");
    xml.push_str("<xf numFmtId=\"0\" fontId=\"0\" fillId=\"0\" borderId=\"0\" pivotButton=\"0\" quotePrefix=\"0\" xfId=\"0\"/>");
    for (font_idx, fill_idx, num_fmt_id, bold, italic) in &xfs {
        // FIX: Use actual numFmtId instead of hardcoded 0
        let mut attrs = format!("numFmtId=\"{}\" fontId=\"{}\" fillId=\"{}\" borderId=\"0\" xfId=\"0\"", num_fmt_id, font_idx + 1, fill_idx + 2);
        if *bold || *italic { attrs.push_str(" applyFont=\"1\""); }
        // FIX: Apply fill whenever a non-default fill is in use
        if *fill_idx > 0 { attrs.push_str(" applyFill=\"1\""); }
        attrs.push_str(" applyBorder=\"0\" applyNumberFormat=\"1\"");
        xml.push_str("<xf ");
        xml.push_str(&attrs);
        xml.push_str("/>");
    }
    xml.push_str("</cellXfs>\n");
    xml.push_str("<cellStyles count=\"1\"><cellStyle name=\"Normal\" xfId=\"0\" builtinId=\"0\" hidden=\"0\"/></cellStyles>\n");
    xml.push_str("<dxfs count=\"0\"/>\n");
    xml.push_str(r#"<tableStyles count="0" defaultTableStyle="TableStyleMedium9" defaultPivotStyle="PivotStyleLight16"/>"#);
    xml.push_str("\n</styleSheet>");
    xml
}
// ============================================================================
// Internal helpers (used only by the writers above)
// ============================================================================

/// Parse sheet names to file paths from workbook.xml and its relationships.
/// Returns a HashMap for O(1) lookup.
fn parse_sheet_name_to_path_map(
    workbook_xml: &str,
    rels_xml: &str,
) -> Result<std::collections::HashMap<String, String>, OfficeError> {
    use quick_xml::events::Event;
    use quick_xml::Reader;

    let mut name_to_path = std::collections::HashMap::new();

    // Parse rels: rId -> target path
    let mut rid_to_path = std::collections::HashMap::new();
    let mut rels_reader = Reader::from_str(rels_xml);
    rels_reader.config_mut().trim_text(true);
    let mut rels_buf = Vec::new();
    loop {
        match rels_reader.read_event_into(&mut rels_buf) {
            Ok(Event::Empty(ref e)) | Ok(Event::Start(ref e)) => {
                if e.local_name().as_ref() == b"Relationship" {
                    let mut rid = None;
                    let mut target = None;
                    for attr in e.attributes().flatten() {
                        match attr.key.as_ref() {
                            b"Id" => rid = Some(String::from_utf8_lossy(&attr.value).to_string()),
                            b"Target" => {
                                target = Some(String::from_utf8_lossy(&attr.value).to_string())
                            }
                            _ => {}
                        }
                    }
                    if let (Some(r), Some(t)) = (rid, target) {
                        rid_to_path.insert(r, t);
                    }
                }
            }
            Ok(Event::Eof) => break,
            _ => {}
        }
        rels_buf.clear();
    }

    // Parse workbook: sheet name -> rId
    let mut wb_reader = Reader::from_str(workbook_xml);
    wb_reader.config_mut().trim_text(true);
    let mut wb_buf = Vec::new();
    loop {
        match wb_reader.read_event_into(&mut wb_buf) {
            Ok(Event::Empty(ref e)) | Ok(Event::Start(ref e)) => {
                if e.local_name().as_ref() == b"sheet" {
                    let mut name = None;
                    let mut rid = None;
                    for attr in e.attributes().flatten() {
                        match attr.key.as_ref() {
                            b"name" => {
                                name = Some(String::from_utf8_lossy(&attr.value).to_string())
                            }
                            b"r:id" => rid = Some(String::from_utf8_lossy(&attr.value).to_string()),
                            _ => {}
                        }
                    }
                    if let (Some(n), Some(r)) = (name, rid) {
                        if let Some(path) = rid_to_path.get(&r) {
                            name_to_path.insert(n, format!("xl/{}", path));
                        }
                    }
                }
            }
            Ok(Event::Eof) => break,
            _ => {}
        }
        rels_buf.clear();
    }

    Ok(name_to_path)
}

/// Serialize a single sheet to its worksheet XML. Cells are written inline
/// (numeric as `<v>`, strings as `<is><t>`), and rows are emitted in row-order
/// so the file is consumable by every spreadsheet application.
fn build_workbook_styles(workbook: &XlsxWorkbook) -> (String, Vec<std::collections::HashMap<(usize, usize), usize>>) {
    // CRITICAL: `used_styles` MUST be ordered by `idx` (the cellXfs index).
    // The write path writes cellXfs[1..] in `used_styles` iteration order, and
    // the sheet XML references each cell's style by that same `idx`. If we used
    // a HashMap here the cellXfs order would be randomised and cell styles
    // would land on the wrong cells (e.g. A7 written as #1F3864 read back as
    // the #548235 written to B3). Use an ordered vec with linear lookup for
    // dedup; the style count is small (hundreds at most) so this is fine.
    let mut used_styles: Vec<(SheetStyleKey, usize)> = Vec::new();
    let mut key_to_idx: std::collections::HashMap<SheetStyleKey, usize> = std::collections::HashMap::new();
    let mut next_idx: usize = 1;

    let mut per_sheet: Vec<std::collections::HashMap<(usize, usize), usize>> = Vec::new();
    for sheet in &workbook.sheets {
        let mut sheet_map: std::collections::HashMap<(usize, usize), usize> = std::collections::HashMap::new();
        for cell in &sheet.cells {
            if let Some(style) = &cell.style {
                let key = SheetStyleKey::from(style);
                let idx = if let Some(&i) = key_to_idx.get(&key) {
                    i
                } else {
                    let i = next_idx;
                    next_idx += 1;
                    key_to_idx.insert(key.clone(), i);
                    used_styles.push((key, i));
                    i
                };
                sheet_map.insert((cell.row, cell.col), idx);
            }
        }
        per_sheet.push(sheet_map);
    }

    // `used_styles` is appended in the same order `idx` is assigned, so its
    // iteration order matches the cellXfs order the serializer produces.
    let styles_xml = build_styles_xml(&used_styles);
    (styles_xml, per_sheet)
}

fn build_sheet_xml(sheet: &XlsxSheet, style_map: &std::collections::HashMap<(usize, usize), usize>) -> String {
    // Group cells by row for the row-major layout that xlsx requires.
    let mut by_row: HashMap<usize, Vec<&Cell>> = HashMap::new();
    for cell in &sheet.cells {
        by_row.entry(cell.row).or_default().push(cell);
    }
    let mut row_indices: Vec<usize> = by_row.keys().copied().collect();
    row_indices.sort();

    // Compute the dimension (A1-style) covering the populated cells. We need
    // this for Excel/LibreOffice — they expect <dimension ref="..."/> near
    // the top of the worksheet. If the sheet is empty, we still emit "A1".
    let max_row = (sheet.max_row).max(if row_indices.is_empty() { 0 } else { *row_indices.last().unwrap() + 1 });
    let max_col = sheet.max_col.max(1);
    let dim_ref = if sheet.cells.is_empty() {
        "A1".to_string()
    } else {
        format!("A1:{}", cell_address(max_row.saturating_sub(1), max_col.saturating_sub(1)))
    };

    if DEBUG_XLSX {
        eprintln!("[xlsx] create_xlsx_workbook: sheet={}, cells={}, merged={}", sheet.name, sheet.cells.len(), sheet.merged_cells.len());
        eprintln!("[xlsx] create_xlsx_workbook: row_heights={:?}", sheet.row_heights);
        eprintln!("[xlsx] create_xlsx_workbook: col_widths={:?}", sheet.col_widths);
    }

    let mut xml = String::from(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">"#,
    );
    xml.push_str(&format!("<dimension ref=\"{}\"/>", dim_ref));
    xml.push_str("<sheetViews><sheetView workbookViewId=\"0\"><selection activeCell=\"A1\" sqref=\"A1\"/></sheetView></sheetViews>");

    // Add column definitions if we have custom widths
    if !sheet.col_widths.is_empty() {
        xml.push_str("<cols>");
        for (col_idx, width) in &sheet.col_widths {
            xml.push_str(&format!("<col min=\"{}\" max=\"{}\" width=\"{}\" customWidth=\"1\"/>",
                col_idx + 1, col_idx + 1, width));
        }
        xml.push_str("</cols>");
    } else {
        xml.push_str("<sheetFormatPr baseColWidth=\"8\" defaultRowHeight=\"15\"/>");
    }

    xml.push_str("<sheetData>");

    for row in &row_indices {
        let mut cells = by_row.remove(row).unwrap_or_default();
        cells.sort_by_key(|c| c.col);

        // Check if this row has a custom height
        let row_height = sheet.row_heights.get(row);
        if row_height.is_some() || !cells.is_empty() {
            let ht_attr = row_height.map(|h| format!(" ht=\"{}\"", h)).unwrap_or_default();
            let custom_attr = if row_height.is_some() { " customHeight=\"1\"" } else { "" };
            xml.push_str(&format!("<row r=\"{}\"{}{}>", row + 1, ht_attr, custom_attr));
        }

        for cell in &cells {
            let style_index = style_map.get(&(cell.row, cell.col)).copied().unwrap_or(0);
            xml.push_str(&build_cell_xml(cell, style_index));
        }

        if row_height.is_some() || !cells.is_empty() {
            xml.push_str("</row>");
        }
    }
    xml.push_str("</sheetData>");

    if !sheet.merged_cells.is_empty() {
        xml.push_str(&format!("<mergeCells count=\"{}\">", sheet.merged_cells.len()));
        for m in &sheet.merged_cells {
            xml.push_str(&format!("<mergeCell ref=\"{}\"/>", m.address()));
        }
        xml.push_str("</mergeCells>");
    }

    // <pageMargins> is required; readers complain if it's missing. We use the
    // standard 0.75/0.75/1/1/0.5/0.5 defaults.
    xml.push_str("<pageMargins left=\"0.75\" right=\"0.75\" top=\"1\" bottom=\"1\" header=\"0.5\" footer=\"0.5\"/>");
    xml.push_str("</worksheet>");
    xml
}

fn build_cell_xml(cell: &Cell, style_index: usize) -> String {
    let addr = cell.address();
    let mut attrs = format!("r=\"{}\"", addr);

    // Style index.
    attrs.push_str(" s=\"");
    attrs.push_str(&style_index.to_string());
    attrs.push('"');

    // Build the inner body (everything between <c ...> and </c>). The body
    // may be empty for a self-closing <c .../> placeholder.
    let (body, self_closing) = match (&cell.formula, &cell.value) {
        (Some(f), _) => {
            // Formula present — write <f> and, if there's a cached value, a <v>.
            let f_xml = format!("<f>{}</f>", escape_xml_text(f));
            let v_xml = match cell.value {
                CellValue::Empty => String::new(),
                CellValue::Int(n) => format!("<v>{}</v>", n),
                CellValue::Float(f) => format!("<v>{}</v>", f),
                CellValue::Bool(b) => {
                    attrs.push_str(" t=\"b\"");
                    format!("<v>{}</v>", if b { 1 } else { 0 })
                }
                CellValue::String(ref s) => {
                    // Cached string result of a formula: use t="str" and put the
                    // text directly in <v>.
                    attrs.push_str(" t=\"str\"");
                    format!("<v>{}</v>", escape_xml_text(s))
                }
                CellValue::Error(ref e) => {
                    attrs.push_str(" t=\"e\"");
                    format!("<v>{}</v>", escape_xml_text(e))
                }
                CellValue::DateTime(dt) => format!("<v>{}</v>", dt),
            };
            (format!("{}{}", f_xml, v_xml), false)
        }
        (None, CellValue::Empty) => {
            // No formula and no value — emit a self-closing placeholder.
            return format!("<c {}/>", attrs);
        }
        (None, CellValue::Int(n)) => (format!("<v>{}</v>", n), false),
        (None, CellValue::Float(f)) => {
            let v = if f.is_finite() { f.to_string() } else { "0".to_string() };
            (format!("<v>{}</v>", v), false)
        }
        (None, CellValue::Bool(b)) => {
            attrs.push_str(" t=\"b\"");
            let v = if *b { 1 } else { 0 };
            (format!("<v>{}</v>", v), false)
        }
        (None, CellValue::String(s)) => {
            attrs.push_str(" t=\"inlineStr\"");
            (format!("<is><t>{}</t></is>", escape_xml_text(&s)), false)
        }
        (None, CellValue::Error(e)) => {
            attrs.push_str(" t=\"e\"");
            (format!("<v>{}</v>", escape_xml_text(&e)), false)
        }
        (None, CellValue::DateTime(dt)) => (format!("<v>{}</v>", dt), false),
    };

    if self_closing {
        format!("<c {}/>", attrs)
    } else {
        format!("<c {}>{}</c>", attrs, body)
    }
}

fn escape_xml_attr(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

