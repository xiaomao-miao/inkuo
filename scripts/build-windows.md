# Windows release build (MSI + NSIS)

Run on a **Windows 10/11 host** with the following toolchain:

| Tool | Version | Install |
|---|---|---|
| Node.js | 20 LTS | https://nodejs.org/ |
| pnpm | 9+ | `npm i -g pnpm` |
| Rust | stable (MSVC) | `rustup default stable-x86_64-pc-windows-msvc` |
| WebView2 Runtime | latest | Pre-installed on Windows 10 19045+ / Windows 11 |
| Visual Studio Build Tools | 2022, "Desktop C++" workload | https://visualstudio.microsoft.com/downloads/ |
| WiX Toolset | 3.14.1 | `https://github.com/wixtoolset/wix3/releases/download/wix3141rtm/wix314.exe` (defaults to `C:\Program Files (x86)\WiX Toolset v3.14\bin`) |
| NSIS | 3.10+ | https://sourceforge.net/projects/nsis/files/NSIS%203/ (defaults to `C:\Program Files (x86)\NSIS\`) |

## Build

```cmd
pnpm install
pnpm bundle:windows
```

`bundle:windows` runs `tsc --noEmit && vite build` (via Tauri
`beforeBuildCommand`) and then `tauri build --bundles nsis,msi`.

Output:

- `src-tauri\target\release\inkuo.exe` — standalone exe (stripped)
- `src-tauri\target\release\bundle\nsis\inkuo_0.1.0_x64-setup.exe` — NSIS installer
- `src-tauri\target\release\bundle\msi\inkuo_0.1.0_x64_en-US.msi` — MSI installer

The MSI is the recommended format for managed deployments (Intune, GPO,
silent install). The NSIS installer is friendlier for end-user installs
(per-user install, no UAC prompt, smaller download).

## Release profile

`src-tauri/Cargo.toml` declares `[profile.release]` with `lto = "thin"`,
`codegen-units = 1`, `panic = "abort"`, `strip = "symbols"`. The first
build with these settings will take noticeably longer than incremental
debug builds because the linker has to run end-to-end on the C++ stack
(`fastembed` / `ort`, `merman` / `resvg`, `qdrant-edge`). Subsequent
rebuilds are unaffected.

If you ever need to attach a debugger to a release build, set the env
var `NO_STRIP=1` before `tauri build` (or comment out `strip =
"symbols"` locally) so symbols are preserved.

## Notes

- The cloud base URL is hardcoded in `src\utils\cloudBaseUrl.ts` to
  `http://114.215.182.32:8080`. To point at a different environment, edit
  that file before `pnpm tauri build` and rebuild the frontend.
- ort-sys (ONNX runtime used by fastembed) only ships prebuilt binaries
  for `x86_64-pc-windows-msvc`, which is why the rustup default above
  matters — do **not** try to cross-compile from Linux with the GNU
  toolchain.
- The MSI/NSIS install paths are auto-detected by Tauri from the default
  locations above. If you installed WiX or NSIS somewhere custom, add
  the `bin` directory to `PATH` before running the build.
- Code signing is **not** configured. Windows will show "Unknown
  publisher" and SmartScreen will warn on first launch. To sign, set
  `TAURI_SIGNING_PRIVATE_KEY` / `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` and
  populate the `bundle.windows.signingIdentity` / `certificateThumbprint`
  fields in `tauri.conf.json`.
- The release exe embeds a small **frontend console bridge** (`src-tauri/src/commands.rs::frontend_log`,
  `src-tauri/src/lib.rs::DIAG_INIT_JS`). It mirrors frontend `console.*`
  output and uncaught errors to a log file at
  `%LOCALAPPDATA%\com.inkuo.app\frontend-console.log` and to the Rust
  tracing pipeline. This is intentional — it lets you diagnose blank-screen
  or runtime crashes on end-user machines without forcing them to enable
  DevTools. Disable it by removing the `DIAG_INIT_JS` injection in
  `lib.rs` and the `frontend_log` command if you don't want any frontend
  I/O at runtime.

## Frontend chunking notes

`vite.config.ts` uses auto-chunking (Rollup's default) for the bulk of
`node_modules`, with a single manual split for `@tauri-apps/*` helpers.
Earlier configurations that split each large package (react,
react-dom, prosemirror, codemirror, etc.) into its own chunk produced
silent runtime errors at startup (`Cannot read properties of undefined
(reading 'createContext')` / `reading 'empty'`) because Rollup decided
a helper belonged to chunk A but a sibling chunk still expected it on
chunk B and emitted a dangling import statement. Auto-chunking keeps
those cross-references inside a single file, so the bundle is larger
but always loads cleanly.

If you need finer-grained chunks, see the rollback recipe by reading
the prior `vite.config.ts` history and carefully validate the output
by launching the release exe and checking
`%LOCALAPPDATA%\com.inkuo.app\frontend-console.log` — any `window.onerror`
entry there means a chunk reference is dangling.