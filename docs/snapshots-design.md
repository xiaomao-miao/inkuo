# inkuo 工作区快照与回滚 — 设计文档

版本：1.0
日期：2026-07-03
状态：已实现

---

## 一、目标

为 inkuo 增加**手动创建工作区快照 + 一键回滚 + AI 面板撤销联动**的能力，作为对 AI 自动编辑的事后兜底。

设计决策（已与产品对齐）：

| 决策项 | 选择 |
|---|---|
| 触发时机 | 手动 + AI 流开始时自动打 baseline |
| 回滚粒度 | 整个工作区一键回退（预览中可查看单文件变化） |
| 保留策略 | 配置上限（默认最近 50 份/工作区） |
| 存储方式 | 整文件副本（不做增量 diff，对 .docx/.pptx 同样安全） |
| 还原交互 | 预览 + 二次确认 |
| 与 AI 面板"重新输入"联动 | 是 |

## 二、磁盘布局

所有快照数据落在 `~/.inkuo/snapshots/` 下，按工作区路径的 SHA-256 前 16 字符分桶：

```
~/.inkuo/snapshots/
  └── {workspaceHash}/
      ├── index.json                  # 快照清单（元数据，不含文件内容）
      └── {snapshotId}/
          ├── manifest.json           # 该快照包含的文件、相对路径、大小、hash
          └── files/
              ├── {relPath1}          # 整文件副本（UTF-8 文本或二进制原样）
              ├── {relPath2}
              └── ...
```

**为什么用 `workspaceHash` 而不是路径原样**：工作区路径里可能有 `/`、空格、中文，作为目录名需要 URL-encode；用 hash 更干净，且**保证快照不会跨工作区串数据**。

### `index.json`

```json
{
  "version": 1,
  "workspacePath": "/home/user/proj",
  "snapshots": [
    {
      "id": "snap_2026-07-03T18-32-11",
      "createdAt": 1720000000000,
      "label": "AI 基线: 重写第三章",
      "fileCount": 7,
      "totalBytes": 48211,
      "trigger": "manual" | "ai_baseline"
    }
  ]
}
```

### `manifest.json`

```json
{
  "snapshotId": "snap_2026-07-03T18-32-11",
  "workspacePath": "/home/user/proj",
  "files": [
    {
      "relPath": "docs/chapter1.md",
      "absPath": "/home/user/proj/docs/chapter1.md",
      "size": 1234,
      "sha256": "abc123...",
      "isBinary": false
    }
  ]
}
```

`isBinary` 按扩展名（`.md` / `.txt` / `.json` / 代码类视为文本，其余视为二进制）判定，用于前端 UI 提示，不影响回滚字节级一致性。

## 三、后端模块划分

新文件 `src-tauri/src/snapshots.rs`：

```rust
// 路径解析
pub fn get_snapshots_root() -> PathBuf;
pub fn workspace_hash(workspace_path: &str) -> String;
pub fn snapshot_dir(workspace_path: &str, snapshot_id: &str) -> PathBuf;

// 快照核心
pub struct SnapshotManifest { /* 见上 */ }
pub struct SnapshotIndexEntry { /* 见上 */ }

pub fn create_workspace_snapshot(
    workspace_path: &str,
    label: Option<String>,
    trigger: &str,                 // "manual" | "ai_baseline"
    file_paths: Vec<(String, Vec<u8>)>, // (relPath, bytes)
) -> Result<SnapshotManifest, SnapshotError>;

pub fn list_workspace_snapshots(workspace_path: &str)
    -> Result<Vec<SnapshotIndexEntry>, SnapshotError>;

pub fn delete_workspace_snapshot(workspace_path: &str, snapshot_id: &str)
    -> Result<(), SnapshotError>;

pub fn preview_workspace_snapshot_restore(
    workspace_path: &str,
    snapshot_id: &str,
) -> Result<Vec<FileDiffPreview>, SnapshotError>;

pub fn restore_workspace_snapshot(
    workspace_path: &str,
    snapshot_id: &str,
    app_handle: &AppHandle,
) -> Result<Vec<String>, SnapshotError>;
```

### 新增 Tauri 命令（在 `commands.rs` 中定义并注册到 `lib.rs`）

| 命令名 | 用途 |
|---|---|
| `create_workspace_snapshot_cmd` | 创建快照 |
| `list_workspace_snapshots_cmd` | 列快照清单（按 createdAt 倒序） |
| `delete_workspace_snapshot_cmd` | 删一个 |
| `preview_workspace_snapshot_restore_cmd` | 还原前的预览 |
| `restore_workspace_snapshot_cmd` | 执行还原，返回被改/新增/删除的文件路径列表 |
| `collect_workspace_files_cmd` | 枚举工作区所有文件并返回 base64 字节（跳过 `node_modules` / `target` / `.git` / `dist` / `build` / `.next` / `.cache` / `.turbo` / `out`） |
| `read_file_bytes_cmd` | 读单个文件原始字节（base64） |
| `read_snapshot_file_cmd` | 读快照中单个文件的文本内容（用于 UI 预览 diff） |

### 错误类型扩展

`AppCommandError` 新增 5 个 variant：

- `SnapshotNotFound(String)`
- `SnapshotCorrupt(String)`
- `InvalidWorkspacePath(String)`
- `SnapshotWriteFailed(String)`
- `SnapshotReadFailed(String)`

### 关键不变量

- **原子写**：写 `manifest.json` 和每个文件副本都走 tmp + rename，保证中途崩溃不会留下损坏的快照。
- **不修改 index.json 中的旧条目**：仅 append + 按 LRU 裁剪尾部落选条目。
- **还原前自动备份**：每次 `restore_workspace_snapshot` 都会先把当前文件复制到 `~/.inkuo/backups/pre_restore_<timestamp>/`，与 `.bak` 机制独立，可用于事后追查。

## 四、前端模块划分

### `src/services/snapshots.ts`

薄封装，导出：

```typescript
export async function createSnapshot(workspacePath, label?, trigger?, files?): Promise<SnapshotManifest>;
export async function listSnapshots(workspacePath): Promise<SnapshotIndexEntry[]>;
export async function deleteSnapshot(workspacePath, id): Promise<void>;
export async function previewRestore(workspacePath, id): Promise<FileDiffPreview[]>;
export async function restoreSnapshot(workspacePath, id): Promise<string[]>;
export async function collectWorkspaceFiles(workspacePath): Promise<CollectFileResult[]>;
```

### `src/store/baselineStore.ts`

记录 user message id → baseline snapshot id 的映射：

```typescript
interface BaselineState {
  baselines: Record<string, string>;
  recordBaseline(userMessageId, snapshotId): void;
  consumeBaseline(userMessageId): string | undefined;  // 取并清
  peekBaseline(userMessageId): string | undefined;     // 只读
  clearBaseline(userMessageId): void;
  reset(): void;
}
```

使用 `persist` middleware，命名空间 `inkuo-baselines`，跨会话保留，避免在多轮重发中重复打基线。

### `src/components/snapshots/`

- **`SnapshotPanel.tsx`**：左侧 Sidebar 中的"快照"视图入口，列出快照、提供「创建 / 回滚 / 删除」按钮。
- **`SnapshotRestoreDialog.tsx`**：还原前的预览对话框，文本修改文件展示行级 diff（基于现有 `compute_diff` 命令），二进制文件展示大小变化。
- **`useSnapshotActions.ts`**：封装创建/删除/还原的副作用，通知、`confirmDialogStore` 集成。

### `src/components/settings/SnapshotsSettings.tsx`

设置面板新增"快照"标签页：
- 最大保留快照数（数字输入，默认 50）
- AI 流开始时自动创建 baseline（复选框，默认启用）

### `src/components/activitybar/ActivityBar.tsx` + `src/components/layout/Layout.tsx`

`ViewType` 联合类型新增 `'snapshots'`，Layout 根据 `activeView` 渲染 `<SnapshotPanel />`。

## 五、AI 面板联动

核心逻辑集中在 `src/components/aipanel/useChatSessionActions.ts`：

1. **AI 流开始时打 baseline**（`sendMessage`）：
   - 仅在 `isEditing === false`（即新消息，不是重发）且 `mode === 'agent'` 且 `settings.snapshot.autoBaseline === true` 时执行
   - 在 `invoke('ai_agent_stream', ...)` 之前调用 `collectWorkspaceFiles` + `createSnapshot`，把 snapshot id 存到 `useBaselineStore.baselines[userMessageId]`
   - 失败不阻塞流：仅 `console.warn` 记录

2. **AI 流正常完成时清 baseline**：
   - 监听 `ai://stream` 事件，当收到 `event_type === 'done'` 且 `message_id` 匹配时调用 `consumeBaseline(userMessageId)`
   - 收到 `event_type === 'error'` 时**保留** baseline，下次重新编辑可继续回滚

3. **重新编辑时还原 baseline**（`handleSaveEdit`）：
   - 在 `truncateMessagesAfter` 之前，先 `peekBaseline(editingMessageId)` → 拿到 id 后 `restoreSnapshot`
   - 文件被还原 → `file-change` 事件触发 → 编辑器自动重读
   - 还原失败不阻塞：仅 `pushNotification` 报错

## 六、Settings 扩展

`Settings` 接口新增：

```typescript
snapshot: {
  maxCount: number;     // 默认 50
  autoBaseline: boolean; // 默认 true
};
```

后端 `Settings` 结构体同步：

```rust
pub struct SnapshotSettings {
    pub max_count: usize,    // 默认 50
    pub auto_baseline: bool, // 默认 true
}
```

## 七、LRU 与清理

- **LRU 上限**：每次 `create_workspace_snapshot` 末尾调用 `enforce_snapshot_cap` 检查 `settings.snapshot.maxCount`，按 `createdAt` 升序裁剪最旧条目。
- **全局清理任务**：`init_snapshot_cleanup_task` 在 `lib.rs::setup` 阶段挂载，每 5 分钟扫描 `~/.inkuo/snapshots/` 下所有 `{wsHash}/`，删除 `index.json` 不再引用的孤儿目录（manifest 已删除但 files/ 还在的情况）。

## 八、验收清单（节选）

- [x] `cargo build` 通过
- [x] `npm run typecheck` 通过
- [x] 创建快照 + 改文件 + 还原 → 文件字节级恢复
- [x] AI agent 模式下发指令 → 编辑 user message 重发 → 文件回到指令前 + 消息流截断
- [x] AI 流报错/停止 → baseline 保留，下次重发仍能正确回滚
- [x] 跨工作区隔离：A 工作区快照不会出现在 B 工作区列表里

## 九、范围之外（明确不做）

- 不做 git 集成 / 真正的 commit graph（roadmap P3）
- 不做跨设备同步 / 云存储
- 不做快照导出/导入
- 不做 AI 流结束时的自动快照（只做"流开始时的 baseline"）
- 不做文件级细粒度回滚（粒度固定为整个工作区）
