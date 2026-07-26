# 05. 动态叠加片段与内联补全提示

## 5.1 Strict Knowledge Base 片段

源：`src-tauri/prompts/fragments/kb_strict.md:1-41`

这是在用户启用“严格 KB 引用”时追加到当前模式 system prompt 的片段，不是独立模式，而是在 Ask/Plan/Agent 上叠加“只基于 KB”行为。

### 核心行为

- 每个主张以 `database_search` 等知识库工具为主要来源。
- KB 没有相关内容时必须明确说明，不得用常识补缺。
- 优先使用检索片段中的事实、数字和直接引用，只在必要时改述。

### 引用格式

每个回答必须以标题严格为 `## 参考来源` 的章节结尾；每个 bullet 引用实际贡献过内容的文档标题和路径。没有片段时写一条说明 KB 不含相关信息。

### 工具限制

Rust 会删除写工具，只保留只读和 KB 工具。不得编造事实，不得在可见回答中提及 toggle、fragment 或实现，不得用无法追溯到 KB 的背景知识填充。

## 5.2 Web Search 片段

源：`src-tauri/prompts/fragments/web_search.md:1-48`

开启“联网搜索”时追加。在它之前的 inventory 已声明 web_search 为 ON；真正工具可用性以 inventory 为准。

### 何时使用

- 用户询问工作区/KB 之外的真实人物、地点、组织、事件和概念时优先联网。
- 用户自己的文档优先 workspace 工具。
- 外部专名如爱因斯坦、OpenAI、上海倾向联网。
- 不要每轮都搜；对话历史足以回答时跳过网络请求。

### 引用

网络来源的主张应引用工具结果中的标题和 URL；无结果就明确说无结果；只引用或改述 summary，不得新增事实。

当前 provider 是百度百科 AppBuilder `get_content`，配置在“设置→网络搜索”，需要 API key。缺 key 或功能禁用时直接向用户说明，不要循环重试。不得重复搜同一 query，不得提及内部片段/开关实现，不用无关网页结果填充回答。

## 5.3 Runtime State

源：`src-tauri/src/runtime_state.rs:100-164`

每轮动态生成的中文译文：

> ## 运行时状态（本轮）
>
> 下面的区块是当前轮次的权威声明。如果它与本提示前面的章节或你对之前轮次的记忆冲突，请遵循本区块——前面的章节是静态文档，可能与当前模式或开关不一致。
>
> - **当前模式**：{中文模式名}（{工具层级描述}）
> - **功能开关**：无启用项。联网搜索关闭；严格 KB 关闭。
>   或：**已启用的功能开关**：{kb_strict / web_search}。
> - **写工具可用**：YES/NO。用户选择了上面的模式；若不确定本轮是否存在 write/edit/create 工具，答案是 yes/no。
> - **读工具可用**：Ask / Plan / Agent 中始终为 YES。

风险：`available_writes` 只根据 `Mode::Agent` 判断。如果 Agent 模式同时开启 `kb_strict`，实际写工具已被 filter 删除，但 Runtime State 仍会宣称“写工具 YES”；后面 inventory 又说“写工具不可用”。这是本轮 system prompt 内的直接冲突。

## 5.4 功能可用性清单

源：`src-tauri/src/feature_toggles.rs:112-159`

> 下列功能开关由用户控制。下面的状态就是本轮真正可用的状态，不要根据训练数据或之前轮次假设某工具存在。
>
> - `kb_strict` 开启：写工具不可用，只有只读搜索/检索；用户要求编辑时说明原本会怎么做，不调用写工具。
> - `kb_strict` 关闭：写工具可能存在，取决于 Ask/Plan/Agent 模式。
> - `web_search` 开启：工具列表包含 `web_search`，可用于现实世界事实问题。
> - `web_search` 关闭：工具列表不包含它，不得调用；无法从对话/工作区回答外部事实时，建议用户在输入栏开启联网搜索。

该清单与 Runtime State 和两个 Markdown 片段重复描述相同状态。

## 5.5 工作区路径注入

源：`src-tauri/src/commands_agent.rs:329-335`

> ## 当前工作区
> 工作区根目录是：{绝对路径}

绝对路径会暴露本机用户名、项目名和目录结构给云端 Provider。

## 5.6 通用代码内联补全

源：`src-tauri/src/inline_complete/mod.rs:341-379`

System：

> 你是文本补全助手。只能输出补全文本，除此之外什么都不要输出。

User prompt：

> 你是专业的 {language} 代码补全助手。
>
> 当前文件：{file_path}
>
> 用户按下 Tab 请求内联补全。光标位于下面 PREFIX 与 SUFFIX 之间。只输出要插在它们之间的文本。不得重复、改写或回显任何前缀内容。
>
> 规则：
> 1. 只输出要插入的新文本，不要前言、标签、Markdown 围栏或解释。
> 2. 精确匹配周围代码的风格和缩进。
> 3. 自然延续当前函数、代码块或语句。
> 4. 保持简洁，通常 1–5 行。
> 5. 不加解释性注释。
> 6. 绝不重复 PREFIX，即使它以列表标记结尾也不要重复标记。
> 7. 不输出光标标记，也不输出 SUFFIX 开头已经存在的闭合括号或标点。
>
> 随后以 `<|cursor_start|>PREFIX ... <|cursor_end|>` 和 SUFFIX 标记拼接真实内容，并要求立即输出 continuation。

补全温度被设为 0.3，但代码中创建了修改后的 config 后，实际 `adapter` 在修改之前已用原 config 构建；若 adapter 内部复制配置，则 0.3 可能并未生效。

## 5.7 DOCX 内联补全

源：`src-tauri/prompts/docx_complete.md:1-203` 与 `src-tauri/src/inline_complete/mod.rs:439-464`

你是专业文档补全助手，自然、结合上下文地补全 Word 内容。用户按 Tab，光标位于 PREFIX/SUFFIX 之间；只生成桥接二者的新文本，不得重复、改写或回显 PREFIX。

### 输出

只能返回合法 JSON：

```json
{
  "completion": "要插入的纯文本",
  "styles": [
    {"start_offset": 0, "end_offset": 5, "bold": true}
  ]
}
```

completion 通常 1–3 句/段；styles 使用相对 completion 的字符 offset，可指定 bold/italic/underline/strikethrough/color/highlight/font_size/font_family。

### 规则

1. 只输出新文本，连 PREFIX 尾部现有列表标记都不得重复。
2. 匹配周围风格、语气和语言。
3. 简短、完成当前逻辑单元。
4. 延续列表时不要重复已有 marker；新条目使用下一个编号。
5. completion 不含 Markdown，格式放在 styles。
6. 只输出 JSON，offset 合法且不重叠。
7. 不复制 SUFFIX 开头已有内容。

提示包含五组示例：英文句子、加粗片段、中文编号列表、红色强调、反重复。第五例先把正确的 `4.` 输出错误标为 WRONG，随后又自我纠正；这段“Wait —”会向模型展示冲突判断，建议删除。

文件加载失败时，fallback 提示只要求补全文本位置并输出 `{"completion":"...","styles":[]}`，再拼 cursor position、PREFIX 和 SUFFIX。

## 5.8 标记约定

代码和 DOCX 的实际 FIM prompt 都使用 `<|cursor_start|>` / `<|cursor_end|>`，而 snippet 构造内部另有 `<|cursor|>` 用于定位。内部 marker 在发给模型前会被拆成 prefix/suffix，因此不是直接冲突，但命名容易让维护者误判。
