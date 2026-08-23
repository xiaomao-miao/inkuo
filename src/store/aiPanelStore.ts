import { create } from 'zustand';
import { persist } from 'zustand/middleware';

import type {
  ActiveToolCall,
  BuildProgress,
  ChatMessage,
  ChatMode,
  ChatSession,
  CurrentDiff,
  FeatureToggleId,
  FeatureToggleMap,
  MessageRole,
  MessageToolCall,
  MessageToolResult,
  OutputItem,
  SearchResult,
} from '../types';

import { editorDiffActions } from './editorStore';
import type { AIPanelState } from './aiPanelStore.types';
import {
  createDiffSlice,
  createMessageSlice,
  createSessionSlice,
  createSubagentSlice,
  createToolCallSlice,
  createUiSlice,
} from './aiPanelStore/slices';

function pickPersistedUiBits(
  persistedState: unknown,
  currentState: AIPanelState,
): AIPanelState {
  const persisted = (persistedState ?? {}) as Partial<Pick<
    AIPanelState,
    'isOpen' | 'activeTab' | 'panelDisplayMode'
  >>;
  return {
    ...currentState,
    isOpen: persisted.isOpen ?? currentState.isOpen,
    activeTab: persisted.activeTab ?? currentState.activeTab,
    panelDisplayMode: persisted.panelDisplayMode ?? currentState.panelDisplayMode,
  };
}

export const useAIPanelStore = create<AIPanelState>()(
  persist(
    (...args) => ({
      ...createUiSlice(...args),
      ...createSessionSlice(...args),
      ...createMessageSlice(...args),
      ...createToolCallSlice(...args),
      ...createDiffSlice(editorDiffActions)(...args),
      ...createSubagentSlice(...args),
    }),
    {
      name: 'inkuo-ai-panel',
      version: 1,
      partialize: (state) => ({
        isOpen: state.isOpen,
        activeTab: state.activeTab,
        panelDisplayMode: state.panelDisplayMode,
      }),
      merge: pickPersistedUiBits,
    }
  )
);

export type {
  ActiveToolCall,
  BuildProgress,
  ChatMessage,
  ChatMode,
  ChatSession,
  CurrentDiff,
  FeatureToggleId,
  FeatureToggleMap,
  MessageRole,
  MessageToolCall,
  MessageToolResult,
  OutputItem,
  SearchResult,
};
