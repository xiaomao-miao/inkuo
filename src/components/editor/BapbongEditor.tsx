import { useEffect, useRef, useState } from 'react';
import { BapbongEditor } from '@shadow-garden/bapbong-editor';
import type { BapbongEditorOptions } from '@shadow-garden/bapbong-editor';
import type { EditorChange } from '@shadow-garden/bapbong-contracts';
import type { Collection } from '@shadow-garden/bapbong-contracts';
import type { Command } from '@shadow-garden/bapbong-contracts';
import { claimBapbongLoad, type BapbongLoadCursor } from './bapbongLoadState';

export interface BapbongEditorRef {
  loadDocx: (bytes: ArrayBuffer) => Promise<{ headerKeys: string[]; footerKeys: string[] }>;
  exportDocx: () => Promise<Uint8Array>;
  save: () => Promise<Uint8Array | null>;
  focus: () => void;
  destroy: () => void;
  setZoom: (zoom: number) => void;
  getZoom: () => number;
  print: () => void;
  /** Execute a named command (e.g., 'bold', 'italic', 'columns-2') */
  executeCommand: (commandName: string, params?: unknown) => void;
  /** Get the commands collection for checking enabled state */
  getCommands: () => Collection<Command> | null;
  /** Check if a command is currently active (e.g., bold is applied) */
  isCommandActive: (commandName: string) => boolean;
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
  // A ref becoming non-null does not schedule a render. Keep an explicit
  // readiness generation so a buffer that arrived before editor construction
  // is picked up exactly once as soon as the editor is ready.
  const [editorReadyGeneration, setEditorReadyGeneration] = useState(0);
  const zoomRef = useRef(1);
  const loadGenerationRef = useRef(0);
  const loadQueueRef = useRef<Promise<void>>(Promise.resolve());
  const loadCursorRef = useRef<BapbongLoadCursor<BapbongEditor, Uint8Array>>({
    editor: null,
    buffer: null,
  });
  const onLoadRef = useRef(onLoad);
  const onErrorRef = useRef(onError);
  onLoadRef.current = onLoad;
  onErrorRef.current = onError;

  // Initialize editor
  useEffect(() => {
    if (!stackRef.current) return;

    try {
      const editor = new BapbongEditor(stackRef.current, {
        plugins,
      });

      editorRef.current = editor;
      setEditorReadyGeneration((generation) => generation + 1);

      // Subscribe to changes
      editor.onChange((change: EditorChange) => {
        if (change.docChanged) {
          onChange?.();
        }
      });

      // Helper to execute commands
      const executeCommand = (commandName: string, _params?: unknown) => {
        try {
          const cmd = editor.commands.get(commandName);
          if (cmd) {
            // ProseMirror commands take (state, dispatch) - we pass them directly
            cmd.run(editor.state, editor.dispatch);
          } else {
            console.warn(`Command '${commandName}' not found`);
          }
        } catch (err) {
          console.error(`Failed to execute command '${commandName}':`, err);
        }
      };

      // Helper to check command active state
      const isCommandActive = (commandName: string): boolean => {
        try {
          const cmd = editor.commands.get(commandName);
          if (cmd && cmd.isActive) {
            return cmd.isActive(editor.state);
          }
          return false;
        } catch {
          return false;
        }
      };

      // Get commands collection
      const getCommands = (): Collection<Command> | null => {
        return editor.commands;
      };

      // Create ref handle
      const handle: BapbongEditorRef = {
        loadDocx: async (bytes: ArrayBuffer) => {
          try {
            const result = await editor.loadDocx(bytes);
            setIsLoaded(true);
            onLoadRef.current?.(result);
            return result;
          } catch (err) {
            onErrorRef.current?.(err as Error);
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
        executeCommand,
        getCommands,
        isCommandActive,
      };

      // Notify parent that editor is ready
      onEditorViewReady?.(handle);

      // Cleanup
      return () => {
        loadGenerationRef.current += 1;
        editor.destroy();
        editorRef.current = null;
      };
    } catch (err) {
      onErrorRef.current?.(err as Error);
    }
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Load document when buffer changes
  useEffect(() => {
    const editor = editorRef.current;
    const buffer = documentBuffer;
    if (!claimBapbongLoad(loadCursorRef.current, editor, buffer) || !editor || !buffer) return;
    const generation = ++loadGenerationRef.current;
    const bytes = buffer.slice().buffer as ArrayBuffer;
    setIsLoaded(false);

    // `loadDocx` is not re-entrant. Serialize parses and skip superseded
    // buffers before they start, so rapid AI edits cannot leave two layout
    // engines mutating the same canvas concurrently.
    loadQueueRef.current = loadQueueRef.current
      .catch(() => undefined)
      .then(async () => {
        if (generation !== loadGenerationRef.current || !editorRef.current) return;
        try {
          const result = await editorRef.current.loadDocx(bytes);
          if (generation !== loadGenerationRef.current) return;
          setIsLoaded(true);
          onLoadRef.current?.(result);
        } catch (err) {
          if (generation === loadGenerationRef.current) onErrorRef.current?.(err as Error);
        }
      });
  }, [documentBuffer, editorReadyGeneration]);

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
