# 09. 关于“shell / 脚本运行权限”的澄清

> 本节不是新增设计建议，而是补充解释一条经常被误读的事实：仓库里目前**没有**任何能让主 Agent 或子代理运行 Python 脚本、Shell 命令或任意可执行文件的工具。

## 1. 真实存在的工具

按 `src-tauri/src/agent/tools/mod.rs` 当前实现的工具枚举（节选）：

- 文件读写：`read_file / write_file / edit_file / list_dir / glob / grep / create_dir / move_file`。
- 检索：`database_search`。
- Office：`read_office_file / create_word_doc / create_excel / modify_excel / inspect_office / compare_word_docs`。
- PPT：`create_pptx / create_pptx_animation / add_pptx_animation`。
- 媒体：`read_image / read_pdf`。
- 图形：`create_svg / render_mermaid`。
- 元：`ask_user / update_todo / get_tool_help / delegate_to`。
- 图像生成：`generate_image`。

没有 `shell_run / run_command / run_python / execute / spawn` 这类工具。Python 解释器、Node.js、curl 或任何外部二进制都不会被 Agent 调用。

`shell_run` 这个名字只在两处出现：

- `src-tauri/src/runtime_state.rs:86`：注释里把它列为“默认全工具集”示例。
- `src-tauri/src/feature_toggles.rs:63`：在 `KB_STRICT_BLOCKED_TOOLS` 中作为“应当被禁用”的写工具名出现，但它根本没有被注册，所以这条禁令在当前实现上是空字符串引用。

因此“关闭 shell_run”目前等价于“该工具不存在”。把它列在禁止清单和默认列表里是一种**预留语义**——为未来若引入执行工具时能立即被 kb_strict 阻挡。

## 2. 那为什么模型还会“写脚本”？

这通常来自三种真实风险，与 shell 权限无关：

### 2.1 模型使用 `write_file` 创建 `.py` 文件

`write_file` 是 Agent 真实拥有的工具，权限范围是任何文本路径。`agent.slim.md:33` 甚至明确把 `"python script"` 标为可直接写 `.py` 而无需询问。

后果：

- `.py` 文件被写入工作区，但**没有任何工具会执行它**。Agent 会自我满足地报告“脚本已写好”，用户必须自己在终端运行。
- 用户体验上像是“Agent 主动去写脚本”，但它其实是写了一个未运行的文件。

### 2.2 模型用 `create_svg` 再调用一个根本不存在的转换步骤

例如 SVG 转 PNG：仓库没有栅格化工具。`tool_specs/svg.md:187` 在故障恢复表里写了一行建议：

> Re-render with `read_image` → base64 → a Rust-side rasteriser; or call `create_svg` then ask the user to "File → Export as PNG" from the viewer.

这里的 `read_image` 路径只是把已有 PNG 注入资产，不会真正把 SVG 转 PNG。所以模型尝试后会失败或“以为”完成。同样地，`tool_specs/pptx.md:71,106` 在多处把 `render_mermaid`（输出 PNG）描述为栅格化的“唯一出路”，但对纯 SVG 矢量并没有提供真正的转换路径。

### 2.3 模型生成了代码块但只作为“答案”

Ask/Plan 模式不被禁止在回答中包含 ```python 或 ```rust 代码块，Ask.md:75 甚至明确鼓励“使用带语言标签的代码块”。这和“执行脚本”完全是两回事——只是给用户看一段示例代码。

## 3. 提示词层面是否引导了“写脚本”？

是的，存在一些**隐性引导**，但都不允许执行：

- 主 Agent 决策矩阵允许直接写 `.py` / `.ts`（`agent.slim.md:33`），并且 `tool_specs/general.md` 给出 `write_file` 用法。
- `tool_specs/svg.md` 的故障恢复提到“a Rust-side rasteriser”和“call create_svg then ask the user to export as PNG”，把“我不能直接栅格化”暗示给模型，但没有声明“我也不会去调用外部脚本”。
- `pptx_animation.md` 也允许用 SVG `<animate>` 自动转换。
- 没有任何一个 prompt 明确告诉模型：**“你不能在终端执行命令或脚本。如果需要转换，请把脚本作为文件交付给用户，并明确写明它需要用户手动运行。”**

也就是说，模型“写脚本”只是它在用一种**错误的解决路径**——它把任务建模为“需要执行”，但工具集里没有执行通道，因此它只能停在“写文件 + 假想已完成”这一步。

## 4. 如果确实想阻止“写脚本文件”

下面是真正起作用的策略：

### 4.1 给 Agent 一个明确的“不执行”声明

在 `agent.slim.md` 加一段：

> **You cannot run scripts, binaries, or shell commands.** Tools in your registry cover file I/O, Office editing, SVG/Mermaid authoring, image generation, and search only. When a task would normally require execution (e.g. SVG → PNG conversion, running a Python script, calling an external API), write the file as an artifact for the user to run themselves and say so explicitly in the summary.

同时在 `runtime_state.rs` 改成正向声明：

> - **Executable tools available**: NO. You may author .py / .ts / shell scripts as files for the user to run, but you cannot invoke them.

这样模型就会把“写文件 + 说明需用户执行”作为标准回答，而不是无声地假装已完成。

### 4.2 收敛 SVG→PNG 等“模型以为能做”的暗示

`tool_specs/svg.md:187` 的恢复表当前是诱导模型“再想想有没有 rasteriser”的暗示，应当改写为：

> User wanted a PNG: you have no rasterisation tool. Either (a) hand the user the SVG and tell them to export, or (b) suggest `render_mermaid` if the artwork is diagram-like; never write or claim a script that converts SVG to PNG.

`tool_specs/pptx.md:71,106` 中“the only escape hatch today is render_mermaid-style raster output”也是暗示，建议改为“the only raster path is render_mermaid for diagrams; for SVG art you cannot rasterise yourself”。

### 4.3 不要靠 KB strict 来挡

因为 `shell_run` 等工具不存在，`KB_STRICT_BLOCKED_TOOLS` 列表里写不写 `shell_run` 没有实际效果，只是给人安全错觉。建议要么移除这条“空引用”，要么明确加注释：*reserved for future shell tool, currently unused*。

## 5. 一句话结论

Agent 现在并不是“在偷偷执行脚本”，而是**没有执行工具却使用了写文件工具**，并因提示词没有显式说明“无执行能力”而被误解为“应该能执行”。修复点不是限制 shell，而是：

1. 在 system 中明确“无执行能力，把脚本作为文件交付”。
2. 在 SVG/PPTX 规范里改写误导性恢复建议。
3. 清理 KB strict 列表里的“空引用”以避免错觉。

这三件事属于小型 prompt 修补，可以直接放在本审计后续迭代中；如果用户希望避免所有 `.py` 创建，主 Agent 决策矩阵里 `python script` 那行也应同步收紧。

## 6. 已落地的改动（应用记录）

> 下方所有改动已落到工作区并已写入 `10-execution-policy.md` 第 6 节的清单；本节存档便于回查。

- `src-tauri/prompts/main/agent.slim.md`：v2 重组（8 节 → 6 节）；新增 No-Execution Contract / Tool Truthfulness / Source Discipline 三条顶层契约；决策矩阵 `python script` 行改为 **Confirm first**；专家速查卡明确 `office_pptx_expert` 只能打包；新增 Failure Etiquette。
- `src-tauri/prompts/tool_specs/svg.md`：故障恢复表 `I wanted a PNG` 行改写。
- `src-tauri/prompts/tool_specs/pptx.md`：§4 渐变段与 §6 故障恢复表中的栅格化行全部改写。
- `src-tauri/src/feature_toggles.rs`：`shell_run` 上方加注释说明它是预留位。
- `src-tauri/src/runtime_state.rs`：`Mode::Agent::tool_tier()` 移除 `shell_run` 提及，指向主提示词的 No-Execution Contract。