import { useState, useCallback, useEffect, useRef } from 'react';
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
  isActive: boolean;
  onSave: () => void;
  canSave: boolean;
  onFind?: () => void;
  onPrint?: () => void;
  onZoomIn?: () => void;
  onZoomOut?: () => void;
}

const POLL_INTERVAL_MS = 750;

type ActiveStates = {
  bold: boolean;
  italic: boolean;
  underline: boolean;
  strike: boolean;
  subscript: boolean;
  superscript: boolean;
};

const EMPTY_ACTIVE_STATES: ActiveStates = {
  bold: false,
  italic: false,
  underline: false,
  strike: false,
  subscript: false,
  superscript: false,
};

function areToolbarActiveStatesEqual(left: ActiveStates, right: ActiveStates): boolean {
  return left.bold === right.bold &&
    left.italic === right.italic &&
    left.underline === right.underline &&
    left.strike === right.strike &&
    left.subscript === right.subscript &&
    left.superscript === right.superscript;
}

export const BapbongToolbar: React.FC<BapbongToolbarProps> = ({
  editorRef,
  fileName,
  isDirty,
  isActive,
  onSave,
  canSave,
  onFind,
  onPrint,
  onZoomIn,
  onZoomOut,
}) => {
  const [activeStates, setActiveStates] = useState<ActiveStates>(EMPTY_ACTIVE_STATES);
  const windowFocusedRef = useRef(typeof document === 'undefined' || document.hasFocus());

  const refreshActiveStates = useCallback(() => {
    const editor = editorRef.current;
    if (
      !isActive ||
      !editor ||
      document.visibilityState !== 'visible' ||
      !windowFocusedRef.current
    ) return;

    const next: ActiveStates = {
      bold: editor.isCommandActive('bold'),
      italic: editor.isCommandActive('italic'),
      underline: editor.isCommandActive('underline'),
      strike: editor.isCommandActive('strike'),
      subscript: editor.isCommandActive('subscript'),
      superscript: editor.isCommandActive('superscript'),
    };
    // Returning the previous object prevents a toolbar render when neither
    // the selection nor its formatting changed.
    setActiveStates((previous) => areToolbarActiveStatesEqual(previous, next) ? previous : next);
  }, [editorRef, isActive]);

  // Bapbong does not expose a public selection subscription. Refresh on the
  // browser input events that move the selection, with a slow fallback poll
  // for programmatic editor changes. Suspend all work while the window is not
  // visible/focused.
  useEffect(() => {
    if (!isActive) return undefined;
    let frame: number | null = null;
    const scheduleRefresh = () => {
      if (frame !== null) return;
      frame = requestAnimationFrame(() => {
        frame = null;
        refreshActiveStates();
      });
    };
    const handleFocus = () => {
      windowFocusedRef.current = true;
      scheduleRefresh();
    };
    const handleBlur = () => {
      windowFocusedRef.current = false;
    };
    const interval = setInterval(refreshActiveStates, POLL_INTERVAL_MS);
    window.addEventListener('focus', handleFocus);
    window.addEventListener('blur', handleBlur);
    document.addEventListener('visibilitychange', scheduleRefresh);
    document.addEventListener('pointerup', scheduleRefresh, true);
    document.addEventListener('keyup', scheduleRefresh, true);
    scheduleRefresh();

    return () => {
      clearInterval(interval);
      if (frame !== null) cancelAnimationFrame(frame);
      window.removeEventListener('focus', handleFocus);
      window.removeEventListener('blur', handleBlur);
      document.removeEventListener('visibilitychange', scheduleRefresh);
      document.removeEventListener('pointerup', scheduleRefresh, true);
      document.removeEventListener('keyup', scheduleRefresh, true);
    };
  }, [isActive, refreshActiveStates]);

  // Execute a command on the editor
  const executeCommand = useCallback((commandName: string, params?: unknown) => {
    editorRef.current?.executeCommand(commandName, params);
    requestAnimationFrame(refreshActiveStates);
  }, [editorRef, refreshActiveStates]);

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
