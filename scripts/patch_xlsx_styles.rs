use std::io::{Read, Write, Seek};
use zip::{ZipArchive, ZipWriter, write::SimpleFileOptions, CompressionMethod};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args().nth(1).expect("usage: patch_xlsx_styles <file>");
    let data = std::fs::read(&path)?;
    let mut out = Vec::new();
    let mut src = std::io::Cursor::new(data);
    {
        let mut archive = ZipArchive::new(src)?;
        for i in 0..archive.len() {
            let mut entry = archive.by_index(i)?;
            let name = entry.name().to_string();
            let mut buf = Vec::new();
            entry.read_to_end(&mut buf)?;
            if name == "xl/worksheets/sheet1.xml" {
                let s = String::from_utf8_lossy(&buf);
                let patched = s.replace("<c r=\"A1\" s=\"0\"/>", "<c r=\"A1\" s=\"1\"/>");
                out.extend_from_slice(patched.as_bytes());
            } else if name == "xl/styles.xml" {
                let patched = br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">
<numFmts count="0"/>
<fonts count="2"><font><name val="Calibri"/><family val="2"/><color theme="1"/><sz val="11"/><scheme val="minor"/></font><font><name val="Calibri"/><family val="2"/><color rgb="FFFF0000"/><b/><sz val="11"/><scheme val="minor"/></font></fonts>
<fills count="3"><fill><patternFill/></fill><fill><patternFill patternType="gray125"/></fill><fill><patternFill patternType="solid"><fgColor rgb="FFFF00"/><bgColor rgb="FFFF00"/></patternFill></fill></fills>
<borders count="1"><border><left/><right/><top/><bottom/><diagonal/></border></borders>
<cellStyleXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/></cellStyleXfs>
<cellXfs count="2"><xf numFmtId="0" fontId="0" fillId="0" borderId="0" xfId="0"/><xf numFmtId="0" fontId="1" fillId="2" borderId="0" xfId="0" applyFont="1" applyFill="1"/></cellXfs>
<cellStyles count="1"><cellStyle name="Normal" xfId="0" builtinId="0" hidden="0"/></cellStyles>
<dxfs count="0"/>
<tableStyles count="0" defaultTableStyle="TableStyleMedium9" defaultPivotStyle="PivotStyleLight16"/>
</styleSheet>"#;
                out.extend_from_slice(patched);
            } else {
                out.extend_from_slice(&buf);
            }
        }
    }
    let mut file = std::fs::File::create(&path)?;
    file.write_all(&out)?;
    Ok(())
}
