# Windows release build (MSI + NSIS)

Run on a **Windows 10/11 host** with the following toolchain:

| Tool | Version | Install |
|---|---|---|
| Node.js | 20 LTS | https://nodejs.org/ |
| pnpm | latest | `npm i -g pnpm` |
| Rust | stable (MSVC) | `rustup default stable-x86_64-pc-windows-msvc` |
| WebView2 Runtime | latest | Pre-installed on Windows 10 19045+ / Windows 11 |
| Visual Studio Build Tools | 2022, "Desktop C++" workload | https://visualstudio.microsoft.com/downloads/ |
| WiX Toolset | 3.14.1 | `https://github.com/wixtoolset/wix3/releases/download/wix3141rtm/wix314.exe` (defaults to `C:\Program Files (x86)\WiX Toolset v3.14\bin`) |
| NSIS | 3.10+ | https://sourceforge.net/projects/nsis/files/NSIS%203/ (defaults to `C:\Program Files (x86)\NSIS\`) |

## Build

```cmd
pnpm install
pnpm tauri build --bundles nsis,msi
```

Output:

- `src-tauri\target\release\inkuo.exe` — standalone exe
- `src-tauri\target\release\bundle\nsis\inkuo_0.1.0_x64-setup.exe` — NSIS installer
- `src-tauri\target\release\bundle\msi\inkuo_0.1.0_x64_en-US.msi` — MSI installer

## Notes

- The cloud base URL is hardcoded in `src\utils\cloudBaseUrl.ts` to
  `http://114.215.182.32:8080`. To point at a different environment, edit that
  file before `pnpm tauri build` and rebuild the frontend.
- ort-sys (ONNX runtime used by fastembed) only ships prebuilt binaries for
  x86_64-pc-windows-msvc, which is why the rustup default above matters —
  do **not** try to cross-compile from Linux with the GNU toolchain.
