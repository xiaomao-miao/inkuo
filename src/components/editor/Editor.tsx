import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { lazy, Suspense } from 'react';
import { type Extension } from '@codemirror/state';
import { type ReactCodeMirrorRef } from '@uiw/react-codemirror';
import CodeMirror from '@uiw/react-codemirror';
import { Compartment } from '@codemirror/state';
import { Sparkles } from 'lucide-react';
import { useEditorStore, useSidebarStore, useSettingsStore, SETTINGS_TAB_ID, CLOUD_TAB_ID, type OpenTab } from '../../store';
import { detectFileKind, type FileKind } from '../../types';
import { DiffOverlay } from './DiffOverlay';
import { InlineCompleteProvider } from '../inline-complete';
import { useDocumentLoader } from './useDocumentLoader';
import { useDocumentSave } from './useDocumentSave';
import { useExternalFileSync } from './useExternalFileSync';
import { createDiffDecorationsField } from './diffDecorationsField';
import { createEditorExtensions, languageExtensionForKind } from './editorExtensions';
import { LazyImageViewer, LazyPdfViewer, LazySvgViewer } from './LazyMediaViewers';
import { EditorBody } from './EditorBody';
import { useEditorInlineCompletion } from './useEditorInlineCompletion';
import { useEditorKeyboardShortcuts, useEditorSelectionSync } from './useEditorInteraction';
import styles from './Editor.module.css';
import inlineCompleteStyles from '../inline-complete/InlineComplete.module.css';

// Code-split heavy route-level panels and the Office editor bundle.
// Each of these pulls in a substantial dependency tree:
//   - SettingsPanel: settings UI + child panels
//   - CloudPage: cloud auth UI + Tauri shell helpers
//   - OfficeViewer: DocxEditor (ProseMirror + OOXML + mammoth) and
//     Workbook (FortuneSheet + xlsx) — the single largest contributors
//     to the main chunk in the baseline build (~5.8 MB main, ~1.5 MB
//     gzip). Lazy-loading them removes ~3 MB from first paint and
//     shifts the cost to when the user actually opens a .docx/.xlsx
//     tab or visits Settings / Cloud.
//
// The lazy components are mounted behind a Suspense boundary that
// shows a skeleton so the UI stays responsive during chunk fetch.
const SettingsPanel = lazy(() =>
  import('../settings/SettingsPanel').then((m) => ({ default: m.SettingsPanel }))
);
const CloudPage = lazy(() =>
  import('../cloud/CloudPage').then((m) => ({ default: m.CloudPage }))
);

const EditorContent: React.FC<{
  editorRef: React.RefObject<ReactCodeMirrorRef | null>;
  /** Coarse-grained file kind. Used to pick a CodeMirror language
   *  extension (e.g. typescript for `.ts`, python for `.py`,
   *  markdown for `.md`). When omitted (legacy callers), the editor
   *  falls back to the markdown language pack as before. */
  fileKind?: FileKind;
}> = ({ editorRef, fileKind }) => {
  const selectedFile = useSidebarStore((state) => state.selectedFile);
  const currentDoc = useEditorStore((state) => (selectedFile ? state.documentContents[selectedFile] : null));
  const setContent = useEditorStore((state) => state.setContent);
  const isPreviewMode = useEditorStore((state) => state.isPreviewMode);
  const togglePreviewMode = useEditorStore((state) => state.togglePreviewMode);
  const settings = useSettingsStore((state) => state.settings);
  const setOpenTabDirty = useSidebarStore((state) => state.setOpenTabDirty);
  const [refreshToken, setRefreshToken] = useState(0);

  // Each editor instance needs its own `Compartment` for the dynamic diff
  // decoration extensions. Sharing one at module scope was incorrect — it
  // meant each tab attached extensions to the same Compartment object,
  // so a `Compartment.reconfigure(...)` call on one editor's `EditorView`
  // would (in principle) be visible to whichever editor observed the
  // stored reference next. Holding the compartment in `useMemo` gives
  // every `EditorContent` instance a stable, private reference it can
  // pass to `.of(...)` and `.reconfigure(...)` independently.
  const diffDecorationsCompartment = useMemo(() => new Compartment(), []);

  const currentMetadata = currentDoc?.metadata;
  const currentDiff = currentDoc?.diff;
  const currentContent = currentMetadata?.content ?? '';
  const isDirty = currentMetadata?.isDirty ?? false;
  const diffHunks = useMemo(() => currentDiff?.hunks ?? [], [currentDiff?.hunks]);
  const isDiffMode = currentDiff?.isActive || false;
  const selection = currentMetadata?.selection ?? null;

  // Resolve the language extension for the current file. We resolve this
  // asynchronously because some language parsers (e.g. via
  // `@codemirror/language-data`) are dynamically imported on first use.
  const [language, setLanguage] = useState<Extension | null>(null);

  useEffect(() => {
    if (!selectedFile) {
      setLanguage(null);
      return;
    }
    const kind = fileKind ?? detectFileKind(selectedFile);
    const base = selectedFile.split(/[\\/]/).pop() ?? selectedFile;
    const dot = base.lastIndexOf('.');
    const ext = dot >= 0 && dot < base.length - 1 ? base.slice(dot + 1).toLowerCase() : '';

    let cancelled = false;
    languageExtensionForKind(kind, ext).then((ext) => {
      if (!cancelled) setLanguage(ext);
    }).catch(() => {
      if (!cancelled) setLanguage(null);
    });

    return () => {
      cancelled = true;
    };
  }, [selectedFile, fileKind]);

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

  const editorExtensions = useMemo(() => {
    return createEditorExtensions({
      diffDecorationsField: diffDecorationsCompartment.of(diffDecorationsField),
      inlineCompletionKeyHandler,
      inlineAutoTrigger,
      autoTriggerStateRef,
      language,
    });
  }, [diffDecorationsField, inlineCompletionKeyHandler, inlineAutoTrigger, autoTriggerStateRef, language]);

  return (
    <div className={`${styles.editorContainer} editorContainer`} data-inline-complete-styles={inlineCompleteStyles}>
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
                drawSelection: true,
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
          选择一个文件开始编辑，或按 <kbd>Ctrl</kbd>+<kbd>Shift</kbd>+<kbd>I</kbd> 调用 AI 助手
        </span>
      </div>
    </div>
  </div>
);

/**
 * Lightweight Suspense fallback shown while a lazy-loaded route or
 * editor chunk is being fetched. Reuses the existing Skeleton
 * primitives so the look matches the rest of the app. Kept inline in
 * Editor.tsx (rather than a separate file) because the lazy routes are
 * co-located here and the fallback only matters during chunk fetch.
 */
const RouteFallback: React.FC<{ label: string }> = ({ label }) => (
  <div className={styles.editorContainer} role="status" aria-live="polite">
    <div className={styles.editorWrapper}>
      <div className={styles.noFileHint}>
        <span className={styles.spinner} aria-hidden="true" />
        <span className={styles.hintText}>{label}</span>
      </div>
    </div>
  </div>
);

// OfficeViewer pulls in DocxEditor (ProseMirror + OOXML parser +
// prosemirror-tables + 200+ KB of layout/serializer code) plus Workbook
// (FortuneSheet + xlsx ~430 KB). Together these are the largest single
// contributor to the main chunk in the baseline build.
//
// We split OfficeViewer into its own chunk and lazy-load it so the cost
// is paid only when the user opens a .docx or .xlsx tab. The lazy
// boundary returns a Suspense fallback (skeleton) until the chunk arrives,
// keeping the right-hand editor pane visually stable.
const WordEditor = lazy(() =>
  import('./OfficeViewer').then((m) => ({ default: m.WordEditor }))
);
const ExcelEditor = lazy(() =>
  import('./OfficeViewer').then((m) => ({ default: m.ExcelEditor }))
);

const SettingsState: React.FC = () => (
  <Suspense fallback={<RouteFallback label="正在加载设置" />}>
    <SettingsPanel />
  </Suspense>
);

const CloudState: React.FC = () => (
  <Suspense fallback={<RouteFallback label="正在加载云服务" />}>
    <CloudPage />
  </Suspense>
);

function detectFileType(path: string): FileKind {
  return detectFileKind(path);
}

type RenderableOfficeTab = {
  tab: OpenTab;
  fileType: Extract<FileKind, 'word' | 'excel'>;
};

const OFFICE_KINDS: ReadonlyArray<RenderableOfficeTab['fileType']> = ['word', 'excel'];

const OfficeTabRenderer: React.FC<{
  tab: OpenTab;
  fileType: RenderableOfficeTab['fileType'];
  isActive: boolean;
}> = ({ tab, fileType, isActive }) => {
  const officeState = useEditorStore((state) => state.documentContents[tab.path]?.office);

  // We render the active tab (and only the active tab). Switching tabs
  // unmounts the previous editor and mounts the new one, which is the only
  // reliable way to keep ProseMirror / ExcelGrid quiescent when not in
  // view. With `visibility: hidden` the editors continue to run their
  // ResizeObserver + measurement loops in the background, which leaked
  // out as a constantly flickering right-hand scrollbar on first open.
  if (!isActive) {
    return null;
  }

  if (fileType === 'word') {
    const tabCached = officeState?.docxBuffer ?? null;
    return (
      <Suspense fallback={<RouteFallback label="正在加载 Word 编辑器" />}>
        <WordEditor
          key={tab.id}
          filePath={tab.path}
          fileName={tab.name}
          initialBuffer={tabCached ? new Uint8Array(tabCached) : null}
          isActive={isActive}
        />
      </Suspense>
    );
  }

  return (
    <Suspense fallback={<RouteFallback label="正在加载 Excel 编辑器" />}>
      <ExcelEditor
        key={tab.id}
        filePath={tab.path}
        fileName={tab.name}
        isActive={isActive}
      />
    </Suspense>
  );
};

export const Editor: React.FC = () => {
  const editorRef = useRef<ReactCodeMirrorRef>(null);
  const selectedFile = useSidebarStore((state) => state.selectedFile);
  const activeTabId = useSidebarStore((state) => state.activeTabId);
  const openTabs = useSidebarStore((state) => state.openTabs);
  const isSettingsTab = activeTabId === SETTINGS_TAB_ID;

  const activeFileType: FileKind | null = selectedFile ? detectFileType(selectedFile) : null;
  const officeTabs = useMemo<RenderableOfficeTab[]>(() => openTabs.flatMap((tab: OpenTab) => {
    const fileType = detectFileType(tab.path);
    if (!(OFFICE_KINDS as readonly string[]).includes(fileType)) return [];
    return [{ tab, fileType: fileType as RenderableOfficeTab['fileType'] }];
  }), [openTabs]);

  // SVG files classify as `image` for the sidebar icon, but the editor
  // route sends them to a specialised viewer (checker background, fit-to-
  // viewport) rather than the raster ImageViewer — the raster viewer's
  // white-on-white hide makes transparent regions hard to inspect.
  // Hooks must run unconditionally on every render, so this lives ABOVE
  // the early returns below; the `selectedFile` guard happens inside.
  const isSvg = useMemo(() => {
    if (!selectedFile) return false;
    const base = selectedFile.split(/[\\/]/).pop() ?? selectedFile;
    return base.toLowerCase().endsWith('.svg');
  }, [selectedFile]);

  if (isSettingsTab) {
    return <SettingsState />;
  }

  if (activeTabId === CLOUD_TAB_ID) {
    return <CloudState />;
  }

  if (!selectedFile) {
    return <EmptyState />;
  }

  // Decide which top-level viewer to render. The office tabs are stacked
  // (only one is shown at a time, the rest keep their state); the
  // markdown/code/text editor is rendered separately and shares the
  // same selected-file state. Image and PDF viewers are mounted as
  // their own dedicated stack items because they have no live-edit
  // state and should not participate in the OfficeTabRenderer cache.
  const isOffice = activeFileType && (OFFICE_KINDS as readonly string[]).includes(activeFileType);
  const isEditableText =
    activeFileType === 'markdown' ||
    activeFileType === 'text' ||
    activeFileType === 'code' ||
    activeFileType === 'config' ||
    activeFileType === 'data';
  const isImage = activeFileType === 'image';
  const isPdf = activeFileType === 'pdf';

  return (
    <div className={styles.officeStack}>
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

      {isEditableText && (
        <div className={styles.officeStackItem}>
          <InlineCompleteProvider>
            <EditorContent editorRef={editorRef} fileKind={activeFileType!} />
          </InlineCompleteProvider>
        </div>
      )}

      {isSvg && (
        <div className={styles.officeStackItem}>
          <LazySvgViewer filePath={selectedFile} />
        </div>
      )}

      {isImage && !isSvg && (
        <div className={styles.officeStackItem}>
          <LazyImageViewer filePath={selectedFile} />
        </div>
      )}

      {isPdf && (
        <div className={styles.officeStackItem}>
          <LazyPdfViewer filePath={selectedFile} />
        </div>
      )}

      {/* Binary / archive / audio / video modes currently have no in-app
         viewer; if the active file is one of those, render an empty
         hint so the editor pane isn't blank. */}
      {selectedFile && !isOffice && !isEditableText && !isImage && !isPdf && (
        <UnsupportedFileHint fileKind={activeFileType!} fileName={selectedFile} />
      )}
    </div>
  );
};

/**
 * Fallback hint shown for file kinds the editor doesn't yet support
 * (binary, archive, audio, video, etc.). Includes a button to open
 * the file with the system's default application, mirroring the
 * behavior of the existing Office tabs that delegate unknown formats
 * to the host OS.
 */
const UnsupportedFileHint: React.FC<{ fileKind: FileKind; fileName: string }> = ({
  fileKind,
  fileName,
}) => {
  const displayName = fileName.split(/[\\/]/).pop() ?? fileName;
  const openExternal = async () => {
    try {
      const { invoke } = await import('@tauri-apps/api/core');
      await invoke('open_with_default_app', { path: fileName });
    } catch {
      /* noop */
    }
  };

  return (
    <div className={styles.officeStackItem}>
      <div className={styles.noFileHint}>
        <Sparkles className={styles.hintIcon} size={20} />
        <span>
          不支持在 inkuo 中预览 <code>{displayName}</code>（类型：{fileKind}）。
          <button
            onClick={openExternal}
            style={{
              marginLeft: 8,
              background: 'none',
              border: '1px solid var(--border-color)',
              padding: '2px 10px',
              borderRadius: 4,
              cursor: 'pointer',
              color: 'inherit',
            }}
          >
            用系统应用打开
          </button>
        </span>
      </div>
    </div>
  );
};
