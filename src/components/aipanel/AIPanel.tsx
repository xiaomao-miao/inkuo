import React, { useState, useRef, useEffect, useMemo } from 'react';
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
  Check,
  X,
  Terminal,
  File,
  FolderOpen,
  Search,
  FileEdit,
  Sparkles,
  Plus,
  Minus,
} from 'lucide-react';
import {
  useAIPanelStore,
  useEditorStore,
  useSidebarStore,
  type ChatMessage,
  type ChatMode,
  type ActiveToolCall,
} from '../../store';
import styles from './AIPanel.module.css';
import { parsePlanBlocks, type PlanBlock } from './planRender';

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

// Tool icons
const TOOL_ICONS: Record<string, React.ReactNode> = {
  read_file: <File size={14} />,
  write_file: <FileEdit size={14} />,
  edit_file: <FileEdit size={14} />,
  list_dir: <FolderOpen size={14} />,
  glob: <File size={14} />,
  grep: <Search size={14} />,
};

// Get tool display name
const getToolDisplayName = (name: string): string => {
  const names: Record<string, string> = {
    read_file: '读取文件',
    write_file: '写入文件',
    edit_file: '编辑文件',
    list_dir: '列出目录',
    glob: '查找文件',
    grep: '搜索文本',
  };
  return names[name] || name;
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
    acceptAllHunks,
    rejectAllHunks,
    setIsOpen,
    clearToolCalls,
  } = useAIPanelStore();

  const activeSession = useMemo(
    () => sessions.find((s) => s.id === activeSessionId) ?? sessions[0],
    [sessions, activeSessionId]
  );

  const messages = activeSession?.messages ?? [];
  const isStreaming = activeSession?.isStreaming ?? false;
  const currentDiff = activeSession?.currentDiff ?? null;
  const mode: ChatMode = activeSession?.mode ?? 'ask';
  const activeToolCalls = activeSession?.activeToolCalls ?? [];

  const { selectedFile } = useSidebarStore();
  const { documentContents, getSelection } = useEditorStore();

  const [input, setInput] = useState('');

  const messagesEndRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLTextAreaElement>(null);

  // Ref to track accumulated content for streaming
  const streamingContentRef = useRef<Record<string, string>>({});

  // Ref to track the unlisten function
  const unlistenRef = useRef<(() => void) | null>(null);

  // Ref to track if listener is being set up (prevent race condition)
  const isSettingUpRef = useRef(false);

  // Refs to track latest values for event handlers
  const getSelectionRef = useRef(getSelection);
  const selectedFileRef = useRef(selectedFile);
  const documentContentsRef = useRef(documentContents);
  const workspacePathRef = useRef<string | null>(null);

  // Keep refs updated
  useEffect(() => {
    getSelectionRef.current = getSelection;
    selectedFileRef.current = selectedFile;
    documentContentsRef.current = documentContents;
    workspacePathRef.current = useSidebarStore.getState().workspacePath;
  });

  // Scroll to bottom when new messages arrive
  useEffect(() => {
    messagesEndRef.current?.scrollIntoView({ behavior: 'smooth' });
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
    const instruction = input.trim();

    const userMessage: ChatMessage = {
      id: Date.now().toString(),
      role: 'user',
      content: instruction,
      timestamp: Date.now(),
    };

    const assistantMessageId = (Date.now() + 1).toString();
    const assistantPlaceholder: ChatMessage = {
      id: assistantMessageId,
      role: 'assistant',
      content: '',
      timestamp: Date.now(),
    };

    addMessage(sessionId, userMessage);
    addMessage(sessionId, assistantPlaceholder);
    setInput('');
    setIsStreaming(sessionId, true);

    // Clear any previous tool calls
    clearToolCalls(sessionId);

    try {
      const selection = getSelection();
      const currentDoc = selectedFile ? documentContents[selectedFile] : null;
      const originalText = selection || currentDoc?.content || '';

      if (mode === 'agent') {
        // Use full agent with tool calling with context memory
        const workspacePath = useSidebarStore.getState().workspacePath || undefined;
        // Get conversation history (excluding the messages we're about to add)
        const conversationHistory = messages.map(m => ({
          id: m.id,
          role: m.role,
          content: m.content,
          tool_calls: m.toolCalls,
          tool_call_id: m.toolCallId,
        }));
        await invoke('ai_agent_stream', {
          sessionId,
          messageId: assistantMessageId,
          instruction,
          workspacePath,
          history: conversationHistory,
        });
      } else {
        // Use simple chat/edit mode
        await invoke('ai_chat_stream', {
          sessionId,
          messageId: assistantMessageId,
          mode,
          instruction,
          originalText: originalText.slice(0, 5000),
        });
      }
    } catch (err) {
      updateMessage(sessionId, assistantMessageId, `抱歉，发生了错误：${err}`);
      setIsStreaming(sessionId, false);
    }
  };

  const handleKeyDown = (e: React.KeyboardEvent) => {
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

          // Handle error event
          if (event_type === 'error') {
            console.log('[Stream Error]', error);
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

            const newToolCall: ActiveToolCall = {
              id: tool_call_id,
              name: tool_name,
              arguments: args,
              status: 'executing',
              startTime: Date.now(),
            };

            useAIPanelStore.setState((state) => ({
              sessions: state.sessions.map((s) =>
                s.id === session_id
                  ? { ...s, activeToolCalls: [...s.activeToolCalls, newToolCall] }
                  : s
              ),
            }));
            return;
          }

          // Handle tool result
          if (event_type === 'tool_result' && tool_call_id) {
            const isError = !!error;
            const duration = useAIPanelStore.getState().sessions
              .find((s) => s.id === session_id)
              ?.activeToolCalls.find((tc) => tc.id === tool_call_id)?.startTime;

            useAIPanelStore.setState((state) => ({
              sessions: state.sessions.map((s) =>
                s.id === session_id
                  ? {
                      ...s,
                      activeToolCalls: s.activeToolCalls.map((tc) =>
                        tc.id === tool_call_id
                          ? {
                              ...tc,
                              status: isError ? 'error' : 'success',
                              result: content,
                              error: isError ? error : undefined,
                              duration: duration ? Date.now() - duration : undefined,
                            }
                          : tc
                      ),
                    }
                  : s
              ),
            }));
            return;
          }

          // Handle text delta
          if (typeof content === 'string' && content.length > 0) {
            // Use ref to accumulate content reliably
            const currentAccumulated = streamingContentRef.current[message_id] || '';
            streamingContentRef.current[message_id] = currentAccumulated + content;

            useAIPanelStore.setState((state) => ({
              sessions: state.sessions.map((s) =>
                s.id === session_id
                  ? {
                      ...s,
                      messages: s.messages.map((m) =>
                        m.id === message_id
                          ? { ...m, content: streamingContentRef.current[message_id] || '' }
                          : m
                      ),
                    }
                  : s
              ),
            }));
          }

          // Handle done event
          if (done) {
            // Clean up ref
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

            // Handle final content for agent mode - compute diff if needed
            if (final_content && mode === 'agent') {
              try {
                const selection = getSelectionRef.current();
                const currentDoc = selectedFileRef.current
                  ? documentContentsRef.current[selectedFileRef.current]
                  : null;
                const originalText = selection || currentDoc?.content || '';

                const diff = await invoke<any>('compute_diff', {
                  oldText: originalText,
                  newText: final_content,
                });

                useAIPanelStore.getState().setCurrentDiff(session_id, {
                  originalText,
                  newText: final_content,
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
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const cycleMode = () => {
    if (!activeSession) return;
    const order: ChatMode[] = ['ask', 'plan', 'agent'];
    const idx = order.indexOf(mode);
    setSessionMode(activeSession.id, order[(idx + 1) % order.length]);
  };

  // Render tool call card
  const renderToolCall = (toolCall: ActiveToolCall) => {
    const icon = TOOL_ICONS[toolCall.name] || <Terminal size={14} />;
    const displayName = getToolDisplayName(toolCall.name);

    return (
      <div
        key={toolCall.id}
        className={`${styles.toolCall} ${styles[`toolCall_${toolCall.status}`]}`}
      >
        <div className={styles.toolHeader}>
          <div className={styles.toolIcon}>{icon}</div>
          <span className={styles.toolName}>{displayName}</span>
          <div className={styles.toolStatus}>
            {toolCall.status === 'executing' && (
              <>
                <Loader2 size={12} className={styles.spinning} />
                <span>执行中...</span>
              </>
            )}
            {toolCall.status === 'success' && (
              <>
                <Check size={12} />
                <span>成功</span>
              </>
            )}
            {toolCall.status === 'error' && (
              <>
                <X size={12} />
                <span>失败</span>
              </>
            )}
          </div>
        </div>

        <div className={styles.toolArgs}>
          <pre>{JSON.stringify(toolCall.arguments, null, 2)}</pre>
        </div>

        {toolCall.result && (
          <div className={`${styles.toolResult} ${toolCall.error ? styles.errorResult : ''}`}>
            <div className={styles.toolResultLabel}>
              {toolCall.error ? '错误信息' : '执行结果'}
            </div>
            <pre>{toolCall.result}</pre>
          </div>
        )}

        {toolCall.duration && (
          <div className={styles.toolDuration}>
            耗时: {toolCall.duration}ms
          </div>
        )}
      </div>
    );
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
                  {s.messages.length > 0
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
      <div className={styles.content}>
        {/* Active Tool Calls */}
        {activeToolCalls.length > 0 && (
          <div className={styles.toolCallsContainer}>
            <div className={styles.toolCallsHeader}>
              <Terminal size={14} />
              <span>工具调用 ({activeToolCalls.length})</span>
            </div>
            <div className={styles.toolCallsList}>
              {activeToolCalls.map(renderToolCall)}
            </div>
          </div>
        )}

        {/* Diff view */}
        {currentDiff && (
          <div className={styles.diffContainer}>
            <div className={styles.diffHeader}>
              <h4>AI 修改建议</h4>
              <div className={styles.diffActions}>
                <button
                  className={styles.diffBtnAccept}
                  onClick={() => activeSession && acceptAllHunks(activeSession.id)}
                >
                  <Check size={14} />
                  接受全部
                </button>
                <button
                  className={styles.diffBtnReject}
                  onClick={() => activeSession && rejectAllHunks(activeSession.id)}
                >
                  <X size={14} />
                  拒绝全部
                </button>
              </div>
            </div>
            <div className={styles.diffSummary}>{currentDiff.summary}</div>
            <div className={styles.diffContent}>
              <div className={styles.diffOld}>
                <div className={styles.diffLabel}><Minus size={12} /><span>原文</span></div>
                <pre className={styles.diffText}>{currentDiff.originalText}</pre>
              </div>
              <div className={styles.diffNew}>
                <div className={styles.diffLabel}><Plus size={12} /><span>修改后</span></div>
                <pre className={styles.diffText}>{currentDiff.newText}</pre>
              </div>
            </div>
          </div>
        )}

        {/* Messages */}
        {messages.length === 0 && !currentDiff ? (
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
            {messages.map((message) => (
              <div key={message.id} className={`${styles.message} ${styles[message.role]}`}>
                {message.role === 'user' ? (
                  <div className={styles.messageBubble}>
                    {message.content}
                  </div>
                ) : (
                  <div className={styles.messageContent}>
                    {mode === 'plan' && message.content ? (
                      <div className={styles.planBlocks}>
                        {parsePlanBlocks(message.content).map((b: PlanBlock, idx: number) => (
                          <div key={idx} className={styles.planBlock}>
                            <div className={styles.planTitle}>{b.title}</div>
                            <pre className={styles.planBody}>{b.lines.join('\n')}</pre>
                          </div>
                        ))}
                      </div>
                    ) : (
                      message.content
                    )}
                  </div>
                )}
              </div>
            ))}
            {isStreaming && (
              <div className={styles.messageContent}>
                <Loader2 size={14} className={styles.loadingSpinner} />
                <span>
                  {activeToolCalls.length > 0
                    ? `正在执行工具... (${activeToolCalls.length})`
                    : '正在思考...'}
                </span>
              </div>
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
