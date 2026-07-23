import React, { useEffect, useState } from 'react';
import { Link2 } from 'lucide-react';
import { FormPopover } from '../primitives';
import styles from '../WordToolbar.module.css';

export interface LinkPopoverProps {
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

// Allow-list of URL schemes we accept. Relative paths (`./`, `../`, `/`) and
// bare `www.` are also permitted because they're common inside Word documents.
const URL_PREFIX_REGEX = /^(https?:\/\/|mailto:|tel:|file:|\/|\.\/|\.\.\/|www\.)/i;

const PROTOCOL_PRESETS = [
  { label: 'http://', value: 'http://' },
  { label: 'https://', value: 'https://' },
  { label: 'mailto:', value: 'mailto:' },
  { label: 'tel:', value: 'tel:' },
] as const;

/**
 * Settings panel for inserting or editing a hyperlink. Mirrors the Word
 * "Insert Hyperlink" dialog: URL field + optional display text + quick
 * presets for common URL prefixes. Replaces the previous `window.prompt`
 * which was a single-line modal and didn't allow the user to pick a
 * different display string for the link.
 */
export const LinkPopover: React.FC<LinkPopoverProps> = ({
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

  const trimmed = url.trim();
  const isValid = trimmed.length > 0 && URL_PREFIX_REGEX.test(trimmed);

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
        onConfirm(trimmed, display.trim() || trimmed);
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
        {PROTOCOL_PRESETS.map((p) => (
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
