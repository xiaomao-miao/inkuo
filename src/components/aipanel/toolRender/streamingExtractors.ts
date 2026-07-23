// Streaming-JSON extractors for tool arguments.
//
// These power the "live" display in the chat panel: as the model streams
// a tool-call's `arguments` JSON, the renderer grabs whatever it can out
// of the partial string so the user sees content appear progressively
// instead of waiting for the full block.
//
// We deliberately use a regex + simple state machine rather than a full
// JSON parser because the input may be truncated mid-key, mid-string,
// or mid-array — `JSON.parse` would throw and we'd lose the preview
// entirely. The helpers in this file are intentionally permissive and
// gracefully degrade to placeholder strings (`[…正在生成…]`, `{…}`)
// when the input is incomplete.

/**
 * Try to extract a value for a known key from a potentially-incomplete JSON
 * string (e.g. mid-stream). Matches:
 *  - `"key": "string value"`        — JSON-decoded to handle escape sequences
 *  - `"key": 12.3` / `true` / `false` — primitive, returned as-is
 *  - `"key": [`                     — array start → placeholder
 *  - `"key": {`                     — object start → placeholder
 */
export function extractFieldFromRaw(raw: string, key: string): string | null {
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
  // Match: "key": [ ... (array start)
  const arrRe = new RegExp(`"${key}"\\s*:\\s*\\[`);
  if (arrRe.test(raw)) return '[…正在生成…]';
  // Match: "key": { ... (object start)
  const objRe = new RegExp(`"${key}"\\s*:\\s*\\{`);
  if (objRe.test(raw)) return '{…}';
  return null;
}

/**
 * Extract the body of a JSON array whose key matches `key`, e.g.
 *   raw = '{ "elements": [ { "text": "…" }, …'
 * returns the substring starting at the first `[` after the key.
 * Returns null if the key isn't present.
 */
export function extractArrayBody(raw: string, key: string): string | null {
  const m = raw.match(new RegExp(`"${key}"\\s*:\\s*\\[([\\s\\S]*)$`));
  return m ? m[1] : null;
}

/**
 * Naively split the body of a JSON array into per-object snippets.
 *
 * Walks the body character-by-character, tracking depth (curly + square
 * brackets) and string state, splitting on top-level commas. Designed
 * for streaming input so unbalanced trailing fragments just land in the
 * last entry — call sites filter out empties.
 */
export function splitArrayEntries(body: string): string[] {
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

/** Streaming render of create_word_doc elements from partial raw JSON. */
export function renderElementsFromRaw(raw: string, key: string): string | null {
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
export function renderOperationsFromRaw(raw: string, key: string): string | null {
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
export function renderSheetsFromRaw(raw: string, key: string): string | null {
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
