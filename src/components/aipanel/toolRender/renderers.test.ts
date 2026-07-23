// Unit tests for the fully-rendered (post-stream) view of tool
// argument arrays. These functions take parsed JSON (no regex), so the
// input shapes here are intended to match what comes out of the model
// after the JSON has been fully received.

import { describe, expect, it } from 'vitest';

import {
  renderElements,
  renderOperations,
  renderSheets,
} from './renderers';

describe('renderers', () => {
  describe('renderElements (create_word_doc)', () => {
    it('joins plain text paragraphs with newlines', () => {
      const elements = [
        { text: '第一段' },
        { text: '第二段' },
      ];
      expect(renderElements(elements)).toBe('第一段\n第二段');
    });

    it('prepends heading markers to Title/Heading1/2/3 styles', () => {
      const elements = [
        { style: 'Heading1', text: '大标题' },
        { style: 'Heading2', text: '小标题' },
        { style: 'Heading3', text: '更小' },
        { style: 'Title', text: '封页' },
      ];
      expect(renderElements(elements)).toBe('# 大标题\n## 小标题\n### 更小\n封页');
    });

    it('uses runs[] when text is absent', () => {
      const elements = [{ runs: [{ text: '运行1' }, { text: '运行2' }] }];
      expect(renderElements(elements)).toBe('运行1运行2');
    });

    it('renders tables as header + pipe-separated rows', () => {
      const elements = [
        {
          type: 'table',
          header: ['甲', '乙'],
          rows: [['A', 'B'], ['C', 'D']],
        },
      ];
      expect(renderElements(elements)).toBe('甲 | 乙\nA | B\nC | D');
    });

    it('detects table shape from header or rows arrays', () => {
      const elements = [{ header: ['h'], rows: [['a']] }];
      expect(renderElements(elements)).toBe('h\na');
    });

    it('returns null for empty / non-array input', () => {
      expect(renderElements([])).toBeNull();
      expect(renderElements(null)).toBeNull();
      expect(renderElements('not array')).toBeNull();
    });

    it('skips non-object elements without crashing', () => {
      const elements = [
        null,
        'string',
        42,
        { text: 'ok' },
      ];
      expect(renderElements(elements)).toBe('ok');
    });

    it('returns null when nothing produces text', () => {
      const elements = [{ style: 'Heading1' }, {}];
      expect(renderElements(elements)).toBeNull();
    });
  });

  describe('renderOperations (modify_excel)', () => {
    it('renders modify_cell with formulas preferred over values', () => {
      const ops = [
        {
          type: 'modify_cell',
          sheet: 'S1',
          address: 'A1',
          formula: 'SUM(B1:B5)',
        },
        { type: 'modify_cell', sheet: 'S1', address: 'A2', value: '文本' },
      ];
      expect(renderOperations(ops)).toBe(
        'S1!A1 = =SUM(B1:B5)\nS1!A2 = 文本',
      );
    });

    it('renders write_range with row-by-row values', () => {
      const ops = [
        {
          type: 'write_range',
          sheet: 'S',
          start_cell: 'A1',
          values: [['x', 'y'], ['z']],
        },
      ];
      expect(renderOperations(ops)).toBe('写入 S!A1 起：\n  x | y\n  z');
    });

    it('renders merge_cells with cancel/merge label', () => {
      expect(
        renderOperations([
          { type: 'merge_cells', sheet: 'S', start_cell: 'A1', end_cell: 'B2' },
        ]),
      ).toBe('合并 S!A1:B2');
      expect(
        renderOperations([
          {
            type: 'merge_cells',
            op: 'unmerge',
            sheet: 'S',
            start_cell: 'A1',
            end_cell: 'B2',
          },
        ]),
      ).toBe('取消合并 S!A1:B2');
    });

    it('renders resize_dimension rows vs columns correctly', () => {
      expect(
        renderOperations([
          { type: 'resize_dimension', dimension: 'row', index: 1, size: 24 },
        ]),
      ).toBe('调整行 1 尺寸 → 24');
      expect(
        renderOperations([
          { type: 'resize_dimension', dimension: 'col', index: 'C', size: 80 },
        ]),
      ).toBe('调整列 C 尺寸 → 80');
    });

    it('renders sheet_op with localized labels', () => {
      const sheet_create = renderOperations([
        { type: 'sheet_op', op: 'create', new_name: 'S2' },
      ]);
      expect(sheet_create).toBe('新建工作表  → S2');

      const sheet_rename = renderOperations([
        { type: 'sheet_op', op: 'rename', sheet: 'S1', new_name: 'S2' },
      ]);
      expect(sheet_rename).toBe('重命名工作表 S1 → S2');
    });

    it('falls back to previewValue for unknown op types', () => {
      const result = renderOperations([{ type: 'unhandled', sheet: 'S' }]);
      expect(result).not.toBeNull();
      // previewValue() summarizes non-matching ops as `{N 字段}`.
      expect(result).toBe('{2 字段}');
    });

    it('omits sheet prefix when sheet is empty', () => {
      expect(
        renderOperations([{ type: 'modify_cell', address: 'A1', value: 1 }]),
      ).toBe('A1 = 1');
    });
  });

  describe('renderSheets (create_excel)', () => {
    it('renders each sheet as a header line + per-cell lines', () => {
      const sheets = [
        {
          name: '销售',
          cells: [
            { address: 'A1', value: 100 },
            { address: 'A2', formula: 'SUM(A1)' },
          ],
          merged: ['A3:B3'],
        },
        {
          name: '库存',
          cells: [{ address: 'A1', value: 5 }],
        },
      ];
      expect(renderSheets(sheets)).toBe(
        '【销售】\n  A1: 100\n  A2: =SUM(A1)\n  合并区域: A3:B3\n' +
          '【库存】\n  A1: 5',
      );
    });

    it('falls back to "工作表" when name is missing', () => {
      expect(renderSheets([{ cells: [{ address: 'A1', value: 1 }] }])).toBe(
        '【工作表】\n  A1: 1',
      );
    });

    it('returns null for empty / non-array input', () => {
      expect(renderSheets([])).toBeNull();
      expect(renderSheets(null)).toBeNull();
    });

    it('skips non-object cells gracefully', () => {
      const sheets = [
        {
          name: 'S',
          cells: [null, 'x', 42, { address: 'A1', value: 'ok' }],
        },
      ];
      expect(renderSheets(sheets)).toBe('【S】\n  A1: ok');
    });

    it('omits the merged-region line when there are no merged cells', () => {
      expect(
        renderSheets([{ name: 'S', cells: [{ address: 'A1', value: 1 }] }]),
      ).toBe('【S】\n  A1: 1');
    });
  });
});
