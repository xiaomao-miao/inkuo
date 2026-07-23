// Unit tests for the top-level orchestrator and per-tool schema.

import { describe, expect, it } from 'vitest';

import { PATH_DEDUP_KEYS, TOOL_FIELD_LABELS } from './fieldLabels';
import { formatArgumentsForDisplay } from './formatArguments';

describe('fieldLabels', () => {
  it('TOOL_FIELD_LABELS covers every tool that has a custom schema', () => {
    const expected = [
      'grep',
      'glob',
      'list_dir',
      'read_file',
      'read_office_file',
      'create_dir',
      'move_file',
      'database_search',
      'update_todo',
      'ask_user',
      'delegate_to',
      'write_file',
      'edit_file',
      'create_word_doc',
      'compare_word_docs',
      'modify_excel',
      'create_excel',
      'inspect_office',
      'create_svg',
      'create_pptx',
    ];
    expect(Object.keys(TOOL_FIELD_LABELS).sort()).toEqual([...expected].sort());
  });

  it('lists create_word_doc elements with the elements summarizer', () => {
    expect(TOOL_FIELD_LABELS.create_word_doc?.find((f) => f.key === 'elements')).toMatchObject({
      summarize: 'elements',
      label: '正文',
    });
  });

  it('lists modify_excel operations with the operations summarizer', () => {
    expect(TOOL_FIELD_LABELS.modify_excel?.find((f) => f.key === 'operations')).toMatchObject({
      summarize: 'operations',
    });
  });

  it('PATH_DEDUP_KEYS contains the keys that should suppress later matches', () => {
    expect(PATH_DEDUP_KEYS.has('dir_path')).toBe(true);
    expect(PATH_DEDUP_KEYS.has('directory')).toBe(true);
    expect(PATH_DEDUP_KEYS.has('source_path')).toBe(true);
    expect(PATH_DEDUP_KEYS.has('source')).toBe(true);
    expect(PATH_DEDUP_KEYS.has('path')).toBe(false);
    expect(PATH_DEDUP_KEYS.has('destination')).toBe(false);
  });
});

describe('formatArgumentsForDisplay', () => {
  it('returns a labelled multi-line string from parsed args', () => {
    const out = formatArgumentsForDisplay(
      'grep',
      { pattern: 'TODO', path: '/src' },
      undefined,
    );
    expect(out).toBe('搜索：TODO\n路径：/src');
  });

  it('returns null when parsed args are missing and there is no raw fallback', () => {
    expect(formatArgumentsForDisplay('grep', null, undefined)).toBeNull();
  });

  it('skips fields whose values are empty', () => {
    expect(
      formatArgumentsForDisplay(
        'grep',
        { pattern: 'TODO', path: '', file_pattern: undefined as unknown as string },
        undefined,
      ),
    ).toBe('搜索：TODO');
  });

  it('honours PATH_DEDUP_KEYS by stopping at the first match', () => {
    // move_file lists `source_path` then `source` then `destination`.
    // When the parsed args only have `source`, it should render the
    // `源文件：…` line and stop (not try `destination`).
    const out = formatArgumentsForDisplay(
      'move_file',
      { source: '/old.ts', destination: '/new.ts' },
      undefined,
    );
    expect(out).toBe('源文件：/old.ts');
  });

  it('falls back to rawArguments when parsedArgs is missing', () => {
    const out = formatArgumentsForDisplay(
      'grep',
      null,
      '{"pattern": "TODO", "path": ',
    );
    expect(out).toBe('搜索：TODO');
  });

  it('returns a body block on its own line for multi-line summarize values', () => {
    const out = formatArgumentsForDisplay(
      'create_word_doc',
      {
        path: '/x.docx',
        title: '标题',
        elements: [{ text: '第一段' }, { text: '第二段' }],
      },
      undefined,
    );
    expect(out).toBe('文件：/x.docx\n标题：标题\n正文：\n第一段\n第二段');
  });

  it('renders an unknown tool via the best-effort pretty printer', () => {
    const out = formatArgumentsForDisplay(
      'unknown_tool',
      { foo: 'bar', baz: 42, list: [1, 2, 3] },
      undefined,
    );
    expect(out).toContain('foo: bar');
    expect(out).toContain('baz: 42');
    expect(out).toContain('list: [3 项]');
  });

  it('returns null for an unknown tool with no parsed args', () => {
    expect(formatArgumentsForDisplay('unknown_tool', null, undefined)).toBeNull();
  });

  it('returns null for unknown tools whose parsed args are all empty', () => {
    expect(
      formatArgumentsForDisplay('unknown_tool', { foo: '', bar: undefined as unknown }, undefined),
    ).toBeNull();
  });

  it('handles streaming extractors for summarize fields', () => {
    const out = formatArgumentsForDisplay(
      'create_word_doc',
      null,
      '{"elements": [{"text": "第一段"}, {"text": "第二段"}',
    );
    // No `path` / `title` in raw → only the `elements` line is emitted,
    // routed through the streaming extractor.
    expect(out).toBe('正文：\n第一段\n第二段');
  });
});
