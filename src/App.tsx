import { useEffect } from 'react';
import { Layout } from './components/layout';
import { CmdK } from './components/cmdk';
import { useSettingsStore } from './store';
import { useSidebarStore } from './store/sidebarStore';
import { adjustColor } from './utils/color';
import { invoke } from '@tauri-apps/api/core';
import type { FileEntry } from './types';
import './styles/design-tokens.css';
import './styles/global.css';

function App() {
  const settings = useSettingsStore((state) => state.settings);

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
  }, []);

  return (
    <>
      <Layout />
      <CmdK />
    </>
  );
}

export default App;
