import { EditorView, keymap, lineNumbers, drawSelection, rectangularSelection } from '@codemirror/view';
import { Prec, type Extension } from '@codemirror/state';
import { markdown, markdownLanguage } from '@codemirror/lang-markdown';
import { languages } from '@codemirror/language-data';
import { historyKeymap } from '@codemirror/commands';
import { highlightSelectionMatches } from '@codemirror/search';
import { inlineDiffTheme } from './inlineDiffDecorations';
import { inlineCompletionDecoration } from '../inline-complete';
import { useInlineCompleteStore } from '../../store';

export function createEditorTheme() {
  return EditorView.theme({
    '&': {
      height: '100%',
      fontSize: '14px',
      backgroundColor: 'var(--bg-primary)',
    },
    '&.cm-editor': {
      backgroundColor: 'var(--bg-primary)',
    },
    '.cm-scroller': {
      fontFamily: 'var(--font-mono)',
      backgroundColor: 'var(--bg-primary)',
    },
    '.cm-content': {
      padding: '16px 0',
      backgroundColor: 'var(--bg-primary)',
    },
    '.cm-line': {
      padding: '0 16px',
    },
    '.cm-gutters': {
      backgroundColor: 'var(--bg-secondary)',
      borderRight: '1px solid var(--border-color)',
      color: 'var(--fg-muted)',
    },
    '.cm-lineNumbers .cm-gutterElement': {
      color: 'var(--fg-muted)',
      padding: '0 16px 0 8px',
    },
    '.cm-activeLineGutter': {
      backgroundColor: 'var(--bg-tertiary)',
      color: 'var(--fg-secondary)',
    },
  });
}

export function createInlineCompletionKeymap(autoTriggerStateRef: React.RefObject<{
  timer: ReturnType<typeof setTimeout> | null;
  lastAcceptAt: number;
  destroyed: boolean;
}>) {
  return Prec.highest(
    keymap.of([
      {
        key: 'Tab',
        run: (view) => {
          const { currentCompletion, clearCompletion } = useInlineCompleteStore.getState();
          if (!currentCompletion) return false;
          const cursorPosition = view.state.selection.main.head;
          const text = currentCompletion.text;

          if (autoTriggerStateRef.current) {
            autoTriggerStateRef.current.lastAcceptAt = Date.now();
          }

          clearCompletion();
          view.dispatch({
            changes: { from: cursorPosition, insert: text },
            selection: { anchor: cursorPosition + text.length },
            userEvent: 'input.complete',
          });
          return true;
        },
        preventDefault: true,
      },
      {
        key: 'Escape',
        run: () => {
          const { currentCompletion, clearCompletion } = useInlineCompleteStore.getState();
          if (!currentCompletion) return false;
          clearCompletion();
          return true;
        },
      },
    ])
  );
}

export function createEditorExtensions(params: {
  diffDecorationsField: Extension;
  inlineCompletionKeyHandler: Extension;
  inlineAutoTrigger: Extension;
  autoTriggerStateRef: React.RefObject<{
    timer: ReturnType<typeof setTimeout> | null;
    lastAcceptAt: number;
    destroyed: boolean;
  }>;
}) {
  return [
    markdown({ base: markdownLanguage, codeLanguages: languages }),
    lineNumbers(),
    drawSelection(),
    rectangularSelection(),
    highlightSelectionMatches(),
    inlineDiffTheme,
    params.diffDecorationsField,
    params.inlineCompletionKeyHandler,
    params.inlineAutoTrigger,
    createInlineCompletionKeymap(params.autoTriggerStateRef),
    inlineCompletionDecoration(),
    keymap.of([...historyKeymap]),
    createEditorTheme(),
  ];
}
