# InkUO

AI 文档编辑器桌面端（Tauri 2 + React 19）。

## 系统要求

**最低支持：Windows 10 1809（build 17763）/ Windows 11 / Windows Server 2019+**

原因：依赖 Microsoft [WebView2](https://developer.microsoft.com/en-us/microsoft-edge/webview2/)
运行时，微软官方支持范围不含 Win7 / Win8 / Win8.1。在更老的系统上启动 .exe 会被预检拒绝并提示升级。

macOS / Linux 支持见各自打包脚本（`pnpm bundle:linux`）。

## WebView2 运行时

- 默认通过系统级 WebView2 启动（绝大多数 Win10/11 系统已自带）。
- 缺失时由官方 NSIS / MSI 安装器提示用户下载（由微软分发，不计入本应用包体积）。
