import type { ChatMessage, ChatMode, ChatSession } from '../../store';
import { useChatInputState } from './useChatInputState';
import { useChatSessionActions } from './useChatSessionActions';

interface UseChatComposerArgs {
  activeSession: ChatSession | undefined;
  mode: ChatMode;
  messages: ChatMessage[];
  isStreaming: boolean;
}

export function useChatComposer({
  activeSession,
  mode,
  messages,
  isStreaming,
}: UseChatComposerArgs) {
  const {
    input,
    setInput,
    editingMessageId,
    editingContent,
    setEditingContent,
    clearEditingState,
    startEdit,
    cancelEdit,
  } = useChatInputState();

  const {
    handleSend,
    handleStop,
    cycleMode,
    handleSaveEdit,
    handleApplyPlan,
    handleAdjustPlan,
    handleSavePlan,
    destroySessionPlanFiles,
  } = useChatSessionActions({
    activeSession,
    mode,
    messages,
    isStreaming,
    input,
    setInput,
    editingMessageId,
    editingContent,
    clearEditingState,
  });

  return {
    input,
    setInput,
    editingMessageId,
    editingContent,
    setEditingContent,
    handleSend,
    handleStop,
    cycleMode,
    handleStartEdit: startEdit,
    handleCancelEdit: cancelEdit,
    handleSaveEdit,
    handleApplyPlan,
    handleAdjustPlan,
    handleSavePlan,
    destroySessionPlanFiles,
  };
}
