//! Extended `word/styles.xml` payload with the brand-level styles
//! `components.rs` emits.
//!
//! The original [`crate::office::docx::ooxml_boilerplate::STYLES_XML`]
//! defines a minimal styles.xml suitable for vanilla Word documents.
//! This module **adds** a richer styles.xml that the component layer
//! depends on (cover-title gradient, callout paragraph styles, code-block
//! monospace, table-header padding, etc.).
//!
//! Usage: when the renderer detects the document uses component-built
//! elements, it switches `STYLES_XML` to [`EXTENDED_STYLES_XML`] so
//! every `pStyle` reference resolves.
//!
//! The style ids referenced from `components.rs`:
//!   - `CoverTitle`, `CoverSubtitle`
//!   - `ChapterTitle`, `SectionTitle`, `SubsectionTitle`
//!   - `BodyParagraph`
//!   - `ListBullet`, `ListNumber`
//!   - `CalloutBody`
//!   - `CodeBlock`
//!
//! All of them extend Normal via `<w:basedOn w:val="Normal"/>` so the
//! document looks reasonable even if a particular style is missing.

/// Extended `word/styles.xml`. Defines the brand styles that the
/// component layer relies on. Composed with the base styles.xml —
/// every base style is included verbatim, plus the new ones.
pub const EXTENDED_STYLES_XML: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
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

  <w:style w:type="paragraph" w:default="1" w:styleId="Normal">
    <w:name w:val="Normal"/>
    <w:pPr>
      <w:spacing w:after="120" w:line="293" w:lineRule="auto"/>
    </w:pPr>
    <w:rPr>
      <w:rFonts w:ascii="Calibri" w:hAnsi="Calibri"/>
      <w:sz w:val="20"/>
    </w:rPr>
  </w:style>

  <w:style w:type="character" w:default="1" w:styleId="DefaultParagraphFont">
    <w:name w:val="Default Paragraph Font"/>
    <w:uiPriority w:val="1"/>
    <w:semiHidden/>
    <w:unhideWhenUsed/>
  </w:style>

  <w:style w:type="paragraph" w:styleId="Title">
    <w:name w:val="Title"/>
    <w:basedOn w:val="Normal"/>
    <w:pPr>
      <w:jc w:val="center"/>
      <w:spacing w:after="0" w:before="240"/>
    </w:pPr>
    <w:rPr>
      <w:rFonts w:ascii="Calibri" w:hAnsi="Calibri"/>
      <w:b/>
      <w:sz w:val="56"/>
      <w:szCs w:val="56"/>
      <w:color w:val="1F3864"/>
    </w:rPr>
  </w:style>

  <!-- ─── Brand heading ladder ─────────────────────────────────────────── -->
  <w:style w:type="paragraph" w:styleId="CoverTitle">
    <w:name w:val="Cover Title"/>
    <w:basedOn w:val="Normal"/>
    <w:pPr>
      <w:jc w:val="center"/>
      <w:spacing w:before="2400" w:after="240" w:line="300" w:lineRule="auto"/>
    </w:pPr>
    <w:rPr>
      <w:rFonts w:ascii="Calibri" w:hAnsi="Calibri"/>
      <w:b/>
      <w:sz w:val="68"/>
      <w:szCs w:val="68"/>
      <w:color w:val="213B32"/>
    </w:rPr>
  </w:style>

  <w:style w:type="paragraph" w:styleId="CoverSubtitle">
    <w:name w:val="Cover Subtitle"/>
    <w:basedOn w:val="Normal"/>
    <w:pPr>
      <w:jc w:val="center"/>
      <w:spacing w:before="120" w:after="600"/>
      <w:line w:lineRule="auto" w:line="300"/>
    </w:pPr>
    <w:rPr>
      <w:rFonts w:ascii="Calibri" w:hAnsi="Calibri"/>
      <w:i/>
      <w:sz w:val="28"/>
      <w:szCs w:val="28"/>
      <w:color w:val="6E6E6E"/>
    </w:rPr>
  </w:style>

  <w:style w:type="paragraph" w:styleId="ChapterTitle">
    <w:name w:val="Chapter Title"/>
    <w:basedOn w:val="Normal"/>
    <w:next w:val="BodyParagraph"/>
    <w:pPr>
      <w:keepNext/>
      <w:keepLines/>
      <w:spacing w:before="480" w:after="240" w:line="300" w:lineRule="auto"/>
      <w:outlineLvl w:val="0"/>
    </w:pPr>
    <w:rPr>
      <w:rFonts w:ascii="Calibri" w:hAnsi="Calibri"/>
      <w:b/>
      <w:sz w:val="40"/>
      <w:szCs w:val="40"/>
      <w:color w:val="213B32"/>
    </w:rPr>
  </w:style>

  <w:style w:type="paragraph" w:styleId="SectionTitle">
    <w:name w:val="Section Title"/>
    <w:basedOn w:val="Normal"/>
    <w:next w:val="BodyParagraph"/>
    <w:pPr>
      <w:keepNext/>
      <w:keepLines/>
      <w:spacing w:before="360" w:after="120" w:line="293" w:lineRule="auto"/>
      <w:outlineLvl w:val="1"/>
    </w:pPr>
    <w:rPr>
      <w:rFonts w:ascii="Calibri" w:hAnsi="Calibri"/>
      <w:b/>
      <w:sz w:val="28"/>
      <w:szCs w:val="28"/>
      <w:color w:val="2E7D5B"/>
    </w:rPr>
  </w:style>

  <w:style w:type="paragraph" w:styleId="SubsectionTitle">
    <w:name w:val="Subsection Title"/>
    <w:basedOn w:val="Normal"/>
    <w:next w:val="BodyParagraph"/>
    <w:pPr>
      <w:keepNext/>
      <w:keepLines/>
      <w:spacing w:before="240" w:after="80" w:line="293" w:lineRule="auto"/>
      <w:outlineLvl w:val="2"/>
    </w:pPr>
    <w:rPr>
      <w:rFonts w:ascii="Calibri" w:hAnsi="Calibri"/>
      <w:b/>
      <w:sz w:val="23"/>
      <w:szCs w:val="23"/>
      <w:color w:val="2E7D5B"/>
    </w:rPr>
  </w:style>

  <w:style w:type="paragraph" w:styleId="Heading1">
    <w:name w:val="Heading 1"/>
    <w:basedOn w:val="ChapterTitle"/>
    <w:next w:val="BodyParagraph"/>
    <w:pPr>
      <w:keepNext/>
      <w:keepLines/>
      <w:spacing w:before="480" w:after="120"/>
      <w:outlineLvl w:val="0"/>
    </w:pPr>
    <w:rPr>
      <w:rFonts w:ascii="Calibri" w:hAnsi="Calibri"/>
      <w:b/>
      <w:sz w:val="32"/>
      <w:szCs w:val="32"/>
      <w:color w:val="2E74B5"/>
    </w:rPr>
  </w:style>

  <w:style w:type="paragraph" w:styleId="Heading2">
    <w:name w:val="Heading 2"/>
    <w:basedOn w:val="SectionTitle"/>
    <w:next w:val="BodyParagraph"/>
    <w:pPr>
      <w:keepNext/>
      <w:keepLines/>
      <w:spacing w:before="360" w:after="80"/>
      <w:outlineLvl w:val="1"/>
    </w:pPr>
    <w:rPr>
      <w:rFonts w:ascii="Calibri" w:hAnsi="Calibri"/>
      <w:b/>
      <w:sz w:val="26"/>
      <w:szCs w:val="26"/>
      <w:color w:val="2F5496"/>
    </w:rPr>
  </w:style>

  <w:style w:type="paragraph" w:styleId="Heading3">
    <w:name w:val="Heading 3"/>
    <w:basedOn w:val="SubsectionTitle"/>
    <w:next w:val="BodyParagraph"/>
    <w:pPr>
      <w:keepNext/>
      <w:keepLines/>
      <w:spacing w:before="240" w:after="60"/>
      <w:outlineLvl w:val="2"/>
    </w:pPr>
    <w:rPr>
      <w:rFonts w:ascii="Calibri" w:hAnsi="Calibri"/>
      <w:b/>
      <w:sz w:val="24"/>
      <w:szCs w:val="24"/>
      <w:color w:val="1F497D"/>
    </w:rPr>
  </w:style>

  <!-- ─── Body / list ─────────────────────────────────────────────────── -->
  <w:style w:type="paragraph" w:styleId="BodyParagraph">
    <w:name w:val="Body Paragraph"/>
    <w:basedOn w:val="Normal"/>
    <w:pPr>
      <w:spacing w:after="120" w:line="293" w:lineRule="auto"/>
      <w:jc w:val="both"/>
    </w:pPr>
    <w:rPr>
      <w:rFonts w:ascii="Calibri" w:hAnsi="Calibri"/>
      <w:sz w:val="20"/>
      <w:szCs w:val="20"/>
      <w:color w:val="2A2A2A"/>
    </w:rPr>
  </w:style>

  <w:style w:type="paragraph" w:styleId="ListBullet">
    <w:name w:val="List Bullet"/>
    <w:basedOn w:val="BodyParagraph"/>
    <w:pPr>
      <w:spacing w:after="80" w:line="293" w:lineRule="auto"/>
      <w:contextualSpacing/>
    </w:pPr>
  </w:style>

  <w:style w:type="paragraph" w:styleId="ListNumber">
    <w:name w:val="List Number"/>
    <w:basedOn w:val="BodyParagraph"/>
    <w:pPr>
      <w:spacing w:after="80" w:line="293" w:lineRule="auto"/>
      <w:contextualSpacing/>
    </w:pPr>
  </w:style>

  <!-- ─── Callout ─────────────────────────────────────────────────────── -->
  <w:style w:type="paragraph" w:styleId="CalloutBody">
    <w:name w:val="Callout Body"/>
    <w:basedOn w:val="Normal"/>
    <w:pPr>
      <w:spacing w:before="0" w:after="60" w:line="293" w:lineRule="auto"/>
    </w:pPr>
    <w:rPr>
      <w:rFonts w:ascii="Calibri" w:hAnsi="Calibri"/>
      <w:sz w:val="20"/>
      <w:szCs w:val="20"/>
      <w:color w:val="2A2A2A"/>
    </w:rPr>
  </w:style>

  <!-- ─── Code block ──────────────────────────────────────────────────── -->
  <w:style w:type="paragraph" w:styleId="CodeBlock">
    <w:name w:val="Code Block"/>
    <w:basedOn w:val="Normal"/>
    <w:pPr>
      <w:spacing w:before="0" w:after="0" w:line="252" w:lineRule="auto"/>
      <w:shd w:val="clear" w:color="auto" w:fill="F4F1EC"/>
    </w:pPr>
    <w:rPr>
      <w:rFonts w:ascii="Consolas" w:hAnsi="Consolas" w:cs="Consolas"/>
      <w:sz w:val="18"/>
      <w:szCs w:val="18"/>
      <w:color w:val="2A2A2A"/>
    </w:rPr>
  </w:style>

  <!-- ─── Tables ──────────────────────────────────────────────────────── -->
  <w:style w:type="table" w:default="1" w:styleId="TableNormal">
    <w:name w:val="Normal Table"/>
    <w:uiPriority w:val="99"/>
    <w:semiHidden/>
    <w:unhideWhenUsed/>
    <w:tblPr>
      <w:tblInd w:w="0" w:type="dxa"/>
      <w:tblCellMar>
        <w:top w:w="80" w:type="dxa"/>
        <w:left w:w="108" w:type="dxa"/>
        <w:bottom w:w="80" w:type="dxa"/>
        <w:right w:w="108" w:type="dxa"/>
      </w:tblCellMar>
    </w:tblPr>
  </w:style>

  <w:style w:type="table" w:styleId="TableGrid">
    <w:name w:val="Table Grid"/>
    <w:basedOn w:val="TableNormal"/>
    <w:tblPr>
      <w:tblBorders>
        <w:top w:val="single" w:sz="4" w:space="0" w:color="DDDDDD"/>
        <w:left w:val="single" w:sz="4" w:space="0" w:color="DDDDDD"/>
        <w:bottom w:val="single" w:sz="4" w:space="0" w:color="DDDDDD"/>
        <w:right w:val="single" w:sz="4" w:space="0" w:color="DDDDDD"/>
        <w:insideH w:val="single" w:sz="4" w:space="0" w:color="DDDDDD"/>
        <w:insideV w:val="single" w:sz="4" w:space="0" w:color="DDDDDD"/>
      </w:tblBorders>
      <w:tblCellMar>
        <w:top w:w="80" w:type="dxa"/>
        <w:left w:w="108" w:type="dxa"/>
        <w:bottom w:w="80" w:type="dxa"/>
        <w:right w:w="108" w:type="dxa"/>
      </w:tblCellMar>
    </w:tblPr>
    <w:rPr>
      <w:sz w:val="18"/>
    </w:rPr>
  </w:style>

  <w:style w:type="table" w:styleId="BrandTable">
    <w:name w:val="Brand Table"/>
    <w:basedOn w:val="TableGrid"/>
    <w:tblPr>
      <w:tblStyleRowBandSize w:val="1"/>
      <w:tblBorders>
        <w:top w:val="single" w:sz="4" w:space="0" w:color="DDDDDD"/>
        <w:left w:val="single" w:sz="4" w:space="0" w:color="DDDDDD"/>
        <w:bottom w:val="single" w:sz="4" w:space="0" w:color="DDDDDD"/>
        <w:right w:val="single" w:sz="4" w:space="0" w:color="DDDDDD"/>
        <w:insideH w:val="single" w:sz="4" w:space="0" w:color="DDDDDD"/>
        <w:insideV w:val="single" w:sz="4" w:space="0" w:color="DDDDDD"/>
      </w:tblBorders>
      <w:tblCellMar>
        <w:top w:w="100" w:type="dxa"/>
        <w:left w:w="140" w:type="dxa"/>
        <w:bottom w:w="100" w:type="dxa"/>
        <w:right w:w="140" w:type="dxa"/>
      </w:tblCellMar>
    </w:tblPr>
  </w:style>

  <!-- ─── Header / Footer ────────────────────────────────────────────── -->
  <w:style w:type="paragraph" w:styleId="Header">
    <w:name w:val="header"/>
    <w:basedOn w:val="Normal"/>
    <w:link w:val="HeaderChar"/>
    <w:pPr>
      <w:tabs>
        <w:tab w:val="center" w:pos="4680"/>
        <w:tab w:val="right" w:pos="9360"/>
      </w:tabs>
      <w:spacing w:after="0" w:line="240" w:lineRule="auto"/>
    </w:pPr>
    <w:rPr>
      <w:rFonts w:ascii="Calibri" w:hAnsi="Calibri"/>
      <w:sz w:val="18"/>
    </w:rPr>
  </w:style>

  <w:style w:type="character" w:styleId="HeaderChar" w:customStyle="1">
    <w:name w:val="Header Char"/>
    <w:basedOn w:val="DefaultParagraphFont"/>
    <w:link w:val="Header"/>
    <w:rPr>
      <w:rFonts w:ascii="Calibri" w:hAnsi="Calibri"/>
      <w:sz w:val="18"/>
    </w:rPr>
  </w:style>

  <w:style w:type="paragraph" w:styleId="Footer">
    <w:name w:val="footer"/>
    <w:basedOn w:val="Normal"/>
    <w:link w:val="FooterChar"/>
    <w:pPr>
      <w:tabs>
        <w:tab w:val="center" w:pos="4680"/>
        <w:tab w:val="right" w:pos="9360"/>
      </w:tabs>
      <w:spacing w:after="0" w:line="240" w:lineRule="auto"/>
    </w:pPr>
    <w:rPr>
      <w:rFonts w:ascii="Calibri" w:hAnsi="Calibri"/>
      <w:sz w:val="18"/>
    </w:rPr>
  </w:style>

  <w:style w:type="character" w:styleId="FooterChar" w:customStyle="1">
    <w:name w:val="Footer Char"/>
    <w:basedOn w:val="DefaultParagraphFont"/>
    <w:link w:val="Footer"/>
    <w:rPr>
      <w:rFonts w:ascii="Calibri" w:hAnsi="Calibri"/>
      <w:sz w:val="18"/>
    </w:rPr>
  </w:style>

  <w:style w:type="character" w:styleId="PageNumber">
    <w:name w:val="page number"/>
    <w:basedOn w:val="DefaultParagraphFont"/>
    <w:rPr/>
  </w:style>

  <w:style w:type="character" w:styleId="TotalPages">
    <w:name w:val="total pages"/>
    <w:basedOn w:val="DefaultParagraphFont"/>
    <w:rPr/>
  </w:style>
</w:styles>"#;
