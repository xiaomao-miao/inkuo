# 格式：Word（.docx）解析与回写

## 1. 目标
- 打开 docx 并提供 Markdown / 富文本两种视图。
- 编辑后可回写 docx，并提供备份与保存报告。

## 2. 内部表示
- 段落（paragraph）
- 标题（heading level）
- 列表（ordered/unordered）
- 表格（table rows/cells）
- 内联样式（bold/italic/link/code）

## 3. 视图
- Markdown：便于开发者快速编辑
- 富文本：便于排版

## 4. 回写策略（保守）
- 保存前生成 `.bak`。
- 尽量保留原段落结构与样式框架。
- 对无法映射的样式：降级并在保存报告中提示。

## 5. 风险提示
- 复杂版式、浮动元素、宏等不保证无损。
- 任何写回失败必须回滚到备份。
