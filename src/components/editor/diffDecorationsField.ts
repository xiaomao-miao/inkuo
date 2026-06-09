import { EditorView } from '@codemirror/view';
import { Decoration, type DecorationSet } from '@codemirror/view';
import { RangeSetBuilder, StateField } from '@codemirror/state';
import type { DiffHunk } from '../../types';

export function createDiffDecorationsField(diffHunks: DiffHunk[]) {
  return StateField.define<DecorationSet>({
    create() {
      return Decoration.none;
    },
    update(_decorations, tr) {
      const builder = new RangeSetBuilder<Decoration>();

      for (const hunk of diffHunks) {
        for (const change of hunk.changes) {
          const cls =
            change.tag === 'insert'
              ? 'inkuoDiffInsert'
              : change.tag === 'delete'
                ? 'inkuoDiffDelete'
                : '';

          if (!cls) continue;

          const lineNo = change.tag === 'delete' ? change.old_line : change.new_line;
          if (!lineNo) continue;

          try {
            const line = tr.state.doc.line(lineNo);
            builder.add(line.from, line.from, Decoration.line({ class: cls }));
          } catch {
            // line number out of range
          }
        }
      }

      return builder.finish();
    },
    provide: (field) => EditorView.decorations.from(field),
  });
}
