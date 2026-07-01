import { invoke } from '@tauri-apps/api/core';
import type { EditorView } from 'prosemirror-view';
import type { InlineCompletionRequest, InlineCompletionResponse, InlineStyle } from '../../types/inline-complete';
import { useInlineCompleteStore } from '../../store';
import { reportError } from '../../utils/errors';
import { showWordInlineCompletion } from './wordInlineCompletePlugin';
import { TIMING, PROSEMIRROR_SNIPPET_BOUNDS } from '../../constants/timing';

function clampStyles(styles: InlineStyle[] | undefined, textLen: number): InlineStyle[] {
  if (!styles || styles.length === 0) return [];

  const normalized = styles
    .map((s) => {
      const start = Math.max(0, Math.min(textLen, s.start_offset ?? 0));
      const end = Math.max(start, Math.min(textLen, s.end_offset ?? textLen));
      return { ...s, start_offset: start, end_offset: end };
    })
    .filter((s) => (s.end_offset ?? 0) > (s.start_offset ?? 0));

  normalized.sort((a, b) => (a.start_offset ?? 0) - (b.start_offset ?? 0));
  const out: InlineStyle[] = [];
  let cursor = 0;
  for (const s of normalized) {
    const sStart = s.start_offset ?? 0;
    const sEnd = s.end_offset ?? 0;
    if (sEnd <= cursor) continue;
    const start = Math.max(sStart, cursor);
    const end = sEnd;
    out.push({ ...s, start_offset: start, end_offset: end });
    cursor = end;
    if (cursor >= textLen) break;
  }
  return out;
}

// ─── Per-editor context via WeakMap ──────────────────────────────────────────

interface EditorContext {
  completionTimer: ReturnType<typeof setTimeout> | null;
  requestSeq: number;
  cancelSeq: number;
  lastAcceptTime: number;
  /// Backend-side request id of the in-flight call (if any). Used to route
  /// the cancel command to a specific request so cancelling one editor's
  /// completion does not abort completions running in other windows.
  inFlightRequestId: string | null;
}

const editorContexts = new WeakMap<EditorView, EditorContext>();
/// Best-effort iteration list for `dismissAllWordCompletions`. WeakMap does
/// not expose enumeration, but the entries are kept alive only as long as
/// the view itself is referenced by the editor — when a `WordEditor` unmounts
/// the view (and therefore its entry) is dropped and this Set's reference is
/// the only thing keeping it alive. We clean up on access in `dismissAllWordCompletions`
/// by checking for `view.isDestroyed` and dropping dead entries.
const trackedViews = new Set<EditorView>();

function getOrCreateContext(view: EditorView): EditorContext {
  let ctx = editorContexts.get(view);
  if (!ctx) {
    ctx = {
      completionTimer: null,
      requestSeq: 0,
      cancelSeq: 0,
      lastAcceptTime: 0,
      inFlightRequestId: null,
    };
    editorContexts.set(view, ctx);
    trackedViews.add(view);
  }
  return ctx;
}

// ─── Targeted cancel for a specific backend request ───────────────────────────

/** Wake the backend's cancel channel for a specific in-flight request id.
 * Best-effort: errors are logged, not surfaced, because the request may
 * already have completed by the time the cancel lands. */
function cancelBackendRequest(requestId: string | null) {
  if (!requestId) return;
  void invoke('ai_inline_complete_cancel', { requestId })
    .catch((err) => {
      console.warn('[WordInlineCompletion] Cancel request failed:', err);
    });
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

function getSnippet(view: EditorView) {
  const cursor = view.state.selection.head;
  const docLen = view.state.doc.content.size;
  const from = Math.max(0, cursor - PROSEMIRROR_SNIPPET_BOUNDS.MAX_BEFORE);
  const to = Math.min(docLen, cursor + PROSEMIRROR_SNIPPET_BOUNDS.MAX_AFTER);
  const snippetText = view.state.doc.textBetween(from, to);
  const cursorInSnippet = cursor - from;
  return { snippetText, cursorInSnippet, from };
}

// ProseMirror's EditorView has a 'composing' property not in the type definitions
// This is used to detect IME composition state
const isComposing = (view: EditorView) => {
  const viewWithComposing = view as EditorView & { composing?: boolean };
  return viewWithComposing.composing === true;
};

// ─── Public API ───────────────────────────────────────────────────────────────

export function scheduleWordInlineCompletion(view: EditorView, filePath: string) {
  const ctx = getOrCreateContext(view);

  // Any new user input cancels the previous request for THIS editor.
  if (ctx.completionTimer !== null) {
    clearTimeout(ctx.completionTimer);
    ctx.completionTimer = null;
  }

  const store = useInlineCompleteStore.getState();
  if (!store.enabled || store.isLoading || store.currentCompletion) return;
  if (!view.hasFocus()) return;
  if (!view.state.selection.empty) return;
  if (Date.now() - ctx.lastAcceptTime < TIMING.COMPLETION_RETRIGGER_DELAY_MS) return;
  if (isComposing(view)) return;

  // If a previous request for this editor is still in-flight on the backend,
  // target it specifically so we don't disturb other editors' requests.
  cancelBackendRequest(ctx.inFlightRequestId);
  ctx.inFlightRequestId = null;

  const { snippetText, cursorInSnippet, from } = getSnippet(view);
  const triggerHeadAtSchedule = view.state.selection.head;

  ctx.completionTimer = setTimeout(async () => {
    ctx.completionTimer = null;

    const latest = useInlineCompleteStore.getState();
    if (!latest.enabled || latest.isLoading || latest.currentCompletion) return;
    if (!view.hasFocus()) return;
    if (!view.state.selection.empty) return;
    if (view.state.selection.head !== triggerHeadAtSchedule) return;

    const mySeq = ++ctx.requestSeq;
    const myCancelSeq = ctx.cancelSeq;
    // Mint a fresh id per call. Including the editor's `requestSeq` keeps
    // it unique even if two completions race from the same editor; including
    // the file path keeps ids distinct across the two windows of a multi-
    // window setup so the backend registry never sees a collision.
    const requestId = `${filePath}#${Date.now()}#${mySeq}`;

    latest.setLoading(true);
    latest.setError(null);
    ctx.inFlightRequestId = requestId;

    try {
      const response = await invoke<InlineCompletionResponse>('ai_inline_complete', {
        request: {
          request_id: requestId,
          document: snippetText,
          cursor_position: cursorInSnippet,
          language: 'docx',
          file_path: filePath,
          snippet: { text: snippetText, start_offset: from },
        } as InlineCompletionRequest,
      });

      // Ignore if a newer request started or this editor was cancelled.
      if (mySeq !== ctx.requestSeq) {
        return;
      }
      if (myCancelSeq !== ctx.cancelSeq) {
        return;
      }
      if (ctx.inFlightRequestId === requestId) {
        ctx.inFlightRequestId = null;
      }

      const current = useInlineCompleteStore.getState();
      const headNow = view.state.selection.head;

      // Drop stale response if cursor moved while waiting.
      if (headNow !== triggerHeadAtSchedule) return;

      if (!current.currentCompletion && response.completions.length > 0) {
        const item = response.completions[0];
        const text = item.text;
        const safeStyles = clampStyles(item.styles, text.length);
        const normalizedItem =
          safeStyles.length > 0 ? { ...item, styles: safeStyles } : { ...item, styles: undefined };

        current.setCompletion(normalizedItem, headNow);
        showWordInlineCompletion(view, text);
      }
    } catch (err) {
      const message = reportError('word-inline-complete-request', err);
      const current = useInlineCompleteStore.getState();
      current.setError(message);
    } finally {
      if (mySeq === ctx.requestSeq) {
        useInlineCompleteStore.getState().setLoading(false);
        if (ctx.inFlightRequestId === requestId) {
          ctx.inFlightRequestId = null;
        }
      }
    }
  }, store.debounceMs);
}

/** Clear timers for a specific editor (call when focus leaves or user dismisses). */
export function clearWordTimersForEditor(view: EditorView) {
  const ctx = editorContexts.get(view);
  if (!ctx) return;

  ctx.cancelSeq++;
  ctx.requestSeq++;

  if (ctx.completionTimer !== null) {
    clearTimeout(ctx.completionTimer);
    ctx.completionTimer = null;
  }

  cancelBackendRequest(ctx.inFlightRequestId);
  ctx.inFlightRequestId = null;

  // Best-effort cancel: if a request is in-flight, its response will be ignored
  // because myCancelSeq !== ctx.cancelSeq.
  if (useInlineCompleteStore.getState().isLoading) {
    useInlineCompleteStore.getState().setLoading(false);
  }
}

/** Dismiss all pending inline completions globally.
 * Cancels any in-flight backend request and clears the loading state.
 * Note: This does NOT clear per-editor timers — use clearWordTimersForEditor for that.
 */
export function dismissAllWordCompletions() {
  if (useInlineCompleteStore.getState().isLoading) {
    useInlineCompleteStore.getState().setLoading(false);
  }
  for (const view of trackedViews) {
    const ctx = editorContexts.get(view);
    if (!ctx) {
      trackedViews.delete(view);
      continue;
    }
    if (view.isDestroyed) {
      trackedViews.delete(view);
      editorContexts.delete(view);
      continue;
    }
    cancelBackendRequest(ctx.inFlightRequestId);
    ctx.inFlightRequestId = null;
  }
}

/** @deprecated Use dismissAllWordCompletions instead. Alias for backward compatibility. */
export const clearWordTimers = dismissAllWordCompletions;

export function markAccepted(view: EditorView) {
  const ctx = editorContexts.get(view);
  if (ctx) {
    ctx.lastAcceptTime = Date.now();
  }
}
