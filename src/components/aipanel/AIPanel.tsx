import React, { useState, useRef, useEffect, useMemo, useCallback } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { parse as parsePartialJson } from 'jsonchunk';
import {
  useAIPanelStore,
  useEditorStore,
  useSidebarStore,
  useSettingsStore,
  type ChatMode,
  type ChatMessage,
  type DiffHunk,
  type OutputItem,
  type SearchResult,
} from '../../store';
import { ChatHeader } from './ChatHeader';
import { ChatView } from './ChatView';
import { ChatInput } from './ChatInput';
import { KnowledgeView } from './KnowledgeView';
import styles from './AIPanel.module.css';

// Type for stream payload from Rust backend (matches src-tauri/src/streaming.rs)
// Time to wait before clearing tool calls after stream completes (allows user to see results)
const TOOL_CALL_CLEAR_DELAY_MS = 2000;
interface StreamPayload {
  session_id: string;
  message_id: string;
  event_type: string;
  content?: string;
  summary?: string;
  tool_call_id?: string;
  tool_name?: string;
  tool_args?: string;
  final_content?: string;
  error?: string;
  search_results?: SearchResult[];
  done: boolean;
  file_path?: string;
  original_content?: string;
  new_content?: string;
  diff_summary?: {
    file_name: string;
    added_lines: number;
    deleted_lines: number;
    hunks: DiffHunk[];
  };
  office_file_modified?: {
    path: string;
    format: string;
  };
}

// Helper: build conversation history from messages for AI API
function buildConversationHistory(messages: ChatMessage[]) {
  return messages.map(m => {
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
}

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
    setKnowledgeBase,
    clearSearchResults,
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

  const [input, setInput] = useState('');

  // Track which user message is being edited
  const [editingMessageId, setEditingMessageId] = useState<string | null>(null);
  const [editingContent, setEditingContent] = useState('');

  const inputRef = useRef<HTMLTextAreaElement>(null);

  // Ref to track accumulated text content for the current streaming message
  const streamingContentRef = useRef<Record<string, string>>({});

  // Microtask batching for text deltas (optimized: only update target session/message)
  const pendingTextDeltasRef = useRef<Record<string, string>>({});
  const flushTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const pendingFlushRef = useRef<Set<string>>(new Set());

  // Batching for tool-call args streaming. We receive dozens of small SSE deltas
  // per second from the backend; merging them at 16ms keeps the React render
  // cost flat regardless of how large the streamed content is.
  //   pendingToolArgsRef[toolCallId] = { sessionId, messageId, rawArgs, parsedArgs, streamingContent }
  const pendingToolArgsRef = useRef<Record<
    string,
    { sessionId: string; messageId: string; rawArgs: string; parsedArgs: Record<string, unknown>; streamingContent?: string }
  >>({});
  const pendingToolArgsOrderRef = useRef<string[]>([]);
  const flushToolArgsTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  // Note: Empty deps intentionally - this callback uses refs to access latest state,
  // avoiding stale closure issues without needing to re-create the callback.
  const flushTextDeltas = useCallback(() => {
    const deltas = pendingTextDeltasRef.current;
    const toFlush = [...pendingFlushRef.current];
    if (toFlush.length === 0) return;

    pendingTextDeltasRef.current = {};
    pendingFlushRef.current = new Set();
    flushTimeoutRef.current = null;

    useAIPanelStore.setState((state) => {
      // Build a map of messageId -> delta for fast lookup
      const deltaMap = new Map(toFlush.map(id => [id, deltas[id]]));

      return {
        sessions: state.sessions.map((s) => {
          // Check if this session has any messages that need updating
          const sessionMessageIds = toFlush.filter(id =>
            s.messages.some(m => m.id === id)
          );
          if (sessionMessageIds.length === 0) return s;

          const updatedMessages = s.messages.map((m) => {
            const delta = deltaMap.get(m.id);
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
      };
    });
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  // Note: Empty deps intentionally - scheduleTextFlush calls flushTextDeltas via ref pattern
  // to avoid stale closure issues with the store update callback.
  const scheduleTextFlush = useCallback(() => {
    if (flushTimeoutRef.current !== null) return;
    flushTimeoutRef.current = setTimeout(flushTextDeltas, 16);
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  // Note: Empty deps intentionally - the flush reads from refs only.
  // We do a single setState that touches exactly one outputItem per tool call,
  // so React re-renders are bounded by the number of distinct tool calls rather
  // than the number of SSE deltas.
  const flushToolArgs = useCallback(() => {
    const pending = pendingToolArgsRef.current;
    const order = pendingToolArgsOrderRef.current;
    if (order.length === 0) return;

    pendingToolArgsRef.current = {};
    pendingToolArgsOrderRef.current = [];
    flushToolArgsTimeoutRef.current = null;

    useAIPanelStore.setState((state) => {
      // Group pending tool call ids by session so we can early-out sessions
      // that have nothing to update without iterating their messages.
      const sessionIds = new Set<string>();
      for (const id of order) {
        const e = pending[id];
        if (e) sessionIds.add(e.sessionId);
      }

      return {
        sessions: state.sessions.map((s) => {
          if (!sessionIds.has(s.id)) return s;
          return {
            ...s,
            messages: s.messages.map((m) => {
              let mutated = false;
              const updatedItems = m.outputItems.map((item) => {
                if (item.type !== 'tool_call_start') return item;
                const e = pending[item.toolCallId];
                if (!e || e.messageId !== m.id) return item;
                mutated = true;
                return {
                  ...item,
                  arguments: e.parsedArgs,
                  rawArguments: e.rawArgs,
                  streamingContent: e.streamingContent,
                  isExecuting: true,
                };
              });
              return mutated ? { ...m, outputItems: updatedItems } : m;
            }),
          };
        }),
      };
    });
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  // Note: Empty deps intentionally - reads refs only. Coalesces many args_delta
  // events into a single 16ms tick.
  const scheduleToolArgsFlush = useCallback(() => {
    if (flushToolArgsTimeoutRef.current !== null) return;
    flushToolArgsTimeoutRef.current = setTimeout(flushToolArgs, 16);
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  const flushAllPending = useCallback(() => {
    if (flushTimeoutRef.current !== null) {
      clearTimeout(flushTimeoutRef.current);
      flushTimeoutRef.current = null;
    }
    flushTextDeltas();
    flushToolArgs();
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
    setIsStreaming(sessionId, true);

    clearToolCalls(sessionId);

    try {
      const workspacePath = useSidebarStore.getState().workspacePath || undefined;
      const { apiConfigs, activeApiConfigId } = useSettingsStore.getState().settings;
      const activeConfig = apiConfigs.find(c => c.id === activeApiConfigId) ?? apiConfigs[0];
      const conversationHistory = buildConversationHistory(messages);

        if (mode === 'knowledge') {
        clearSearchResults(sessionId);
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
    const order: ChatMode[] = ['ask', 'plan', 'agent', 'knowledge'];
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

  // Knowledge base handlers
  const workspacePath = useSidebarStore((state) => state.workspacePath);

  const { setBuildProgress } = useAIPanelStore();

  const handleKnowledgeBuild = useCallback(async () => {
    if (!activeSession || !workspacePath) return;

    const sessionId = activeSession.id;

    // Listen to build progress events
    let unlistenProgress: (() => void) | undefined;
    try {
      const { listen } = await import('@tauri-apps/api/event');
      unlistenProgress = await listen<{
        session_id: string;
        phase: string;
        current: number;
        total: number;
        message: string;
      }>('kb://build-progress', (event) => {
        if (event.payload.session_id !== sessionId) return;
        if (event.payload.phase === 'done') {
          setBuildProgress(sessionId, undefined);
        } else {
          setBuildProgress(sessionId, {
            phase: event.payload.phase as 'scanning' | 'chunking' | 'embedding' | 'storing',
            current: event.payload.current,
            total: event.payload.total,
            currentFile: event.payload.message,
          });
        }
      });
    } catch (err) {
      console.error('Failed to listen to build progress:', err);
    }

    try {
      const result = await invoke<{ total_documents: number; total_chunks: number; workspace_id: string }>('knowledge_build', {
        workspacePath,
        sessionId,
      });

      setKnowledgeBase(sessionId, {
        workspaceId: result.workspace_id,
        documentCount: result.total_documents,
        chunkCount: result.total_chunks,
        lastUpdated: Date.now(),
      });
    } catch (err) {
      console.error('Failed to build knowledge base:', err);
    } finally {
      unlistenProgress?.();
    }
  }, [activeSession, workspacePath, setKnowledgeBase, setBuildProgress]);

  const handleKnowledgeClear = useCallback(async () => {
    if (!activeSession || !workspacePath) return;

    const sessionId = activeSession.id;

    try {
      await invoke('knowledge_clear', { workspacePath });
      setKnowledgeBase(sessionId, undefined);
      clearSearchResults(sessionId);
    } catch (err) {
      console.error('Failed to clear knowledge base:', err);
    }
  }, [activeSession, workspacePath, setKnowledgeBase, clearSearchResults]);

  // Load knowledge base status on mount or workspace change
  useEffect(() => {
    if (!activeSession || !workspacePath) return;

    const sessionId = activeSession.id;

    invoke<{ workspace_id: string; document_count: number; chunk_count: number; last_updated: string } | null>(
      'knowledge_status',
      { workspacePath }
    ).then((status) => {
      if (status) {
        setKnowledgeBase(sessionId, {
          workspaceId: status.workspace_id,
          documentCount: status.document_count,
          chunkCount: status.chunk_count,
          lastUpdated: new Date(status.last_updated).getTime(),
        });
      }
    }).catch(console.error);
  }, [activeSession?.id, workspacePath, setKnowledgeBase]);

  // Streaming events
  useEffect(() => {
    const setupListener = async () => {
      if (unlistenRef.current || isSettingUpRef.current) return;
      isSettingUpRef.current = true;

      try {
        unlistenRef.current = await listen<StreamPayload>('ai://stream', async (event) => {
          const payload = event.payload;
          const {
            session_id, message_id, event_type, content, done, summary,
            final_content, error, tool_call_id, tool_name, tool_args,
            diff_summary, office_file_modified, search_results,
          } = payload;

          if (!payload || !session_id || !message_id) return;

          // Handle error event
          if (event_type === 'error') {
            flushAllPending();
            delete streamingContentRef.current[message_id];
            useAIPanelStore.getState().setErrorMessage(session_id, message_id, error ?? '发生错误');
            if (modeRef.current === 'knowledge') {
              useAIPanelStore.getState().setSearchResults(session_id, []);
            }
            return;
          }

          // Handle tool call start — fired by the backend the first time a
          // tool_call index appears in the streaming response (i.e. the first
          // SSE delta for that call). We immediately create a card in
          // "executing" state so the user sees feedback the moment the AI
          // decides to call a tool — no more waiting for the entire 10000-char
          // payload to be received.
          if (event_type === 'tool_call_start' && tool_call_id && tool_name) {
            const rawArgs = tool_args ?? '';

            // Use jsonchunk to parse partial JSON and extract content field
            const partial = parsePartialJson(rawArgs) as Record<string, unknown> | undefined;
            const streamingContent = (partial?.content || partial?.new_text || partial?.json_content) as string | undefined;

            let args: Record<string, unknown> = {};
            try {
              if (rawArgs) args = JSON.parse(rawArgs);
            } catch {
              // Partial JSON during streaming — keep partial parsed values
              args = partial || {};
            }

            useAIPanelStore.getState().updateSession(session_id, (s) => {
              // Check if a tool_call_start for this id already exists (idempotency
              // in case the backend re-emits start for the same call).
              const alreadyExists = s.activeToolCalls.some((tc) => tc.id === tool_call_id);
              const updatedActiveToolCalls = alreadyExists
                ? s.activeToolCalls.map((tc) =>
                    tc.id === tool_call_id
                      ? { ...tc, name: tool_name, arguments: args, status: 'executing' as const }
                      : tc
                  )
                : [
                    ...s.activeToolCalls,
                    {
                      id: tool_call_id,
                      name: tool_name,
                      arguments: args,
                      status: 'executing' as const,
                      startTime: Date.now(),
                    },
                  ];

return {
                    ...s,
                    activeToolCalls: updatedActiveToolCalls,
                    messages: s.messages.map((m) => {
                      if (m.id !== message_id) return m;
                      // If the assistant message already has a tool_call_start item
                      // for this id, update it; otherwise append a new one.
                      const existingIdx = m.outputItems.findIndex(
                        (it) => it.type === 'tool_call_start' && it.toolCallId === tool_call_id
                      );
                      if (existingIdx >= 0) {
                        const updated = [...m.outputItems];
                        const prev = updated[existingIdx] as Extract<OutputItem, { type: 'tool_call_start' }>;
                        updated[existingIdx] = {
                          ...prev,
                          toolName: tool_name,
                          arguments: args,
                          rawArguments: rawArgs,
                          streamingContent: streamingContent ?? undefined,
                          isExecuting: true,
                        };
                        return { ...m, outputItems: updated };
                      }
                      return {
                        ...m,
                        toolCalls: [...(m.toolCalls || []), { id: tool_call_id, name: tool_name, arguments: args }],
                        outputItems: [
                          ...m.outputItems,
                          {
                            type: 'tool_call_start' as const,
                            toolCallId: tool_call_id,
                            toolName: tool_name,
                            arguments: args,
                            rawArguments: rawArgs,
                            streamingContent: streamingContent ?? undefined,
                            isExecuting: true,
                          },
                        ],
                      };
                    }),
                  };
            });
            return;
          }

          // Handle tool call args delta — fired on every subsequent SSE chunk
          // for the same tool call. We coalesce many deltas into one store
          // update at ~16ms granularity to keep React renders bounded. The
          // raw args accumulate server-side, so each event already carries
          // the *full* current string; we just overwrite the cached value.
          if (event_type === 'tool_call_args_delta' && tool_call_id) {
            const rawArgs = tool_args ?? '';

            // Use jsonchunk to parse partial JSON and extract content field
            const partial = parsePartialJson(rawArgs) as Record<string, unknown> | undefined;
            const streamingContent = (partial?.content || partial?.new_text || partial?.json_content) as string | undefined;

            let args: Record<string, unknown> = {};
            try { if (rawArgs) args = JSON.parse(rawArgs); } catch { /* fallback to partial */ }

            const prev = pendingToolArgsRef.current[tool_call_id];
            // Skip the work if the raw payload hasn't actually changed (defensive).
            if (!prev || prev.rawArgs !== rawArgs) {
              pendingToolArgsRef.current[tool_call_id] = {
                sessionId: session_id,
                messageId: message_id,
                rawArgs,
                parsedArgs: args,
                streamingContent: streamingContent ?? undefined,
              };
              if (!prev) pendingToolArgsOrderRef.current.push(tool_call_id);
            }
            scheduleToolArgsFlush();
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
              messages: s.messages.map((m) => {
                if (m.id !== message_id) return m;
                // Update the existing tool_call_start in-place with result info
                // and filter out the tool_result item since we merged it into tool_call_start
                const updatedItems = m.outputItems
                  .filter((it) => !(it.type === 'tool_result' && (it as { toolCallId?: string }).toolCallId === tool_call_id))
                  .map((it) => {
                    if (it.type !== 'tool_call_start' || (it as { toolCallId?: string }).toolCallId !== tool_call_id) return it;
                    return {
                      ...it,
                      isExecuting: false,
                      status: isError ? 'error' as const : 'success' as const,
                      result: content || '',
                      duration,
                      diffSummary: diff_summary ?? undefined,
                    };
                  });
                return {
                  ...m,
                  toolResults: [...(m.toolResults || []), toolResult],
                  outputItems: updatedItems,
                };
              }),
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

            setTimeout(() => clearToolCalls(session_id), TOOL_CALL_CLEAR_DELAY_MS);

            if (currentMode === 'knowledge' && search_results) {
              useAIPanelStore.getState().setSearchResults(session_id, search_results);
            }

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
      if (flushToolArgsTimeoutRef.current !== null) {
        clearTimeout(flushToolArgsTimeoutRef.current);
        flushToolArgsTimeoutRef.current = null;
      }
      pendingTextDeltasRef.current = {};
      pendingFlushRef.current = new Set();
      pendingToolArgsRef.current = {};
      pendingToolArgsOrderRef.current = [];
      streamingContentRef.current = {};
    };
    // Note: Empty deps intentionally - this effect sets up event listener once.
    // It uses refs (unlistenRef, isSettingUpRef, flushAllPending) to access latest state
    // without triggering re-runs, avoiding stale closures and listener duplication.
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

      <div className={styles.panelBody}>
        {mode === 'knowledge' && activeSession && (
          <KnowledgeView
            sessionId={activeSession.id}
            onBuild={handleKnowledgeBuild}
            onClear={handleKnowledgeClear}
          />
        )}

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
      </div>

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
