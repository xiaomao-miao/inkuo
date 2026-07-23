// Shared types for the workspace `ContextMenu`.
//
// Kept in their own module so menu builders, the row renderer, and
// the orchestrating `ContextMenu` component can all import them
// without circular dependencies.

import type { ReactNode } from 'react';
import type { OpenTab } from '../../../store';

/**
 * Recursive menu-item descriptor. Submenus are just nested `MenuItem`s,
 * `id === 'divider'` is the special separator row, and a row with an
 * `action` becomes a leaf button.
 *
 * `submenu` and `action` may coexist for completeness, but the renderer
 * treats a non-empty `submenu` as a hover-opened nested panel and skips
 * `action` (item.disabled still gates the click path).
 */
export interface MenuItem {
  id: string;
  label: string;
  icon?: ReactNode;
  shortcut?: string;
  disabled?: boolean;
  danger?: boolean;
  checked?: boolean;
  submenu?: MenuItem[];
  action?: () => void | Promise<void>;
}

/** Absolute pixel coordinates relative to the viewport. */
export interface Position {
  left: number;
  top: number;
}

/**
 * Bundle of state + side-effect callbacks every menu builder needs.
 * Centralizing these keeps each builder focused on describing its
 * menu shape without juggling individual hook reads.
 */
export interface MenuBuilderContext {
  workspacePath: string | null;
  openTabs: OpenTab[];
  selectedFile: string | null;
  /** Relative paths already part of the knowledge base. */
  knowledgeMembers: string[];
  /** Path to invalidate / refresh after a mutation. */
  refresh: (parentPath: string) => Promise<void> | void;
  closeMenu: () => void;
  notify: (kind: 'success' | 'error' | 'info', title: string, message?: string) => void;
}

/** Special id reserved for the divider row. */
export const DIVIDER_ID = 'divider';

/** Reserved id for the workspace-root context (no file entry). */
export const WORKSPACE_TARGET_KIND = 'workspace';

/**
 * Convenience predicate for menu builders that need to act on the
 * entry target — a tagged-union selector could replace this if more
 * target kinds appear.
 */
export function isDivider(item: MenuItem): boolean {
  return item.id === DIVIDER_ID;
}
