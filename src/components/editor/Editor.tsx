import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { type ReactCodeMirrorRef } from '@uiw/react-codemirror';
import CodeMirror from '@uiw/react-codemirror';
import { Compartment } from '@codemirror/state';
import { Sparkles } from 'lucide-react';
import { useEditorStore, useSidebarStore, SETTINGS_TAB_ID, type OpenTab } from '../../store';
import { DiffOverlay } from './DiffOverlay';
import { SettingsPanel } from '../settings/SettingsPanel';
import { WordEditor, ExcelEditor } from './OfficeViewer';
import { InlineCompleteProvider } from '../inline-complete';
import { useDocumentLoader } from './useDocumentLoader';
import { useDocumentSave } from './useDocumentSave';
import { useExternalFileSync } from './useExternalFileSync';
import { createDiffDecorationsField } from './diffDecorationsField';
import { createEditorExtensions } from './editorExtensions';
import { EditorBody } from './EditorBody';
import { useEditorInlineCompletion } from './useEditorInlineCompletion';
import { useEditorKeyboardShortcuts, useEditorSelectionSync } from './useEditorInteraction';
import styles from './Editor.module.css';
import inlineCompleteStyles from '../inline-complete/InlineComplete.module.css';

const diffDecorationsCompartment = new Compartment();

const EditorContent: React.FC<{
  editorRef: React.RefObject<ReactCodeMirrorRef | null>;
}> = ({ editorRef }) => {
  const {
    documentContents,
    setContent,
    isPreviewMode,
    togglePreviewMode,
  } = useEditorStore();
  const { selectedFile, setOpenTabDirty } = useSidebarStore();
  const [refreshToken, setRefreshToken] = useState(0);

  const currentDoc = selectedFile ? documentContents[selectedFile] : null;
  const currentContent = currentDoc?.content || '';
  const isDirty = currentDoc?.isDirty || false;
  const diffHunks = currentDoc?.diffHunks || [];
  const isDiffMode = currentDoc?.isDiffMode || false;
  const selection = currentDoc?.selection || null;

  const requestDocumentRefresh = useCallback(() => {
    setRefreshToken((current) => current + 1);
  }, []);

  useDocumentLoader(selectedFile, currentDoc ? {
    content: currentDoc.content,
    mtime: currentDoc.mtime,
  } : null, refreshToken);
  useExternalFileSync(selectedFile, requestDocumentRefresh);
  const handleSave = useDocumentSave(selectedFile, currentContent, isDirty);
  const handleUpdate = useEditorSelectionSync(selectedFile, currentContent, selection, editorRef);
  const toggleCurrentPreviewMode = useEditorKeyboardShortcuts(selectedFile, handleSave, togglePreviewMode);
  const {
    autoTriggerStateRef,
    inlineAutoTrigger,
    inlineCompletionKeyHandler,
  } = useEditorInlineCompletion(editorRef);

  const diffDecorationsField = useMemo(() => createDiffDecorationsField(diffHunks), [diffHunks]);

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

  const inPreviewMode = selectedFile ? !!isPreviewMode[selectedFile] : false;

  const editorExtensions = useMemo(() => createEditorExtensions({
    diffDecorationsField: diffDecorationsCompartment.of(diffDecorationsField),
    inlineCompletionKeyHandler,
    inlineAutoTrigger,
    autoTriggerStateRef,
  }), [diffDecorationsField, inlineCompletionKeyHandler, inlineAutoTrigger, autoTriggerStateRef]);

  return (
    <div className={styles.editorContainer} data-inline-complete-styles={inlineCompleteStyles}>
      <EditorBody
        inPreviewMode={inPreviewMode}
        currentContent={currentContent}
        selectedFile={selectedFile}
        isDiffMode={isDiffMode}
        diffHunks={diffHunks}
        selection={selection}
        document={currentDoc?.document}
        onTogglePreview={toggleCurrentPreviewMode}
      >
        <>
          <CodeMirror
              ref={editorRef}
              value={currentContent}
              onChange={handleChange}
              onUpdate={handleUpdate}
              extensions={editorExtensions}
              className={styles.codeMirror}
              basicSetup={{
                lineNumbers: true,
                highlightActiveLineGutter: false,
                highlightSpecialChars: true,
                history: false,
                foldGutter: true,
                drawSelection: false,
                dropCursor: true,
                allowMultipleSelections: true,
                indentOnInput: false,
                syntaxHighlighting: true,
                bracketMatching: true,
                closeBrackets: true,
                autocompletion: false,
                rectangularSelection: false,
                crosshairCursor: false,
                highlightActiveLine: false,
                highlightSelectionMatches: false,
                closeBracketsKeymap: false,
                searchKeymap: false,
                foldKeymap: false,
                completionKeymap: false,
                lintKeymap: false,
              }}
            />
            {isDiffMode && <DiffOverlay hunks={diffHunks} />}
          </>
      </EditorBody>
    </div>
  );
};

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

const SettingsState: React.FC = () => (
  <div className={styles.editorContainer}>
    <SettingsPanel />
  </div>
);

function detectFileType(path: string): 'markdown' | 'word' | 'excel' {
  const ext = path.split('.').pop()?.toLowerCase() || '';
  if (ext === 'docx') return 'word';
  if (ext === 'xlsx' || ext === 'xls') return 'excel';
  return 'markdown';
}

type RenderableOfficeTab = {
  tab: OpenTab;
  fileType: 'word' | 'excel';
};

export const Editor: React.FC = () => {
  const editorRef = useRef<ReactCodeMirrorRef>(null);
  const { selectedFile, activeTabId, openTabs } = useSidebarStore();
  const { documentContents } = useEditorStore();
  const isSettingsTab = activeTabId === SETTINGS_TAB_ID;

  const activeFileType = selectedFile ? detectFileType(selectedFile) : null;
  const officeTabs = useMemo<RenderableOfficeTab[]>(() => openTabs.flatMap((tab) => {
    const fileType = detectFileType(tab.path);
    return fileType === 'markdown' ? [] : [{ tab, fileType }];
  }), [openTabs]);

  if (isSettingsTab) {
    return <SettingsState />;
  }

  if (!selectedFile) {
    return <EmptyState />;
  }

  return (
    <>
      {officeTabs.map(({ tab, fileType }) => {
        const isActive = tab.path === selectedFile && activeFileType === fileType;

        if (fileType === 'word') {
          const tabCached = documentContents[tab.path]?.docxBuffer ?? null;
          return (
            <WordEditor
              key={tab.id}
              filePath={tab.path}
              fileName={tab.name}
              initialBuffer={tabCached ? new Uint8Array(tabCached) : null}
              isActive={isActive}
            />
          );
        }

        const tabCached = documentContents[tab.path]?.excelData ?? null;
        return (
          <ExcelEditor
            key={tab.id}
            filePath={tab.path}
            fileName={tab.name}
            initialData={tabCached}
            isActive={isActive}
          />
        );
      })}

      {activeFileType === 'markdown' && (
        <InlineCompleteProvider>
          <EditorContent editorRef={editorRef} />
        </InlineCompleteProvider>
      )}
    </>
  );
};
