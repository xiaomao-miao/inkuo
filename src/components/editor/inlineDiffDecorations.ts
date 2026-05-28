import { RangeSetBuilder } from '@codemirror/state';
import { Decoration, EditorView } from '@codemirror/view';
import type { DiffHunk } from '../../store';

function tagToClass(tag: string) {
  if (tag === 'insert') return 'inkuoDiffInsert';
  if (tag === 'delete') return 'inkuoDiffDelete';
  return '';
}

export function buildInlineDiffDecorations(view: EditorView, hunks: DiffHunk[]) {
  const builder = new RangeSetBuilder<Decoration>();

  // CodeMirror uses 1-based line numbers.
  for (const hunk of hunks) {
    for (const change of hunk.changes) {
      const cls = tagToClass(change.tag);
      if (!cls) continue;

      const lineNo = change.tag === 'delete' ? change.old_line : change.new_line;
      if (!lineNo) continue;

      const line = view.state.doc.line(lineNo);
      builder.add(line.from, line.from, Decoration.line({ class: cls }));
    }
  }

  return builder.finish();
}

export const inlineDiffTheme = EditorView.baseTheme({
  '.inkuoDiffInsert': {
    backgroundColor: 'var(--diff-added-bg)',
  },
  '.inkuoDiffDelete': {
    backgroundColor: 'var(--diff-removed-bg)',
  },
});
