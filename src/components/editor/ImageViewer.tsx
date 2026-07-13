import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { Loader2, AlertCircle, ZoomIn, ZoomOut, RotateCcw } from 'lucide-react';
import type { ViewerFilePayload } from '../../types';
import styles from './ImageViewer.module.css';

interface ImageViewerProps {
  filePath: string;
}

export const ImageViewer: React.FC<ImageViewerProps> = ({ filePath }) => {
  const [payload, setPayload] = useState<ViewerFilePayload | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [zoom, setZoom] = useState(1);
  const [rotation, setRotation] = useState(0);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    setError(null);
    setPayload(null);
    setZoom(1);
    setRotation(0);

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

  if (loading) {
    return (
      <div className={styles.center}>
        <Loader2 size={20} className={styles.spin} />
        <span>正在加载图片…</span>
      </div>
    );
  }

  if (error || !payload) {
    return (
      <div className={styles.center}>
        <AlertCircle size={20} />
        <span>无法加载图片{error ? `：${error}` : ''}</span>
      </div>
    );
  }

  const dataUrl = `data:${payload.mime};base64,${payload.data_base64}`;
  const fileName = filePath.split(/[\\/]/).pop() ?? filePath;

  return (
    <div className={styles.container}>
      <div className={styles.toolbar}>
        <span className={styles.fileName} title={fileName}>{fileName}</span>
        <span className={styles.sizeLabel}>{formatBytes(payload.size)}</span>
        <div className={styles.spacer} />
        <button
          className={styles.toolButton}
          onClick={() => setZoom((z) => Math.max(0.1, z - 0.1))}
          title="缩小"
        >
          <ZoomOut size={14} />
        </button>
        <span className={styles.zoomLabel}>{Math.round(zoom * 100)}%</span>
        <button
          className={styles.toolButton}
          onClick={() => setZoom((z) => Math.min(10, z + 0.1))}
          title="放大"
        >
          <ZoomIn size={14} />
        </button>
        <button
          className={styles.toolButton}
          onClick={() => setRotation((r) => (r + 90) % 360)}
          title="旋转"
        >
          <RotateCcw size={14} />
        </button>
        <button
          className={styles.toolButton}
          onClick={() => {
            setZoom(1);
            setRotation(0);
          }}
          title="重置"
        >
          重置
        </button>
      </div>

      <div className={styles.viewport}>
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

function formatBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  return `${(n / (1024 * 1024)).toFixed(1)} MB`;
}
