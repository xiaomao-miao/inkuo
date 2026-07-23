// `contextMenu/` — split out of the original monolithic
// `ContextMenu.tsx`.
//
// Modules:
//   - `ContextMenu.tsx` — orchestrator (positioning + dismiss handlers)
//   - `MenuRow.tsx`     — single-row component
//   - `menuBuilders.tsx` — `buildWorkspaceMenu` / `buildEntryMenu`
//   - `geometry.ts`     — viewport clamp helper
//   - `pathHelpers.ts`  — basename / parent / dedup-name helpers
//   - `types.ts`        — `MenuItem` / `Position` / context types
//
// Re-exported as a single surface so callers can `import { ContextMenu } from './contextMenu'`.

export { ContextMenu } from './ContextMenu';
export { MenuRow } from './MenuRow';
export { buildEntryMenu, buildWorkspaceMenu } from './menuBuilders';
export { clampToViewport } from './geometry';
export {
  basename,
  fileExtension,
  fileStem,
  joinPath,
  parentPath,
  uniqueSiblingName,
} from './pathHelpers';
export type {
  MenuBuilderContext,
  MenuItem,
  Position,
} from './types';
export { DIVIDER_ID, WORKSPACE_TARGET_KIND, isDivider } from './types';