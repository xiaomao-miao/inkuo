# UI：Inline Diff（hunks + 摘要 + Apply/Reject）

## 1. 目标
- AI 修改以内联 diff 呈现于原文位置。
- 每个 hunk 有一句话摘要，降低审查成本。

## 2. Diff 单元
- 最小单元：hunk（连续改动区间）。
- 每个 hunk 包含：
  - 删除片段（红）
  - 新增片段（绿）
  - 摘要卡片（summary）

## 3. 交互（MUST）
- `Tab`：Apply 当前 hunk
- `Shift+Tab`：Apply 全部
- `Esc`：Reject 当前 hunk
- `Cmd/Ctrl+Esc`：Reject 全部

## 4. 鼠标操作
- 每个 hunk：Apply / Reject / Copy

## 5. Undo/Redo
- Apply/Reject 必须进入编辑器 undo 栈。
- 用户可随时 Undo 回到 AI 修改前。

## 6. 降级
- Diff 计算失败或超阈值：降级为双栏对比或“替换确认”模式。
