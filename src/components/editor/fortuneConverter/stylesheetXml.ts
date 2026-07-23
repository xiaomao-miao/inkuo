// Build the `xl/styles.xml` content for an .xlsx workbook.
//
// Excel uses sparse style indices: cell `<c s="N"/>` references
// `<xf>` N in `cellXfs`, which in turn points at `<font>`, `<fill>`,
// and `<numFmt>` indices. To keep the file size sane and the fonts
// deduplicated, we first scan the unique style keys to assign each
// combination of (font, fill, numFmt, bold, italic) a stable index.
//
// Split out from `sheetjsExport.ts` so this tangle of XML building
// stays isolated from the worksheet / zip assembly logic.

/** Fields inside a single style key — order must match `parseStyleKey`. */
export interface ParsedStyleKey {
  numberFormat: string;
  fillFg: string;
  // fillBg slot is reserved in the key shape but currently unused
  // by FortuneSheet; kept here so the parser matches the producer.
  fillBg: string;
  bold: boolean;
  italic: boolean;
  fontColor: string;
  fontSize: number | null;
  fontName: string;
  horizontalAlign: string;
  verticalAlign: string;
}

export interface FontSpec {
  name: string;
  sz: number;
  color: string;
  bold: boolean;
  italic: boolean;
}

export interface FillSpec {
  patternType: string;
  fgColor: string;
}

export interface CellXfSpec {
  fontId: number;
  fillId: number;
  numFmtId: number;
  bold: boolean;
  italic: boolean;
}

/**
 * Parse a style key string into its components.
 *
 * Key shape (pipe-separated, exactly 10 parts):
 *   number_format|fill_fg|fill_bg|bold|italic|font_color|font_size|font_name|h_align|v_align
 */
export function parseStyleKey(key: string): ParsedStyleKey {
  const parts = key.split('|');
  return {
    numberFormat: parts[0] || '',
    fillFg: parts[1] || '',
    fillBg: parts[2] || '',
    bold: parts[3] === '1',
    italic: parts[4] === '1',
    fontColor: parts[5] || '',
    fontSize: parts[6] ? parseInt(parts[6], 10) : null,
    fontName: parts[7] || '',
    horizontalAlign: parts[8] || '',
    verticalAlign: parts[9] || '',
  };
}

/** Compose a style key from its components. The inverse of `parseStyleKey`. */
export function composeStyleKey(p: ParsedStyleKey): string {
  return [
    p.numberFormat,
    p.fillFg,
    p.fillBg,
    p.bold ? '1' : '0',
    p.italic ? '1' : '0',
    p.fontColor,
    p.fontSize != null ? String(p.fontSize) : '',
    p.fontName,
    p.horizontalAlign,
    p.verticalAlign,
  ].join('|');
}

/** The default font that occupies `<fonts>` index 0. */
export const DEFAULT_FONT: FontSpec = {
  name: 'Calibri',
  sz: 11,
  color: '000000',
  bold: false,
  italic: false,
};

/** Escape a string for safe inclusion inside an XML attribute or text node. */
export function escapeXml(str: string): string {
  return str
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&apos;');
}

/** Convert a 0-based column index to its A-Z column letter (A, B, … Z, AA, AB …). */
export function colLetter(col: number): string {
  let result = '';
  col++;
  while (col > 0) {
    col--;
    result = String.fromCharCode(65 + (col % 26)) + result;
    col = Math.floor(col / 26);
  }
  return result;
}

/**
 * Compute the dedup key for a font spec — same shape used to insert
 * into `fontSet` and used by `collectFonts` to skip duplicates.
 */
function fontSpecKey(f: FontSpec): string {
  return `${f.name}|${f.sz}|${f.color}|${f.bold ? '1' : '0'}|${f.italic ? '1' : '0'}`;
}

/** Compute the dedup key for a font from a parsed style key. */
function parsedStyleFontKey(s: ParsedStyleKey): string {
  return `${s.fontName || 'Calibri'}|${s.fontSize ?? 11}|${s.fontColor || '000000'}|${s.bold ? '1' : '0'}|${s.italic ? '1' : '0'}`;
}

/**
 * Build the deduplicated font list from the unique style keys. Index 0
 * is always the default Calibri 11pt black (matching Excel's empty
 * workbook), so callers can skip it with `fontList.slice(1)`.
 */
function collectFonts(uniqueStyleKeys: string[]): FontSpec[] {
  const list: FontSpec[] = [{ ...DEFAULT_FONT }];
  const seen = new Set<string>([fontSpecKey(DEFAULT_FONT)]);

  for (const key of uniqueStyleKeys) {
    const s = parseStyleKey(key);
    // Include bold/italic in fontKey to properly deduplicate fonts with different styles
    const fontKey = parsedStyleFontKey(s);
    if (seen.has(fontKey)) continue;
    seen.add(fontKey);
    list.push({
      name: s.fontName || 'Calibri',
      sz: s.fontSize ?? 11,
      color: s.fontColor || '000000',
      bold: s.bold,
      italic: s.italic,
    });
  }
  return list;
}

/** Build the deduplicated fill list. Index 0 = none, index 1 = gray125. */
function collectFills(uniqueStyleKeys: string[]): FillSpec[] {
  const list: FillSpec[] = [
    { patternType: '', fgColor: '' },
    { patternType: 'gray125', fgColor: '' },
  ];
  for (const key of uniqueStyleKeys) {
    const s = parseStyleKey(key);
    if (s.fillFg && !list.some((f) => f.patternType === 'solid' && f.fgColor === s.fillFg)) {
      list.push({ patternType: 'solid', fgColor: s.fillFg });
    }
  }
  return list;
}

/**
 * Build the styles XML document from a list of unique style keys.
 *
 * Format reference:
 *   xl/styles.xml schema is part of OOXML ECMA-376 Part 1, §18.8.
 *   Structure: numFmts (custom formats), fonts, fills, borders,
 *   cellStyleXfs, cellXfs, cellStyles, dxfs, tableStyles.
 *
 * Returns an empty string when there are no styles to emit.
 */
export function buildStylesXml(uniqueStyleKeys: string[]): string {
  if (uniqueStyleKeys.length === 0) return '';

  const fontList = collectFonts(uniqueStyleKeys);
  const fillList = collectFills(uniqueStyleKeys);

  const fontSet = new Map<string, number>();
  fontList.forEach((f, i) => fontSet.set(fontSpecKey(f), i));

  const fillSet = new Map<string, number>();
  fillList.forEach((f, i) => fillSet.set(f.patternType === 'solid' ? f.fgColor : f.patternType || 'none', i));

  const numFmtMap = new Map<string, number>();
  let nextNumFmtId = 164;
  for (const key of uniqueStyleKeys) {
    const s = parseStyleKey(key);
    if (s.numberFormat && !numFmtMap.has(s.numberFormat)) {
      numFmtMap.set(s.numberFormat, nextNumFmtId++);
    }
  }

  // Index 0 = default (font 0, fill 0, numFmtId 0). One entry per distinct style key.
  const cellXfs: CellXfSpec[] = [
    { fontId: 0, fillId: 0, numFmtId: 0, bold: false, italic: false },
  ];
  for (const key of uniqueStyleKeys) {
    const s = parseStyleKey(key);
    const fontKey = parsedStyleFontKey(s);
    const fillKey = s.fillFg || 'none';
    const numFmtId = s.numberFormat ? (numFmtMap.get(s.numberFormat) ?? 0) : 0;
    cellXfs.push({
      fontId: fontSet.get(fontKey) ?? 0,
      fillId: fillSet.get(fillKey) ?? 0,
      numFmtId,
      bold: s.bold,
      italic: s.italic,
    });
  }

  // ── Build the <fonts> block ──
  let fontsXml = `<fonts count="${fontList.length}">`;
  fontsXml += '<font><name val="Calibri"/><family val="2"/><color theme="1"/><sz val="11"/><scheme val="minor"/></font>';
  for (const f of fontList.slice(1)) {
    fontsXml += '<font>';
    fontsXml += `<name val="${escapeXml(f.name)}"/><family val="2"/>`;
    fontsXml += f.color ? `<color rgb="${escapeXml(f.color)}"/>` : '<color theme="1"/>';
    fontsXml += `<sz val="${f.sz}"/>`;
    if (f.bold) fontsXml += '<b/>';
    if (f.italic) fontsXml += '<i/>';
    fontsXml += '<scheme val="minor"/></font>';
  }
  fontsXml += '</fonts>';

  // ── Build the <fills> block ──
  let fillsXml = `<fills count="${fillList.length}">`;
  for (const fl of fillList) {
    if (!fl.patternType) {
      fillsXml += '<fill><patternFill/></fill>';
    } else {
      fillsXml += `<fill><patternFill patternType="${fl.patternType}">`;
      if (fl.fgColor) fillsXml += `<fgColor rgb="${escapeXml(fl.fgColor)}"/>`;
      fillsXml += '</patternFill></fill>';
    }
  }
  fillsXml += '</fills>';

  // ── Build the <numFmts> block ──
  let numFmtsXml = `<numFmts count="${numFmtMap.size}">`;
  for (const [fmt, id] of numFmtMap) {
    numFmtsXml += `<numFmt numFmtId="${id}" formatCode="${escapeXml(fmt)}"/>`;
  }
  numFmtsXml += '</numFmts>';

  // ── Build the <cellXfs> block ──
  let cellXfsXml = `<cellXfs count="${cellXfs.length}">`;
  for (const xf of cellXfs) {
    let attrs = `numFmtId="${xf.numFmtId}" fontId="${xf.fontId}" fillId="${xf.fillId}" borderId="0" xfId="0"`;
    if (xf.bold || xf.italic) attrs += ' applyFont="1"';
    if (xf.fillId > 1) attrs += ' applyFill="1"';
    if (xf.numFmtId > 0) attrs += ' applyNumberFormat="1"';
    cellXfsXml += `<xf ${attrs}/>`;
  }
  cellXfsXml += '</cellXfs>';

  return [
    '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>',
    `<styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">`,
    numFmtsXml,
    fontsXml,
    fillsXml,
    '<borders count="1"><border><left/><right/><top/><bottom/><diagonal/></border></borders>',
    '<cellStyleXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/></cellStyleXfs>',
    cellXfsXml,
    '<cellStyles count="1"><cellStyle name="Normal" xfId="0" builtinId="0" hidden="0"/></cellStyles>',
    '<dxfs count="0"/>',
    '<tableStyles count="0" defaultTableStyle="TableStyleMedium9" defaultPivotStyle="PivotStyleLight16"/>',
    '</styleSheet>',
  ].join('');
}