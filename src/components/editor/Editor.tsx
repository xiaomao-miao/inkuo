import { useCallback, useEffect, useRef } from 'react';
import { Decoration, type DecorationSet } from '@codemirror/view';
import CodeMirror, { type ReactCodeMirrorRef } from '@uiw/react-codemirror';
import { markdown, markdownLanguage } from '@codemirror/lang-markdown';
import { languages } from '@codemirror/language-data';
import { EditorView, keymap, lineNumbers, drawSelection, rectangularSelection, type ViewUpdate } from '@codemirror/view';
import { Prec, StateField, RangeSetBuilder } from '@codemirror/state';
import { inlineDiffTheme } from './inlineDiffDecorations';
import { historyKeymap } from '@codemirror/commands';
import { highlightSelectionMatches } from '@codemirror/search';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { Sparkles } from 'lucide-react';
import { useEditorStore, useSidebarStore, useInlineCompleteStore, SETTINGS_TAB_ID } from '../../store';
import type { Document } from '../../types';
import { DiffOverlay } from './DiffOverlay';
import { DiffActionBar } from './DiffActionBar';
import { MarkdownPreview } from './MarkdownPreview';
import { SettingsPanel } from '../settings/SettingsPanel';
import { WordEditor, ExcelEditor } from './OfficeViewer';
import {
  InlineCompleteProvider,
  useInlineComplete,
  InlineCompleteStatus,
  inlineCompletionDecoration,
} from '../inline-complete';
import { TIMING, CODEMIRROR_SNIPPET_BOUNDS } from '../../constants/timing';
import { detectLanguage } from '../../types/inline-complete';
import styles from './Editor.module.css';
import inlineCompleteStyles from '../inline-complete/InlineComplete.module.css';

// ============================================================================
// Keyboard event handler for inline completion (higher priority than keymap)
// ============================================================================
const inlineCompletionKeyHandler = EditorView.domEventHandlers({
  keydown(event, view) {
    if (!view) return false;

    const storeState = useInlineCompleteStore.getState();
    const { currentCompletion, clearCompletion } = storeState;

    // Tab - Accept completion
    if (event.key === 'Tab' && currentCompletion) {
      // Prevent CodeMirror's default Tab indentation
      event.preventDefault();
      event.stopPropagation();

      const cursorPosition = view.state.selection.main.head;
      const text = currentCompletion.text;

      clearCompletion();
      view.dispatch({
        changes: { from: cursorPosition, insert: text },
        selection: { anchor: cursorPosition + text.length },
        userEvent: 'input.complete',
      });
      return true;
    }

    // Escape - Dismiss completion
    if (event.key === 'Escape' && currentCompletion) {
      event.preventDefault();
      clearCompletion();
      return true;
    }

    return false;
  },
});

// ============================================================================
// Editor Content Component (inside Provider) - CAN use useInlineComplete
// ============================================================================
const EditorContent: React.FC<{
  editorRef: React.RefObject<ReactCodeMirrorRef | null>;
}> = ({ editorRef }) => {
  const {
    documentContents,
    setDocumentContent,
    setContent,
    setSelection,
    markSaved,
    updateTabDirty,
    isPreviewMode,
    togglePreviewMode,
  } = useEditorStore();
  const { selectedFile, setOpenTabDirty } = useSidebarStore();
  const { triggerCompletion } = useInlineComplete();

  // Ref for triggerCompletion to avoid effect re-runs
  const triggerCompletionRef = useRef(triggerCompletion);
  triggerCompletionRef.current = triggerCompletion;

  // Track last selected file
  const lastSelectedFileRef = useRef<string | null>(null);

  // Get current document state from store
  const currentDoc = selectedFile ? documentContents[selectedFile] : null;
  const currentContent = currentDoc?.content || '';
  const isDirty = currentDoc?.isDirty || false;
  const diffHunks = currentDoc?.diffHunks || [];
  const isDiffMode = currentDoc?.isDiffMode || false;
  const selection = currentDoc?.selection || null;

  // Clear completion on file switch
  useEffect(() => {
    if (selectedFile !== lastSelectedFileRef.current) {
      lastSelectedFileRef.current = selectedFile;
      useInlineCompleteStore.getState().clearCompletion();
    }
  }, [selectedFile]);

  // Cursor-like auto trigger state (stable across renders)
  const autoTriggerStateRef = useRef<{
    timer: ReturnType<typeof setTimeout> | null;
    lastAcceptAt: number;
    destroyed: boolean;
  }>({
    timer: null,
    lastAcceptAt: 0,
    destroyed: false,
  });

  // Cleanup on unmount — cancel any pending timer and mark destroyed
  useEffect(() => {
    const ref = autoTriggerStateRef.current;
    return () => {
      ref.destroyed = true;
      if (ref.timer !== null) {
        clearTimeout(ref.timer);
        ref.timer = null;
      }
    };
  }, []);

  const inlineAutoTrigger = EditorView.updateListener.of((update) => {
    const view = update.view;

    if (!view.hasFocus) return;

    // After accepting a completion, don't immediately trigger again.
    const now = Date.now();
    if (now - autoTriggerStateRef.current.lastAcceptAt < TIMING.COMPLETION_RETRIGGER_DELAY_MS) return;

    // Only consider real user input/delete events.
    const isUserInput = update.transactions.some(
      (tr) =>
        tr.isUserEvent('input') ||
        tr.isUserEvent('input.type') ||
        tr.isUserEvent('delete')
    );
    if (!isUserInput) return;

    // If a completion is currently shown, clear it on new input and allow re-trigger.
    const storeState = useInlineCompleteStore.getState();
    if (storeState.currentCompletion) {
      useInlineCompleteStore.getState().clearCompletion();
    }

    // If selection isn't empty, don't inline-complete.
    const sel = view.state.selection.main;
    if (!sel.empty) return;

    if (!storeState.enabled) return;
    if (storeState.isLoading) return;

    // Debounce (use configured debounceMs)
    if (autoTriggerStateRef.current.timer) {
      clearTimeout(autoTriggerStateRef.current.timer);
    }

    const filePath = useSidebarStore.getState().selectedFile;
    autoTriggerStateRef.current.timer = setTimeout(() => {
      autoTriggerStateRef.current.timer = null;
      if (autoTriggerStateRef.current.destroyed) return;
      if (!view.hasFocus) return;
      const latestSel = view.state.selection.main;
      if (!latestSel.empty) return;

      const latestStore = useInlineCompleteStore.getState();
      if (!latestStore.enabled || latestStore.isLoading || latestStore.currentCompletion) return;

      const docLen = view.state.doc.length;
      const cursor = latestSel.head;

      // Smart snippet: capture a window around cursor, bounded by max chars.
      // This avoids `doc.toString()` on large documents, which can freeze input.
      const from = Math.max(0, cursor - CODEMIRROR_SNIPPET_BOUNDS.MAX_BEFORE);
      const to = Math.min(docLen, cursor + CODEMIRROR_SNIPPET_BOUNDS.MAX_AFTER);

      const snippetText = view.state.doc.sliceString(from, to);
      const cursorInSnippet = cursor - from;

      triggerCompletionRef.current({
        document: snippetText,
        cursorPosition: cursorInSnippet,
        language: detectLanguage(filePath || undefined),
        filePath: filePath || undefined,
        snippet: { text: snippetText, start_offset: from },
      });
    }, storeState.debounceMs);
  });

  // Clear completion & cancel pending timer when clicking outside the editor.
  useEffect(() => {
    const onPointerDown = (e: PointerEvent) => {
      const view = editorRef.current?.view;
      if (!view) return;
      if (!view.dom.contains(e.target as Node)) {
        useInlineCompleteStore.getState().clearCompletion();
        if (autoTriggerStateRef.current.timer) {
          clearTimeout(autoTriggerStateRef.current.timer);
          autoTriggerStateRef.current.timer = null;
        }
      }
    };

    window.addEventListener('pointerdown', onPointerDown, true);
    return () => window.removeEventListener('pointerdown', onPointerDown, true);
  }, [editorRef]);

  const diffHunksRef = useRef(diffHunks);
  diffHunksRef.current = diffHunks;

  const diffDecorationsField = StateField.define<DecorationSet>({
    create() {
      return Decoration.none;
    },
    update(_decorations, tr) {
      const hunks = diffHunksRef.current || [];
      const builder = new RangeSetBuilder<Decoration>();

      for (const hunk of hunks) {
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
    provide: (f) => EditorView.decorations.from(f),
  });

  // Reload currently open file when it is modified externally (e.g., by AI panel).
  // We track file-change events in a ref counter so the effect re-runs on external changes.
  const forceRefreshRef = useRef<Record<string, number>>({});
  const forceRefreshCount = forceRefreshRef.current[selectedFile || ''] ?? 0;

  useEffect(() => {
    let cancelled = false;

    const loadDocument = async () => {
      if (!selectedFile) return;

      const cached = useEditorStore.getState().documentContents[selectedFile];

      try {
        const result = await invoke<{ document: Document; content: string; mtime: number }>('read_document', {
          path: selectedFile,
        });

        // Check if we need to update:
        // - No cache exists
        // - Cache content is empty
        // - Cache mtime is 0 (legacy/invalid cache)
        // - File mtime differs from cache
        // - A file-change event triggered this reload (forceRefreshCount changed)
        const needsReload = !cached ||
          cached.content === '' ||
          cached.mtime === 0 ||
          result.mtime !== cached.mtime;

        if (!cancelled) {
          if (needsReload) {
            setDocumentContent(selectedFile, result.document, result.content, result.mtime);
            setOpenTabDirty(selectedFile, false);
          }
        }
      } catch (err) {
        console.error('Failed to load document:', err);
      }
    };

    loadDocument();

    return () => {
      cancelled = true;
    };
  }, [selectedFile, setDocumentContent, setOpenTabDirty, forceRefreshCount]);

  // Restore selection from store when a new document loads.
  // This handles the case where the knowledge panel sets a selection for a file
  // that is not yet in the documentContents cache.
  useEffect(() => {
    const view = editorRef.current?.view;
    if (!view) return;
    const docState = useEditorStore.getState().documentContents[selectedFile ?? ''];
    const sel = docState?.selection;
    if (!sel) return;

    const currentSel = view.state.selection.main;
    // Only apply if the selection differs from what's in the store
    if (currentSel.from !== sel.from || currentSel.to !== sel.to) {
      view.dispatch({
        selection: { anchor: sel.from, head: sel.to },
        scrollIntoView: true,
        userEvent: 'kb.navigate',
      });
    }
  // Re-run when the document content changes (document loaded) or when the
    // selection in the store changes (knowledge panel navigation).
  }, [selectedFile, currentContent, editorRef]);

  // Listen for external file changes and force a reload by updating mtime in the store.
  useEffect(() => {
    let unlisten: (() => void) | null = null;

    const setup = async () => {
        unlisten = await listen<{ path: string }>('file-written', (event) => {
        if (!selectedFile) return;
        const changedPath = event.payload.path || '';
        if (changedPath === selectedFile) {
          forceRefreshRef.current[selectedFile] = (forceRefreshRef.current[selectedFile] || 0) + 1;
        }
      });
    };

    setup();
    return () => {
      if (unlisten) unlisten();
    };
  }, [selectedFile]);

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
      setOpenTabDirty(selectedFile, false);
    } catch (err) {
      console.error('Failed to save document:', err);
    }
  }, [selectedFile, currentContent, isDirty, markSaved, updateTabDirty, setOpenTabDirty]);

  // Sync isDirty state to sidebar store whenever it changes
  useEffect(() => {
    if (selectedFile && isDirty !== undefined) {
      setOpenTabDirty(selectedFile, isDirty);
    }
  }, [selectedFile, isDirty, setOpenTabDirty]);

  const handleChange = useCallback((value: string) => {
    if (selectedFile) {
      setContent(selectedFile, value);
    }
  }, [selectedFile, setContent]);

  const handleUpdate = useCallback((viewUpdate: ViewUpdate) => {
    if (viewUpdate.selectionSet && selectedFile) {
      const { from, to } = viewUpdate.state.selection.main;
      const currentDoc = useEditorStore.getState().documentContents[selectedFile];
      if (from !== to) {
        if (!currentDoc?.selection || currentDoc.selection.from !== from || currentDoc.selection.to !== to) {
          setSelection(selectedFile, { from, to });
        }
      } else if (currentDoc?.selection) {
        setSelection(selectedFile, null);
      }
    }
  }, [selectedFile, setSelection]);

  // Stable ref for togglePreviewMode to avoid effect re-runs
  const togglePreviewModeRef = useRef(togglePreviewMode);
  togglePreviewModeRef.current = togglePreviewMode;

  // Keyboard shortcuts for Save (Cmd/Ctrl+S) and Preview toggle (Cmd/Ctrl+Shift+P)
  useEffect(() => {
    const handleGlobalKeyDown = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key === 's') {
        e.preventDefault();
        handleSave();
      }
      if ((e.metaKey || e.ctrlKey) && e.shiftKey && e.key === 'p') {
        e.preventDefault();
        if (selectedFile) {
          togglePreviewModeRef.current(selectedFile);
        }
      }
    };

    window.addEventListener('keydown', handleGlobalKeyDown);
    return () => window.removeEventListener('keydown', handleGlobalKeyDown);
  }, [handleSave, selectedFile]);

  const inPreviewMode = selectedFile ? !!isPreviewMode[selectedFile] : false;

  return (
    <div className={styles.editorContainer} data-inline-complete-styles={inlineCompleteStyles}>
      <div className={styles.editorWrapper}>
        {inPreviewMode ? (
          <MarkdownPreview content={currentContent} fileName={selectedFile?.split('/').pop() || ''} />
        ) : (
          <>
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
            inlineDiffTheme,
            diffDecorationsField,
            // Keyboard handler for inline completion (Tab/Escape)
            inlineCompletionKeyHandler,
            // Cursor-like auto trigger (only on real user input)
            inlineAutoTrigger,
            // Ensure Tab accept has highest precedence over indentation
            Prec.highest(
              keymap.of([
                {
                  key: 'Tab',
                  run: (view) => {
                    const { currentCompletion, clearCompletion } = useInlineCompleteStore.getState();
                    if (!currentCompletion) return false;
                    const cursorPosition = view.state.selection.main.head;
                    const text = currentCompletion.text;

                    // Mark accept time to avoid immediate re-trigger
                    autoTriggerStateRef.current.lastAcceptAt = Date.now();

                    clearCompletion();
                    view.dispatch({
                      changes: { from: cursorPosition, insert: text },
                      selection: { anchor: cursorPosition + text.length },
                      userEvent: 'input.complete',
                    });
                    return true;
                  },
                  preventDefault: true,
                },
                {
                  key: 'Escape',
                  run: () => {
                    const { currentCompletion, clearCompletion } = useInlineCompleteStore.getState();
                    if (!currentCompletion) return false;
                    clearCompletion();
                    return true;
                  },
                },
              ])
            ),
            // Render ghost text inside CodeMirror (cursor-anchored)
            inlineCompletionDecoration(),
            keymap.of([
              ...historyKeymap,
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
            history: false, // We add historyKeymap manually
            foldGutter: true,
            drawSelection: false, // We add drawSelection manually
            dropCursor: true,
            allowMultipleSelections: true,
            indentOnInput: false,
            syntaxHighlighting: true,
            bracketMatching: true,
            closeBrackets: true,
            autocompletion: false, // Disable native autocompletion
            rectangularSelection: false, // We add rectangularSelection manually
            crosshairCursor: false,
            highlightActiveLine: false,
            highlightSelectionMatches: false, // We add highlightSelectionMatches manually
            closeBracketsKeymap: false,
            searchKeymap: false, // We add searchKeymap manually
            foldKeymap: false,
            completionKeymap: false,
            lintKeymap: false,
          }}
            />
            {isDiffMode && <DiffOverlay hunks={diffHunks} />}
            {/* ghost text is rendered via CodeMirror decoration */}
          </>
        )}
      </div>

      {!inPreviewMode && <DiffActionBar />}

      <div className={styles.statusBar}>
        <span className={styles.statusItem}>
          {currentDoc?.document?.doc_type || 'Markdown'}
        </span>
        <span className={styles.statusItem}>
          {currentContent.split('\n').length} 行
        </span>
        {selection && !inPreviewMode && (
          <span className={styles.statusItem}>
            已选择 {selection.to - selection.from} 字符
          </span>
        )}
        {isDiffMode && !inPreviewMode && (
          <span className={styles.statusItem} data-type="diff">
            {diffHunks.length} 个差异块
          </span>
        )}
        {!inPreviewMode && <InlineCompleteStatus />}
        <span className={styles.statusItem} style={{ marginLeft: 'auto' }}>
          {inPreviewMode ? (
            <button
              className={styles.previewToggle}
              onClick={() => selectedFile && togglePreviewMode(selectedFile)}
            >
              退出阅读
            </button>
          ) : (
            <button
              className={styles.previewToggle}
              onClick={() => selectedFile && togglePreviewMode(selectedFile)}
            >
              阅读模式
            </button>
          )}
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
function detectFileType(path: string): 'markdown' | 'word' | 'excel' {
  const ext = path.split('.').pop()?.toLowerCase() || '';
  if (ext === 'docx') return 'word';
  if (ext === 'xlsx' || ext === 'xls') return 'excel';
  return 'markdown';
}

export const Editor: React.FC = () => {
  const editorRef = useRef<ReactCodeMirrorRef>(null);
  const { selectedFile, activeTabId, openTabs } = useSidebarStore();
  const { documentContents } = useEditorStore();
  const isSettingsTab = activeTabId === SETTINGS_TAB_ID;

  // Determine active file type
  const activeFileType = selectedFile ? detectFileType(selectedFile) : null;

  // Show empty state if no file selected
  if (isSettingsTab) {
    return <SettingsState />;
  }

  if (!selectedFile) {
    return <EmptyState />;
  }

  return (
    <>
      {/* Always render WordEditor — keep mounted for tab-switch persistence.
          Each editor instance is keyed by tab path. */}
      {openTabs.map(tab => {
        const tabFileType = detectFileType(tab.path);
        if (tabFileType !== 'word') return null;
        const tabCached = documentContents[tab.path]?.docxBuffer ?? null;
        return (
          <WordEditor
            key={tab.id}
            filePath={tab.path}
            fileName={tab.name}
            initialBuffer={tabCached ? new Uint8Array(tabCached) : null}
            isActive={tab.path === selectedFile && activeFileType === 'word'}
          />
        );
      })}

      {/* Always render ExcelEditor */}
      {openTabs.map(tab => {
        const tabFileType = detectFileType(tab.path);
        if (tabFileType !== 'excel') return null;
        const tabCached = documentContents[tab.path]?.excelData ?? null;
        return (
          <ExcelEditor
            key={tab.id}
            filePath={tab.path}
            fileName={tab.name}
            initialData={tabCached}
            isActive={tab.path === selectedFile && activeFileType === 'excel'}
          />
        );
      })}

      {/* Markdown editor - only render when active (CodeMirror is lightweight) */}
      {activeFileType === 'markdown' && (
        <InlineCompleteProvider>
          <EditorContent editorRef={editorRef} />
        </InlineCompleteProvider>
      )}
    </>
  );
};
