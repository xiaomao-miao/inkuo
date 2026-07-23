// Fully-rendered (post-stream) views of tool argument arrays.
//
// These take the parsed value of a `summarize` field and turn it into
// multi-line readable text. Mirror functions for the streaming case live
// in `streamingExtractors.ts` — when the field is fully parsed we prefer
// these because they preserve more structure (run lists, merged cells,
// formula precedence, etc.).
//
// The output of every renderer is "newline-separated summary lines"
// suitable for dropping under the tool card's label.

import {
  cellText,
  HEADING_PREFIX,
  previewValue,
  textFromRuns,
  unwrapCellValue,
} from './valueHelpers';

/**
 * Render an `elements[]` array (create_word_doc) as full readable body text.
 * Each paragraph becomes a line; tables become pipe-separated rows.
 */
export function renderElements(elements: unknown): string | null {
  if (!Array.isArray(elements) || elements.length === 0) return null;
  const lines: string[] = [];

  for (const el of elements) {
    if (!el || typeof el !== 'object') continue;
    const e = el as Record<string, unknown>;
    const isTable = e.type === 'table' || Array.isArray(e.rows) || Array.isArray(e.header);

    if (isTable) {
      const header = Array.isArray(e.header) ? e.header : [];
      if (header.length > 0) {
        lines.push(header.map(cellText).join(' | '));
      }
      const rows = Array.isArray(e.rows) ? e.rows : [];
      for (const row of rows) {
        if (Array.isArray(row)) {
          lines.push(row.map(cellText).join(' | '));
        }
      }
    } else {
      const style = typeof e.style === 'string' ? e.style : '';
      const prefix = HEADING_PREFIX[style] ?? '';
      const text = typeof e.text === 'string' && e.text.length > 0 ? e.text : textFromRuns(e.runs);
      if (text) lines.push(`${prefix}${text}`);
    }
  }

  return lines.length > 0 ? lines.join('\n') : null;
}

const OP_LABELS: Record<string, string> = {
  create: '新建',
  rename: '重命名',
  delete: '删除',
  hide: '隐藏',
  unhide: '显示',
};

/**
 * Render an `operations[]` array (modify_excel) as full readable lines.
 * One line per operation, prefixed with the sheet name when known.
 */
export function renderOperations(operations: unknown): string | null {
  if (!Array.isArray(operations) || operations.length === 0) return null;
  const lines: string[] = [];

  for (const op of operations) {
    if (!op || typeof op !== 'object') continue;
    const o = op as Record<string, unknown>;
    const sheet = typeof o.sheet === 'string' ? o.sheet : '';
    const sheetPrefix = sheet ? `${sheet}!` : '';

    switch (o.type) {
      case 'modify_cell': {
        const addr = typeof o.address === 'string' ? o.address : '';
        const val = o.formula ? `=${o.formula}` : unwrapCellValue(o.value);
        lines.push(`${sheetPrefix}${addr} = ${val}`);
        break;
      }
      case 'write_range': {
        const start = typeof o.start_cell === 'string' ? o.start_cell : '';
        const values = Array.isArray(o.values) ? o.values : [];
        lines.push(`写入 ${sheetPrefix}${start} 起：`);
        for (const row of values) {
          if (Array.isArray(row)) {
            lines.push(`  ${row.map((c) => unwrapCellValue(c)).join(' | ')}`);
          }
        }
        break;
      }
      case 'merge_cells': {
        const label = o.op === 'unmerge' ? '取消合并' : '合并';
        lines.push(`${label} ${sheetPrefix}${o.start_cell ?? ''}:${o.end_cell ?? ''}`);
        break;
      }
      case 'resize_dimension': {
        const dim = o.dimension === 'col' ? '列' : '行';
        lines.push(`调整${dim} ${o.index ?? ''} 尺寸 → ${o.size ?? ''}`);
        break;
      }
      case 'sheet_op': {
        const opName = OP_LABELS[String(o.op)] || String(o.op);
        const target = o.new_name ? `${o.sheet ?? ''} → ${o.new_name}` : (o.sheet ?? o.new_name ?? '');
        lines.push(`${opName}工作表 ${target}`);
        break;
      }
      default:
        lines.push(previewValue(o, 60));
    }
  }

  return lines.length > 0 ? lines.join('\n') : null;
}

/**
 * Render a `sheets[]` array (create_excel) as full readable content per sheet.
 * Each sheet becomes a block with a header line and one line per cell.
 */
export function renderSheets(sheets: unknown): string | null {
  if (!Array.isArray(sheets) || sheets.length === 0) return null;
  const blocks: string[] = [];

  for (const s of sheets) {
    if (!s || typeof s !== 'object') continue;
    const sheet = s as Record<string, unknown>;
    const name = typeof sheet.name === 'string' ? sheet.name : '工作表';
    const cells = Array.isArray(sheet.cells) ? sheet.cells : [];
    const merged = Array.isArray(sheet.merged) ? sheet.merged : [];

    const lines: string[] = [`【${name}】`];
    for (const cell of cells) {
      if (!cell || typeof cell !== 'object') continue;
      const c = cell as Record<string, unknown>;
      const addr = typeof c.address === 'string' ? c.address : '';
      const val = c.formula ? `=${c.formula}` : unwrapCellValue(c.value);
      lines.push(`  ${addr}: ${val}`);
    }
    if (merged.length > 0) {
      lines.push(`  合并区域: ${merged.map((m) => String(m)).join('、')}`);
    }
    blocks.push(lines.join('\n'));
  }

  return blocks.length > 0 ? blocks.join('\n') : null;
}
