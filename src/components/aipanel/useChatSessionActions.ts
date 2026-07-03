import { invoke } from '@tauri-apps/api/core';
import { listen, UnlistenFn } from '@tauri-apps/api/event';
import { useCallback, useRef } from 'react';
import {
  useAIPanelStore,
  useSidebarStore,
  useSettingsStore,
  useBaselineStore,
  type ChatMessage,
  type ChatMode,
  type ChatSession,
} from '../../store';
import { buildConversationHistory } from './messageTransform';
import { extractErrorMessage } from '../../utils/errors';
import {
  collectWorkspaceFiles,
  createSnapshot,
  restoreSnapshot,
} from '../../services/snapshots';
import { useNotificationStore } from '../../store/notificationStore';

interface UseChatSessionActionsArgs {
  activeSession: ChatSession | undefined;
  mode: ChatMode;
  messages: ChatMessage[];
  isStreaming: boolean;
  input: string;
  setInput: (value: string) => void;
  editingMessageId: string | null;
  editingContent: string;
  clearEditingState: () => void;
}

interface AgentStreamEvent {
  session_id: string;
  message_id: string;
  event_type: string;
  done?: boolean;
  error?: string;
  final_content?: string;
}

export function useChatSessionActions({
  activeSession,
  mode,
  messages,
  isStreaming,
  input,
  setInput,
  editingMessageId,
  editingContent,
  clearEditingState,
}: UseChatSessionActionsArgs) {
  const addMessage = useAIPanelStore((state) => state.addMessage);
  const updateMessage = useAIPanelStore((state) => state.updateMessage);
  const setIsStreaming = useAIPanelStore((state) => state.setIsStreaming);
  const truncateMessagesAfter = useAIPanelStore((state) => state.truncateMessagesAfter);
  const clearToolCalls = useAIPanelStore((state) => state.clearToolCalls);
  const pushNotification = useNotificationStore((s) => s.pushNotification);

  // Keep references so event listeners can read the latest values.
  const recordBaseline = useRef(useBaselineStore.getState().recordBaseline);
  const consumeBaseline = useRef(useBaselineStore.getState().consumeBaseline);

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

    const workspacePath = useSidebarStore.getState().workspacePath || undefined;
    const { apiConfigs, activeApiConfigId, snapshot, agent_max_iterations } = useSettingsStore.getState().settings;
    const activeConfig = apiConfigs.find((config) => config.id === activeApiConfigId) ?? apiConfigs[0];
    const conversationHistory = buildConversationHistory(messages);

    // Auto-baseline: when sending a brand-new (not re-edited) agent-mode
    // instruction, capture a snapshot so re-editing the user message can
    // roll the workspace back.  Failure here is non-fatal — we just skip
    // the baseline and the user can still create one manually.
    if (
      !isEditing &&
      mode === 'agent' &&
      snapshot.autoBaseline &&
      workspacePath
    ) {
      try {
        const files = await collectWorkspaceFiles(workspacePath);
        if (files.length > 0) {
          const label = `AI 基线: ${instruction.slice(0, 30)}`;
          const manifest = await createSnapshot(
            workspacePath,
            label,
            'ai_baseline',
            files
          );
          recordBaseline.current(userMessageId, manifest.snapshotId);
        }
      } catch (err) {
        // Best-effort: log and continue.
        // eslint-disable-next-line no-console
        console.warn('[snapshot] baseline creation failed', err);
      }
    }

    // Subscribe to the agent stream's terminal events so we can consume
    // the baseline when the run completes successfully.  We keep the
    // listener open until the matching message id is seen finished.
    let unlistenAgent: UnlistenFn | null = null;
    if (mode === 'agent' || mode === 'plan' || mode === 'ask') {
      listen<AgentStreamEvent>('ai://stream', (event) => {
        const payload = event.payload;
        if (!payload) return;
        if (payload.session_id !== sessionId) return;
        if (payload.message_id !== assistantMessageId) return;
        if (payload.event_type === 'done') {
          // Successful completion — drop the baseline.
          consumeBaseline.current(userMessageId);
          if (unlistenAgent) {
            unlistenAgent();
            unlistenAgent = null;
          }
        } else if (payload.event_type === 'error') {
          // Keep the baseline so the user can re-edit and retry.
          if (unlistenAgent) {
            unlistenAgent();
            unlistenAgent = null;
          }
        }
      }).then((fn) => {
        unlistenAgent = fn;
      });
    }

    try {
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
          useAIPanelStore
            .getState()
            .setErrorMessage(sessionId, assistantMessageId, `抱歉，发生了错误：${extractErrorMessage(err)}`);
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
        // Forward the user-configured agent loop cap. The Rust side clamps
        // / defaults internally; we just send the raw value (1–200).
        maxIterations: agent_max_iterations,
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
        updateMessage(sessionId, assistantMessageId, `抱歉，发生了错误：${extractErrorMessage(err)}`);
        setIsStreaming(sessionId, false);
      });
    } catch (err) {
      updateMessage(sessionId, assistantMessageId, `抱歉，发生了错误：${extractErrorMessage(err)}`);
      setIsStreaming(sessionId, false);
    }
  }, [activeSession, input, isStreaming, editingMessageId, updateMessage, addMessage, clearEditingState, setInput, setIsStreaming, clearToolCalls, messages, mode]);

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

  const handleSaveEdit = useCallback(async () => {
    if (!activeSession || !editingMessageId || !editingContent.trim() || isStreaming) return;

    const newContent = editingContent.trim();
    const workspacePath = useSidebarStore.getState().workspacePath;

    // Roll the workspace back to the baseline that was captured at the
    // start of the original agent run, if any.  Failure is non-fatal —
    // the user will still get the truncated conversation and re-sent
    // instruction, but with files at their current state.
    if (workspacePath) {
      const baselineId = useBaselineStore.getState().peekBaseline(editingMessageId);
      if (baselineId) {
        try {
          await restoreSnapshot(workspacePath, baselineId);
        } catch (err) {
          pushNotification({
            kind: 'error',
            title: '回滚基线失败',
            message: extractErrorMessage(err),
          });
        }
      }
    }

    truncateMessagesAfter(activeSession.id, editingMessageId);
    clearEditingState();
    setInput(newContent);
    await sendMessage(newContent);
  }, [activeSession, editingMessageId, editingContent, isStreaming, truncateMessagesAfter, clearEditingState, setInput, sendMessage, pushNotification]);

  return {
    handleSend,
    handleStop,
    cycleMode,
    handleSaveEdit,
  };
}
