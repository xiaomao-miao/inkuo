import { useCallback, useEffect } from 'react';
import type { ViewUpdate } from '@codemirror/view';
import type { ReactCodeMirrorRef } from '@uiw/react-codemirror';
import { useGlobalKeydown } from '../../hooks/useGlobalKeydown';
import { useEditorStore } from '../../store';

export function useEditorSelectionSync(
  selectedFile: string | null,
  currentContent: string,
  selection: { from: number; to: number } | null,
  editorRef: React.RefObject<ReactCodeMirrorRef | null>
) {
  const { setSelection } = useEditorStore();

  useEffect(() => {
    const view = editorRef.current?.view;
    if (!view || !selection) return;

    const currentSelection = view.state.selection.main;
    if (currentSelection.from !== selection.from || currentSelection.to !== selection.to) {
      view.dispatch({
        selection: { anchor: selection.from, head: selection.to },
        scrollIntoView: true,
        userEvent: 'kb.navigate',
      });
    }
  }, [selection, currentContent, editorRef]);

  return useCallback((viewUpdate: ViewUpdate) => {
    if (!viewUpdate.selectionSet || !selectedFile) return;

    const { from, to } = viewUpdate.state.selection.main;

    if (from !== to) {
      if (!selection || selection.from !== from || selection.to !== to) {
        setSelection(selectedFile, { from, to });
      }
      return;
    }

    if (selection) {
      setSelection(selectedFile, null);
    }
  }, [selectedFile, selection, setSelection]);
}

export function useEditorKeyboardShortcuts(
  selectedFile: string | null,
  handleSave: () => void,
  togglePreviewMode: (path: string) => void,
) {
  const handleGlobalKeyDown = useCallback((event: KeyboardEvent) => {
    if ((event.metaKey || event.ctrlKey) && event.key === 's') {
      event.preventDefault();
      handleSave();
    }

    if ((event.metaKey || event.ctrlKey) && event.shiftKey && event.key === 'p') {
      event.preventDefault();
      if (selectedFile) {
        togglePreviewMode(selectedFile);
      }
    }
  }, [handleSave, selectedFile, togglePreviewMode]);

  useGlobalKeydown(handleGlobalKeyDown);

  return useCallback(() => {
    if (selectedFile) {
      togglePreviewMode(selectedFile);
    }
  }, [selectedFile, togglePreviewMode]);
}
