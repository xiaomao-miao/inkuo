import React, { useCallback, useEffect, useMemo, useState } from 'react';
import type { EditorView } from 'prosemirror-view';
import type { MarkType } from 'prosemirror-model';
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
  Heading2,
  Heading3,
  WrapText,
  type LucideIcon,
} from 'lucide-react';
import styles from './OfficeViewer.module.css';

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

function runCommand(view: EditorView | null, command: (state: any, dispatch?: any, view?: any) => boolean) {
  if (!view) return;
  command(view.state, view.dispatch, view);
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
}

const IconButton: React.FC<IconButtonProps> = ({
  icon: Icon,
  title,
  active,
  disabled,
  onClick,
  size,
}) => (
  <button
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
  const current = options.find((o) => o.value === value);
  return (
    <div className={styles.wDropdown} style={width ? { width } : undefined}>
      <button
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
      {open && (
        <>
          <div className={styles.wDropdownBackdrop} onClick={() => setOpen(false)} />
          <div className={styles.wDropdownMenu}>
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
          </div>
        </>
      )}
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
        type="button"
        className={styles.wFontSizeDropdown}
        onMouseDown={(e) => e.preventDefault()}
        onClick={() => setOpen((o) => !o)}
        title="字号列表"
      >
        <ChevronDown size={11} />
      </button>
      {open && (
        <>
          <div className={styles.wDropdownBackdrop} onClick={() => setOpen(false)} />
          <div className={`${styles.wDropdownMenu} ${styles.wFontSizeMenu}`}>
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
          </div>
        </>
      )}
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
  return (
    <div className={styles.wColorPicker}>
      <button
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
      {open && (
        <>
          <div className={styles.wDropdownBackdrop} onClick={() => setOpen(false)} />
          <div className={`${styles.wDropdownMenu} ${styles.wColorPickerGrid}`}>
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
          </div>
        </>
      )}
    </div>
  );
};

interface TablePickerProps {
  onInsert: (rows: number, cols: number) => void;
}

const TablePicker: React.FC<TablePickerProps> = ({ onInsert }) => {
  const [hover, setHover] = useState<{ rows: number; cols: number }>({ rows: 0, cols: 0 });
  const [open, setOpen] = useState(false);
  const maxRows = 10;
  const maxCols = 5;

  return (
    <div className={styles.wTablePicker}>
      <button
        type="button"
        className={styles.wColorPickerTrigger}
        title="插入表格"
        onMouseDown={(e) => e.preventDefault()}
        onClick={() => setOpen((o) => !o)}
      >
        <Table2 size={13} />
        <ChevronDown size={9} />
      </button>
      {open && (
        <>
          <div className={styles.wDropdownBackdrop} onClick={() => setOpen(false)} />
          <div className={`${styles.wDropdownMenu} ${styles.wTableMenu}`}>
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
          </div>
        </>
      )}
    </div>
  );
};

interface SymbolPickerProps {
  onInsert: (symbol: string) => void;
}

const SymbolPicker: React.FC<SymbolPickerProps> = ({ onInsert }) => {
  const [open, setOpen] = useState(false);
  return (
    <div className={styles.wSymbolPicker}>
      <button
        type="button"
        className={styles.wColorPickerTrigger}
        title="插入特殊符号"
        onMouseDown={(e) => e.preventDefault()}
        onClick={() => setOpen((o) => !o)}
      >
        <Sigma size={13} />
        <ChevronDown size={9} />
      </button>
      {open && (
        <>
          <div className={styles.wDropdownBackdrop} onClick={() => setOpen(false)} />
          <div className={`${styles.wDropdownMenu} ${styles.wSymbolMenu}`}>
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
          </div>
        </>
      )}
    </div>
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
  const isActive = (name: string): boolean => {
    const mt = markTypes?.[name];
    if (!state || !mt) return false;
    return isMarkActive(state, mt);
  };

  const isBold = isActive('bold');
  const isItalic = isActive('italic');
  const isUnderline = isActive('underline');
  const isStrike = isActive('strike');
  const isSuper = isActive('superscript');
  const isSub = isActive('subscript');
  const isLink = isHyperlinkActive(state!);
  const alignment = state ? getParagraphAlignment(state) : null;
  const styleId = state ? getStyleId(state) : null;
  const fontSizeHp = (markTypes?.fontSize && state) ? getMarkAttr(state, markTypes.fontSize, 'size') : null;
  const fontFamily = (markTypes?.fontFamily && state) ? getMarkAttr(state, markTypes.fontFamily, 'ascii') : null;
  const textColor = (markTypes?.textColor && state) ? getMarkAttr(state, markTypes.textColor, 'rgb') : null;
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

  const handleLineSpacing = useCallback(
    (v: string) => {
      const n = Number(v);
      if (n === 1) runCommand(view, singleSpacing);
      else if (n === 1.5) runCommand(view, oneAndHalfSpacing);
      else if (n === 2) runCommand(view, doubleSpacing);
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
      if (view && text) {
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
  const handleInsertLink = useCallback(() => {
    const sel = window.getSelection()?.toString() ?? '';
    const url = window.prompt('输入链接 URL', 'https://');
    if (!url) return;
    if (sel && state) {
      runCommand(view, setHyperlink(url));
    } else if (state) {
      runCommand(view, insertHyperlink(url, url));
    }
  }, [view, state]);
  const handleRemoveLink = useCallback(() => runCommand(view, removeHyperlink), [view]);
  const handleInsertSymbol = useCallback(
    (sym: string) => {
      if (view) view.dispatch(view.state.tr.insertText(sym, view.state.selection.from, view.state.selection.to));
    },
    [view],
  );

  // ── Selection ─────────────────────────────────────────────────────────────
  const handleSelectAll = useCallback(() => {
    document.execCommand('selectAll');
  }, []);
  const handleClearFormatting = useCallback(() => runCommand(view, clearFormatting), [view]);

  // ── Watermark ─────────────────────────────────────────────────────────────
  const handleWatermark = useCallback(() => {
    const text = window.prompt('水印文字', 'CONFIDENTIAL');
    if (!text) return;
    runCommand(view, setWatermark({ text, color: { rgb: 'C0C0C0' }, angle: -45 } as any));
  }, [view]);

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
            disabled={!view}
            onClick={() => {/* PM history via Ctrl+Z */}}
          />
          <IconButton
            icon={Redo2}
            title="重做 (Ctrl+Y)"
            disabled={!view}
            onClick={() => {/* PM history via Ctrl+Y */}}
          />
        </div>
        <span className={styles.wToolbarGroupSep} />

        {/* Clipboard */}
        <div className={styles.wToolbarGroup}>
          <IconButton icon={Clipboard} title="粘贴 (Ctrl+V)" disabled={!view} onClick={handlePaste} />
          <IconButton icon={Scissors} title="剪切 (Ctrl+X)" disabled={!view} onClick={handleCut} />
          <IconButton icon={CopyIcon} title="复制 (Ctrl+C)" disabled={!view} onClick={handleCopy} />
          <IconButton icon={Brush} title="格式刷" disabled={!view} onClick={() => {/* format painter stub */}} />
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
          <IconButton icon={ArrowDownAZ} title="降序排序" disabled onClick={() => {}} />
          <IconButton icon={ArrowUpAZ} title="升序排序" disabled onClick={() => {}} />
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
          <IconButton icon={Link2} title="插入超链接" active={isLink} onClick={handleInsertLink} />
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
          <IconButton icon={Sigma} title="插入数学公式" disabled onClick={() => {}} />
        </div>
        <span className={styles.wToolbarGroupSep} />

        {/* Page / Document */}
        <div className={styles.wToolbarGroup}>
          <IconButton icon={PaintBucket} title="页面颜色" disabled onClick={() => {}} />
          <IconButton icon={Heading2} title="页眉" disabled onClick={() => {}} />
          <IconButton icon={Heading3} title="页脚" disabled onClick={() => {}} />
          <IconButton icon={SpellCheck2} title="拼写检查" disabled onClick={() => {}} />
          <IconButton icon={Pilcrow} title="水印" onClick={handleWatermark} />
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
    </div>
  );
};