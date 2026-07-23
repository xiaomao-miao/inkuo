import React, { useCallback, useRef, useState } from 'react';
import { ChevronDown, Table2 } from 'lucide-react';
import { DropdownPortal } from '../primitives';
import styles from '../WordToolbar.module.css';

export interface TablePickerProps {
  onInsert: (rows: number, cols: number) => void;
}

const MAX_ROWS = 10;
const MAX_COLS = 5;

/**
 * Excel-style "hover to choose table size" grid (rows × cols). The header
 * shows the currently hovered size; clicking any cell inserts a table of
 * that shape and closes the picker. `onMouseLeave` on the grid resets the
 * hover indicator so the user can cancel visually without committing.
 */
export const TablePicker: React.FC<TablePickerProps> = ({ onInsert }) => {
  const [hover, setHover] = useState<{ rows: number; cols: number }>({ rows: 0, cols: 0 });
  const [open, setOpen] = useState(false);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const close = useCallback(() => setOpen(false), []);

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
          {Array.from({ length: MAX_ROWS }).map((_, r) =>
            Array.from({ length: MAX_COLS }).map((_, c) => {
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
