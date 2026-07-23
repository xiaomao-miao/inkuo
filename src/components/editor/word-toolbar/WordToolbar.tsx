// WordToolbar — the toolbar layout for the Word editor.
//
// This file is intentionally thin: it owns only the React layout (which
// IconButton / Dropdown goes in which row / group) and the open-state for
// each popover. All the ProseMirror / DOM / document-model logic lives in:
//
//   - `./handlers.ts`              — actions (font change, clipboard, sort, …)
//   - `./useWordToolbarState.ts`   — read-only state derivations (isBold, …)
//   - `./popovers/`                — settings panels (link, math, watermark, header/footer)
//   - `./controls/`                 — inline form controls (font size, colour pickers, table/symbol pickers)
//   - `./primitives.tsx`           — IconButton / Dropdown / FormPopover chrome
//   - `./constants.ts`             — font lists, colour palettes, presets
//   - `./helpers.ts`               — ProseMirror dispatch + positioning hooks

import React, { useCallback, useEffect, useRef, useState } from 'react';
import type { EditorView } from 'prosemirror-view';
import {
  ArrowDownAZ,
  ArrowUpAZ,
  Bold,
  Brush,
  Clipboard,
  Copy as CopyIcon,
  Eraser,
  Heading1,
  Italic,
  Image as ImageIcon,
  Link2,
  List,
  ListOrdered,
  IndentDecrease,
  IndentIncrease,
  AlignCenter,
  AlignJustify,
  AlignLeft,
  AlignRight,
  PanelBottom,
  PanelTop,
  PencilLine,
  Pilcrow,
  Printer,
  Redo2,
  Replace,
  Save,
  Scissors,
  Search,
  Sigma,
  SpellCheck2,
  Strikethrough,
  Type,
  Underline as UnderlineIcon,
  Undo2,
  WrapText,
  ZoomIn,
  ZoomOut,
  Sparkles,
} from 'lucide-react';
import {
  FONT_FAMILIES,
  HIGHLIGHT_COLORS,
  LINE_SPACING_OPTIONS,
  PARAGRAPH_STYLES,
  TEXT_COLORS,
  ZOOM_LEVELS,
} from './constants';
import {
  ColorPicker,
  FontSizeControl,
  PageColorPicker,
  SymbolPicker,
  TablePicker,
} from './controls';
import {
  useWordToolbarHandlers,
  WORD_TOOLBAR_PAGE_COLOR_PALETTE,
} from './handlers';
import {
  HeaderFooterPopover,
  LinkPopover,
  MathPopover,
  WatermarkPopover,
} from './popovers';
import { Dropdown, IconButton } from './primitives';
import { parseZoomFactorFromPct } from './numeric';
import { useWordToolbarState } from './useWordToolbarState';
import styles from './WordToolbar.module.css';

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

const ACTIVE_STATE_TICK_MS = 250;

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
  // ── Poll editor state to refresh toolbar active-state ───────────────────
  // ProseMirror's selection / mark state isn't a React value, so we
  // bump a `tick` counter every ACTIVE_STATE_TICK_MS to force the toolbar
  // to re-render its active-state badges. Without this the bold / italic
  // buttons would stay highlighted even after the user moved the cursor.
  const [, setTick] = useState(0);
  useEffect(() => {
    if (!view) return;
    const handle = window.setInterval(() => setTick((t) => t + 1), ACTIVE_STATE_TICK_MS);
    return () => window.clearInterval(handle);
  }, [view]);

  // ── Zoom level (mirrors the imperative editor's zoom) ───────────────────
  const [zoomLevel, setZoomLevel] = useState(1);
  useEffect(() => {
    setZoomLevel(getZoom() || 1);
  }, [getZoom]);

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
    const z = parseZoomFactorFromPct(value);
    if (z !== null) {
      setZoom(z);
      setZoomLevel(z);
    }
  };

  // ── Read-only state from the editor view ────────────────────────────────
  const state = useWordToolbarState(view);

  // ── Popover open-state (the only state the layout owns directly) ────────
  const [linkOpen, setLinkOpen] = useState(false);
  const [linkInitialText, setLinkInitialText] = useState('');
  const [linkInitialUrl, setLinkInitialUrl] = useState<string | undefined>(undefined);
  const [linkEditing, setLinkEditing] = useState(false);

  const [mathOpen, setMathOpen] = useState(false);
  const mathTriggerRef = useRef<HTMLButtonElement>(null);
  const linkTriggerRef = useRef<HTMLButtonElement>(null);

  const [watermarkOpen, setWatermarkOpen] = useState(false);
  const watermarkTriggerRef = useRef<HTMLButtonElement>(null);

  const [headerOpen, setHeaderOpen] = useState(false);
  const [footerOpen, setFooterOpen] = useState(false);
  const headerTriggerRef = useRef<HTMLButtonElement>(null);
  const footerTriggerRef = useRef<HTMLButtonElement>(null);

  // ── Action handlers ─────────────────────────────────────────────────────
  const handlers = useWordToolbarHandlers(
    view,
    editor,
    state.isLink,
    state.fontSizePt,
    notify,
    {
      openLink: ({ initialText, isEditingExisting }) => {
        setLinkInitialText(initialText);
        setLinkInitialUrl(undefined);
        setLinkEditing(isEditingExisting);
        setLinkOpen(true);
      },
      closeLink: () => setLinkOpen(false),
      openMath: () => setMathOpen(true),
      closeMath: () => setMathOpen(false),
      openWatermark: () => setWatermarkOpen(true),
      closeWatermark: () => setWatermarkOpen(false),
      openHeader: () => setHeaderOpen(true),
      closeHeader: () => setHeaderOpen(false),
      openFooter: () => setFooterOpen(true),
      closeFooter: () => setFooterOpen(false),
    },
  );

  const handleModeChange = useCallback(
    (v: string) => onModeChange(v as WordToolbarProps['mode']),
    [onModeChange],
  );

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
            onClick={handlers.handleUndo}
          />
          <IconButton
            icon={Redo2}
            title="重做 (Ctrl+Y)"
            disabled={!view || !editor}
            onClick={handlers.handleRedo}
          />
        </div>
        <span className={styles.wToolbarGroupSep} />

        {/* Clipboard */}
        <div className={styles.wToolbarGroup}>
          <IconButton icon={Clipboard} title="粘贴 (Ctrl+V)" disabled={!view} onClick={handlers.handlePaste} />
          <IconButton icon={Scissors} title="剪切 (Ctrl+X)" disabled={!view} onClick={handlers.handleCut} />
          <IconButton icon={CopyIcon} title="复制 (Ctrl+C)" disabled={!view} onClick={handlers.handleCopy} />
          <IconButton
            icon={Brush}
            title={handlers.paintedMarks ? '格式刷 (再次点击应用,Esc 取消)' : '格式刷 (复制选区格式)'}
            disabled={!view}
            active={!!handlers.paintedMarks}
            onClick={handlers.handleFormatPainter}
          />
        </div>
        <span className={styles.wToolbarGroupSep} />

        {/* Font */}
        <div className={styles.wToolbarGroup}>
          <Dropdown
            value={(state.fontFamily as string) ?? 'Microsoft YaHei'}
            onChange={handlers.handleFontFamily}
            title="字体"
            width={118}
            displayValue={(state.fontFamily as string) ?? '默认字体'}
            icon={Type}
            options={FONT_FAMILIES.map((f) => ({ value: f, label: f }))}
          />
          <FontSizeControl
            value={state.fontSizePt}
            onChange={handlers.handleFontSize}
            onStep={handlers.handleFontSizeStep}
            disabled={!view}
          />
        </div>
        <span className={styles.wToolbarGroupSep} />

        {/* Marks */}
        <div className={styles.wToolbarGroup}>
          <IconButton icon={Bold} title="加粗 (Ctrl+B)" active={state.isBold} onClick={handlers.toggleBold} />
          <IconButton icon={Italic} title="斜体 (Ctrl+I)" active={state.isItalic} onClick={handlers.toggleItalic} />
          <IconButton icon={UnderlineIcon} title="下划线 (Ctrl+U)" active={state.isUnderline} onClick={handlers.toggleUnderline} />
          <IconButton icon={Strikethrough} title="删除线" active={state.isStrike} onClick={handlers.toggleStrike} />
          <IconButton
            icon={() => <span style={{ fontSize: 11, fontWeight: 700, fontStyle: 'italic' }}>X²</span>}
            title="上标"
            active={state.isSuper}
            onClick={handlers.toggleSuperscript}
          />
          <IconButton
            icon={() => <span style={{ fontSize: 11, fontWeight: 700, fontStyle: 'italic' }}>X₂</span>}
            title="下标"
            active={state.isSub}
            onClick={handlers.toggleSubscript}
          />
        </div>
        <span className={styles.wToolbarGroupSep} />

        {/* Colors */}
        <div className={styles.wToolbarGroup}>
          <ColorPicker colors={TEXT_COLORS} fontColor={state.fontColor} onChange={handlers.handleFontColor} title="字体颜色" />
          <ColorPicker colors={HIGHLIGHT_COLORS} onChange={handlers.handleHighlight} highlight title="文字底色" />
          <IconButton icon={Eraser} title="清除格式" disabled={!view} onClick={handlers.handleClearFormatting} />
          <IconButton icon={Pilcrow} title="显示/隐藏格式标记" onClick={handlers.handleShowFormattingMarks} />
        </div>
        <span className={styles.wToolbarGroupSep} />

        {/* Paragraph styles — last group of Row 1 */}
        <div className={styles.wToolbarGroup}>
          <Dropdown
            value={state.styleId ?? 'Normal'}
            onChange={handlers.handleStyleChange}
            title="段落样式"
            width={120}
            icon={Heading1}
            options={PARAGRAPH_STYLES}
            displayValue={PARAGRAPH_STYLES.find((s) => s.value === (state.styleId ?? 'Normal'))?.label ?? '正文'}
          />
        </div>
      </div>
      {/* ── end of Row 1 ── */}

      {/* ═══════════ Row 2: alignment · lists · insert · page · find · zoom ═══ */}
      <div className={styles.wToolbarScroll}>
        {/* Alignment */}
        <div className={styles.wToolbarGroup}>
          <IconButton icon={AlignLeft} title="左对齐" active={state.alignment === 'left'} onClick={handlers.alignLeft} />
          <IconButton icon={AlignCenter} title="居中" active={state.alignment === 'center'} onClick={handlers.alignCenter} />
          <IconButton icon={AlignRight} title="右对齐" active={state.alignment === 'right'} onClick={handlers.alignRight} />
          <IconButton icon={AlignJustify} title="两端对齐" active={state.alignment === 'both'} onClick={handlers.alignJustify} />
        </div>
        <span className={styles.wToolbarGroupSep} />

        {/* Indent / lists / line spacing */}
        <div className={styles.wToolbarGroup}>
          <IconButton icon={List} title="项目符号列表" onClick={handlers.toggleBulletList} />
          <IconButton icon={ListOrdered} title="编号列表" onClick={handlers.toggleNumberedList} />
          <IconButton icon={IndentDecrease} title="减少缩进" onClick={handlers.decreaseIndent} />
          <IconButton icon={IndentIncrease} title="增加缩进" onClick={handlers.increaseIndent} />
          <IconButton icon={ArrowDownAZ} title="降序排序" disabled={!view} onClick={() => handlers.sortSelection('desc')} />
          <IconButton icon={ArrowUpAZ} title="升序排序" disabled={!view} onClick={() => handlers.sortSelection('asc')} />
          <Dropdown
            value="1"
            onChange={handlers.handleLineSpacing}
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
          <TablePicker onInsert={handlers.handleInsertTable} />
          <IconButton icon={ImageIcon} title="插入图片" onClick={handlers.handleInsertImage} />
          <IconButton icon={Link2} title="插入超链接" active={state.isLink} onClick={handlers.handleInsertLink} buttonRef={linkTriggerRef} />
          {state.isLink && (
            <button
              type="button"
              className={styles.wToolbarTextBtn}
              onClick={handlers.handleRemoveLink}
              title="移除链接"
            >
              取消链接
            </button>
          )}
          <SymbolPicker onInsert={handlers.handleInsertSymbol} />
          <IconButton icon={Sigma} title="插入数学公式 (LaTeX)" disabled={!view} onClick={handlers.handleInsertMath} buttonRef={mathTriggerRef} />
        </div>
        <span className={styles.wToolbarGroupSep} />

        {/* Page / Document */}
        <div className={styles.wToolbarGroup}>
          <PageColorPicker
            colors={WORD_TOOLBAR_PAGE_COLOR_PALETTE}
            onChange={handlers.handlePageColor}
            disabled={!editor}
            title="页面颜色"
          />
          <IconButton
            icon={PanelTop}
            title="插入页眉"
            disabled={!editor}
            onClick={handlers.handleInsertHeader}
            buttonRef={headerTriggerRef}
          />
          <IconButton
            icon={PanelBottom}
            title="插入页脚"
            disabled={!editor}
            onClick={handlers.handleInsertFooter}
            buttonRef={footerTriggerRef}
          />
          <IconButton
            icon={SpellCheck2}
            title="拼写检查 (浏览器原生)"
            onClick={handlers.handleToggleSpellCheck}
          />
          <IconButton icon={Pilcrow} title="水印" disabled={!view} onClick={handlers.handleWatermark} buttonRef={watermarkTriggerRef} />
          <button
            type="button"
            className={styles.wToolbarTextBtn}
            title="插入分页符 (Ctrl+Enter)"
            onClick={handlers.handleInsertPageBreak}
          >
            分页
          </button>
        </div>
        <span className={styles.wToolbarGroupSep} />

        {/* Find / Replace */}
        <div className={styles.wToolbarGroup}>
          <IconButton icon={Search} title="查找 (Ctrl+F)" onClick={onFind} />
          {onReplace && <IconButton icon={Replace} title="替换 (Ctrl+H)" onClick={onReplace} />}
          <IconButton icon={Eraser} title="全选 (Ctrl+A)" onClick={handlers.handleSelectAll} />
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
            onChange={handleModeChange}
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
        onConfirm={handlers.handleLinkConfirm}
      />
      <MathPopover
        triggerRef={mathTriggerRef}
        open={mathOpen}
        onClose={() => setMathOpen(false)}
        onConfirm={handlers.handleMathConfirm}
      />
      <WatermarkPopover
        triggerRef={watermarkTriggerRef}
        open={watermarkOpen}
        onClose={() => setWatermarkOpen(false)}
        currentWatermark={handlers.currentWatermark}
        onConfirm={handlers.handleWatermarkConfirm}
      />
      <HeaderFooterPopover
        kind="header"
        triggerRef={headerTriggerRef}
        open={headerOpen}
        onClose={() => setHeaderOpen(false)}
        onConfirm={(cfg) => handlers.handleHeaderFooterConfirm('header', cfg)}
      />
      <HeaderFooterPopover
        kind="footer"
        triggerRef={footerTriggerRef}
        open={footerOpen}
        onClose={() => setFooterOpen(false)}
        onConfirm={(cfg) => handlers.handleHeaderFooterConfirm('footer', cfg)}
      />
    </div>
  );
};
