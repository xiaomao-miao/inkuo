import { useEffect, useMemo, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { Loader2, AlertCircle, ZoomIn, ZoomOut, RotateCcw, Maximize2 } from 'lucide-react';
import type { ViewerFilePayload } from '../../types';
import sharedStyles from './ImageViewer.module.css';
import styles from './SvgViewer.module.css';

interface SvgViewerProps {
  filePath: string;
}

/**
 * SVG-aware image viewer. The Rust-side `read_file_for_viewer` already
 * base64-encodes the raw file bytes with `image/svg+xml` MIME, so we render
 * the SVG as a normal `<img>` (browser handles scaling + pan-zoom via CSS
 * transforms). Two extras vs. the raster ImageViewer:
 *
 *   1. **Checkerboard background.** SVG can have transparent regions; the
 *      checker makes them obvious so the user can tell "this is empty"
 *      from "this is white but I forgot to set a fill".
 *   2. **"Fit to viewport"** — the raster viewer only does free-form zoom
 *      in/out, which is annoying for SVGs that arrived in unknown viewBox
 *      sizes. We compute a fit-to-screen scale at load time and reset to
 *      it on demand.
 *
 * If we ever need to support interactive editing (drag handles, element
 * picking), this is the seam to extend — but YAGNI today, a static preview
 * covers 100% of the AI-generated SVG use case.
 */
export const SvgViewer: React.FC<SvgViewerProps> = ({ filePath }) => {
  const [payload, setPayload] = useState<ViewerFilePayload | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [zoom, setZoom] = useState(1);
  const [rotation, setRotation] = useState(0);
  /** Bounding box of the inner `<svg>` content. `null` until we successfully
   *  parse the file. Lets us reset the zoom to "fit" instead of 100%. */
  const [intrinsic, setIntrinsic] = useState<{ w: number; h: number } | null>(null);

  const fileName = useMemo(() => filePath.split(/[\\/]/).pop() ?? filePath, [filePath]);

  const dataUrl = useMemo(() => {
    if (!payload) return '';
    return `data:${payload.mime};base64,${payload.data_base64}`;
  }, [payload]);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    setError(null);
    setPayload(null);
    setZoom(1);
    setRotation(0);
    setIntrinsic(null);

    invoke<ViewerFilePayload>('read_file_for_viewer', { path: filePath })
      .then((data) => {
        if (cancelled) return;
        setPayload(data);
        // Pull the raw SVG text and parse its viewBox / width / height so
        // we can fit-to-screen on reset. Doing this client-side avoids a
        // round-trip to Rust for what is a 4-attribute regex.
        if (data.mime === 'image/svg+xml') {
          try {
            const decoded = atob(data.data_base64);
            const dims = parseSvgDimensions(decoded);
            if (dims) setIntrinsic(dims);
          } catch {
            /* leave intrinsic null → fall back to 100% reset */
          }
        }
      })
      .catch((err: unknown) => {
        if (!cancelled) setError(String(err));
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });

    return () => {
      cancelled = true;
    };
  }, [filePath]);

  const handleReset = () => {
    setZoom(1);
    setRotation(0);
  };

  const handleFit = () => {
    if (!intrinsic) {
      handleReset();
      return;
    }
    // 800px target width is a reasonable default that fits most 400–1200
    // unit SVGs into a 1200×800 editor pane without further zoom. We clamp
    // to 2× so a 24×24 icon doesn't blow up to 100% screen width on click.
    const targetW = 800;
    const scale = Math.min(2, targetW / intrinsic.w);
    setZoom(scale);
    setRotation(0);
  };

  if (loading) {
    return (
      <div className={sharedStyles.center}>
        <Loader2 size={20} className={sharedStyles.spin} />
        <span>正在加载 SVG…</span>
      </div>
    );
  }

  if (error || !payload) {
    return (
      <div className={sharedStyles.center}>
        <AlertCircle size={20} />
        <span>无法加载 SVG{error ? `：${error}` : ''}</span>
      </div>
    );
  }

  return (
    <div className={sharedStyles.container}>
      <div className={sharedStyles.toolbar}>
        <span className={sharedStyles.fileName} title={fileName}>{fileName}</span>
        <span className={sharedStyles.sizeLabel}>{formatBytes(payload.size)}</span>
        {intrinsic && (
          <span className={sharedStyles.sizeLabel}>
            {intrinsic.w}×{intrinsic.h}
          </span>
        )}
        <div className={sharedStyles.spacer} />
        <button
          className={sharedStyles.toolButton}
          onClick={() => setZoom((z) => Math.max(0.05, z - 0.1))}
          title="缩小"
        >
          <ZoomOut size={14} />
        </button>
        <span className={sharedStyles.zoomLabel}>{Math.round(zoom * 100)}%</span>
        <button
          className={sharedStyles.toolButton}
          onClick={() => setZoom((z) => Math.min(20, z + 0.1))}
          title="放大"
        >
          <ZoomIn size={14} />
        </button>
        <button
          className={sharedStyles.toolButton}
          onClick={handleFit}
          title="适应窗口"
        >
          <Maximize2 size={14} />
        </button>
        <button
          className={sharedStyles.toolButton}
          onClick={() => setRotation((r) => (r + 90) % 360)}
          title="旋转 90°"
        >
          <RotateCcw size={14} />
        </button>
        <button
          className={sharedStyles.toolButton}
          onClick={handleReset}
          title="重置"
        >
          重置
        </button>
      </div>

      <div className={styles.viewport} data-svg-checker="true">
        <img
          src={dataUrl}
          alt={fileName}
          className={styles.image}
          style={{
            transform: `scale(${zoom}) rotate(${rotation}deg)`,
          }}
          draggable={false}
        />
      </div>
    </div>
  );
};

/**
 * Pull `width` / `height` / `viewBox` from a raw SVG string so the viewer
 * can display the intrinsic dimensions and pick a sensible fit-to-screen
 * scale. Best-effort — returns `null` on anything we can't parse.
 */
function parseSvgDimensions(svg: string): { w: number; h: number } | null {
  const rootMatch = svg.match(/<svg\b[^>]*>/i);
  if (!rootMatch) return null;
  const root = rootMatch[0];

  // Try viewBox first ("x y w h"), then width/height attrs.
  const viewBoxMatch = root.match(/viewBox\s*=\s*"([^"]+)"/i);
  if (viewBoxMatch) {
    const parts = viewBoxMatch[1]
      .split(/[\s,]+/)
      .map((p) => p.trim())
      .filter(Boolean);
    if (parts.length === 4) {
      const w = Number(parts[2]);
      const h = Number(parts[3]);
      if (Number.isFinite(w) && Number.isFinite(h) && w > 0 && h > 0) {
        return { w, h };
      }
    }
  }

  const wMatch = root.match(/\bwidth\s*=\s*"([^"]+)"/i);
  const hMatch = root.match(/\bheight\s*=\s*"([^"]+)"/i);
  if (wMatch && hMatch) {
    const stripUnit = (s: string) => Number(s.replace(/[a-z%]+$/i, ''));
    const w = stripUnit(wMatch[1]);
    const h = stripUnit(hMatch[1]);
    if (Number.isFinite(w) && Number.isFinite(h) && w > 0 && h > 0) {
      return { w, h };
    }
  }

  return null;
}

function formatBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  return `${(n / (1024 * 1024)).toFixed(1)} MB`;
}
