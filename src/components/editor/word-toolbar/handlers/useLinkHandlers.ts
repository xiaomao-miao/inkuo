// Subset of `useWordToolbarHandlers` for the link popover wiring.
//
// The toolbar's link flow:
//   1. User clicks "Insert link" → `handleInsertLink` snapshots the
//      current window selection text and signals the parent to open
//      `<LinkPopover>` with that as the seed text.
//   2. The popover edits the text + URL → user confirms.
//   3. `handleLinkConfirm` writes the new link back into the document:
//      if the user replaced the selected text, dispatch the replace +
//      a fresh `insertHyperlink` at the (possibly collapsed) selection.
//      Otherwise, dispatch `setHyperlink(url)` against the existing
//      selection.
//
// `handleRemoveLink` lives in `useBasicCommandHandlers` because it's
// a pure command dispatch (no popover state to coordinate).

import { useCallback } from 'react';
import type { EditorView } from 'prosemirror-view';
import {
  insertHyperlink,
  setHyperlink,
} from '@eigenpal/docx-editor-core/prosemirror/commands';

import { isViewReady, runCommand } from '../helpers';
import { extractSelectionText } from './selection';

export interface LinkHandlers {
  handleInsertLink: () => void;
  handleLinkConfirm: (url: string, displayText: string) => void;
}

export interface LinkDeps {
  view: EditorView | null;
  /** Whether the current selection is already a link — lets the popover know to edit vs insert. */
  isLink: boolean;
  /** Parent's popover open / close callbacks. */
  openLink: (payload: { initialText: string; isEditingExisting: boolean }) => void;
  closeLink: () => void;
}

export function useLinkHandlers({ view, isLink, openLink, closeLink }: LinkDeps): LinkHandlers {
  const handleInsertLink = useCallback(() => {
    openLink({ initialText: extractSelectionText(), isEditingExisting: isLink });
  }, [isLink, openLink]);

  const handleLinkConfirm = useCallback(
    (url: string, displayText: string) => {
      closeLink();
      if (!isViewReady(view)) return;
      const { from, to } = view.state.selection;
      // If the user changed the displayed text in the popover, replace
      // the selected range first; otherwise keep the original selection
      // so `setHyperlink` can apply the link mark to existing text.
      if (
        from !== to &&
        view.state.doc.textBetween(from, to, '\n', '\n') !== displayText
      ) {
        view.dispatch(view.state.tr.insertText(displayText, from, to));
      }
      // After a text replace the selection might have collapsed; recompute.
      const sel2 = view.state.selection;
      if (sel2.from === sel2.to) {
        runCommand(view, insertHyperlink(url, displayText));
      } else {
        runCommand(view, setHyperlink(url));
      }
      view.focus();
    },
    [view, closeLink],
  );

  return { handleInsertLink, handleLinkConfirm };
}