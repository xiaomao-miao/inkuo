# 架构总览（Architecture Overview）

本文档定义 inkuo 最终产物的总体架构、运行时拓扑与模块边界。

## 1. 目标
- Local‑First：核心编辑、索引、AI 自带 Key 调用均在本机完成。
- 可扩展：云同步、官方 AI、团队能力通过可选云服务挂载。
- 可控可审查：任何 AI 改动以 ChangeSet + Diff 呈现，支持逐块 Apply/Reject。

## 2. 运行时拓扑

```
[UI (React)]
  - App Shell (三栏布局)
  - Editor Host (Markdown / RichText / Grid)
  - Diff Rendering Layer
  - AI Right Panel (Chat / Edit-Agent)
      ↕  IPC
[Local Core (Rust)]
  - File System / Workspace
  - Document Engine (md/docx/xlsx)
  - Diff Engine
  - AI Proxy (providers)
  - RAG Index (SQLite + vec)
  - Security (keyring)
      ↕  HTTPS (optional)
[Cloud Services]
  - Auth/Billing
  - Sync (E2EE)
  - Official AI Gateway
  - Team Admin
```

## 3. 模块职责边界

### 3.1 前端（UI）负责
- 编辑器渲染与交互（光标、选区、输入法）。
- Diff/hunk 装饰层渲染（颜色、卡片、按钮、快捷键）。
- Cmd+K 浮窗与右侧 AI 面板的交互状态机。
- Settings UI（theme/provider/scope/安全提示）。

### 3.2 本地核心（Rust）负责
- 文件读写、watch、备份/恢复、回收站（delete/rename）。
- 文档解析与序列化：Markdown AST、docx/xlsx 内部表示与回写。
- Diff 计算：文本 diff 与映射元数据（hunks、统计、风险评估）。
- AI Proxy：统一调用、流式处理、协议校验、降级。
- RAG：索引构建、增量更新、召回、上下文拼装（含来源元数据）。
- Keyring：存取密钥，确保前端不拿到明文 key。

### 3.3 云端（可选）负责
- 登录、订阅、配额、账单。
- E2EE 同步（仅存密文）。
- Official AI Gateway（鉴权、路由、稳定性增强）。
- 团队管理与审计（Team/Enterprise）。

## 4. 不变约束（Implementation Invariants）
- AI 自带 Key 模式：请求不得经过 inkuo 服务器。
- Workspace 写入：必须“先预览后应用”，并可 Undo。
- delete/rename：必须二次确认并可恢复。
