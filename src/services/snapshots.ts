/**
 * Workspace file-content snapshot service.
 *
 * Thin wrapper around the Tauri commands exposed by `src-tauri/src/snapshots.rs`.
 * Used by the SnapshotPanel UI and by the AI panel's "re-edit" flow to roll
 * back a workspace to a known-good state.
 *
 * The base64 dance is needed because Tauri serialises command arguments as
 * JSON, which is not binary-safe.
 *
 * NOTE: the Rust side uses snake_case for all command arguments.  We map
 * from camelCase here so callers can stay idiomatic.
 */

import { invoke } from '@tauri-apps/api/core';

export type SnapshotTrigger = 'manual' | 'ai_baseline';
export type ChangeKind = 'added' | 'modified' | 'deleted' | 'unchanged';

/** Raw shape returned by Rust — snake_case fields. */
interface RustSnapshotIndexEntry {
  id: string;
  created_at: number;
  label: string | null;
  file_count: number;
  total_bytes: number;
  trigger: string;
}

interface RustSnapshotManifestFile {
  rel_path: string;
  abs_path: string;
  size: number;
  sha256: string;
  is_binary: boolean;
}

interface RustSnapshotManifest {
  snapshot_id: string;
  workspace_path: string;
  label: string | null;
  trigger: string;
  created_at: number;
  files: RustSnapshotManifestFile[];
}

interface RustFileDiffPreview {
  rel_path: string;
  abs_path: string;
  change_kind: ChangeKind;
  is_binary: boolean;
  snapshot_bytes: number;
  disk_bytes_now: number;
}

/** Public camelCase view used by the React UI. */
export interface SnapshotIndexEntry {
  id: string;
  createdAt: number;
  label: string | null;
  fileCount: number;
  totalBytes: number;
  trigger: SnapshotTrigger;
}

export interface SnapshotManifestFile {
  relPath: string;
  absPath: string;
  size: number;
  sha256: string;
  isBinary: boolean;
}

export interface SnapshotManifest {
  snapshotId: string;
  workspacePath: string;
  label: string | null;
  trigger: SnapshotTrigger;
  createdAt: number;
  files: SnapshotManifestFile[];
}

export interface FileDiffPreview {
  relPath: string;
  absPath: string;
  changeKind: ChangeKind;
  isBinary: boolean;
  snapshotBytes: number;
  diskBytesNow: number;
}

function mapIndexEntry(raw: RustSnapshotIndexEntry): SnapshotIndexEntry {
  return {
    id: raw.id,
    createdAt: raw.created_at,
    label: raw.label ?? null,
    fileCount: raw.file_count,
    totalBytes: raw.total_bytes,
    trigger: (raw.trigger === 'ai_baseline' ? 'ai_baseline' : 'manual'),
  };
}

function mapManifest(raw: RustSnapshotManifest): SnapshotManifest {
  return {
    snapshotId: raw.snapshot_id,
    workspacePath: raw.workspace_path,
    label: raw.label ?? null,
    trigger: raw.trigger === 'ai_baseline' ? 'ai_baseline' : 'manual',
    createdAt: raw.created_at,
    files: raw.files.map((f) => ({
      relPath: f.rel_path,
      absPath: f.abs_path,
      size: f.size,
      sha256: f.sha256,
      isBinary: f.is_binary,
    })),
  };
}

function mapPreview(raw: RustFileDiffPreview): FileDiffPreview {
  return {
    relPath: raw.rel_path,
    absPath: raw.abs_path,
    // Rust serialises enums as PascalCase ("Modified"), normalise to lowercase
    // so the TS-side discriminated union and counter bucket keys match.
    changeKind: raw.change_kind.toLowerCase() as ChangeKind,
    isBinary: raw.is_binary,
    snapshotBytes: raw.snapshot_bytes,
    diskBytesNow: raw.disk_bytes_now,
  };
}

/** Convert a Uint8Array to base64 in a browser-safe way. */
export function bytesToBase64(bytes: Uint8Array): string {
  let binary = '';
  const chunkSize = 0x8000;
  for (let i = 0; i < bytes.length; i += chunkSize) {
    const chunk = bytes.subarray(i, i + chunkSize);
    binary += String.fromCharCode.apply(null, Array.from(chunk));
  }
  return btoa(binary);
}

interface RustCreateSnapshotArgs {
  workspace_path: string;
  label: string | null;
  trigger: string;
  files: Array<{ rel_path: string; content_base64: string }>;
}

export async function createSnapshot(
  workspacePath: string,
  label?: string | null,
  trigger: SnapshotTrigger = 'manual',
  files: Array<{ relPath: string; contentBase64: string }> = []
): Promise<SnapshotManifest> {
  const args: RustCreateSnapshotArgs = {
    workspace_path: workspacePath,
    label: label ?? null,
    trigger,
    files: files.map((f) => ({
      rel_path: f.relPath,
      content_base64: f.contentBase64,
    })),
  };
  const raw = await invoke<RustSnapshotManifest>('create_workspace_snapshot_cmd', {
    args,
  });
  return mapManifest(raw);
}

export async function listSnapshots(workspacePath: string): Promise<SnapshotIndexEntry[]> {
  const raw = await invoke<RustSnapshotIndexEntry[]>('list_workspace_snapshots_cmd', {
    workspacePath,
  });
  return raw.map(mapIndexEntry);
}

export async function deleteSnapshot(workspacePath: string, id: string): Promise<void> {
  await invoke('delete_workspace_snapshot_cmd', {
    workspacePath,
    snapshotId: id,
  });
}

export async function previewRestore(
  workspacePath: string,
  id: string
): Promise<FileDiffPreview[]> {
  const raw = await invoke<RustFileDiffPreview[]>('preview_workspace_snapshot_restore_cmd', {
    workspacePath,
    snapshotId: id,
  });
  return raw.map(mapPreview);
}

export async function restoreSnapshot(
  workspacePath: string,
  id: string
): Promise<string[]> {
  return invoke<string[]>('restore_workspace_snapshot_cmd', {
    workspacePath,
    snapshotId: id,
  });
}

export interface CollectFileResult {
  relPath: string;
  contentBase64: string;
}

/**
 * Enumerate every file under `workspacePath` and return its raw bytes
 * (base64-encoded).  The backend skips heavy/derived directories like
 * `node_modules`, `target`, etc.
 */
export async function collectWorkspaceFiles(
  workspacePath: string
): Promise<CollectFileResult[]> {
  interface RustFile {
    rel_path: string;
    content_base64: string;
  }
  const files = await invoke<RustFile[]>('collect_workspace_files_cmd', {
    workspacePath,
  });
  return files.map((f) => ({
    relPath: f.rel_path,
    contentBase64: f.content_base64,
  }));
}
