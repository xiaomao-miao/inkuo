import { useCallback, useEffect, useRef } from 'react';
import type { ViewUpdate } from '@codemirror/view';
import type { ReactCodeMirrorRef } from '@uiw/react-codemirror';
import { useEditorStore } from '../../store';

export function useEditorSelectionSync(
  selectedFile: string | null,
  currentContent: string,
  editorRef: React.RefObject<ReactCodeMirrorRef | null>
) {
  const { setSelection } = useEditorStore();

  useEffect(() => {
    const view = editorRef.current?.view;
    if (!view) return;

    const docState = useEditorStore.getState().documentContents[selectedFile ?? ''];
    const selection = docState?.selection;
    if (!selection) return;

    const currentSelection = view.state.selection.main;
    if (currentSelection.from !== selection.from || currentSelection.to !== selection.to) {
      view.dispatch({
        selection: { anchor: selection.from, head: selection.to },
        scrollIntoView: true,
        userEvent: 'kb.navigate',
      });
    }
  }, [selectedFile, currentContent, editorRef]);

  return useCallback((viewUpdate: ViewUpdate) => {
    if (!viewUpdate.selectionSet || !selectedFile) return;

    const { from, to } = viewUpdate.state.selection.main;
    const currentDoc = useEditorStore.getState().documentContents[selectedFile];

    if (from !== to) {
      if (!currentDoc?.selection || currentDoc.selection.from !== from || currentDoc.selection.to !== to) {
        setSelection(selectedFile, { from, to });
      }
      return;
    }

    if (currentDoc?.selection) {
      setSelection(selectedFile, null);
    }
  }, [selectedFile, setSelection]);
}

export function useEditorKeyboardShortcuts(
  selectedFile: string | null,
  handleSave: () => void,
  togglePreviewMode: (path: string) => void,
) {
  const togglePreviewModeRef = useRef(togglePreviewMode);
  togglePreviewModeRef.current = togglePreviewMode;

  useEffect(() => {
    const handleGlobalKeyDown = (event: KeyboardEvent) => {
      if ((event.metaKey || event.ctrlKey) && event.key === 's') {
        event.preventDefault();
        handleSave();
      }

      if ((event.metaKey || event.ctrlKey) && event.shiftKey && event.key === 'p') {
        event.preventDefault();
        if (selectedFile) {
          togglePreviewModeRef.current(selectedFile);
        }
      }
    };

    window.addEventListener('keydown', handleGlobalKeyDown);
    return () => window.removeEventListener('keydown', handleGlobalKeyDown);
  }, [handleSave, selectedFile]);

  return useCallback(() => {
    if (selectedFile) {
      togglePreviewMode(selectedFile);
    }
  }, [selectedFile, togglePreviewMode]);
}
