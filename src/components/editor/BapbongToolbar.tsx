import { useState, useCallback, useEffect } from 'react';
import {
  Bold,
  Italic,
  Underline,
  Strikethrough,
  AlignLeft,
  AlignCenter,
  AlignRight,
  AlignJustify,
  List,
  ListOrdered,
  Undo2,
  Redo2,
  Search,
  ZoomIn,
  ZoomOut,
  Printer,
  Save,
  Image,
  Table2,
  Subscript,
  Superscript,
} from 'lucide-react';
import type { BapbongEditorRef } from './BapbongEditor';
import styles from './BapbongToolbar.module.css';

interface BapbongToolbarProps {
  editorRef: React.MutableRefObject<BapbongEditorRef | null>;
  fileName: string;
  isDirty: boolean;
  onSave: () => void;
  canSave: boolean;
  onFind?: () => void;
  onPrint?: () => void;
  onZoomIn?: () => void;
  onZoomOut?: () => void;
}

// Active state polling interval
const POLL_INTERVAL_MS = 250;

export const BapbongToolbar: React.FC<BapbongToolbarProps> = ({
  editorRef,
  fileName,
  isDirty,
  onSave,
  canSave,
  onFind,
  onPrint,
  onZoomIn,
  onZoomOut,
}) => {
  const [, setTick] = useState(0);
  const [activeStates, setActiveStates] = useState({
    bold: false,
    italic: false,
    underline: false,
    strike: false,
    subscript: false,
    superscript: false,
  });

  // Poll for active state changes
  useEffect(() => {
    const interval = setInterval(() => {
      const editor = editorRef.current;
      if (!editor) return;

      // Check active states from editor
      setActiveStates({
        bold: editor.isCommandActive('bold'),
        italic: editor.isCommandActive('italic'),
        underline: editor.isCommandActive('underline'),
        strike: editor.isCommandActive('strike'),
        subscript: editor.isCommandActive('subscript'),
        superscript: editor.isCommandActive('superscript'),
      });

      // Force re-render
      setTick((t) => t + 1);
    }, POLL_INTERVAL_MS);

    return () => clearInterval(interval);
  }, [editorRef]);

  // Execute a command on the editor
  const executeCommand = useCallback((commandName: string, params?: unknown) => {
    editorRef.current?.executeCommand(commandName, params);
  }, [editorRef]);

  return (
    <div className={styles.toolbar}>
      {/* File info */}
      <div className={styles.toolbarSection}>
        <span className={styles.fileName}>
          {fileName}
          {isDirty && <span className={styles.dirtyDot}>·</span>}
        </span>
      </div>

      <div className={styles.separator} />

      {/* History */}
      <div className={styles.toolbarSection}>
        <button
          className={styles.toolbarButton}
          onClick={() => executeCommand('undo')}
          title="撤销 (Ctrl+Z)"
        >
          <Undo2 size={16} />
        </button>
        <button
          className={styles.toolbarButton}
          onClick={() => executeCommand('redo')}
          title="重做 (Ctrl+Y)"
        >
          <Redo2 size={16} />
        </button>
      </div>

      <div className={styles.separator} />

      {/* Font formatting */}
      <div className={styles.toolbarSection}>
        <button
          className={`${styles.toolbarButton} ${activeStates.bold ? styles.active : ''}`}
          onClick={() => executeCommand('bold')}
          title="加粗 (Ctrl+B)"
        >
          <Bold size={16} />
        </button>
        <button
          className={`${styles.toolbarButton} ${activeStates.italic ? styles.active : ''}`}
          onClick={() => executeCommand('italic')}
          title="斜体 (Ctrl+I)"
        >
          <Italic size={16} />
        </button>
        <button
          className={`${styles.toolbarButton} ${activeStates.underline ? styles.active : ''}`}
          onClick={() => executeCommand('underline')}
          title="下划线 (Ctrl+U)"
        >
          <Underline size={16} />
        </button>
        <button
          className={`${styles.toolbarButton} ${activeStates.strike ? styles.active : ''}`}
          onClick={() => executeCommand('strike')}
          title="删除线"
        >
          <Strikethrough size={16} />
        </button>
        <button
          className={`${styles.toolbarButton} ${activeStates.superscript ? styles.active : ''}`}
          onClick={() => executeCommand('superscript')}
          title="上标"
        >
          <Superscript size={16} />
        </button>
        <button
          className={`${styles.toolbarButton} ${activeStates.subscript ? styles.active : ''}`}
          onClick={() => executeCommand('subscript')}
          title="下标"
        >
          <Subscript size={16} />
        </button>
      </div>

      <div className={styles.separator} />

      {/* Alignment */}
      <div className={styles.toolbarSection}>
        <button
          className={styles.toolbarButton}
          onClick={() => executeCommand('align-left')}
          title="左对齐"
        >
          <AlignLeft size={16} />
        </button>
        <button
          className={styles.toolbarButton}
          onClick={() => executeCommand('align-center')}
          title="居中对齐"
        >
          <AlignCenter size={16} />
        </button>
        <button
          className={styles.toolbarButton}
          onClick={() => executeCommand('align-right')}
          title="右对齐"
        >
          <AlignRight size={16} />
        </button>
        <button
          className={styles.toolbarButton}
          onClick={() => executeCommand('align-justify')}
          title="两端对齐"
        >
          <AlignJustify size={16} />
        </button>
      </div>

      <div className={styles.separator} />

      {/* Lists */}
      <div className={styles.toolbarSection}>
        <button
          className={styles.toolbarButton}
          onClick={() => executeCommand('bullet-list')}
          title="项目符号列表"
        >
          <List size={16} />
        </button>
        <button
          className={styles.toolbarButton}
          onClick={() => executeCommand('ordered-list')}
          title="编号列表"
        >
          <ListOrdered size={16} />
        </button>
      </div>

      <div className={styles.separator} />

      {/* Insert */}
      <div className={styles.toolbarSection}>
        <button
          className={styles.toolbarButton}
          onClick={() => executeCommand('insert-image')}
          title="插入图片"
        >
          <Image size={16} />
        </button>
        <button
          className={styles.toolbarButton}
          onClick={() => executeCommand('insert-table')}
          title="插入表格"
        >
          <Table2 size={16} />
        </button>
      </div>

      <div className={styles.separator} />

      {/* Find */}
      <div className={styles.toolbarSection}>
        <button
          className={styles.toolbarButton}
          onClick={onFind}
          title="查找 (Ctrl+F)"
        >
          <Search size={16} />
        </button>
      </div>

      <div className={styles.spacer} />

      {/* View controls */}
      <div className={styles.toolbarSection}>
        <button
          className={styles.toolbarButton}
          onClick={onZoomOut}
          title="缩小"
        >
          <ZoomOut size={16} />
        </button>
        <button
          className={styles.toolbarButton}
          onClick={onZoomIn}
          title="放大"
        >
          <ZoomIn size={16} />
        </button>
        <button
          className={styles.toolbarButton}
          onClick={onPrint}
          title="打印 (Ctrl+P)"
        >
          <Printer size={16} />
        </button>
      </div>

      <div className={styles.separator} />

      {/* Save */}
      <div className={styles.toolbarSection}>
        <button
          className={`${styles.saveButton} ${isDirty ? styles.dirty : ''}`}
          onClick={onSave}
          disabled={!canSave}
          title="保存 (Ctrl+S)"
        >
          <Save size={14} />
          <span>保存</span>
        </button>
      </div>
    </div>
  );
};
