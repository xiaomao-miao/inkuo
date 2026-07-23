import React, { useCallback, useRef, useState } from 'react';
import { ChevronDown, Sigma } from 'lucide-react';
import { DropdownPortal } from '../primitives';
import { SYMBOLS } from '../constants';
import styles from '../WordToolbar.module.css';

export interface SymbolPickerProps {
  onInsert: (symbol: string) => void;
}

/**
 * Special-character grid grouped by category. The list itself lives in
 * `constants.ts`; the picker just renders a button per symbol and fires
 * `onInsert(symbol)`. The parent routes the symbol into a ProseMirror
 * transaction.
 */
export const SymbolPicker: React.FC<SymbolPickerProps> = ({ onInsert }) => {
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
