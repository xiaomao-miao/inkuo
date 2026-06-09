import { useEffect, useMemo, useRef } from 'react';
import { EditorView } from '@codemirror/view';
import { useInlineComplete, inlineCompletionDecoration } from '../inline-complete';
import { useInlineCompleteStore, useSidebarStore } from '../../store';
import { TIMING, CODEMIRROR_SNIPPET_BOUNDS } from '../../constants/timing';
import { detectLanguage } from '../../types/inline-complete';

export interface InlineAutoTriggerState {
  timer: ReturnType<typeof setTimeout> | null;
  lastAcceptAt: number;
  destroyed: boolean;
}

export function createInlineCompletionKeyHandler() {
  return EditorView.domEventHandlers({
    keydown(event, view) {
      if (!view) return false;

      const storeState = useInlineCompleteStore.getState();
      const { currentCompletion, clearCompletion } = storeState;

      if (event.key === 'Tab' && currentCompletion) {
        event.preventDefault();
        event.stopPropagation();

        const cursorPosition = view.state.selection.main.head;
        const text = currentCompletion.text;

        clearCompletion();
        view.dispatch({
          changes: { from: cursorPosition, insert: text },
          selection: { anchor: cursorPosition + text.length },
          userEvent: 'input.complete',
        });
        return true;
      }

      if (event.key === 'Escape' && currentCompletion) {
        event.preventDefault();
        clearCompletion();
        return true;
      }

      return false;
    },
  });
}

export function useEditorInlineCompletion(editorRef: React.RefObject<{ view?: EditorView | null } | null>) {
  const { triggerCompletion } = useInlineComplete();
  const triggerCompletionRef = useRef(triggerCompletion);
  const lastSelectedFileRef = useRef<string | null>(null);
  const autoTriggerStateRef = useRef<InlineAutoTriggerState>({
    timer: null,
    lastAcceptAt: 0,
    destroyed: false,
  });

  triggerCompletionRef.current = triggerCompletion;

  const selectedFile = useSidebarStore((state) => state.selectedFile);

  useEffect(() => {
    if (selectedFile !== lastSelectedFileRef.current) {
      lastSelectedFileRef.current = selectedFile;
      useInlineCompleteStore.getState().clearCompletion();
    }
  }, [selectedFile]);

  useEffect(() => {
    const ref = autoTriggerStateRef.current;
    return () => {
      ref.destroyed = true;
      if (ref.timer !== null) {
        clearTimeout(ref.timer);
        ref.timer = null;
      }
    };
  }, []);

  useEffect(() => {
    const onPointerDown = (event: PointerEvent) => {
      const view = editorRef.current?.view;
      if (!view) return;
      if (!view.dom.contains(event.target as Node)) {
        useInlineCompleteStore.getState().clearCompletion();
        if (autoTriggerStateRef.current.timer) {
          clearTimeout(autoTriggerStateRef.current.timer);
          autoTriggerStateRef.current.timer = null;
        }
      }
    };

    window.addEventListener('pointerdown', onPointerDown, true);
    return () => window.removeEventListener('pointerdown', onPointerDown, true);
  }, [editorRef]);

  const inlineAutoTrigger = useMemo(() => EditorView.updateListener.of((update) => {
    const view = update.view;

    if (!view.hasFocus) return;

    const now = Date.now();
    if (now - autoTriggerStateRef.current.lastAcceptAt < TIMING.COMPLETION_RETRIGGER_DELAY_MS) return;

    const isUserInput = update.transactions.some(
      (transaction) =>
        transaction.isUserEvent('input') ||
        transaction.isUserEvent('input.type') ||
        transaction.isUserEvent('delete')
    );
    if (!isUserInput) return;

    const storeState = useInlineCompleteStore.getState();
    if (storeState.currentCompletion) {
      useInlineCompleteStore.getState().clearCompletion();
    }

    const selection = view.state.selection.main;
    if (!selection.empty) return;
    if (!storeState.enabled || storeState.isLoading) return;

    if (autoTriggerStateRef.current.timer) {
      clearTimeout(autoTriggerStateRef.current.timer);
    }

    const filePath = useSidebarStore.getState().selectedFile;
    autoTriggerStateRef.current.timer = setTimeout(() => {
      autoTriggerStateRef.current.timer = null;
      if (autoTriggerStateRef.current.destroyed) return;
      if (!view.hasFocus) return;

      const latestSelection = view.state.selection.main;
      if (!latestSelection.empty) return;

      const latestStore = useInlineCompleteStore.getState();
      if (!latestStore.enabled || latestStore.isLoading || latestStore.currentCompletion) return;

      const docLength = view.state.doc.length;
      const cursor = latestSelection.head;
      const from = Math.max(0, cursor - CODEMIRROR_SNIPPET_BOUNDS.MAX_BEFORE);
      const to = Math.min(docLength, cursor + CODEMIRROR_SNIPPET_BOUNDS.MAX_AFTER);
      const snippetText = view.state.doc.sliceString(from, to);
      const cursorInSnippet = cursor - from;

      triggerCompletionRef.current({
        document: snippetText,
        cursorPosition: cursorInSnippet,
        language: detectLanguage(filePath || undefined),
        filePath: filePath || undefined,
        snippet: { text: snippetText, start_offset: from },
      });
    }, storeState.debounceMs);
  }), []);

  return {
    autoTriggerStateRef,
    inlineAutoTrigger,
    inlineCompletionKeyHandler: useMemo(() => createInlineCompletionKeyHandler(), []),
    inlineCompletionDecoration,
  };
}
