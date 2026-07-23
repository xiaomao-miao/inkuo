// Line-spacing helpers for the Word toolbar.
//
// The toolbar accepts an arbitrary numeric spacing value (typically
// 1, 1.5, 2, or a custom multiplier). The Word editor exposes
// dedicated commands for the three canonical values and a generic
// `setLineSpacing` for everything else. This module picks the right
// one for a given input.

import {
  doubleSpacing,
  oneAndHalfSpacing,
  setLineSpacing,
  singleSpacing,
} from '@eigenpal/docx-editor-core/prosemirror/commands';

import { runCommand } from '../helpers';
import type { EditorView } from 'prosemirror-view';

export interface LineSpacingCommand {
  /** Human-readable label, e.g. '单倍行距' / '1.5 倍行距'. */
  label: string;
  /** Multiplier, e.g. `1` / `1.5` / `2` / `2.5`. */
  value: number;
}

/**
 * Predefined values for the dropdown UI. The values align with the
 * commands in the editor core; passing any of these through
 * `dispatchLineSpacing` will hit the dedicated shortcut instead of
 * the generic command.
 */
export const LINE_SPACING_OPTIONS: ReadonlyArray<LineSpacingCommand> = [
  { label: '单倍', value: 1 },
  { label: '1.5 倍', value: 1.5 },
  { label: '2 倍', value: 2 },
  { label: '自定义', value: 2.5 },
];

/**
 * Map a numeric line-spacing value to the appropriate ProseMirror
 * command. Returns `false` (no dispatch) for invalid input so the
 * toolbar can ignore bad dropdown values.
 *
 *   1     → `singleSpacing`
 *   1.5   → `oneAndHalfSpacing`
 *   2     → `doubleSpacing`
 *   other → `setLineSpacing(n)`
 */
export function dispatchLineSpacing(view: EditorView | null, rawValue: string | number): boolean {
  const n = Number(rawValue);
  if (!Number.isFinite(n) || n <= 0) return false;
  if (n === 1) {
    runCommand(view, singleSpacing);
    return true;
  }
  if (n === 1.5) {
    runCommand(view, oneAndHalfSpacing);
    return true;
  }
  if (n === 2) {
    runCommand(view, doubleSpacing);
    return true;
  }
  runCommand(view, setLineSpacing(n));
  return true;
}