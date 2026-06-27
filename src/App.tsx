import { useEffect } from 'react';
import { Layout } from './components/layout';
import { CmdK } from './components/cmdk';
import { WelcomePage } from './components/welcome';
import { WorkspaceBootstrap } from './components/WorkspaceBootstrap';
import { useSettingsStore, useEditorStore } from './store';
import { useSidebarStore } from './store/sidebarStore';
import { adjustColor } from './utils/color';
import { invoke } from '@tauri-apps/api/core';
import type { FileEntry } from './types';
import './styles/design-tokens.css';
import './styles/global.css';

// For new windows opened via "File > New Window", the Rust side sets
// window.__INKUO_FRESH_WINDOW__ via initialization_script (URL query strings
// don't propagate through Tauri 2's WebviewUrl::App in dev mode). Reset the
// live workspace view *before* the first render so the welcome page shows
// immediately and the new window starts on a blank slate. Per-workspace
// snapshots (`workspaceSnapshots`) and user preferences (theme, API configs,
// panel widths, AI panel UI mode) are intentionally preserved — so reopening
// the same workspace from the new window restores the user's tabs and AI
// chat history.
if ((window as unknown as { __INKUO_FRESH_WINDOW__?: boolean }).__INKUO_FRESH_WINDOW__ === true) {
  useSidebarStore.setState({
    workspacePath: null,
    directoryCache: new Map(),
    expandedDirs: new Set(),
    loadingDirs: new Set(),
    selectedFile: null,
    isLoading: false,
    openTabs: [],
    activeTabId: null,
    knowledgeSelectMode: false,
    knowledgeCheckedPaths: new Set(),
    inlineEdit: null,
    // workspaceSnapshots is preserved on purpose.
  });
  // Drop any cached document content from the previous window so the new one
  // doesn't show stale buffers for tabs the user never opened here.
  useEditorStore.setState({ documentContents: {} });
  // The AI panel's live sessions are not persisted at the aiPanelStore level
  // — they ride along inside `workspaceSnapshots.aiSessions`. We don't touch
  // them here; the aiPanelStore mounts with its default empty session on a
  // fresh webview, which is what we want for the new window's welcome page.
}

function App() {
  const settings = useSettingsStore((state) => state.settings);
  const workspacePath = useSidebarStore((state) => state.workspacePath);

  useEffect(() => {
    document.documentElement.setAttribute('data-theme', settings.theme);

    document.documentElement.style.setProperty('--accent-primary', settings.accent_color);
    document.documentElement.style.setProperty('--accent-hover', adjustColor(settings.accent_color, 20));
    document.documentElement.style.setProperty('--accent-active', adjustColor(settings.accent_color, -20));
  }, [settings.theme, settings.accent_color]);

  // Workspace polling (500ms) — reloads all cached directories from disk
  // every 500ms so the file tree reflects external changes immediately,
  // without relying on the OS-level inotify / PollWatcher (which has known
  // reliability issues on some Linux filesystems and editor atomic-rename
  // saves). Each refresh is one readdir per cached directory.
  useEffect(() => {
    if (!workspacePath) return;

    const id = setInterval(async () => {
      const store = useSidebarStore.getState();
      const ws = store.workspacePath;
      if (!ws) return;
      const paths = new Set<string>([ws, ...store.directoryCache.keys()]);
      for (const p of paths) {
        try {
          const entries = await invoke<FileEntry[]>('list_directory', { path: p });
          store.setCachedChildren(p, entries);
        } catch {
          /* ignore */
        }
      }
    }, 500);
    return () => clearInterval(id);
  }, [workspacePath]);

  // If no workspace is set, show the welcome page
  if (!workspacePath) {
    return (
      <>
        <WorkspaceBootstrap />
        <WelcomePage />
        <CmdK />
      </>
    );
  }

  return (
    <>
      <WorkspaceBootstrap />
      <Layout />
      <CmdK />
    </>
  );
}

export default App;
