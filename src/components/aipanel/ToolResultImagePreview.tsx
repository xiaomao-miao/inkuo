// Post-stream preview chip for the `generate_image` tool.
//
// When the tool finishes successfully, the result string is the JSON
// payload emitted by the Rust side:
//
//   {"status":"ok","file_path":"...","prompt":"...","width":1024,...}
//
// The Rust side also stamps `ToolResult.file_path` so the registry can
// trigger a file-change event; the chat panel can then resolve the
// path through the same image-viewer pipeline used by the rest of the
// app (see `ImageViewer.tsx`).
//
// This component is small on purpose: it extracts the file path out of
// the result, asks Rust for the base64 bytes via `read_file_for_viewer`,
// and renders a `<img>` with a fixed max-height. We keep the click
// target on the whole chip so the user can re-open the image in the
// main editor pane.
//
// Why not `convertFileSrc` (the obvious choice for local files)? Tauri
// 2 only serves the `asset://` protocol to paths explicitly listed in
// `tauri.conf.json#app.security.assetProtocol.scope`; we never opted
// the whole workspace into that scope, so `convertFileSrc` returns a
// URL that the WebView refuses to load. Going through the same
// `read_file_for_viewer` command as `ImageViewer` keeps the preview
// pipeline consistent and avoids a new capability surface.
//
// Split out of `ToolCallCard.tsx` so the card stays focused on layout;
// the chip can be unit-tested in isolation and reused later (e.g. in
// a "history" sidebar of recently generated images).

import React from 'react';
import { invoke } from '@tauri-apps/api/core';
import { Image as ImageIcon, ExternalLink } from 'lucide-react';
import type { ViewerFilePayload } from '../../types';
import toolPreviewStyles from './ToolResultPreview.module.css';

interface ToolResultImagePreviewProps {
  /** Raw tool_result output (the JSON string from Rust). */
  result?: string;
  /** Optional override for the file path; takes priority over parsing
   *  `result`. The frontend stores this separately so the renderer can
   *  show the preview even before the streamed JSON is fully parsed. */
  filePath?: string;
  /** Workspace root for resolving relative file paths. */
  workspacePath?: string;
  /** Click handler — wired up by the parent so clicking the chip opens
   *  the image in the editor pane. */
  onFileClick?: (path: string) => void;
}

/** Try to pull `file_path` out of a result JSON string without going
 * through a full JSON parse (the result is usually tiny, so we lean on
 * a regex; if it fails we just return null and let the caller fall back
 * to the explicit `filePath` prop). */
function extractFilePathFromResult(result: string | undefined): string | null {
  if (!result) return null;
  const match = result.match(/"file_path"\s*:\s*"([^"]+)"/);
  return match ? match[1] : null;
}

/** Resolve a workspace-relative path against the active workspace so
 * Rust can find the file. The fallback when no workspace is known is
 * to trust the path as-is (absolute paths survive unchanged). */
function resolveAbsolutePath(path: string, workspacePath?: string): string {
  if (!workspacePath || path.startsWith('/') || /^[a-zA-Z]:[\\/]/.test(path)) {
    return path;
  }
  return `${workspacePath}/${path}`;
}

export const ToolResultImagePreview: React.FC<ToolResultImagePreviewProps> = ({
  result,
  filePath,
  workspacePath,
  onFileClick,
}) => {
  const parsed = extractFilePathFromResult(result);
  const targetPath = filePath ?? parsed;

  const [src, setSrc] = React.useState<string>('');
  const [loadError, setLoadError] = React.useState<string | null>(null);

  React.useEffect(() => {
    let cancelled = false;
    setSrc('');
    setLoadError(null);
    if (!targetPath) return;

    const absolutePath = resolveAbsolutePath(targetPath, workspacePath);
    invoke<ViewerFilePayload>('read_file_for_viewer', { path: absolutePath })
      .then((payload) => {
        if (cancelled) return;
        setSrc(`data:${payload.mime};base64,${payload.data_base64}`);
      })
      .catch((err: unknown) => {
        if (cancelled) return;
        setLoadError(String(err));
      });

    return () => {
      cancelled = true;
    };
  }, [targetPath, workspacePath]);

  if (!targetPath) return null;

  const fileName = targetPath.split(/[/\\]/).pop() ?? targetPath;
  const fullPath = resolveAbsolutePath(targetPath, workspacePath);

  return (
    <div className={toolPreviewStyles.previewBlock}>
      <div className={toolPreviewStyles.previewHeader}>
        <ImageIcon size={12} />
        <span>生成结果</span>
        <span className={toolPreviewStyles.previewFileName}>{fileName}</span>
      </div>
      <button
        type="button"
        className={toolPreviewStyles.previewImageButton}
        onClick={() => onFileClick?.(fullPath)}
        title="点击打开图片"
      >
        {src && (
          <img
            src={src}
            alt={fileName}
            className={toolPreviewStyles.previewImage}
            draggable={false}
            loading="lazy"
          />
        )}
        {!src && !loadError && (
          <span className={toolPreviewStyles.previewPlaceholder}>加载中…</span>
        )}
        {loadError && (
          <span className={toolPreviewStyles.previewPlaceholder}>
            加载失败：{loadError}
          </span>
        )}
        <span className={toolPreviewStyles.previewOverlay}>
          <ExternalLink size={12} />
          <span>打开</span>
        </span>
      </button>
    </div>
  );
};

/** Convenience predicate: does this tool have an image-preview chip? Used
 * by `ToolCallCard` to decide whether to render `<ToolResultImagePreview />`
 * after the standard preview section. */
export function hasImageResultPreview(
  toolName: string,
  status: 'pending' | 'executing' | 'success' | 'error'
): boolean {
  return toolName === 'generate_image' && status === 'success';
}

// Re-export the CSS module type so callers can compose styles without
// importing the module directly (keeps the public surface tight).
export type { default as PreviewStyles } from './ToolResultPreview.module.css';