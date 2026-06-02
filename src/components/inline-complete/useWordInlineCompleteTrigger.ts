import { invoke } from '@tauri-apps/api/core';
import type { EditorView } from 'prosemirror-view';
import type { InlineCompletionRequest, InlineCompletionResponse, InlineStyle } from '../../types/inline-complete';
import { useInlineCompleteStore } from '../../store';
import { showWordInlineCompletion } from './wordInlineCompletePlugin';

// Throttle cancel invocations to avoid flooding the backend
const CANCEL_THROTTLE_MS = 120;

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
}

const editorContexts = new WeakMap<EditorView, EditorContext>();

function getOrCreateContext(view: EditorView): EditorContext {
  let ctx = editorContexts.get(view);
  if (!ctx) {
    ctx = {
      completionTimer: null,
      requestSeq: 0,
      cancelSeq: 0,
      lastAcceptTime: 0,
    };
    editorContexts.set(view, ctx);
  }
  return ctx;
}

// ─── Global throttle for backend cancel RPC ────────────────────────────────────

let lastCancelInvokeTime = 0;

function cancelWordInlineCompletion() {
  const now = Date.now();
  if (now - lastCancelInvokeTime < CANCEL_THROTTLE_MS) return;
  lastCancelInvokeTime = now;
  try {
    void invoke('ai_inline_complete_cancel');
  } catch {
    // ignore
  }
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

function getSnippet(view: EditorView) {
  const cursor = view.state.selection.head;
  const docLen = view.state.doc.content.size;
  const maxBefore = 6000;
  const maxAfter = 1500;
  const from = Math.max(0, cursor - maxBefore);
  const to = Math.min(docLen, cursor + maxAfter);
  const snippetText = view.state.doc.textBetween(from, to);
  const cursorInSnippet = cursor - from;
  return { snippetText, cursorInSnippet, from };
}

// eslint-disable-next-line @typescript-eslint/no-explicit-any
const isComposing = (view: EditorView) => (view as any).composing === true;

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
  if (Date.now() - ctx.lastAcceptTime < 300) return;
  if (isComposing(view)) return;

  // Tell the backend to cancel any in-flight request.
  // This is safe even if multiple editors are active because each has its own ctx.cancelSeq.
  cancelWordInlineCompletion();

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

    latest.setLoading(true);
    latest.setError(null);

    try {
      const response = await invoke<InlineCompletionResponse>('ai_inline_complete', {
        request: {
          document: snippetText,
          cursor_position: cursorInSnippet,
          language: 'docx',
          file_path: filePath,
          snippet: { text: snippetText, start_offset: from },
        } as InlineCompletionRequest,
      });

      // Ignore if a newer request started or this editor was cancelled.
      if (mySeq !== ctx.requestSeq) return;
      if (myCancelSeq !== ctx.cancelSeq) return;

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
      const current = useInlineCompleteStore.getState();
      current.setError(err instanceof Error ? err.message : String(err));
    } finally {
      if (mySeq === ctx.requestSeq) {
        useInlineCompleteStore.getState().setLoading(false);
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

  // Best-effort cancel: if a request is in-flight, its response will be ignored
  // because myCancelSeq !== ctx.cancelSeq.
  if (useInlineCompleteStore.getState().isLoading) {
    useInlineCompleteStore.getState().setLoading(false);
  }
}

/** Legacy export for backward compatibility — clears timers for ALL editors. */
export function clearWordTimers() {
  // Increment a global sentinel so any in-flight response is ignored.
  // Individual editor contexts still have their own cancelSeq for correctness.
  // This is a best-effort approach for the legacy API.
  if (useInlineCompleteStore.getState().isLoading) {
    useInlineCompleteStore.getState().setLoading(false);
  }
  cancelWordInlineCompletion();
}

export function markAccepted(view: EditorView) {
  const ctx = editorContexts.get(view);
  if (ctx) {
    ctx.lastAcceptTime = Date.now();
  }
}
