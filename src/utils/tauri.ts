export function isTauriRuntime(): boolean {
  if (typeof window === 'undefined') {
    return false;
  }
  // Tauri v2 exposes both `__TAURI_INTERNALS__` (the IPC bridge) and
  // `__TAURI__` (the legacy global). Either is sufficient to confirm we
  // are running inside the Tauri shell and may call `invoke`/`listen`.
  const internals = (window as Window & { __TAURI_INTERNALS__?: unknown })
    .__TAURI_INTERNALS__;
  const tauri = (window as Window & { __TAURI__?: unknown }).__TAURI__;
  return typeof internals !== 'undefined' || typeof tauri !== 'undefined';
}
