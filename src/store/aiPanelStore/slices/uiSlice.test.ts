import { describe, expect, it } from 'vitest';
import { create } from 'zustand';
import type { AIPanelStateCreator, AIPanelUiSlice } from '../../aiPanelStore.types';
import { createUiSlice } from './uiSlice';

const buildStore = () => create<AIPanelUiSlice>()((...args) => ({
  ...createUiSlice(...args as Parameters<AIPanelStateCreator<AIPanelUiSlice>>),
}));

describe('ai panel display mode', () => {
  it('defaults to minimal and can reveal the detailed trace', () => {
    const store = buildStore();
    expect(store.getState().panelDisplayMode).toBe('minimal');
    store.getState().togglePanelDisplayMode();
    expect(store.getState().panelDisplayMode).toBe('detailed');
    store.getState().setPanelDisplayMode('minimal');
    expect(store.getState().panelDisplayMode).toBe('minimal');
  });
});
