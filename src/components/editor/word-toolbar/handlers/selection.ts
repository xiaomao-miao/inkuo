// Selection helpers used by the Word toolbar's sort, link, math,
// and line-spacing handlers.
//
// The Word toolbar operates on the ProseMirror `view` directly, so
// most "selection helpers" are thin convenience wrappers around
// `view.state.selection` / `view.state.tr`. We isolate them here so
// the toolbar's React hooks can stay focused on popover wiring.

import type { EditorView } from 'prosemirror-view';
import type { Node as PMNode } from 'prosemirror-model';

import { isViewReady } from '../helpers';

/**
 * Read the current `Selection` from a global. The default reads from
 * `window`, but tests / SSR contexts can pass an explicit `selection`
 * (e.g. `globalThis`) so they don't need a DOM.
 */
export interface GlobalSelection {
  getSelection: () => { toString(): string } | null;
}

/**
 * Internal: `window.getSelection?.()?.toString() ?? ''`. Pure (no
 * React), but reads the global. Callers should pass `globalThis` from
 * tests; the production caller (`useLinkHandlers`) reads `window`
 * directly via `defaultGlobal()`.
 */
export function readSelectionText(global: GlobalSelection = defaultGlobal()): string {
  const s = global.getSelection();
  return s?.toString() ?? '';
}

/**
 * Default `GlobalSelection` reading from `window` when available.
 * Returns a stub when `window` is undefined (SSR / node test envs)
 * so the helper doesn't crash at import time.
 */
function defaultGlobal(): GlobalSelection {
  if (typeof window === 'undefined') {
    return { getSelection: () => null };
  }
  return { getSelection: () => window.getSelection() };
}

/**
 * Public façade for production callers. Equivalent to
 * `readSelectionText(defaultGlobal())`. Kept as a thin wrapper so
 * consumers see a stable name; tests can import `readSelectionText`
 * directly with a stub global.
 */
export function extractSelectionText(): string {
  return readSelectionText();
}

/**
 * Walk `from..to` and collect each top-level textblock with the slice
 * of its text that intersects the selection. Skips leaf / non-block
 * nodes by returning `true` from the callback. Used by the sort
 * handler to build the input list.
 */
export interface SortableLine {
  /** Absolute position of the paragraph start. */
  pos: number;
  /** The textblock node itself. */
  node: PMNode;
  /** Clipped start position (>= `from`). */
  start: number;
  /** Clipped end position (<= `to`). */
  end: number;
  /** Clipped text content. */
  text: string;
}

export function collectSortableTextblocks(
  view: EditorView,
  from: number,
  to: number,
): SortableLine[] {
  if (!isViewReady(view)) return [];
  const collected: SortableLine[] = [];
  view.state.doc.nodesBetween(from, to, (node, pos) => {
    if (node.isTextblock) {
      const start = Math.max(pos, from);
      const end = Math.min(pos + node.nodeSize, to);
      const text = node.textBetween(start - pos, end - pos, '\n', '\n');
      collected.push({ pos, node, start, end, text });
    }
    return true;
  });
  return collected;
}

/**
 * Compare two lines by text content using the Chinese-locale
 * collator. The toolbar's sort UI labels it "智能排序" so we default
 * to `zh-Hans-CN` but the same comparator works fine for English.
 */
export function compareLinesAsc(a: SortableLine, b: SortableLine): number {
  return a.text.localeCompare(b.text, 'zh-Hans-CN');
}

export function compareLinesDesc(a: SortableLine, b: SortableLine): number {
  return b.text.localeCompare(a.text, 'zh-Hans-CN');
}