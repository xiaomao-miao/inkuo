# 数据模型（SQLite / 向量索引 / 会话）

本文档为实现提供概念级数据模型与关键字段建议。

## 1. SQLite 数据库
- 用途：
  - 文件元数据、索引状态
  - RAG chunks 与 embedding
  - AI 会话与 ChangeSet 历史

## 2. 表设计（建议）

### 2.1 documents
- `id` (pk)
- `path` (unique)
- `type` (`md`/`docx`/`xlsx`/`txt`/`pdf`…)
- `title`
- `hash`
- `updated_at`

### 2.2 blocks
- `id` (pk)
- `doc_id` (fk)
- `kind` (`paragraph`/`heading`/`list`/`table`/`codeblock`…)
- `start_offset` / `end_offset`（或行列 range）
- `text`（用于索引；必要时截断）
- `metadata_json`

### 2.3 embedding_chunks
- `id` (pk)
- `doc_id` (fk)
- `block_id` (nullable)
- `range_json`
- `text`
- `embedding`（vec）
- `hash`
- `updated_at`

### 2.4 ai_sessions
- `id` (pk)
- `doc_id` (nullable：workspace 级可为空)
- `scope` (`selection`/`file`/`workspace`/`pick`)
- `instruction`
- `provider` / `model`
- `created_at`

### 2.5 ai_changesets
- `id` (pk)
- `session_id` (fk)
- `summary`
- `risk` (`low`/`medium`/`high`)
- `files_json`
- `patches_json`
- `diff_view_json`
- `created_at`

## 3. 引用（Citations）
- citations 建议作为结构化 JSON 伴随输出与写入会话：
  - `source_path`
  - `range`
  - `snippet`
  - `hash`

## 4. 约束
- 所有 JSON 字段必须可 schema 校验（版本化）。
- 大文本不可直接入库（截断 + 指针）。
