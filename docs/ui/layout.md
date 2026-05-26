# UI：三栏布局（Cursor-like Layout）

## 1. 布局结构
- 左侧：Workspace（文档目录）
- 中间：Editor（主编辑器）
- 右侧：AI Panel（Chat / Edit-Agent）

## 2. 行为规范（MUST）
- 左右面板：
  - MUST 支持折叠/展开
  - MUST 支持拖拽改变宽度
  - MUST 支持键盘快捷键切换可见性
- 布局记忆：
  - MUST 记住左右面板展开状态与宽度
  - MUST 记住右侧 AI 面板的模式（Chat 或 Edit-Agent）
  - SHOULD 支持按 Workspace 保存偏好（不同项目不同布局）

## 3. 默认布局
- 首次启动：左侧与右侧默认展开（可在 onboarding 中选择）。
- 后续启动：以“记住上次状态”为准。

## 4. 可访问性
- focus ring 清晰可见，与主题 accent 一致。
- 面板折叠/展开必须可被屏幕阅读器识别（aria）。
