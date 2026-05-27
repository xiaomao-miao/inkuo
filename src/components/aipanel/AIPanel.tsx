import React, { useState, useRef, useEffect, useMemo, useCallback } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import {
  MessageSquare,
  PanelRightClose,
  Loader2,
  Trash2,
  Send,
  PlusCircle,
  StopCircle,
  Terminal,
  Sparkles,
  X,
  Pencil,
  RotateCcw,
} from 'lucide-react';
import {
  useAIPanelStore,
  useEditorStore,
  useSidebarStore,
  type ChatMessage,
  type ChatMode,
  type MessageToolCall,
  type MessageToolResult,
} from '../../store';
import { useSettingsStore } from '../../store';
import styles from './AIPanel.module.css';
import { parsePlanBlocks, type PlanBlock } from './planRender';
import { MarkdownRenderer } from './MarkdownRenderer';
import { InlineDiffPreview } from './InlineDiffPreview';
import { ToolCallCard } from './ToolCallCard';

const MODE_LABELS: Record<ChatMode, string> = {
  ask: 'Ask',
  plan: 'Plan',
  agent: 'Agent',
};

const MODE_HINTS: Record<ChatMode, string> = {
  ask: '只回答（不修改文件）',
  plan: '只输出计划（不修改文件）',
  agent: 'Full Agent（可调用工具读写文件）',
};

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
    acceptAllHunks,
    rejectAllHunks,
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

  const { selectedFile } = useSidebarStore();
  const { documentContents, getSelection } = useEditorStore();

  const [input, setInput] = useState('');

  // Track which user message is being edited
  const [editingMessageId, setEditingMessageId] = useState<string | null>(null);
  const [editingContent, setEditingContent] = useState('');

  const messagesEndRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLTextAreaElement>(null);

  // Ref to track accumulated text content for the current streaming message
  const streamingContentRef = useRef<Record<string, string>>({});

  // Microtask batching for text deltas — batches rapid text events into a single state update
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

  // Ref to track the unlisten function
  const unlistenRef = useRef<(() => void) | null>(null);

  // Ref to track if listener is being set up (prevent race condition)
  const isSettingUpRef = useRef(false);

  // Refs to track latest values for event handlers
  const getSelectionRef = useRef(getSelection);
  const selectedFileRef = useRef(selectedFile);
  const documentContentsRef = useRef(documentContents);
  const workspacePathRef = useRef<string | null>(null);
  // Track current mode to avoid stale closure in streaming listener
  const modeRef = useRef(mode);

  // Keep refs updated
  useEffect(() => {
    getSelectionRef.current = getSelection;
    selectedFileRef.current = selectedFile;
    documentContentsRef.current = documentContents;
    workspacePathRef.current = useSidebarStore.getState().workspacePath;
  });

  // Keep modeRef in sync with mode
  useEffect(() => {
    modeRef.current = mode;
  }, [mode]);

  // Track scroll position to enable/disable auto-scroll
  const contentRef = useRef<HTMLDivElement>(null);
  const isAtBottomRef = useRef(true);

  const checkIfAtBottom = () => {
    if (!contentRef.current) return true;
    const { scrollTop, scrollHeight, clientHeight } = contentRef.current;
    isAtBottomRef.current = scrollHeight - scrollTop - clientHeight < 50;
  };

  // Scroll to bottom only when user is already at bottom or it's a new message
  useEffect(() => {
    if (isAtBottomRef.current || messages.length <= 2) {
      messagesEndRef.current?.scrollIntoView({ behavior: 'smooth' });
    }
  }, [messages, activeToolCalls]);

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
    // Use the input directly (could be from editing or a new message)
    const instruction = input.trim();

    const isEditing = editingMessageId !== null;

    // Generate IDs
    const userMessageId = isEditing ? editingMessageId : Date.now().toString();
    const assistantMessageId = (Date.now() + 1).toString();

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

    // If editing, the message already exists; we just need to update it and add assistant response
    // If new message, add both messages
    if (isEditing) {
      updateMessage(sessionId, userMessageId, instruction);
    } else {
      addMessage(sessionId, userMessage);
    }
    addMessage(sessionId, assistantPlaceholder);

    // Clear edit mode state
    setEditingMessageId(null);
    setEditingContent('');
    setInput('');

    // Clear any previous tool calls
    clearToolCalls(sessionId);

    try {
      const selection = getSelection();
      const currentDoc = selectedFile ? documentContents[selectedFile] : null;
      const originalText = selection || currentDoc?.content || '';

      if (mode === 'agent') {
        // Use full agent with tool calling with context memory
        const workspacePath = useSidebarStore.getState().workspacePath || undefined;
        const aiConfig = useSettingsStore.getState().settings;
        // Get conversation history (excluding the messages we're about to add)
        const conversationHistory = messages.map(m => {
          // For tool messages, use content; for others, extract text from outputItems
          let textContent = '';
          if (m.role === 'tool') {
            textContent = m.content || '';
          } else if (m.outputItems && m.outputItems.length > 0) {
            // Reconstruct content from outputItems for assistant messages
            textContent = m.outputItems
              .filter(item => item.type === 'text')
              .map(item => item.content)
              .join('');
          } else {
            textContent = m.content || '';
          }
          return {
            id: m.id,
            role: m.role,
            content: textContent,
            tool_calls: m.toolCalls,
            tool_call_id: m.toolCallId,
          };
        });
        // Don't await - let the invoke run in background while UI remains responsive
        invoke('ai_agent_stream', {
          sessionId,
          messageId: assistantMessageId,
          instruction,
          workspacePath,
          history: conversationHistory,
          configInput: {
            provider: aiConfig.ai_provider,
            api_key: aiConfig.ai_api_key,
            base_url: aiConfig.ai_base_url,
            model: aiConfig.ai_model,
          },
        }).catch((err) => {
          updateMessage(sessionId, assistantMessageId, `抱歉，发生了错误：${err}`);
          setIsStreaming(sessionId, false);
        });
      } else {
        // Use simple chat/edit mode
        // Don't await - let the invoke run in background while UI remains responsive
        invoke('ai_chat_stream', {
          sessionId,
          messageId: assistantMessageId,
          mode,
          instruction,
          originalText: originalText.slice(0, 5000),
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

  const handleKeyDown = (e: React.KeyboardEvent) => {
    // When editing a message, Enter saves and resends (without Shift)
    if (editingMessageId) {
      if (e.key === 'Enter' && !e.shiftKey) {
        e.preventDefault();
        handleSaveEdit();
      }
      // Shift+Enter allows newlines in edit mode
      return;
    }

    // Normal mode
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      handleSend();
    }
  };

  const handleStop = async () => {
    if (!activeSession) return;
    try {
      // Cancel based on current mode
      if (mode === 'agent') {
        await invoke('ai_agent_cancel', { sessionId: activeSession.id });
      } else {
        await invoke('ai_stream_cancel', { sessionId: activeSession.id });
      }
    } catch {
      // ignore
    }
  };

  // Streaming events
  useEffect(() => {
    const setupListener = async () => {
      // Prevent multiple concurrent setups
      if (unlistenRef.current || isSettingUpRef.current) return;
      isSettingUpRef.current = true;

      try {
        unlistenRef.current = await listen<any>('ai://stream', async (event) => {
          const payload = event.payload;
          const {
            session_id,
            message_id,
            event_type,
            content,
            done,
            summary,
            final_content,
            error,
            tool_call_id,
            tool_name,
            tool_args,
            diff_summary,
          } = payload;

          if (!payload || !session_id || !message_id) return;

          console.log('[Stream Event]', {
            session_id,
            message_id,
            event_type,
            content: content?.slice?.(0, 50),
            done,
            tool_call_id,
            tool_name,
          });

          // Debug: log message list after each event
          const debugMessages = useAIPanelStore.getState().sessions.find(s => s.id === session_id)?.messages ?? [];
          console.log('[Messages Debug] count:', debugMessages.length, 'roles:', debugMessages.map(m => m.role));

          // Handle error event
          if (event_type === 'error') {
            console.log('[Stream Error]', error);
            flushAllPending();
            useAIPanelStore.setState((state) => ({
              sessions: state.sessions.map((s) =>
                s.id === session_id
                  ? {
                      ...s,
                      messages: s.messages.map((m) =>
                        m.id === message_id ? { ...m, content: error ?? '发生错误' } : m
                      ),
                      isStreaming: false,
                    }
                  : s
              ),
            }));
            return;
          }

          // Handle tool call start
          if (event_type === 'tool_call_start' && tool_call_id && tool_name) {
            let args = {};
            try {
              if (tool_args) {
                args = JSON.parse(tool_args);
              }
            } catch {
              // ignore parse error
            }

            const newToolCall: MessageToolCall = {
              id: tool_call_id,
              name: tool_name,
              arguments: args,
            };

            useAIPanelStore.setState((state) => ({
              sessions: state.sessions.map((s) =>
                s.id === session_id
                  ? {
                      ...s,
                      messages: s.messages.map((m) =>
                        m.id === message_id
                          ? {
                              ...m,
                              toolCalls: [...(m.toolCalls || []), newToolCall],
                              // Append tool_call_start item to outputItems
                              outputItems: [
                                ...m.outputItems,
                                { type: 'tool_call_start', toolCallId: tool_call_id, toolName: tool_name, arguments: args },
                              ],
                            }
                          : m
                      ),
                      activeToolCalls: [...s.activeToolCalls, {
                        id: tool_call_id,
                        name: tool_name,
                        arguments: args,
                        status: 'executing' as const,
                        startTime: Date.now(),
                      }],
                    }
                  : s
              ),
            }));
            return;
          }

          // Handle tool result
          if (event_type === 'tool_result' && tool_call_id) {
            const isError = !!error;
            const toolCall = useAIPanelStore.getState().sessions
              .find((s) => s.id === session_id)
              ?.activeToolCalls.find((tc) => tc.id === tool_call_id);
            const duration = toolCall?.startTime ? Date.now() - toolCall.startTime : undefined;

            // Build tool result to add to message
            const toolResult: MessageToolResult = {
              toolCallId: tool_call_id,
              result: content || '',
              isError,
              duration,
              diffSummary: diff_summary ?? undefined,
            };

            useAIPanelStore.setState((state) => ({
              sessions: state.sessions.map((s) =>
                s.id === session_id
                  ? {
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
                              outputItems: [
                                ...m.outputItems,
                                {
                                  type: 'tool_result',
                                  toolCallId: tool_call_id,
                                  status: isError ? 'error' : 'success',
                                  result: content || '',
                                  duration,
                                  diffSummary: diff_summary ?? undefined,
                                },
                              ],
                            }
                          : m
                      ),
                    }
                  : s
              ),
            }));

            // Also add a standalone tool message so it appears in history for subsequent requests
            // This is critical for multi-turn conversations — the API requires tool_call_id responses
            const toolMessage: ChatMessage = {
              id: `tool-${tool_call_id}-${Date.now()}`,
              role: 'tool',
              content: content || '',
              toolCallId: tool_call_id,
              timestamp: Date.now(),
              outputItems: [],
            };
            addMessage(session_id, toolMessage);
            return;
          }

          // Handle text delta — batch into a microtask to avoid per-token React state updates
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
            // Flush any pending text deltas before finalizing
            flushAllPending();
            // Fallback: use accumulated streaming content if final_content is missing
            const effectiveContent = final_content || streamingContentRef.current[message_id] || '';

            // Clean up refs
            delete streamingContentRef.current[message_id];

            // Clear tool calls after a delay
            setTimeout(() => {
              clearToolCalls(session_id);
            }, 2000);

            useAIPanelStore.setState((state) => ({
              sessions: state.sessions.map((s) =>
                s.id === session_id ? { ...s, isStreaming: false } : s
              ),
            }));

            // Update message with final content (backward compat + fallback)
            // Also clear isPendingMarkdown from all text items so markdown gets rendered on done
            if (effectiveContent) {
              useAIPanelStore.setState((state) => ({
                sessions: state.sessions.map((s) =>
                  s.id === session_id
                    ? {
                        ...s,
                        messages: s.messages.map((m) =>
                          m.id === message_id
                            ? {
                                ...m,
                                // Set legacy content field as fallback
                                content: m.content || effectiveContent,
                                // If outputItems is empty, create a single text item
                                // Otherwise, clear isPendingMarkdown so markdown renders
                                outputItems: m.outputItems.length > 0
                                  ? m.outputItems.map((item) =>
                                      item.type === 'text'
                                        ? { ...item, isPendingMarkdown: false }
                                        : item
                                    )
                                  : [{ type: 'text', content: effectiveContent, isPendingMarkdown: false }],
                              }
                            : m
                        ),
                      }
                    : s
                ),
              }));
            }

            // Handle final content for agent mode - compute diff ONLY if:
            // 1. User has selected text (no selection = no diff needed)
            // 2. AI content is different from the original selection
            // Note: Most agent interactions are conversational, not document edits
            const selection = getSelectionRef.current();
            if (effectiveContent && currentMode === 'agent' && selection && effectiveContent !== selection) {
              try {
                const originalText = selection;

                const diff = await invoke<any>('compute_diff', {
                  oldText: originalText,
                  newText: effectiveContent,
                });

                // Set diff on the message itself
                setMessageDiff(session_id, message_id, {
                  originalText,
                  newText: effectiveContent,
                  hunks: diff?.hunks ?? [],
                  summary: summary ?? 'AI 已修改内容',
                });
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
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const cycleMode = () => {
    if (!activeSession) return;
    const order: ChatMode[] = ['ask', 'plan', 'agent'];
    const idx = order.indexOf(mode);
    setSessionMode(activeSession.id, order[(idx + 1) % order.length]);
  };

  // Start editing a user message
  const handleStartEdit = (messageId: string, currentContent: string) => {
    setEditingMessageId(messageId);
    setEditingContent(currentContent);
    setInput(currentContent);
  };

  // Cancel editing
  const handleCancelEdit = () => {
    setEditingMessageId(null);
    setEditingContent('');
    setInput('');
  };

  // Save edited message and truncate subsequent messages, then resend
  const handleSaveEdit = async () => {
    if (!activeSession || !editingMessageId || !editingContent.trim()) return;
    if (isStreaming) return;

    const newContent = editingContent.trim();
    const sessionId = activeSession.id;

    // Truncate all messages after the edited one (clears subsequent history)
    truncateMessagesAfter(sessionId, editingMessageId);

    // Clear edit mode state
    setEditingMessageId(null);
    setEditingContent('');

    // Set the input for resend
    setInput(newContent);

    // Trigger resend
    await handleSend();
  };

  return (
    <aside className={styles.panel}>
      {/* Header */}
      <div className={styles.header}>
        <div className={styles.sessionBar}>
          <button
            className={styles.newSessionBtn}
            onClick={() => { const id = createSession(); setActiveSession(id); }}
            title="新建对话"
            type="button"
          >
            <PlusCircle size={16} />
          </button>
          <div className={styles.sessionList}>
            {sessions.map((s) => (
              <button
                key={s.id}
                type="button"
                className={`${styles.sessionChip} ${s.id === activeSession?.id ? styles.sessionActive : ''}`}
                onClick={() => setActiveSession(s.id)}
              >
                <MessageSquare size={12} />
                <span className={styles.sessionTitle}>
                  {s.messages.length > 0 && s.messages[0].content
                    ? s.messages[0].content.slice(0, 20) + (s.messages[0].content.length > 20 ? '...' : '')
                    : '新对话'}
                </span>
                {sessions.length > 1 && (
                  <span
                    className={styles.sessionClose}
                    onClick={(e) => { e.preventDefault(); e.stopPropagation(); deleteSession(s.id); }}
                    title="关闭"
                  >
                    <X size={11} />
                  </span>
                )}
              </button>
            ))}
          </div>
        </div>
        <button className={styles.closeButton} title="关闭面板" onClick={() => setIsOpen(false)}>
          <PanelRightClose size={16} />
        </button>
      </div>

      {/* Chat Content */}
      <div className={styles.content} ref={contentRef} onScroll={checkIfAtBottom}>
        {/* Messages */}
        {messages.length === 0 ? (
          <div className={styles.emptyState}>
            <div className={styles.emptyIcon}><Sparkles size={32} /></div>
            <h3>开始对话</h3>
            <p>
              {mode === 'agent'
                ? '使用 Agent 模式，可以帮你读写文件、搜索代码'
                : '询问关于文档的问题或请求 AI 帮助你写作'}
            </p>
            <div className={styles.quickActions}>
              <button className={styles.quickAction} onClick={() => setInput('总结这篇文档的主要内容')}>
                总结文档
              </button>
              <button className={styles.quickAction} onClick={() => setInput('解释这段代码/文本的工作原理')}>
                解释内容
              </button>
              {mode === 'agent' && (
                <button
                  className={styles.quickAction}
                  onClick={() => setInput('查看项目结构，列出 src 目录下的所有文件')}
                >
                  查看项目结构
                </button>
              )}
            </div>
          </div>
        ) : (
          <div className={styles.messages}>
            {/* Flattened message + tool result rendering — Cursor-style interleaving */}
            {messages.flatMap((message) => {
              const elements: React.ReactNode[] = [];

              if (message.role === 'user') {
                const isEditing = editingMessageId === message.id;
                elements.push(
                  <div key={message.id} className={`${styles.message} ${styles.user}`}>
                    <div className={styles.messageBubble}>
                      {isEditing ? (
                        <div className={styles.editMode}>
                          <textarea
                            className={styles.editTextarea}
                            value={editingContent}
                            onChange={(e) => {
                              setEditingContent(e.target.value);
                              setInput(e.target.value);
                            }}
                            autoFocus
                          />
                          <div className={styles.editActions}>
                            <button
                              className={styles.editCancelBtn}
                              onClick={handleCancelEdit}
                              title="取消"
                              type="button"
                            >
                              <X size={12} />
                              取消
                            </button>
                            <button
                              className={styles.editSaveBtn}
                              onClick={handleSaveEdit}
                              disabled={!editingContent.trim()}
                              title="重新发送"
                              type="button"
                            >
                              <RotateCcw size={12} />
                              重新发送
                            </button>
                          </div>
                        </div>
                      ) : (
                        <>
                          <div className={styles.messageText}>{message.content}</div>
                          {!isStreaming && (
                            <button
                              className={styles.editBtn}
                              onClick={() => handleStartEdit(message.id, message.content || '')}
                              title="编辑并重新发送"
                              type="button"
                            >
                              <Pencil size={12} />
                            </button>
                          )}
                        </>
                      )}
                    </div>
                  </div>
                );
              } else if (message.role === 'tool') {
                // Tool messages are rendered as part of the assistant's outputItems (tool result cards)
                // Skip standalone rendering to avoid duplication
              } else if (message.role === 'assistant') {
                // Determine if this is the assistant message currently receiving stream deltas.
                const streamingMessageId = activeSession?.messages
                  .slice()
                  .reverse()
                  .find((m) => m.role === 'assistant')?.id;
                const isThisStreaming = isStreaming && message.id === streamingMessageId;

                // Use outputItems if available for interleaved rendering, otherwise fall back to legacy content
                const hasOutputItems = message.outputItems && message.outputItems.length > 0;

                elements.push(
                  <div key={message.id} className={`${styles.message} ${styles.assistant}`}>
                    <div className={styles.messageContent}>
                      {/* Render via outputItems (interleaved text + tool cards) */}
                      {hasOutputItems ? (
                        message.outputItems.map((item, idx) => {
                          if (item.type === 'text') {
                            return (
                              <div key={idx} className={styles.outputTextItem}>
                                {item.isPendingMarkdown ? (
                                  // During streaming, show raw text without markdown parsing
                                  // to avoid broken table rendering from partial markdown
                                  <pre style={{ margin: 0, padding: 0, fontFamily: 'inherit', fontSize: 'inherit', lineHeight: 'inherit', whiteSpace: 'pre-wrap', background: 'transparent' }}>
                                    {item.content}
                                  </pre>
                                ) : (
                                  <MarkdownRenderer content={item.content} />
                                )}
                              </div>
                            );
                          }
                          if (item.type === 'tool_call_start') {
                            return (
                              <div key={idx} className={styles.toolExecutingIndicator}>
                                <Loader2 size={12} className={styles.spinning} />
                                <span className={styles.streamingToolName}>{item.toolName}</span>
                                <span className={styles.toolExecutingText}>正在执行...</span>
                              </div>
                            );
                          }
                          if (item.type === 'tool_result') {
                            const toolCall = message.toolCalls?.find(tc => tc.id === item.toolCallId);
                            return (
                              <div key={idx} className={styles.toolResultItem}>
                                {/* Continue generating indicator before the card */}
                                {isThisStreaming && idx === message.outputItems.length - 1 && (
                                  <div className={styles.continueGenerating}>
                                    <span className={styles.continueDots}>
                                      <span className={styles.dot} />
                                      <span className={styles.dot} />
                                      <span className={styles.dot} />
                                    </span>
                                  </div>
                                )}
                                <ToolCallCard
                                  id={item.toolCallId}
                                  name={toolCall?.name || 'unknown'}
                                  arguments={toolCall?.arguments || {}}
                                  status={item.status}
                                  result={item.result}
                                  error={item.status === 'error' ? item.result : undefined}
                                  duration={item.duration}
                                  diffSummary={item.diffSummary as any}
                                  onAccept={() => activeSession && acceptAllHunks(activeSession.id)}
                                  onReject={() => activeSession && rejectAllHunks(activeSession.id)}
                                />
                              </div>
                            );
                          }
                          if (item.type === 'tool_error') {
                            return (
                              <div key={idx} className={styles.toolErrorItem}>
                                <div className={styles.toolErrorBadge}>
                                  <X size={12} />
                                  <span>工具执行失败</span>
                                </div>
                                <pre className={styles.toolErrorText}>{item.error}</pre>
                              </div>
                            );
                          }
                          return null;
                        })
                      ) : (
                        /* Legacy fallback: use content + toolResults */
                        <>
                          {/* Inline streaming tool call badges */}
                          {isThisStreaming && activeToolCalls.map((tc) => (
                            <div key={tc.id} className={styles.streamingToolCall}>
                              <Loader2 size={12} className={styles.spinning} />
                              <span className={styles.streamingToolName}>{tc.name}</span>
                            </div>
                          ))}
                          {message.toolCalls && message.toolCalls.length > 0 && !message.toolResults?.length && (
                            <div className={styles.toolExecutingIndicator}>
                              <Loader2 size={12} className={styles.spinning} />
                              <span>正在执行工具...</span>
                            </div>
                          )}
                          {mode === 'plan' && message.content ? (
                            <div className={styles.planBlocks}>
                              {parsePlanBlocks(message.content).map((b: PlanBlock, idx: number) => (
                                <div key={idx} className={styles.planBlock}>
                                  <div className={styles.planTitle}>{b.title}</div>
                                  <pre className={styles.planBody}>{b.lines.join('\n')}</pre>
                                </div>
                              ))}
                            </div>
                          ) : message.content ? (
                            // During streaming, show raw text to avoid broken markdown
                            isThisStreaming ? (
                              <pre style={{ margin: 0, padding: 0, fontFamily: 'inherit', fontSize: 'inherit', lineHeight: 'inherit', whiteSpace: 'pre-wrap', background: 'transparent' }}>
                                {message.content}
                              </pre>
                            ) : (
                              <MarkdownRenderer content={message.content} />
                            )
                          ) : !message.toolResults?.length && !isThisStreaming ? (
                            <div className={styles.toolOnlyPlaceholder}>工具执行完成</div>
                          ) : null}
                          {message.toolResults?.map((result) => {
                            const toolCall = message.toolCalls?.find(tc => tc.id === result.toolCallId);
                            return (
                              <div key={`tool-${result.toolCallId}`} className={styles.toolResultItem}>
                                <ToolCallCard
                                  id={result.toolCallId}
                                  name={toolCall?.name || 'unknown'}
                                  arguments={toolCall?.arguments || {}}
                                  status={result.isError ? 'error' : 'success'}
                                  result={result.result}
                                  error={result.isError ? result.result : undefined}
                                  duration={result.duration}
                                  diffSummary={result.diffSummary as any}
                                  onAccept={() => activeSession && acceptAllHunks(activeSession.id)}
                                  onReject={() => activeSession && rejectAllHunks(activeSession.id)}
                                />
                              </div>
                            );
                          })}
                        </>
                      )}

                      {/* Inline diff preview */}
                      {message.diff && !isThisStreaming && (
                        <InlineDiffPreview
                          originalText={message.diff.originalText}
                          newText={message.diff.newText}
                          onAccept={() => activeSession && acceptAllHunks(activeSession.id)}
                          onReject={() => activeSession && rejectAllHunks(activeSession.id)}
                        />
                      )}

                      {/* Streaming indicator at end of content */}
                      {isThisStreaming && !hasOutputItems && (
                        <span className={styles.streamingCursor} />
                      )}
                    </div>
                  </div>
                );
              }

              return elements;
            })}

            {/* Streaming diff preview - shows during text editing */}
            {pendingDiff && (
              <InlineDiffPreview
                originalText={pendingDiff.originalText}
                newText={pendingDiff.newText}
                onAccept={() => activeSession && acceptAllHunks(activeSession.id)}
                onReject={() => activeSession && rejectAllHunks(activeSession.id)}
                isStreaming={isStreaming}
              />
            )}
            <div ref={messagesEndRef} />
          </div>
        )}
      </div>

      {/* Input Area */}
      <div className={styles.inputArea}>
        <div className={styles.inputLeft}>
          <button
            type="button"
            className={`${styles.modeButton} ${mode === 'agent' ? styles.agentModeActive : ''}`}
            onClick={cycleMode}
            disabled={!activeSession || isStreaming}
            title={MODE_HINTS[mode]}
          >
            {mode === 'agent' && <Terminal size={12} />}
            {MODE_LABELS[mode]}
          </button>
        </div>
        <textarea
          ref={inputRef}
          className={styles.input}
          placeholder={
            mode === 'agent'
              ? '输入指令... (例如：帮我创建一个 README.md)'
              : '输入消息... (Enter 发送，Shift+Enter 换行)'
          }
          value={input}
          onChange={e => setInput(e.target.value)}
          onKeyDown={handleKeyDown}
          rows={1}
        />
        <div className={styles.inputActions}>
          {isStreaming ? (
            <button className={styles.iconBtn} onClick={handleStop} title="停止生成" type="button">
              <StopCircle size={14} />
            </button>
          ) : messages.length > 0 && activeSession ? (
            <button
              className={styles.iconBtn}
              onClick={() => clearMessages(activeSession.id)}
              title="清空对话"
            >
              <Trash2 size={14} />
            </button>
          ) : null}
          <button
            className={styles.sendBtn}
            onClick={handleSend}
            disabled={!input.trim() || isStreaming}
          >
            {isStreaming ? (
              <Loader2 size={16} className={styles.loadingSpinner} />
            ) : (
              <Send size={16} />
            )}
          </button>
        </div>
      </div>
    </aside>
  );
};
