import { useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { Loader2, AlertCircle, ChevronLeft, ChevronRight, Download } from 'lucide-react';
import type { ViewerFilePayload } from '../../types';
import styles from './PdfViewer.module.css';

interface ImageViewerProps {
  filePath: string;
}

/**
 * Module-scoped worker URL promise. `import('.../pdf.worker.min.mjs?url')`
 * is a Vite-managed asset reference that returns a string pointing at
 * the worker's hashed file in `dist/assets/`. Vite resolves this once
 * per build and caches it; we cache the resulting string so we don't
 * pay the import cost on every page render.
 */
let workerUrlPromise: Promise<string> | null = null;
function getWorkerUrl(): Promise<string> {
  if (!workerUrlPromise) {
    workerUrlPromise = import(
      'pdfjs-dist/legacy/build/pdf.worker.min.mjs?url'
    ).then((m) => m.default);
  }
  return workerUrlPromise;
}

/**
 * In-app PDF viewer powered by `pdfjs-dist` v4.
 *
 * Implementation notes:
 *   - We import the legacy UMD build (`pdfjs-dist/legacy/build/pdf.mjs`)
 *     because the modern ESM bundle assumes real-ESM browsers; under
 *     Vite + Tauri WebView the legacy build is reliably loadable.
 *   - The worker is loaded via the same legacy build using
 *     `GlobalWorkerOptions.workerSrc`. The legacy worker is shipped as
 *     `pdfjs-dist/legacy/build/pdf.worker.min.mjs`. Vite resolves it
 *     as a static asset, and we cache the resulting URL at module
 *     scope so it's only fetched once per session.
 *   - We decimate large pages into an off-screen canvas at a chosen
 *     scale so that 50–200 page PDFs stay interactive.
 *   - Rendering is intentionally **read-only** (no annotations, no
 *     text editing) per product scope.
 */
export const PdfViewer: React.FC<ImageViewerProps> = ({ filePath }) => {
  const containerRef = useRef<HTMLDivElement>(null);
  const [payload, setPayload] = useState<ViewerFilePayload | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [pageCount, setPageCount] = useState(0);
  const [currentPage, setCurrentPage] = useState(1);
  const [scale, setScale] = useState(1.5);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    setError(null);
    setPayload(null);
    setPageCount(0);
    setCurrentPage(1);

    invoke<ViewerFilePayload>('read_file_for_viewer', { path: filePath })
      .then((data) => {
        if (!cancelled) setPayload(data);
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

  // Render the current page into the viewport whenever the input data,
  // page number, or zoom level changes.
  useEffect(() => {
    if (!payload || !containerRef.current) return;

    let cancelled = false;
    let renderTask: { cancel: () => void; promise: Promise<void> } | null = null;

    (async () => {
      try {
        const pdfjs = await import('pdfjs-dist/legacy/build/pdf.mjs');
        // Worker URL is cached at module scope — see `getWorkerUrl`.
        // First call resolves the Vite asset URL; subsequent renders
        // (page change, zoom, file swap) hit the in-memory promise.
        const workerSrc = await getWorkerUrl();
        if (cancelled) return;

        pdfjs.GlobalWorkerOptions.workerSrc = workerSrc;

        const bytes = Uint8Array.from(atob(payload.data_base64), (c) =>
          c.charCodeAt(0),
        );

        const loadingTask = pdfjs.getDocument({ data: bytes });
        const pdf = await loadingTask.promise;
        if (cancelled) return;
        setPageCount(pdf.numPages);

        const safePage = Math.min(currentPage, pdf.numPages);
        const page = await pdf.getPage(safePage);
        if (cancelled) return;

        const viewport = page.getViewport({ scale });

        const canvas = document.createElement('canvas');
        canvas.className = styles.pageCanvas;
        canvas.width = viewport.width;
        canvas.height = viewport.height;

        const ctx = canvas.getContext('2d');
        if (!ctx) throw new Error('无法创建 Canvas 上下文');

        renderTask = page.render({ canvasContext: ctx, viewport });

        await renderTask.promise;
        if (cancelled) return;

        const host = containerRef.current!;
        host.replaceChildren(canvas);
      } catch (err) {
        if (!cancelled) {
          setError(`PDF 渲染失败：${String(err)}`);
        }
      }
    })();

    return () => {
      cancelled = true;
      if (renderTask) {
        try { renderTask.cancel(); } catch { /* noop */ }
      }
    };
  }, [payload, currentPage, scale]);

  if (loading) {
    return (
      <div className={styles.center}>
        <Loader2 size={20} className={styles.spin} />
        <span>正在加载 PDF…</span>
      </div>
    );
  }

  if (error || !payload) {
    return (
      <div className={styles.center}>
        <AlertCircle size={20} />
        <span>无法加载 PDF{error ? `：${error}` : ''}</span>
      </div>
    );
  }

  const fileName = filePath.split(/[\\/]/).pop() ?? filePath;
  const dataUrl = `data:${payload.mime};base64,${payload.data_base64}`;

  return (
    <div className={styles.container}>
      <div className={styles.toolbar}>
        <span className={styles.fileName} title={fileName}>{fileName}</span>
        <span className={styles.sizeLabel}>{formatBytes(payload.size)}</span>
        <div className={styles.spacer} />
        <button
          className={styles.toolButton}
          onClick={() => setCurrentPage((p) => Math.max(1, p - 1))}
          disabled={currentPage <= 1}
          title="上一页"
        >
          <ChevronLeft size={14} />
        </button>
        <span className={styles.pageLabel}>
          {pageCount > 0
            ? `${currentPage} / ${pageCount}`
            : '—'}
        </span>
        <button
          className={styles.toolButton}
          onClick={() =>
            setCurrentPage((p) =>
              pageCount > 0 ? Math.min(pageCount, p + 1) : p,
            )
          }
          disabled={pageCount > 0 && currentPage >= pageCount}
          title="下一页"
        >
          <ChevronRight size={14} />
        </button>
        <button
          className={styles.toolButton}
          onClick={() => setScale((s) => Math.max(0.5, s - 0.25))}
          title="缩小"
        >
          −
        </button>
        <span className={styles.pageLabel}>{Math.round(scale * 100)}%</span>
        <button
          className={styles.toolButton}
          onClick={() => setScale((s) => Math.min(4, s + 0.25))}
          title="放大"
        >
          +
        </button>
        <a
          className={styles.toolButton}
          href={dataUrl}
          download={fileName}
          title="下载 PDF"
        >
          <Download size={14} />
        </a>
      </div>

      <div className={styles.viewport}>
        <div ref={containerRef} className={styles.pageHost} />
      </div>
    </div>
  );
};

function formatBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  return `${(n / (1024 * 1024)).toFixed(1)} MB`;
}
