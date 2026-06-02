import React, { useState, useRef, useEffect, useMemo, useCallback } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import {
  useAIPanelStore,
  useEditorStore,
  useSidebarStore,
  useSettingsStore,
  type ChatMode,
  type ChatMessage,
} from '../../store';
import { ChatHeader } from './ChatHeader';
import { ChatView } from './ChatView';
import { ChatInput } from './ChatInput';
import styles from './AIPanel.module.css';

export const AIPanel: React.FC = () => {
  const {
    sessions,
    activeSessionId,
    createSession,
    deleteSession,
    setActiveSession,
    setSessionMode,
    addMessage,
    updateMessage,
    setIsStreaming,
    clearMessages,
    truncateMessagesAfter,
    setIsOpen,
    clearToolCalls,
    setMessageDiff,
  } = useAIPanelStore();

  const activeSession = useMemo(
    () => sessions.find((s) => s.id === activeSessionId) ?? sessions[0],
    [sessions, activeSessionId]
  );

  const messages = activeSession?.messages ?? [];
  const isStreaming = activeSession?.isStreaming ?? false;
  const pendingDiff = activeSession?.pendingDiff ?? null;
  const mode: ChatMode = activeSession?.mode ?? 'ask';
  const activeToolCalls = activeSession?.activeToolCalls ?? [];

  const {} = useSidebarStore();
  const {} = useEditorStore();

  const [input, setInput] = useState('');

  // Track which user message is being edited
  const [editingMessageId, setEditingMessageId] = useState<string | null>(null);
  const [editingContent, setEditingContent] = useState('');

  const inputRef = useRef<HTMLTextAreaElement>(null);

  // Ref to track accumulated text content for the current streaming message
  const streamingContentRef = useRef<Record<string, string>>({});

  // Microtask batching for text deltas
  const pendingTextDeltasRef = useRef<Record<string, string>>({});
  const flushTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const pendingFlushRef = useRef<Set<string>>(new Set());

  const flushTextDeltas = useCallback(() => {
    const deltas = pendingTextDeltasRef.current;
    const toFlush = [...pendingFlushRef.current];
    if (toFlush.length === 0) return;

    pendingTextDeltasRef.current = {};
    pendingFlushRef.current = new Set();
    flushTimeoutRef.current = null;

    useAIPanelStore.setState((state) => ({
      sessions: state.sessions.map((s) => {
        const updatedMessages = s.messages.map((m) => {
          const delta = deltas[m.id];
          if (!delta) return m;
          const items = m.outputItems;
          const lastItem = items[items.length - 1];
          if (lastItem && lastItem.type === 'text') {
            const updated = { ...lastItem, content: lastItem.content + delta };
            return { ...m, outputItems: [...items.slice(0, -1), updated] };
          }
          return { ...m, outputItems: [...items, { type: 'text' as const, content: delta, isPendingMarkdown: true }] };
        });
        return { ...s, messages: updatedMessages };
      }),
    }));
  }, []);

  // eslint-disable-next-line react-hooks/exhaustive-deps
  const scheduleTextFlush = useCallback(() => {
    if (flushTimeoutRef.current !== null) return;
    flushTimeoutRef.current = setTimeout(flushTextDeltas, 16);
  }, [flushTextDeltas]);

  const flushAllPending = useCallback(() => {
    if (flushTimeoutRef.current !== null) {
      clearTimeout(flushTimeoutRef.current);
      flushTimeoutRef.current = null;
    }
    flushTextDeltas();
  }, [flushTextDeltas]);

  // Refs for event handlers
  const unlistenRef = useRef<(() => void) | null>(null);
  const isSettingUpRef = useRef(false);
  const modeRef = useRef(mode);

  useEffect(() => {
    modeRef.current = mode;
  }, [mode]);

  // Auto-resize textarea
  useEffect(() => {
    if (inputRef.current) {
      inputRef.current.style.height = 'auto';
      inputRef.current.style.height = `${Math.min(inputRef.current.scrollHeight, 150)}px`;
    }
  }, [input]);

  const handleSend = async () => {
    if (!activeSession || !input.trim() || isStreaming) return;

    const sessionId = activeSession.id;
    const instruction = input.trim();
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

    setEditingMessageId(null);
    setEditingContent('');
    setInput('');

    clearToolCalls(sessionId);

    try {
      if (mode === 'agent') {
        const workspacePath = useSidebarStore.getState().workspacePath || undefined;
        const { apiConfigs, activeApiConfigId } = useSettingsStore.getState().settings;
        const activeConfig = apiConfigs.find(c => c.id === activeApiConfigId) ?? apiConfigs[0];
        const conversationHistory = messages.map(m => {
          let textContent = '';
          if (m.role === 'tool') {
            textContent = m.content || '';
          } else if (m.outputItems && m.outputItems.length > 0) {
            textContent = m.outputItems.filter(item => item.type === 'text').map(item => item.content).join('');
          } else {
            textContent = m.content || '';
          }
          return { id: m.id, role: m.role, content: textContent, tool_calls: m.toolCalls, tool_call_id: m.toolCallId };
        });

        invoke('ai_agent_stream', {
          sessionId, messageId: assistantMessageId, instruction,
          workspacePath, readOnly: false, history: conversationHistory,
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
      } else {
        const workspacePath = useSidebarStore.getState().workspacePath || undefined;
        const { apiConfigs, activeApiConfigId } = useSettingsStore.getState().settings;
        const activeConfig = apiConfigs.find(c => c.id === activeApiConfigId) ?? apiConfigs[0];
        const conversationHistory = messages.map(m => {
          let textContent = '';
          if (m.role === 'tool') {
            textContent = m.content || '';
          } else if (m.outputItems && m.outputItems.length > 0) {
            textContent = m.outputItems.filter(item => item.type === 'text').map(item => item.content).join('');
          } else {
            textContent = m.content || '';
          }
          return { id: m.id, role: m.role, content: textContent, tool_calls: m.toolCalls, tool_call_id: m.toolCallId };
        });

        invoke('ai_agent_stream', {
          sessionId, messageId: assistantMessageId, instruction,
          workspacePath, readOnly: true, history: conversationHistory,
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
      }
    } catch (err) {
      updateMessage(sessionId, assistantMessageId, `抱歉，发生了错误：${err}`);
      setIsStreaming(sessionId, false);
    }
  };

  const handleStop = async () => {
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
  };

  const cycleMode = () => {
    if (!activeSession) return;
    const order: ChatMode[] = ['ask', 'plan', 'agent'];
    const idx = order.indexOf(mode);
    setSessionMode(activeSession.id, order[(idx + 1) % order.length]);
  };

  const handleStartEdit = (messageId: string, currentContent: string) => {
    setEditingMessageId(messageId);
    setEditingContent(currentContent);
    setInput(currentContent);
  };

  const handleCancelEdit = () => {
    setEditingMessageId(null);
    setEditingContent('');
    setInput('');
  };

  const handleSaveEdit = async () => {
    if (!activeSession || !editingMessageId || !editingContent.trim()) return;
    if (isStreaming) return;

    const newContent = editingContent.trim();
    const sessionId = activeSession.id;

    truncateMessagesAfter(sessionId, editingMessageId);

    setEditingMessageId(null);
    setEditingContent('');
    setInput(newContent);

    await handleSend();
  };

  // Streaming events
  useEffect(() => {
    const setupListener = async () => {
      if (unlistenRef.current || isSettingUpRef.current) return;
      isSettingUpRef.current = true;

      try {
        unlistenRef.current = await listen<any>('ai://stream', async (event) => {
          const payload = event.payload;
          const {
            session_id, message_id, event_type, content, done, summary,
            final_content, error, tool_call_id, tool_name, tool_args,
            diff_summary, office_file_modified,
          } = payload;

          if (!payload || !session_id || !message_id) return;

          // Handle error event
          if (event_type === 'error') {
            flushAllPending();
            delete streamingContentRef.current[message_id];
            useAIPanelStore.getState().setErrorMessage(session_id, message_id, error ?? '发生错误');
            return;
          }

          // Handle tool call start
          if (event_type === 'tool_call_start' && tool_call_id && tool_name) {
            let args = {};
            try { if (tool_args) args = JSON.parse(tool_args); } catch { /* ignore */ }

            const newToolCall = {
              id: tool_call_id, name: tool_name, arguments: args,
              status: 'executing' as const, startTime: Date.now(),
            };

            useAIPanelStore.getState().updateSession(session_id, (s) => ({
              ...s,
              messages: s.messages.map((m) =>
                m.id === message_id
                  ? {
                      ...m,
                      toolCalls: [...(m.toolCalls || []), { id: tool_call_id, name: tool_name, arguments: args }],
                      outputItems: [...m.outputItems, { type: 'tool_call_start' as const, toolCallId: tool_call_id, toolName: tool_name, arguments: args }],
                    }
                  : m
              ),
              activeToolCalls: [...s.activeToolCalls, newToolCall],
            }));
            return;
          }

          // Handle tool result
          if (event_type === 'tool_result' && tool_call_id) {
            const isError = !!error;
            const toolCall = useAIPanelStore.getState().sessions
              .find((s) => s.id === session_id)?.activeToolCalls.find((tc) => tc.id === tool_call_id);
            const duration = toolCall?.startTime ? Date.now() - toolCall.startTime : undefined;

            const toolResult = {
              toolCallId: tool_call_id, result: content || '', isError,
              duration, diffSummary: diff_summary ?? undefined,
            };

            useAIPanelStore.getState().updateSession(session_id, (s) => ({
              ...s,
              activeToolCalls: s.activeToolCalls.map((tc) =>
                tc.id === tool_call_id
                  ? { ...tc, status: isError ? 'error' : 'success', result: content, error: isError ? error : undefined, duration }
                  : tc
              ),
              messages: s.messages.map((m) =>
                m.id === message_id
                  ? {
                      ...m,
                      toolResults: [...(m.toolResults || []), toolResult],
                      outputItems: [...m.outputItems, { type: 'tool_result' as const, toolCallId: tool_call_id, status: isError ? 'error' : 'success', result: content || '', duration, diffSummary: diff_summary ?? undefined }],
                    }
                  : m
              ),
            }));

            // Add standalone tool message
            const toolMessage: ChatMessage = {
              id: `tool-${tool_call_id}-${crypto.randomUUID()}`,
              role: 'tool', content: content || '', toolCallId: tool_call_id,
              timestamp: Date.now(), outputItems: [],
            };
            addMessage(session_id, toolMessage);

            // Refresh editor buffer if office file modified
            if (office_file_modified) {
              const { path } = office_file_modified;
              const { invalidateOfficeBuffer } = useEditorStore.getState();
              const { setOpenTabDirty } = useSidebarStore.getState();
              invalidateOfficeBuffer(path);
              setOpenTabDirty(path, false);
            }
            return;
          }

          // Handle text delta
          if (typeof content === 'string' && content.length > 0) {
            const currentAccumulated = streamingContentRef.current[message_id] || '';
            streamingContentRef.current[message_id] = currentAccumulated + content;

            pendingTextDeltasRef.current[message_id] =
              (pendingTextDeltasRef.current[message_id] || '') + content;
            pendingFlushRef.current.add(message_id);

            scheduleTextFlush();
          }

          // Handle done event
          if (done) {
            const currentMode = modeRef.current;
            flushAllPending();
            const effectiveContent = final_content || streamingContentRef.current[message_id] || '';

            delete streamingContentRef.current[message_id];

            setTimeout(() => clearToolCalls(session_id), 2000);

            if (effectiveContent) {
              useAIPanelStore.getState().finishMessageStreaming(session_id, message_id, effectiveContent);
            } else {
              useAIPanelStore.getState().updateSession(session_id, (s) => ({ ...s, isStreaming: false }));
            }

            // Compute diff for agent mode
            if (effectiveContent && currentMode === 'agent') {
              try {
                const selection = useEditorStore.getState().getSelection?.();
                if (selection && effectiveContent !== selection) {
                  const originalText = selection;
                  const diff = await invoke<any>('compute_diff', { oldText: originalText, newText: effectiveContent });
                  setMessageDiff(session_id, message_id, {
                    originalText, newText: effectiveContent, hunks: diff?.hunks ?? [],
                    summary: summary ?? 'AI 已修改内容',
                  });
                }
              } catch {
                // ignore diff failure
              }
            }
          }
        });
      } finally {
        isSettingUpRef.current = false;
      }
    };

    setupListener();

    return () => {
      unlistenRef.current?.();
      unlistenRef.current = null;
      if (flushTimeoutRef.current !== null) {
        clearTimeout(flushTimeoutRef.current);
        flushTimeoutRef.current = null;
      }
      pendingTextDeltasRef.current = {};
      pendingFlushRef.current = new Set();
      streamingContentRef.current = {};
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const handleSetInput = useCallback((v: string) => setInput(v), []);

  return (
    <aside className={styles.panel}>
      <ChatHeader
        sessions={sessions}
        activeSessionId={activeSessionId}
        onCreateSession={createSession}
        onSelectSession={setActiveSession}
        onDeleteSession={deleteSession}
        onClose={() => setIsOpen(false)}
      />

      <ChatView
        messages={messages}
        activeSession={activeSession}
        isStreaming={isStreaming}
        pendingDiff={pendingDiff}
        mode={mode}
        activeToolCalls={activeToolCalls}
        editingMessageId={editingMessageId}
        editingContent={editingContent}
        onStartEdit={handleStartEdit}
        onCancelEdit={handleCancelEdit}
        onSaveEdit={handleSaveEdit}
        onSetEditingContent={setEditingContent}
        onSetInput={handleSetInput}
      />

      <ChatInput
        input={input}
        setInput={setInput}
        mode={mode}
        isStreaming={isStreaming}
        hasMessages={messages.length > 0}
        onSend={handleSend}
        onStop={handleStop}
        onClear={() => activeSession && clearMessages(activeSession.id)}
        onCycleMode={cycleMode}
      />
    </aside>
  );
};
