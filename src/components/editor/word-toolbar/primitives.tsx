// Common UI primitives used by the Word toolbar dropdowns and popovers.
//
// `IconButton`, `Dropdown`, `DropdownPortal`, `FormPopover` together form the
// chrome around the toolbar's many actions. They used to be declared inline
// in WordToolbar.tsx alongside the WordToolbar itself; pulling them out keeps
// the main file focused on the toolbar layout and lets future toolbars
// (Excel, PowerPoint) reuse the same dropdown machinery.

import React, { useCallback, useState } from 'react';
import { createPortal } from 'react-dom';
import { ChevronDown, type LucideIcon } from 'lucide-react';
import styles from './WordToolbar.module.css';
import {
  useDropdownPosition,
  useEscapeToClose,
  usePlacementTransform,
} from './helpers';

// ─── IconButton ───────────────────────────────────────────────────────────────

export interface IconButtonProps {
  icon: LucideIcon | React.ComponentType<{ size?: number }>;
  title: string;
  active?: boolean;
  disabled?: boolean;
  onClick: () => void;
  size?: number;
  /** Forwarded to the underlying <button> so callers can anchor popovers. */
  buttonRef?: React.RefObject<HTMLButtonElement | null>;
}

export const IconButton: React.FC<IconButtonProps> = ({
  icon: Icon,
  title,
  active,
  disabled,
  onClick,
  size,
  buttonRef,
}) => (
  <button
    ref={buttonRef}
    type="button"
    className={`${styles.wToolbarIconBtn} ${active ? styles.wToolbarIconBtnActive : ''}`}
    title={title}
    aria-label={title}
    aria-pressed={active}
    disabled={disabled}
    onMouseDown={(e) => e.preventDefault() /* keep editor focus */}
    onClick={onClick}
  >
    <Icon size={size ?? 13} />
  </button>
);

// ─── DropdownPortal ───────────────────────────────────────────────────────────

export interface DropdownPortalProps {
  triggerRef: React.RefObject<HTMLElement | null>;
  open: boolean;
  onClose: () => void;
  /** Class name applied to the menu panel. */
  menuClassName?: string;
  /** Optional style override for the menu panel (used to anchor placement / width). */
  menuStyle?: React.CSSProperties;
  children: React.ReactNode;
}

/**
 * Renders a backdrop + menu into `document.body` so the menu escapes any
 * `overflow: hidden` / `contain` ancestors of the trigger (e.g. the toolbar
 * root and the office stack). Closes on backdrop click and on Escape.
 */
export const DropdownPortal: React.FC<DropdownPortalProps> = ({
  triggerRef,
  open,
  onClose,
  menuClassName,
  menuStyle,
  children,
}) => {
  const layout = useDropdownPosition(triggerRef, open);
  const menuRef = React.useRef<HTMLDivElement>(null);
  useEscapeToClose(open, onClose);
  usePlacementTransform(menuRef, layout, open);

  if (typeof document === 'undefined') return null;
  if (!open || !layout) return null;

  const anchorStyle: React.CSSProperties = {
    position: 'fixed',
    top: layout.top,
    left: layout.left,
    minWidth: layout.width,
    zIndex: 1000,
  };

  return createPortal(
    <>
      <div
        className={styles.wDropdownBackdrop}
        onMouseDown={(e) => {
          // Prevent the editor's mousedown handler from stealing focus
          // before we close.
          e.preventDefault();
          e.stopPropagation();
          onClose();
        }}
      />
      <div
        ref={menuRef}
        className={`${styles.wDropdownMenu} ${menuClassName ?? ''}`}
        style={{ ...anchorStyle, ...menuStyle }}
      >
        {children}
      </div>
    </>,
    document.body,
  );
};

// ─── Dropdown ─────────────────────────────────────────────────────────────────

export interface DropdownProps {
  value: string;
  options: Array<{ value: string; label: string }>;
  onChange: (value: string) => void;
  title: string;
  width?: number;
  displayValue?: string;
  icon?: LucideIcon;
}

export const Dropdown: React.FC<DropdownProps> = ({
  value,
  options,
  onChange,
  title,
  width,
  displayValue,
  icon: Icon,
}) => {
  const [open, setOpen] = useState(false);
  const triggerRef = React.useRef<HTMLButtonElement>(null);
  const current = options.find((o) => o.value === value);
  const close = useCallback(() => setOpen(false), []);
  return (
    <div className={styles.wDropdown} style={width ? { width } : undefined}>
      <button
        ref={triggerRef}
        type="button"
        className={styles.wDropdownTrigger}
        title={title}
        onMouseDown={(e) => e.preventDefault()}
        onClick={() => setOpen((o) => !o)}
      >
        {Icon && <Icon size={12} />}
        <span className={styles.wDropdownLabel}>{displayValue ?? current?.label ?? value}</span>
        <ChevronDown size={11} />
      </button>
      <DropdownPortal triggerRef={triggerRef} open={open} onClose={close}>
        {options.map((o) => (
          <button
            key={o.value}
            type="button"
            className={`${styles.wDropdownOption} ${o.value === value ? styles.wDropdownOptionActive : ''}`}
            onClick={() => {
              onChange(o.value);
              setOpen(false);
            }}
          >
            {o.label}
          </button>
        ))}
      </DropdownPortal>
    </div>
  );
};

// ─── FormPopover ──────────────────────────────────────────────────────────────

export interface FormPopoverProps {
  triggerRef: React.RefObject<HTMLElement | null>;
  open: boolean;
  onClose: () => void;
  /** Header label shown above the form body. */
  title: string;
  /** Optional leading icon next to the title (lucide component or string char). */
  titleIcon?: React.ReactNode;
  /** Approximate menu width in px. */
  width?: number;
  /** Body content (form fields). */
  children: React.ReactNode;
  /** Footer button labels. Defaults: "取消" / "确定". */
  confirmLabel?: string;
  cancelLabel?: string;
  /** Disable the confirm button (e.g. invalid form). */
  confirmDisabled?: boolean;
  /** Confirm handler. */
  onConfirm: () => void;
}

/**
 * `FormPopover` wraps `DropdownPortal` and adds a consistent settings-panel
 * chrome (title bar + footer with Cancel/Confirm). Triggered by a toolbar
 * button; renders into a portal at `<body>` so it escapes any `overflow:hidden`
 * ancestor. Used to replace the legacy `window.prompt` calls with an in-app
 * panel themed to match the rest of the toolbar.
 */
export const FormPopover: React.FC<FormPopoverProps> = ({
  triggerRef,
  open,
  onClose,
  title,
  titleIcon,
  width = 320,
  children,
  confirmLabel = '确定',
  cancelLabel = '取消',
  confirmDisabled,
  onConfirm,
}) => {
  const layout = useDropdownPosition(triggerRef, open);
  const menuRef = React.useRef<HTMLDivElement>(null);
  useEscapeToClose(open, onClose);
  usePlacementTransform(menuRef, layout, open);

  // Submit on Enter, cancel on Escape (handled by DropdownPortal's keydown,
  // we only handle Enter here to keep the wiring self-contained).
  React.useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Enter' && !e.shiftKey && !e.altKey) {
        const tag = (e.target as HTMLElement | null)?.tagName;
        // Avoid hijacking Enter inside multi-line textareas.
        if (tag === 'TEXTAREA') return;
        if (confirmDisabled) return;
        e.preventDefault();
        onConfirm();
      }
    };
    window.addEventListener('keydown', onKey, true);
    return () => window.removeEventListener('keydown', onKey, true);
  }, [open, confirmDisabled, onConfirm]);

  if (typeof document === 'undefined') return null;
  if (!open || !layout) return null;

  const anchorStyle: React.CSSProperties = {
    position: 'fixed',
    top: layout.top,
    left: layout.left,
    width,
    zIndex: 1000,
  };

  return createPortal(
    <>
      <div
        className={styles.wDropdownBackdrop}
        onMouseDown={(e) => {
          e.preventDefault();
          e.stopPropagation();
          onClose();
        }}
      />
      <div
        ref={menuRef}
        className={`${styles.wDropdownMenu} ${styles.wFormMenu}`}
        style={anchorStyle}
        role="dialog"
        aria-label={title}
        onMouseDown={(e) => e.stopPropagation()}
      >
        <div className={styles.wFormHeader}>
          {titleIcon && <span className={styles.wFormHeaderIcon}>{titleIcon}</span>}
          <span className={styles.wFormHeaderTitle}>{title}</span>
        </div>
        <div className={styles.wFormBody}>{children}</div>
        <div className={styles.wFormFooter}>
          <button
            type="button"
            className={styles.wFormBtnSecondary}
            onClick={onClose}
          >
            {cancelLabel}
          </button>
          <button
            type="button"
            className={styles.wFormBtnPrimary}
            disabled={confirmDisabled}
            onClick={onConfirm}
          >
            {confirmLabel}
          </button>
        </div>
      </div>
    </>,
    document.body,
  );
};