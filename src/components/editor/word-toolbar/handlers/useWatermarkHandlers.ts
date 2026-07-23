// Subset of `useWordToolbarHandlers` for the watermark popover.
//
// The watermark popover needs three pieces:
//   1. The current watermark state from the document (`currentWatermark`)
//      — derived from the ProseMirror doc attrs. Surfaced to the
//      popover so it can pre-fill the form on open.
//   2. An `open` callback for the popover (`handleWatermark`).
//   3. A `confirm` callback that writes the chosen config (or clears
//      when `cfg` is `null`) back into the document.

import { useCallback, useMemo } from 'react';
import type { EditorView } from 'prosemirror-view';
import { setWatermark } from '@eigenpal/docx-editor-core/prosemirror/commands';

import { isViewReady, runCommand } from '../helpers';
import { buildWatermarkSpec } from './domMutations';
import type { WatermarkApply, WatermarkState } from './types';

export interface WatermarkHandlers {
  currentWatermark: WatermarkState | null;
  handleWatermark: () => void;
  handleWatermarkConfirm: (cfg: WatermarkApply | null) => void;
}

/**
 * Read the current watermark spec out of the document's `attrs`. The
 * editor core exposes the watermark as a structured attribute on the
 * doc node; this helper isolates the structural-cast dance so the
 * React hook stays declarative.
 */
export function currentWatermarkFromView(view: EditorView | null): WatermarkState | null {
  if (!isViewReady(view)) return null;
  try {
    const w = (view.state.doc as unknown as { attrs?: { watermark?: unknown } })
      .attrs?.watermark;
    if (!w || typeof w !== 'object') return null;
    const obj = w as { kind?: string; text?: string };
    if (obj.kind === 'text' && typeof obj.text === 'string') {
      return { kind: 'text' as const, text: obj.text };
    }
    if (obj.kind === 'picture') {
      return { kind: 'picture' as const };
    }
    return null;
  } catch {
    return null;
  }
}

export interface WatermarkHandlers {
  currentWatermark: WatermarkState | null;
  handleWatermark: () => void;
  handleWatermarkConfirm: (cfg: WatermarkApply | null) => void;
}

export interface WatermarkDeps {
  view: EditorView | null;
  openWatermark: () => void;
  closeWatermark: () => void;
}

export function useWatermarkHandlers({
  view,
  openWatermark,
  closeWatermark,
}: WatermarkDeps): WatermarkHandlers {
  const currentWatermark = useMemo<WatermarkState | null>(
    () => currentWatermarkFromView(view),
    [view],
  );

  const handleWatermark = useCallback(() => {
    if (!view) return;
    openWatermark();
  }, [view, openWatermark]);

  const handleWatermarkConfirm = useCallback(
    (cfg: WatermarkApply | null) => {
      closeWatermark();
      if (!view) return;
      if (cfg === null) {
        runCommand(view, setWatermark(null as unknown as never));
        return;
      }
      runCommand(view, setWatermark(buildWatermarkSpec(cfg) as unknown as never));
    },
    [view, closeWatermark],
  );

  return { currentWatermark, handleWatermark, handleWatermarkConfirm };
}