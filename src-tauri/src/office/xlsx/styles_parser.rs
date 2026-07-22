//! Streaming parser for the `styles.xml` part of an XLSX workbook.
//!
//! The styles parser used to live inside the 3 400-line `xlsx/mod.rs` and
//! brought with it ~500 lines of intermediate-state types
//! (`CellXf` / `AlignmentXf` / `FontXf` / `FillXf` / `StylesInfo`). Splitting
//! them into one file keeps `mod.rs` focused on the public API and the
//! zip-driven I/O surfaces.
//!
//! Public surface (all `pub(crate)` because only `mod.rs` consumes them):
//! - [`parse_styles`] — turns `styles.xml` into a [`StylesInfo`] record.
//! - [`resolve_number_format`] — number-format-id → Excel-style format string.
//! - [`CellXf`] / [`AlignmentXf`] / [`FontXf`] / [`FillXf`] / [`StylesInfo`]
//!   — the intermediate types surfaced only by the parser / consumed by
//!   the sheet parser.

use std::collections::HashMap;

use quick_xml::events::{BytesStart, Event};
use quick_xml::reader::Reader as XmlReader;

use crate::office::xlsx::CellStyle;

pub(crate) fn parse_styles(xml: &str) -> StylesInfo {
    let mut reader = XmlReader::from_str(xml);
    reader.config_mut().trim_text(false);
    let mut buf = Vec::new();
    let mut num_formats: HashMap<u32, String> = HashMap::new();
    let mut cell_xfs: Vec<CellXf> = Vec::new();
    let mut fonts: Vec<FontXf> = Vec::new();
    let mut fills: Vec<FillXf> = Vec::new();

    let mut in_num_fmts = false;
    let mut in_cell_xfs = false;
    let mut in_fonts = false;
    let mut in_fills = false;
    let mut in_font = false;
    let mut in_fill = false;
    let mut current_cell_xf = CellXf::default();
    let mut current_font = FontXf::default();
    let mut current_fill = FillXf::default();
    let mut current_align: Option<AlignmentXf> = None;

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                let name = e.local_name();
                match name.as_ref() {
                    b"numFmts" => in_num_fmts = true,
                    b"cellXfs" => in_cell_xfs = true,
                    b"fonts" => in_fonts = true,
                    b"fills" => in_fills = true,
                    b"font" if in_fonts => {
                        current_font = FontXf::default();
                        in_font = true;
                    }
                    b"fill" if in_fills => {
                        current_fill = FillXf::default();
                        in_fill = true;
                    }
                    b"alignment" => {
                        let mut a = AlignmentXf::default();
                        for attr in e.attributes().with_checks(false).flatten() {
                            let v = attr.value.as_ref();
                            if let Ok(s) = std::str::from_utf8(v) {
                                let key = attr.key.as_ref();
                                let local = strip_xml_ns(key);
                                match local {
                                    b"horizontal" => a.horizontal = Some(s.to_string()),
                                    b"vertical" => a.vertical = Some(s.to_string()),
                                    b"wrapText" => a.wrap_text = s.as_bytes() == b"1" || s.as_bytes() == b"true",
                                    _ => {}
                                }
                            }
                        }
                        current_align = Some(a);
                    }
                    _ => {}
                }
                if in_num_fmts && name.as_ref() == b"numFmt" {
                    let mut id: u32 = 0;
                    let mut code = String::new();
                    for attr in e.attributes().with_checks(false).flatten() {
                        let v = attr.value.as_ref();
                        if let Ok(s) = std::str::from_utf8(v) {
                            let key = attr.key.as_ref();
                            let local = strip_xml_ns(key);
                            match local {
                                b"numFmtId" => {
                                    id = s.parse().unwrap_or(0);
                                }
                                b"formatCode" => {
                                    code = s.to_string();
                                }
                                _ => {}
                            }
                        }
                    }
                    num_formats.insert(id, code);
                }
                if in_cell_xfs && name.as_ref() == b"xf" {
                    current_cell_xf = CellXf::default();
                    for attr in e.attributes().with_checks(false).flatten() {
                        let v = attr.value.as_ref();
                        if let Ok(s) = std::str::from_utf8(v) {
                            let key = attr.key.as_ref();
                            let local = strip_xml_ns(key);
                            match local {
                                b"numFmtId" => current_cell_xf.num_fmt_id = s.parse().unwrap_or(0),
                                b"fontId" => current_cell_xf.font_id = s.parse().unwrap_or(0),
                                b"fillId" => current_cell_xf.fill_id = s.parse().unwrap_or(0),
                                b"borderId" => current_cell_xf.border_id = s.parse().unwrap_or(0),
                                b"applyNumberFormat" => {
                                    let bytes = s.as_bytes();
                                    if bytes == b"0" || bytes == b"false" {
                                        current_cell_xf.apply_number_format = false;
                                    }
                                }
                                b"applyFont" => {
                                    let bytes = s.as_bytes();
                                    if bytes == b"0" || bytes == b"false" {
                                        current_cell_xf.apply_font = false;
                                    }
                                }
                                b"applyFill" => {
                                    let bytes = s.as_bytes();
                                    if bytes == b"0" || bytes == b"false" {
                                        current_cell_xf.apply_fill = false;
                                    }
                                }
                                b"applyBorder" => {
                                    let bytes = s.as_bytes();
                                    if bytes == b"0" || bytes == b"false" {
                                        current_cell_xf.apply_border = false;
                                    }
                                }
                                b"applyAlignment" => {
                                    let bytes = s.as_bytes();
                                    if bytes == b"0" || bytes == b"false" {
                                        current_cell_xf.apply_alignment = false;
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                }
                if in_font && name.as_ref() == b"sz" {
                    if let Some(v) = attr_value(e, b"val") {
                        if let Ok(s) = std::str::from_utf8(&v) {
                            current_font.size = s.parse().ok();
                        }
                    }
                }
                if in_font && name.as_ref() == b"color" {
                    if let Some(v) = attr_value(e, b"rgb") {
                        if let Ok(s) = std::str::from_utf8(&v) {
                            current_font.color = Some(s.trim_start_matches('#').to_ascii_uppercase());
                        }
                    }
                }
                if in_font && name.as_ref() == b"name" {
                    if let Some(v) = attr_value(e, b"val") {
                        if let Ok(s) = std::str::from_utf8(&v) {
                            current_font.name = Some(s.to_string());
                        }
                    }
                }
                if in_font && name.as_ref() == b"b" {
                    current_font.bold = true;
                }
                if in_font && name.as_ref() == b"i" {
                    current_font.italic = true;
                }
                if in_fill && name.as_ref() == b"patternFill" {
                    if let Some(v) = attr_value(e, b"patternType") {
                        if let Ok(s) = std::str::from_utf8(&v) {
                            current_fill.pattern_type = Some(s.to_string());
                        }
                    }
                }
                if in_fill && name.as_ref() == b"fgColor" {
                    if let Some(v) = attr_value(e, b"rgb") {
                        if let Ok(s) = std::str::from_utf8(&v) {
                            current_fill.fg_color = Some(s.trim_start_matches('#').to_ascii_uppercase());
                        }
                    }
                }
                if in_fill && name.as_ref() == b"bgColor" {
                    if let Some(v) = attr_value(e, b"rgb") {
                        if let Ok(s) = std::str::from_utf8(&v) {
                            current_fill.bg_color = Some(s.trim_start_matches('#').to_ascii_uppercase());
                        }
                    }
                }
            }
            // FIX: Handle Empty events (self-closing tags like <b/>, <i/>, <sz val="11"/>)
            Ok(Event::Empty(ref e)) => {
                let name = e.local_name();
                // Container elements - set flags
                match name.as_ref() {
                    b"numFmts" => in_num_fmts = true,
                    b"cellXfs" => in_cell_xfs = true,
                    b"fonts" => in_fonts = true,
                    b"fills" => in_fills = true,
                    _ => {}
                }
                // numFmt within numFmts
                if in_num_fmts && name.as_ref() == b"numFmt" {
                    let mut id: u32 = 0;
                    let mut code = String::new();
                    for attr in e.attributes().with_checks(false).flatten() {
                        let v = attr.value.as_ref();
                        if let Ok(s) = std::str::from_utf8(v) {
                            let key = attr.key.as_ref();
                            let local = strip_xml_ns(key);
                            match local {
                                b"numFmtId" => { id = s.parse().unwrap_or(0); }
                                b"formatCode" => { code = s.to_string(); }
                                _ => {}
                            }
                        }
                    }
                    num_formats.insert(id, code);
                }
                // xf within cellXfs
                if in_cell_xfs && name.as_ref() == b"xf" {
                    current_cell_xf = CellXf::default();
                    for attr in e.attributes().with_checks(false).flatten() {
                        let v = attr.value.as_ref();
                        if let Ok(s) = std::str::from_utf8(v) {
                            let key = attr.key.as_ref();
                            let local = strip_xml_ns(key);
                            match local {
                                b"numFmtId" => current_cell_xf.num_fmt_id = s.parse().unwrap_or(0),
                                b"fontId" => current_cell_xf.font_id = s.parse().unwrap_or(0),
                                b"fillId" => current_cell_xf.fill_id = s.parse().unwrap_or(0),
                                b"borderId" => current_cell_xf.border_id = s.parse().unwrap_or(0),
                                b"applyNumberFormat" => {
                                    let bytes = s.as_bytes();
                                    if bytes == b"0" || bytes == b"false" {
                                        current_cell_xf.apply_number_format = false;
                                    }
                                }
                                b"applyFont" => {
                                    let bytes = s.as_bytes();
                                    if bytes == b"0" || bytes == b"false" {
                                        current_cell_xf.apply_font = false;
                                    }
                                }
                                b"applyFill" => {
                                    let bytes = s.as_bytes();
                                    if bytes == b"0" || bytes == b"false" {
                                        current_cell_xf.apply_fill = false;
                                    }
                                }
                                b"applyBorder" => {
                                    let bytes = s.as_bytes();
                                    if bytes == b"0" || bytes == b"false" {
                                        current_cell_xf.apply_border = false;
                                    }
                                }
                                b"applyAlignment" => {
                                    let bytes = s.as_bytes();
                                    if bytes == b"0" || bytes == b"false" {
                                        current_cell_xf.apply_alignment = false;
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                    // For Empty xf, immediately push
                    if let Some(a) = current_align.take() {
                        current_cell_xf.alignment = Some(a);
                    }
                    cell_xfs.push(current_cell_xf.clone());
                }
                // Font child elements within a font
                if in_font {
                    match name.as_ref() {
                        b"sz" => {
                            if let Some(v) = attr_value(e, b"val") {
                                if let Ok(s) = std::str::from_utf8(&v) {
                                    current_font.size = s.parse().ok();
                                }
                            }
                        }
                        b"color" => {
                            if let Some(v) = attr_value(e, b"rgb") {
                                if let Ok(s) = std::str::from_utf8(&v) {
                                    current_font.color = Some(s.trim_start_matches('#').to_ascii_uppercase());
                                }
                            }
                        }
                        b"name" => {
                            if let Some(v) = attr_value(e, b"val") {
                                if let Ok(s) = std::str::from_utf8(&v) {
                                    current_font.name = Some(s.to_string());
                                }
                            }
                        }
                        b"b" => { current_font.bold = true; }
                        b"i" => { current_font.italic = true; }
                        _ => {}
                    }
                }
                // Fill child elements within a fill
                if in_fill {
                    match name.as_ref() {
                        b"patternFill" => {
                            if let Some(v) = attr_value(e, b"patternType") {
                                if let Ok(s) = std::str::from_utf8(&v) {
                                    current_fill.pattern_type = Some(s.to_string());
                                }
                            }
                        }
                        b"fgColor" => {
                            if let Some(v) = attr_value(e, b"rgb") {
                                if let Ok(s) = std::str::from_utf8(&v) {
                                    current_fill.fg_color = Some(s.trim_start_matches('#').to_ascii_uppercase());
                                }
                            }
                        }
                        b"bgColor" => {
                            if let Some(v) = attr_value(e, b"rgb") {
                                if let Ok(s) = std::str::from_utf8(&v) {
                                    current_fill.bg_color = Some(s.trim_start_matches('#').to_ascii_uppercase());
                                }
                            }
                        }
                        _ => {}
                    }
                }
                // For Empty font element, immediately push
                if name.as_ref() == b"font" && in_fonts {
                    fonts.push(current_font.clone());
                    current_font = FontXf::default();
                }
                // For Empty fill element, immediately push
                if name.as_ref() == b"fill" && in_fills {
                    fills.push(current_fill.clone());
                    current_fill = FillXf::default();
                }
            }
            Ok(Event::End(ref e)) => {
                let name = e.local_name();
                match name.as_ref() {
                    b"numFmts" => in_num_fmts = false,
                    b"cellXfs" => in_cell_xfs = false,
                    b"fonts" => in_fonts = false,
                    b"fills" => in_fills = false,
                    b"font" if in_font => {
                        fonts.push(current_font.clone());
                        in_font = false;
                    }
                    b"fill" if in_fill => {
                        fills.push(current_fill.clone());
                        in_fill = false;
                    }
                    b"xf" if in_cell_xfs => {
                        if let Some(a) = current_align.take() {
                            current_cell_xf.alignment = Some(a);
                        }
                        cell_xfs.push(current_cell_xf.clone());
                    }
                    _ => {}
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
        buf.clear();
    }

    StylesInfo {
        num_formats,
        cell_xfs,
        fonts,
        fills,
    }
}

pub(crate) fn strip_xml_ns(key: &[u8]) -> &[u8] {
    match key.iter().position(|&b| b == b':') {
        Some(i) => &key[i + 1..],
        None => key,
    }
}

pub(crate) fn attr_value(e: &BytesStart, name: &[u8]) -> Option<Vec<u8>> {
    for attr in e.attributes().with_checks(false).flatten() {
        let key = attr.key.as_ref();
        let local = strip_xml_ns(key);
        if local == name {
            return Some(attr.value.into_owned());
        }
    }
    None
}

pub(crate) fn resolve_number_format(id: u32, custom: &HashMap<u32, String>) -> String {
    if let Some(s) = custom.get(&id) {
        return s.clone();
    }
    match id {
        0 => "General".to_string(),
        1 => "0".to_string(),
        2 => "0.00".to_string(),
        3 => "#,##0".to_string(),
        4 => "#,##0.00".to_string(),
        9 => "0%".to_string(),
        10 => "0.00%".to_string(),
        11 => "0.00E+00".to_string(),
        14 => "m/d/yyyy".to_string(),
        22 => "m/d/yyyy h:mm".to_string(),
        49 => "@".to_string(),
        _ => "General".to_string(),
    }
}

#[derive(Default, Clone, Debug)]
pub(crate) struct CellXf {
    num_fmt_id: u32,
    font_id: u32,
    fill_id: u32,
    border_id: u32,
    apply_number_format: bool,
    apply_font: bool,
    apply_fill: bool,
    apply_border: bool,
    apply_alignment: bool,
    alignment: Option<AlignmentXf>,
}

#[derive(Default, Clone, Debug)]
pub(crate) struct AlignmentXf {
    horizontal: Option<String>,
    vertical: Option<String>,
    wrap_text: bool,
}

#[derive(Default, Clone, Debug)]
pub(crate) struct FontXf {
    size: Option<u32>,
    color: Option<String>,
    name: Option<String>,
    bold: bool,
    italic: bool,
}

#[derive(Default, Clone, Debug)]
pub(crate) struct FillXf {
    pattern_type: Option<String>,
    fg_color: Option<String>,
    bg_color: Option<String>,
}

pub(crate) struct StylesInfo {
    pub(crate) num_formats: HashMap<u32, String>,
    pub(crate) cell_xfs: Vec<CellXf>,
    pub(crate) fonts: Vec<FontXf>,
    pub(crate) fills: Vec<FillXf>,
}

impl StylesInfo {
    pub(crate) fn resolve_style(&self, xf_index: usize) -> Option<CellStyle> {
        let xf = self.cell_xfs.get(xf_index)?;
        let mut style = CellStyle::default();

        // FIX: Per Excel spec, apply* attributes default to true when absent.
        // We invert the logic: if the attribute is explicitly "0" or "false",
        // skip that aspect; otherwise apply it.
        // Since we only set apply* to false when explicitly parsed as such,
        // and the parser leaves them true by default for non-trivial xfs,
        // we treat apply_*=true as the expected case.
        if xf.apply_number_format {
            style.number_format = resolve_number_format(xf.num_fmt_id, &self.num_formats);
        } else {
            style.number_format = "General".to_string();
        }

        // Apply font whenever font_id points to a valid font
        if xf.font_id < self.fonts.len() as u32 {
            if let Some(font) = self.fonts.get(xf.font_id as usize) {
                style.font_bold = font.bold;
                style.font_italic = font.italic;
                style.font_color = font.color.clone();
                // OOXML SpreadsheetML's `sz@val` is in points (a double, not 1/100 pt).
                // Keep the parsed value as-is so the writer can emit it back unchanged.
                style.font_size = font.size;
                style.font_name = font.name.clone();
            }
        }

        // Apply fill whenever fill_id points to a non-trivial fill
        if xf.fill_id < self.fills.len() as u32 {
            if let Some(fill) = self.fills.get(xf.fill_id as usize) {
                let has_pattern = fill
                    .pattern_type
                    .as_ref()
                    .map(|p| p != "none" && !p.is_empty())
                    .unwrap_or(false);
                if has_pattern {
                    style.fill_fg_color = fill.fg_color.clone();
                    style.fill_bg_color = fill.bg_color.clone();
                }
            }
        }

        if let Some(ref a) = xf.alignment {
            style.alignment_h = a.horizontal.clone();
            style.alignment_v = a.vertical.clone();
        }

        Some(style)
    }
}

