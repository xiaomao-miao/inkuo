import React, { useCallback, useRef, useState } from 'react';
import { ChevronDown, Highlighter } from 'lucide-react';
import { DropdownPortal } from '../primitives';
import styles from '../WordToolbar.module.css';

export interface ColorPickerProps {
  colors: string[];
  onChange: (c: string) => void;
  title: string;
  /** Switches the trigger to a highlighter icon and uses the highlight palette semantics. */
  highlight?: boolean;
  /** Current font color (hex), used to underline the `A` glyph in the trigger button. */
  fontColor?: string;
}

/**
 * Grid of colour swatches for font / highlight colour. `highlight: true`
 * uses the standard highlight palette and renders a yellow highlighter
 * icon in the trigger; otherwise the trigger shows a stylised `A` glyph
 * with `fontColor` applied as the underline colour so the user can see
 * the active colour at a glance.
 */
export const ColorPicker: React.FC<ColorPickerProps> = ({
  colors,
  onChange,
  title,
  highlight,
  fontColor,
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
