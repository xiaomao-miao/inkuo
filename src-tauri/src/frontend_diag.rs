//! Frontend diagnostic bridge.
//!
//! Release builds hide the WebView2 DevTools behind a right-click menu,
//! and when the renderer fails silently the user can't see any console
//! output at all. This module owns the JS bootstrap that hooks
//! `window.console.*` + `window.onerror` + `unhandledrejection` and
//! forwards each event to the `frontend_log` Tauri command, which
//! appends to `<app_data_dir>/frontend-console.log`.
//!
//! Pulled out of `lib.rs` so the main entry stays focused on app
//! setup; the diagnostic window-rebuild ceremony was previously a
//! ~140-line block in the middle of `run()` and made the surrounding
//! logic hard to read.

use tauri::{Manager, WebviewWindowBuilder};

/// Init script that hooks the browser-side console + error events.
///
/// Returns a static literal so callers can attach it via
/// `WebviewWindowBuilder::initialization_script`. The body is *not*
/// aggressively minified so debug builds (F12 / DevTools) stay
/// readable.
pub const DIAG_INIT_JS: &str = r#"
    (function () {
      try {
        if (window.__inkuoDiagInstalled) return;
        window.__inkuoDiagInstalled = true;
        var orig = {
          log: console.log, info: console.info, warn: console.warn,
          error: console.error, debug: console.debug
        };
        function fmt() {
          var parts = [];
          for (var i = 0; i < arguments.length; i++) {
            var a = arguments[i];
            try {
              if (a instanceof Error) {
                parts.push(a.name + ': ' + a.message + '\n' + (a.stack || ''));
              } else if (typeof a === 'object') {
                parts.push(JSON.stringify(a));
              } else {
                parts.push(String(a));
              }
            } catch (e) {
              parts.push(Object.prototype.toString.call(a));
            }
          }
          return parts.join(' ');
        }
        function send(level, args, stack) {
          try {
            var message = fmt.apply(null, args);
            var payload = {
              level: level,
              message: message,
              url: location.href,
              stack: stack || null
            };
            if (window.__TAURI__ && window.__TAURI__.core) {
              window.__TAURI__.core.invoke('frontend_log', { payload: payload });
            }
          } catch (e) { /* swallow */ }
        }
        ['log','info','warn','error','debug'].forEach(function (k) {
          console[k] = function () {
            try { orig[k].apply(console, arguments); } catch (e) {}
            send(k, Array.prototype.slice.call(arguments), null);
          };
        });
        window.addEventListener('error', function (ev) {
          var e = ev.error || ev.message;
          var msg = (e && e.message) || String(e);
          var stack = (e && e.stack) || null;
          send('error', ['[window.onerror] ' + msg + ' @ ' + (ev.filename || '') + ':' + (ev.lineno || 0) + ':' + (ev.colno || 0)], stack);
        });
        window.addEventListener('unhandledrejection', function (ev) {
          var r = ev.reason;
          var msg = (r && r.message) || (typeof r === 'string' ? r : JSON.stringify(r));
          var stack = (r && r.stack) || null;
          send('error', ['[unhandledrejection] ' + msg], stack);
        });
        console.log('[inkuo-diag] frontend diagnostic bridge installed at ' + location.href);
      } catch (e) { /* never throw out of init script */ }
    })();
"#;

/// Close every auto-created webview window (tauri.conf.json spawns them
/// before `setup` runs) and rebuild the first one with the diagnostic
/// init script attached. We use `from_config(...).build()` so the
/// conf-defined window settings (size, decorations, theme, …) still
/// apply.
pub fn rebuild_main_webview_with_diag(app: &tauri::App) {
    let existing_labels: Vec<String> =
        app.webview_windows().keys().cloned().collect();
    for label in &existing_labels {
        if let Some(w) = app.get_webview_window(label) {
            let _ = w.close();
        }
    }
    // Give the window a moment to actually be destroyed. On Windows
    // this is normally synchronous but wry's runtime may still hold
    // the label briefly.
    for _ in 0..20 {
        if app.webview_windows().is_empty() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }

    let win_configs_snapshot = app.config().app.windows.clone();
    if let Some(mut win_config) = win_configs_snapshot.into_iter().next() {
        // Defensively rename to a unique label so a stale wry entry
        // can't collide with the rebuild.
        win_config.label = format!(
            "diag-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis())
                .unwrap_or(0)
        );
        match WebviewWindowBuilder::from_config(app, &win_config) {
            Ok(builder) => {
                let builder = builder.initialization_script(DIAG_INIT_JS);
                match builder.build() {
                    Ok(_) => tracing::info!(
                        "FRONTEND DIAG: rebuilt main webview (label={}) with console bridge",
                        win_config.label
                    ),
                    Err(e) => tracing::warn!(
                        "FRONTEND DIAG: failed to build main webview: {e}"
                    ),
                }
            }
            Err(e) => tracing::warn!(
                "FRONTEND DIAG: from_config failed, leaving default webview: {e}"
            ),
        }
    }
}
