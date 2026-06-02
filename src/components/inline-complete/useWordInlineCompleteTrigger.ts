import { invoke } from '@tauri-apps/api/core';
import type { DocxEditorRef } from '@eigenpal/docx-editor-react';
import type { EditorView } from 'prosemirror-view';
import type { InlineCompletionRequest, InlineCompletionResponse, InlineStyle } from '../../types/inline-complete';
import { useInlineCompleteStore } from '../../store';
import { showWordInlineCompletion } from './wordInlineCompletePlugin';

function cancelWordInlineCompletion() {
  const now = Date.now();
  if (now - lastCancelInvokeTime < 120) return;
  lastCancelInvokeTime = now;

  try {
    void invoke('ai_inline_complete_cancel');
  } catch {
    // ignore
  }
}

function clampStyles(styles: InlineStyle[] | undefined, textLen: number): InlineStyle[] {
  if (!styles || styles.length === 0) return [];

  const normalized = styles
    .map((s) => {
      const start = Math.max(0, Math.min(textLen, s.start_offset ?? 0));
      const end = Math.max(start, Math.min(textLen, s.end_offset ?? textLen));
      return { ...s, start_offset: start, end_offset: end };
    })
    .filter((s) => (s.end_offset ?? 0) > (s.start_offset ?? 0));

  // Sort and drop overlaps (keep earlier segments, then truncate later ones).
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

/** Shared refs for the active Word editor instance. */
export interface WordInlineCompleteRefs {
  editorRef: React.RefObject<DocxEditorRef | null>;
  pmViewRef: React.MutableRefObject<EditorView | null>;
  filePath: string;
}

export const wordInlineRefs = {
  current: null as WordInlineCompleteRefs | null,
};

let completionTimer: ReturnType<typeof setTimeout> | null = null;
let requestSeq = 0;
let cancelSeq = 0;
let lastCancelInvokeTime = 0;
let lastAcceptTime = 0;

export function markAccepted() {
  lastAcceptTime = Date.now();
}

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

export function scheduleWordInlineCompletion(view: EditorView, filePath: string) {
  // Any new user input should cancel the previous completion request ASAP.
  cancelWordInlineCompletion();

  const store = useInlineCompleteStore.getState();
  if (!store.enabled || store.isLoading || store.currentCompletion) return;
  if (!view.hasFocus()) return;
  if (!view.state.selection.empty) return;
  if (Date.now() - lastAcceptTime < 300) return;

  // During IME composition, avoid scheduling requests.
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  if ((view as any).composing) return;

  if (completionTimer) {
    clearTimeout(completionTimer);
    completionTimer = null;
  }

  // Only keep the latest schedule; do not touch store state here.
  // This avoids causing heavy React re-renders during key-repeat (e.g. long-press Backspace).
  const { snippetText, cursorInSnippet, from } = getSnippet(view);
  const triggerHeadAtSchedule = view.state.selection.head;

  completionTimer = setTimeout(async () => {
    const latest = useInlineCompleteStore.getState();
    if (!latest.enabled || latest.isLoading || latest.currentCompletion) return;

    // Re-check focus/selection and cursor position at fire time
    if (!view.hasFocus()) return;
    if (!view.state.selection.empty) return;
    if (view.state.selection.head !== triggerHeadAtSchedule) return;

    const mySeq = ++requestSeq;
    const myCancelSeq = cancelSeq;

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

      // If a newer request started or we cancelled while waiting, ignore this response.
      if (mySeq !== requestSeq) return;
      if (myCancelSeq !== cancelSeq) return;

      const current = useInlineCompleteStore.getState();
      const headNow = view.state.selection.head;

      // Drop stale responses if cursor moved while waiting.
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
      if (mySeq === requestSeq) {
        useInlineCompleteStore.getState().setLoading(false);
      }
    }
  }, store.debounceMs);
}

export function clearWordTimers() {
  cancelSeq++;
  requestSeq++;
  cancelWordInlineCompletion();

  // Best-effort cancel: if a request is in-flight, we at least ensure
  // its response cannot update UI state.
  if (useInlineCompleteStore.getState().isLoading) {
    useInlineCompleteStore.getState().setLoading(false);
  }

  if (completionTimer) {
    clearTimeout(completionTimer);
    completionTimer = null;
  }
}
