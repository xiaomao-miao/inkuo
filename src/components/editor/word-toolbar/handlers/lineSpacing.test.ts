// Unit tests for the line-spacing dispatch helper.
//
// `dispatchLineSpacing` itself depends on `runCommand`, which expects
// a ProseMirror view. The mapping (rawValue → command) is the
// interesting part — ProseMirror dispatch is delegated to the editor
// core commands which have their own coverage. Here we just verify
// the values exposed to the UI and the invalid-input behavior of the
// dispatcher (we'd need a real `EditorView` to test the live paths,
// which lives in the Word integration tests).

import { describe, expect, it, vi } from 'vitest';
import type { EditorView } from 'prosemirror-view';

import {
  dispatchLineSpacing,
  LINE_SPACING_OPTIONS,
  type LineSpacingCommand,
} from './lineSpacing';

describe('LINE_SPACING_OPTIONS', () => {
  it('exposes the three canonical values plus a custom-suggestion stub', () => {
    const values = LINE_SPACING_OPTIONS.map((opt) => opt.value);
    expect(values).toContain(1);
    expect(values).toContain(1.5);
    expect(values).toContain(2);
  });

  it('has a label for every option', () => {
    for (const opt of LINE_SPACING_OPTIONS) {
      expect(typeof opt.label).toBe('string');
      expect(opt.label.length).toBeGreaterThan(0);
    }
  });
});

describe('dispatchLineSpacing', () => {
  /** A minimal stub — `dispatchLineSpacing` only checks `view == null`. */
  function makeViewStub(): EditorView {
    return { state: null, dispatch: null } as unknown as EditorView;
  }

  it('returns false for non-finite numeric inputs', () => {
    expect(dispatchLineSpacing(makeViewStub(), NaN)).toBe(false);
    expect(dispatchLineSpacing(makeViewStub(), Infinity)).toBe(false);
  });

  it('returns false for zero or negative inputs', () => {
    expect(dispatchLineSpacing(makeViewStub(), 0)).toBe(false);
    expect(dispatchLineSpacing(makeViewStub(), -1)).toBe(false);
  });

  it('returns false for unparseable strings', () => {
    expect(dispatchLineSpacing(makeViewStub(), '')).toBe(false);
    expect(dispatchLineSpacing(makeViewStub(), 'abc')).toBe(false);
  });

  it('returns true for valid inputs even when view is null', () => {
    // `runCommand` is a no-op when `view` is null; the dispatcher
    // still returns true so the UI doesn't have to special-case.
    expect(dispatchLineSpacing(null, 1)).toBe(true);
    expect(dispatchLineSpacing(null, 1.5)).toBe(true);
    expect(dispatchLineSpacing(null, 2)).toBe(true);
    expect(dispatchLineSpacing(null, 3)).toBe(true); // custom
  });
});

// Type-level check (compile-time, no runtime cost).
function typeCheck(opt: LineSpacingCommand): LineSpacingCommand {
  return opt;
}
void typeCheck;

// Silence vi's unused-import warning when this file is reused in tests.
void vi;
