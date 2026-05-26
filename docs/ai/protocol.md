# AI：结构化输出协议（summary/content/rules）

本协议用于 Cmd+K 与右侧 Edit/Agent 的可落地编辑输出。目标：可解析、可 diff、可应用。

## 1. 基础编辑输出（Cmd+K / 单文件编辑）

### 1.1 必填字段
- `summary`：一句话说明改了什么/为什么改
- `content`：修改后的目标文本（与 scope 对应）

### 1.2 可选字段
- `rules_applied[]`：模型遵循的规则
- `citations[]`：引用来源（path + range + snippet hash）

### 1.3 JSON 约束
- 必须为严格 JSON（无尾逗号）。
- 字段必须可 schema 校验。

## 2. 降级策略（MUST）
当 provider 不支持严格 JSON 或返回不可解析内容：
1. 视为纯文本输出，作为 `content`。
2. `summary` 由本地生成（基于 diff 统计与启发式）。
3. UI 明示“协议降级，建议复核”。

## 3. 安全约束
- 默认启用代码块保护、表格/列表结构保护（除非用户明确允许）。
- 对全文替换与大规模重排必须提示风险。
