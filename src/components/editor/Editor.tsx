import React, { useCallback, useEffect, useRef, useState } from 'react';
import CodeMirror, { ReactCodeMirrorRef } from '@uiw/react-codemirror';
import { markdown, markdownLanguage } from '@codemirror/lang-markdown';
import { languages } from '@codemirror/language-data';
import { EditorView, keymap, lineNumbers, drawSelection, rectangularSelection } from '@codemirror/view';
import { defaultKeymap, historyKeymap } from '@codemirror/commands';
import { searchKeymap, highlightSelectionMatches } from '@codemirror/search';
import { invoke } from '@tauri-apps/api/core';
import { Sparkles } from 'lucide-react';
import { useEditorStore, useSidebarStore, useInlineCompleteStore } from '../../store';
import { SETTINGS_TAB_ID } from '../../store';
import { DiffOverlay } from './DiffOverlay';
import { SettingsPanel } from '../settings/SettingsPanel';
import { InlineCompleteProvider, useInlineComplete, GhostTextOverlay, InlineCompleteStatus } from '../inline-complete';
import { detectLanguage } from '../../types/inline-complete';
import styles from './Editor.module.css';

// ============================================================================
// Editor Content Component (inside Provider) - CAN use useInlineComplete
// ============================================================================
const EditorContent: React.FC<{
  editorRef: React.RefObject<ReactCodeMirrorRef | null>;
  setEditorState: React.Dispatch<React.SetStateAction<{ document: string; cursorPosition: number }>>;
}> = ({ editorRef, setEditorState }) => {
  const {
    documentContents,
    setDocumentContent,
    setContent,
    setSelection,
    markSaved,
    updateTabDirty,
  } = useEditorStore();
  const { selectedFile } = useSidebarStore();
  const { triggerCompletion } = useInlineComplete(); // Now inside Provider!

  // Get current document state from store
  const currentDoc = selectedFile ? documentContents[selectedFile] : null;
  const currentContent = currentDoc?.content || '';
  const isDirty = currentDoc?.isDirty || false;
  const diffHunks = currentDoc?.diffHunks || [];
  const isDiffMode = currentDoc?.isDiffMode || false;
  const selection = currentDoc?.selection || null;

  // Auto-trigger completion when typing (debounced)
  useEffect(() => {
    if (!selectedFile || !currentContent) return;

    const timer = setTimeout(() => {
      const view = editorRef.current?.view;
      if (!view) return;

      const cursorPosition = view.state.selection.main.head;
      const { isLoading, currentCompletion, enabled, triggerPosition } = useInlineCompleteStore.getState();

      // Don't trigger if loading or already has a completion at this position
      if (isLoading) return;
      if (currentCompletion && triggerPosition === cursorPosition) return;
      if (enabled) {
        console.log('[Editor] Auto-triggering completion at position', cursorPosition);
        triggerCompletion({
          document: currentContent,
          cursorPosition,
          language: detectLanguage(selectedFile || undefined),
          filePath: selectedFile,
        });
      }
    }, 800); // Increased debounce to 800ms to reduce API calls

    return () => clearTimeout(timer);
  }, [currentContent, selectedFile, triggerCompletion, editorRef]);

  // Load document when file is selected
  useEffect(() => {
    const loadDocument = async () => {
      if (!selectedFile) return;

      try {
        const result = await invoke<{ document: any; content: string }>('read_document', {
          path: selectedFile,
        });
        setDocumentContent(selectedFile, result.document, result.content);
      } catch (err) {
        console.error('Failed to load document:', err);
      }
    };

    loadDocument();
  }, [selectedFile, setDocumentContent]);

  // Save document
  const handleSave = useCallback(async () => {
    if (!selectedFile || !isDirty) return;

    try {
      await invoke('write_document', {
        path: selectedFile,
        content: currentContent,
      });
      markSaved(selectedFile);
      updateTabDirty(selectedFile, false);
    } catch (err) {
      console.error('Failed to save document:', err);
    }
  }, [selectedFile, currentContent, isDirty, markSaved, updateTabDirty]);

  const handleChange = useCallback((value: string) => {
    if (selectedFile) {
      setContent(selectedFile, value);
      const view = editorRef.current?.view;
      if (view) {
        setEditorState({
          document: value,
          cursorPosition: view.state.selection.main.head,
        });
      }
    }
  }, [selectedFile, setContent, editorRef, setEditorState]);

  const handleUpdate = useCallback((viewUpdate: any) => {
    if (viewUpdate.selection && selectedFile) {
      const { from, to } = viewUpdate.state.selection.main;
      if (from !== to) {
        setSelection(selectedFile, { from, to });
      } else {
        setSelection(selectedFile, null);
      }
    }

    const view = viewUpdate.view;
    if (view) {
      setEditorState({
        document: view.state.doc.toString(),
        cursorPosition: view.state.selection.main.head,
      });
    }
  }, [selectedFile, setSelection, setEditorState]);

  // Keyboard shortcuts handler
  const handleKeyDown = useCallback((e: KeyboardEvent) => {
    const view = editorRef.current?.view;
    if (!view) return;

    const { currentCompletion, clearCompletion } = useInlineCompleteStore.getState();

    // Cmd/Ctrl+S - Save
    if ((e.metaKey || e.ctrlKey) && e.key === 's') {
      e.preventDefault();
      handleSave();
      return;
    }

    // Tab - Accept existing completion
    if (e.key === 'Tab' && currentCompletion) {
      e.preventDefault();
      const completion = currentCompletion;
      const cursorPosition = view.state.selection.main.head;
      clearCompletion();
      view.dispatch({
        changes: { from: cursorPosition, insert: completion.text },
        selection: { anchor: cursorPosition + completion.text.length },
      });
      return;
    }

    // Escape - Dismiss completion
    if (e.key === 'Escape' && currentCompletion) {
      e.preventDefault();
      clearCompletion();
      return;
    }

    // Diff mode: Tab to apply hunk
    if (e.key === 'Tab' && isDiffMode) {
      e.preventDefault();
      const { applyHunk } = useEditorStore.getState();
      const currentDoc = documentContents[selectedFile || ''];
      if (currentDoc?.diffHunks?.length > 0) {
        const activeIndex = currentDoc.activeHunkIndex || 0;
        applyHunk(selectedFile!, currentDoc.diffHunks[activeIndex].id);
      }
      return;
    }

    // Diff mode: Escape to reject
    if (e.key === 'Escape' && isDiffMode) {
      e.preventDefault();
      const { rejectHunk, clearDiff } = useEditorStore.getState();
      const currentDoc = documentContents[selectedFile || ''];
      if (currentDoc?.diffHunks?.length > 0) {
        const activeIndex = currentDoc.activeHunkIndex || 0;
        rejectHunk(selectedFile!, currentDoc.diffHunks[activeIndex].id);
      } else {
        clearDiff(selectedFile!);
      }
      return;
    }
  }, [isDiffMode, documentContents, selectedFile, handleSave, editorRef]);

  // Register keyboard handler
  useEffect(() => {
    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [handleKeyDown]);

  return (
    <div className={styles.editorContainer}>
      <div className={styles.editorWrapper}>
        <CodeMirror
          ref={editorRef}
          value={currentContent}
          onChange={handleChange}
          onUpdate={handleUpdate}
          extensions={[
            markdown({ base: markdownLanguage, codeLanguages: languages }),
            lineNumbers(),
            drawSelection(),
            rectangularSelection(),
            highlightSelectionMatches(),
            keymap.of([
              ...defaultKeymap,
              ...historyKeymap,
              ...searchKeymap,
            ]),
            EditorView.theme({
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
            }),
          ]}
          className={styles.codeMirror}
          basicSetup={{
            lineNumbers: true,
            highlightActiveLineGutter: false,
            highlightSpecialChars: true,
            history: true,
            foldGutter: true,
            drawSelection: true,
            dropCursor: true,
            allowMultipleSelections: true,
            indentOnInput: true,
            syntaxHighlighting: true,
            bracketMatching: true,
            closeBrackets: true,
            autocompletion: true,
            rectangularSelection: true,
            crosshairCursor: false,
            highlightActiveLine: false,
            highlightSelectionMatches: true,
            closeBracketsKeymap: true,
            searchKeymap: true,
            foldKeymap: true,
            completionKeymap: true,
            lintKeymap: true,
          }}
        />
        {isDiffMode && <DiffOverlay hunks={diffHunks} />}
        <GhostTextOverlay editorRef={editorRef} />
      </div>

      <div className={styles.statusBar}>
        <span className={styles.statusItem}>
          {currentDoc?.document?.doc_type || 'Markdown'}
        </span>
        <span className={styles.statusItem}>
          {currentContent.split('\n').length} 行
        </span>
        {selection && (
          <span className={styles.statusItem}>
            已选择 {selection.to - selection.from} 字符
          </span>
        )}
        {isDiffMode && (
          <span className={styles.statusItem} data-type="diff">
            {diffHunks.length} 个差异块
          </span>
        )}
        <InlineCompleteStatus />
        <span className={styles.statusItem} style={{ marginLeft: 'auto' }}>
          Ctrl+S 保存
        </span>
      </div>
    </div>
  );
};

// ============================================================================
// Empty/No-file state component
// ============================================================================
const EmptyState: React.FC = () => (
  <div className={styles.editorContainer}>
    <div className={styles.editorWrapper}>
      <div className={styles.noFileHint}>
        <Sparkles size={24} className={styles.hintIcon} />
        <span className={styles.hintText}>
          选择一个文件开始编辑，或按 <kbd>Ctrl</kbd>+<kbd>K</kbd> 调用 AI 助手
        </span>
      </div>
    </div>
  </div>
);

// ============================================================================
// Settings Panel wrapper
// ============================================================================
const SettingsState: React.FC = () => (
  <div className={styles.editorContainer}>
    <SettingsPanel />
  </div>
);

// ============================================================================
// Main Editor Component
// ============================================================================
export const Editor: React.FC = () => {
  const editorRef = useRef<ReactCodeMirrorRef>(null);
  const [, setEditorState] = useState({ document: '', cursorPosition: 0 });
  const { selectedFile, activeTabId } = useSidebarStore();
  const isSettingsTab = activeTabId === SETTINGS_TAB_ID;

  // Show empty state if no file selected
  if (isSettingsTab) {
    return <SettingsState />;
  }

  if (!selectedFile) {
    return <EmptyState />;
  }

  // Wrap editor content with InlineCompleteProvider
  return (
    <InlineCompleteProvider>
      <EditorContent
        editorRef={editorRef}
        setEditorState={setEditorState}
      />
    </InlineCompleteProvider>
  );
};
