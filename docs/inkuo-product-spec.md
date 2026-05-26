# inkuo 产品规格说明书（最终产物）

版本：3.0（最终产物文档）  
日期：2026-05-26  
项目代号：**inkuo —— 为思想留下的印记**

> 本文档用于描述 inkuo 的“最终产物”（目标形态），不按阶段拆分，不讨论 MVP/迭代路线，仅以可交付的软件产品为目标，给出完整的功能、交互、技术与运营说明。

---

## 0. 一句话定位
**inkuo 是一款 Local‑First 的 AI 文档编辑器：AI 直接在原文中完成可审查的修改，提供 Git 式可视化 Diff + 一句话修改意图摘要，并支持一键应用/拒绝。**

---

## 1. 产品目标与非目标

### 1.1 产品目标
1. **像 Cursor 改代码一样改文档**：AI 修改发生在原文位置，用户在“编辑流”里完成闭环。
2. **可信可控**：所有 AI 修改都必须可视化对比、可回退、可逐块接受/拒绝。
3. **Local‑First**：默认本地文件与本地索引，本地知识库可离线使用；无云账号也能完整工作。
4. **隐私零妥协**：自带 Key 模式下，调用链路直连用户模型供应商或本地模型，不经过 inkuo 服务器。
5. **原生体验**：启动快、占用低、键盘优先；支持 Vim/Emacs 编辑模式。

### 1.2 非目标（明确不做 / 不承诺）
1. **不承诺 Office 级 100% 版式还原**：Word/Excel 以“内容编辑与可回写”为核心，提供备份与恢复机制，允许存在边缘样式差异。
2. **不把 AI 当聊天应用**：inkuo 主要输出“可应用的编辑建议”，而不是长对话。
3. **不强制云端**：云同步/协作属于可选能力，离线与本地工作流必须完整。

---

## 2. 用户与使用场景

### 2.1 目标用户
- 开发者/工程师：README、API 文档、架构设计、周报。
- 研究人员/学生：论文笔记、文献综述、跨文档引用。
- 内容创作者：长文写作、提纲与结构重写、风格统一。
- 知识工作者：方案、汇报、制度、SOP。

### 2.2 典型场景
1. **段落润色**：选中一段 → Cmd+K → “更专业，保留所有数据与术语” → Diff + 摘要 → Tab 接受。
2. **结构重排**：选中多段 → Cmd+K → “重写为 5 个步骤的小标题结构” → Diff 分块 → 逐块接受。
3. **跨文档问答 / 写作辅助**：输入 `@` 引用多篇文档/表格 → “总结差异并给结论” → 生成可插入到当前文档的片段，并附引用来源。
4. **Word/Excel 无损回写**：打开 `.docx` / `.xlsx` → 编辑内容与数据 → 保存 → 写回原格式并保留备份。

---

## 3. 核心体验：In‑Context AI 编辑流

### 3.1 编辑器基础能力
- 文档类型：
  - Markdown（`.md`）为一等公民。
  - Word（`.docx`）与 Excel（`.xlsx`）可打开、编辑并回写。
- 编辑器能力：
  - 语法高亮、折叠、搜索替换、命令面板。
  - Vim/Emacs 模式切换。
  - 深色/浅色主题。
  - **Cursor 风格 UI（可配置配色）**：整体布局、间距、阴影、边框与对比度以 Cursor 的“极简开发者工具风格”为基准；用户可在设置中切换主题与强调色，且可导入/导出主题。
  - 可配置字体、行距、宽度、拼写检查（可选）。

### 3.2 Cmd+K：上下文编辑入口

#### 触发方式
- 快捷键：`Cmd+K`（macOS） / `Ctrl+K`（Windows/Linux）。
- 右键菜单：`AI Edit…`。
- 命令面板：`AI: Edit Selection`。

#### 作用范围（Scope）规则
1. **有选区**：仅对选区内容生效。
2. **无选区**：对“光标所在段落”生效（段落由空行或块级边界界定）。
3. **用户手动扩展范围**：弹窗内提供 Scope 切换：
   - Selection / Paragraph / Section（当前标题下）/ Document（全文）。

#### 输入框行为
- 浮窗锚定在选区附近，避免遮挡。
- 支持多行指令。
- 支持预置指令模板（可编辑）：
  - “更专业” “更精炼” “改成表格” “生成小标题” “翻译为英文并保留术语” 等。

### 3.3 AI 输出协议（关键约束）
AI 必须输出结构化结果，确保可解析、可落地：

- **输出必须包含：**
  - `summary`：一句话说明“改了什么/为什么改”。
  - `content`：修改后的目标文本（与 scope 对应）。
  - `rules_applied`（可选）：列出遵循的约束（如“保留数字/不改代码块”）。

推荐使用 JSON Schema / 严格 JSON（不同模型能力不同，但产品层面以该协议为标准）。

当模型无法严格输出时的降级：
- 以文本输出为 `content`；
- `summary` 由本地生成（例如基于 diff 统计：“新增 2 句，删除 1 句，调整语气为正式”）。

### 3.4 Diff 可视化与摘要卡片

#### Diff 渲染原则
- Diff **以内联方式**呈现在原文位置，不在侧边栏堆输出。
- 采用 Git 风格：
  - 删除：红色背景（或红色删除线）。
  - 新增：绿色背景。
- Diff 的最小单元：**连续改动区间（hunk）**。

#### 摘要卡片
- 每个 diff hunk 顶部显示 `summary`（一句话）。
- Hover/展开可显示更细粒度解释（如逐句原因），并可展示“变更统计”（可选）。

### 3.5 接受 / 拒绝 / 逐块控制

#### 基本快捷键
- `Tab`：接受当前 hunk（Apply）。
- `Shift+Tab`：接受全部 hunk。
- `Esc`：拒绝当前 hunk。
- `Cmd/Ctrl+Esc`：拒绝全部并退出 diff 模式。

#### 鼠标交互
- 每个 hunk 右上角提供：Apply / Reject / Copy。
- 支持“只复制修改后的内容”与“复制 diff”两种。

#### Undo/Redo 与回退
- Apply / Reject 必须进入编辑器的 Undo 栈。
- 任何时候用户都可以 `Undo` 回到 AI 修改前。
- inkuo 额外提供“本次 AI 会话历史”（session log），支持回放/重做（可选）。

### 3.6 安全护栏（防止 AI 破坏结构）
- 默认启用：
  - **代码块保护**：不允许模型改动 fenced code block 内部内容（除非用户明确允许）。
  - **表格/列表结构保护**：Markdown 表格与列表优先做结构化编辑；无法保证时提示用户“将改为纯文本结果，是否继续”。
- 对高风险改动（例如大段重排、全文替换）提供二次确认，并默认分块展示 diff。

---

## 4. 知识库与引用：@file / @note / @table

### 4.1 引用语法
- 在 Cmd+K 指令或任意输入框内输入 `@` 唤起引用选择器。
- 支持类型：
  - `@file`：引用工作区内文件（md/docx/xlsx/pdf/纯文本）。
  - `@section`：引用某文件内某标题段。
  - `@table`：引用表格区域（来自 md 表格或 xlsx 区域）。
  - `@selection`：引用当前编辑器选区（用于组合指令）。

### 4.2 引用解析与上下文拼装
- 引用项进入上下文时必须携带：
  - `title`（来源名）
  - `path`（文件路径）
  - `range`（段落/表格坐标）
  - `excerpt`（截断内容）
- 默认策略：
  - 先用向量检索召回相关片段，再按 token 预算拼装。
  - 必须附带来源引用标记（例如 `[^source:xxx]`），便于回溯。

### 4.3 本地 RAG 存储
- 向量索引：SQLite + 向量扩展（如 sqlite-vec）。
- 元数据：文件 hash、段落边界、更新时间。
- 更新策略：文件保存后增量更新索引。

---

## 5. 多格式：Word / Excel 的编辑与回写

### 5.1 Word（.docx）

#### 打开与内部表示
- 解析 docx 为：
  - 主体内容：段落、标题、列表、表格。
  - 样式元数据：常用样式映射（标题级别、粗体、斜体、链接、引用等）。
- 编辑时提供两种视图：
  - Markdown 视图（默认，利于开发者/文本编辑）。
  - 富文本视图（所见即所得，适用于排版需求）。

#### 回写策略（保守）
- 保存时：
  1) 自动创建备份：`filename.docx.bak`（可配置保留数量）。
  2) 将内部表示写回 docx：尽量保留原有段落结构与样式框架。
  3) 无法无损映射的样式降级为近似样式，并在保存报告中提示。

### 5.2 Excel（.xlsx）

#### 打开与编辑
- 以数据网格方式展示，保留：
  - 多工作表、冻结行列、基础格式。
  - 公式（尽量保留，不随意重写）。
- 支持自然语言操作：
  - “把 B 列格式改成百分比并保留两位小数”
  - “按销售额降序排序并给前 10 行加高亮”

#### 回写策略
- 保存时：
  1) 自动备份 `.xlsx.bak`。
  2) 尽量只写回修改过的单元格与样式。
  3) 对无法保留的复杂对象（图表、宏、复杂条件格式）做保守处理并提示。

---

## 6. 账号、同步与协作（可选能力，但为最终产物定义）

### 6.1 登录与套餐
- Community：无需登录即可使用本地全部能力（含自带 Key AI）。
- Pro/Team/Enterprise：登录后启用云同步、官方 AI、团队能力。

### 6.2 云同步
- 端到端加密（E2EE）：客户端生成密钥，服务端仅存密文。
- 冲突策略：
  - 基于版本向量/CRDT 的合并优先；
  - 无法自动合并时生成冲突副本，并提供 diff 解决。

### 6.3 实时协作
- 基于 CRDT 的多人编辑。
- 权限：只读/可编辑/可管理。
- 审计日志：记录关键变更与 AI 应用。

---

## 7. AI 提供商与“自带 Key”

### 7.1 支持的模型来源
- OpenAI / OpenAI-Compatible（DeepSeek 等）。
- Ollama（本地模型）。
- 企业自建模型网关（OpenAI 兼容协议）。

### 7.2 Key 与隐私
- Key 只存储在本机安全存储（系统 keychain / secret-service）。
- 默认不把 key 暴露给前端 JS：由本地核心模块代理请求。
- 明示数据流路径：设置页面必须展示“请求直连模型供应商/本地模型，inkuo 不经手”。

### 7.3 官方 AI（会员）
- 通过 inkuo 网关统一鉴权、配额、模型路由。
- 支持按量计费与套餐。
- 提供稳定性增强：重试、缓存、区域加速（对用户透明）。

---

## 8. 系统架构与模块划分

### 8.1 总体架构
```
[UI: React + Editor]
        ↕ (IPC)
[Local Core: Rust]
  - File I/O
  - Index / RAG
  - AI Provider Adapter (Local Proxy)
  - Diff Engine
        ↕ (HTTP, Optional)
[Cloud Services]
  - Auth & Billing
  - Sync (E2EE)
  - Official AI Gateway
  - Team Admin
```

### 8.2 本地核心模块
1. **Document Engine**：
   - Markdown AST（comrak）
   - Word/Excel 解析与回写
2. **Diff Engine**：
   - 文本 diff（Myers / patience）
   - 映射到编辑器装饰层
3. **AI Adapter**：
   - OpenAI-compatible
   - Ollama
   - 官方 AI
   - 统一流式输出协议
4. **Index & RAG**：
   - 分段、嵌入、向量检索、引用注入
5. **Security**：
   - keyring
   - 权限隔离与沙箱策略

### 8.3 前端模块
- Editor Host（Markdown / Rich Text / Grid）
- Diff Rendering Layer（decorations + hunk widget）
- Cmd+K UI（scope、引用、模板、历史）
- Settings（Provider、Key、模型、隐私提示）
- File Explorer / Workspace

---

## 9. 数据模型（概念层）

### 9.1 文档与段落
- Document：`id, path, type, title, updated_at, hash`
- Block：`doc_id, block_id, kind, range, text, metadata`

### 9.2 AI 会话
- AISession：`session_id, doc_id, scope, instruction, provider, model, created_at`
- AIChangeSet：`session_id, hunks[], summary, stats`

### 9.3 知识库索引
- EmbeddingChunk：`chunk_id, doc_id, range, text, embedding, updated_at`
- Citation：`source_doc, range, snippet, hash`

---

## 10. 界面布局与 AI 面板（Cursor-like 三栏）

### 10.1 三栏布局（默认 Cursor-like）
- **左侧：Workspace / 文档目录**
  - 文件树、搜索、最近打开、收藏（可选）。
  - 支持多根目录挂载（Workspace folders）。
- **中间：主编辑器**
  - 按文件类型切换：Markdown / 富文本（Word）/ 数据网格（Excel）。
  - 支持分屏（可选）。
- **右侧：AI 面板（Chat / Edit/Agent）**
  - 常驻可折叠，可拖拽宽度。
  - **记住上次布局状态**（是否展开、宽度、所处标签页）。

### 10.2 右侧 AI 面板：Chat 与 Edit/Agent

#### Chat（对话）
- 用于问答、解释、总结、生成内容草稿。
- 每条回复提供快捷操作：
  - `Copy`
  - `Insert to cursor`（插入到当前光标）
  - `Apply to…`（将本条回复作为“修改指令/修改结果”应用到文件或选区）

#### Edit/Agent（对话式修改项目）
- 面向“直接修改工作区”的任务：
  - “把 docs 下所有文档标题统一成 X 风格”
  - “把这份文档改成最终产物口吻，删掉 MVP/阶段描述”
  - “批量更新跨文档引用，并补充术语表”

**强约束：二阶段（Plan → Apply）**
1. **Plan 阶段（只读）**：先输出计划与影响面，禁止直接写入。
2. **Apply 阶段（可写）**：用户确认后才生成可应用的变更集（ChangeSet）。

- **Plan 输出必须包含：**
  - `plan_summary`：一句话目标与策略
  - `files_to_touch[]`：将要读取/修改/新增/删除的文件（含原因）
  - `risk`：风险等级（`low`/`medium`/`high`）与触发原因
  - `needs_confirmation`：是否需要用户确认（Workspace 级默认 true）

- **Apply 输出必须为 ChangeSet（变更集），包含：**
  - `summary`：本次改动总摘要
  - `risk`：风险等级（`low`/`medium`/`high`）
  - `files[]`：受影响文件列表
    - `path`
    - `action`：`modify`/`create`/`delete`/`rename`
    - `reason`
  - `patches[]`：对每个文件给出可应用补丁（推荐 unified diff 或等价结构化 patch）
  - `diff_view[]`：用于 UI 展示的 diff hunks + 每块摘要（与中间编辑器 Inline Diff 一致）

- 应用方式必须可审查：
  - Workspace 级改动默认进入“预览”状态，不直接落盘
  - 支持 `Apply all` / `Apply file` / `Apply hunk` 与对应 Reject
  - 所有 Apply/Reject 必须进入 Undo 栈，并可从会话历史回滚
  - 当 `action` 包含 `delete/rename` 时，必须二次确认并提供可恢复入口（回收站/备份）

### 10.3 Scope（作用范围）与引用
- 右侧 AI 面板顶部提供 Scope 选择：
  - `Selection` / `Current File` / `Workspace` / `Pick files…`
- 当选择 `Workspace` 时：
  - 必须二次确认，并展示将触达的文件范围（包含排除项）
- 输入框支持 `@` 引用：
  - `@file` / `@section` / `@selection` / `@table`

### 10.4 多文件变更视图（Changes）
- Edit/Agent 产生多文件变更时，右侧展示一个 `Changes` 列表（类似 git changes）：
  - 文件按路径分组，可筛选（新增/修改/删除）
  - 点击文件 → 中间编辑器打开该文件并呈现 Inline Diff
- Diff 交互与 Cmd+K 保持一致：
  - hunk 摘要卡片 + `Tab`/`Esc` 快捷键逐块控制

### 10.5 安全边界（Workspace 写入保护）
- 默认原则：**先预览、后应用**。
- 对以下操作强制二次确认：
  - 删除文件、批量重命名、改动超出 N 个文件、改动超出 token/字符阈值
- 支持“只读模式”开关：
  - 只允许生成建议与 diff，不允许写入文件

---

## 11. 主题与视觉风格（Cursor-like + 可配色）

### 10.1 视觉原则
- **Cursor-like**：默认主题参考 Cursor 的观感（中性底色、克制的边框与阴影、清晰的层级、强调色用于焦点与选中状态），整体偏“开发者工具”的简洁与高对比可读性。
- **内容优先**：减少不必要装饰；diff、引用、错误态使用一致的视觉语言。
- **键盘优先**：所有可达操作必须可通过键盘完成，并提供清晰的 focus ring。

### 10.2 主题系统
- 内置主题：
  - `Cursor Dark`（默认，强调色为蓝紫/冷色系）
  - `Cursor Light`
  - `High Contrast Dark/Light`
- 用户自定义（强调色优先）：
  - **用户主要可配置项为“强调色（Accent）”**，其余中性色阶（背景/前景/面板/边框）保持 Cursor-like 的设计基线，确保整体观感一致、不会被配色“玩坏”。
  - 允许附加微调项（可选开启）：diff（新增/删除）色相、选区颜色与 focus ring 强度。
  - 支持导入/导出主题（JSON），并可在团队/工作区内共享。

### 10.3 设计令牌（Design Tokens）
- 采用 token 驱动（CSS variables / design tokens），并在以下层面打通：
  - App Shell（侧边栏、标题栏、面板、弹窗）
  - Editor（文本、选区、光标、语法高亮）
  - Diff Layer（新增/删除/摘要卡片）
  - Data Grid（Excel 视图）
- 主题切换必须：
  - 即时生效，不重启
  - 不改变文档内容，仅改变表现层
  - 支持按工作区/项目保存主题偏好（可选）

---

## 12. 可靠性、性能与质量标准

### 12.1 性能目标
- 冷启动：常规设备 < 1s（目标），< 2s（上限）。
- 大文档：10 万字 Markdown 仍可流畅滚动与编辑。
- Diff 渲染：应用 100 个 hunks 不卡顿；必要时虚拟化渲染。

### 12.2 稳定性目标
- AI 调用失败可重试、可取消、不中断编辑器主流程。
- 永远保证：文档不会因 AI 失败而丢失；保存路径可恢复。

### 12.3 可观测性（本地优先）
- 本地日志：默认仅存本机，可一键导出（脱敏）。
- 云端：仅会员功能上报必要指标（可关闭）。

---

## 13. 安全与合规
- 本地文件权限提示：首次访问目录需授权。
- 密钥安全：keyring + 权限隔离。
- 云同步：E2EE；服务端零明文。
- 企业版：SSO、审计、保留策略。

---

## 14. 定价与版本（最终产物）

| 层级 | 价格 | 权益 |
|------|------|------|
| Community | $0 | 本地编辑器 + 自带 Key AI 全功能（Cmd+K、Inline Diff、Apply/Reject、@引用与本地知识库） |
| Pro | $19/月 | 官方 AI（免配置）、E2EE 云同步、跨设备、Agent 自动化、优先支持 |
| Team | $39/席/月 | 团队空间、共享知识库、权限管理、协作与审计 |
| Enterprise | 定制 | 私有化部署、SSO、SLA、专属模型接入 |

---

## 15. 交付物清单（最终产品应包含）
1. 桌面客户端：macOS / Windows / Linux。
2. 内置更新机制（可关闭）。
3. 文档与入门：首次启动引导、示例库、快捷键手册。
4. 官网 Landing + Waitlist + 下载页。
5. 付费系统（Pro/Team/Enterprise）。

---

## 16. 附录：主题配置（建议）

> 主题系统以 design tokens 驱动，默认提供 Cursor-like 的中性色阶；用户主要调整 Accent（强调色）。

### 14.1 推荐主题 JSON（示例）

```json
{
  "name": "Cursor Dark (Custom Accent)",
  "base": "cursor-dark",
  "accent": "#7C5CFF",
  "options": {
    "diffHueAdded": 145,
    "diffHueRemoved": 5,
    "selectionAlpha": 0.25,
    "focusRingStrength": 1.0
  }
}
```

### 14.2 约束
- `base` 必须为内置基线之一（如 `cursor-dark` / `cursor-light`）。
- `accent` 为主要可配置项；除 `options` 白名单外，不允许覆盖基础中性色阶。
- 导入主题必须校验字段，避免破坏可读性。

---

## 17. 附录：默认快捷键（建议）
- `Cmd/Ctrl+K`：AI Edit
- `Tab`：Apply hunk
- `Shift+Tab`：Apply all
- `Esc`：Reject hunk
- `Cmd/Ctrl+Esc`：Reject all
- `Cmd/Ctrl+P`：命令面板
- `Cmd/Ctrl+S`：保存

---

## 18. 产品承诺（对用户的公开声明模板）
1. 你使用自己的 Key 时，inkuo **不经手**你的内容与密钥。
2. 任何 AI 修改都必须可视化、可回退、可逐块控制。
3. 文件永远属于你：标准格式存储，可用 Git 管理。

