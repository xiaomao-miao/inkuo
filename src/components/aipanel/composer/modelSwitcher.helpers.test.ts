// Unit tests for the pure helpers around `<ModelSwitcher>`.
//
// The switcher has a small set of pure transformations:
//   - encodeSelectValue / decodeSelectValue round-trip
//   - currentSelectValue picks the highlighted option
//   - activeSelectionLabel summarizes the chosen model
//   - shouldHideSwitcher returns true when both groups are empty
//
// All four are pure (no React, no store reads) so we can test them
// directly.

import { describe, expect, it } from 'vitest';

import {
  activeSelectionLabel,
  currentSelectValue,
  decodeSelectValue,
  encodeSelectValue,
  shouldHideSwitcher,
} from './modelSwitcher.helpers';

describe('encodeSelectValue / decodeSelectValue', () => {
  it('round-trips a cloud id', () => {
    const encoded = encodeSelectValue('cloud', 'gpt-4');
    expect(encoded).toBe('cloud:gpt-4');
    expect(decodeSelectValue(encoded)).toEqual({ kind: 'cloud', id: 'gpt-4' });
  });

  it('round-trips a local config id', () => {
    const encoded = encodeSelectValue('local', 'cfg-1');
    expect(encoded).toBe('local:cfg-1');
    expect(decodeSelectValue(encoded)).toEqual({ kind: 'local', id: 'cfg-1' });
  });

  it('returns null for an empty string', () => {
    expect(decodeSelectValue('')).toBeNull();
  });

  it('returns null when the kind is unknown', () => {
    expect(decodeSelectValue('magic:foo')).toBeNull();
    expect(decodeSelectValue('cloud')).toBeNull();
  });

  it('returns null when the id is empty', () => {
    expect(decodeSelectValue('cloud:')).toBeNull();
    expect(decodeSelectValue('local:')).toBeNull();
  });

  it('uses only the first two colon-separated parts (split limit=2)', () => {
    // The decoder uses `split(':', 2)` so an id containing a `:`
    // would be truncated. Document the limitation rather than
    // papering over it — call sites must use id formats that don't
    // contain `:`.
    expect(decodeSelectValue('cloud:foo:bar')).toEqual({
      kind: 'cloud',
      id: 'foo',
    });
  });
});

describe('currentSelectValue', () => {
  it('returns the cloud id when cloud mode is on and a model is selected', () => {
    expect(
      currentSelectValue({
        cloudMode: true,
        activeCloudModelId: 'gpt-4',
        activeApiConfigId: null,
        fallbackLocalConfigId: 'cfg-1',
      }),
    ).toBe('cloud:gpt-4');
  });

  it('returns empty string when cloud mode is on but no model is selected', () => {
    expect(
      currentSelectValue({
        cloudMode: true,
        activeCloudModelId: null,
        activeApiConfigId: 'cfg-1',
        fallbackLocalConfigId: 'cfg-2',
      }),
    ).toBe('');
  });

  it('returns the active local id when in local mode', () => {
    expect(
      currentSelectValue({
        cloudMode: false,
        activeCloudModelId: null,
        activeApiConfigId: 'cfg-active',
        fallbackLocalConfigId: 'cfg-fallback',
      }),
    ).toBe('local:cfg-active');
  });

  it('falls back to the first config when local mode has no active id', () => {
    expect(
      currentSelectValue({
        cloudMode: false,
        activeCloudModelId: null,
        activeApiConfigId: null,
        fallbackLocalConfigId: 'cfg-first',
      }),
    ).toBe('local:cfg-first');
  });

  it('returns empty string when nothing is available', () => {
    expect(
      currentSelectValue({
        cloudMode: false,
        activeCloudModelId: null,
        activeApiConfigId: null,
        fallbackLocalConfigId: null,
      }),
    ).toBe('');
  });
});

describe('activeSelectionLabel', () => {
  it('shows the cloud model name when in cloud mode', () => {
    expect(
      activeSelectionLabel(true, {
        activeCloudModelName: 'GPT-4',
        activeLocalConfigName: 'Local',
        firstLocalConfigName: 'First',
      }),
    ).toBe('云端 · GPT-4');
  });

  it('shows 未选择 when the cloud model name is missing', () => {
    expect(
      activeSelectionLabel(true, {
        activeCloudModelName: null,
        activeLocalConfigName: null,
        firstLocalConfigName: null,
      }),
    ).toBe('云端 · 未选择');
  });

  it('shows the active local config name when in local mode', () => {
    expect(
      activeSelectionLabel(false, {
        activeCloudModelName: null,
        activeLocalConfigName: 'My Config',
        firstLocalConfigName: 'Another Config',
      }),
    ).toBe('本地 · My Config');
  });

  it('falls back to the first local config when the active one is missing', () => {
    expect(
      activeSelectionLabel(false, {
        activeCloudModelName: null,
        activeLocalConfigName: null,
        firstLocalConfigName: 'First Config',
      }),
    ).toBe('本地 · First Config');
  });

  it('shows 未选择 when nothing is configured', () => {
    expect(
      activeSelectionLabel(false, {
        activeCloudModelName: null,
        activeLocalConfigName: null,
        firstLocalConfigName: null,
      }),
    ).toBe('本地 · 未选择');
  });
});

describe('shouldHideSwitcher', () => {
  it('returns true when both groups are empty', () => {
    expect(shouldHideSwitcher({ hasCloudOptions: false, hasLocalOptions: false })).toBe(true);
  });

  it('returns false when only cloud options are available', () => {
    expect(shouldHideSwitcher({ hasCloudOptions: true, hasLocalOptions: false })).toBe(false);
  });

  it('returns false when only local options are available', () => {
    expect(shouldHideSwitcher({ hasCloudOptions: false, hasLocalOptions: true })).toBe(false);
  });

  it('returns false when both groups are available', () => {
    expect(shouldHideSwitcher({ hasCloudOptions: true, hasLocalOptions: true })).toBe(false);
  });
});