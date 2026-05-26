# 安全：Workspace 写入保护（Write Guard）

## 1. 目标
- 防止 Edit/Agent 在工作区范围产生不可控修改。
- 保证所有写入：可预览、可审查、可回滚。

## 2. 基本原则
- 默认：先预览后应用（preview-first）。
- delete/rename：强制二次确认 + 可恢复。

## 3. 风险等级（建议）
- low：少量行级修改、单文件
- medium：多文件修改、结构性重排
- high：delete/rename、批量替换、大范围改动

## 4. 强制确认条件（MUST）
- 影响文件数 > N
- 任一文件 patch 超过阈值（行数/字节）
- action 包含 delete/rename
- 涉及敏感路径（例如 `.env`、密钥文件、系统目录）

## 5. 只读模式
- 开启后：允许 Plan 与 ChangeSet 预览；禁止 apply。

## 6. 恢复机制
- 删除进入回收站或备份区。
- 保存前备份（docx/xlsx）。
- 提供“最近应用的 ChangeSet”列表与一键回滚。
