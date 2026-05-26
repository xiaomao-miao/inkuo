# UI：Cmd+K（In-Context AI Edit）

## 1. 入口
- `Cmd/Ctrl+K`
- 右键菜单 `AI Edit…`
- 命令面板 `AI: Edit Selection`

## 2. Scope 规则（MUST）
- 有选区：Selection
- 无选区：Paragraph
- 可切换：Selection / Paragraph / Section / Document

## 3. 输入框
- 多行输入
- 支持模板（可编辑）
- 支持 `@` 引用（见 RAG 文档）

## 4. 输出处理
- AI 输出必须解析为 `summary/content/rules_applied`（失败降级）。
- 生成 ChangeSet 后进入 Inline Diff 模式。

## 5. 取消与重试
- 用户可以随时取消生成。
- 失败可重试，并保留指令历史。
