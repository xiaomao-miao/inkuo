// Single menu-row component (rendered recursively for submenus).
//
// The row handles:
//   - "divider" sentinel rows (rendered as an `<hr>`-style separator)
//   - leaf rows (icon + label + optional shortcut, closes menu on click)
//   - submenu host rows (icon + label + chevron, nested panel opens on hover)
//
// Kept in its own file because (a) the structure is self-contained
// and (b) splitting it out makes the orchestrating `ContextMenu` much
// easier to skim.

import { ChevronRight } from 'lucide-react';
import type { MouseEvent as ReactMouseEvent } from 'react';

import { useContextMenuStore } from '../../../store';

import { DIVIDER_ID, type MenuItem } from './types';
import styles from './ContextMenu.module.css';

interface MenuRowProps {
  item: MenuItem;
  depth?: number;
}

export const MenuRow = ({ item, depth = 0 }: MenuRowProps) => {
  const isSubmenu = !!item.submenu && item.submenu.length > 0;
  const className = [
    styles.item,
    item.disabled ? styles.disabled : '',
    item.danger ? styles.danger : '',
    item.checked ? styles.checked : '',
  ]
    .filter(Boolean)
    .join(' ');

  const handleClick = (e: ReactMouseEvent) => {
    e.stopPropagation();
    if (item.disabled) return;
    if (isSubmenu) return; // hover opens submenu
    item.action?.();
    useContextMenuStore.getState().close();
  };

  if (item.id === DIVIDER_ID) {
    return <div role="separator" className={styles.divider} />;
  }

  const content = (
    <>
      {item.icon && <span className={styles.itemIcon}>{item.icon}</span>}
      <span className={styles.itemLabel}>{item.label}</span>
      {item.shortcut && <span className={styles.itemShortcut}>{item.shortcut}</span>}
      {isSubmenu && (
        <span className={styles.itemChevron}>
          <ChevronRight size={12} />
        </span>
      )}
    </>
  );

  if (isSubmenu) {
    return (
      <div className={styles.submenuHost}>
        <button type="button" className={className} tabIndex={depth === 0 ? 0 : -1}>
          {content}
        </button>
        <div className={`${styles.contextMenu} ${styles.submenu}`} role="menu">
          {item.submenu!.map((sub) => (
            <MenuRow key={sub.id} item={sub} depth={depth + 1} />
          ))}
        </div>
      </div>
    );
  }

  return (
    <button
      type="button"
      className={className}
      onClick={handleClick}
      onMouseDown={(e) => e.stopPropagation()}
      disabled={item.disabled}
      tabIndex={depth === 0 ? 0 : -1}
    >
      {content}
    </button>
  );
};
