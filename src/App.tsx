import { useEffect, useMemo } from 'react';
import { Layout } from './components/layout';
import { CmdK } from './components/cmdk';
import { WelcomePage } from './components/welcome';
import { WorkspaceBootstrap } from './components/WorkspaceBootstrap';
import { useSettingsStore, useEditorStore } from './store';
import { useSidebarStore } from './store/sidebarStore';
import { adjustColor } from './utils/color';
import './styles/design-tokens.css';
import './styles/global.css';

function App() {
  const settings = useSettingsStore((state) => state.settings);
  const workspacePath = useSidebarStore((state) => state.workspacePath);

  // For new windows opened via "File > New Window", the Rust side sets
  // `window.__INKUO_FRESH_WINDOW__` via initialization_script. Reset the
  // live workspace view *before* the first render so the welcome page
  // shows immediately and the new window starts on a blank slate.
  // Per-workspace snapshots (`workspaceSnapshots`) and user preferences
  // (theme, API configs, panel widths, AI panel UI mode) are intentionally
  // preserved — so reopening the same workspace from the new window
  // restores the user's tabs and AI chat history.
  //
  // We do this inside an effect (running once at mount) rather than at
  // module top-level so it executes in a render commit, not during module
  // evaluation. The check runs in the very first commit, well before the
  // first paint can show a stale workspace, because effects with an empty
  // dep array run synchronously after layout.
  useEffect(() => {
    // The Rust side sets `window.__INKUO_FRESH_WINDOW__ = true` via
    // `initialization_script` for windows opened from "File > New Window"
    // so we can wipe the live workspace view (open tabs, document cache,
    // sidebar expansion, AI panel live state) without disturbing
    // per-workspace snapshots or user preferences.
    //
    // The flag stays set for the lifetime of the page, which means a hard
    // reload of *this* window would otherwise rerun the reset and silently
    // throw away the user's tabs. Delete it after reading so a refresh of
    // an existing window keeps the workspace intact.
    const freshWindow = (window as unknown as { __INKUO_FRESH_WINDOW__?: boolean }).__INKUO_FRESH_WINDOW__ === true;
    if (!freshWindow) {
      return;
    }
    delete (window as unknown as { __INKUO_FRESH_WINDOW__?: boolean }).__INKUO_FRESH_WINDOW__;

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
    // Drop any cached document content from the previous window so the
    // new one doesn't show stale buffers for tabs the user never opened
    // here.
    useEditorStore.setState({ documentContents: {} });
    // The AI panel's live sessions ride along inside
    // `workspaceSnapshots.aiSessions`. The aiPanelStore mounts with its
    // default empty session on a fresh webview, which is what we want for
    // the new window's welcome page.
  }, []);

  const accentHover = useMemo(
    () => adjustColor(settings.accent_color, 20),
    [settings.accent_color],
  );
  const accentActive = useMemo(
    () => adjustColor(settings.accent_color, -20),
    [settings.accent_color],
  );

  useEffect(() => {
    document.documentElement.setAttribute('data-theme', settings.theme);

    document.documentElement.style.setProperty('--accent-primary', settings.accent_color);
    document.documentElement.style.setProperty('--accent-hover', accentHover);
    document.documentElement.style.setProperty('--accent-active', accentActive);
  }, [settings.theme, settings.accent_color, accentHover, accentActive]);

  // No manual polling here: filesystem changes are delivered through the
  // Tauri `file-change` event (see `useWorkspaceFileWatcher`), which debounces
  // and refreshes only the affected parent directory. The previous 500ms
  // full-tree poll was both costly on large workspaces and redundant with
  // the event-driven watcher.

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
