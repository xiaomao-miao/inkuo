// Unit tests for the toggle registry helpers.
//
// The `TOGGLES` constant itself is small (currently two entries) so
// we lean on a smoke-test rather than exhaustive enumeration — the
// rule helpers (`isToggleDisabled`, `toggleTooltip`) are the part
// that benefits most from explicit coverage because they encode
// the disabled-state logic the UI relies on.

import { describe, expect, it } from 'vitest';
import React from 'react';
import type { ChatMode } from '../../../types';

import {
  isToggleDisabled,
  TOGGLES,
  toggleTooltip,
  type ToggleSpec,
} from './toggles';

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
    expect(
      isToggleDisabled(stubSpec(), { sessionId: null, mode: 'ask' }),
    ).toBe(true);
    expect(
      isToggleDisabled(stubSpec(), { sessionId: '', mode: 'ask' }),
    ).toBe(true);
  });

  it('disables when an explicit `disabled` flag is set', () => {
    expect(
      isToggleDisabled(stubSpec(), {
        sessionId: 'session-1',
        disabled: true,
        mode: 'ask',
      }),
    ).toBe(true);
  });

  it('disables when the spec declares the current mode unusable', () => {
    expect(
      isToggleDisabled(stubSpec({ disabledIn: ['plan'] }), {
        sessionId: 'session-1',
        mode: 'plan',
      }),
    ).toBe(true);
  });

  it('does not disable for modes not in `disabledIn`', () => {
    expect(
      isToggleDisabled(stubSpec({ disabledIn: ['plan'] }), {
        sessionId: 'session-1',
        mode: 'ask',
      }),
    ).toBe(false);
    expect(
      isToggleDisabled(stubSpec({ disabledIn: ['plan'] }), {
        sessionId: 'session-1',
        mode: 'agent',
      }),
    ).toBe(false);
  });

  it('combines rules — any of them disabled → disabled', () => {
    expect(
      isToggleDisabled(
        stubSpec({ disabledIn: ['plan'] }),
        { sessionId: null, mode: 'plan' },
      ),
    ).toBe(true);
    expect(
      isToggleDisabled(
        stubSpec({ disabledIn: ['plan'] }),
        { sessionId: 'session-1', mode: 'plan' },
      ),
    ).toBe(true);
  });

  it('is enabled only when every rule passes', () => {
    expect(
      isToggleDisabled(stubSpec(), {
        sessionId: 'session-1',
        mode: 'agent',
      }),
    ).toBe(false);
    expect(
      isToggleDisabled(stubSpec(), {
        sessionId: 'session-1',
        mode: 'ask',
      }),
    ).toBe(false);
  });
});

describe('toggleTooltip', () => {
  it('returns the disabled reason when the row is disabled', () => {
    expect(
      toggleTooltip(
        stubSpec({ disabledReason: 'plan mode does not support KB.' }),
        true,
      ),
    ).toBe('plan mode does not support KB.');
  });

  it('returns a generic fallback when no reason is provided', () => {
    expect(toggleTooltip(stubSpec(), true)).toBe('当前模式不可用');
  });

  it('returns the hint when the row is enabled', () => {
    expect(toggleTooltip(stubSpec({ hint: 'do the thing' }), false)).toBe(
      'do the thing',
    );
  });
});

describe('mode coverage', () => {
  const allModes: ChatMode[] = ['ask', 'agent', 'plan'];

  it('every mode has at least one toggle either enabled or disabled deterministically', () => {
    for (const mode of allModes) {
      for (const spec of TOGGLES) {
        const disabled = isToggleDisabled(spec, {
          sessionId: 'session-1',
          mode,
        });
        expect(typeof disabled).toBe('boolean');
      }
    }
  });
});