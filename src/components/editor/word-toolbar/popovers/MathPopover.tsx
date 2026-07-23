import React, { useEffect, useState } from 'react';
import { Sigma } from 'lucide-react';
import { FormPopover } from '../primitives';
import { MATH_PRESETS } from '../constants';
import styles from '../WordToolbar.module.css';

export interface MathPopoverProps {
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
export const MathPopover: React.FC<MathPopoverProps> = ({
  triggerRef,
  open,
  onClose,
  onConfirm,
}) => {
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
