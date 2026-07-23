import React, { useCallback, useRef, useState } from 'react';
import { ChevronDown, PaintBucket } from 'lucide-react';
import { DropdownPortal } from '../primitives';
import styles from '../WordToolbar.module.css';

export interface PageColorPickerProps {
  colors: string[];
  onChange: (c: string) => void;
  title: string;
  /** Whether the picker should be enabled (mirrors editor handle availability). */
  disabled?: boolean;
}

/**
 * Page-colour picker. Unlike the inline `ColorPicker` it doesn't have a
 * "highlight" mode and always renders a leading "无" (none) swatch so the
 * user can clear the page colour without picking a replacement.
 */
export const PageColorPicker: React.FC<PageColorPickerProps> = ({
  colors,
  onChange,
  title,
  disabled,
}) => {
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
