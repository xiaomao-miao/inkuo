# 前端架构（Frontend Architecture）

本文档定义 UI 层模块、状态管理与与本地核心的交互原则。

## 1. UI 模块
- App Shell（三栏布局：左 Workspace / 中 Editor / 右 AI）
- Workspace Panel（文件树、搜索、最近）
- Editor Host
  - Markdown Editor（CodeMirror 6）
  - Rich Text Editor（ProseMirror）
  - Data Grid（Excel）
- Diff Layer（decorations、hunk widget、快捷键）
- Cmd+K Modal（scope、@引用、模板、历史）
- Right AI Panel（Chat / Edit-Agent）
- Settings（Provider/Key/Theme/Security）

## 2. 状态管理建议
- 文档状态：
  - 当前打开文件、未保存更改、光标/选区、编辑器视图状态
- Diff 会话状态：
  - 当前 ChangeSet、hunk 列表、当前聚焦 hunk、Apply/Reject 进度
- Right Panel 状态：
  - 当前模式 Chat/Edit-Agent、对话历史、scope、引用集合
- 布局状态：
  - 左右面板是否展开、宽度、上次选择 tab/模式（需要持久化）

## 3. IPC 交互原则
- 前端只负责 UI 与用户交互；任何写盘、keyring、索引等由本地核心处理。
- IPC 命令必须是可取消的（尤其是 AI 流式与索引更新）。

## 4. 错误与降级
- AI 调用失败：不中断编辑流程；显示可重试按钮；保留已生成内容。
- Diff 失败：降级为“原文/新文”双栏对比或纯文本替换确认。
