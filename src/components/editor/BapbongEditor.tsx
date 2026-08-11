import { useEffect, useRef, useState } from 'react';
import { BapbongEditor } from '@shadow-garden/bapbong-editor';
import type { BapbongEditorOptions } from '@shadow-garden/bapbong-editor';
import type { EditorChange } from '@shadow-garden/bapbong-contracts';

export interface BapbongEditorRef {
  loadDocx: (bytes: ArrayBuffer) => Promise<{ headerKeys: string[]; footerKeys: string[] }>;
  exportDocx: () => Promise<Uint8Array>;
  save: () => Promise<Uint8Array | null>;
  focus: () => void;
  destroy: () => void;
  setZoom: (zoom: number) => void;
  getZoom: () => number;
  print: () => void;
}

export interface BapbongEditorProps {
  /** The docx file bytes to load */
  documentBuffer: Uint8Array | null;
  /** Called when the document changes */
  onChange?: () => void;
  /** Called when the editor view is ready */
  onEditorViewReady?: (editor: BapbongEditorRef) => void;
  /** Custom plugins to add */
  plugins?: BapbongEditorOptions['plugins'];
  /** CSS class for the container */
  className?: string;
  /** Called on document load */
  onLoad?: (info: { headerKeys: string[]; footerKeys: string[] }) => void;
  /** Called on error */
  onError?: (error: Error) => void;
}

/**
 * React wrapper for bapbong editor
 * 
 * @example
 * ```tsx
 * <BapbongEditor
 *   documentBuffer={docxBuffer}
 *   onChange={() => setIsDirty(true)}
 *   onLoad={(info) => console.log('Loaded:', info)}
 * />
 * ```
 */
export const BapbongEditorComponent = ({
  documentBuffer,
  onChange,
  onEditorViewReady,
  plugins,
  className,
  onLoad,
  onError,
}: BapbongEditorProps) => {
  const stackRef = useRef<HTMLDivElement>(null);
  const editorRef = useRef<BapbongEditor | null>(null);
  const containerRef = useRef<HTMLDivElement>(null);
  const [isLoaded, setIsLoaded] = useState(false);
  const zoomRef = useRef(1);

  // Initialize editor
  useEffect(() => {
    if (!stackRef.current) return;

    try {
      const editor = new BapbongEditor(stackRef.current, {
        plugins,
      });

      editorRef.current = editor;

      // Subscribe to changes
      editor.onChange((change: EditorChange) => {
        if (change.docChanged) {
          onChange?.();
        }
      });

      // Create ref handle
      const handle: BapbongEditorRef = {
        loadDocx: async (bytes: ArrayBuffer) => {
          try {
            const result = await editor.loadDocx(bytes);
            setIsLoaded(true);
            onLoad?.(result);
            return result;
          } catch (err) {
            onError?.(err as Error);
            throw err;
          }
        },
        exportDocx: () => editor.exportDocx(),
        save: async () => {
          try {
            const bytes = await editor.exportDocx();
            return bytes;
          } catch {
            return null;
          }
        },
        focus: () => editor.focus(),
        destroy: () => editor.destroy(),
        setZoom: (zoom: number) => {
          zoomRef.current = zoom;
          if (stackRef.current) {
            stackRef.current.style.transform = `scale(${zoom})`;
            stackRef.current.style.transformOrigin = 'top left';
          }
        },
        getZoom: () => zoomRef.current,
        print: () => window.print(),
      };

      // Notify parent that editor is ready
      onEditorViewReady?.(handle);

      // Cleanup
      return () => {
        editor.destroy();
        editorRef.current = null;
      };
    } catch (err) {
      onError?.(err as Error);
    }
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Load document when buffer changes
  useEffect(() => {
    if (!editorRef.current || !documentBuffer) return;

    const load = async () => {
      try {
        await editorRef.current!.loadDocx(documentBuffer.buffer);
        setIsLoaded(true);
      } catch (err) {
        onError?.(err as Error);
      }
    };

    load();
  }, [documentBuffer, onError]);

  return (
    <div
      ref={containerRef}
      className={className}
      style={{
        width: '100%',
        height: '100%',
        overflow: 'auto',
        position: 'relative',
      }}
    >
      <div
        ref={stackRef}
        style={{
          minHeight: '100%',
        }}
      />
      {!isLoaded && (
        <div
          style={{
            position: 'absolute',
            top: 0,
            left: 0,
            right: 0,
            bottom: 0,
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'center',
            background: 'rgba(255,255,255,0.9)',
          }}
        >
          正在加载文档...
        </div>
      )}
    </div>
  );
};
