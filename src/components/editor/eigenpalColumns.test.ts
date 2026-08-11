import { describe, expect, it } from 'vitest';
import {
  layoutDocument,
  type FlowBlock,
  type Measure,
  type ParagraphFragment,
} from '@eigenpal/docx-editor-core/layout-engine';

const paragraph = (id: string, lineCount: number): { block: FlowBlock; measure: Measure } => ({
  block: {
    kind: 'paragraph',
    id,
    runs: [{ kind: 'text', text: `${id} `.repeat(lineCount * 8) }],
  },
  measure: {
    kind: 'paragraph',
    lines: Array.from({ length: lineCount }, (_, index) => ({
      fromRun: 0,
      fromChar: index * 8,
      toRun: 0,
      toChar: (index + 1) * 8,
      width: 120,
      ascent: 15,
      descent: 5,
      lineHeight: 20,
    })),
    totalHeight: lineCount * 20,
  },
});

const section = (id: string, count: number): { block: FlowBlock; measure: Measure } => ({
  block: {
    kind: 'sectionBreak',
    id,
    type: 'continuous',
    columns: { count, gap: 24, equalWidth: true },
  },
  measure: { kind: 'sectionBreak' },
});

describe('Eigenpal continuous column layout patch', () => {
  it('balances an intermediate two-column section and uses column width', () => {
    const entries = [
      paragraph('heading', 1),
      section('heading-end', 1),
      paragraph('left', 10),
      paragraph('right', 10),
      section('columns-end', 2),
      paragraph('footer', 1),
    ];

    const layout = layoutDocument(
      entries.map(({ block }) => block),
      entries.map(({ measure }) => measure),
      {
        pageSize: { w: 600, h: 800 },
        margins: { top: 60, right: 60, bottom: 60, left: 60 },
        columns: { count: 1, gap: 24, equalWidth: true },
        finalPageSize: { w: 600, h: 800 },
        finalMargins: { top: 60, right: 60, bottom: 60, left: 60 },
        bodyBreakType: 'continuous',
      },
    );

    const fragments = layout.pages[0]?.fragments as ParagraphFragment[];
    const left = fragments.find((fragment) => fragment.blockId === 'left');
    const right = fragments.find((fragment) => fragment.blockId === 'right');
    const footer = fragments.find((fragment) => fragment.blockId === 'footer');

    expect(left).toMatchObject({ x: 60, y: 80, width: 228 });
    expect(right).toMatchObject({ x: 312, y: 80, width: 228 });
    expect(footer).toMatchObject({ x: 60, y: 280, width: 480 });
  });
});
