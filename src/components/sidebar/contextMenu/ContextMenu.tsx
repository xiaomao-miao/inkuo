// Workspace context menu — root orchestrator.
//
// Responsibilities here are limited to:
//   1. Reading the sidebar / clipboard / settings state needed to
//      assemble the builder context.
//   2. Picking the right builder (`buildWorkspaceMenu` vs
//      `buildEntryMenu`) based on the current `target`.
//   3. Positioning the floating panel at the click coordinates and
//      clamping to the viewport.
//   4. Wiring the dismiss handlers (outside click, Escape, scroll,
//      resize).
//
// All menu shape / side-effect logic lives in `./menuBuilders.tsx`,
// which keeps this component scannable. The row renderer lives in
// `./MenuRow.tsx`.

import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react';
import { createPortal } from 'react-dom';

import {
  useContextMenuStore,
  useNotificationStore,
  useSidebarStore,
} from '../../../store';
import { loadDirectoryChildren } from '../../../services/workspace';
import { reportError } from '../../../utils/errors';

import { clampToViewport } from './geometry';
import { buildEntryMenu, buildWorkspaceMenu } from './menuBuilders';
import { MenuRow } from './MenuRow';
import type { MenuBuilderContext, MenuItem, Position } from './types';
import styles from './ContextMenu.module.css';

export const ContextMenu = () => {
  const target = useContextMenuStore((s) => s.target);
  const menuRef = useRef<HTMLDivElement | null>(null);
  const [pos, setPos] = useState<Position>({ left: 0, top: 0 });

  const close = useContextMenuStore((s) => s.close);
  const workspacePath = useSidebarStore((s) => s.workspacePath);
  const openTabs = useSidebarStore((s) => s.openTabs);
  const selectedFile = useSidebarStore((s) => s.selectedFile);
  const knowledgeBase = useSidebarStore((s) => s.knowledgeBase);
  const pushNotification = useNotificationStore((s) => s.pushNotification);

  // Position the menu at the click coordinates, then clamp to viewport.
  useLayoutEffect(() => {
    if (!target) return;
    setPos({ left: target.x, top: target.y });
  }, [target]);

  useEffect(() => {
    if (!target) return;
    // After first paint, clamp to viewport.
    requestAnimationFrame(() => {
      if (menuRef.current) {
        setPos(clampToViewport(target.x, target.y, menuRef.current));
      }
    });
  }, [target]);

  // Close on outside click, Escape, scroll, resize.
  useEffect(() => {
    if (!target) return;
    const onMouseDown = (e: MouseEvent) => {
      if (menuRef.current && !menuRef.current.contains(e.target as Node)) {
        close();
      }
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.preventDefault();
        close();
      }
    };
    const onScroll = () => close();
    const onResize = () => close();
    window.addEventListener('mousedown', onMouseDown, true);
    window.addEventListener('keydown', onKey);
    window.addEventListener('scroll', onScroll, true);
    window.addEventListener('resize', onResize);
    return () => {
      window.removeEventListener('mousedown', onMouseDown, true);
      window.removeEventListener('keydown', onKey);
      window.removeEventListener('scroll', onScroll, true);
      window.removeEventListener('resize', onResize);
    };
  }, [target, close]);

  const refresh = useCallback(
    async (parentPath: string) => {
      // After a paste / copy / delete we re-read the parent directory so
      // the tree shows the new state immediately, instead of waiting on
      // the (now 500 ms) backend poll-watcher. Atomic replace — the tree
      // never sees an empty row mid-refresh.
      try {
        const children = await loadDirectoryChildren(parentPath);
        useSidebarStore.getState().setCachedChildren(parentPath, children);
      } catch (err) {
        useSidebarStore.getState().evictCachedChildren(parentPath);
        reportError('contextmenu-refresh', err);
      }
    },
    [],
  );

  const notify = useCallback(
    (kind: 'success' | 'error' | 'info', title: string, message?: string) => {
      pushNotification({
        kind,
        title,
        ...(message ? { message } : {}),
      });
    },
    [pushNotification],
  );

  const items = useMemo<MenuItem[]>(() => {
    if (!target) return [];
    const ctx: MenuBuilderContext = {
      workspacePath,
      openTabs,
      selectedFile,
      knowledgeMembers: knowledgeBase?.members ?? [],
      refresh,
      closeMenu: close,
      notify,
    };
    if (target.kind === 'workspace') {
      return buildWorkspaceMenu(ctx);
    }
    if (target.entry) {
      return buildEntryMenu(target.entry, ctx);
    }
    return [];
  }, [target, workspacePath, openTabs, selectedFile, knowledgeBase, refresh, close, notify]);

  if (!target || typeof document === 'undefined') return null;

  return createPortal(
    <div
      ref={menuRef}
      className={styles.contextMenu}
      style={{ left: pos.left, top: pos.top }}
      role="menu"
    >
      {items.map((item) => (
        <MenuRow key={item.id} item={item} />
      ))}
    </div>,
    document.body,
  );
};
