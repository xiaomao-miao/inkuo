#!/usr/bin/env python3
from pathlib import Path

path = Path('/home/maomao/work/inkuo/src-tauri/src/office/xlsx.rs')
text = path.read_text()

needle = "impl Cell {\n    pub fn address(&self) -> String {\n        cell_address(self.row, self.col)\n    }\n}"
insert = """impl Cell {
    pub fn address(&self) -> String {
        cell_address(self.row, self.col)
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default, PartialEq, Eq, Hash)]
pub struct SheetStyleKey {
    pub number_format: String,
    pub fill_fg_color: Option<String>,
    pub fill_bg_color: Option<String>,
    pub font_bold: bool,
    pub font_italic: bool,
    pub font_color: Option<String>,
    pub font_size: Option<u32>,
    pub font_name: Option<String>,
    pub alignment_h: Option<String>,
    pub alignment_v: Option<String>,
}

impl From<&CellStyle> for SheetStyleKey {
    fn from(value: &CellStyle) -> Self {
        Self {
            number_format: value.number_format.clone(),
            fill_fg_color: value.fill_fg_color.clone(),
            fill_bg_color: value.fill_bg_color.clone(),
            font_bold: value.font_bold,
            font_italic: value.font_italic,
            font_color: value.font_color.clone(),
            font_size: value.font_size,
            font_name: value.font_name.clone(),
            alignment_h: value.alignment_h.clone(),
            alignment_v: value.alignment_v.clone(),
        }
    }
}

fn build_styles_xml(used_styles: &std::collections::HashMap<SheetStyleKey, usize>) -> String {
    let mut num_fmts: std::collections::BTreeMap<String, u32> = std::collections::BTreeMap::new();
    let mut next_num_fmt = 164u32;
    let mut fonts: Vec<(CellStyle, usize)> = Vec::new();
    let mut font_index: std::collections::HashMap<Option<String>, usize> = std::collections::HashMap::new();
    let mut fills: Vec<(Option<String>, Option<String>, usize)> = Vec::new();
    let mut fill_index: std::collections::HashMap<(Option<String>, Option<String>), usize> = std::collections::HashMap::new();

    let default_font_idx = *font_index.entry(None).or_insert_with(|| {
        let idx = fonts.len();
        fonts.push((CellStyle::default(), idx));
        idx
    });
    let default_fill_idx = *fill_index.entry((None, None)).or_insert_with(|| {
        let idx = fills.len();
        fills.push((None, None, idx));
        idx
    });
    let default_num_fmt_idx = *num_fmts.entry(String::new()).or_insert(0);

    let mut xfs: Vec<(Option<String>, usize, usize, Option<String>, Option<String>, bool, bool, Option<String>, Option<String>)> = Vec::new();

    for (key, _) in used_styles.iter() {
        let font_idx = *font_index.entry(key.font_name.clone()).or_insert_with(|| {
            let idx = fonts.len();
            fonts.push((key.clone(), idx));
            idx
        });
        let fill_idx = *fill_index.entry((key.fill_fg_color.clone(), key.fill_bg_color.clone())).or_insert_with(|| {
            let idx = fills.len();
            fills.push((key.fill_fg_color.clone(), key.fill_bg_color.clone(), idx));
            idx
        });
        let num_fmt_idx = if key.number_format.is_empty() {
            default_num_fmt_idx
        } else {
            *num_fmts.entry(key.number_format.clone()).or_insert_with(|| {
                let id = next_num_fmt;
                next_num_fmt += 1;
                id
            })
        };
        xfs.push((
            key.number_format.clone(),
            font_idx,
            fill_idx,
            key.font_color.clone(),
            key.font_size,
            key.font_bold,
            key.font_italic,
            key.alignment_h.clone(),
            key.alignment_v.clone(),
        ));
    }

    let mut xml = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<styleSheet xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\">\n");
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
        if style.font_bold {
            xml.push_str("<b/>");
        }
        if style.font_italic {
            xml.push_str("<i/>");
        }
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
    for (_, font_idx, fill_idx, _, _, bold, italic, _, _) in &xfs {
        let mut attrs = format!("numFmtId=\"0\" fontId=\"{}\" fillId=\"{}\" borderId=\"0\" xfId=\"0\"", font_idx + 1, fill_idx + 2);
        if *bold || *italic {
            attrs.push_str(" applyFont=\"1\"");
        }
        attrs.push_str(" applyBorder=\"0\" applyNumberFormat=\"1\"");
        xml.push_str("<xf ");
        xml.push_str(&attrs);
        xml.push_str("/>");
    }
    xml.push_str("</cellXfs>\n");
    xml.push_str("<cellStyles count=\"1\"><cellStyle name=\"Normal\" xfId=\"0\" builtinId=\"0\" hidden=\"0\"/></cellStyles>\n");
    xml.push_str("<dxfs count=\"0\"/>\n");
    xml.push_str("<tableStyles count=\"0\" defaultTableStyle=\"TableStyleMedium9\" defaultPivotStyle=\"PivotStyleLight16\"/>\n");
    xml.push_str("</styleSheet>");
    xml
}
"""

if needle not in text:
    raise SystemExit('needle not found')
text = text.replace(needle, insert, 1)
path.write_text(text)
