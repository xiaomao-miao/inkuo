// Re-export of `./contextMenu`.
//
// The original monolithic `ContextMenu.tsx` has been split into:
//   - `ContextMenu.tsx`  — orchestrator (positioning + dismiss handlers)
//   - `MenuRow.tsx`      — single-row component
//   - `menuBuilders.tsx` — `buildWorkspaceMenu` / `buildEntryMenu`
//   - `geometry.ts`      — viewport clamp helper
//   - `pathHelpers.ts`   — basename / parent / dedup-name helpers
//   - `types.ts`         — `MenuItem` / `Position` / context types
//
// Existing callers (`App` / layout containers) keep working with the
// single import path. New consumers can import the builder helpers
// directly for tests / programmatic menu construction.

export * from './contextMenu';