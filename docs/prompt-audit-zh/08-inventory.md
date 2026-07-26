# 08. 提示词资产全量索引

## A. 核心 system prompt

| 资产 | 来源 | 状态 | 触发 |
|---|---|---|---|
| Agent | `src-tauri/prompts/main/agent.slim.md` | 运行时 | Agent 模式 |
| Ask | `src-tauri/prompts/ask.md` | 运行时 | Ask 模式、部分浮动 Ask |
| Plan | `src-tauri/prompts/plan.md` | 运行时 | Plan 模式 |
| Edit | `src-tauri/prompts/edit.md` | 运行时 | Cmd+K 编辑 |
| DOCX Complete | `src-tauri/prompts/docx_complete.md` | 运行时读取 | Word Tab 补全 |

## B. 动态 system 叠加

| 资产 | 来源 | 状态 |
|---|---|---|
| Runtime State | `src-tauri/src/runtime_state.rs:100-164` | 每轮 |
| Toggle inventory | `src-tauri/src/feature_toggles.rs:112-159` | 每轮 |
| KB strict | `src-tauri/prompts/fragments/kb_strict.md` | 条件开启 |
| Web search | `src-tauri/prompts/fragments/web_search.md` | 条件开启 |
| Workspace path | `src-tauri/src/commands_agent.rs:329-335` | 有工作区时 |

## C. 子代理 system prompt

| Profile | 文件 | 运行时注册 |
|---|---|---|
| `office_word_expert` | `subagents/office_word_expert.md` | 是 |
| `office_excel_expert` | `subagents/office_excel_expert.md` | 是 |
| `office_pptx_expert` | `subagents/office_pptx_expert.md` | 是 |
| `md_writer` | `subagents/md_writer.md` | 是 |
| `researcher` | `subagents/researcher.md` | 是 |
| `batch_editor` | `subagents/batch_editor.md` | 是 |
| `code_expert` | `subagents/code_expert.md` | 是 |
| `flowchart_expert` | `subagents/flowchart_expert.md` | 是 |
| `word_image_expert` | `subagents/word_image_expert.md` | 是 |
| 设计说明 | `subagents/README.md` | 仅文档，不发送 |

## D. Tool Specs

| Category/文件 | `TOOL_SPECS` 注册 | 可经 `get_tool_help` 取得 |
|---|---:|---:|
| general | 是 | 是 |
| word | 是 | 是 |
| excel | 是 | 是 |
| pptx | 是 | 实现可查，但工具短描述未公布 |
| markdown | 是 | 是 |
| media | 是 | 是 |
| svg | 是 | 是 |
| pptx_animation | 否 | 否 |
| add_pptx_animation | 否 | 否 |

## E. Rust 内嵌提示

| 内容 | 位置 | 状态 |
|---|---|---|
| 通用代码 FIM user prompt | `inline_complete/mod.rs:341-358` | 运行时 |
| 补全最小 system | `inline_complete/mod.rs:379` | 运行时 |
| DOCX fallback | `inline_complete/mod.rs:439-464` | 文件缺失时 |
| 子代理 task/context 拼接 | `agent_loop.rs:903-906` | 委派时 |
| AI 连接测试 | `ai_config.rs:424-430` | 设置测试 |
| 图像测试 prompt | `ai_config.rs:468-474,526-536,630-636` | 设置测试 |

## F. 前端 user prompt 模板

| 内容 | 来源 | 状态 |
|---|---|---|
| AI 回答选区介绍/解释/展开/拒绝 | `SelectionQuickActions.tsx:51-81` | 点击即发送 |
| 空会话总结/解释/目录 | `ChatEmptyState.tsx:38-45` | 点击填充，手动发送 |
| 选区右键四模板 | `menuBuilders.tsx:1134-1161` | 浮动 Ask |
| 文件树四模板 | `menuBuilders.tsx:1251-1281` | 浮动 Ask |
| 编辑器文件四模板 | `menuBuilders.tsx:1613-1640` | 浮动 Ask |
| Cmd+K 四预设 | `CmdK.tsx:18-23` | Edit instruction |
| 执行 Plan | `useChatSessionActions.ts:507-525` | 自动切 Agent 并发送 |
| 调整 Plan | `useChatSessionActions.ts:534-548` | 填充输入框 |

## G. 工具 schema 描述

所有 `src-tauri/src/agent/tools/**/*.rs` 中 `definition()` 的 description 和参数 description 都会随 API `tools` 数组进入模型。最关键的是：

- `meta_tools.rs`：`get_tool_help`、`delegate_to`。
- 文件/搜索/Office/计划/Todo/询问用户等工具。
- `generate_image.prompt/negative_prompt` 与子代理 task/context 等二级模型输入。

本审计没有逐字复制每个参数 label，因为它们数量大且主要由 JSON schema 表达；但已将影响路由与能力发现的 meta tool 描述纳入问题分析。

## H. 非提示词或仅文档

- `src/constants/chatModes.ts`：模式 UI 标签。
- `composer/toggles.tsx`：开关 label/hint，发送给后端的是 id。
- `toolRender/registries.ts`、`fieldLabels.ts`：显示名称。
- `README.md`、`docs/architecture.md`、cloud deployment 文档：普通文档。
- 测试文件中的 `hi/hello/summarize`：fixture。
- `ai_config.rs` 顶部 `_MARKER_QQXX42...`：未被消费的静态标记，不是 prompt。

## I. 装配优先级速查

```text
模式基础 system
  ↓ 追加
Runtime State
  ↓ 追加
Toggle inventory
  ↓ 开启时追加
KB/Web 使用指导
  ↓ 追加
Workspace path
  ↓
前端 history
  ↓
当前 instruction
  ↓
API tools schema
```

模型行为还受 Provider、模型上下文窗口、tool calling 能力、历史长度和工具结果影响，不能只看单个 Markdown prompt 判断。
