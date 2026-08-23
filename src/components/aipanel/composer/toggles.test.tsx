// Unit tests for the toggle registry helpers.
//
// The `TOGGLES` constant itself is small (currently three entries) so
// we lean on a smoke-test rather than exhaustive enumeration — the
// rule helper (`isToggleDisabled`) is the part that benefits most
// from explicit coverage because it encodes the disabled-state logic
// the UI relies on.

import { describe, expect, it } from 'vitest';
import React from 'react';

import { isToggleDisabled, TOGGLES, toggleTooltip, type ToggleSpec } from './toggles';

function stubSpec(overrides: Partial<ToggleSpec> = {}): ToggleSpec {
  return {
    id: 'kb_strict',
    label: 'kb',
    hint: 'kb hint',
    icon: React.createElement('span'),
    ...overrides,
  };
}

describe('TOGGLES', () => {
  it('contains well-known entries', () => {
    const ids = TOGGLES.map((t) => t.id);
    expect(ids).toContain('kb_strict');
    expect(ids).toContain('web_search');
    expect(ids).toContain('sandbox');
  });

  it('every entry has an icon, a label, and a hint', () => {
    for (const t of TOGGLES) {
      expect(t.icon).toBeDefined();
      expect(t.label.length).toBeGreaterThan(0);
      expect(t.hint.length).toBeGreaterThan(0);
    }
  });
});

describe('isToggleDisabled', () => {
  it('disables when no session id is present', () => {
    expect(isToggleDisabled({ sessionId: null })).toBe(true);
    expect(isToggleDisabled({ sessionId: '' })).toBe(true);
  });

  it('disables when an explicit `disabled` flag is set', () => {
    expect(isToggleDisabled({ sessionId: 'session-1', disabled: true })).toBe(true);
  });

  it('is enabled only when a session id is present and no flag is set', () => {
    expect(isToggleDisabled({ sessionId: 'session-1' })).toBe(false);
  });
});

describe('toggleTooltip', () => {
  it('returns the generic fallback when the row is disabled', () => {
    expect(toggleTooltip(stubSpec(), true)).toBe('当前模式不可用');
  });

  it('returns the hint when the row is enabled', () => {
    expect(toggleTooltip(stubSpec({ hint: 'do the thing' }), false)).toBe(
      'do the thing',
    );
  });
});
