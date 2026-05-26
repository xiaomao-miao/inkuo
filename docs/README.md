# inkuo 开发文档（索引）

本目录基于 `inkuo-product-spec.md` 拆分出可直接用于实现的开发文档。目标是：后续开发只要按这些文档逐项实现与验收即可。

## 0. 产品规格（总纲）
- `inkuo-product-spec.md`：最终产物的产品规格总纲（权威来源）。

## 1. 架构与模块
- `architecture/overview.md`：总体架构、模块边界、运行时拓扑。
- `architecture/local-core.md`：Rust 本地核心职责与模块划分。
- `architecture/frontend.md`：前端模块划分与状态管理建议。
- `architecture/data-model.md`：数据模型（SQLite、向量索引、会话记录）。

## 2. UI / 交互（Cursor-like）
- `ui/layout.md`：三栏布局、可折叠、布局记忆。
- `ui/theme.md`：主题系统（Cursor-like 基线 + Accent 可配置）、design tokens。
- `ui/editor.md`：编辑器承载层（Markdown/Word/Excel）。
- `ui/cmdk.md`：Cmd+K 浮窗与 scope 选择。
- `ui/inline-diff.md`：Inline Diff 视觉与交互（hunk、摘要、Apply/Reject、Undo）。
- `ui/right-panel-ai.md`：右侧 AI 面板（Chat / Edit-Agent）交互规范。

## 3. AI 能力与协议
- `ai/providers.md`：OpenAI-compatible / DeepSeek / Ollama / 官方 AI 适配规范。
- `ai/protocol.md`：结构化输出协议（summary/content/rules）、降级策略。
- `ai/agent-changeset.md`：Edit/Agent 的二阶段（Plan→Apply）与 ChangeSet/patch 规范。

## 4. 知识库（RAG）与引用
- `rag/at-mentions.md`：`@file/@section/@table/@selection` 引用语法与 UI。
- `rag/indexing.md`：分段、嵌入、增量更新、向量检索。
- `rag/citations.md`：引用回溯与输出格式约束。

## 5. 多格式（Word / Excel）
- `formats/docx.md`：docx 解析、内部表示、回写与备份。
- `formats/xlsx.md`：xlsx 网格、公式保留、写回策略与风险提示。

## 6. 安全、权限与可靠性
- `security/key-storage.md`：keyring/secret-service、前端不接触明文 key。
- `security/workspace-write-guard.md`：Workspace 写入保护、二次确认、只读模式。
- `quality/performance.md`：性能目标与测试口径。
- `quality/observability.md`：日志、脱敏导出、可观测性。

---

> 约定：除非另有说明，本目录文档中的 MUST/SHOULD 词汇具有规范性含义，用于实现与验收。
