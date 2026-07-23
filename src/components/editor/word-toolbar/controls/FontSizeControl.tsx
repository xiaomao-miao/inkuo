import React, { useCallback, useEffect, useRef, useState } from 'react';
import { ChevronDown, ChevronUp } from 'lucide-react';
import { DropdownPortal } from '../primitives';
import { FONT_SIZES_PT } from '../constants';
import { parseFontSizePt } from '../numeric';
import styles from '../WordToolbar.module.css';

export interface FontSizeDropdownProps {
  value: number;
  onChange: (pt: number) => void;
  onStep: (delta: number) => void;
  disabled?: boolean;
}

/**
 * Spinner + free-text input + dropdown for the font size. Used by the
 * toolbar's font group. The input commits on Enter / blur; values outside
 * `[1, 400]` pt snap back to the current size (sanity-check guard).
 */
export const FontSizeControl: React.FC<FontSizeDropdownProps> = ({
  value,
  onChange,
  onStep,
  disabled,
}) => {
  const [open, setOpen] = useState(false);
  const [input, setInput] = useState(String(value));
  useEffect(() => setInput(String(value)), [value]);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const close = useCallback(() => setOpen(false), []);

  const commit = () => {
    const n = parseFontSizePt(input);
    if (n !== null) {
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
