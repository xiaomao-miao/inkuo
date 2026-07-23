// Toggle spec registry for the AI-panel composer.
//
// Each entry describes a feature toggle rendered inside the composer
// when it's expanded. The data is the single source of truth — the
// row UI (`ComposerToggleRows`) and the collapsed-mode hint strip
// (`ActiveToggleStrip`) both read from this list so they stay in
// sync. To add a new toggle, drop in a new entry.
//
// `disabledIn` / `disabledReason` carve out modes where the toggle is
// unusable (e.g. plan mode doesn't return grounded citations, so
// `kb_strict` is dimmed). The UI explains the rationale via the
// tooltip on the dimmed row.

import React from 'react';
import { Database, Globe } from 'lucide-react';

import type { ChatMode, FeatureToggleId } from '../../../types';

export interface ToggleSpec {
  id: FeatureToggleId;
  label: string;
  hint: string;
  icon: React.ReactNode;
  /** Modes in which this toggle is unusable. */
  disabledIn?: ChatMode[];
  disabledReason?: string;
}

/**
 * Active toggles rendered inside the composer when expanded.
 *
 * Single source of truth — `ComposerToggleRows` iterates this list to
 * render the rows; `ActiveToggleStrip` filters it to render the
 * collapsed-mode header.
 */
export const TOGGLES: ReadonlyArray<ToggleSpec> = [
  {
    id: 'kb_strict',
    label: '严格 KB 引用',
    hint: '回答必须基于知识库检索结果，末尾列出参考来源。',
    icon: <Database size={13} />,
    disabledIn: ['plan'],
    disabledReason: 'Plan 模式不返回引用型回答。',
  },
  {
    id: 'web_search',
    label: '联网搜索',
    hint: '允许 Agent 检索最新网页内容（后续需配置 API）。',
    icon: <Globe size={13} />,
  },
];

/**
 * Compute whether a toggle should be disabled for the given inputs.
 * Pulled out of `<ComposerToggleRows>` so the rule is testable and
 * can be shared with `ActiveToggleStrip` if the latter ever wants
 * to dim its badges too.
 */
export function isToggleDisabled(
  spec: ToggleSpec,
  args: { sessionId: string | null; disabled?: boolean; mode: ChatMode },
): boolean {
  const { sessionId, disabled, mode } = args;
  return (
    !!disabled ||
    sessionId === null ||
    sessionId === '' ||
    (spec.disabledIn?.includes(mode) ?? false)
  );
}

/**
 * Tooltip text for a toggle row. The helper exists so the rule is
 * consistent: dimmed rows show the disabled reason (with a generic
 * fallback), enabled rows show the hint that describes what the
 * toggle does.
 */
export function toggleTooltip(spec: ToggleSpec, disabled: boolean): string {
  return disabled ? spec.disabledReason ?? '当前模式不可用' : spec.hint;
}
