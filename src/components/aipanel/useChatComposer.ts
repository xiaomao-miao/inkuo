import { invoke } from '@tauri-apps/api/core';
import { useCallback, useState } from 'react';
import {
  useAIPanelStore,
  useSidebarStore,
  useSettingsStore,
  type ChatMessage,
  type ChatMode,
  type ChatSession,
} from '../../store';
import { buildConversationHistory } from './messageTransform';

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
  const addMessage = useAIPanelStore((state) => state.addMessage);
  const updateMessage = useAIPanelStore((state) => state.updateMessage);
  const setIsStreaming = useAIPanelStore((state) => state.setIsStreaming);
  const truncateMessagesAfter = useAIPanelStore((state) => state.truncateMessagesAfter);
  const clearToolCalls = useAIPanelStore((state) => state.clearToolCalls);

  const [input, setInput] = useState('');
  const [editingMessageId, setEditingMessageId] = useState<string | null>(null);
  const [editingContent, setEditingContent] = useState('');

  const clearEditingState = useCallback(() => {
    setEditingMessageId(null);
    setEditingContent('');
  }, []);

  const sendMessage = useCallback(async (instructionOverride?: string) => {
    const instruction = (instructionOverride ?? input).trim();
    if (!activeSession || !instruction || isStreaming) return;

    const sessionId = activeSession.id;
    const isEditing = editingMessageId !== null;
    const userMessageId = isEditing ? editingMessageId : crypto.randomUUID();
    const assistantMessageId = crypto.randomUUID();

    const userMessage: ChatMessage = {
      id: userMessageId,
      role: 'user',
      content: instruction,
      timestamp: Date.now(),
      outputItems: [],
    };

    const assistantPlaceholder: ChatMessage = {
      id: assistantMessageId,
      role: 'assistant',
      timestamp: Date.now(),
      outputItems: [],
    };

    if (isEditing) {
      updateMessage(sessionId, userMessageId, instruction);
    } else {
      addMessage(sessionId, userMessage);
    }
    addMessage(sessionId, assistantPlaceholder);

    clearEditingState();
    setInput('');
    setIsStreaming(sessionId, true);
    clearToolCalls(sessionId);

    try {
      const workspacePath = useSidebarStore.getState().workspacePath || undefined;
      const { apiConfigs, activeApiConfigId } = useSettingsStore.getState().settings;
      const activeConfig = apiConfigs.find((config) => config.id === activeApiConfigId) ?? apiConfigs[0];
      const conversationHistory = buildConversationHistory(messages);

      if (mode === 'knowledge') {
        invoke('ai_chat_stream', {
          sessionId,
          messageId: assistantMessageId,
          mode,
          instruction,
          originalText: '',
          workspacePath,
          configInput: {
            provider: activeConfig.provider,
            api_key: activeConfig.apiKey,
            base_url: activeConfig.baseUrl,
            model: activeConfig.model,
            temperature: activeConfig.temperature,
            max_tokens: activeConfig.maxTokens,
          },
        }).catch((err) => {
          useAIPanelStore.getState().setErrorMessage(sessionId, assistantMessageId, `抱歉，发生了错误：${err}`);
          setIsStreaming(sessionId, false);
        });
        return;
      }

      invoke('ai_agent_stream', {
        sessionId,
        messageId: assistantMessageId,
        instruction,
        workspacePath,
        readOnly: mode !== 'agent',
        history: conversationHistory,
        configInput: {
          provider: activeConfig.provider,
          api_key: activeConfig.apiKey,
          base_url: activeConfig.baseUrl,
          model: activeConfig.model,
          temperature: activeConfig.temperature,
          max_tokens: activeConfig.maxTokens,
        },
      }).catch((err) => {
        updateMessage(sessionId, assistantMessageId, `抱歉，发生了错误：${err}`);
        setIsStreaming(sessionId, false);
      });
    } catch (err) {
      updateMessage(sessionId, assistantMessageId, `抱歉，发生了错误：${err}`);
      setIsStreaming(sessionId, false);
    }
  }, [activeSession, input, isStreaming, editingMessageId, updateMessage, addMessage, clearEditingState, setIsStreaming, clearToolCalls, messages, mode]);

  const handleSend = useCallback(async () => {
    await sendMessage();
  }, [sendMessage]);

  const handleStop = useCallback(async () => {
    if (!activeSession) return;
    try {
      if (mode === 'agent') {
        await invoke('ai_agent_cancel', { sessionId: activeSession.id });
      } else {
        await invoke('ai_stream_cancel', { sessionId: activeSession.id });
      }
    } catch {
      // ignore
    }
  }, [activeSession, mode]);

  const cycleMode = useCallback(() => {
    if (!activeSession) return;
    const order: ChatMode[] = ['ask', 'plan', 'agent', 'knowledge'];
    const idx = order.indexOf(mode);
    useAIPanelStore.getState().setSessionMode(activeSession.id, order[(idx + 1) % order.length]);
  }, [activeSession, mode]);

  const handleStartEdit = useCallback((messageId: string, currentContent: string) => {
    setEditingMessageId(messageId);
    setEditingContent(currentContent);
    setInput(currentContent);
  }, []);

  const handleCancelEdit = useCallback(() => {
    clearEditingState();
    setInput('');
  }, [clearEditingState]);

  const handleSaveEdit = useCallback(async () => {
    if (!activeSession || !editingMessageId || !editingContent.trim() || isStreaming) return;

    const newContent = editingContent.trim();
    truncateMessagesAfter(activeSession.id, editingMessageId);
    clearEditingState();
    setInput(newContent);
    await sendMessage(newContent);
  }, [activeSession, editingMessageId, editingContent, isStreaming, truncateMessagesAfter, clearEditingState, sendMessage]);

  return {
    input,
    setInput,
    editingMessageId,
    editingContent,
    setEditingContent,
    handleSend,
    handleStop,
    cycleMode,
    handleStartEdit,
    handleCancelEdit,
    handleSaveEdit,
  };
}
