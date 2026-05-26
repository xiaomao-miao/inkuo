# AI：Edit/Agent ChangeSet 与 Patch 规范

本规范用于右侧 AI 面板的 Edit/Agent 模式，实现“对话式修改工作区任意文件”，且必须可审查、可回滚。

---

## 1. 二阶段：Plan → Apply（强约束）

### 1.1 Plan（只读）
- 目的：先让用户看到影响面与风险，避免误改。
- Plan 阶段禁止产生写盘 patch。

Plan 输出（必须为严格 JSON）：
- `plan_summary`：一句话目标与策略
- `files_to_touch[]`：数组，元素包含：
  - `path`
  - `intent`：`read`/`modify`/`create`/`delete`/`rename`
  - `reason`
- `risk`：`low`/`medium`/`high`
- `needs_confirmation`：布尔值（workspace 默认 true）

### 1.2 Apply（可写，但先预览后落盘）
- 目的：输出可应用的变更集。
- Apply 输出必须能在 UI 中生成：文件列表 → 打开文件 → Inline Diff → Apply/Reject。

---

## 2. ChangeSet（Apply 输出）

ChangeSet 输出（必须为严格 JSON）：
- `summary`
- `risk`：`low`/`medium`/`high`
- `files[]`：
  - `path`
  - `action`：`modify`/`create`/`delete`/`rename`
  - `reason`
- `patches[]`：每个文件一条 patch（见第 3 节）
- `diff_view[]`：用于 UI 的 hunks 信息（可由本地 diff engine 生成；模型可选输出）

约束：
- `files[]` 与 `patches[]` 必须一一对应。
- 当存在 `delete/rename`：
  - MUST 提供恢复策略（回收站/备份路径）
  - MUST 要求二次确认

---

## 3. Patch 格式（统一：Unified Diff）

### 3.1 为什么统一用 unified diff
- 可读、可审查
- 可直接映射到 hunks
- 便于在本地实现 patch apply 与冲突检测

### 3.2 Patch 约束（MUST）
- `patches[].format` 固定为 `unified-diff`。
- 必须包含文件路径标头：
  - `--- a/<path>`
  - `+++ b/<path>`
- 必须包含至少 1 个 hunk：`@@ ... @@`
- 编码：UTF-8 文本。
- 禁止二进制 patch。
- 单个文件 patch 超过阈值（例如 200KB 或 2000 行 diff）必须拆分或要求用户确认。

### 3.3 Apply 行为
- patch apply 必须：
  - 可检测冲突（基于上下文行）
  - 冲突时不得静默覆盖，必须提示并进入手动选择/重新生成

---

## 4. 安全与权限
- 默认先预览后应用：ChangeSet 生成后处于 preview 状态。
- 提供只读模式：只生成 Plan/ChangeSet 预览，不允许 apply。
- 对 workspace 范围修改必须显示：
  - 影响文件数
  - delete/rename 数量
  - 风险等级与原因
