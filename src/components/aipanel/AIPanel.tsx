import React, { useState, useRef, useEffect, useMemo } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import {
  MessageSquare,
  PanelRightClose,
  Loader2,
  Copy,
  Trash2,
  ChevronDown,
  FileText,
  MousePointer,
  Sparkles,
  Plus,
  Minus,
  AlignLeft,
  FileCode,
  Send,
  PlusCircle,
  StopCircle,
  Check,
  X,
} from 'lucide-react';
import { useAIPanelStore, useEditorStore, useSidebarStore, type ChatMessage, type ChatMode } from '../../store';
import styles from './AIPanel.module.css';
import { parsePlanBlocks, type PlanBlock } from './planRender';

type ScopeType = 'selection' | 'paragraph' | 'section' | 'document';

// Template prompts
const TEMPLATES = [
  { icon: <Sparkles size={14} />, label: '更专业', prompt: '请用更专业的语气重写' },
  { icon: <Minus size={14} />, label: '更精炼', prompt: '请更精炼地表达，去除冗余' },
  { icon: <AlignLeft size={14} />, label: '改写成表格', prompt: '请改写成表格格式' },
  { icon: <FileCode size={14} />, label: '生成小标题', prompt: '请生成合适的小标题' },
  { icon: <Plus size={14} />, label: '扩展内容', prompt: '请扩展这段内容，提供更多细节' },
  { icon: <FileText size={14} />, label: '翻译英文', prompt: '请翻译为英文并保留专业术语' },
];

const SCOPE_OPTIONS: { key: ScopeType; icon: React.ReactNode; label: string }[] = [
  { key: 'selection', icon: <MousePointer size={14} />, label: '选区' },
  { key: 'paragraph', icon: <AlignLeft size={14} />, label: '段落' },
  { key: 'section', icon: <FileText size={14} />, label: '章节' },
  { key: 'document', icon: <FileCode size={14} />, label: '文档' },
];

const MODE_LABELS: Record<ChatMode, string> = {
  ask: 'Ask',
  plan: 'Plan',
  agent: 'Agent',
};

const MODE_HINTS: Record<ChatMode, string> = {
  ask: '只回答（不修改文件）',
  plan: '只输出计划（不修改文件）',
  agent: '允许修改（会给出可应用变更）',
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
    setCurrentDiff,
    acceptAllHunks,
    rejectAllHunks,
    setIsOpen,
  } = useAIPanelStore();

  const activeSession = useMemo(
    () => sessions.find((s) => s.id === activeSessionId) ?? sessions[0],
    [sessions, activeSessionId]
  );

  const messages = activeSession?.messages ?? [];
  const isStreaming = activeSession?.isStreaming ?? false;
  const currentDiff = activeSession?.currentDiff ?? null;
  const mode: ChatMode = activeSession?.mode ?? 'ask';

  const { selectedFile } = useSidebarStore();
  const { documentContents, getSelection } = useEditorStore();

  const [input, setInput] = useState('');
  const [scope, setScope] = useState<ScopeType>('selection');
  const [showScopeMenu, setShowScopeMenu] = useState(false);
  const [showTemplates, setShowTemplates] = useState(false);

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

  // Keep refs updated
  useEffect(() => {
    getSelectionRef.current = getSelection;
    selectedFileRef.current = selectedFile;
    documentContentsRef.current = documentContents;
  });

  // Scroll to bottom when new messages arrive
  useEffect(() => {
    messagesEndRef.current?.scrollIntoView({ behavior: 'smooth' });
  }, [messages]);

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

    try {
      const selection = getSelection();
      const currentDoc = selectedFile ? documentContents[selectedFile] : null;
      const originalText = selection || currentDoc?.content || '';

      if (mode === 'agent') {
        await invoke('ai_edit_stream', {
          sessionId,
          messageId: assistantMessageId,
          instruction,
          originalText: originalText.slice(0, 5000),
          scope,
          context: [],
        });
      } else {
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

  const handleCopy = (content: string) => {
    navigator.clipboard.writeText(content);
  };

  const handleTemplateClick = (prompt: string) => {
    setInput(prev => prev ? prev + ' ' + prompt : prompt);
    setShowTemplates(false);
    inputRef.current?.focus();
  };

  const handleStop = async () => {
    if (!activeSession) return;
    try {
      await invoke('ai_stream_cancel', { sessionId: activeSession.id });
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
        const { session_id, message_id, event_type, content, done, summary, final_content, error } = payload;

        if (!payload || !session_id || !message_id) return;

        // Debug logging
        console.log('[Stream Event]', { session_id, message_id, event_type, content: content?.slice(0, 50), done });

        if (event_type === 'error') {
          console.log('[Stream Error]', error);
          useAIPanelStore.setState((state) => {
              const session = state.sessions.find((s) => s.id === session_id);
              if (!session) return state;
              return {
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
              };
            });
            return;
          }

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

          if (done) {
            // Clean up ref
            delete streamingContentRef.current[message_id];

            useAIPanelStore.setState((state) => ({
              sessions: state.sessions.map((s) =>
                s.id === session_id ? { ...s, isStreaming: false } : s
              ),
            }));

            if (final_content) {
              const state = useAIPanelStore.getState();
              const currentSession = state.sessions.find((s) => s.id === session_id);

              if (currentSession?.mode === 'agent') {
                try {
                  const selection = getSelectionRef.current();
                  const currentDoc = selectedFileRef.current ? documentContentsRef.current[selectedFileRef.current] : null;
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

  const getScopeIcon = () => {
    const opt = SCOPE_OPTIONS.find(o => o.key === scope);
    return opt?.icon ?? <MousePointer size={14} />;
  };

  const getScopeLabel = () => {
    const opt = SCOPE_OPTIONS.find(o => o.key === scope);
    return opt?.label ?? '选区';
  };

  const cycleMode = () => {
    if (!activeSession) return;
    const order: ChatMode[] = ['ask', 'plan', 'agent'];
    const idx = order.indexOf(mode);
    setSessionMode(activeSession.id, order[(idx + 1) % order.length]);
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
        {/* Scope bar (shown when there's a currentDiff or when user wants edit scope) */}
        {mode === 'agent' && (
          <div className={styles.scopeBar}>
            <div className={styles.scopeSelector}>
              <button
                className={styles.scopeButton}
                onClick={() => setShowScopeMenu(!showScopeMenu)}
              >
                {getScopeIcon()}
                <span>{getScopeLabel()}</span>
                <ChevronDown size={12} />
              </button>
              {showScopeMenu && (
                <div className={styles.scopeMenu}>
                  {SCOPE_OPTIONS.map((opt) => (
                    <button
                      key={opt.key}
                      className={`${styles.scopeMenuItem} ${scope === opt.key ? styles.active : ''}`}
                      onClick={() => { setScope(opt.key); setShowScopeMenu(false); }}
                    >
                      {opt.icon}
                      <span>{opt.label}</span>
                    </button>
                  ))}
                </div>
              )}
            </div>
            <button
              className={styles.templateButton}
              onClick={() => setShowTemplates(!showTemplates)}
            >
              <Sparkles size={14} />
              <span>模板</span>
            </button>
          </div>
        )}

        {showTemplates && (
          <div className={styles.templates}>
            {TEMPLATES.map((t, i) => (
              <button
                key={i}
                className={styles.templateItem}
                onClick={() => handleTemplateClick(t.prompt)}
              >
                {t.icon}
                <span>{t.label}</span>
              </button>
            ))}
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
            <p>询问关于文档的问题或请求 AI 帮助你写作</p>
            <div className={styles.quickActions}>
              <button className={styles.quickAction} onClick={() => setInput('总结这篇文档的主要内容')}>
                总结文档
              </button>
              <button className={styles.quickAction} onClick={() => setInput('解释这段代码/文本的工作原理')}>
                解释内容
              </button>
            </div>
          </div>
        ) : (
          <div className={styles.messages}>
            {messages.map((message) => (
              <div key={message.id} className={`${styles.message} ${styles[message.role]}`}>
                <div className={styles.messageHeader}>
                  <span className={styles.messageRole}>
                    {message.role === 'user' ? '你' : 'AI'}
                  </span>
                  <div className={styles.messageActions}>
                    <button className={styles.actionBtn} onClick={() => handleCopy(message.content)} title="复制">
                      <Copy size={12} />
                    </button>
                  </div>
                </div>
                <div className={styles.messageContent}>
                  {mode === 'plan' && message.role === 'assistant' ? (
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
              </div>
            ))}
            {isStreaming && (
              <div className={`${styles.message} ${styles.assistant}`}>
                <div className={styles.messageHeader}>
                  <span className={styles.messageRole}>AI</span>
                </div>
                <div className={styles.messageContent}>
                  <Loader2 size={14} className={styles.loadingSpinner} />
                  <span>正在思考...</span>
                </div>
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
            className={styles.modeButton}
            onClick={cycleMode}
            disabled={!activeSession || isStreaming}
            title={MODE_HINTS[mode]}
          >
            {MODE_LABELS[mode]}
          </button>
        </div>
        <textarea
          ref={inputRef}
          className={styles.input}
          placeholder={
            mode === 'agent'
              ? '输入编辑指令... (例如：让这段话更专业)'
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
