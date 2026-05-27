import React, { useCallback, useEffect, useRef } from 'react';
import CodeMirror, { ReactCodeMirrorRef } from '@uiw/react-codemirror';
import { markdown, markdownLanguage } from '@codemirror/lang-markdown';
import { languages } from '@codemirror/language-data';
import { EditorView, keymap, lineNumbers, drawSelection, rectangularSelection } from '@codemirror/view';
import { defaultKeymap, history, historyKeymap } from '@codemirror/commands';
import { searchKeymap, highlightSelectionMatches } from '@codemirror/search';
import { invoke } from '@tauri-apps/api/core';
import { Sparkles } from 'lucide-react';
import { useEditorStore, useSidebarStore } from '../../store';
import { SETTINGS_TAB_ID } from '../../store';
import { DiffOverlay } from './DiffOverlay';
import { SettingsPanel } from '../settings/SettingsPanel';
import styles from './Editor.module.css';

export const Editor: React.FC = () => {
  const editorRef = useRef<ReactCodeMirrorRef>(null);
  const {
    documentContents,
    setDocumentContent,
    setContent,
    setSelection,
    markSaved,
    updateTabDirty,
  } = useEditorStore();
  const { selectedFile, activeTabId } = useSidebarStore();
  const isSettingsTab = activeTabId === SETTINGS_TAB_ID;

  // Get current document state from store
  const currentDoc = selectedFile ? documentContents[selectedFile] : null;
  const currentContent = currentDoc?.content || '';
  const isDirty = currentDoc?.isDirty || false;
  const diffHunks = currentDoc?.diffHunks || [];
  const isDiffMode = currentDoc?.isDiffMode || false;
  const selection = currentDoc?.selection || null;

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

  // Keyboard shortcuts
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      // Cmd/Ctrl+S - Save
      if ((e.metaKey || e.ctrlKey) && e.key === 's') {
        e.preventDefault();
        handleSave();
      }
      // Tab - Apply current hunk
      if (e.key === 'Tab' && isDiffMode) {
        e.preventDefault();
        const { applyHunk } = useEditorStore.getState();
        const currentDoc = documentContents[selectedFile || ''];
        if (currentDoc?.diffHunks?.length > 0) {
          const activeIndex = currentDoc.activeHunkIndex || 0;
          applyHunk(selectedFile!, currentDoc.diffHunks[activeIndex].id);
        }
      }
      // Escape - Reject current hunk or close diff mode
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
      }
    };
    
    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [isDiffMode, documentContents, selectedFile]);

  const handleChange = useCallback((value: string) => {
    if (selectedFile) {
      setContent(selectedFile, value);
    }
  }, [selectedFile, setContent]);

  const handleUpdate = useCallback((viewUpdate: any) => {
    if (viewUpdate.selection && selectedFile) {
      const { from, to } = viewUpdate.state.selection.main;
      if (from !== to) {
        setSelection(selectedFile, { from, to });
      } else {
        setSelection(selectedFile, null);
      }
    }
  }, [selectedFile, setSelection]);

  // No file selected - show inline hint or settings
  if (isSettingsTab) {
    return (
      <div className={styles.editorContainer}>
        <SettingsPanel />
      </div>
    );
  }

  if (!selectedFile || !currentDoc) {
    return (
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
  }

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
      </div>
      
      <div className={styles.statusBar}>
        <span className={styles.statusItem}>
          {currentDoc.document?.doc_type || 'Markdown'}
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
        <span className={styles.statusItem} style={{ marginLeft: 'auto' }}>
          Ctrl+S 保存
        </span>
      </div>
    </div>
  );
};
