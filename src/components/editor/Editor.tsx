import React, { useCallback, useEffect, useRef } from 'react';
import CodeMirror, { ReactCodeMirrorRef } from '@uiw/react-codemirror';
import { markdown, markdownLanguage } from '@codemirror/lang-markdown';
import { languages } from '@codemirror/language-data';
import { EditorView, keymap, lineNumbers, highlightActiveLine, highlightActiveLineGutter, drawSelection, rectangularSelection } from '@codemirror/view';
import { defaultKeymap, history, historyKeymap } from '@codemirror/commands';
import { searchKeymap, highlightSelectionMatches } from '@codemirror/search';
import { oneDark } from '@codemirror/theme-one-dark';
import { invoke } from '@tauri-apps/api/core';
import { Save, Sparkles } from 'lucide-react';
import { useEditorStore, useSidebarStore, useCmdKStore } from '../../store';
import { DiffOverlay } from './DiffOverlay';
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
  const { selectedFile } = useSidebarStore();
  const { open: openCmdK } = useCmdKStore();

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
      // Cmd/Ctrl+K - Open Cmd+K
      if ((e.metaKey || e.ctrlKey) && e.key === 'k') {
        e.preventDefault();
        openCmdK();
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

  if (!selectedFile || !currentDoc) {
    return (
      <div className={styles.emptyState}>
        <div className={styles.emptyContent}>
          <Sparkles size={48} className={styles.emptyIcon} />
          <h2>欢迎使用 inkuo</h2>
          <p>选择一个文件开始编辑，或使用 Cmd+K 调用 AI 助手</p>
        </div>
      </div>
    );
  }

  return (
    <div className={styles.editorContainer}>
      <div className={styles.editorHeader}>
        <div className={styles.fileInfo}>
          <span className={styles.fileName}>{currentDoc.document?.title || '未命名文件'}</span>
          {isDirty && <span className={styles.dirtyIndicator}>●</span>}
        </div>
        <div className={styles.editorActions}>
          <button 
            className={styles.actionButton}
            onClick={openCmdK}
            title="AI 编辑 (Cmd+K)"
          >
            <Sparkles size={16} />
          </button>
          <button 
            className={styles.actionButton}
            onClick={handleSave}
            disabled={!isDirty}
            title="保存 (Cmd+S)"
          >
            <Save size={16} />
          </button>
        </div>
      </div>
      
      <div className={styles.editorWrapper}>
        <CodeMirror
          ref={editorRef}
          value={currentContent}
          onChange={handleChange}
          onUpdate={handleUpdate}
          extensions={[
            markdown({ base: markdownLanguage, codeLanguages: languages }),
            lineNumbers(),
            highlightActiveLine(),
            highlightActiveLineGutter(),
            history(),
            drawSelection(),
            rectangularSelection(),
            highlightSelectionMatches(),
            keymap.of([
              ...defaultKeymap,
              ...historyKeymap,
              ...searchKeymap,
            ]),
            oneDark,
            EditorView.theme({
              '&': {
                height: '100%',
                fontSize: '14px',
              },
              '.cm-scroller': {
                fontFamily: 'var(--font-mono)',
              },
              '.cm-content': {
                padding: '16px 0',
              },
              '.cm-line': {
                padding: '0 16px',
              },
              '.cm-gutters': {
                backgroundColor: 'var(--bg-secondary)',
                borderRight: '1px solid var(--border-color)',
              },
            }),
          ]}
          className={styles.codeMirror}
          basicSetup={{
            lineNumbers: true,
            highlightActiveLineGutter: true,
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
            highlightActiveLine: true,
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
          {currentDoc.document?.doc_type || 'PlainText'}
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
      </div>
    </div>
  );
};
