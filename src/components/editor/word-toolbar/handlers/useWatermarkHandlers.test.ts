// Unit tests for `currentWatermarkFromView`.
//
// The helper reads the document attrs and narrows the structural
// type to the discriminated union used by `<WatermarkPopover>`. We
// construct minimal `EditorView` shapes via a series of type casts
// since the helper only accesses `view.state.doc`.

import { describe, expect, it, vi } from 'vitest';
import type { EditorView } from 'prosemirror-view';

import { currentWatermarkFromView } from './useWatermarkHandlers';

function viewWithDoc(doc: unknown): EditorView {
  return {
    state: { doc, schema: {} } as unknown as EditorView['state'],
  } as unknown as EditorView;
}

function viewWithoutSchema(): EditorView {
  // `isViewReady` requires `state.schema` to be truthy. We craft an
  // unready view so the helper short-circuits without trying to
  // access `state.doc.attrs.watermark`.
  return { state: { schema: null } } as unknown as EditorView;
}

describe('currentWatermarkFromView', () => {
  it('returns null when the view is not ready (no schema)', () => {
    expect(currentWatermarkFromView(viewWithoutSchema())).toBeNull();
  });

  it('returns null when the view is null', () => {
    expect(currentWatermarkFromView(null)).toBeNull();
  });

  it('returns null when the doc has no attrs', () => {
    expect(currentWatermarkFromView(viewWithDoc({}))).toBeNull();
  });

  it('returns null when attrs.watermark is missing', () => {
    expect(currentWatermarkFromView(viewWithDoc({ attrs: {} }))).toBeNull();
  });

  it('returns the text watermark spec when the kind is "text"', () => {
    const view = viewWithDoc({ attrs: { watermark: { kind: 'text', text: 'DRAFT' } } });
    expect(currentWatermarkFromView(view)).toEqual({
      kind: 'text',
      text: 'DRAFT',
    });
  });

  it('returns the picture watermark spec when the kind is "picture"', () => {
    const view = viewWithDoc({ attrs: { watermark: { kind: 'picture' } } });
    expect(currentWatermarkFromView(view)).toEqual({ kind: 'picture' });
  });

  it('returns null for an unknown watermark kind', () => {
    const view = viewWithDoc({ attrs: { watermark: { kind: 'video', url: 'x' } } });
    expect(currentWatermarkFromView(view)).toBeNull();
  });

  it('returns null for a text watermark without a `text` field', () => {
    const view = viewWithDoc({ attrs: { watermark: { kind: 'text' } } });
    expect(currentWatermarkFromView(view)).toBeNull();
  });

  it('returns null when reading the doc throws', () => {
    // Hand-craft an object whose attribute getter throws. The
    // helper must catch this rather than crashing.
    const view = viewWithDoc({
      get attrs() {
        throw new Error('boom');
      },
    });
    // Suppress the error log the helper may emit during DEV.
    const spy = vi.spyOn(console, 'warn').mockImplementation(() => {});
    expect(currentWatermarkFromView(view)).toBeNull();
    spy.mockRestore();
  });
});
