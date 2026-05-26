# UI：编辑器承载层（Editor Host）

## 1. 编辑器类型
- Markdown：CodeMirror 6
- Rich Text（Word 视图）：ProseMirror
- Data Grid（Excel 视图）：自研网格或成熟组件（需支持大表性能）

## 2. 共性能力（MUST）
- Undo/Redo 统一行为（AI Apply/Reject 也必须进入 undo 栈）。
- 查找替换、跳转、命令面板。
- 跨编辑器的一致快捷键与焦点管理。

## 3. Markdown（CodeMirror）
- 语法高亮、折叠、Vim/Emacs 模式。
- Diff Layer 以 decorations/marks 实现。

## 4. Rich Text（ProseMirror）
- 用于 docx 的所见即所得编辑。
- 与 docx 内部表示可双向映射。

## 5. Excel Grid
- 需要：
  - 虚拟滚动
  - 多工作表
  - 基础格式显示
  - 公式展示（尽量不重写）
- AI 对数据区域的修改必须生成 ChangeSet，并支持预览与回滚。
