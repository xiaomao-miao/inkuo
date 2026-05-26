# RAG：引用（Citations）与可回溯输出

## 1. 目标
- 所有“基于引用内容生成”的输出都应可回溯。
- 用户可查看引用来源与片段。

## 2. Citation 数据结构（建议）
- `source_path`
- `range`
- `snippet`
- `hash`

## 3. UI 行为
- 在 AI 回复或摘要卡片中显示引用标记（可 hover 查看来源）。
- 点击来源可打开文件并定位 range。

## 4. 约束
- citations 不得泄露用户密钥与敏感配置文件内容（可加入路径黑名单）。
