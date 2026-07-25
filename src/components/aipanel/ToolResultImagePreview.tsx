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
// This component is small on purpose: it just extracts the file path
// out of the result, builds an `asset://` URL via Tauri's
// `convertFileSrc`, and renders an `<img>` with a fixed max-height. We
// keep the click target on the whole chip so the user can re-open the
// image in the main editor pane.
//
// Split out of `ToolCallCard.tsx` so the card stays focused on layout;
// the chip can be unit-tested in isolation and reused later (e.g. in
// a "history" sidebar of recently generated images).

import React from 'react';
import { Image as ImageIcon, ExternalLink } from 'lucide-react';
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

/** Convert a workspace-relative or absolute path to a `tauri://localhost/...`
 * URL that the WebView can load. Mirrors what `ImageViewer` does; we
 * can't import from there without a circular dep, so the conversion is
 * duplicated (one line). When Tauri isn't available (e.g. plain Vite
 * dev preview outside the desktop app), the relative path is returned
 * as-is so the `<img>` fails gracefully. */
async function toAssetUrl(path: string): Promise<string> {
  if (typeof window === 'undefined') return path;
  const w = window as unknown as { __TAURI_INTERNALS__?: unknown };
  if (!w.__TAURI_INTERNALS__) return path;
  // convertFileSrc is provided by `@tauri-apps/api/core`. Dynamic import
  // keeps the desktop runtime as an optional dependency for browser
  // preview builds (where `__TAURI_INTERNALS__` is undefined).
  try {
    const mod = await import('@tauri-apps/api/core');
    return mod.convertFileSrc(path);
  } catch {
    return path;
  }
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

  React.useEffect(() => {
    let cancelled = false;
    if (!targetPath) {
      setSrc('');
      return;
    }
    toAssetUrl(targetPath).then((url) => {
      if (!cancelled) setSrc(url);
    });
    return () => {
      cancelled = true;
    };
  }, [targetPath]);

  if (!targetPath) return null;

  const fileName = targetPath.split(/[/\\]/).pop() ?? targetPath;
  const fullPath = workspacePath && !targetPath.startsWith('/')
    ? `${workspacePath}/${targetPath}`
    : targetPath;

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
