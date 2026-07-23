import React, { useEffect, useState } from 'react';
import { Eraser, Pilcrow } from 'lucide-react';
import { FormPopover } from '../primitives';
import { WATERMARK_COLORS, WATERMARK_FONTS } from '../constants';
import styles from '../WordToolbar.module.css';

export interface CurrentWatermarkText {
  kind: 'text';
  text: string;
}

export interface CurrentWatermarkPicture {
  kind: 'picture';
}

export type CurrentWatermark = CurrentWatermarkText | CurrentWatermarkPicture;

export interface WatermarkConfig {
  text: string;
  font: string;
  color: string;
  semitransparent: boolean;
  layout: 'diagonal' | 'horizontal';
  fontSize: number;
}

export interface WatermarkPopoverProps {
  triggerRef: React.RefObject<HTMLElement | null>;
  open: boolean;
  onClose: () => void;
  /** Existing watermark on the doc (if any) — for "replace / remove" controls. */
  currentWatermark: CurrentWatermark | null;
  /**
   * Confirm handler. Receives a fully-formed `WatermarkConfig` to apply, or
   * `null` to clear the existing watermark.
   */
  onConfirm: (cfg: WatermarkConfig | null) => void;
}

const TEXT_PRESETS = ['CONFIDENTIAL', 'DRAFT', 'DO NOT COPY', '内部资料', '机密'] as const;

const PREVIEW_MIN_PT = 14;
const PREVIEW_MAX_PT = 36;
const PREVIEW_DIVISOR = 2.5;

/**
 * Watermark settings panel. Builds a full `WatermarkConfig` that the
 * editor's `setWatermark` command will accept verbatim. Replaces the prior
 * `window.prompt` (which only collected text and never produced a
 * structurally valid TextWatermark — leading to silent mis-renders).
 *
 * `onConfirm(null)` clears the existing watermark when one is present.
 */
export const WatermarkPopover: React.FC<WatermarkPopoverProps> = ({
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
  const previewFontSize = Math.min(
    PREVIEW_MAX_PT,
    Math.max(PREVIEW_MIN_PT, fontSize / PREVIEW_DIVISOR),
  );

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
            fontSize: previewFontSize,
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
          {TEXT_PRESETS.map((preset) => (
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
