import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { type ReactCodeMirrorRef } from '@uiw/react-codemirror';
import CodeMirror from '@uiw/react-codemirror';
import { Compartment } from '@codemirror/state';
import { Sparkles } from 'lucide-react';
import { useEditorStore, useSidebarStore, useSettingsStore, SETTINGS_TAB_ID, type OpenTab } from '../../store';
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
  const selectedFile = useSidebarStore((state) => state.selectedFile);
  const currentDoc = useEditorStore((state) => (selectedFile ? state.documentContents[selectedFile] : null));
  const setContent = useEditorStore((state) => state.setContent);
  const isPreviewMode = useEditorStore((state) => state.isPreviewMode);
  const togglePreviewMode = useEditorStore((state) => state.togglePreviewMode);
  const settings = useSettingsStore((state) => state.settings);
  const setOpenTabDirty = useSidebarStore((state) => state.setOpenTabDirty);
  const [refreshToken, setRefreshToken] = useState(0);

  const currentMetadata = currentDoc?.metadata;
  const currentDiff = currentDoc?.diff;
  const currentContent = currentMetadata?.content ?? '';
  const isDirty = currentMetadata?.isDirty ?? false;
  const diffHunks = useMemo(() => currentDiff?.hunks ?? [], [currentDiff?.hunks]);
  const isDiffMode = currentDiff?.isActive || false;
  const selection = currentMetadata?.selection ?? null;

  const requestDocumentRefresh = useCallback(() => {
    setRefreshToken((current) => current + 1);
  }, []);

  useDocumentLoader(selectedFile, currentMetadata ? {
    content: currentMetadata.content,
    mtime: currentMetadata.mtime,
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
        document={currentMetadata?.document}
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
                lineNumbers: settings.editor_line_numbers,
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

function detectFileType(path: string): 'markdown' | 'plaintext' | 'word' | 'excel' {
  const ext = path.split('.').pop()?.toLowerCase() || '';
  if (ext === 'docx') return 'word';
  if (ext === 'xlsx' || ext === 'xls') return 'excel';
  if (ext === 'md' || ext === 'markdown') return 'markdown';
  return 'plaintext';
}

type RenderableOfficeTab = {
  tab: OpenTab;
  fileType: 'word' | 'excel';
};

const OfficeTabRenderer: React.FC<{
  tab: OpenTab;
  fileType: 'word' | 'excel';
  isActive: boolean;
}> = ({ tab, fileType, isActive }) => {
  const officeState = useEditorStore((state) => state.documentContents[tab.path]?.office);

  if (fileType === 'word') {
    const tabCached = officeState?.docxBuffer ?? null;
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

  return (
    <ExcelEditor
      key={tab.id}
      filePath={tab.path}
      fileName={tab.name}
      initialData={officeState?.excelData ?? null}
      isActive={isActive}
    />
  );
};

export const Editor: React.FC = () => {
  const editorRef = useRef<ReactCodeMirrorRef>(null);
  const selectedFile = useSidebarStore((state) => state.selectedFile);
  const activeTabId = useSidebarStore((state) => state.activeTabId);
  const openTabs = useSidebarStore((state) => state.openTabs);
  const isSettingsTab = activeTabId === SETTINGS_TAB_ID;

  const activeFileType = selectedFile ? detectFileType(selectedFile) : null;
  const officeTabs = useMemo<RenderableOfficeTab[]>(() => openTabs.flatMap((tab: OpenTab) => {
    const fileType = detectFileType(tab.path);
    if (fileType !== 'word' && fileType !== 'excel') return [];
    return [{ tab, fileType }];
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

        return (
          <OfficeTabRenderer
            key={tab.id}
            tab={tab}
            fileType={fileType}
            isActive={isActive}
          />
        );
      })}

      {(activeFileType === 'markdown' || activeFileType === 'plaintext') && (
        <InlineCompleteProvider>
          <EditorContent editorRef={editorRef} />
        </InlineCompleteProvider>
      )}
    </>
  );
};
