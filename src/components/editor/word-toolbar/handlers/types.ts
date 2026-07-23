// Shared types for the Word toolbar's handler bundle.
//
// The toolbar talks to the editor through a small interface that
// only exposes the operations the toolbar needs (undo / redo /
// read / load document). Keeping it narrow makes it easy to mock
// the editor in tests without depending on the editor-core types.

export type { HeaderFooterApply, HeaderFooterKind } from './domMutations';
export type { WatermarkApplyConfig as WatermarkApply } from './domMutations';

/**
 * Thin handle on the Word document editor. The editor handle
 * exposes undo / redo + document read / write — that's everything
 * the toolbar needs to coordinate with the document model.
 *
 * `getDocument` / `loadDocument` accept / return `unknown` at the
 * boundary because the editor core's structural type may include
 * extra fields; the toolbar's `applyXxx` helpers narrow to the
 * `DocModel` shape via `JSON.parse(JSON.stringify(...))`.
 *
 * `getDocument` / `loadDocument` are optional because some embed
 * contexts (e.g. settings previews) provide an editor without the
 * full mutation surface; the toolbar falls back gracefully.
 */
export interface EditorHandle {
  undo: () => boolean;
  redo: () => boolean;
  getDocument?: () => unknown;
  loadDocument?: (doc: unknown) => void;
  [key: string]: unknown;
}

/** Error/info notification — same shape used across all sub-hooks. */
export type Notify = (kind: 'error' | 'info', message: string) => void;

/**
 * Watermark state snapshot read from the document. Picture carries
 * no fields (the blob lives in the watermark attr); text carries
 * the displayed string so the popover can pre-fill its form.
 *
 * Matches the discriminated union shape used by `<WatermarkPopover>`
 * (`CurrentWatermark` in `popovers/WatermarkPopover.tsx`).
 */
export type WatermarkState =
  | { kind: 'text'; text: string }
  | { kind: 'picture' };