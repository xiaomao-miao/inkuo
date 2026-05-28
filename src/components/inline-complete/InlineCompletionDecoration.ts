import { EditorView, Decoration, ViewPlugin, ViewUpdate, WidgetType } from '@codemirror/view';
import { RangeSetBuilder } from '@codemirror/state';
import { useInlineCompleteStore } from '../../store';

/**
 * CodeMirror ViewPlugin that renders inline completion as a widget
 * anchored at the cursor position (so it scrolls correctly and never
 * overlaps existing document text).
 */
export function inlineCompletionDecoration() {
  return ViewPlugin.fromClass(
    class {
      decorations;

      constructor(view: EditorView) {
        this.decorations = this.buildDecorations(view);
      }

      update(update: ViewUpdate) {
        if (
          update.docChanged ||
          update.selectionSet ||
          update.viewportChanged ||
          update.focusChanged
        ) {
          this.decorations = this.buildDecorations(update.view);
        }
      }

      buildDecorations(view: EditorView) {
        const state = useInlineCompleteStore.getState();
        const completionText = state.currentCompletion?.text;
        if (!state.enabled || !completionText) return Decoration.none;

        const head = view.state.selection.main.head;
        if (state.triggerPosition != null && head !== state.triggerPosition) {
          return Decoration.none;
        }

        const builder = new RangeSetBuilder<Decoration>();

        class GhostWidget extends WidgetType {
          private text: string;
          constructor(text: string) {
            super();
            this.text = text;
          }

          eq(other: GhostWidget) {
            return other.text === this.text;
          }

          toDOM() {
            const span = document.createElement('span');
            span.className = 'cm-inline-completion-ghost';
            span.style.whiteSpace = 'pre';
            span.textContent = this.text;
            return span;
          }

          ignoreEvent() {
            return true;
          }
        }

        const deco = Decoration.widget({
          widget: new GhostWidget(completionText),
          side: 1,
        });

        builder.add(head, head, deco);
        return builder.finish();
      }
    },
    {
      decorations: (v) => v.decorations,
      eventHandlers: {
        blur() {
          // Hide on blur so it doesn't stick around when focus changes
          useInlineCompleteStore.getState().clearCompletion();
        },
      },
    }
  );
}
