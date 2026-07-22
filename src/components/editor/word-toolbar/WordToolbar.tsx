import React, { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import type { EditorView } from 'prosemirror-view';
import type { Mark, MarkType, Node as PMNode } from 'prosemirror-model';
import {
  toggleBold,
  toggleItalic,
  toggleUnderline,
  toggleStrike,
  toggleSuperscript,
  toggleSubscript,
  setTextColor,
  setHighlight,
  clearHighlight,
  setFontSize,
  setFontFamily,
  alignLeft,
  alignCenter,
  alignRight,
  alignJustify,
  toggleBulletList,
  toggleNumberedList,
  increaseIndent,
  decreaseIndent,
  applyStyle,
  clearStyle,
  singleSpacing,
  oneAndHalfSpacing,
  doubleSpacing,
  setLineSpacing,
  insertPageBreak,
  isMarkActive,
  getMarkAttr,
  getParagraphAlignment,
  getStyleId,
  insertTable,
  insertImageFromFile,
  insertHyperlink,
  removeHyperlink,
  setHyperlink,
  isHyperlinkActive,
  setWatermark,
  clearFormatting,
} from '@eigenpal/docx-editor-core/prosemirror/commands';
import {
  Undo2,
  Redo2,
  Search,
  Save,
  Bold,
  Italic,
  Underline as UnderlineIcon,
  Strikethrough,
  AlignLeft,
  AlignCenter,
  AlignRight,
  AlignJustify,
  List,
  ListOrdered,
  IndentDecrease,
  IndentIncrease,
  ChevronDown,
  ZoomIn,
  ZoomOut,
  Printer,
  Sparkles,
  Scissors,
  Copy as CopyIcon,
  Clipboard,
  Brush,
  Image as ImageIcon,
  Link2,
  Table2,
  Sigma,
  Pilcrow,
  Type,
  PaintBucket,
  Eraser,
  ChevronUp,
  ArrowDownAZ,
  ArrowUpAZ,
  Replace,
  Highlighter,
  SpellCheck2,
  Heading1,
  WrapText,
  PanelTop,
  PanelBottom,
  PencilLine,
  type LucideIcon,
} from 'lucide-react';
import styles from './WordToolbar.module.css';

// ─── Constants ────────────────────────────────────────────────────────────────

const FONT_FAMILIES = [
  'Microsoft YaHei',
  'SimSun',
  'SimHei',
  'KaiTi',
  'FangSong',
  'Arial',
  'Times New Roman',
  'Calibri',
  'Helvetica',
  'Georgia',
  'Tahoma',
  'Verdana',
];

const FONT_SIZES_PT = [8, 9, 10, 11, 12, 14, 16, 18, 20, 22, 24, 28, 32, 36, 48, 72, 96];

const TEXT_COLORS = [
  '#000000', '#434343', '#666666', '#999999', '#B7B7B7', '#CCCCCC', '#D9D9D9', '#EFEFEF', '#F3F3F3', '#FFFFFF',
  '#980000', '#FF0000', '#FF9900', '#FFFF00', '#00FF00', '#00FFFF', '#4A86E8', '#0000FF', '#9900FF', '#FF00FF',
  '#E6B8B7', '#F4CCCC', '#FCE5CD', '#FFF2CC', '#D9EAD3', '#D0E0E3', '#C9DAF8', '#CFE2F3', '#D9D2E9', '#EAD1DC',
];

const HIGHLIGHT_COLORS = [
  'none', 'yellow', 'green', 'cyan', 'magenta', 'red', 'blue', 'darkBlue', 'darkCyan', 'darkGreen',
  'darkMagenta', 'darkRed', 'darkYellow', 'darkGray', 'lightGray', 'black', 'white',
];

const PARAGRAPH_STYLES: Array<{ value: string; label: string }> = [
  { value: 'Normal', label: '正文' },
  { value: 'Heading1', label: '标题 1' },
  { value: 'Heading2', label: '标题 2' },
  { value: 'Heading3', label: '标题 3' },
  { value: 'Heading4', label: '标题 4' },
  { value: 'Heading5', label: '标题 5' },
  { value: 'Heading6', label: '标题 6' },
  { value: 'Title', label: '标题' },
  { value: 'Subtitle', label: '副标题' },
  { value: 'Quote', label: '引用' },
  { value: 'IntenseQuote', label: '明显引用' },
  { value: 'ListParagraph', label: '列表段落' },
  { value: 'NoSpacing', label: '无间距' },
];

const ZOOM_LEVELS = [0.5, 0.75, 1, 1.25, 1.5, 1.75, 2, 2.5, 3];

const LINE_SPACING_OPTIONS = [
  { value: '1', label: '1.0' },
  { value: '1.15', label: '1.15' },
  { value: '1.5', label: '1.5' },
  { value: '2', label: '2.0' },
  { value: '2.5', label: '2.5' },
  { value: '3', label: '3.0' },
];

// (kept for reference; rows/cols are bounded directly inside TablePicker)

const SYMBOLS = [
  '§', '©', '®', '™', '¶', '†', '‡', '•', '…', '–', '—', '·',
  '€', '£', '¥', '¢', '₹', '₽', '₩', '₪', '¢', '¤',
  '°', '′', '″', 'µ', 'π', 'Ω', '∞', '√', '÷', '×', '±', '≈', '≠', '≤', '≥', '∑',
  '←', '→', '↑', '↓', '↔', '⇒', '⇔',
  '★', '☆', '♠', '♡', '♢', '♣', '♪', '♫', '♥', '♦', '♀', '♂',
  '☺', '☻', '✓', '✗', '✔', '✘',
  '☎', '✉', '✂', '✏', '✒', '⚙', '⚡', '⚠', '☂', '❤',
];

// ─── Helpers ──────────────────────────────────────────────────────────────────

// Returns true only if the view is alive AND has a valid state to dispatch
// against. ProseMirror nulls `view.state` during teardown, so a stale `view`
// ref captured by a click handler can survive a tab switch / file reload and
// cause `Cannot read properties of undefined (reading 'schema')` deep inside
// `chainCommands`. Treat that as "no view" rather than passing it on. The
// schema check is non-obvious but matters: during the very first render after
// `EditorView` construction, `view.state` is set synchronously but `schema`
// is wired in slightly later by prosemirror internals; calling any of our
// query helpers in that gap throws and unmounts the whole React tree.
function isViewReady(view: EditorView | null): view is EditorView {
  return !!view && !!view.state && !!view.state.schema;
}

// ProseMirror command-runner code (e.g. `chunk-STIS5BU3.js:4713`) destructures
// `state.schema` at the very top of many commands, including `insertPageBreak`
// from `@eigenpal/docx-editor-core`. If `state` is undefined that throws an
// uncatchable TypeError out of the click handler. Belt-and-suspenders: guard at
// the dispatch site too, so even if a future call leaks past `isViewReady` we
// degrade to a no-op rather than a black-screen crash.
function runCommand(view: EditorView | null, command: (state: any, dispatch?: any, view?: any) => boolean) {
  if (!isViewReady(view)) return;
  try {
    command(view.state, view.dispatch, view);
  } catch (err) {
    // Swallow command-runner crashes during teardown races. The view will be
    // re-created on the next legitimate interaction; we never want a transient
    // ProseMirror teardown to bring down the React tree (and the Tauri window).
    if (import.meta.env?.DEV) {
      // eslint-disable-next-line no-console
      console.warn('[WordToolbar] command dispatch ignored:', err);
    }
  }
}

function hpToPt(hp: unknown): number | null {
  if (hp == null) return null;
  const n = typeof hp === 'number' ? hp : Number(hp);
  if (!Number.isFinite(n)) return null;
  return Math.round(n / 2);
}

function rgbToHex(rgb: unknown): string | null {
  if (!rgb) return null;
  const s = String(rgb);
  return s.startsWith('#') ? s : `#${s}`;
}

// ─── Sub-buttons ──────────────────────────────────────────────────────────────

interface IconButtonProps {
  icon: LucideIcon | React.ComponentType<{ size?: number }>;
  title: string;
  active?: boolean;
  disabled?: boolean;
  onClick: () => void;
  size?: number;
  /** Forwarded to the underlying <button> so callers can anchor popovers. */
  buttonRef?: React.RefObject<HTMLButtonElement | null>;
}

const IconButton: React.FC<IconButtonProps> = ({
  icon: Icon,
  title,
  active,
  disabled,
  onClick,
  size,
  buttonRef,
}) => (
  <button
    ref={buttonRef}
    type="button"
    className={`${styles.wToolbarIconBtn} ${active ? styles.wToolbarIconBtnActive : ''}`}
    title={title}
    aria-label={title}
    aria-pressed={active}
    disabled={disabled}
    onMouseDown={(e) => e.preventDefault() /* keep editor focus */}
    onClick={onClick}
  >
    <Icon size={size ?? 13} />
  </button>
);

interface DropdownPortalLayout {
  top: number;
  left: number;
  width: number;
  /** Whether the menu opens below or above the trigger. */
  placement: 'bottom' | 'top';
}

/**
 * Compute the fixed-position coordinates for a dropdown menu anchored to a
 * trigger element. Works regardless of any `overflow: hidden` / `contain`
 * ancestors the trigger sits inside, because the menu itself is rendered
 * into a portal at `document.body` (see `DropdownPortal`).
 */
function useDropdownPosition(
  triggerRef: React.RefObject<HTMLElement | null>,
  open: boolean,
): DropdownPortalLayout | null {
  const [layout, setLayout] = useState<DropdownPortalLayout | null>(null);

  useLayoutEffect(() => {
    if (!open) {
      setLayout(null);
      return;
    }
    const compute = () => {
      const el = triggerRef.current;
      if (!el) return;
      const rect = el.getBoundingClientRect();
      const GAP = 2;
      const MARGIN = 8;
      const MIN_BELOW = 160; // heuristic: prefer flipping up if below space is tiny
      const viewportH = window.innerHeight;
      const spaceBelow = viewportH - rect.bottom - MARGIN;
      const spaceAbove = rect.top - MARGIN;
      const placement: 'bottom' | 'top' =
        spaceBelow >= MIN_BELOW || spaceBelow >= spaceAbove ? 'bottom' : 'top';
      setLayout({
        top: placement === 'bottom' ? rect.bottom + GAP : rect.top - GAP,
        left: rect.left,
        width: rect.width,
        placement,
      });
    };
    compute();
    window.addEventListener('resize', compute);
    window.addEventListener('scroll', compute, true);
    return () => {
      window.removeEventListener('resize', compute);
      window.removeEventListener('scroll', compute, true);
    };
  }, [open, triggerRef]);

  return layout;
}interface DropdownPortalProps {
  triggerRef: React.RefObject<HTMLElement | null>;
  open: boolean;
  onClose: () => void;
  /** Class name applied to the menu panel. */
  menuClassName?: string;
  /** Optional style override for the menu panel (used to anchor placement / width). */
  menuStyle?: React.CSSProperties;
  children: React.ReactNode;
}

/**
 * Renders a backdrop + menu into `document.body` so the menu escapes any
 * `overflow: hidden` / `contain` ancestors of the trigger (e.g. the toolbar
 * root and the office stack). Closes on backdrop click and on Escape.
 */
const DropdownPortal: React.FC<DropdownPortalProps> = ({
  triggerRef,
  open,
  onClose,
  menuClassName,
  menuStyle,
  children,
}) => {
  const layout = useDropdownPosition(triggerRef, open);
  const menuRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.stopPropagation();
        onClose();
      }
    };
    window.addEventListener('keydown', onKey, true);
    return () => window.removeEventListener('keydown', onKey, true);
  }, [open, onClose]);

  // Re-apply the upward translate whenever the menu mounts or `placement`
  // changes (e.g. after a resize that pushes the trigger close to the bottom
  // of the viewport and flips the menu to open upward). Without this,
  // the ref callback alone would only run on initial mount.
  useLayoutEffect(() => {
    const el = menuRef.current;
    if (!el || !layout) return;
    if (layout.placement === 'top') {
      el.style.transform = `translateY(-${el.offsetHeight}px)`;
    } else {
      el.style.transform = '';
    }
  }, [layout, open]);

  if (typeof document === 'undefined') return null;
  if (!open || !layout) return null;

  const anchorStyle: React.CSSProperties = {
    position: 'fixed',
    top: layout.top,
    left: layout.left,
    minWidth: layout.width,
    zIndex: 1000,
  };

  return createPortal(
    <>
      <div
        className={styles.wDropdownBackdrop}
        onMouseDown={(e) => {
          // Prevent the editor's mousedown handler from stealing focus
          // before we close.
          e.preventDefault();
          e.stopPropagation();
          onClose();
        }}
      />
      <div
        ref={menuRef}
        className={`${styles.wDropdownMenu} ${menuClassName ?? ''}`}
        style={{ ...anchorStyle, ...menuStyle }}
      >
        {children}
      </div>
    </>,
    document.body,
  );
};

interface DropdownProps {
  value: string;
  options: Array<{ value: string; label: string }>;
  onChange: (value: string) => void;
  title: string;
  width?: number;
  displayValue?: string;
  icon?: LucideIcon;
}

const Dropdown: React.FC<DropdownProps> = ({
  value,
  options,
  onChange,
  title,
  width,
  displayValue,
  icon: Icon,
}) => {
  const [open, setOpen] = useState(false);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const current = options.find((o) => o.value === value);
  const close = useCallback(() => setOpen(false), []);
  return (
    <div className={styles.wDropdown} style={width ? { width } : undefined}>
      <button
        ref={triggerRef}
        type="button"
        className={styles.wDropdownTrigger}
        title={title}
        onMouseDown={(e) => e.preventDefault()}
        onClick={() => setOpen((o) => !o)}
      >
        {Icon && <Icon size={12} />}
        <span className={styles.wDropdownLabel}>{displayValue ?? current?.label ?? value}</span>
        <ChevronDown size={11} />
      </button>
      <DropdownPortal triggerRef={triggerRef} open={open} onClose={close}>
        {options.map((o) => (
          <button
            key={o.value}
            type="button"
            className={`${styles.wDropdownOption} ${o.value === value ? styles.wDropdownOptionActive : ''}`}
            onClick={() => {
              onChange(o.value);
              setOpen(false);
            }}
          >
            {o.label}
          </button>
        ))}
      </DropdownPortal>
    </div>
  );
};

interface FontSizeDropdownProps {
  value: number;
  onChange: (pt: number) => void;
  onStep: (delta: number) => void;
  disabled?: boolean;
}

const FontSizeControl: React.FC<FontSizeDropdownProps> = ({ value, onChange, onStep, disabled }) => {
  const [open, setOpen] = useState(false);
  const [input, setInput] = useState(String(value));
  useEffect(() => setInput(String(value)), [value]);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const close = useCallback(() => setOpen(false), []);

  const commit = () => {
    const n = Number(input);
    if (Number.isFinite(n) && n > 0 && n <= 400) {
      onChange(n);
    } else {
      setInput(String(value));
    }
  };

  return (
    <div className={styles.wFontSizeCluster}>
      <input
        type="text"
        className={styles.wFontSizeInput}
        value={input}
        onChange={(e) => setInput(e.target.value)}
        onBlur={commit}
        onKeyDown={(e) => {
          if (e.key === 'Enter') {
            e.preventDefault();
            commit();
          }
        }}
        title="字号 (pt)"
        disabled={disabled}
      />
      <div className={styles.wFontSizeSpinner}>
        <button
          type="button"
          className={styles.wSpinnerBtn}
          onMouseDown={(e) => e.preventDefault()}
          onClick={() => onStep(1)}
          title="增大字号"
          disabled={disabled}
        >
          <ChevronUp size={9} />
        </button>
        <button
          type="button"
          className={styles.wSpinnerBtn}
          onMouseDown={(e) => e.preventDefault()}
          onClick={() => onStep(-1)}
          title="减小字号"
          disabled={disabled}
        >
          <ChevronDown size={9} />
        </button>
      </div>
      <button
        ref={triggerRef}
        type="button"
        className={styles.wFontSizeDropdown}
        onMouseDown={(e) => e.preventDefault()}
        onClick={() => setOpen((o) => !o)}
        title="字号列表"
      >
        <ChevronDown size={11} />
      </button>
      <DropdownPortal triggerRef={triggerRef} open={open} onClose={close} menuClassName={styles.wFontSizeMenu}>
        {FONT_SIZES_PT.map((s) => (
          <button
            key={s}
            type="button"
            className={`${styles.wDropdownOption} ${s === value ? styles.wDropdownOptionActive : ''}`}
            onClick={() => {
              onChange(s);
              setInput(String(s));
              setOpen(false);
            }}
          >
            {s}
          </button>
        ))}
      </DropdownPortal>
    </div>
  );
};

interface ColorPickerProps {
  colors: string[];
  onChange: (c: string) => void;
  title: string;
  highlight?: boolean;
  fontColor?: string;
}

const ColorPicker: React.FC<ColorPickerProps> = ({ colors, onChange, title, highlight, fontColor }) => {
  const [open, setOpen] = useState(false);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const close = useCallback(() => setOpen(false), []);
  return (
    <div className={styles.wColorPicker}>
      <button
        ref={triggerRef}
        type="button"
        className={styles.wColorPickerTrigger}
        title={title}
        onMouseDown={(e) => e.preventDefault()}
        onClick={() => setOpen((o) => !o)}
      >
        {highlight ? (
          <Highlighter size={13} style={{ color: '#facc15' }} />
        ) : (
          <span className={styles.wColorPickerChar} style={{ color: fontColor || '#000', textDecoration: 'underline' }}>A</span>
        )}
        <ChevronDown size={9} />
      </button>
      <DropdownPortal triggerRef={triggerRef} open={open} onClose={close} menuClassName={styles.wColorPickerGrid}>
        {colors.map((c) => (
          <button
            key={c}
            type="button"
            className={styles.wColorSwatch}
            style={{
              background: c === 'none' ? '#fff' : c.toLowerCase(),
              border: c === 'none' ? '1px solid var(--border-color)' : 'none',
            }}
            title={c}
            onClick={() => {
              onChange(c);
              setOpen(false);
            }}
          />
        ))}
      </DropdownPortal>
    </div>
  );
};

interface PageColorPickerProps {
  colors: string[];
  onChange: (c: string) => void;
  title: string;
  /** Whether the picker should be enabled (mirrors editor handle availability). */
  disabled?: boolean;
}

const PageColorPicker: React.FC<PageColorPickerProps> = ({ colors, onChange, title, disabled }) => {
  const [open, setOpen] = useState(false);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const close = useCallback(() => setOpen(false), []);
  return (
    <div className={styles.wColorPicker}>
      <button
        ref={triggerRef}
        type="button"
        className={styles.wColorPickerTrigger}
        title={title}
        onMouseDown={(e) => e.preventDefault()}
        onClick={() => setOpen((o) => !o)}
        disabled={disabled}
      >
        <PaintBucket size={13} />
        <ChevronDown size={9} />
      </button>
      <DropdownPortal triggerRef={triggerRef} open={open} onClose={close} menuClassName={styles.wColorPickerGrid}>
        <button
          type="button"
          className={styles.wColorSwatch}
          style={{
            background: 'transparent',
            border: '1px dashed var(--border-color)',
            position: 'relative',
          }}
          title="无颜色"
          onClick={() => {
            onChange('none');
            setOpen(false);
          }}
        >
          <span style={{ position: 'absolute', inset: 0, display: 'flex', alignItems: 'center', justifyContent: 'center', fontSize: 9, color: 'var(--fg-muted)' }}>无</span>
        </button>
        {colors.map((c) => (
          <button
            key={c}
            type="button"
            className={styles.wColorSwatch}
            style={{ background: c.toLowerCase() }}
            title={c}
            onClick={() => {
              onChange(c);
              setOpen(false);
            }}
          />
        ))}
      </DropdownPortal>
    </div>
  );
};

interface TablePickerProps {
  onInsert: (rows: number, cols: number) => void;
}

const TablePicker: React.FC<TablePickerProps> = ({ onInsert }) => {
  const [hover, setHover] = useState<{ rows: number; cols: number }>({ rows: 0, cols: 0 });
  const [open, setOpen] = useState(false);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const close = useCallback(() => setOpen(false), []);
  const maxRows = 10;
  const maxCols = 5;

  return (
    <div className={styles.wTablePicker}>
      <button
        ref={triggerRef}
        type="button"
        className={styles.wColorPickerTrigger}
        title="插入表格"
        onMouseDown={(e) => e.preventDefault()}
        onClick={() => setOpen((o) => !o)}
      >
        <Table2 size={13} />
        <ChevronDown size={9} />
      </button>
      <DropdownPortal triggerRef={triggerRef} open={open} onClose={close} menuClassName={styles.wTableMenu}>
        <div className={styles.wTableGridHeader}>
          {hover.rows > 0 && hover.cols > 0
            ? `${hover.rows} × ${hover.cols} 表格`
            : '选择行列'}
        </div>
        <div
          className={styles.wTableGrid}
          onMouseLeave={() => setHover({ rows: 0, cols: 0 })}
        >
          {Array.from({ length: maxRows }).map((_, r) =>
            Array.from({ length: maxCols }).map((_, c) => {
              const active = r < hover.rows && c < hover.cols;
              return (
                <div
                  key={`${r}-${c}`}
                  className={`${styles.wTableCell} ${active ? styles.wTableCellActive : ''}`}
                  onMouseEnter={() => setHover({ rows: r + 1, cols: c + 1 })}
                  onClick={() => {
                    onInsert(r + 1, c + 1);
                    setOpen(false);
                  }}
                />
              );
            }),
          )}
        </div>
      </DropdownPortal>
    </div>
  );
};

interface SymbolPickerProps {
  onInsert: (symbol: string) => void;
}

const SymbolPicker: React.FC<SymbolPickerProps> = ({ onInsert }) => {
  const [open, setOpen] = useState(false);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const close = useCallback(() => setOpen(false), []);
  return (
    <div className={styles.wSymbolPicker}>
      <button
        ref={triggerRef}
        type="button"
        className={styles.wColorPickerTrigger}
        title="插入特殊符号"
        onMouseDown={(e) => e.preventDefault()}
        onClick={() => setOpen((o) => !o)}
      >
        <Sigma size={13} />
        <ChevronDown size={9} />
      </button>
      <DropdownPortal triggerRef={triggerRef} open={open} onClose={close} menuClassName={styles.wSymbolMenu}>
        <div className={styles.wTableGridHeader}>符号</div>
        <div className={styles.wSymbolGrid}>
          {SYMBOLS.map((s) => (
            <button
              key={s}
              type="button"
              className={styles.wSymbolCell}
              onClick={() => {
                onInsert(s);
                setOpen(false);
              }}
            >
              {s}
            </button>
          ))}
        </div>
      </DropdownPortal>
    </div>
  );
};

// ─── Settings popovers (replace window.prompt with in-app panels) ─────────────

/**
 * `FormPopover` wraps `DropdownPortal` and adds a consistent settings-panel
 * chrome (title bar + footer with Cancel/Confirm). Triggered by a toolbar
 * button; renders into a portal at `<body>` so it escapes any `overflow:hidden`
 * ancestor. Used to replace the legacy `window.prompt` calls with an in-app
 * panel themed to match the rest of the toolbar.
 */
interface FormPopoverProps {
  triggerRef: React.RefObject<HTMLElement | null>;
  open: boolean;
  onClose: () => void;
  /** Header label shown above the form body. */
  title: string;
  /** Optional leading icon next to the title (lucide component or string char). */
  titleIcon?: React.ReactNode;
  /** Approximate menu width in px. */
  width?: number;
  /** Body content (form fields). */
  children: React.ReactNode;
  /** Footer button labels. Defaults: "取消" / "确定". */
  confirmLabel?: string;
  cancelLabel?: string;
  /** Disable the confirm button (e.g. invalid form). */
  confirmDisabled?: boolean;
  /** Confirm handler. */
  onConfirm: () => void;
}

/**
 * Renders a settings panel into a portal at `<body>`. The panel shares
 * styling with the existing dropdown menus but lays out vertically: a
 * header row, a scrollable body, and a footer row with action buttons.
 */
const FormPopover: React.FC<FormPopoverProps> = ({
  triggerRef,
  open,
  onClose,
  title,
  titleIcon,
  width = 320,
  children,
  confirmLabel = '确定',
  cancelLabel = '取消',
  confirmDisabled,
  onConfirm,
}) => {
  const layout = useDropdownPosition(triggerRef, open);
  const menuRef = useRef<HTMLDivElement>(null);

  // Reset the upward translate whenever the menu opens or placement changes.
  useLayoutEffect(() => {
    const el = menuRef.current;
    if (!el || !layout) return;
    if (layout.placement === 'top') {
      el.style.transform = `translateY(-${el.offsetHeight}px)`;
    } else {
      el.style.transform = '';
    }
  }, [layout, open]);

  // Submit on Enter, cancel on Escape (handled by DropdownPortal's keydown,
  // we only handle Enter here to keep the wiring self-contained).
  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Enter' && !e.shiftKey && !e.altKey) {
        const tag = (e.target as HTMLElement | null)?.tagName;
        // Avoid hijacking Enter inside multi-line textareas.
        if (tag === 'TEXTAREA') return;
        if (confirmDisabled) return;
        e.preventDefault();
        onConfirm();
      }
    };
    window.addEventListener('keydown', onKey, true);
    return () => window.removeEventListener('keydown', onKey, true);
  }, [open, confirmDisabled, onConfirm]);

  if (typeof document === 'undefined') return null;
  if (!open || !layout) return null;

  const anchorStyle: React.CSSProperties = {
    position: 'fixed',
    top: layout.top,
    left: layout.left,
    width,
    zIndex: 1000,
  };

  return createPortal(
    <>
      <div
        className={styles.wDropdownBackdrop}
        onMouseDown={(e) => {
          e.preventDefault();
          e.stopPropagation();
          onClose();
        }}
      />
      <div
        ref={menuRef}
        className={`${styles.wDropdownMenu} ${styles.wFormMenu}`}
        style={anchorStyle}
        role="dialog"
        aria-label={title}
        onMouseDown={(e) => e.stopPropagation()}
      >
        <div className={styles.wFormHeader}>
          {titleIcon && <span className={styles.wFormHeaderIcon}>{titleIcon}</span>}
          <span className={styles.wFormHeaderTitle}>{title}</span>
        </div>
        <div className={styles.wFormBody}>{children}</div>
        <div className={styles.wFormFooter}>
          <button
            type="button"
            className={styles.wFormBtnSecondary}
            onClick={onClose}
          >
            {cancelLabel}
          </button>
          <button
            type="button"
            className={styles.wFormBtnPrimary}
            disabled={confirmDisabled}
            onClick={onConfirm}
          >
            {confirmLabel}
          </button>
        </div>
      </div>
    </>,
    document.body,
  );
};

// ─── LinkPopover ──────────────────────────────────────────────────────────────

interface LinkPopoverProps {
  /** Current selection text (if any) — used as the default display text. */
  initialText: string;
  /** Currently-selected link URL if the cursor sits inside one. */
  initialUrl?: string;
  /** True when the cursor is already inside a hyperlink (so we're editing it). */
  isEditingExisting: boolean;
  /** Confirm handler receives the URL and display text. */
  onConfirm: (url: string, displayText: string) => void;
  /** Trigger button ref so the popover anchors next to it. */
  triggerRef: React.RefObject<HTMLElement | null>;
  open: boolean;
  onClose: () => void;
}

/**
 * Settings panel for inserting or editing a hyperlink. Mirrors the Word
 * "Insert Hyperlink" dialog: URL field + optional display text + quick
 * presets for common URL prefixes. Replaces the previous `window.prompt`
 * which was a single-line modal and didn't allow the user to pick a
 * different display string for the link.
 */
const LinkPopover: React.FC<LinkPopoverProps> = ({
  initialText,
  initialUrl,
  isEditingExisting,
  triggerRef,
  open,
  onClose,
  onConfirm,
}) => {
  const [url, setUrl] = useState(initialUrl ?? 'https://');
  const [display, setDisplay] = useState(initialText);

  // Reset whenever the popover re-opens with a new context.
  useEffect(() => {
    if (open) {
      setUrl(initialUrl ?? 'https://');
      setDisplay(initialText);
    }
  }, [open, initialText, initialUrl]);

  const isValid = url.trim().length > 0 && /^(https?:\/\/|mailto:|tel:|file:|\/|\.\/|\.\.\/|www\.)/i.test(url.trim());

  return (
    <FormPopover
      triggerRef={triggerRef}
      open={open}
      onClose={onClose}
      title={isEditingExisting ? '编辑超链接' : '插入超链接'}
      titleIcon={<Link2 size={12} />}
      width={340}
      confirmDisabled={!isValid}
      confirmLabel={isEditingExisting ? '应用' : '插入'}
      onConfirm={() => {
        onConfirm(url.trim(), display.trim() || url.trim());
      }}
    >
      <div className={styles.wFormField}>
        <label className={styles.wFormLabel}>地址 (URL)</label>
        <input
          type="text"
          className={styles.wFormInput}
          value={url}
          autoFocus
          placeholder="https://example.com"
          onChange={(e) => setUrl(e.target.value)}
        />
        <div className={styles.wFormHint}>
          支持 http(s)://、mailto:、tel:、file: 以及相对路径
        </div>
      </div>
      <div className={styles.wFormField}>
        <label className={styles.wFormLabel}>显示文字</label>
        <input
          type="text"
          className={styles.wFormInput}
          value={display}
          placeholder={initialText || '链接文字'}
          onChange={(e) => setDisplay(e.target.value)}
        />
      </div>
      <div className={styles.wFormChips}>
        {[
          { label: 'http://', value: 'http://' },
          { label: 'https://', value: 'https://' },
          { label: 'mailto:', value: 'mailto:' },
          { label: 'tel:', value: 'tel:' },
        ].map((p) => (
          <button
            key={p.value}
            type="button"
            className={styles.wFormChip}
            onClick={() => setUrl((u) => (u ? p.value + u.replace(/^\w+:\/\//, '') : p.value))}
            title={p.value}
          >
            {p.label}
          </button>
        ))}
      </div>
    </FormPopover>
  );
};

// ─── MathPopover ──────────────────────────────────────────────────────────────

const MATH_PRESETS = [
  { label: 'x²+y²=r²', latex: 'x^2 + y^2 = r^2' },
  { label: '√(a²+b²)', latex: '\\sqrt{a^2 + b^2}' },
  { label: 'a/b 分数', latex: '\\frac{a}{b}' },
  { label: 'Σ 求和', latex: '\\sum_{i=1}^{n} x_i' },
  { label: '∫ 积分', latex: '\\int_a^b f(x)\\,dx' },
  { label: 'lim 极限', latex: '\\lim_{x \\to 0} \\frac{\\sin x}{x}' },
  { label: '矩阵', latex: '\\begin{bmatrix} a & b \\\\ c & d \\end{bmatrix}' },
  { label: '希腊 αβγ', latex: '\\alpha\\,\\beta\\,\\gamma' },
];

interface MathPopoverProps {
  triggerRef: React.RefObject<HTMLElement | null>;
  open: boolean;
  onClose: () => void;
  /** Confirm handler receives the LaTeX string (without surrounding `$$`). */
  onConfirm: (latex: string) => void;
}

/**
 * Math/LaTeX insertion panel. Provides a text input + preset chips for
 * common equations. Replaces the prior `window.prompt('输入 LaTeX')`.
 */
const MathPopover: React.FC<MathPopoverProps> = ({ triggerRef, open, onClose, onConfirm }) => {
  const [latex, setLatex] = useState('');

  useEffect(() => {
    if (open) setLatex('');
  }, [open]);

  return (
    <FormPopover
      triggerRef={triggerRef}
      open={open}
      onClose={onClose}
      title="插入数学公式 (LaTeX)"
      titleIcon={<Sigma size={12} />}
      width={360}
      confirmDisabled={latex.trim().length === 0}
      onConfirm={() => onConfirm(latex.trim())}
    >
      <div className={styles.wFormField}>
        <label className={styles.wFormLabel}>LaTeX 公式</label>
        <textarea
          className={styles.wFormTextarea}
          rows={3}
          autoFocus
          value={latex}
          placeholder="例如: x^2 + y^2 = r^2"
          onChange={(e) => setLatex(e.target.value)}
        />
        <div className={styles.wFormHint}>
          插入后在文档中显示为 $$…$$,与 Word 的 LaTeX 公式区段一致
        </div>
      </div>
      <div className={styles.wFormField}>
        <label className={styles.wFormLabel}>常用预设</label>
        <div className={styles.wFormChipsWrap}>
          {MATH_PRESETS.map((p) => (
            <button
              key={p.label}
              type="button"
              className={styles.wFormChip}
              onClick={() => setLatex(p.latex)}
              title={p.latex}
            >
              {p.label}
            </button>
          ))}
        </div>
      </div>
    </FormPopover>
  );
};

// ─── WatermarkPopover ─────────────────────────────────────────────────────────

const WATERMARK_COLORS = [
  '#C0C0C0', '#808080', '#404040', '#000000',
  '#D9D9D9', '#F2F2F2', '#FFFFFF',
  '#FF6B6B', '#4ECDC4', '#45B7D1', '#FFA07A',
  '#980000', '#0066CC', '#3D9970', '#FFB400',
];

const WATERMARK_FONTS = [
  'Microsoft YaHei',
  'SimSun',
  'SimHei',
  'KaiTi',
  'Arial',
  'Calibri',
  'Times New Roman',
  'Helvetica',
  'Georgia',
];

interface WatermarkPopoverProps {
  triggerRef: React.RefObject<HTMLElement | null>;
  open: boolean;
  onClose: () => void;
  /** Existing watermark on the doc (if any) — for "replace / remove" controls. */
  currentWatermark: { kind: 'text'; text: string } | { kind: 'picture' } | null;
  /** Confirm handler receives the TextWatermark config (or null to clear). */
  onConfirm: (cfg: {
    text: string;
    font: string;
    color: string;
    semitransparent: boolean;
    layout: 'diagonal' | 'horizontal';
    fontSize: number;
  } | null) => void;
}

/**
 * Watermark settings panel. Builds a full `TextWatermark` object that the
 * editor's `setWatermark` command will accept verbatim. Replaces the prior
 * `window.prompt` (which only collected text and never produced a
 * structurally valid TextWatermark — leading to silent mis-renders).
 */
const WatermarkPopover: React.FC<WatermarkPopoverProps> = ({
  triggerRef,
  open,
  onClose,
  currentWatermark,
  onConfirm,
}) => {
  const isExistingText = currentWatermark?.kind === 'text';
  const [text, setText] = useState('CONFIDENTIAL');
  const [font, setFont] = useState('Calibri');
  const [color, setColor] = useState('#C0C0C0');
  const [semitransparent, setSemitransparent] = useState(true);
  const [layout, setLayout] = useState<'diagonal' | 'horizontal'>('diagonal');
  const [fontSize, setFontSize] = useState<number>(72);

  // Seed defaults from the existing watermark whenever the popover opens.
  useEffect(() => {
    if (!open) return;
    if (isExistingText) {
      setText(currentWatermark.text);
    } else {
      setText('CONFIDENTIAL');
    }
  }, [open, isExistingText, currentWatermark]);

  const canConfirm = text.trim().length > 0;
  const previewTransform = layout === 'diagonal' ? 'rotate(-30deg)' : 'rotate(0deg)';
  const previewOpacity = semitransparent ? 0.5 : 1;

  return (
    <FormPopover
      triggerRef={triggerRef}
      open={open}
      onClose={onClose}
      title="页面水印"
      titleIcon={<Pilcrow size={12} />}
      width={360}
      confirmDisabled={!canConfirm}
      confirmLabel="应用水印"
      onConfirm={() => onConfirm({ text: text.trim(), font, color, semitransparent, layout, fontSize })}
    >
      <div className={styles.wWatermarkPreviewWrap}>
        <div
          className={styles.wWatermarkPreview}
          style={{
            color,
            fontFamily: font,
            fontSize: Math.min(36, Math.max(14, fontSize / 2.5)),
            opacity: previewOpacity,
            transform: previewTransform,
          }}
        >
          {text || '水印预览'}
        </div>
      </div>

      <div className={styles.wFormField}>
        <label className={styles.wFormLabel}>文字内容</label>
        <input
          type="text"
          className={styles.wFormInput}
          value={text}
          autoFocus
          maxLength={64}
          onChange={(e) => setText(e.target.value)}
          placeholder="例如 CONFIDENTIAL / DRAFT"
        />
        <div className={styles.wFormChips}>
          {['CONFIDENTIAL', 'DRAFT', 'DO NOT COPY', '内部资料', '机密'].map((preset) => (
            <button
              key={preset}
              type="button"
              className={styles.wFormChip}
              onClick={() => setText(preset)}
            >
              {preset}
            </button>
          ))}
        </div>
      </div>

      <div className={styles.wFormField}>
        <label className={styles.wFormLabel}>字体</label>
        <select
          className={styles.wFormSelect}
          value={font}
          onChange={(e) => setFont(e.target.value)}
        >
          {WATERMARK_FONTS.map((f) => (
            <option key={f} value={f} style={{ fontFamily: f }}>
              {f}
            </option>
          ))}
        </select>
      </div>

      <div className={styles.wFormField}>
        <label className={styles.wFormLabel}>颜色</label>
        <div className={styles.wWatermarkColorRow}>
          {WATERMARK_COLORS.map((c) => (
            <button
              key={c}
              type="button"
              className={`${styles.wColorSwatch} ${c === color ? styles.wColorSwatchActive : ''}`}
              style={{ background: c.toLowerCase() }}
              title={c}
              onClick={() => setColor(c)}
            />
          ))}
        </div>
      </div>

      <div className={styles.wFormField}>
        <label className={styles.wFormLabel}>布局</label>
        <div className={styles.wFormToggleRow}>
          <button
            type="button"
            className={`${styles.wFormToggle} ${layout === 'diagonal' ? styles.wFormToggleActive : ''}`}
            onClick={() => setLayout('diagonal')}
          >
            倾斜
          </button>
          <button
            type="button"
            className={`${styles.wFormToggle} ${layout === 'horizontal' ? styles.wFormToggleActive : ''}`}
            onClick={() => setLayout('horizontal')}
          >
            水平
          </button>
        </div>
      </div>

      <div className={styles.wFormField}>
        <label className={styles.wFormLabel}>
          字号 {fontSize}pt
        </label>
        <input
          type="range"
          min={24}
          max={144}
          step={6}
          value={fontSize}
          onChange={(e) => setFontSize(Number(e.target.value))}
          className={styles.wFormRange}
        />
      </div>

      <div className={styles.wFormField}>
        <label className={styles.wFormCheckbox}>
          <input
            type="checkbox"
            checked={semitransparent}
            onChange={(e) => setSemitransparent(e.target.checked)}
          />
          <span>半透明 (Word 的"半透明"选项)</span>
        </label>
      </div>

      {currentWatermark && (
        <div className={styles.wFormField}>
          <button
            type="button"
            className={styles.wFormBtnDanger}
            onClick={() => onConfirm(null)}
          >
            <Eraser size={11} />
            <span>移除当前水印</span>
          </button>
        </div>
      )}
    </FormPopover>
  );
};

// ─── HeaderFooterPopover ──────────────────────────────────────────────────────

interface HeaderFooterPopoverProps {
  /** Whether this is for a header or a footer. */
  kind: 'header' | 'footer';
  triggerRef: React.RefObject<HTMLElement | null>;
  open: boolean;
  onClose: () => void;
  onConfirm: (cfg: {
    text: string;
    alignment: 'left' | 'center' | 'right';
    includePageNumber: boolean;
    insertBeforeFirstPage: boolean;
  }) => void;
}

/**
 * Header/footer insertion panel. Lets the user pick the content (custom
 * text / page number / both), the alignment, and whether to also clear
 * the existing first-page header/footer. Replaces the prior `window.prompt`
 * which only asked for a single text string.
 */
const HeaderFooterPopover: React.FC<HeaderFooterPopoverProps> = ({
  kind,
  triggerRef,
  open,
  onClose,
  onConfirm,
}) => {
  const [text, setText] = useState(kind === 'header' ? '页眉' : '页脚');
  const [alignment, setAlignment] = useState<'left' | 'center' | 'right'>('center');
  const [includePageNumber, setIncludePageNumber] = useState(false);
  const [insertBeforeFirstPage, setInsertBeforeFirstPage] = useState(false);

  useEffect(() => {
    if (open) {
      setText(kind === 'header' ? '页眉' : '页脚');
      setAlignment('center');
      setIncludePageNumber(false);
      setInsertBeforeFirstPage(false);
    }
  }, [open, kind]);

  const canConfirm = text.trim().length > 0 || includePageNumber;

  return (
    <FormPopover
      triggerRef={triggerRef}
      open={open}
      onClose={onClose}
      title={kind === 'header' ? '插入页眉' : '插入页脚'}
      titleIcon={kind === 'header' ? <PanelTop size={12} /> : <PanelBottom size={12} />}
      width={340}
      confirmDisabled={!canConfirm}
      confirmLabel={kind === 'header' ? '插入页眉' : '插入页脚'}
      onConfirm={() => onConfirm({ text: text.trim(), alignment, includePageNumber, insertBeforeFirstPage })}
    >
      <div className={styles.wFormField}>
        <label className={styles.wFormLabel}>显示文字</label>
        <input
          type="text"
          className={styles.wFormInput}
          value={text}
          autoFocus
          maxLength={120}
          onChange={(e) => setText(e.target.value)}
          placeholder={kind === 'header' ? '页眉文字 (例如: 公司名称)' : '页脚文字 (例如: 版权信息)'}
        />
        <div className={styles.wFormHint}>
          留空可只插入页码
        </div>
      </div>

      <div className={styles.wFormField}>
        <label className={styles.wFormLabel}>对齐方式</label>
        <div className={styles.wFormToggleRow}>
          {([
            { v: 'left', label: '左对齐' },
            { v: 'center', label: '居中' },
            { v: 'right', label: '右对齐' },
          ] as const).map((opt) => (
            <button
              key={opt.v}
              type="button"
              className={`${styles.wFormToggle} ${alignment === opt.v ? styles.wFormToggleActive : ''}`}
              onClick={() => setAlignment(opt.v)}
            >
              {opt.label}
            </button>
          ))}
        </div>
      </div>

      <div className={styles.wFormField}>
        <label className={styles.wFormCheckbox}>
          <input
            type="checkbox"
            checked={includePageNumber}
            onChange={(e) => setIncludePageNumber(e.target.checked)}
          />
          <span>同时插入页码 (在文字右侧)</span>
        </label>
      </div>

      <div className={styles.wFormField}>
        <label className={styles.wFormCheckbox}>
          <input
            type="checkbox"
            checked={insertBeforeFirstPage}
            onChange={(e) => setInsertBeforeFirstPage(e.target.checked)}
          />
          <span>也在首页显示 (清除首页单独的{kind === 'header' ? '页眉' : '页脚'})</span>
        </label>
      </div>
    </FormPopover>
  );
};

// ─── WordToolbar ──────────────────────────────────────────────────────────────

export interface WordToolbarProps {
  view: EditorView | null;
  fileName: string;
  isDirty: boolean;
  isLoading: boolean;
  mode: 'editing' | 'suggesting' | 'viewing';
  onModeChange: (m: 'editing' | 'suggesting' | 'viewing') => void;
  onSave: () => void;
  canSave: boolean;
  onTriggerAI: () => void;
  onFind: () => void;
  onReplace?: () => void;
  /** Imperative actions that bypass the editor. */
  setZoom: (z: number) => void;
  getZoom: () => number;
  print: () => void;
  /**
   * Imperative editor handle — used for undo/redo, programmatic document
   * edits that don't have a ProseMirror command (page color, header/footer
   * insertion), and any future imperative surfaces.
   */
  editor?: {
    undo: () => boolean;
    redo: () => boolean;
    /** Read the current document model so callers can mutate + reload it. */
    getDocument?: () => unknown | null;
    /** Push a mutated document back into the editor. */
    loadDocument?: (doc: unknown) => void;
  } | null;
  /**
   * Notification sink for actions that surface user-visible errors (failed
   * page-color updates, header/footer failures, etc.). Optional — when not
   * provided we just `console.error`.
   */
  notify?: (kind: 'error' | 'info', message: string) => void;
}

export const WordToolbar: React.FC<WordToolbarProps> = ({
  view,
  fileName,
  isDirty,
  isLoading,
  mode,
  onModeChange,
  onSave,
  canSave,
  onTriggerAI,
  onFind,
  onReplace,
  setZoom,
  getZoom,
  print,
  editor = null,
  notify,
}) => {
  const [, setTick] = useState(0);
  const [zoomLevel, setZoomLevel] = useState(1);

  // Poll editor state to refresh toolbar active-state every 250 ms.
  useEffect(() => {
    if (!view) return;
    const handle = window.setInterval(() => setTick((t) => t + 1), 250);
    return () => window.clearInterval(handle);
  }, [view]);

  useEffect(() => {
    setZoomLevel(getZoom() || 1);
  }, [getZoom]);

  // ── Schema marks ──────────────────────────────────────────────────────────
  const markTypes = useMemo(() => {
    const s = view?.state.schema;
    if (!s) return null;
    const names = [
      'bold', 'italic', 'underline', 'strike',
      'superscript', 'subscript',
      'fontSize', 'fontFamily', 'textColor', 'highlight',
      'hyperlink',
    ];
    const m: Record<string, MarkType | null> = {};
    for (const n of names) m[n] = s.marks[n] ?? null;
    return m;
  }, [view]);

  // ── Active-state queries ──────────────────────────────────────────────────
  const state = view?.state;
  const schemaReady = !!state?.schema;
  const isActive = (name: string): boolean => {
    const mt = markTypes?.[name];
    if (!schemaReady || !mt) return false;
    return isMarkActive(state!, mt);
  };

  const isBold = isActive('bold');
  const isItalic = isActive('italic');
  const isUnderline = isActive('underline');
  const isStrike = isActive('strike');
  const isSuper = isActive('superscript');
  const isSub = isActive('subscript');
  const isLink = schemaReady ? isHyperlinkActive(state!) : false;
  const alignment = schemaReady ? getParagraphAlignment(state!) : null;
  const styleId = schemaReady ? getStyleId(state!) : null;
  const fontSizeHp = (markTypes?.fontSize && schemaReady) ? getMarkAttr(state!, markTypes.fontSize, 'size') : null;
  const fontFamily = (markTypes?.fontFamily && schemaReady) ? getMarkAttr(state!, markTypes.fontFamily, 'ascii') : null;
  const textColor = (markTypes?.textColor && schemaReady) ? getMarkAttr(state!, markTypes.textColor, 'rgb') : null;
  const currentFontSizePt = hpToPt(fontSizeHp) ?? 12;
  const currentFontColor = rgbToHex(textColor) ?? '#000000';

  // ── Handlers ───────────────────────────────────────────────────────────────
  const handleFontFamily = useCallback(
    (v: string) => runCommand(view, setFontFamily(v)),
    [view],
  );
  const handleFontSize = useCallback(
    (pt: number) => runCommand(view, setFontSize(Math.round(pt * 2))),
    [view],
  );
  const handleFontSizeStep = useCallback(
    (delta: number) => {
      const next = Math.max(1, Math.min(400, Math.round(currentFontSizePt + delta)));
      runCommand(view, setFontSize(next * 2));
    },
    [view, currentFontSizePt],
  );

  const handleFontColor = useCallback(
    (hex: string) => runCommand(view, setTextColor({ rgb: hex.replace('#', '') })),
    [view],
  );
  const handleHighlight = useCallback(
    (color: string) => (color === 'none' ? runCommand(view, clearHighlight) : runCommand(view, setHighlight(color))),
    [view],
  );

  const handleStyleChange = useCallback(
    (id: string) => {
      if (id === 'Normal') runCommand(view, clearStyle);
      else runCommand(view, applyStyle(id));
    },
    [view],
  );

  // ── Clipboard ──────────────────────────────────────────────────────────────
  const handleCopy = useCallback(() => {
    document.execCommand('copy');
  }, []);
  const handleCut = useCallback(() => {
    document.execCommand('cut');
  }, []);
  const handlePaste = useCallback(async () => {
    try {
      const text = await navigator.clipboard.readText();
      if (isViewReady(view) && text) {
        view.dispatch(view.state.tr.insertText(text, view.state.selection.from, view.state.selection.to));
        view.focus();
      }
    } catch {
      // Fall back to native paste via execCommand.
      document.execCommand('paste');
    }
  }, [view]);

  // ── Insert ────────────────────────────────────────────────────────────────
  const handleInsertTable = useCallback(
    (rows: number, cols: number) => runCommand(view, insertTable(rows, cols)),
    [view],
  );
  const handleInsertImage = useCallback(() => {
    const input = document.createElement('input');
    input.type = 'file';
    input.accept = 'image/*';
    input.onchange = () => {
      const file = input.files?.[0];
      if (file && view) insertImageFromFile(view, file);
    };
    input.click();
  }, [view]);
  const linkTriggerRef = useRef<HTMLButtonElement>(null);
  const [linkOpen, setLinkOpen] = useState(false);
  const [linkInitialText, setLinkInitialText] = useState('');
  const [linkInitialUrl, setLinkInitialUrl] = useState<string | undefined>(undefined);
  const [linkEditing, setLinkEditing] = useState(false);
  const handleInsertLink = useCallback(() => {
    const sel = window.getSelection()?.toString() ?? '';
    setLinkInitialText(sel);
    setLinkInitialUrl(undefined);
    setLinkEditing(!!isLink);
    setLinkOpen(true);
  }, [isLink]);
  const handleRemoveLink = useCallback(() => runCommand(view, removeHyperlink), [view]);
  const handleLinkConfirm = useCallback(
    (url: string, displayText: string) => {
      // Insert or update the hyperlink on the current selection. If the user
      // changed the display text while editing, replace the selection's text
      // content with `displayText` first, then apply the mark.
      if (!isViewReady(view)) {
        setLinkOpen(false);
        return;
      }
      const { from, to } = view.state.selection;
      if (from !== to && view.state.doc.textBetween(from, to, '\n', '\n') !== displayText) {
        view.dispatch(view.state.tr.insertText(displayText, from, to));
      }
      // After a text replace the selection might have collapsed; recompute.
      const sel2 = view.state.selection;
      if (sel2.from === sel2.to) {
        runCommand(view, insertHyperlink(url, displayText));
      } else {
        runCommand(view, setHyperlink(url));
      }
      view.focus();
      setLinkOpen(false);
    },
    [view],
  );
  const handleInsertSymbol = useCallback(
    (sym: string) => {
      if (!isViewReady(view)) return;
      view.dispatch(view.state.tr.insertText(sym, view.state.selection.from, view.state.selection.to));
    },
    [view],
  );

  // ── Selection ─────────────────────────────────────────────────────────────
  const handleSelectAll = useCallback(() => {
    document.execCommand('selectAll');
  }, []);
  const handleClearFormatting = useCallback(() => runCommand(view, clearFormatting), [view]);

  // ── Watermark ─────────────────────────────────────────────────────────────
  // `setWatermark` accepts a `TextWatermark` config object:
  //   { kind: 'text', text, font, color, semitransparent, layout, fontSize }
  // The previous handler passed `{ text, color: { rgb }, angle: -45 }` which
  // didn't match the API — the watermark stored on the doc would silently
  // fail to render. The new WatermarkPopover collects a full, well-typed
  // config and we pass it through verbatim.
  const watermarkTriggerRef = useRef<HTMLButtonElement>(null);
  const [watermarkOpen, setWatermarkOpen] = useState(false);
  const currentWatermark = useMemo(() => {
    if (!isViewReady(view)) return null;
    try {
      const w = (view.state.doc as unknown as { attrs?: { watermark?: unknown } }).attrs?.watermark;
      if (!w || typeof w !== 'object') return null;
      const obj = w as { kind?: string; text?: string };
      if (obj.kind === 'text' && typeof obj.text === 'string') {
        return { kind: 'text' as const, text: obj.text };
      }
      if (obj.kind === 'picture') {
        return { kind: 'picture' as const };
      }
      return null;
    } catch {
      return null;
    }
  }, [view]);
  const handleWatermark = useCallback(() => {
    if (!view) return;
    setWatermarkOpen(true);
  }, [view]);
  const handleWatermarkConfirm = useCallback(
    (cfg: {
      text: string;
      font: string;
      color: string;
      semitransparent: boolean;
      layout: 'diagonal' | 'horizontal';
      fontSize: number;
    } | null) => {
      setWatermarkOpen(false);
      if (!view) return;
      if (cfg === null) {
        runCommand(view, setWatermark(null as any));
        return;
      }
      runCommand(view, setWatermark({
        kind: 'text',
        text: cfg.text,
        font: cfg.font,
        color: cfg.color,
        semitransparent: cfg.semitransparent,
        layout: cfg.layout,
        fontSize: cfg.fontSize,
      } as any));
    },
    [view],
  );

  // ── History (undo / redo) ────────────────────────────────────────────────
  // The PM history plugin is wired into the editor by DocxEditor; the
  // PagedEditorRef surfaces `undo()` / `redo()` so the toolbar buttons can
  // drive it without re-implementing history.
  const handleUndo = useCallback(() => {
    if (!editor) return;
    editor.undo();
    view?.focus();
  }, [editor, view]);
  const handleRedo = useCallback(() => {
    if (!editor) return;
    editor.redo();
    view?.focus();
  }, [editor, view]);

  // ── Format painter ───────────────────────────────────────────────────────
  // Click 1: copy the marks of the current selection's anchor and enter
  //   "armed" state (Brush button highlights).
  // Click 2+: apply those marks across a new selection, then disarm.
  // ESC: cancel without applying.
  const [paintedMarks, setPaintedMarks] = useState<readonly Mark[] | null>(null);
  const applyPaintedMarks = useCallback(
    (from: number, to: number) => {
      if (!view || !paintedMarks || paintedMarks.length === 0) return;
      const tr = view.state.tr;
      // Strip every mark we are about to paint, then add the painted set
      // (PM has no bulk-remove-marks helper, so iterate types).
      const targetTypes = new Set(paintedMarks.map((m) => m.type));
      tr.removeMark(from, to, ...targetTypes);
      for (const mark of paintedMarks) {
        tr.addMark(from, to, mark);
      }
      view.dispatch(tr);
      view.focus();
    },
    [view, paintedMarks],
  );
  const handleFormatPainter = useCallback(() => {
    if (!isViewReady(view)) return;
    if (paintedMarks) {
      // Already armed — second click applies to current selection and disarms.
      applyPaintedMarks(view.state.selection.from, view.state.selection.to);
      setPaintedMarks(null);
      return;
    }
    const $from = view.state.selection.$from;
    const marks: readonly Mark[] = $from.marks();
    if (marks.length === 0) {
      // Nothing to copy from a caret sitting in plain text — try the parent
      // paragraph's stored marks so the user can still paint a clean style.
      const parent = $from.parent;
      const firstRun = parent.childAfter(0);
      if (firstRun.node && firstRun.node.marks.length > 0) {
        setPaintedMarks(firstRun.node.marks);
      } else {
        notify?.('info', '请先选择带格式的文字,再使用格式刷');
        return;
      }
    } else {
      setPaintedMarks(marks);
    }
    view.focus();
  }, [view, paintedMarks, applyPaintedMarks, notify]);
  // Cancel format painter on Escape while armed.
  useEffect(() => {
    if (!paintedMarks) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.stopPropagation();
        setPaintedMarks(null);
      }
    };
    window.addEventListener('keydown', onKey, true);
    return () => window.removeEventListener('keydown', onKey, true);
  }, [paintedMarks]);

  // ── Sort (paragraph text) ────────────────────────────────────────────────
  // Collect the text of every paragraph fully or partially covered by the
  // current selection, replace each paragraph's runs with a single run of
  // the sorted text, preserving the paragraph's marks.
  const sortSelection = useCallback(
    (direction: 'asc' | 'desc') => {
      if (!isViewReady(view)) return;
      const { state, dispatch } = view;
      const { from, to } = state.selection;
      if (from === to) {
        notify?.('info', '请先选择要排序的段落');
        return;
      }
      // Walk paragraph nodes intersecting the selection in document order.
      const collected: Array<{ pos: number; node: PMNode; start: number; end: number; text: string }> = [];
      state.doc.nodesBetween(from, to, (node, pos) => {
        if (node.isTextblock) {
          const start = Math.max(pos, from);
          const end = Math.min(pos + node.nodeSize, to);
          const text = node.textBetween(start - pos, end - pos, '\n', '\n');
          collected.push({ pos, node, start, end, text });
        }
        return true;
      });
      if (collected.length < 2) {
        notify?.('info', '至少需要两段内容才能排序');
        return;
      }
      // Capture per-paragraph formatting from the first run so the replacement
      // text adopts it (otherwise the sort looks visually wrong).
      const lines = collected.map((c) => {
        const firstRun = c.node.childAfter(0)?.node;
        const marks = firstRun && firstRun.isText ? firstRun.marks : [];
        return { ...c, marks };
      });
      const sorted = [...lines].sort((a, b) =>
        direction === 'asc'
          ? a.text.localeCompare(b.text, 'zh-Hans-CN')
          : b.text.localeCompare(a.text, 'zh-Hans-CN'),
      );
      const tr = state.tr;
      // Walk in reverse so earlier positions remain stable as we mutate.
      for (let i = lines.length - 1; i >= 0; i--) {
        const target = lines[i];
        const replacement = sorted[i];
        // Replace the slice of paragraph text between [start, end] with the
        // new text. We keep the surrounding paragraph structure intact.
        const fromInPara = target.start - target.pos;
        const toInPara = target.end - target.pos;
        if (fromInPara === toInPara) continue;
        // Remove old range.
        tr.delete(target.start, target.end);
        // Insert new text at the deletion point. If we have captured marks,
        // open a TextSelection at the deletion site so the typed text picks
        // them up via storedMarks.
        const insertAt = target.start;
        tr.insertText(replacement.text, insertAt);
        // Apply captured marks across the inserted slice.
        if (replacement.marks.length > 0) {
          // After insertion the new text runs from `insertAt` to
          // `insertAt + replacement.text.length`.
          for (const mark of replacement.marks) {
            tr.addMark(insertAt, insertAt + replacement.text.length, mark);
          }
        }
      }
      dispatch(tr);
      view.focus();
    },
    [view, notify],
  );

  // ── Line spacing (handles all values, not just 1 / 1.5 / 2) ──────────────
  const handleLineSpacing = useCallback(
    (v: string) => {
      const n = Number(v);
      if (!Number.isFinite(n) || n <= 0) return;
      // `setLineSpacing` accepts any positive number of lines; falls back to
      // the three presets if someone passes an exact match so callers don't
      // have to know which command implementation is which.
      if (n === 1) runCommand(view, singleSpacing);
      else if (n === 1.5) runCommand(view, oneAndHalfSpacing);
      else if (n === 2) runCommand(view, doubleSpacing);
      else runCommand(view, setLineSpacing(n));
    },
    [view],
  );

  // ── Math formula ─────────────────────────────────────────────────────────
  // The editor doesn't yet ship a native equation editor, so we insert the
  // LaTeX wrapped in `$$ ... $$`. The user composes the formula via a
  // in-app panel (MathPopover) — better than `window.prompt` because we can
  // surface preset equations and avoid OS-level dialog styling.
  const mathTriggerRef = useRef<HTMLButtonElement>(null);
  const [mathOpen, setMathOpen] = useState(false);
  const handleInsertMath = useCallback(() => {
    if (!isViewReady(view)) return;
    setMathOpen(true);
  }, [view]);
  const handleMathConfirm = useCallback(
    (latex: string) => {
      if (!isViewReady(view)) {
        setMathOpen(false);
        return;
      }
      const { from, to } = view.state.selection;
      view.dispatch(view.state.tr.insertText(`$$${latex}$$`, from, to));
      view.focus();
      setMathOpen(false);
    },
    [view],
  );

  // ── Page color ───────────────────────────────────────────────────────────
  // The PM model doesn't carry section properties directly, so we go through
  // the document model: read the current Document, set the background on the
  // final section properties, push it back. Page color is a doc-level
  // property in OOXML, so undoing through PM history doesn't capture it —
  // we still let the user pick "无颜色" via the existing `none` color string.
  const PAGE_COLOR_PALETTE = [
    '#FFFFFF', '#F2F2F2', '#D9D9D9', '#BFBFBF', '#A6A6A6', '#808080',
    '#FFF2CC', '#FFE699', '#FFD966', '#F4B183', '#C00000', '#E06666',
    '#E2EFDA', '#C6E0B4', '#A9D08E', '#70AD47', '#385723', '#1F4E79',
    '#DEEBF7', '#BDD7EE', '#9DC3E6', '#5B9BD5', '#2E75B6', '#1F3864',
  ];
  const handlePageColor = useCallback(
    (color: string) => {
      if (!editor?.getDocument || !editor?.loadDocument) {
        notify?.('error', '页面颜色需要编辑器支持,当前不可用');
        return;
      }
      const doc = editor.getDocument() as {
        body?: {
          finalSectionProperties?: { background?: { color?: { rgb?: string } } };
        };
      } | null;
      if (!doc || !doc.body) {
        notify?.('error', '无法读取文档模型,无法设置页面颜色');
        return;
      }
      const next = JSON.parse(JSON.stringify(doc)) as typeof doc;
      if (!next.body) next.body = {};
      if (!next.body.finalSectionProperties) next.body.finalSectionProperties = {};
      if (color === 'none' || !color) {
        delete next.body.finalSectionProperties.background;
      } else {
        next.body.finalSectionProperties.background = {
          color: { rgb: color.replace('#', '').toUpperCase() },
        };
      }
      try {
        editor.loadDocument(next);
      } catch (e) {
        notify?.('error', `设置页面颜色失败: ${(e as Error).message}`);
      }
    },
    [editor, notify],
  );

  // ── Header / Footer ──────────────────────────────────────────────────────
  // The header/footer is a part of the OOXML package referenced from the
  // final section properties. We synthesize an empty header/footer part,
  // register it on the document, and reload — the editor's HF UI will
  // surface it. The HeaderFooterPopover collects alignment + page-number
  // options in addition to text; we fold all of that into the paragraph
  // run so the part renders the user's intent immediately.
  const headerTriggerRef = useRef<HTMLButtonElement>(null);
  const footerTriggerRef = useRef<HTMLButtonElement>(null);
  const [headerOpen, setHeaderOpen] = useState(false);
  const [footerOpen, setFooterOpen] = useState(false);
  const handleInsertHeader = useCallback(() => {
    if (!editor) return;
    setHeaderOpen(true);
  }, [editor]);
  const handleInsertFooter = useCallback(() => {
    if (!editor) return;
    setFooterOpen(true);
  }, [editor]);
  const handleHeaderFooterConfirm = useCallback(
    (
      kind: 'header' | 'footer',
      cfg: {
        text: string;
        alignment: 'left' | 'center' | 'right';
        includePageNumber: boolean;
        insertBeforeFirstPage: boolean;
      },
    ) => {
      if (kind === 'header') setHeaderOpen(false);
      else setFooterOpen(false);
      if (!editor?.getDocument || !editor?.loadDocument) {
        notify?.('error', '页眉页脚需要编辑器支持,当前不可用');
        return;
      }
      const doc = editor.getDocument() as null | {
        body?: {
          finalSectionProperties?: {
            headerReferences?: Array<{ type: string; rId: string }>;
            footerReferences?: Array<{ type: string; rId: string }>;
            titlePage?: boolean;
          };
        };
        headers?: Map<string, unknown> | Record<string, unknown>;
        footers?: Map<string, unknown> | Record<string, unknown>;
      };
      if (!doc || !doc.body) {
        notify?.('error', '无法读取文档模型');
        return;
      }
      const rId = `rId${kind}-${Date.now()}`;

      // Build the runs for the header/footer paragraph. If `includePageNumber`
      // is on, append a "PAGE" placeholder run after the text — the editor's
      // HF UI renders that as an actual page-number field at save time.
      const runs: Array<Record<string, unknown>> = [];
      if (cfg.text) {
        runs.push({ text: cfg.text, type: 'run' });
      }
      if (cfg.includePageNumber) {
        if (runs.length > 0) {
          runs.push({ text: ' ', type: 'run' });
        }
        runs.push({ text: 'PAGE', type: 'field', fieldType: 'PAGE' });
      }
      if (runs.length === 0) {
        // Safety: nothing to write.
        return;
      }
      const newPart = {
        type: kind,
        hdrFtrType: 'default',
        content: [
          {
            type: 'paragraph',
            alignment: cfg.alignment,
            runs,
          },
        ],
      };

      const { headers, footers, ...rest } = doc;
      const next = JSON.parse(JSON.stringify(rest)) as typeof doc;
      if (!next.body) next.body = {};
      if (!next.body.finalSectionProperties) next.body.finalSectionProperties = {};
      const partsMap = new Map<string, unknown>(
        kind === 'header'
          ? headers instanceof Map
            ? Array.from(headers.entries())
            : Object.entries(headers ?? {})
          : footers instanceof Map
            ? Array.from(footers.entries())
            : Object.entries(footers ?? {}),
      );
      partsMap.set(rId, newPart);
      if (kind === 'header') {
        if (!next.body.finalSectionProperties.headerReferences) {
          next.body.finalSectionProperties.headerReferences = [];
        }
        next.body.finalSectionProperties.headerReferences.push({
          type: 'default',
          rId,
        });
        next.headers = partsMap;
      } else {
        if (!next.body.finalSectionProperties.footerReferences) {
          next.body.finalSectionProperties.footerReferences = [];
        }
        next.body.finalSectionProperties.footerReferences.push({
          type: 'default',
          rId,
        });
        next.footers = partsMap;
      }
      // "Insert before first page" → clear the firstPage reference if any.
      if (cfg.insertBeforeFirstPage) {
        next.body.finalSectionProperties.titlePage = false;
        if (kind === 'header') {
          const refs = next.body.finalSectionProperties.headerReferences ?? [];
          next.body.finalSectionProperties.headerReferences =
            refs.filter((r) => r.type !== 'first');
        } else {
          const refs = next.body.finalSectionProperties.footerReferences ?? [];
          next.body.finalSectionProperties.footerReferences =
            refs.filter((r) => r.type !== 'first');
        }
      }
      try {
        editor.loadDocument(next);
      } catch (e) {
        notify?.('error', `插入${kind === 'header' ? '页眉' : '页脚'}失败: ${(e as Error).message}`);
      }
    },
    [editor, notify],
  );

  // ── Spell check toggle ───────────────────────────────────────────────────
  // The ProseMirror editor doesn't bundle a Hunspell pipeline, but every
  // browser ships a native spellcheck that we can switch on by toggling the
  // `spellcheck` attribute on the editor's contenteditable root. While the
  // editor is mounted, the docx container holds the editable element.
  const [spellCheckOn, setSpellCheckOn] = useState(false);
  const handleToggleSpellCheck = useCallback(() => {
    const root = document.querySelector<HTMLElement>('[data-office-editor-root="word"]');
    if (!root) {
      notify?.('error', '找不到编辑器容器');
      return;
    }
    const next = !spellCheckOn;
    setSpellCheckOn(next);
    // `contenteditable` nodes expose spellcheck on their descendants; flip it
    // on every editable surface inside the root.
    const editable = root.querySelectorAll<HTMLElement>(
      '[contenteditable="true"], .ProseMirror, [spellcheck]',
    );
    editable.forEach((el) => {
      el.setAttribute('spellcheck', next ? 'true' : 'false');
    });
    notify?.('info', next ? '已开启浏览器拼写检查' : '已关闭拼写检查');
  }, [spellCheckOn, notify]);

  // ── Zoom ──────────────────────────────────────────────────────────────────
  const handleZoomIn = () => {
    const next = Math.min(5, +(zoomLevel + 0.1).toFixed(2));
    setZoom(next);
    setZoomLevel(next);
  };
  const handleZoomOut = () => {
    const next = Math.max(0.25, +(zoomLevel - 0.1).toFixed(2));
    setZoom(next);
    setZoomLevel(next);
  };
  const handleZoomCommit = (value: string) => {
    const n = Number(value);
    if (Number.isFinite(n) && n > 0 && n <= 500) {
      const z = n / 100;
      setZoom(z);
      setZoomLevel(z);
    }
  };

  // ── Page color / page border ──────────────────────────────────────────────
  const handleShowFormattingMarks = useCallback(() => {
    // Toggle formatting marks via ProseMirror - typically via config on DocxEditor.
    // Without direct API surface we toggle a CSS class on the editor container.
    const el = document.querySelector('[data-office-editor-root="word"]');
    if (!el) return;
    el.classList.toggle('inkuo-show-formatting-marks');
  }, []);

  return (
    <div className={styles.wToolbarRoot}>
      {/* ═══════════ Row 1: clipboard · font · marks · colors ═══════════════ */}
      <div className={styles.wToolbarScroll}>
        {/* Save group (always visible) */}
        <div className={styles.wToolbarGroup}>
          <span className={styles.wToolbarFileName} title={fileName}>
            {fileName}
            {isDirty && <span className={styles.wToolbarDirtyDot}>●</span>}
          </span>
          {isLoading && <span className={styles.wToolbarStatusPill}>加载中</span>}
        </div>
        <span className={styles.wToolbarGroupSep} />

        {/* History */}
        <div className={styles.wToolbarGroup}>
          <IconButton
            icon={Undo2}
            title="撤销 (Ctrl+Z)"
            disabled={!view || !editor}
            onClick={handleUndo}
          />
          <IconButton
            icon={Redo2}
            title="重做 (Ctrl+Y)"
            disabled={!view || !editor}
            onClick={handleRedo}
          />
        </div>
        <span className={styles.wToolbarGroupSep} />

        {/* Clipboard */}
        <div className={styles.wToolbarGroup}>
          <IconButton icon={Clipboard} title="粘贴 (Ctrl+V)" disabled={!view} onClick={handlePaste} />
          <IconButton icon={Scissors} title="剪切 (Ctrl+X)" disabled={!view} onClick={handleCut} />
          <IconButton icon={CopyIcon} title="复制 (Ctrl+C)" disabled={!view} onClick={handleCopy} />
          <IconButton
            icon={Brush}
            title={paintedMarks ? '格式刷 (再次点击应用,Esc 取消)' : '格式刷 (复制选区格式)'}
            disabled={!view}
            active={!!paintedMarks}
            onClick={handleFormatPainter}
          />
        </div>
        <span className={styles.wToolbarGroupSep} />

        {/* Font */}
        <div className={styles.wToolbarGroup}>
          <Dropdown
            value={(fontFamily as string) ?? 'Microsoft YaHei'}
            onChange={handleFontFamily}
            title="字体"
            width={118}
            displayValue={(fontFamily as string) ?? '默认字体'}
            icon={Type}
            options={FONT_FAMILIES.map((f) => ({ value: f, label: f }))}
          />
          <FontSizeControl
            value={currentFontSizePt}
            onChange={handleFontSize}
            onStep={handleFontSizeStep}
            disabled={!view}
          />
        </div>
        <span className={styles.wToolbarGroupSep} />

        {/* Marks */}
        <div className={styles.wToolbarGroup}>
          <IconButton icon={Bold} title="加粗 (Ctrl+B)" active={isBold} onClick={() => runCommand(view, toggleBold)} />
          <IconButton icon={Italic} title="斜体 (Ctrl+I)" active={isItalic} onClick={() => runCommand(view, toggleItalic)} />
          <IconButton icon={UnderlineIcon} title="下划线 (Ctrl+U)" active={isUnderline} onClick={() => runCommand(view, toggleUnderline)} />
          <IconButton icon={Strikethrough} title="删除线" active={isStrike} onClick={() => runCommand(view, toggleStrike)} />
          <IconButton
            icon={() => <span style={{ fontSize: 11, fontWeight: 700, fontStyle: 'italic' }}>X²</span>}
            title="上标"
            active={isSuper}
            onClick={() => runCommand(view, toggleSuperscript)}
          />
          <IconButton
            icon={() => <span style={{ fontSize: 11, fontWeight: 700, fontStyle: 'italic' }}>X₂</span>}
            title="下标"
            active={isSub}
            onClick={() => runCommand(view, toggleSubscript)}
          />
        </div>
        <span className={styles.wToolbarGroupSep} />

        {/* Colors */}
        <div className={styles.wToolbarGroup}>
          <ColorPicker colors={TEXT_COLORS} fontColor={currentFontColor} onChange={handleFontColor} title="字体颜色" />
          <ColorPicker colors={HIGHLIGHT_COLORS} onChange={handleHighlight} highlight title="文字底色" />
          <IconButton icon={Eraser} title="清除格式" disabled={!view} onClick={handleClearFormatting} />
          <IconButton icon={Pilcrow} title="显示/隐藏格式标记" onClick={handleShowFormattingMarks} />
        </div>
        <span className={styles.wToolbarGroupSep} />

        {/* Paragraph styles — last group of Row 1 */}
        <div className={styles.wToolbarGroup}>
          <Dropdown
            value={styleId ?? 'Normal'}
            onChange={handleStyleChange}
            title="段落样式"
            width={120}
            icon={Heading1}
            options={PARAGRAPH_STYLES}
            displayValue={PARAGRAPH_STYLES.find((s) => s.value === (styleId ?? 'Normal'))?.label ?? '正文'}
          />
        </div>
      </div>
      {/* ── end of Row 1 ── */}

      {/* ═══════════ Row 2: alignment · lists · insert · page · find · zoom ═══ */}
      <div className={styles.wToolbarScroll}>
        {/* Alignment */}
        <div className={styles.wToolbarGroup}>
          <IconButton icon={AlignLeft} title="左对齐" active={alignment === 'left'} onClick={() => runCommand(view, alignLeft)} />
          <IconButton icon={AlignCenter} title="居中" active={alignment === 'center'} onClick={() => runCommand(view, alignCenter)} />
          <IconButton icon={AlignRight} title="右对齐" active={alignment === 'right'} onClick={() => runCommand(view, alignRight)} />
          <IconButton icon={AlignJustify} title="两端对齐" active={alignment === 'both'} onClick={() => runCommand(view, alignJustify)} />
        </div>
        <span className={styles.wToolbarGroupSep} />

        {/* Indent / lists / line spacing */}
        <div className={styles.wToolbarGroup}>
          <IconButton icon={List} title="项目符号列表" onClick={() => runCommand(view, toggleBulletList)} />
          <IconButton icon={ListOrdered} title="编号列表" onClick={() => runCommand(view, toggleNumberedList)} />
          <IconButton icon={IndentDecrease} title="减少缩进" onClick={() => runCommand(view, decreaseIndent())} />
          <IconButton icon={IndentIncrease} title="增加缩进" onClick={() => runCommand(view, increaseIndent())} />
          <IconButton icon={ArrowDownAZ} title="降序排序" disabled={!view} onClick={() => sortSelection('desc')} />
          <IconButton icon={ArrowUpAZ} title="升序排序" disabled={!view} onClick={() => sortSelection('asc')} />
          <Dropdown
            value="1"
            onChange={handleLineSpacing}
            title="行间距"
            width={68}
            options={LINE_SPACING_OPTIONS}
            displayValue="行距"
            icon={WrapText}
          />
        </div>
        <span className={styles.wToolbarGroupSep} />

        {/* Insert */}
        <div className={styles.wToolbarGroup}>
          <TablePicker onInsert={handleInsertTable} />
          <IconButton icon={ImageIcon} title="插入图片" onClick={handleInsertImage} />
          <IconButton icon={Link2} title="插入超链接" active={isLink} onClick={handleInsertLink} buttonRef={linkTriggerRef} />
          {isLink && (
            <button
              type="button"
              className={styles.wToolbarTextBtn}
              onClick={handleRemoveLink}
              title="移除链接"
            >
              取消链接
            </button>
          )}
          <SymbolPicker onInsert={handleInsertSymbol} />
          <IconButton icon={Sigma} title="插入数学公式 (LaTeX)" disabled={!view} onClick={handleInsertMath} buttonRef={mathTriggerRef} />
        </div>
        <span className={styles.wToolbarGroupSep} />

        {/* Page / Document */}
        <div className={styles.wToolbarGroup}>
          <PageColorPicker
            colors={PAGE_COLOR_PALETTE}
            onChange={handlePageColor}
            disabled={!editor}
            title="页面颜色"
          />
          <IconButton
            icon={PanelTop}
            title="插入页眉"
            disabled={!editor}
            onClick={handleInsertHeader}
            buttonRef={headerTriggerRef}
          />
          <IconButton
            icon={PanelBottom}
            title="插入页脚"
            disabled={!editor}
            onClick={handleInsertFooter}
            buttonRef={footerTriggerRef}
          />
          <IconButton
            icon={SpellCheck2}
            title={spellCheckOn ? '关闭拼写检查' : '开启拼写检查 (浏览器原生)'}
            active={spellCheckOn}
            onClick={handleToggleSpellCheck}
          />
          <IconButton icon={Pilcrow} title="水印" disabled={!view} onClick={handleWatermark} buttonRef={watermarkTriggerRef} />
          <button
            type="button"
            className={styles.wToolbarTextBtn}
            title="插入分页符 (Ctrl+Enter)"
            onClick={() => runCommand(view, insertPageBreak)}
          >
            分页
          </button>
        </div>
        <span className={styles.wToolbarGroupSep} />

        {/* Find / Replace */}
        <div className={styles.wToolbarGroup}>
          <IconButton icon={Search} title="查找 (Ctrl+F)" onClick={onFind} />
          {onReplace && <IconButton icon={Replace} title="替换 (Ctrl+H)" onClick={onReplace} />}
          <IconButton icon={Eraser} title="全选 (Ctrl+A)" onClick={handleSelectAll} />
        </div>
        <span className={styles.wToolbarGroupSep} />

        {/* Zoom */}
        <div className={styles.wToolbarGroup}>
          <IconButton icon={ZoomOut} title="缩小" onClick={handleZoomOut} />
          <Dropdown
            value={String(Math.round(zoomLevel * 100))}
            onChange={handleZoomCommit}
            title="缩放"
            width={60}
            options={ZOOM_LEVELS.map((z) => ({ value: String(Math.round(z * 100)), label: `${Math.round(z * 100)}%` }))}
            displayValue={`${Math.round(zoomLevel * 100)}%`}
          />
          <IconButton icon={ZoomIn} title="放大" onClick={handleZoomIn} />
        </div>
        <span className={styles.wToolbarGroupSep} />

        {/* AI + mode + save + print */}
        <div className={styles.wToolbarGroup}>
          <button
            type="button"
            className={styles.wToolbarTextBtn}
            onClick={onTriggerAI}
            title="AI 补全 (Tab 接受 / Esc 拒绝)"
          >
            <Sparkles size={12} />
            <span>AI 补全</span>
          </button>
          <Dropdown
            value={mode}
            onChange={(v) => onModeChange(v as any)}
            title="编辑模式"
            width={68}
            icon={PencilLine}
            displayValue={
              mode === 'editing' ? '编辑' : mode === 'suggesting' ? '修订' : '只读'
            }
            options={[
              { value: 'editing', label: '编辑' },
              { value: 'suggesting', label: '修订' },
              { value: 'viewing', label: '只读' },
            ]}
          />
          <button
            type="button"
            className={`${styles.wToolbarSaveBtn} ${isDirty ? styles.wToolbarSaveBtnDirty : ''}`}
            onClick={onSave}
            disabled={!canSave}
            title="保存 (Ctrl+S)"
          >
            <Save size={12} />
            <span>保存</span>
          </button>
          <IconButton icon={Printer} title="打印" onClick={print} />
        </div>
      </div>

      {/* ── Settings popovers (rendered through React Portal) ── */}
      <LinkPopover
        triggerRef={linkTriggerRef}
        open={linkOpen}
        onClose={() => setLinkOpen(false)}
        initialText={linkInitialText}
        initialUrl={linkInitialUrl}
        isEditingExisting={linkEditing}
        onConfirm={handleLinkConfirm}
      />
      <MathPopover
        triggerRef={mathTriggerRef}
        open={mathOpen}
        onClose={() => setMathOpen(false)}
        onConfirm={handleMathConfirm}
      />
      <WatermarkPopover
        triggerRef={watermarkTriggerRef}
        open={watermarkOpen}
        onClose={() => setWatermarkOpen(false)}
        currentWatermark={currentWatermark}
        onConfirm={handleWatermarkConfirm}
      />
      <HeaderFooterPopover
        kind="header"
        triggerRef={headerTriggerRef}
        open={headerOpen}
        onClose={() => setHeaderOpen(false)}
        onConfirm={(cfg) => handleHeaderFooterConfirm('header', cfg)}
      />
      <HeaderFooterPopover
        kind="footer"
        triggerRef={footerTriggerRef}
        open={footerOpen}
        onClose={() => setFooterOpen(false)}
        onConfirm={(cfg) => handleHeaderFooterConfirm('footer', cfg)}
      />
    </div>
  );
};