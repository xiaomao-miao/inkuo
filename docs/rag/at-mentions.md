# RAG：@ 引用语法与交互（@file/@section/@table/@selection）

## 1. 目标
- 允许用户在 Cmd+K 与右侧 AI 面板中引用工作区内容。
- 引用必须可回溯（带来源元数据）。

## 2. 触发与选择器
- 输入 `@` 弹出引用选择器。
- 支持模糊搜索与类型过滤。

## 3. 支持的引用类型
- `@file`：文件级引用（md/docx/xlsx/pdf/纯文本）
- `@section`：文件内标题段落引用
- `@table`：表格区域引用（md 表格或 xlsx 区域）
- `@selection`：当前编辑器选区引用

## 4. 引用项结构（MUST）
- `title`
- `path`
- `range`（段落 range 或表格坐标）
- `excerpt`（截断文本）
- `hash`（用于一致性校验）

## 5. 约束
- 引用注入必须受 token 预算限制。
- 必须保证来源可回溯：输出可带 citations。
