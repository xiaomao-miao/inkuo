# 本地核心（Rust Local Core）

本文档定义 Rust 本地核心的模块拆分、关键接口与实现约束。

## 1. 模块清单
1. Workspace & FS
2. Document Engine
3. Diff Engine
4. AI Proxy / Provider Adapters
5. Knowledge Base
6. Security（Keyring / 权限）
7. Session Log（AI 会话历史）

---

## 2. Workspace & FS

### 2.1 职责
- 打开/关闭工作区（单根或多根目录）。
- 文件读写（文本、二进制、docx/xlsx）。
- 变更监听（watch）用于索引更新与 UI 刷新。
- 备份与恢复：
  - docx/xlsx 保存前生成 `.bak`（可配置保留数量）
  - delete/rename 进入回收站或安全备份区

### 2.2 写入保护（必须）
- Edit/Agent 的多文件写入必须经过“预览 → Apply”。
- 对 delete/rename、批量改动、超阈值改动必须二次确认。
- 允许全局只读模式：禁止写盘，只生成 diff。

---

## 3. Document Engine

### 3.1 Markdown
- 解析：comrak（或等价 AST）
- 目标：
  - 块级边界识别（段落、标题、列表、表格、代码块）
  - 为 scope、diff、knowledge chunks 提供稳定 range

### 3.2 Word（docx）
- 内部表示：段落/标题/列表/表格 + 样式元数据
- 回写策略：保守映射 + 保存报告
- 失败策略：
  - 写回失败不得破坏原文件
  - 必须保留可恢复备份

### 3.3 Excel（xlsx）
- 内部表示：workbook/sheet/cell + 样式/公式引用
- 写回策略：尽量只写回变更单元格

---

## 4. Diff Engine

### 4.1 输入输出
- 输入：原文本、目标文本、scope range、（可选）块级结构信息
- 输出：
  - hunks（连续改动区间）
  - 每个 hunk 的统计信息（新增/删除字符数、行数）
  - 用于 UI 的映射（start/end offsets、锚点信息）

### 4.2 约束
- 输出必须稳定：同一对输入产生的 hunks 顺序与边界应一致。
- 大文本 diff 必须有性能保护（阈值、降级）。

---

## 5. AI Proxy 与 Provider Adapters

### 5.1 目标
- 统一：流式、取消、超时、重试、结构化输出校验。
- 统一协议：`summary/content/rules_applied`，无法满足时走降级。

### 5.2 Key 安全
- Provider 所需 key 由 Rust 从 keyring 读取。
- 前端不得拿到明文 key。

---

## 6. Knowledge Base
- 实现：`src-tauri/src/knowledge/*`
- 存储：本地 embedding + 向量存储（当前为 Qdrant Edge 嵌入式）
- chunk：基于块级边界（标题/段落/表格）
- 增量更新：基于文件 hash 与元数据
- 对外入口：`knowledge_build` / `knowledge_search` / `knowledge_update` / `knowledge_status` / `knowledge_clear`

---

## 7. Session Log
- 记录 AI 会话：instruction、scope、provider、模型、ChangeSet
- 目的：回滚、复用、审计（本地）
