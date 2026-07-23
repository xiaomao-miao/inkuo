import React, { useEffect, useState } from 'react';
import { PanelBottom, PanelTop } from 'lucide-react';
import { FormPopover } from '../primitives';
import styles from '../WordToolbar.module.css';

export type HeaderFooterKind = 'header' | 'footer';

export interface HeaderFooterConfig {
  text: string;
  alignment: 'left' | 'center' | 'right';
  includePageNumber: boolean;
  insertBeforeFirstPage: boolean;
}

export interface HeaderFooterPopoverProps {
  /** Whether this is for a header or a footer. */
  kind: HeaderFooterKind;
  triggerRef: React.RefObject<HTMLElement | null>;
  open: boolean;
  onClose: () => void;
  onConfirm: (cfg: HeaderFooterConfig) => void;
}

const ALIGNMENT_OPTIONS = [
  { v: 'left', label: '左对齐' },
  { v: 'center', label: '居中' },
  { v: 'right', label: '右对齐' },
] as const;

const DEFAULT_TEXT: Record<HeaderFooterKind, string> = {
  header: '页眉',
  footer: '页脚',
};

/**
 * Header/footer insertion panel. Lets the user pick the content (custom
 * text / page number / both), the alignment, and whether to also clear
 * the existing first-page header/footer. Replaces the prior `window.prompt`
 * which only asked for a single text string.
 */
export const HeaderFooterPopover: React.FC<HeaderFooterPopoverProps> = ({
  kind,
  triggerRef,
  open,
  onClose,
  onConfirm,
}) => {
  const [text, setText] = useState(DEFAULT_TEXT[kind]);
  const [alignment, setAlignment] = useState<'left' | 'center' | 'right'>('center');
  const [includePageNumber, setIncludePageNumber] = useState(false);
  const [insertBeforeFirstPage, setInsertBeforeFirstPage] = useState(false);

  useEffect(() => {
    if (open) {
      setText(DEFAULT_TEXT[kind]);
      setAlignment('center');
      setIncludePageNumber(false);
      setInsertBeforeFirstPage(false);
    }
  }, [open, kind]);

  const canConfirm = text.trim().length > 0 || includePageNumber;
  const label = kind === 'header' ? '页眉' : '页脚';
  const placeholder = kind === 'header' ? '页眉文字 (例如: 公司名称)' : '页脚文字 (例如: 版权信息)';

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
          placeholder={placeholder}
        />
        <div className={styles.wFormHint}>
          留空可只插入页码
        </div>
      </div>

      <div className={styles.wFormField}>
        <label className={styles.wFormLabel}>对齐方式</label>
        <div className={styles.wFormToggleRow}>
          {ALIGNMENT_OPTIONS.map((opt) => (
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
          <span>也在首页显示 (清除首页单独的{label})</span>
        </label>
      </div>
    </FormPopover>
  );
};
