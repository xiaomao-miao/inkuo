# UI：右侧 AI 面板（Chat / Edit-Agent）

## 1. 面板定位
- 常驻右侧，可折叠，可拖拽宽度。
- 记住上次状态：展开/宽度/模式。

## 2. 两种模式

### 2.1 Chat
- 对话问答、解释、总结、生成草稿。
- 每条消息提供：
  - Copy
  - Insert to cursor
  - Apply to…

### 2.2 Edit/Agent
- 对话式修改工作区任意文件。
- 必须输出二阶段（Plan→Apply）。

## 3. Scope（MUST）
- Selection / Current File / Workspace / Pick files…
- Workspace scope 必须二次确认，并展示影响范围。

## 4. 多文件变更（Changes 视图）
- 当生成 ChangeSet 涉及多文件：
  - 右侧展示文件列表与风险提示
  - 点击文件在中间编辑器显示 Inline Diff

## 5. 安全护栏（MUST）
- 先预览后应用。
- delete/rename 强制二次确认与可恢复。
- 支持只读模式（不写盘）。
