// Pure state derivation for the Word toolbar.
//
// Reads from the current ProseMirror `EditorView` and exposes a stable
// bundle of "is this mark active / what is the current font / etc." facts.
// Pulled out of WordToolbar.tsx so the toolbar's JSX doesn't have to thread
// 10+ derived values through its render closure.

import { useMemo } from 'react';
import type { EditorView } from 'prosemirror-view';
import type { MarkType } from 'prosemirror-model';
import {
  getMarkAttr,
  getParagraphAlignment,
  getStyleId,
  isHyperlinkActive,
  isMarkActive,
} from '@eigenpal/docx-editor-core/prosemirror/commands';
import { hpToPt, rgbToHex } from './helpers';

export interface WordToolbarState {
  /** Schema-derived MarkType lookup keyed by short name. Null entries are missing marks. */
  markTypes: Record<string, MarkType | null> | null;
  /** Whether the schema is loaded; many queries gate on this. */
  schemaReady: boolean;

  isBold: boolean;
  isItalic: boolean;
  isUnderline: boolean;
  isStrike: boolean;
  isSuper: boolean;
  isSub: boolean;
  isLink: boolean;

  // `getParagraphAlignment` can return any ParagraphAlignment value (including
  // the rarer distribute / kashida / thai options) or `null` when the cursor
  // sits in a node that doesn't carry paragraph properties. Toolbar buttons
  // only care about the four common ones, so we widen the union to include
  // all legal values to stay type-compatible with the upstream API.
  alignment: ParagraphAlignmentValue | null;
  styleId: string | null;
  fontSizePt: number;
  fontFamily: string | null;
  fontColor: string;
}

const MARK_NAMES = [
  'bold', 'italic', 'underline', 'strike',
  'superscript', 'subscript',
  'fontSize', 'fontFamily', 'textColor', 'highlight',
  'hyperlink',
] as const;

const DEFAULT_FONT_SIZE_PT = 12;
const DEFAULT_FONT_COLOR = '#000000';

/**
 * Mirrors the upstream `ParagraphAlignment` union from `@eigenpal/docx-editor-core`.
 * Inlined here because the formatting types aren't part of the package's
 * public re-exports; if the upstream adds new options we'd update this
 * union in lockstep.
 */
export type ParagraphAlignmentValue =
  | 'left' | 'center' | 'right' | 'both'
  | 'distribute' | 'mediumKashida' | 'highKashida' | 'lowKashida' | 'thaiDistribute';

/**
 * Derive all the toolbar's display state from the current editor view.
 * The hook is intentionally read-only — it never dispatches transactions,
 * which means it's safe to call on every render without affecting the
 * editor's undo history.
 */
export function useWordToolbarState(view: EditorView | null): WordToolbarState {
  const markTypes = useMemo<Record<string, MarkType | null> | null>(() => {
    const s = view?.state.schema;
    if (!s) return null;
    const out: Record<string, MarkType | null> = {};
    for (const name of MARK_NAMES) {
      out[name] = s.marks[name] ?? null;
    }
    return out;
  }, [view]);

  const state = view?.state;
  const schemaReady = !!state?.schema;

  const isActive = (name: string): boolean => {
    const mt = markTypes?.[name];
    if (!schemaReady || !mt || !state) return false;
    return isMarkActive(state, mt);
  };

  const isLink = schemaReady && state ? isHyperlinkActive(state) : false;
  const alignment = schemaReady && state ? getParagraphAlignment(state) : null;
  const styleId = schemaReady && state ? getStyleId(state) : null;

  const fontSizeHp = markTypes?.fontSize && schemaReady && state
    ? getMarkAttr(state, markTypes.fontSize, 'size')
    : null;
  const fontFamily = markTypes?.fontFamily && schemaReady && state
    ? getMarkAttr(state, markTypes.fontFamily, 'ascii')
    : null;
  const textColor = markTypes?.textColor && schemaReady && state
    ? getMarkAttr(state, markTypes.textColor, 'rgb')
    : null;

  return {
    markTypes,
    schemaReady,

    isBold: isActive('bold'),
    isItalic: isActive('italic'),
    isUnderline: isActive('underline'),
    isStrike: isActive('strike'),
    isSuper: isActive('superscript'),
    isSub: isActive('subscript'),
    isLink,

    alignment,
    styleId,
    fontSizePt: hpToPt(fontSizeHp) ?? DEFAULT_FONT_SIZE_PT,
    fontFamily: fontFamily as string | null,
    fontColor: rgbToHex(textColor) ?? DEFAULT_FONT_COLOR,
  };
}