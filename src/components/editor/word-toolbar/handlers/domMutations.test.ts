// Unit tests for the document-model mutation helpers.
//
// All helpers round-trip through `JSON.parse(JSON.stringify(doc))`,
// so the test inputs can be plain JSON literals rather than fully
// typed `DocModel`s. We cast each fixture to keep the tests short.

import { describe, expect, it } from 'vitest';

import {
  applyHeaderFooter,
  applyPageColor,
  buildHeaderFooterRuns,
  buildWatermarkSpec,
  type DocModel,
} from './domMutations';

describe('applyPageColor', () => {
  it('sets the background color on an empty doc', () => {
    const out = applyPageColor({}, '#ff0000') as DocModel;
    expect(out.body?.finalSectionProperties?.background?.color?.rgb).toBe('FF0000');
  });

  it('strips the leading # and upper-cases the color', () => {
    const out = applyPageColor({}, '#a1b2c3') as DocModel;
    expect(out.body?.finalSectionProperties?.background?.color?.rgb).toBe('A1B2C3');
  });

  it('preserves an existing section that has no background', () => {
    const out = applyPageColor(
      { body: { finalSectionProperties: { titlePage: false } } },
      '#abcdef',
    ) as DocModel;
    expect(out.body?.finalSectionProperties?.titlePage).toBe(false);
    expect(out.body?.finalSectionProperties?.background?.color?.rgb).toBe('ABCDEF');
  });

  it('clears the background when color is "none"', () => {
    const out = applyPageColor(
      { body: { finalSectionProperties: { background: { color: { rgb: 'FF0000' } } } } },
      'none',
    ) as DocModel;
    expect(out.body?.finalSectionProperties?.background).toBeUndefined();
  });

  it('clears the background when color is empty string', () => {
    const out = applyPageColor(
      { body: { finalSectionProperties: { background: { color: { rgb: 'FF0000' } } } } },
      '',
    ) as DocModel;
    expect(out.body?.finalSectionProperties?.background).toBeUndefined();
  });

  it('returns null when "clearing" a doc that has no background', () => {
    expect(applyPageColor({ body: {} }, 'none')).toBeNull();
    expect(applyPageColor({ body: {} }, '')).toBeNull();
  });

  it('does not mutate the input doc', () => {
    const input = { body: { finalSectionProperties: {} } } as DocModel;
    applyPageColor(input, '#000000');
    expect(input.body?.finalSectionProperties?.background).toBeUndefined();
  });
});

describe('buildHeaderFooterRuns', () => {
  it('returns a single text run when only text is provided', () => {
    const runs = buildHeaderFooterRuns({ text: 'Page 1' });
    expect(runs).toEqual([{ text: 'Page 1', type: 'run' }]);
  });

  it('returns a single PAGE field when only the page-number toggle is on', () => {
    const runs = buildHeaderFooterRuns({ text: '', includePageNumber: true });
    expect(runs).toEqual([{ text: 'PAGE', type: 'field', fieldType: 'PAGE' }]);
  });

  it('inserts a separator space between text and the PAGE field when both are present', () => {
    const runs = buildHeaderFooterRuns({ text: 'Hello', includePageNumber: true });
    expect(runs).toEqual([
      { text: 'Hello', type: 'run' },
      { text: ' ', type: 'run' },
      { text: 'PAGE', type: 'field', fieldType: 'PAGE' },
    ]);
  });

  it('returns an empty array when both text and page-number are missing', () => {
    expect(buildHeaderFooterRuns({ text: '' })).toEqual([]);
    expect(buildHeaderFooterRuns({ text: '', includePageNumber: false })).toEqual([]);
  });
});

describe('applyHeaderFooter', () => {
  it('returns null when no runs would be produced (empty text + no page number)', () => {
    const input = {} as DocModel;
    expect(applyHeaderFooter(input, 'header', { text: '' })).toBeNull();
  });

  it('inserts a header with the correct reference + parts map', () => {
    const out = applyHeaderFooter(
      { body: {} } as DocModel,
      'header',
      { text: 'My Doc' },
    ) as DocModel;
    expect(out.headers).toBeDefined();
    expect(out.body?.finalSectionProperties?.headerReferences).toHaveLength(1);
    const ref = out.body?.finalSectionProperties?.headerReferences?.[0];
    expect(ref?.type).toBe('default');
    expect(typeof ref?.rId).toBe('string');
    expect(out.body?.finalSectionProperties?.titlePage).toBeUndefined();
  });

  it('inserts a footer with the correct reference', () => {
    const out = applyHeaderFooter(
      { body: {} } as DocModel,
      'footer',
      { text: 'Page Footer' },
    ) as DocModel;
    expect(out.footers).toBeDefined();
    expect(out.body?.finalSectionProperties?.footerReferences).toHaveLength(1);
  });

  it('appends to existing header references rather than overwriting', () => {
    const input = {
      body: { finalSectionProperties: { headerReferences: [{ type: 'first', rId: 'rId-first' }] } },
    } as DocModel;
    const out = applyHeaderFooter(input, 'header', { text: 'Hello' }) as DocModel;
    const refs = out.body?.finalSectionProperties?.headerReferences ?? [];
    expect(refs).toHaveLength(2);
    expect(refs[0]?.type).toBe('first');
    expect(refs[1]?.type).toBe('default');
  });

  it('preserves existing entries in the headers map (Map form)', () => {
    const input = {
      body: {},
      headers: new Map<string, unknown>([['rId-existing', { type: 'header', kind: 'old' }]]),
    } as unknown as DocModel;
    const out = applyHeaderFooter(input, 'header', { text: 'New' }) as DocModel;
    expect(out.headers).toBeDefined();
    const map = out.headers as unknown as Map<string, unknown>;
    expect(map.has('rId-existing')).toBe(true);
    expect(map.size).toBe(2);
  });

  it('preserves existing entries in the headers map (object form)', () => {
    const input = {
      body: {},
      headers: { 'rId-existing': { type: 'header', kind: 'old' } },
    } as unknown as DocModel;
    const out = applyHeaderFooter(input, 'header', { text: 'New' }) as DocModel;
    // `applyHeaderFooter` rebuilds headers as a `Map` for downstream
    // consumption. Verify the existing entry is preserved.
    const map = out.headers as unknown as Map<string, unknown>;
    expect(map.has('rId-existing')).toBe(true);
    expect(map.size).toBe(2);
  });

  it('strips the "first" page header reference when insertBeforeFirstPage is true', () => {
    const input = {
      body: {
        finalSectionProperties: {
          headerReferences: [
            { type: 'first', rId: 'rId-first' },
            { type: 'default', rId: 'rId-default' },
          ],
          titlePage: true,
        },
      },
    } as DocModel;
    const out = applyHeaderFooter(input, 'header', {
      text: 'Cover',
      insertBeforeFirstPage: true,
    }) as DocModel;
    const refs = out.body?.finalSectionProperties?.headerReferences ?? [];
    expect(refs.some((r) => r.type === 'first')).toBe(false);
    expect(out.body?.finalSectionProperties?.titlePage).toBe(false);
  });

  it('does not mutate the input doc', () => {
    const input = { body: {} } as DocModel;
    applyHeaderFooter(input, 'header', { text: 'X' });
    expect(input.body?.finalSectionProperties?.headerReferences).toBeUndefined();
  });
});

describe('buildWatermarkSpec', () => {
  it('produces a watermark spec with all fields preserved', () => {
    const spec = buildWatermarkSpec({
      text: 'CONFIDENTIAL',
      font: 'Arial',
      color: '#ff0000',
      semitransparent: true,
      layout: 'diagonal',
      fontSize: 96,
    });
    expect(spec).toEqual({
      kind: 'text',
      text: 'CONFIDENTIAL',
      font: 'Arial',
      color: '#ff0000',
      semitransparent: true,
      layout: 'diagonal',
      fontSize: 96,
    });
  });

  it('passes through `horizontal` layout as-is', () => {
    const spec = buildWatermarkSpec({
      text: 'DRAFT',
      font: 'Calibri',
      color: '#cccccc',
      semitransparent: false,
      layout: 'horizontal',
      fontSize: 48,
    });
    expect(spec.layout).toBe('horizontal');
    expect(spec.semitransparent).toBe(false);
  });
});