const TOOL_DISPLAY_NAMES: Record<string, string> = {
  read_file: '读取文件',
  write_file: '写入文件',
  edit_file: '编辑文件',
  list_dir: '列出目录',
  glob: '查找文件',
  grep: '搜索文本',
  read_office_file: '读取 Office 文件',
  create_word_doc: '创建 Word 文档',
  compare_word_docs: '比较 Word 文档',
  create_dir: '创建目录',
  knowledge_build: '构建知识库',
  move_file: '移动文件',
  database_search: '搜索知识库',
  // Office tools (unified backend)
  modify_excel: '修改 Excel',
  create_excel: '创建 Excel',
  inspect_office: '检查 Office 文件',
  // Image / vector tools
  create_svg: '生成 SVG 图片',
  create_pptx: '生成 PPT',
  // Meta / sub-agent tools
  get_tool_help: '加载工具帮助',
  delegate_to: '委派子代理',
  update_todo: '更新任务列表',
  ask_user: '向用户提问',
};

const EXPERT_DISPLAY_NAMES: Record<string, string> = {
  office_word_expert: 'Word 文档专家',
  office_excel_expert: 'Excel 文档专家',
  office_pptx_expert: 'PPT 演示专家',
  md_writer: 'Markdown 写作专家',
  researcher: '调研员',
  batch_editor: '批量编辑员',
  code_expert: '代码工程专家',
};

export function getExpertDisplayName(name: string): string {
  return EXPERT_DISPLAY_NAMES[name] || name;
}

export function getToolDisplayName(name: string): string {
  return TOOL_DISPLAY_NAMES[name] || name;
}

export const COMPACT_TOOLS = new Set([
  'list_dir',
  'glob',
  'grep',
  'read_file',
  'read_office_file',
  'create_dir',
  'move_file',
]);

export const FILE_MODIFICATION_TOOLS = new Set([
  'write_file',
  'edit_file',
  'create_word_doc',
  'modify_excel',
  'create_excel',
  'create_pptx',
]);

export const PREVIEW_STRING_KEYS = new Set([
  'content',
  'new_text',
  'pattern',
  'json_content',
]);

export function isFileModificationTool(name: string): boolean {
  return FILE_MODIFICATION_TOOLS.has(name);
}

export function extractFileNameFromPath(path: string | undefined | null): string | null {
  if (!path) return null;
  return path.split('/').pop() || path.split('\\').pop() || path;
}

/**
 * Per-tool field definitions: each entry maps a field key to a human-readable
 * Chinese label. Fields listed first get higher display priority.
 *
 * `summarize` selects a special formatter for nested-array fields whose raw
 * JSON would be unreadable ('elements', 'operations', 'sheets').
 */
type ToolField = { key: string; label: string; summarize?: 'elements' | 'operations' | 'sheets' };

const TOOL_FIELD_LABELS: Record<string, ToolField[]> = {
  grep: [
    { key: 'pattern', label: '搜索' },
    { key: 'path', label: '路径' },
    { key: 'file_pattern', label: '文件类型' },
  ],
  glob: [
    { key: 'pattern', label: '匹配' },
    { key: 'path', label: '目录' },
  ],
  list_dir: [
    { key: 'path', label: '目录' },
  ],
  read_file: [
    { key: 'path', label: '文件' },
    { key: 'start_line', label: '起始行' },
    { key: 'end_line', label: '结束行' },
  ],
  read_office_file: [
    { key: 'path', label: '文件' },
  ],
  create_dir: [
    { key: 'dir_path', label: '目录' },
    { key: 'directory', label: '目录' },
  ],
  move_file: [
    { key: 'source_path', label: '源文件' },
    { key: 'source', label: '源文件' },
    { key: 'destination', label: '目标' },
  ],
  database_search: [
    { key: 'query', label: '查询' },
    { key: 'top_k', label: '结果数' },
  ],
  update_todo: [
    { key: 'action', label: '操作' },
    { key: 'items', label: '任务' },
  ],
  ask_user: [
    { key: 'question', label: '问题' },
    { key: 'options', label: '选项' },
  ],
  delegate_to: [
    { key: 'expert', label: '专家' },
    { key: 'task', label: '任务' },
    { key: 'context', label: '背景' },
  ],
  // File tools
  write_file: [
    { key: 'path', label: '文件' },
    { key: 'content', label: '内容' },
  ],
  edit_file: [
    { key: 'path', label: '文件' },
    { key: 'old_text', label: '原文' },
    { key: 'new_text', label: '替换为' },
  ],
  // Office: Word
  create_word_doc: [
    { key: 'path', label: '文件' },
    { key: 'title', label: '标题' },
    { key: 'elements', label: '正文', summarize: 'elements' },
  ],
  compare_word_docs: [
    { key: 'path1', label: '原文档' },
    { key: 'path2', label: '新文档' },
  ],
  // Office: Excel (unified backend tools)
  modify_excel: [
    { key: 'path', label: '文件' },
    { key: 'operations', label: '操作', summarize: 'operations' },
  ],
  create_excel: [
    { key: 'path', label: '文件' },
    { key: 'sheets', label: '内容', summarize: 'sheets' },
  ],
  inspect_office: [
    { key: 'path', label: '文件' },
    { key: 'format', label: '格式' },
    { key: 'mode', label: '模式' },
    { key: 'sheet', label: '工作表' },
    { key: 'range', label: '区域' },
  ],
  // Image / vector tools
  create_svg: [
    { key: 'description', label: '描述' },
    { key: 'output_path', label: '保存到' },
    { key: 'aspect_ratio', label: '比例' },
  ],
  create_pptx: [
    { key: 'output_path', label: '保存到' },
    { key: 'title', label: '标题' },
  ],
};

/**
 * Try to extract a value for a known key from a potentially-incomplete JSON
 * string (e.g. mid-stream). Uses a simple regex approach rather than a full
 * JSON parser so it works on truncated input.
 */
function extractFieldFromRaw(raw: string, key: string): string | null {
  // Match: "key": "value" (string value, possibly spanning multiple chunks)
  const strRe = new RegExp(`"${key}"\\s*:\\s*"((?:[^"\\\\]|\\\\.)*?)(?:"|$)`);
  const strMatch = raw.match(strRe);
  if (strMatch) {
    try {
      return JSON.parse(`"${strMatch[1]}"`);
    } catch {
      return strMatch[1];
    }
  }
  // Match: "key": number or boolean
  const primRe = new RegExp(`"${key}"\\s*:\\s*([0-9]+(?:\\.[0-9]+)?|true|false)`);
  const primMatch = raw.match(primRe);
  if (primMatch) return primMatch[1];
  // Match: "key": [ ... (array start, return raw preview up to next 200 chars)
  const arrRe = new RegExp(`"${key}"\\s*:\\s*\\[`);
  if (arrRe.test(raw)) return '[…正在生成…]';
  // Match: "key": { ... (object start)
  const objRe = new RegExp(`"${key}"\\s*:\\s*\\{`);
  if (objRe.test(raw)) return '{…}';
  return null;
}

/** Naively split the body of a JSON array into per-object snippets. */
function splitArrayEntries(body: string): string[] {
  const entries: string[] = [];
  let depth = 0;
  let inStr = false;
  let esc = false;
  let buf = '';
  for (let i = 0; i < body.length; i++) {
    const ch = body[i];
    if (esc) {
      buf += ch;
      esc = false;
      continue;
    }
    if (ch === '\\') {
      buf += ch;
      esc = true;
      continue;
    }
    if (ch === '"') {
      inStr = !inStr;
      buf += ch;
      continue;
    }
    if (!inStr) {
      if (ch === '{' || ch === '[') depth++;
      if (ch === '}' || ch === ']') depth--;
      if (ch === ',' && depth === 0) {
        entries.push(buf);
        buf = '';
        continue;
      }
    }
    buf += ch;
  }
  if (buf.trim().length > 0) entries.push(buf);
  return entries;
}

/**
 * Format a value (any JSON value) into a short human-readable preview.
 * Truncates strings and summarizes arrays/objects without dumping raw JSON.
 */
function previewValue(v: unknown, maxLen = 80): string {
  if (v === null || v === undefined) return '';
  if (typeof v === 'string') {
    return v.length > maxLen ? `${v.slice(0, maxLen)}…` : v;
  }
  if (typeof v === 'number' || typeof v === 'boolean') return String(v);
  if (Array.isArray(v)) {
    if (v.length === 0) return '[]';
    return `[${v.length} 项]`;
  }
  if (typeof v === 'object') {
    const keys = Object.keys(v);
    if (keys.length === 0) return '{}';
    return `{${keys.length} 字段}`;
  }
  return String(v);
}

const HEADING_PREFIX: Record<string, string> = {
  Title: '',
  Heading1: '# ',
  Heading2: '## ',
  Heading3: '### ',
};

/** Extract text from a run list when a paragraph uses `runs[]` instead of `text`. */
function textFromRuns(runs: unknown): string {
  if (!Array.isArray(runs)) return '';
  return runs
    .map((r) => (r && typeof r === 'object' ? (r as Record<string, unknown>).text : ''))
    .filter((t) => typeof t === 'string')
    .join('');
}

/** Render a table cell (string or {text, ...} object) to text. */
function cellText(cell: unknown): string {
  if (typeof cell === 'string') return cell;
  if (cell && typeof cell === 'object') {
    const t = (cell as Record<string, unknown>).text;
    if (typeof t === 'string') return t;
  }
  return '';
}

/**
 * Render an `elements[]` array (create_word_doc) as full readable body text.
 * Each paragraph becomes a line; tables become pipe-separated rows.
 */
function renderElements(elements: unknown): string | null {
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

/** Unwrap a create_excel/modify_excel typed value object `{type, value}`. */
function unwrapCellValue(value: unknown): string {
  if (value && typeof value === 'object' && 'value' in (value as Record<string, unknown>)) {
    const inner = (value as Record<string, unknown>).value;
    return inner === null || inner === undefined ? '' : String(inner);
  }
  if (value === null || value === undefined) return '';
  return String(value);
}

/**
 * Render an `operations[]` array (modify_excel) as full readable lines.
 */
function renderOperations(operations: unknown): string | null {
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
        const opLabels: Record<string, string> = {
          create: '新建', rename: '重命名', delete: '删除', hide: '隐藏', unhide: '显示',
        };
        const opName = opLabels[String(o.op)] || String(o.op);
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
 */
function renderSheets(sheets: unknown): string | null {
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

/**
 * Format tool arguments into a human-readable multi-line string.
 *
 * - If `parsedArgs` is available, uses it directly.
 * - Falls back to regex extraction from `rawArguments` (handles mid-stream).
 * - Returns null if nothing useful can be extracted.
 */
export function formatArgumentsForDisplay(
  toolName: string,
  parsedArgs: Record<string, unknown> | null,
  rawArguments: string | undefined
): string | null {
  const fieldDefs = TOOL_FIELD_LABELS[toolName];
  if (!fieldDefs || fieldDefs.length === 0) {
    // Unknown tool: best-effort pretty print of parsedArgs
    if (parsedArgs && Object.keys(parsedArgs).length > 0) {
      return Object.entries(parsedArgs)
        .filter(([, v]) => v !== undefined && v !== null && v !== '')
        .map(([k, v]) => {
          const display = typeof v === 'string' ? v : previewValue(v, 80);
          return `${k}: ${display}`;
        })
        .join('\n');
    }
    return null;
  }

  const lines: string[] = [];
  for (const { key, label, summarize } of fieldDefs) {
    let value: string | null = null;

    if (parsedArgs) {
      const v = parsedArgs[key];
      if (v !== undefined && v !== null && v !== '') {
        if (summarize === 'elements') {
          value = renderElements(v);
        } else if (summarize === 'operations') {
          value = renderOperations(v);
        } else if (summarize === 'sheets') {
          value = renderSheets(v);
        } else if (typeof v === 'string') {
          value = v;
        } else {
          value = previewValue(v, 80);
        }
      }
    } else if (rawArguments) {
      // Streaming fallback — extract as much readable text as possible from the
      // partial JSON so the user watches content appear live.
      if (summarize === 'elements') {
        value = renderElementsFromRaw(rawArguments, key);
      } else if (summarize === 'operations') {
        value = renderOperationsFromRaw(rawArguments, key);
      } else if (summarize === 'sheets') {
        value = renderSheetsFromRaw(rawArguments, key);
      } else {
        value = extractFieldFromRaw(rawArguments, key);
      }
    }

    if (value !== null) {
      // Multi-line body values go on their own line under the label for readability.
      if (value.includes('\n')) {
        lines.push(`${label}：\n${value}`);
      } else {
        lines.push(`${label}：${value}`);
      }
      // Only show the first matching field for path-like dedup (e.g. dir_path vs directory)
      if (key === 'dir_path' || key === 'directory' || key === 'source_path' || key === 'source') break;
    }
  }

  return lines.length > 0 ? lines.join('\n') : null;
}

/** Streaming: extract array body for `key` from partial raw JSON. */
function extractArrayBody(raw: string, key: string): string | null {
  const m = raw.match(new RegExp(`"${key}"\\s*:\\s*\\[([\\s\\S]*)$`));
  return m ? m[1] : null;
}

/** Streaming render of create_word_doc elements from partial raw JSON. */
function renderElementsFromRaw(raw: string, key: string): string | null {
  const body = extractArrayBody(raw, key);
  if (body === null) return null;
  const entries = splitArrayEntries(body);
  const lines: string[] = [];
  for (const entry of entries) {
    const text = extractFieldFromRaw(entry, 'text');
    if (text && text !== '[…正在生成…]' && text !== '{…}') {
      lines.push(text);
      continue;
    }
    // table header fallback
    const headerBody = entry.match(/"header"\s*:\s*\[([\s\S]*?)(?:\]|$)/);
    if (headerBody) {
      const cells = [...headerBody[1].matchAll(/"((?:[^"\\]|\\.)*?)"/g)].map((mm) => mm[1]);
      if (cells.length > 0) lines.push(cells.join(' | '));
    }
  }
  return lines.length > 0 ? lines.join('\n') : null;
}

/** Streaming render of modify_excel operations from partial raw JSON. */
function renderOperationsFromRaw(raw: string, key: string): string | null {
  const body = extractArrayBody(raw, key);
  if (body === null) return null;
  const entries = splitArrayEntries(body);
  const lines: string[] = [];
  for (const entry of entries) {
    const type = extractFieldFromRaw(entry, 'type');
    const addr = extractFieldFromRaw(entry, 'address');
    const sheet = extractFieldFromRaw(entry, 'sheet');
    const prefix = sheet && sheet !== '[…正在生成…]' ? `${sheet}!` : '';
    if (type === 'modify_cell' && addr) {
      const formula = extractFieldFromRaw(entry, 'formula');
      lines.push(`${prefix}${addr} = ${formula ? `=${formula}` : '…'}`);
    } else if (type) {
      lines.push(`${prefix}${type}…`);
    }
  }
  return lines.length > 0 ? lines.join('\n') : null;
}

/** Streaming render of create_excel sheets from partial raw JSON. */
function renderSheetsFromRaw(raw: string, key: string): string | null {
  const body = extractArrayBody(raw, key);
  if (body === null) return null;
  const entries = splitArrayEntries(body);
  const lines: string[] = [];
  for (const entry of entries) {
    const name = extractFieldFromRaw(entry, 'name');
    if (name && name !== '[…正在生成…]' && name !== '{…}') {
      lines.push(`【${name}】`);
    }
    const cellsBody = entry.match(/"cells"\s*:\s*\[([\s\S]*)$/);
    if (cellsBody) {
      const cellEntries = splitArrayEntries(cellsBody[1]);
      for (const ce of cellEntries) {
        const addr = extractFieldFromRaw(ce, 'address');
        if (addr && addr !== '[…正在生成…]') lines.push(`  ${addr}…`);
      }
    }
  }
  return lines.length > 0 ? lines.join('\n') : null;
}
