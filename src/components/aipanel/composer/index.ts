// `composer/` — split from the original monolithic `ChatInput.tsx`.
//
// Modules:
//   - `ChatInput.tsx`               — orchestrator (textarea + send button + bottom row)
//   - `ComposerToggleRows.tsx`      — the expand-mode toggle rows
//   - `ActiveToggleStrip.tsx`       — quiet header line when collapsed
//   - `ModelSwitcher.tsx`           — cloud / local model picker
//   - `useComposerPanelAnimation.ts` — height pin + transition race
//   - `useComposerDismiss.ts`        — outside click / focus-out / ESC
//   - `toggles.ts`                  — toggle spec registry (single source of truth)
//   - `modelSwitcher.helpers.ts`    — pure helpers for the picker
//
// Public surface re-exported here so callers can keep the
// `ChatInput` import path working.

export { ChatInput } from './ChatInput';
export { ComposerToggleRows } from './ComposerToggleRows';
export { ActiveToggleStrip } from './ActiveToggleStrip';
export { ModelSwitcher } from './ModelSwitcher';
export { TOGGLES, isToggleDisabled, toggleTooltip, type ToggleSpec } from './toggles';
export {
  decodeSelectValue,
  encodeSelectValue,
  activeSelectionLabel,
  currentSelectValue,
  shouldHideSwitcher,
  type ModelKind,
} from './modelSwitcher.helpers';
export { useComposerPanelAnimation } from './useComposerPanelAnimation';
export { useComposerDismiss } from './useComposerDismiss';