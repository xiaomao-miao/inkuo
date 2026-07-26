# 06. 前端生成的用户提示与测试提示

这些内容不是 system prompt，但会作为 user instruction 进入模型，因此同样影响行为。

## 6.1 AI 回答选区快捷操作

源：`src/components/aipanel/SelectionQuickActions.tsx:51-81`

- 介绍：`请简要介绍以下内容：\n\n"""\n{选中文本}\n"""`
- 解释：`请帮我解释这段内容：...`
- 详细展开：`请对以下内容进行更详细的展开说明：...`
- 拒绝：`我对这段回答/引用不满意，请忽略它并重新回答：...`

这些文本无需翻译，本身已是中文。选中内容可能包含模型回答、工具结果或用户消息。三引号没有转义；若内容本身含 `"""`，逻辑边界会提前闭合。“请忽略它”还可能被理解为忽略更广泛的上下文。

## 6.2 空会话建议

源：`src/components/aipanel/ChatEmptyState.tsx:38-45`

点击只填入输入框，用户发送后才成为 instruction：

- `总结这篇文档的主要内容`
- `解释这段代码/文本的工作原理`
- `查看当前文档目录结构`

这些短提示不附当前文档路径或 selection；模型只能依赖工作区工具自行推断“这篇/这段/当前”，歧义较高。

## 6.3 右键选区、文件树和编辑器文件

源：`src/components/sidebar/contextMenu/menuBuilders.tsx:1134-1161,1251-1281,1613-1640`

选区模板：

- `请帮我解释以下内容：...`
- `请把以下内容翻译成英文（保留原文格式与代码块）：...`
- `请简要总结以下内容的要点：...`
- `请把以下内容改写得更清晰流畅，保留原意：...`

文件模板把“以下内容”改成“以下文件的内容”。三组模板重复维护，已出现措辞分叉。文件内容按 `content.length/slice` 截断，注释称 24 KB，但实际单位是 JavaScript UTF-16 code units，并非 UTF-8 bytes；中文的真实字节数可能明显更大，也可能切断 emoji surrogate pair。

所有模板同样使用未转义三引号，文件正文中的指令会直接嵌入 user message，没有明确标为“不可信数据”。

## 6.4 Cmd+K 编辑预设

源：`src/components/cmdk/CmdK.tsx:18-23`

- 更专业：`将语言改得更专业正式`
- 更精炼：`精简内容，保留核心信息`
- 润色语法：`修正语法错误，优化句式`
- 添加小标题：`为每个段落添加简洁的小标题`

随后与 `originalText/scope/context` 一起进入 Edit Mode。当前 context 恒为空。若选择 scope 但没有真实 selection，代码静默退化为文档前 500 字符，可能编辑错误目标。

## 6.5 Plan 输出再执行

源：`src/components/aipanel/useChatSessionActions.ts:493-548`

### 应用计划

```text
请按照以下计划执行：{plan_summary}

涉及文件：
- {path} ({intent}): {reason}

风险说明：{risk_reason}
请按顺序处理每个文件，对每个 delete/rename 操作先和我确认。
```

它自动切到 Agent 并立即发送。`plan_summary/path/intent/reason/risk_reason` 都来自模型之前的结构化输出，没有转义或可信度标记。计划模型生成的文本因此被提升为高权限 Agent 的 user instruction。删除/重命名确认目前主要靠自然语言提示，仍应由工具层硬限制。

### 调整计划

```text
请调整计划："{plan_summary}"

当前涉及文件：
- ...

请告诉我需要怎么调整。
```

该文本只填入输入框，不自动发送。

## 6.6 当前消息与历史的重复

源：`src/components/aipanel/useChatSessionActions.ts:239-331,402-426`

普通发送时先把 user message 加进 store，再对包含该消息的 `liveMessages` 构造 history；随后还单独传同一 instruction。后端又按“system→history→current user”追加，因此当前问题会重复。

编辑旧消息和重发路径有专门的 `buildConversationHistoryBefore`，正确排除了目标消息；问题主要存在于 brand-new user message 分支。

## 6.7 AI 连接测试提示

源：`src-tauri/src/ai_config.rs:414-430`

文本模型连接测试：

> 请严格逐字说出：`Hello, connection successful!`

它只在设置中测试连接，不参与正常对话。

图像 Provider 测试使用：

> 一个小红色圆形（`a small red circle`）

用于 Ollama、腾讯 Token Hub 和腾讯 TC3 的最小图像请求，位置分别为 `ai_config.rs:468-474,526-536,630-636`。

## 6.8 工具参数中的二级 Prompt

- `delegate_to.task/context`：主模型写给子模型的用户消息。
- `generate_image.prompt/negative_prompt`：主模型写给图像模型。
- `create_svg.description`：影响生成资产的描述。

它们由模型动态生成，不是仓库固定文案，但构成模型到模型的信任链，应做长度限制、结构校验和来源标记。
