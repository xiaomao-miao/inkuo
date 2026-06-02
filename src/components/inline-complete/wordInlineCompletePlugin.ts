import { Plugin, PluginKey, TextSelection } from 'prosemirror-state';
import { Decoration, DecorationSet, type EditorView } from 'prosemirror-view';
import { useInlineCompleteStore } from '../../store';
import type { InlineStyle } from '../../types/inline-complete';
import { markAccepted } from './useWordInlineCompleteTrigger';
import {
  toggleBold,
  toggleItalic,
  toggleUnderline,
  toggleStrike,
  setTextColor,
  setHighlight,
  setFontSize,
  setFontFamily,
} from '@eigenpal/docx-editor-core/prosemirror/commands';

export const wordInlineCompletePluginKey = new PluginKey<WordInlineCompletePluginState>('inkuoWordInlineComplete');

export interface WordInlineCompletePluginState {
  active: boolean;
  /** Anchor position (PM doc position) where ghost should render */
  anchorPos: number;
  /** Completion text to show */
  text: string;
  /** Stable id for decoration equality */
  id: string;
}

const emptyState: WordInlineCompletePluginState = {
  active: false,
  anchorPos: 0,
  text: '',
  id: '',
};

export function showWordInlineCompletion(view: EditorView, text: string) {
  const anchorPos = view.state.selection.head;
  const tr = view.state.tr.setMeta(wordInlineCompletePluginKey, {
    type: 'show',
    text,
    anchorPos,
  });
  view.dispatch(tr);
}

export function clearWordInlineCompletion(view: EditorView) {
  const tr = view.state.tr.setMeta(wordInlineCompletePluginKey, { type: 'clear' });
  view.dispatch(tr);
}

function buildDecorations(doc: import('prosemirror-model').Node, state: WordInlineCompletePluginState): DecorationSet {
  if (!state.active || !state.text) return DecorationSet.empty;

  const deco = Decoration.widget(
    state.anchorPos,
    () => {
      const span = document.createElement('span');
      span.className = 'inkuo-word-inline-completion-ghost';
      span.style.whiteSpace = 'pre-wrap';
      span.textContent = state.text;
      return span;
    },
    { side: 1 }
  );

  return DecorationSet.create(doc, [deco]);
}

function applyInlineStylesToInsertedText(view: EditorView, from: number, to: number, styles: InlineStyle[] | undefined) {
  if (!styles || styles.length === 0) return;

  // Apply per-range formatting by selecting the range then running commands.
  // Commands are provided by docx-editor-core and operate on EditorState.
  const applyForRange = (rangeFrom: number, rangeTo: number, s: InlineStyle) => {
    if (rangeFrom >= rangeTo) return;

    const trSel = view.state.tr.setSelection(TextSelection.create(view.state.doc, rangeFrom, rangeTo));
    view.dispatch(trSel);

    if (s.bold) toggleBold(view.state, view.dispatch, view);
    if (s.italic) toggleItalic(view.state, view.dispatch, view);
    if (s.underline) toggleUnderline(view.state, view.dispatch, view);
    if (s.strikethrough) toggleStrike(view.state, view.dispatch, view);

    if (s.color) {
      // docx-editor-core expects { rgb?: string }
      setTextColor({ rgb: String(s.color).replace('#', '') })(view.state, view.dispatch, view);
    }
    if (s.highlight) {
      setHighlight(String(s.highlight))(view.state, view.dispatch, view);
    }
    if (s.fontSize) {
      setFontSize(Number(s.fontSize))(view.state, view.dispatch, view);
    }
    if (s.fontFamily) {
      setFontFamily(String(s.fontFamily))(view.state, view.dispatch, view);
    }
  };

  for (const s of styles) {
    const start = Math.max(0, Math.min(to - from, s.start_offset ?? 0));
    const end = Math.max(start, Math.min(to - from, s.end_offset ?? (to - from)));
    applyForRange(from + start, from + end, s);
  }

  // Restore cursor to end of inserted text
  const restore = view.state.tr.setSelection(TextSelection.create(view.state.doc, to, to));
  view.dispatch(restore);
}

export function createWordInlineCompletePlugin(options: {
  onUserInput?: (view: EditorView) => void;
} = {}) {
  return new Plugin<WordInlineCompletePluginState>({
    key: wordInlineCompletePluginKey,

    state: {
      init() {
        return emptyState;
      },
      apply(tr, prev, _oldState, newState) {
        const meta = tr.getMeta(wordInlineCompletePluginKey);
        if (meta?.type === 'show') {
          return {
            active: true,
            anchorPos: meta.anchorPos ?? newState.selection.head,
            text: meta.text ?? '',
            id: `ghost-${Date.now()}-${Math.random().toString(36).slice(2)}`,
          };
        }
        if (meta?.type === 'clear') {
          return emptyState;
        }

        // If selection moved away from trigger position, hide (like md)
        if (prev.active) {
          const store = useInlineCompleteStore.getState();
          const currentHead = newState.selection.head;
          if (store.triggerPosition != null && currentHead !== store.triggerPosition) {
            return emptyState;
          }
        }

        // Map anchor through document changes
        if (prev.active && tr.docChanged) {
          const mapped = tr.mapping.map(prev.anchorPos);
          return { ...prev, anchorPos: mapped };
        }

        return prev;
      },
    },

    props: {
      decorations(state) {
        const pluginState = wordInlineCompletePluginKey.getState(state);
        return buildDecorations(state.doc, pluginState || emptyState);
      },
      handleKeyDown(view, event) {
        const store = useInlineCompleteStore.getState();
        const pluginState = wordInlineCompletePluginKey.getState(view.state) || emptyState;

        if (!pluginState.active || !store.currentCompletion) return false;

        if (event.key === 'Tab') {
          event.preventDefault();
          event.stopPropagation();

          const completion = store.currentCompletion;
          const text = completion.text;
          const head = view.state.selection.head;

          // Insert real text
          view.dispatch(view.state.tr.insertText(text, head, head));

          // Apply rich-text formatting if provided
          if (completion.styles && completion.styles.length > 0) {
            applyInlineStylesToInsertedText(view, head, head + text.length, completion.styles);
          }

          // Mark accept time to avoid immediate re-trigger
          markAccepted(view);

          // Clear store and plugin state
          useInlineCompleteStore.getState().clearCompletion();
          clearWordInlineCompletion(view);
          return true;
        }

        if (event.key === 'Escape') {
          event.preventDefault();
          event.stopPropagation();
          useInlineCompleteStore.getState().clearCompletion();
          clearWordInlineCompletion(view);
          return true;
        }

        // Any other typed key cancels ghost (like md: clear on new input)
        if (event.key.length === 1 || event.key === 'Enter' || event.key === 'Backspace' || event.key === 'Delete') {
          useInlineCompleteStore.getState().clearCompletion();
          clearWordInlineCompletion(view);
          return false;
        }

        return false;
      },
      handleDOMEvents: {
        compositionstart(view) {
          // During IME composition, hide existing ghost to avoid conflicts.
          const store = useInlineCompleteStore.getState();
          if (store.currentCompletion) {
            store.clearCompletion();
            clearWordInlineCompletion(view);
          }
          return false;
        },
      },
    },

    view(editorView) {
      let composing = false;

      const onCompositionStart = () => { composing = true; };
      const onCompositionEnd = () => { composing = false; };

      editorView.dom.addEventListener('compositionstart', onCompositionStart);
      editorView.dom.addEventListener('compositionend', onCompositionEnd);

      return {
        update(view, prevState) {
          const store = useInlineCompleteStore.getState();

          // If doc changed and we're not composing, treat as user input signal.
          const docChanged = prevState.doc !== view.state.doc;
          const selChanged = prevState.selection !== view.state.selection;

          if (!view.hasFocus()) return;

          // Hide ghost on selection change (cursor moved)
          if (selChanged && store.currentCompletion) {
            store.clearCompletion();
            clearWordInlineCompletion(view);
          }

          // If we're composing, never trigger. Also, keep any pending timer from firing.
          if (composing) return;

          if (!store.enabled) return;
          if (store.isLoading) return;
          if (store.currentCompletion) return;

          if (docChanged) {
            options.onUserInput?.(view);
          }

        },
        destroy() {
          editorView.dom.removeEventListener('compositionstart', onCompositionStart);
          editorView.dom.removeEventListener('compositionend', onCompositionEnd);
        }
      };
    },
  });
}
