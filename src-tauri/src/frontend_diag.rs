//! Frontend diagnostics compatibility shim.
//!
//! Older builds closed the configuration-created webview during `setup`,
//! waited synchronously for its label to disappear and rebuilt it with a JS
//! console bridge. Apart from risking a permanently windowless application
//! when rebuilding failed, forwarding every `console.log`/`debug` call caused
//! an IPC + file-flush storm during long AI tasks and could starve the renderer.
//!
//! Keep the legacy setup call harmless. Diagnostics remain available through
//! DevTools and normal Rust tracing; they must never mutate the application's
//! primary window lifecycle.

/// Legacy setup hook retained so the central command-registration file does
/// not need a coordinated edit. Intentionally leaves every webview untouched.
pub fn rebuild_main_webview_with_diag(_app: &tauri::App) {
    tracing::debug!(
        "frontend diagnostics: preserving the configuration-created webview; console IPC bridge disabled"
    );
}
