import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from 'react';
import { lazy, Suspense } from 'react';
import { type Extension } from '@codemirror/state';
import { type ReactCodeMirrorRef } from '@uiw/react-codemirror';
import CodeMirror from '@uiw/react-codemirror';
import { Compartment } from '@codemirror/state';
import { EditorView } from '@codemirror/view';
import { Sparkles } from 'lucide-react';
import { useEditorStore, useSidebarStore, useSettingsStore, useContextMenuStore, useEditorHandleStore, SETTINGS_TAB_ID, CLOUD_TAB_ID, type OpenTab, type EditorCommands } from '../../store';
import { undo as cmUndo, redo as cmRedo, undoDepth, redoDepth } from '@codemirror/commands';
import { detectFileKind, type FileKind } from '../../types';
import { DiffOverlay } from './DiffOverlay';
import { InlineCompleteProvider } from '../inline-complete';
import { useDocumentLoader } from './useDocumentLoader';
import { useDocumentSave } from './useDocumentSave';
import { useExternalFileSync } from './useExternalFileSync';
import { ExternalFileConflictBanner } from './ExternalFileConflictBanner';
import { decideExternalRefresh } from './externalFileConflict';
import { createDiffDecorationsField } from './diffDecorationsField';
import { createEditorExtensions, languageExtensionForKind } from './editorExtensions';
import { LazyImageViewer, LazyPdfViewer, LazySvgViewer } from './LazyMediaViewers';
import { EditorBody } from './EditorBody';
import { useEditorInlineCompletion } from './useEditorInlineCompletion';
import { useEditorKeyboardShortcuts, useEditorSelectionSync } from './useEditorInteraction';
import { shouldMountOfficeTab } from './officeTabRetention';
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
  const discardDirtyRefreshTokenRef = useRef<number | null>(null);
  const [hasExternalConflict, setHasExternalConflict] = useState(false);
  const dirtyStateRef = useRef(isDirty);
  dirtyStateRef.current = isDirty;
  // Container ref for the editor. The context-menu listener attaches
  // here in capture phase so we always see the event before CM does.
  const editorContainerRef = useRef<HTMLDivElement | null>(null);

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
    const decision = decideExternalRefresh(dirtyStateRef.current, false);
    if (decision === 'show-conflict') {
      setHasExternalConflict(true);
      return;
    }
    setRefreshToken((current) => current + 1);
  }, []);

  const handleDirtyLoadConflict = useCallback(() => {
    setHasExternalConflict(true);
  }, []);

  const handleKeepLocalVersion = useCallback(() => {
    setHasExternalConflict(false);
  }, []);

  const handleReloadExternalVersion = useCallback(() => {
    setHasExternalConflict(false);
    setRefreshToken((current) => {
      const next = current + 1;
      discardDirtyRefreshTokenRef.current = next;
      return next;
    });
  }, []);

  useDocumentLoader(selectedFile, currentMetadata ? {
    content: currentMetadata.content,
    mtime: currentMetadata.mtime,
  } : null, refreshToken, discardDirtyRefreshTokenRef.current === refreshToken, handleDirtyLoadConflict);
  useExternalFileSync(selectedFile, requestDocumentRefresh);
  const handleSave = useDocumentSave(selectedFile, currentContent, isDirty);
  // Subscribe to the editor handle store's setters so we can publish
  // the live CodeMirror commands and capabilities to the top-bar menu.
  // Done up here (before `handleUpdateWithCapabilities`) so the wrapper
  // callback can reference the setter without hitting a TDZ.
  const setEditorCommands = useEditorHandleStore((s) => s.setCommands);
  const setEditorCapabilities = useEditorHandleStore((s) => s.setCapabilities);
  const handleUpdate = useEditorSelectionSync(selectedFile, currentContent, selection, editorRef);
  // Wrap selection sync so we also probe undo/redo/selection state on
  // every CodeMirror transaction. The hook returns a callback of the
  // same signature, so it's a drop-in replacement for `onUpdate`.
  const handleUpdateWithCapabilities = useCallback(
    (update: Parameters<typeof handleUpdate>[0]) => {
      handleUpdate(update);
      const view = editorRef.current?.view;
      if (view && !(view as { isDestroyed?: boolean }).isDestroyed) {
        // Use the read-only `undoDepth` / `redoDepth` accessors, NOT
        // the `cmUndo` / `cmRedo` commands. The commands are functions
        // that fire-and-dispatch a transaction if the corresponding
        // depth is positive — calling them here would create an
        // infinite `dispatch → onUpdate → dispatch → ...` loop on
        // every transaction. The depth accessors just read state.
        setEditorCapabilities({
          canUndo: undoDepth(view.state) > 0,
          canRedo: redoDepth(view.state) > 0,
          hasSelection: !view.state.selection.main.empty,
        });
      }
    },
    [handleUpdate, editorRef, setEditorCapabilities],
  );
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

  // Suppress the webview's native context menu inside the markdown /
  // code / text editor and route right-clicks to the app's
  // `ContextMenu`. We attach listeners in *two* places for
  // robustness:
  //   1. `editorContainerRef` (capture phase) — primary path. Any
  //      contextmenu landing inside the editor container is captured
  //      before any descendant handler can swallow it.
  //   2. `document` (capture phase) — defensive fallback. Some
  //      WebKitGTK/WebView2 builds route the event to the document
  //      before the container ever sees it; the document listener
  //      filters by `node.contains(target)` so it only handles
  //      matches inside the editor.
  //
  // Two routes:
  //   - non-empty browser selection → `kind: 'selection'`, the
  //     existing AI / search / copy menu.
  //   - empty / collapsed selection → `kind: 'editor'`, a small
  //     editor menu (cut / copy / paste / find / replace / select-all
  //     + an "用 AI 处理当前文件" submenu that reads the *live* CM
  //     buffer so unsaved edits are reflected).
  useEffect(() => {
    if (!selectedFile) return undefined;
    const handle = (e: MouseEvent) => {
      const node = editorContainerRef.current;
      if (!node) return;
      const target = e.target as Node | null;
      if (!target || !node.contains(target)) return;
      e.preventDefault();
      e.stopPropagation();
      const text = globalThis.window.getSelection()?.toString() ?? '';
      const trimmed = text.trim();
      const x = e.clientX;
      const y = e.clientY;
      if (trimmed.length > 0) {
        useContextMenuStore.getState().open({
          kind: 'selection',
          path: selectedFile,
          x,
          y,
          selectionText: text,
        });
        return;
      }
      const view = editorRef.current?.view ?? null;
      const guardView = (): EditorView | null => {
        if (!view || (view as { isDestroyed?: boolean }).isDestroyed) return null;
        return view;
      };
      const commands: EditorCommands = {
        cut: () => {
          const v = guardView();
          if (!v) return;
          v.focus();
          // CM's default cut keymap action is exposed as a
          // `Command` `defaultKeymap` lookup. Calling it
          // through the editor's own command palette keeps
          // the behavior aligned with the keymap binding.
          // (The simpler `document.execCommand('cut')` is
          // deprecated but still works for clipboard handoff
          // to the system clipboard.)
          document.execCommand('cut');
        },
        copy: () => {
          const v = guardView();
          if (!v) return;
          v.focus();
          document.execCommand('copy');
        },
        paste: () => {
          const v = guardView();
          if (!v) return;
          v.focus();
          document.execCommand('paste');
        },
        selectAll: () => {
          const v = guardView();
          if (!v) return;
          v.focus();
          v.dispatch({ selection: { anchor: 0, head: v.state.doc.length } });
        },
        find: () => {
          const v = guardView();
          if (!v) return;
          v.focus();
          // Synthesize Ctrl+F — CM's `search` keymap is registered
          // on the CodeMirror container; firing a keydown on the
          // active element routes the binding to the right view.
          const ev = new KeyboardEvent('keydown', {
            key: 'f',
            code: 'KeyF',
            ctrlKey: true,
            bubbles: true,
            cancelable: true,
          });
          (v.contentDOM ?? v.dom).dispatchEvent(ev);
        },
        replace: () => {
          const v = guardView();
          if (!v) return;
          v.focus();
          const ev = new KeyboardEvent('keydown', {
            key: 'h',
            code: 'KeyH',
            ctrlKey: true,
            bubbles: true,
            cancelable: true,
          });
          (v.contentDOM ?? v.dom).dispatchEvent(ev);
        },
        readContent: () => {
          const v = guardView();
          if (!v) return '';
          return v.state.doc.toString();
        },
        undo: () => {
          const v = guardView();
          if (!v) return;
          v.focus();
          cmUndo(v);
        },
        redo: () => {
          const v = guardView();
          if (!v) return;
          v.focus();
          cmRedo(v);
        },
      };
      useContextMenuStore.getState().open({
        kind: 'editor',
        path: selectedFile,
        x,
        y,
        editorCommands: commands,
      });
    };
    // Primary: container-level capture. CM's own event listeners
    // run in bubble phase, so this fires first.
    const node = editorContainerRef.current;
    if (node) {
      node.addEventListener('contextmenu', handle, { capture: true });
    }
    // Defensive: document-level capture. Some WebKit builds re-deliver
    // the event through `document` before the container sees it.
    document.addEventListener('contextmenu', handle, { capture: true });
    return () => {
      if (node) {
        node.removeEventListener('contextmenu', handle, { capture: true } as EventListenerOptions);
      }
      document.removeEventListener('contextmenu', handle, { capture: true } as EventListenerOptions);
    };
  }, [selectedFile, editorRef]);

  const handleChange = useCallback((value: string) => {
    if (selectedFile) {
      setContent(selectedFile, value);
    }
  }, [selectedFile, setContent]);

  // Publish the live CodeMirror commands to the shared editor handle
  // store so the top-bar menu (TitleBar) can dispatch cut/copy/paste/
  // undo/redo/find/replace without threading the editor's ref through
  // the component tree. Mirrors the snapshot we already push into the
  // context menu on right-click — same closure shape, different sink.
  useEffect(() => {
    if (!selectedFile) {
      setEditorCommands(null);
      return undefined;
    }
    const view = editorRef.current?.view ?? null;
    if (!view) return undefined;

    const readCapabilities = () => {
      try {
        // Use the read-only `undoDepth` / `redoDepth` accessors instead
        // of invoking the `cmUndo` / `cmRedo` commands. The commands
        // dispatch a transaction when their depth is positive, which
        // would fire `onUpdate` and recurse back into this same probe.
        const canUndo = undoDepth(view.state) > 0;
        const canRedo = redoDepth(view.state) > 0;
        const hasSelection = !view.state.selection.main.empty;
        setEditorCapabilities({ canUndo, canRedo, hasSelection });
      } catch {
        setEditorCapabilities({ canUndo: false, canRedo: false, hasSelection: false });
      }
    };

    const commands: EditorCommands = {
      cut: () => {
        if (!view || (view as { isDestroyed?: boolean }).isDestroyed) return;
        view.focus();
        document.execCommand('cut');
      },
      copy: () => {
        if (!view || (view as { isDestroyed?: boolean }).isDestroyed) return;
        view.focus();
        document.execCommand('copy');
      },
      paste: () => {
        if (!view || (view as { isDestroyed?: boolean }).isDestroyed) return;
        view.focus();
        document.execCommand('paste');
      },
      selectAll: () => {
        if (!view || (view as { isDestroyed?: boolean }).isDestroyed) return;
        view.focus();
        view.dispatch({ selection: { anchor: 0, head: view.state.doc.length } });
      },
      find: () => {
        if (!view || (view as { isDestroyed?: boolean }).isDestroyed) return;
        view.focus();
        const ev = new KeyboardEvent('keydown', {
          key: 'f',
          code: 'KeyF',
          ctrlKey: true,
          bubbles: true,
          cancelable: true,
        });
        (view.contentDOM ?? view.dom).dispatchEvent(ev);
      },
      replace: () => {
        if (!view || (view as { isDestroyed?: boolean }).isDestroyed) return;
        view.focus();
        const ev = new KeyboardEvent('keydown', {
          key: 'h',
          code: 'KeyH',
          ctrlKey: true,
          bubbles: true,
          cancelable: true,
        });
        (view.contentDOM ?? view.dom).dispatchEvent(ev);
      },
      readContent: () => {
        if (!view || (view as { isDestroyed?: boolean }).isDestroyed) return '';
        return view.state.doc.toString();
      },
      undo: () => {
        if (!view || (view as { isDestroyed?: boolean }).isDestroyed) return;
        view.focus();
        cmUndo(view);
      },
      redo: () => {
        if (!view || (view as { isDestroyed?: boolean }).isDestroyed) return;
        view.focus();
        cmRedo(view);
      },
    };

    setEditorCommands(commands);
    readCapabilities();

    // Probe the history fields on every CodeMirror transaction. The
    // hook here is `EditorView.updateListener`, but we're already
    // attached via `onUpdate` (selection sync) — extending the same
    // listener would be ideal, but keeping the publish path colocated
    // with the commands makes the lifecycle easier to reason about.
    // We use a one-line `view.dispatch` wrapper via a `MutationObserver`
    // is overkill; instead, defer to `useEditorSelectionSync` which
    // already runs on every doc/selection change. We just need an
    // additional probe inside it — see the `handleUpdate` call below.

    return () => {
      setEditorCommands(null);
      setEditorCapabilities({ canUndo: false, canRedo: false, hasSelection: false });
    };
  }, [selectedFile, editorRef, setEditorCommands, setEditorCapabilities]);

  const inPreviewMode = selectedFile ? !!isPreviewMode[selectedFile] : false;

  // Sync the editor font size setting into a CSS variable so the
  // CodeMirror theme reads it without rebuilding extensions. The view
  // menu's 放大/缩小/重置 size buttons all funnel through `updateSetting
  // ('editor_font_size', ...)`, so this single effect covers every
  // trigger (top-bar menu, settings panel, future keybindings, etc.).
  useEffect(() => {
    if (typeof document === 'undefined') return;
    document.documentElement.style.setProperty(
      '--editor-font-size',
      `${settings.editor_font_size}px`,
    );
  }, [settings.editor_font_size]);

  const editorExtensions = useMemo(() => {
    return createEditorExtensions({
      diffDecorationsField: diffDecorationsCompartment.of(diffDecorationsField),
      inlineCompletionKeyHandler,
      inlineAutoTrigger,
      autoTriggerStateRef,
      language,
    });
  }, [diffDecorationsCompartment, diffDecorationsField, inlineCompletionKeyHandler, inlineAutoTrigger, autoTriggerStateRef, language]);

  return (
    <div ref={editorContainerRef} className={`${styles.editorContainer} editorContainer`} data-inline-complete-styles={inlineCompleteStyles}>
      {hasExternalConflict && selectedFile && (
        <ExternalFileConflictBanner
          fileName={selectedFile.split(/[\\/]/).pop() ?? selectedFile}
          onKeepLocal={handleKeepLocalVersion}
          onReloadFromDisk={handleReloadExternalVersion}
        />
      )}
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
              onUpdate={handleUpdateWithCapabilities}
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
          选择一个文件开始编辑，或按 <kbd>Ctrl</kbd>+<kbd>Shift</kbd>+<kbd>L</kbd> 调用 AI 助手
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
  import('./BapbongWordEditor').then((m) => ({ default: m.BapbongWordEditor }))
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

  // Word and Excel keep their authoritative unsaved model inside the editor
  // engine. A dirty editor therefore survives tab switches. Clean inactive
  // editors still unmount to release their substantial layout/runtime cost.
  // The parked wrapper uses `display: none`, which keeps React/editor state
  // alive without feeding hidden geometry into ResizeObserver loops.
  if (!shouldMountOfficeTab(isActive, tab.isDirty)) {
    return null;
  }

  const editor = fileType === 'word' ? (() => {
    const tabCached = officeState?.docxBuffer ?? null;
    return (
      <Suspense fallback={<RouteFallback label="正在加载 Word 编辑器" />}>
        <WordEditor
          filePath={tab.path}
          fileName={tab.name}
          initialBuffer={tabCached ? new Uint8Array(tabCached) : null}
          isActive={isActive}
        />
      </Suspense>
    );
  })() : (
    <Suspense fallback={<RouteFallback label="正在加载 Excel 编辑器" />}>
      <ExcelEditor
        filePath={tab.path}
        fileName={tab.name}
        isActive={isActive}
      />
    </Suspense>
  );

  return (
    <div
      className={`${styles.officeStackItem} ${
        isActive ? '' : styles.officeStackItemParked
      }`}
      aria-hidden={!isActive}
    >
      {editor}
    </div>
  );
};

/*
 * Active non-Office routes share the same absolute stack slot as Office.
 * Keeping this wrapper at the Editor level means Settings/Cloud/empty-state
 * switches no longer replace the whole tree and accidentally unmount a dirty
 * parked Office editor.
 */
const ActiveEditorRoute: React.FC<{ children: React.ReactNode }> = ({ children }) => (
  <div className={styles.officeStackItem}>{children}</div>
);

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

  // Decide which top-level viewer to render. Dirty Office editors stay in the
  // stack even when a text/media/settings/cloud tab becomes active. This is
  // essential because their unsaved model cannot be reconstructed from the
  // disk-backed editor store after an unmount.
  const isOffice = Boolean(
    activeFileType && (OFFICE_KINDS as readonly string[]).includes(activeFileType),
  );
  const isEditableText =
    activeFileType === 'markdown' ||
    activeFileType === 'text' ||
    activeFileType === 'code' ||
    activeFileType === 'config' ||
    activeFileType === 'data';
  const isImage = activeFileType === 'image';
  const isPdf = activeFileType === 'pdf';

  let activeNonOfficeRoute: ReactNode = null;
  if (isSettingsTab) {
    activeNonOfficeRoute = <SettingsState />;
  } else if (activeTabId === CLOUD_TAB_ID) {
    activeNonOfficeRoute = <CloudState />;
  } else if (!selectedFile) {
    activeNonOfficeRoute = <EmptyState />;
  } else if (!isOffice && isEditableText) {
    activeNonOfficeRoute = (
      <InlineCompleteProvider>
        <EditorContent editorRef={editorRef} fileKind={activeFileType!} />
      </InlineCompleteProvider>
    );
  } else if (!isOffice && isSvg) {
    activeNonOfficeRoute = <LazySvgViewer filePath={selectedFile} />;
  } else if (!isOffice && isImage) {
    activeNonOfficeRoute = <LazyImageViewer filePath={selectedFile} />;
  } else if (!isOffice && isPdf) {
    activeNonOfficeRoute = <LazyPdfViewer filePath={selectedFile} />;
  } else if (!isOffice) {
    activeNonOfficeRoute = (
      <UnsupportedFileHint fileKind={activeFileType!} fileName={selectedFile} />
    );
  }

  return (
    <div className={styles.officeStack}>
      {officeTabs.map(({ tab, fileType }) => {
        const isActive = tab.id === activeTabId && activeFileType === fileType;

        return (
          <OfficeTabRenderer
            key={tab.id}
            tab={tab}
            fileType={fileType}
            isActive={isActive}
          />
        );
      })}
      {activeNonOfficeRoute && (
        <ActiveEditorRoute>{activeNonOfficeRoute}</ActiveEditorRoute>
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
  );
};
