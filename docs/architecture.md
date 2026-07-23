# InkUO 项目架构总览

> **AI 文档编辑器桌面端（Tauri 2 + React 19）** — 自研 OOXML 引擎 + Agent 工具生态 + 本地 RAG + 可选 Cloud Server

- **仓库根**：`/home/maomao/work/inkuo`
- **应用标识**：`com.inkuo.app`
- **品牌隐喻**：i 的笔画被画成一滴墨水（`inkuo` = ink + 品牌后缀）
- **最低系统要求**：Windows 10 1809+ / Windows 11 / Windows Server 2019+（依赖 WebView2；macOS / Linux 各自打包脚本）

---

## 1. 顶层仓库结构

整个仓库本质上是一个 **pnpm monorepo + Cargo workspace + .NET solution** 三栈并行的多模块项目。

```
inkuo/
├── src/                    ← React 19 前端（桌面 UI）
├── src-tauri/              ← Rust 后端（Tauri 2 主进程）
│   ├── src/                Rust 源码
│   ├── prompts/            编译期嵌入的 agent 提示词（markdown）
│   ├── examples/           Office 测试样本
│   └── capabilities/       Tauri 权限声明
├── cloud-server/           ← .NET 8 云服务（ASP.NET Core Minimal API + EF Core + Postgres）
│   ├── src/                4 个 .NET 项目
│   ├── admin-frontend/     React + AntD 管理后台 SPA（独立 pnpm package）
│   ├── tests/              xUnit 测试
│   ├── Dockerfile          多阶段构建（Node SPA + .NET runtime）
│   └── docker-compose.yml  一键起 Api + Billing + Admin + Postgres
├── dist/                   前端构建产物（被 Tauri webview 加载）
├── public/                 静态资源
├── scripts/                一次性迁移脚本（fix_xlsx_styles.py / patch_xlsx_styles.rs / …）
├── docs/                   项目文档（本目录）
├── Examples/、Docs/、$PLUGINSDIR/、Bin/、Stubs/、Plugins/  ← NSIS 安装器源码（Windows 打包用）
├── package.json            前端 + admin-frontend 的 pnpm 工作区根
├── pnpm-workspace.yaml     workspaces: . / cloud-server/admin-frontend
├── pnpm-lock.yaml
├── vite.config.ts          前端构建配置
├── tsconfig.json / tsconfig.node.json
├── eslint.config.js
├── index.html              Vite 入口
└── shuyun.pem              部署用证书（不在版本控制范围内提及）
```

---

## 2. 八个核心业务模块

| # | 模块 | 路径 | 职责一句话 |
|---|------|------|-----------|
| 1 | **前端 UI（React 19）** | `src/` | 编辑器、侧栏、AI 面板、设置、Cloud Tab、欢迎页 |
| 2 | **Tauri 主进程 / Rust 后端** | `src-tauri/src/` | 文档解析、AI 代理、文件系统、IPC 命令、设备层 |
| 3 | **AI 代理引擎（Agent Loop）** | `src-tauri/src/agent/` | 工具调用循环、子代理调度、提示词加载 |
| 4 | **Office 文档引擎** | `src-tauri/src/office/` | .docx / .xlsx 自研解析与写入（不依赖 mammoth/openpyxl） |
| 5 | **本地知识库（RAG）** | `src-tauri/src/knowledge/` | 分块、向量化、Qdrant Edge 嵌入式检索 |
| 6 | **Cloud 客户端** | `src-tauri/src/cloud/` + `src/components/cloud/` | JWT、账号管理、inkuo Cloud 调用 |
| 7 | **Cloud Server（.NET）** | `cloud-server/src/` | API / Billing / Admin + EF Core + Postgres |
| 8 | **Admin Web SPA** | `cloud-server/admin-frontend/` | React + AntD 操作后台 |

下面逐一详细展开。

---

## 模块 1 — 前端 UI（`src/`）

15 个 UI 子模块 + 7 个支持层。

### 1.1 `components/` 组件目录

| 子目录 | 关键文件 | 职责 |
|--------|---------|------|
| `activitybar/` | `ActivityBar.tsx` | 左侧视图切换：files / search / git / extensions / **knowledge** / **snapshots** |
| `aipanel/` | `AIPanel.tsx` + 30+ 文件 | AI 聊天面板全栈（详见 §1.2） |
| `cmdk/` | `CmdK.tsx` | ⌘K 浮层 AI 行内编辑器（选中即编辑、4 种 scope + 4 个预设模板） |
| `cloud/` | `CloudPage.tsx` + `CloudPanel.tsx` + `cloudApi.ts` | Cloud 账号登录/注册/账户信息面板 |
| `common/` | `Skeleton.tsx` + `Tooltip.tsx` + `EmptyState.tsx` | 设计系统原子组件 |
| `editor/` | `Editor.tsx` + 20+ 文件 | 主编辑器（CodeMirror）+ 各格式 viewer（详见 §1.3） |
| `inline-complete/` | `InlineCompleteProvider.tsx` + `GhostTextOverlay.tsx` | Tab 触发 Ghost Text 补全 |
| `layout/` | `Layout.tsx` | 主壳：titlebar + activitybar + sidebar + editor + aipanel |
| `resizable/` | `ResizableHandle.tsx` | 侧栏/AI 面板的拖拽手柄 |
| `settings/` | `SettingsPanel.tsx` + 5 个子面板 | 设置：Models / Knowledge / WebSearch / Snapshots / Appearance |
| `sidebar/` | `Sidebar.tsx` + 7 个文件 | 文件树 + 上下文菜单 + 重命名 + 知识库 + 通知 + 确认弹窗 |
| `snapshots/` | `SnapshotPanel.tsx` + `SnapshotRestoreDialog.tsx` | 工作区快照列表与恢复对话框 |
| `titlebar/` | `TitleBar.tsx` | 自定义无装饰标题栏 + 文件菜单（开关工作区/新窗口/云账号/设置/退出） |
| `welcome/` | `WelcomePage.tsx` + `Wordmark.tsx` | 启动页 + 品牌字标 |
| —— | `WorkspaceBootstrap.tsx` | 全局挂载点：自动保存工作区快照（1.5s debounce） |

### 1.2 `aipanel/` 内部组件

`AIPanel` 是一个完整的"AI 聊天 IDE 子壳"，可拆为 5 层：

- **壳层**：`AIPanel.tsx`（容器）/ `AIPanelLayout.module.css`
- **Header / Composer**：`ChatHeader.tsx`（模式切换 ask↔plan↔agent）/ `ChatInput.tsx`（563 行，含 feature toggles）/ `ChatEmptyState.tsx`
- **会话管理**：`HistorySidebar.tsx` + `useChatSessionActions` + `todoSync.ts`
- **消息渲染**：`ChatView.tsx` + `MessageItem.tsx` + `UserMessageBubble.tsx` + `AssistantMessageBody.tsx` + `InlineDiffPreview.tsx`
- **工具/计划/Markdown**：`ToolCallCard.tsx` + `PlanCard.tsx` + `AskUserCard.tsx` + `KnowledgeBuildToolCard.tsx` + `KnowledgeToolbar.tsx` + `TodoPanel.tsx` + `DelegateToCard.tsx` + `CompactToolCard.tsx` + `ReasoningBlock.tsx` + `MarkdownRenderer.tsx` + `StreamingMarkdownRenderer.tsx` + `LazyTextContent.tsx` + `CollapsedHistoryPlaceholder.tsx`
- **流式控制 hooks**：`useAgentStream.ts`（串起 text/reasoning/tool 三条流）+ `useAIPanelController.ts` + `useChatComposer.ts` + `useChatInputState.ts` + `useTextStreaming.ts` + `useReasoningStreaming.ts` + `useToolCallStreaming.ts` + `useKnowledgeBase.ts` + `messageStreamActions.ts` + `toolCallStreamActions.ts` + `reasoningStreamActions.ts` + `textStreamActions.ts` + `streamEventDispatcher.ts` + `streamEventHandlers.ts` + `messageTransform.ts` + `streamTypes.ts` + `toolUtils.ts` + `knowledgeReference.ts` + `knowledgeToolbarModel.tsx` + `index.ts`

### 1.3 `editor/` 内部组件

- **CodeMirror 主体**：`Editor.tsx`（CodeMirror 6 + React 19）+ `EditorBody.tsx` + `editorExtensions.ts`（70+ 语言动态加载）+ `TabBar.tsx`
- **各格式 viewer（懒加载以减小 main chunk）**：`MarkdownPreview.tsx` / `PdfViewer.tsx`（pdfjs-dist 4.10.38）/ `ImageViewer.tsx` / `SvgViewer.tsx` / `OfficeViewer.tsx`（DocxEditor + FortuneSheet 桥接）
- **Diff 系统**：`DiffOverlay.tsx` + `DiffActionBar.tsx` + `diffDecorationsField.ts` + `inlineDiffDecorations.ts`
- **SVG 编辑子模块 `svgEditor/`**：`parseSvg.ts` / `serializeSvg.ts` / `useSelection.ts` / `types.ts`
- **Word 工具栏子模块 `word-toolbar/`**：`WordToolbar.tsx`（1929 行）+ `helpers.ts` + `primitives.tsx` + `constants.ts`（ProseMirror marks 套壳）
- **Hooks**：`useDocumentLoader.ts` / `useDocumentSave.ts` / `useEditorInlineCompletion.ts` / `useEditorInteraction.ts` / `useExternalFileSync.ts` / `useKeyboardSave.ts`
- **其他**：`fortuneSheetConverter.ts`（Rust XlsxWorkbook ↔ FortuneSheet 双向转换）+ `LazyMediaViewers.tsx`

### 1.4 `store/` Zustand 状态管理

采用**分片（slice）+ 持久化（persist middleware）**模式：

| Store | 文件 | 职责 |
|-------|------|------|
| `useAIPanelStore` | `aiPanelStore.ts` + `aiPanelStore/slices/{ui,session,message,toolCall,diff,subagent}Slice.ts` | AI 会话、消息、工具调用、Diff、子代理进度 |
| `useSidebarStore` | `sidebarStore.ts`（479 行） | 工作区路径、文件树、Tab 列表、选中文件、KnowledgeBase 元数据 |
| `useEditorStore` | `editorStore.slices.ts` + `editorDiffState.ts` + `editorStore.ts` | 文档内容缓存、Diff 状态、Baseline |
| `useSettingsStore` | `settingsStore.ts`（27 KB） | AI Provider / Cloud Account / Web Search / KB 模型 / 主题 / 动效 / 快照设置 |
| `useLayoutStore` | `layoutStore.ts` | 侧栏宽度、视图、aipanel 宽度 |
| `useCmdKStore` | `cmdKStore.ts` | ⌘K 浮层 scope + instruction + 处理中状态 |
| `useInlineCompleteStore` | `inlineCompleteStore.ts` | Ghost Text 启用 / 防抖 |
| `useNotificationStore` | `notificationStore.ts` | Toast 队列 |
| `useBaselineStore` | `baselineStore.ts` | 编辑前基准（用于 diff） |
| `useClipboardStore` | `clipboardStore.ts` | 内部剪贴板 |
| `useContextMenuStore` | `contextMenuStore.ts` | 文件树右键菜单目标 |
| `useConfirmDialogStore` | `confirmDialogStore.ts` | 全局确认弹窗 |
| `index.ts` | —— | 统一 re-export（13 个 store + 大量类型） |

另有 `aiPanelReducers.ts`（纯函数 reducer，被多个 hook 复用）和 `aiPanelStore.types.ts`。

### 1.5 其他支持层

- **`hooks/`**（11 个业务 hooks）：
  - `useWorkspaceTree.ts` / `useWorkspaceSearch.ts` / `useWorkspaceFileWatcher.ts` / `useWorkspaceSnapshotAutosave.ts` / `useInitialSnapshotLoader.ts`（启动恢复）
  - `useEditorInlineCompletion.ts` / `useEditorInteraction.ts`
  - `useTheme.ts` / `useMotionLevel.ts`
  - `useDebouncedCallback.ts` / `useGlobalKeydown.ts` / `useGlobalPointerDown.ts` / `useTauriEvent.ts`

- **`services/`**（IPC 包装层）：
  - `workspace.ts`（switchWorkspace + 持久化 + LRU 快照同步）
  - `snapshots.ts`（创建/列表/删除/预览/恢复，base64 包装二进制）
  - `documentSave.ts`（编辑保存流程）
  - `planFiles.ts`（PlanCard 落盘到 `.inkuo/plans/`）

- **`types/`**：`index.ts`（974 行，全局 TS 类型）+ `inline-complete.ts` + `generated/`（由 ts-rs 从 Rust 端生成的绑定）

- **`utils/`**：`path.ts`（跨平台路径工具）/ `cloudBaseUrl.ts` / `errors.ts`（reportError）/ `platform.ts` / `settings.ts` / `saveSettings.ts` / `planStream.ts` / `json.ts` / `color.ts` / `openSettingsTab.ts` / `openCloudTab.ts` / `tauri.ts`（环境判定）

- **`constants/`**：`timing.ts`（动画 / debounce 全局阈值）

- **`styles/`**：`design-tokens.css`（OKLCH 主题 token — graphite/verdant/iris 三套） + `motion.css` + `global.css`

### 1.6 顶层入口

- `main.tsx`（Vite 入口，渲染 `<App/>`）
- `App.tsx`（顶层壳：判 workspacePath 是否设置 → 决定渲染 WelcomePage 还是 Layout；调用 `useTheme / useMotionLevel / useInitialSnapshotLoader`；处理"新窗口"flag）
- `vite-env.d.ts`

---

## 模块 2 — Tauri Rust 后端（`src-tauri/src/`）

### 2.1 入口与生命周期

- **`main.rs`** — 极简启动器（仅 `pub fn main() { inkuo_lib::run() }`）
- **`lib.rs`**（492 行）— 应用入口：
  - `MIN_WINDOWS_BUILD = 10_240`（WebView2 底线）
  - `setup_logging()`（tracing stdout + 文件，path = `%LOCALAPPDATA%\com.inkuo.app\inkuo.log`）
  - `windows_build_number()`（ntdll!RtlGetVersion — 绕过 GetVersionExW 的兼容性 shim 谎言）
  - `preflight_os_check()`（OS build 检查）
  - `app_data_dir()`（`dirs::data_local_dir()` → `com.inkuo.app`）
  - `hydrate_cloud_client_from_settings()`（启动同步 hydrate CloudClient）
  - `run()`：preflight → CloudClient 构造 → tauri::Builder → manage(AppState/CloudClient/FileWatcherState) → setup（log hook、backup cleanup、workspace snapshots init、KB shared stores、CloudClient hydrate）→ 注册 6 个插件 → 注册 50+ IPC 命令
  - `build.rs`：embed-resource（Windows manifest 防 OS shim）

### 2.2 核心模块

| 模块 | 行数级 | 职责 |
|------|--------|------|
| `ai.rs` | 730 | AI Provider 适配：OpenAI 协议 / DeepSeek / Ollama / Official，编译期 `include_str!` 加载 ask/plan/edit prompt |
| `ai_config.rs` | 398 | 多 Provider 配置 + Cloud routing 分流（5 种 `AIProviderKind`） |
| `streaming.rs` | 320 | `StreamPayload` 主结构 + `FileDiffSummary` / `StreamDiffHunk` / `OfficeFileModified` / `PlanResultData` / `AskUserStreamPayload` |
| `openai_stream.rs` | 21 | SSE 事件 `data:` 行解析 |
| `document.rs` | 297 | Markdown / PlainText 文档模型 + 块级树 |
| `diff.rs` | 173 | similar crate 的 Diff 引擎（hunks + summary + offsets） |
| `file_watcher.rs` | 177 | notify PollWatcher + `emit_file_change` 跨 inotify 限制 |
| `backup.rs` | 105 | `~/.inkuo/backups/` 备份管理 + 后台清理任务 |
| `fs_utils.rs` | 120 | `walk_dir_safe` 防 symlink 循环 + 深度上限 |
| `security.rs` | 115 | `validate_workspace_path`（防 CVE 路径穿越） |
| `error.rs` | 125 | 统一 `AppError` + `ts_rs` 导出 |
| `runtime_state.rs` | 126 | 每 turn 注入 system prompt 的运行时片段（mode → tool tier） |
| `feature_toggles.rs` | 183 | `kb_strict` / `web_search` 开关 + tool 过滤 |
| `settings_state.rs` | 379 | Settings schema + 磁盘缓存（`SETTINGS_CACHE: Lazy<Mutex<Option>>`） |
| `frontend_diag.rs` | 110 | WebView2 console → `frontend-console.log` 文件桥 |
| `app_handle.rs` | 19 | 进程级 AppHandle 单例（`OnceLock<AppHandle>`） |
| `commands/mod.rs` | ~1000 | IPC 命令聚合：文档 / AI / 设置 / Office / 快照 |

### 2.3 命令层（IPC）

- **`commands/mod.rs`** — Tauri 命令总入口：
  - `read_document / write_document / list_directory / search_directory / compute_diff`
  - `read_office_file / write_office_file / read_office_text / write_office_text`
  - `read_xlsx_structured / write_xlsx_structured`
  - `ai_edit / get_settings / save_settings / test_api_config`
  - `watch_directory / unwatch_directory`
  - `save_workspace_snapshot / load_workspace_snapshot / create_workspace_snapshot_cmd / list_workspace_snapshots_cmd / delete_workspace_snapshot_cmd / preview_workspace_snapshot_restore_cmd / restore_workspace_snapshot_cmd / collect_workspace_empty_dirs_cmd / collect_workspace_files_cmd`
  - `read_file_bytes_cmd / read_file_for_viewer / read_snapshot_file_cmd`
  - `logging::frontend_log`
  - 通过 `pub use context_menu::*` 暴露 9 个文件树命令
  - 通过 `pub use crate::settings_state::*` 暴露 Settings 相关辅助
  - 通过 `pub use crate::runtime::cancel::*` 暴露 cancel 注册表

- **`commands/context_menu.rs`** — 文件树右键：create_file_entry / rename_path / delete_path / copy_path / move_path / path_exists / open_with_default_app / reveal_in_file_manager / create_new_window

- **`commands/snapshot_state.rs`** — 工作区快照的内存 + 磁盘状态（`WORKSPACE_SNAPSHOTS: Lazy<Mutex<HashMap>>` + LRU）

- **`commands/logging.rs`** — `frontend_log` IPC 落地到文件

- **`commands_stream.rs`** — `ai_edit_stream / ai_stream_cancel`（行内编辑流）

- **`commands_agent.rs`** — `ai_agent_stream / ai_agent_cancel / get_available_tools / answer_ask_user`（agent 流）

- **`commands_plan.rs`** — `plan_save / plan_read / plan_delete`（落盘到 `<workspace>/.inkuo/plans/<id>.md`）

- **`commands_cloud.rs`** — `cloud_register / login / logout / fetch_models / fetch_account / persist_account`

### 2.4 子模块目录

```
runtime/                   进程级单例与注册表
  └── cancel.rs            AI 流式 cancel 注册表 + StreamCancelGuard

cloud/mod.rs               CloudClient（reqwest + JWT refresh + ensure_fresh_token）

inline_complete/mod.rs     Tab 触发的 Ghost Text 补全

knowledge/                 本地 RAG
  ├── mod.rs               re-export
  ├── commands.rs          knowledge_build / search / status / update / clear / add_members /
  │                        remove_members / get_members + check_available_models / download_model_files
  ├── embedding_models.rs  模型发现与下载
  ├── chunker.rs           按句切块（中英文混合）
  ├── scanner.rs           工作区扫描
  ├── embedder.rs          fastembed（ONNX 本地模型）
  ├── vector_store.rs      Qdrant Edge 嵌入式存储（单进程 WAL）
  ├── metadata.rs          增量索引元数据
  └── config.rs            KnowledgeConfig / Chunk / Document / SearchResult

snapshots/mod.rs           工作区文件级快照（不依赖 Git，全量复制 + LRU + 后台清理）

office/                    自研 OOXML 引擎（详见模块 4）

agent/                     AI 工具调用代理核心（详见模块 3）
```

### 2.5 关键依赖（`Cargo.toml`）

- Tauri 2 + 6 个插件（opener/fs/dialog/shell/os）
- 异步运行时：tokio（full）、reqwest（json/stream/blocking）
- 序列化：serde + serde_json + ts-rs（TypeScript 绑定生成）
- Diff：similar 2
- 解析：pulldown-cmark / quick-xml / zip
- Office：calamine 0.26、rust_xlsxwriter 0.95
- 本地 AI：fastembed 5（ONNX）、qdrant-edge 0.7（向量）
- 渲染：merman 0.7（headless Mermaid，纯 Rust + resvg PNG）
- 文件监控：notify 6 + notify-debouncer-mini 0.4
- Release profile：`lto="thin" codegen-units=1 panic="abort" strip="symbols"`

### 2.6 提示词目录（`src-tauri/prompts/`）

与代码同级，编译期 `include_str!` 嵌入：

```
prompts/
├── ask.md / plan.md / edit.md            模式 base prompt
├── main/agent.slim.md                    Main Agent 精简 prompt
├── subagents/                            9 个子代理
│   ├── batch_editor.md
│   ├── code_expert.md
│   ├── flowchart_expert.md
│   ├── md_writer.md
│   ├── office_excel_expert.md
│   ├── office_pptx_expert.md
│   ├── office_word_expert.md
│   ├── researcher.md
│   └── word_image_expert.md
├── tool_specs/                           按需 `get_tool_help` 加载
│   ├── add_pptx_animation.md / excel.md / general.md / markdown.md /
│   │   media.md / pptx.md / pptx_animation.md / svg.md / word.md
└── fragments/                            系统提示片段
```

---

## 模块 3 — AI 代理引擎（`src-tauri/src/agent/`）

`agent_loop.rs`（1561 行，最大文件）是核心，**每轮循环**：

```
request → AI → [tool_calls?] → execute → AI → [tool_calls?] → ... → final
```

实现细节：

1. `ai::AIProvider` 调上游 → 收 SSE delta（`openai_stream.rs` 解析）
2. `streaming::StreamPayload` 推到前端 `aipanel/useAgentStream`
3. 解析 `tool_calls`，从 `ToolRegistry` 查工具（22+ 个）
4. `validate_workspace_path` 防穿越（`security.rs` 单点）
5. 结果追加进 message 历史，循环直到最终回复或 `max_iterations=50`（默认）

### 3.1 代理模块

| 文件 | 职责 |
|------|------|
| `mod.rs` | re-export：tools / agent_loop / prompts / profile |
| `agent_loop.rs`（1561） | Agent 主循环 + AgentExecutor + Message + ToolCallMessage + AskUserStreamPayload 桥 |
| `agent_helpers.rs` | DeltaResponse / DeltaToolCall / DeltaFunction 解析器 + `save_plan_to_workspace` + `generate_plan_id_for_session` |
| `profile.rs` | `AgentProfile`（system_prompt + allowed_tools + max_iterations） |
| `prompts.rs` | 编译期 `include_str!` 加载 prompt markdown + `PROFILES` 常量数组 + `resolve_profile / find_profile / find_tool_spec` |

### 3.2 9 个 Agent 子代理（profile）

通过 `delegate_to` 元工具分派：

| Name | Label | 主要工具 |
|------|-------|---------|
| `main` | Main Agent | read/write/edit/list/grep/glob/database_search/create_svg/get_tool_help/delegate_to/update_todo |
| `office_word_expert` | Word Document Expert | read_office_file / create_word_doc / inspect_office / compare_word_docs |
| `office_excel_expert` | Excel Document Expert | read_office_file / modify_excel / create_excel / inspect_office |
| `office_pptx_expert` | PPTX Document Expert | read_office_file / create_pptx / create_pptx_animation / add_pptx_animation / inspect_office |
| `word_image_expert` | Word + Image Expert | office_word_expert + read_image |
| `md_writer` | Markdown Writer | read_file / write_file / edit_file / create_svg |
| `code_expert` | Code Expert | read/write/edit/list/grep/glob/database_search |
| `researcher` | Researcher | database_search + read_file + write_file + web_search |
| `flowchart_expert` | Flowchart Expert | render_mermaid + create_svg + write_file |
| `batch_editor` | Batch Editor | read/write/edit + glob + grep |

设计意图：**主代理刻意不带 Office 工具**，必须 delegate 到专家以保持主代理 tool schema 精简。

### 3.3 22+ 个工具（按大类分文件）

`tools/mod.rs` 定义 `ToolDefinition / ToolRegistry / ToolError / SecurityError / validate_workspace_path`：

| 文件 | 工具 | 类别 |
|------|------|------|
| `file_tools.rs` | `read_file / write_file / edit_file` | 文件 |
| `search_tools.rs` | `list_dir / glob / grep` | 搜索 |
| `database_tools.rs` | `database_search` | RAG |
| `web_search_tool.rs` | `web_search` | 网络（百度百科） |
| `media_tools.rs` | `read_image / read_pdf` | 二进制消费 |
| `mermaid_tools.rs` | `render_mermaid` | 图表（merman crate） |
| `svg_tools.rs` | `create_svg` | 图表 |
| `office/mod.rs` + `office/{create_word_doc,inspect_office}.rs` | `read_office_file / create_word_doc / inspect_office / compare_word_docs / modify_excel / create_excel` | Office |
| `pptx/mod.rs` + `pptx/{svg_parser,_split/}.rs` | `create_pptx` | PPT（SVG → PPTX） |
| `pptx_anim/mod.rs` + `pptx_anim/animation_xml.rs` | `create_pptx_animation / add_pptx_animation` | PPT 动画 |
| `ask_user_tools.rs` | `ask_user`（oneshot 通道暂停 agent） | 人机协作 |
| `todo_tools.rs` | `update_todo`（set/advance/complete_current） | 元 |
| `plan_tools.rs` | `create_plan`（落盘到 `.inkuo/plans/`） | 元 |
| `meta_tools.rs` | `get_tool_help / delegate_to` | 元 |
| `asset_registry.rs` | 二进制资产 side-channel（避免 base64 进 context） | 辅助 |

---

## 模块 4 — Office 文档引擎（`src-tauri/src/office/`）

**完全自研**，未依赖 mammoth / openpyxl / docx-rs 等第三方读写库。

### 4.1 `office/mod.rs`（顶层）

- `pub mod shared; mod docx; mod xlsx;`
- `OfficeFileType` enum（`Word(docx::WordDocument) | Excel(xlsx::ExcelWorkbook)`）
- `pub use` 暴露全部 docx / xlsx 公开类型
- `read_office_file(path)` 入口（按扩展名 dispatch）

### 4.2 `office/shared.rs`（共享类型）

- `OfficeError`（Io/Zip/Xml/Excel/Json/UnsupportedFileType）
- `TableCell / TableRow`（docx / xlsx 共用）

### 4.3 `office/docx/`（9 个文件）

| 文件 | 行数 | 职责 |
|------|------|------|
| `mod.rs` | 1524 | 公开类型（`WordDocument / WordParagraph / WordTable / WordImage / FontRun / FieldRef / DocElement / InsertElement / NumberingRef / WordSection / PageSize / PageSizeMm / PageMargins / HeaderPart / FooterPart / …`）+ 编排 `write_word_document` writer + reader 入口 |
| `types.rs` | 22 | re-export 表面（保持 `crate::office::docx::types::WordDocument` 路径） |
| `xml_parser.rs` | ~1080 | `parse_document_xml` + RunFormat 解析 + `attr_value_str` 流式 |
| `table_parser.rs` | ~280 | 流式 `<w:tbl>` 解析 + RawCell/RawTable/VMergeKind 中间态 + vMerge 解析 |
| `reader.rs` | ~100 | 纯文本 / markdown 渲染（`word_document_to_text`） |
| `writer.rs` | ~680 | OOXML 文档树构造（`build_run_xml / build_run_rpr_xml / build_field_run_xml / field_instr_text / build_document_xml / build_*_sectpr_xml / build_image_drawing_xml / escape_xml / stable_id_to_docpr_id / emit_text_direction`） |
| `zip_writer.rs` | ~620 | `ImageWritePlan / HeaderFooterWritePlan / PreservedImageRef` + `scan_preserved_*` + `build_header_footer_xml` + `append_* / substitute_*` |
| `zip_reader.rs` | ~210 | `read_word_document` + `parse_header_footer_parts` + `resolve_section_refs` |
| `document_helpers.rs` | ~260 | 节/页边距/页大小转换 + `header_footer_ref_to_xml` + `word_doc_props_to_core_xml` |
| `ooxml_boilerplate.rs` | ~360 | 默认 styles / content-types / settings / font-table / theme / app/core properties 静态 XML |

### 4.4 `office/xlsx/`（7 个文件，双层 API）

| 文件 | 行数 | 职责 |
|------|------|------|
| `mod.rs` | ~1300 | 公开类型 + `read_excel_workbook / write_excel_workbook / read_xlsx_structured` + `XlsxWorkbook::apply_operations` |
| `types.rs` | 13 | re-export 表面 |
| `legacy_text.rs` | ~50 | `cell_to_string / excel_workbook_to_text`（legacy flat 2D API） |
| `structured_text.rs` | ~68 | `xlsx_workbook_to_text`（structured API） |
| `styles_parser.rs` | ~494 | CellXf / AlignmentXf / FontXf / FillXf + `parse_styles` state machine |
| `ooxml_boilerplate.rs` | —— | MINIMAL_STYLES_XML / MINIMAL_THEME_XML 静态常量 |
| `writer.rs` | ~860 | `create_xlsx_workbook / write_excel_document / build_workbook_styles / build_sheet_xml / build_cell_xml / parse_sheet_name_to_path_map / escape_xml_attr` |
| `incremental_writer.rs` | ~710 | `CellModification / ExcelOperation / incremental_write_xlsx` + byte-level XML splicing 助手（`find_c_element_end / find_matching_close_c / build_replacement_cell_xml / value_to_xml_body`） |

两层 API：
- **Legacy**（`ExcelWorkbook` flat 2D 网格）— 向后兼容
- **Structured**（`XlsxWorkbook / XlsxSheet / Cell / CellStyle` 含公式/合并/样式）— AI 编辑与保守 round-trip

---

## 模块 5 — 本地知识库 RAG（`src-tauri/src/knowledge/`）

完全自包含，**单进程持久化**：

```
知识库流程：
scan (scanner.rs) → chunk (chunker.rs, 中英文混合按句)
  → embed (embedder.rs, fastembed ONNX 本地模型)
  → store (vector_store.rs, Qdrant Edge 嵌入式)
  → search (database_search 工具 → vector_store)
```

### 5.1 模块拆分

| 文件 | 职责 |
|------|------|
| `mod.rs` | re-export + 注释层 |
| `commands.rs`（894 行） | Tauri 命令：knowledge_build / search / status / update / clear / add_members / remove_members / get_members + check_available_models / download_model_files + `SHARED_STORES: OnceLock<RwLock<HashMap<String, VectorStore>>>`（KB UI 与 agent 共用同一实例，避 WAL 锁冲突） |
| `embedding_models.rs`（217） | 模型发现 + 下载 + 元信息 |
| `chunker.rs`（183） | 按句切块（中英文混合 Regex：`[。！？\.!\?]+`） + `ChunkConfig` |
| `scanner.rs`（136） | 工作区递归扫描 + 文件类型白名单（md/rs/js/ts/py/go/...） + 排除目录（node_modules / .git / target / dist） |
| `embedder.rs`（353） | fastembed（ONNX）+ `ModelSource`（native / user-defined） |
| `vector_store.rs`（348） | Qdrant Edge 嵌入式 + `PointInsertOperations / PointOperations` |
| `metadata.rs`（350） | 增量索引元数据（IndexedFile + KnowledgeBase metadata） |
| `config.rs`（49） | KnowledgeConfig（默认 BAAI/bge-large-zh-v1.5, 1024 维） + Chunk / Document / SearchResult |

### 5.2 设计特点

- 每个 workspace 一个 collection（collection_name 由 workspace hash 派生）
- 增量更新：基于文件 SHA-256
- `SHARED_STORES` 静态 `OnceLock<RwLock<HashMap>>` 保证 UI 构建路径与 agent `database_search` 工具共用同一 `VectorStore` 实例，**避免 Qdrant Edge WAL 单进程锁冲突**

---

## 模块 6 — Cloud 客户端

### 6.1 Rust 端 `src-tauri/src/cloud/mod.rs`（~650 行）

- `CloudError` enum（NotLoggedIn / Network / Server / AuthFailed / QuotaExhausted / InvalidInviteCode / Parse / Other）
- `CloudAccount` struct（base_url / email / user_id / access_token / refresh_token / access_expires_at / plan_name / balance_cents）
- `CloudModelEntry`（model_config_id + display_name + upstream_model + price_per_1k_*）
- `CloudClient`（`Arc<Mutex<CloudAccount>>`）：
  - `register / login / fetch_models / fetch_account / chat_stream`
  - `ensure_fresh_token`（处理 401 自动 refresh，5xx/429 重试一次）
- 设计原则：纯客户端，不读 Settings（由 commands 层注入）；状态由前端 `settingsStore` 持久化到 `settings.json`

### 6.2 前端 `src/components/cloud/`

- `cloudApi.ts`（类型化 invoke 包装）
- `CloudPanel.tsx` + `CloudPage.tsx` + `CloudAccountCard.tsx` + `CloudAuthPanel.tsx`
- 启动时 Rust 端 `hydrate_cloud_client_from_settings` 同步恢复会话

---

## 模块 7 — Cloud Server（`cloud-server/src/`）

`.NET 8 ASP.NET Core Minimal API` + EF Core + PostgreSQL，多端口服务拆分：

### 7.1 `cloud-server/Inkuso.Cloud.slnx`

```
Inkuso.Cloud.Core/         共享库
Inkuso.Cloud.Api/          端口 8080：客户端 API
Inkuso.Cloud.Billing/      端口 8081：对账 Worker + 遗留 /admin/*
Inkuso.Cloud.Admin/        端口 8082：管理后台 API + 静态 SPA 宿主
```

### 7.2 `Inkuso.Cloud.Core/` 共享库

```
Auth/JwtService.cs             JWT 服务（access + refresh token，HS256）
Data/AppDbContext.cs           EF Core + Postgres + auto-migrate
Data/AppDbContextFactory.cs    设计时工厂
Entities/                      10 个实体：
  ├── AdminUser.cs
  ├── InviteCode.cs
  ├── ModelConfig.cs           （含上游 API Key，受 SecretProtector 保护）
  ├── Plan.cs
  ├── RedemptionCode.cs
  ├── RefreshToken.cs
  ├── Subscription.cs
  ├── UsageRecord.cs
  ├── User.cs
  ├── WebSearchProvider.cs
  └── WebSearchUsageRecord.cs
Security/SecretProtector.cs    ASP.NET DataProtection API 加密上游 API Key（dp:<base64> 格式）
Upstream/
  ├── LlmForwarder.cs          SSE 反向代理 + 用量计费
  └── WebSearchForwarder.cs    搜索上游转发
Migrations/                    EF Core 迁移
```

### 7.3 `Inkuso.Cloud.Api/`（端口 8080）

```
Program.cs                  JWT 中间件 + DbContext + LlmForwarder DI + 启动时拒绝弱 secret
Endpoints/
  ├── Auth.cs              POST /auth/{register,login,refresh}
  ├── Models.cs            GET /v1/models
  ├── Chat.cs              POST /v1/chat/completions（OpenAI 兼容，SSE 流）
  ├── Account.cs           GET /account/{me,usage}
  ├── Redeem.cs            POST /redeem
  └── WebSearch.cs         POST /v1/web/search
```

JWT 校验：
- 拒绝 < 32 字符的 secret
- 拒绝以 `change-me` 开头的 secret
- HS256，access 15 分钟，refresh 30 天

### 7.4 `Inkuso.Cloud.Billing/`（端口 8081）

```
Program.cs                  ReconciliationWorker 注册 + 启动 migrate
Services/ReconciliationWorker.cs  后台对账 worker
MapAdminEndpoints()         POST /admin/redemption-codes / invite-codes / GET /admin/stats
```

### 7.5 `Inkuso.Cloud.Admin/`（端口 8082）

```
Program.cs                  Admin JWT（独立 audience "inkuo-admin"）+ SPA 静态托管
Auth/AdminJwtService.cs     独立 audience 的 admin JWT
Middleware/                 认证 / 审计中间件
Endpoints/                  9 个后台 endpoint：
  ├── AdminAuth.cs           /api/auth/{login,me,change-password,create}
  ├── Dashboard.cs           /api/dashboard/{summary,usage-trend,plan-distribution,model-usage}
  ├── Users.cs               /api/users/...（CRUD + 调整余额 + 撤销会话 + 删除）
  ├── Plans.cs               /api/plans/...（CRUD）
  ├── ModelConfigs.cs        /api/model-configs/...（CRUD，?includeKey=true 显示真实 key）
  ├── InviteCodes.cs         /api/invite-codes/...（CRUD + 启用/禁用）
  ├── RedemptionCodes.cs     /api/redemption-codes/...（CRUD + 启用/禁用 + 绑 plan）
  ├── Usage.cs               /api/usage/（按 user/model/date 过滤）
  └── WebSearchProviders.cs  /api/web-search-providers/...
wwwroot/                    admin-frontend 构建产物（部署时拷贝）
```

### 7.6 测试 `cloud-server/tests/Inkuso.Cloud.Core.Tests/`

- `DataProtectionSecretProtectorTests.cs`
- `JwtServiceTests.cs`
- `LlmForwarderCostTests.cs`

### 7.7 部署

- `Dockerfile` 多阶段构建（Node 编译 SPA → .NET runtime）
- `docker-compose.yml`：api + billing + admin + postgres 一键起
- `DEPLOYMENT.md`（11 KB）：生产部署清单
- `scripts/smoke-test.sh`：3 个服务的端到端冒烟测试

---

## 模块 8 — Admin Web SPA（`cloud-server/admin-frontend/`）

```
admin-frontend/
├── package.json / vite.config.ts / tsconfig.json / index.html / .npmrc
├── src/
│   ├── main.tsx / App.tsx / index.css
│   ├── api/             Axios 客户端（10 个域）：
│   │   ├── auth.ts / client.ts / dashboard.ts / inviteCodes.ts /
│   │   ├── modelConfigs.ts / plans.ts / redemptionCodes.ts /
│   │   └── usage.ts / users.ts / webSearchProviders.ts
│   ├── layouts/AdminLayout.tsx    （侧栏 + 顶栏 + Outlet）
│   └── pages/           10 个页面：
│       ├── Login.tsx / Dashboard.tsx（ECharts 30 天趋势 + 计划分布饼图 + 模型 TOP-N 横向条形图）
│       ├── Users.tsx（分页列表 + 详情抽屉 + 调整余额 + 撤销会话 + 删除）
│       ├── Plans.tsx / ModelConfigs.tsx / InviteCodes.tsx /
│       └── RedemptionCodes.tsx / Usage.tsx / Admins.tsx / WebSearchProviders.tsx
└── public/
```

特点：
- API Key 默认脱敏（`<first4>***<last4>`），点击"查看完整 API Key"才显示
- 所有表单有验证
- ECharts 可视化

---

## 横切支撑（贯穿所有模块）

| 横切关注点 | 落地位置 |
|-----------|---------|
| 主题（graphite / verdant / iris） | `src/styles/design-tokens.css` + `useTheme` + `AppearanceSettings` |
| 动效分级 | `src/hooks/useMotionLevel.ts` + `motion.css` |
| 通知 Toast | `notificationStore` + `NotificationStack` |
| 错误上报 | `src/utils/errors.ts` 的 `reportError` |
| 流式取消 | `runtime/cancel.rs` + 各命令的 `StreamCancelGuard` |
| 路径沙箱 | `security.rs` 单点校验 |
| 快照/备份 | `snapshots/`（应用级）+ `backup.rs`（pre-restore）+ `frontend_diag.rs`（崩溃现场） |
| 自动保存工作区状态 | `WorkspaceBootstrap` + `useWorkspaceSnapshotAutosave`（1.5s debounce） |
| 文件变更事件 | `file_watcher.rs` `emit_file_change` → 前端 `useWorkspaceFileWatcher`（事件驱动，无 500ms 轮询） |
| i18n / 文案 | 模式标签 / 工具显示名中文（'ask' / 'plan' / 'agent'）；prompt 英文（LLM 训练更优） |
| 启动 hydrate | `hydrate_cloud_client_from_settings`（同步，避免 race） |
| 资源体积优化 | 前端懒加载（SettingsPanel / CloudPage / OfficeViewer / OfficeViewer 的 DocxEditor） |

---

## 数据流总览

```
┌─────────────────────────────────────────────────────────────────┐
│                         User (Desktop)                           │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│  WebView2 / WebKit (React 19 + CodeMirror + ProseMirror)        │
│  src/                                                           │
└─────────────────────────────────────────────────────────────────┘
                              │ invoke (Tauri IPC)
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│  Rust main process (src-tauri/src/lib.rs)                        │
│  ┌─────────────────┐                                            │
│  │  commands/*     │ 50+ IPC handlers                           │
│  └────────┬────────┘                                            │
│           ├─→ ai.rs ─→ reqwest ─→ ┌──────────────────────────┐  │
│           │                       │ OpenAI / DeepSeek /      │  │
│           │                       │ Ollama / Official        │  │
│           │                       └──────────────────────────┘  │
│           ├─→ cloud/CloudClient ──→ cloud-server (port 8080) ──→ │
│           │                       ┌──────────────────────────┐  │
│           │                       │ Postgres + upstream LLM  │  │
│           │                       └──────────────────────────┘  │
│           ├─→ agent/agent_loop + 22 tools ─→ file/web/office/   │
│           │                                pptx/knowledge       │
│           ├─→ office/{docx,xlsx} ─→ 自研 OOXML 流式读写          │
│           ├─→ knowledge/ ─→ fastembed + Qdrant Edge（嵌入式）    │
│           ├─→ snapshots/ ─→ ~/.inkuo/snapshots/<hash>/...        │
│           └─→ file_watcher ─emit file-change─→ 前端文件树刷新    │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼ (可选 Cloud 模式)
┌─────────────────────────────────────────────────────────────────┐
│  cloud-server (Docker Compose)                                  │
│  ┌────────────┐ ┌────────────┐ ┌────────────┐ ┌────────────┐    │
│  │  Api       │ │ Billing    │ │ Admin API  │ │ Postgres   │    │
│  │  :8080     │ │ :8081      │ │ :8082      │ │            │    │
│  └────────────┘ └────────────┘ └────────────┘ └────────────┘    │
│                                       │                          │
│                                       ▼                          │
│                            admin-frontend (React + AntD)        │
└─────────────────────────────────────────────────────────────────┘
```

---

## 项目关键设计决策

1. **完全自研 OOXML 引擎** — 不依赖 mammoth / openpyxl 等第三方读写库，因为要支持 `create_word_doc` 的结构化增量编辑（按 id replace / 按 anchor 插入）
2. **Rust 侧 agent_loop + TypeScript 侧 aipanel 流式架构** — SSE delta → `StreamPayload` → `useAgentStream` 三路分发（text / reasoning / tool_calls）
3. **二元资源 side-channel（asset_registry）** — 避免 1MB PNG base64 进 context（≈250k tokens）
4. **Prompt 编译期 `include_str!`** — 零运行时 IO，性能 + 部署便利
5. **Main Agent 刻意不带 Office 工具** — 保持 tool schema 小，delegate 给 `office_word_expert` 等
6. **快照非 Git 而是全量复制** — docx/xlsx/pptx 二进制无损，可精确还原
7. **Pre-restore 自动备份** — restore 前在 `~/.inkuo/backups/` 落盘，防止用户操作失误
8. **CloudClient hydrate 同步**（不走 spawn） — 避免 race：用户在 hydrate 完成前发 chat 请求导致 "not logged in"
9. **`SHARED_STORES` 静态 RwLock 复用 KB VectorStore** — Qdrant Edge 单进程 WAL 限制
10. **WebView2 console → 文件桥**（`frontend_diag`） — Release build 无 console 时崩溃现场可恢复
11. **`pollWatcher` 而非 `RecommendedWatcher`** — inotify 在容器/Docker/overlay 上不可靠，轮询换来稳定性
12. **三主题 OKLCH** — 颜色感知一致；subtle/hover 由 `color-mix` 派生，避免散落硬编码 alpha
13. **Dev/release profile 优化** — `lto="thin" codegen-units=1 panic="abort" strip="symbols"` 让 .exe 体积更小

---

## 一句话总结

**InkUO = Tauri 2 桌面壳（Rust + React 19 + CodeMirror 6 + ProseMirror） + 自研 OOXML 引擎（docx/xlsx） + PPT/SVG 生成 + Agent 工具生态（22+ 工具 + 9 子代理 + 编译期嵌入的 18 份 prompt markdown） + 本地 RAG（fastembed + Qdrant Edge） + 可选 Cloud Server（.NET 8 + EF Core + Postgres + JWT + Stripe-free 兑换码计费 + AntD Admin）**。

文档结束。
