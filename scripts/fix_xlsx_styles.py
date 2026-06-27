#!/usr/bin/env python3
from pathlib import Path

p = Path('src-tauri/src/office/xlsx.rs')
lines = p.read_text().splitlines()

def line_eq(n, s):
    return lines[n].strip() == s.strip()

def find_line(s, start=0):
    for i in range(start, len(lines)):
        if lines[i].strip() == s.strip():
            return i
    return -1

def find_lines(pattern, start=0):
    """Find all lines matching pattern (substring)."""
    return [i for i in range(start, len(lines)) if pattern in lines[i]]

# === Step 1: Add SheetStyleKey struct + build_styles_xml after impl Cell { pub fn address } ===
# Find "        cell_address(self.row, self.col)"
addr_line = find_line("        cell_address(self.row, self.col)")
print(f"Found cell_address call at line {addr_line+1}")

# The closing brace is on the next line after the address method
# We insert AFTER the closing brace of impl Cell
impl_cell_close = addr_line + 1  # line after "        cell_address..."
while lines[impl_cell_close].strip() != '}':
    impl_cell_close += 1
print(f"impl Cell closes at line {impl_cell_close+1}")

insert_code = [
    "",
    "#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default, PartialEq, Eq, Hash)]",
    "pub struct SheetStyleKey {",
    "    pub number_format: String,",
    "    pub fill_fg_color: Option<String>,",
    "    pub fill_bg_color: Option<String>,",
    "    pub font_bold: bool,",
    "    pub font_italic: bool,",
    "    pub font_color: Option<String>,",
    "    pub font_size: Option<u32>,",
    "    pub font_name: Option<String>,",
    "    pub alignment_h: Option<String>,",
    "    pub alignment_v: Option<String>,",
    "}",
    "",
    "impl From<&CellStyle> for SheetStyleKey {",
    "    fn from(value: &CellStyle) -> Self {",
    "        Self {",
    "            number_format: value.number_format.clone(),",
    "            fill_fg_color: value.fill_fg_color.clone(),",
    "            fill_bg_color: value.fill_bg_color.clone(),",
    "            font_bold: value.font_bold,",
    "            font_italic: value.font_italic,",
    "            font_color: value.font_color.clone(),",
    "            font_size: value.font_size,",
    "            font_name: value.font_name.clone(),",
    "            alignment_h: value.alignment_h.clone(),",
    "            alignment_v: value.alignment_v.clone(),",
    "        }",
    "    }",
    "}",
    "",
    "fn build_styles_xml(used_styles: &std::collections::HashMap<SheetStyleKey, usize>) -> String {",
    "    let mut num_fmts: std::collections::BTreeMap<String, u32> = std::collections::BTreeMap::new();",
    "    let mut next_num_fmt = 164u32;",
    "    let mut fonts: Vec<(CellStyle, usize)> = Vec::new();",
    "    let mut font_index: std::collections::HashMap<Option<String>, usize> = std::collections::HashMap::new();",
    "    let mut fills: Vec<(Option<String>, Option<String>, usize)> = Vec::new();",
    "    let mut fill_index: std::collections::HashMap<(Option<String>, Option<String>), usize> = std::collections::HashMap::new();",
    "",
    "    let _default_font_idx = *font_index.entry(None).or_insert_with(|| {",
    "        let idx = fonts.len();",
    "        fonts.push((CellStyle::default(), idx));",
    "        idx",
    "    });",
    "    let _default_fill_idx = *fill_index.entry((None, None)).or_insert_with(|| {",
    "        let idx = fills.len();",
    "        fills.push((None, None, idx));",
    "        idx",
    "    });",
    "    let default_num_fmt_idx = *num_fmts.entry(String::new()).or_insert(0);",
    "",
    "    let mut xfs: Vec<(usize, usize, bool, bool)> = Vec::new();",
    "",
    "    for (key, _) in used_styles.iter() {",
    "        let font_idx = *font_index.entry(key.font_name.clone()).or_insert_with(|| {",
    "            let idx = fonts.len();",
    "            fonts.push((key.clone(), idx));",
    "            idx",
    "        });",
    "        let fill_idx = *fill_index.entry((key.fill_fg_color.clone(), key.fill_bg_color.clone())).or_insert_with(|| {",
    "            let idx = fills.len();",
    "            fills.push((key.fill_fg_color.clone(), key.fill_bg_color.clone(), idx));",
    "            idx",
    "        });",
    "        if !key.number_format.is_empty() {",
    "            num_fmts.entry(key.number_format.clone()).or_insert_with(|| {",
    "                let id = next_num_fmt;",
    "                next_num_fmt += 1;",
    "                id",
    "            });",
    "        }",
    "        xfs.push((font_idx, fill_idx, key.font_bold, key.font_italic));",
    "    }",
    "",
    "    let mut xml = String::from(r#\"<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\"#);",
    "    xml.push_str(\"\\n<styleSheet xmlns=\\\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\\\">\\n\");",
    "    if !num_fmts.is_empty() {",
    "        xml.push_str(\"<numFmts count=\\\"\");",
    "        xml.push_str(&num_fmts.len().to_string());",
    "        xml.push_str(\"\\\">\");",
    "        for (fmt, id) in &num_fmts {",
    "            xml.push_str(\"<numFmt numFmtId=\\\"\");",
    "            xml.push_str(&id.to_string());",
    "            xml.push_str(\"\\\" formatCode=\\\"\");",
    "            xml.push_str(&escape_xml_attr(fmt));",
    "            xml.push_str(\"\\\"/>\");",
    "        }",
    "        xml.push_str(\"</numFmts>\\n\");",
    "    } else {",
    "        xml.push_str(\"<numFmts count=\\\"0\\\"/>\\n\");",
    "    }",
    "",
    "    xml.push_str(\"<fonts count=\\\"\");",
    "    xml.push_str(&(fonts.len() + 1).to_string());",
    "    xml.push_str(\"\\\">\");",
    "    xml.push_str(\"<font><name val=\\\"Calibri\\\"/><family val=\\\"2\\\"/><color theme=\\\"1\\\"/><sz val=\\\"11\\\"/><scheme val=\\\"minor\\\"/></font>\");",
    "    for (style, _) in &fonts {",
    "        xml.push_str(\"<font>\");",
    "        xml.push_str(\"<name val=\\\"\");",
    "        xml.push_str(&escape_xml_attr(style.font_name.as_deref().unwrap_or(\"Calibri\")));",
    "        xml.push_str(\"\\\"/>\");",
    "        xml.push_str(\"<family val=\\\"2\\\"/>\");",
    "        if let Some(color) = &style.font_color {",
    "            xml.push_str(\"<color rgb=\\\"\");",
    "            xml.push_str(&escape_xml_attr(color));",
    "            xml.push_str(\"\\\"/>\");",
    "        } else {",
    "            xml.push_str(\"<color theme=\\\"1\\\"/>\");",
    "        }",
    "        xml.push_str(\"<sz val=\\\"\");",
    "        xml.push_str(&style.font_size.unwrap_or(11).to_string());",
    "        xml.push_str(\"\\\"/>\");",
    "        if style.font_bold { xml.push_str(\"<b/>\"); }",
    "        if style.font_italic { xml.push_str(\"<i/>\"); }",
    "        xml.push_str(\"<scheme val=\\\"minor\\\"/>\");",
    "        xml.push_str(\"</font>\");",
    "    }",
    "    xml.push_str(\"</fonts>\\n\");",
    "",
    "    xml.push_str(\"<fills count=\\\"\");",
    "    xml.push_str(&(fills.len() + 2).to_string());",
    "    xml.push_str(\"\\\">\");",
    "    xml.push_str(\"<fill><patternFill/></fill>\");",
    "    xml.push_str(\"<fill><patternFill patternType=\\\"gray125\\\"/></fill>\");",
    "    for (fg, bg, _) in &fills {",
    "        xml.push_str(\"<fill><patternFill patternType=\\\"solid\\\">\");",
    "        if let Some(color) = fg {",
    "            xml.push_str(\"<fgColor rgb=\\\"\");",
    "            xml.push_str(&escape_xml_attr(color));",
    "            xml.push_str(\"\\\"/>\");",
    "        }",
    "        if let Some(color) = bg {",
    "            xml.push_str(\"<bgColor rgb=\\\"\");",
    "            xml.push_str(&escape_xml_attr(color));",
    "            xml.push_str(\"\\\"/>\");",
    "        }",
    "        xml.push_str(\"</patternFill></fill>\");",
    "    }",
    "    xml.push_str(\"</fills>\\n\");",
    "",
    "    xml.push_str(\"<borders count=\\\"1\\\"><border><left/><right/><top/><bottom/><diagonal/></border></borders>\\n\");",
    "    xml.push_str(\"<cellStyleXfs count=\\\"1\\\"><xf numFmtId=\\\"0\\\" fontId=\\\"0\\\" fillId=\\\"0\\\" borderId=\\\"0\\\"/></cellStyleXfs>\\n\");",
    "    xml.push_str(\"<cellXfs count=\\\"\");",
    "    xml.push_str(&(xfs.len() + 1).to_string());",
    "    xml.push_str(\"\\\">\");",
    "    xml.push_str(\"<xf numFmtId=\\\"0\\\" fontId=\\\"0\\\" fillId=\\\"0\\\" borderId=\\\"0\\\" pivotButton=\\\"0\\\" quotePrefix=\\\"0\\\" xfId=\\\"0\\\"/>\");",
    "    for (font_idx, fill_idx, bold, italic) in &xfs {",
    "        let mut attrs = format!(\"numFmtId=\\\"0\\\" fontId=\\\"{}\\\" fillId=\\\"{}\\\" borderId=\\\"0\\\" xfId=\\\"0\\\"\", font_idx + 1, fill_idx + 2);",
    "        if *bold || *italic { attrs.push_str(\" applyFont=\\\"1\\\"\"); }",
    "        attrs.push_str(\" applyBorder=\\\"0\\\" applyNumberFormat=\\\"1\\\"\");",
    "        xml.push_str(\"<xf \");",
    "        xml.push_str(&attrs);",
    "        xml.push_str(\"/>\");",
    "    }",
    "    xml.push_str(\"</cellXfs>\\n\");",
    "    xml.push_str(\"<cellStyles count=\\\"1\\\"><cellStyle name=\\\"Normal\\\" xfId=\\\"0\\\" builtinId=\\\"0\\\" hidden=\\\"0\\\"/></cellStyles>\\n\");",
    "    xml.push_str(\"<dxfs count=\\\"0\\\"/>\\n\");",
    "    xml.push_str(r#\"<tableStyles count=\"0\" defaultTableStyle=\"TableStyleMedium9\" defaultPivotStyle=\"PivotStyleLight16\"/>\"#);",
    "    xml.push_str(\"\\n</styleSheet>\");",
    "    xml",
    "}",
]

for i, line in enumerate(insert_code):
    lines.insert(impl_cell_close + 1 + i, line)

# Update line numbers after insertion
offset = len(insert_code)

def rel(n):
    return n + offset

# === Step 2: Fix build_cell_xml signature ===
# After insertion, find build_cell_xml
build_cell_idx = find_line("fn build_cell_xml(cell: &Cell) -> String {")
print(f"build_cell_xml at line {build_cell_idx+1}")

# Find the "s=\"0\"" line
s0_line = build_cell_idx + 1  # next line should be the style attr line
print(f"Line after fn build_cell_xml: {lines[build_cell_idx+1].strip()}")

# === Step 3: Fix create_xlsx_workbook styles.xml write (line ~2560 + offset) ===
create_styles_line = find_line("zip.write_all(MINIMAL_STYLES_XML.as_bytes())?;", find_line("fn create_xlsx_workbook"))
print(f"create_xlsx_workbook styles write at line {create_styles_line+1}")

# === Step 4: Fix write_excel_document styles.xml write (line ~2767 + offset) ===
# Find the write_excel_document block
write_excel_idx = find_line("pub fn write_excel_document(")
print(f"write_excel_document at line {write_excel_idx+1}")

# Find the styles.xml block in write_excel_document (after pub fn, offset)
we_styles_start = find_line("// 8. xl/styles.xml", write_excel_idx)
print(f"write_excel_document styles block starts at line {we_styles_start+1}")

# Count lines in the block
block_end = we_styles_start
while not lines[block_end].strip().startswith('drop(archive2)'):
    block_end += 1
print(f"Block ends at line {block_end+1}")
block_lines = block_end - we_styles_start + 1

# === Step 5: Fix build_sheet_xml ===
sheet_fn_idx = find_line("fn build_sheet_xml(sheet: &XlsxSheet) -> String {", build_cell_idx + 100)
print(f"build_sheet_xml at line {sheet_fn_idx+1}")

p.write_text('\n'.join(lines))
print("Written!")
