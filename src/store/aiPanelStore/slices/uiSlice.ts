//! UI shell slice of the AI panel store: panel open/closed state, active
//! tab, and the feature-toolbar expansion toggle. Tiny on purpose —
//! these flags are the only things persisted to localStorage by
//! `persist({ partialize })` in the root store.

import type { AIPanelState, AIPanelStateCreator } from '../../aiPanelStore.types';

export const createUiSlice: AIPanelStateCreator<Pick<AIPanelState, 'isOpen' | 'activeTab' | 'featureToolbarExpanded' | 'setIsOpen' | 'togglePanel' | 'setActiveTab' | 'setFeatureToolbarExpanded' | 'toggleFeatureToolbar'>> = (set) => ({
  isOpen: true,
  activeTab: 'chat',
  featureToolbarExpanded: false,
  setIsOpen: (open) => set({ isOpen: open }),
  togglePanel: () => set((state) => ({ isOpen: !state.isOpen })),
  setActiveTab: (tab) => set({ activeTab: tab }),
  setFeatureToolbarExpanded: (open) => set({ featureToolbarExpanded: open }),
  toggleFeatureToolbar: () =>
    set((state) => ({ featureToolbarExpanded: !state.featureToolbarExpanded })),
});
