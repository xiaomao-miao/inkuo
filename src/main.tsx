import React from 'react';
import ReactDOM from 'react-dom/client';
import App from './App';

/**
 * Install a few window-level guards before React mounts. These exist
 * because the app is rendered inside a Tauri WebView (which is just
 * a Chromium window) and would otherwise inherit the browser's stock
 * shortcuts and context menus — neither of which make sense for a
 * desktop document editor.
 *
 *   1. Ctrl + wheel and Ctrl + +/-/0  → suppress. Chromium's
 *      `maximum-scale=1.0` viewport meta isn't enforced on desktop
 *      windows; without a JS guard, the entire app zooms in/out.
 *      We pin the page at 1x by swallowing the events that would
 *      otherwise trigger the browser's built-in zoom.
 *   2. contextmenu inside the AI panel chat view → suppress the
 *      native "View source / Inspect" menu. The custom
 *      SelectionQuickActions toolbar handles right-clicks on message
 *      text; we don't want the Chromium DevTools menu replacing it.
 *      Other regions (sidebar, editor) still get the OS context
 *      menu via their own UI, and bare background right-clicks
 *      intentionally fall through to the native menu so the user
 *      can reach DevTools if they really need to.
 *
 * Both guards short-circuit early on `e.ctrlKey && e.metaKey` so we
 * never break legitimate text-editing or accessibility shortcuts.
 */
function installGlobalGuards(): void {
  const isMac = navigator.platform.toLowerCase().includes('mac');

  // Guard 1: kill browser zoom. We only swallow events that *carry*
  // the Ctrl modifier — the wheel alone (used for scrolling) is left
  // alone. On macOS the modifier is Meta, so we accept both.
  const isZoomKey = (e: KeyboardEvent): boolean => {
    const mod = isMac ? e.metaKey : e.ctrlKey;
    if (!mod) return false;
    // + / = / - / 0 → all map to "browser zoom" by default. We
    // accept ANY key alongside Ctrl/Cmd as a zoom gesture to be
    // safe: Ctrl+Shift+I, Ctrl+R, Ctrl+T etc. all have their own
    // handlers in App and we don't want to break those. The fix is
    // narrowly to swallow wheel + the four zoom keys.
    return (
      e.key === '+' ||
      e.key === '=' ||
      e.key === '-' ||
      e.key === '_' ||
      e.key === '0'
    );
  };

  window.addEventListener(
    'wheel',
    (e) => {
      // Ctrl+wheel is the desktop browser's zoom shortcut. Cmd+wheel
      // on macOS too. Block it so the editor surface stays at 1x.
      if (e.ctrlKey || e.metaKey) {
        e.preventDefault();
      }
    },
    { passive: false },
  );

  window.addEventListener(
    'keydown',
    (e) => {
      if (isZoomKey(e)) {
        e.preventDefault();
      }
    },
    { capture: true },
  );

  // Guard 2: contextmenu inside the AI panel. The selector is bound
  // at runtime because the chat view mounts after this script runs.
  // We re-check on every event — cheap, and avoids the complexity of
  // tearing down / re-binding on panel remount.
  document.addEventListener('contextmenu', (e) => {
    const target = e.target as Element | null;
    if (!target) return;
    // The AI panel's scrollable message container carries
    // `data-aipanel-chat-content` (set in ChatView.tsx). Matching
    // against that attribute is intentional: the AI panel is a
    // small island inside the app, and we don't want to suppress
    // context menus elsewhere (sidebar, file tree, editor all have
    // their own right-click affordances and should pass through).
    const inChatView = target.closest('[data-aipanel-chat-content]');
    if (!inChatView) return;
    e.preventDefault();
  });
}

installGlobalGuards();

ReactDOM.createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>
);
