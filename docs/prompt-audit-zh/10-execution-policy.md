# 10. 执行策略备忘：先禁止写脚本，待工具集齐后再分级开放

> 本节记录与产品方达成的共识，作为后续 prompt 修补和新工具引入的判定依据。

## 1. 决定

**现阶段（v0.x）一律禁止主 Agent / 子代理写可执行脚本或调用 shell。** 包括 `.py / .ts / .js / .sh / .bat / .ps1` 等一切可执行后缀。

**何时允许重新评估：** 当工具集已经覆盖了 Agent 90% 以上的真实诉求（即 SVG→PNG 转换、批量重命名、表格预览、外部数据拉取等都有专用工具），再考虑开放一个**受控沙盒执行通道**。

## 2. 当前允许的例外

仅以下两类允许与“脚本”相关：

1. **作为示例/答复的代码块**：`Ask / Plan` 模式回答里允许 ` ```python ` 等带语言标签的代码块，仅用于解释方案，不写入工作区。
2. **作为占位的标记文件**：允许写入 `script.py` 等文件，但仅在用户**明确要求**“把这个脚本写出来给我/给我团队手动跑”时发生，并需在结果中说明“需用户手动运行”。

## 3. 决策规则

```
如果任务 X 没法用现有工具完成：
  ├─ 若是缺工具 → 在 doc/10 中登记缺口；不要写脚本。
  ├─ 若是用户明确要求脚本 → 允许写文件，标“需手动运行”。
  └─ 若是 Ask/Plan 解释 → 用代码块即可，不要写文件。
```

判断口诀：

- **能用工具做的，绝不写脚本**。
- **没有工具但能写文件的，仅作占位交付**。
- **没有工具也不允许写文件的，明确告诉用户人工处理**。

## 4. 待登记的工具缺口

> 这里是“什么时候再加沙盒”的判定依据。每补一项打勾/打叉。

- [ ] SVG → PNG / JPG 栅格化（最常见的真实诉求）
- [ ] SVG → PDF 栅格化
- [ ] CSV / Excel 预览与聚合统计
- [ ] 批量文件重命名 / 重命名规则脚本
- [ ] Markdown → DOCX / PDF
- [ ] 媒体文件（PNG/JPG）尺寸调整 / 转码
- [ ] 抓取并归纳一组 URL 内容
- [ ] 跑回归测试 / 类型检查并报告
- [ ] 跨项目 glob 替换 + 校验
- [ ] 在沙盒里拉一次 GitHub Issue/PR 数据

当以上 ≥ 7 项被专用工具覆盖，且工具调用成功率 ≥ 95% 时，重新评估沙盒。

## 5. 沙盒开放后的最低要求（前置条件）

如果未来真要开沙盒，下面这些必须先就位，缺一不可：

1. **技能化注册**：禁止 `run_python` / `shell_run` 这种通用入口；只能通过预定义的 `skill://` 调用，每个技能是已评审的 Rust/脚本封装。
2. **网络默认关闭、白名单 IO**：仅允许工作区子目录 + 显式临时目录。
3. **超时与大小硬限**：单次超时、总超时、stdout/stderr 单条上限、累计上限。
4. **执行历史隔离**：stdout/stderr 不进入对话历史，仅以摘要形式注入；保留可下载的运行日志。
5. **提示词中说明触发条件**：在 system prompt 显式说明“只有当 X 工具不可用且用户已确认时，才调用 run_skill”。
6. **审计日志**：每次执行写入可被审计的结构化日志（技能名、参数哈希、耗时、退出码、资源用量）。

## 3. 与现有 prompt 改动挂钩

下面这三处已正式落地（v0.x 禁脚本策略）：

- ✅ `src-tauri/prompts/main/agent.slim.md`：v2 重组（226 → 231 行，8 节 → 6 节）。新增三个**顶层契约**：No-Execution Contract / Tool Truthfulness / Source Discipline；处理请求改为显式六步循环（READ → CLASSIFY → RESOLVE → PLAN → EXECUTE → SUMMARIZE）；专家速查卡明确 `office_pptx_expert` **只能打包 SVG，不能原地编辑**；新增 §4.4 Failure Etiquette；反对模式由 8 条压缩到 6 条。Tier 1 / Tier 2 表合并重复，决策矩阵聚焦“必须先问”的场景。
- ✅ `src-tauri/prompts/tool_specs/svg.md`：故障恢复表中的 "I wanted a PNG" 行已改写，**禁止**暗示 Agent 能栅格化，并明确告知“don't write a Python script to convert”。
- ✅ `src-tauri/prompts/tool_specs/pptx.md`：§4 渐变段落、`§6` “User wanted rasterised images for fidelity” 行均改写，明确告知“无栅格化工具”。
- ✅ `src-tauri/src/commands_agent.rs`：**根本性修复**。`Mode::Agent` 的 session 初始化原来用 `get_full_tool_registry → registry.tool_names()` 作为 `effective_tool_set` 的 base（包含所有 Office 工具），模型能猜到并成功调用 `create_word_doc` 等 Tier 2 工具。修复：导入 `resolve_profile`；`Mode::Agent` 分支改用 `resolve_profile("main", None)` 获取 profile 的 `allowed_tools`（14 个 Tier 1 工具）作为 base；`effective_tool_set` 的 base 改为 `profile_base_tools.as_deref().unwrap_or(&registry.tool_names())`，Agent 走 profile Ask/Plan 走全量只读注册表。

执行策略备忘详见 `10-execution-policy.md`。

## 7. 一句话定调

> **先让 Agent 把手里的工具用对，再用工具而不是脚本解决问题；等到工具真的不够用，再考虑用受控沙盒补齐，且沙盒本身也要先治理好。**