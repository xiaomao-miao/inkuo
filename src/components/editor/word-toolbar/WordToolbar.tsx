import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
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
} from 'lucide-react';
import styles from './WordToolbar.module.css';
import {
  FONT_FAMILIES,
  FONT_SIZES_PT,
  TEXT_COLORS,
  HIGHLIGHT_COLORS,
  PARAGRAPH_STYLES,
  ZOOM_LEVELS,
  LINE_SPACING_OPTIONS,
  SYMBOLS,
  WATERMARK_COLORS,
  WATERMARK_FONTS,
  MATH_PRESETS,
} from './constants';
import { hpToPt, rgbToHex, isViewReady, runCommand } from './helpers';
import {
  IconButton,
  Dropdown,
  DropdownPortal,
  FormPopover,
} from './primitives';

// ─── Constants ────────────────────────────────────────────────────────────────
//
// Constants live in `./constants.ts`. They were previously inlined here; the
// split keeps the toolbar file focused on layout + behaviour and lets other
// surfaces (Excel toolbar, future command-palette UI) reuse the same lists.

// ─── Helpers ──────────────────────────────────────────────────────────────────
//
// `isViewReady`, `runCommand`, `hpToPt`, `rgbToHex`, and the
// `useDropdownPosition` / `useEscapeToClose` / `usePlacementTransform` hooks
// live in `./helpers.ts`. They are pulled out so the dispatch path can be
// shared with other ProseMirror consumers without importing the entire
// toolbar component.

// ─── Sub-buttons ──────────────────────────────────────────────────────────────
//
// `IconButton`, `Dropdown`, `DropdownPortal`, and `FormPopover` live in
// `./primitives.tsx`. They were previously inlined here; extracting them lets
// the Excel/PowerPoint toolbars reuse the same chrome without re-declaring
// the menu positioning logic, and keeps this file focused on the WordToolbar
// layout itself.

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
//
// `FormPopover` lives in `./primitives.tsx`. Word-specific panels
// (LinkPopover, MathPopover, WatermarkPopover, HeaderFooterPopover) stay here
// because they hold domain-specific layout + LaTeX / watermark / page-number
// presets that have no value outside the Word editor.

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
//
// MATH_PRESETS now lives in `./constants.ts` (imported at the top of the file)
// so the Excel and future toolbars can reuse the same equation chips if they
// ever ship a math-insertion flow.

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
//
// WATERMARK_COLORS and WATERMARK_FONTS now live in `./constants.ts` so they
// can be shared with a future watermark settings dialog outside the toolbar.

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