import { create } from 'zustand';
import { persist, createJSONStorage } from 'zustand/middleware';
import type { DiffHunk } from './editorStore';

export type ChatMode = 'ask' | 'plan' | 'agent';

export type MessageRole = 'user' | 'assistant' | 'system' | 'tool';

export interface MessageToolCall {
  id: string;
  name: string;
  arguments: Record<string, unknown>;
}

export interface DiffSummary {
  file_name: string;
  added_lines: number;
  deleted_lines: number;
  hunks: DiffHunk[];
}

export interface MessageToolResult {
  toolCallId: string;
  result: string;
  isError: boolean;
  duration?: number;
  diffSummary?: DiffSummary;
}

export type OutputItem =
  | { type: 'text'; content: string; isPendingMarkdown?: boolean }
  | { type: 'tool_call_start'; toolCallId: string; toolName: string; arguments: Record<string, unknown> }
  | { type: 'tool_result'; toolCallId: string; status: 'success' | 'error'; result: string; duration?: number; diffSummary?: DiffSummary }
  | { type: 'tool_error'; toolCallId: string; error: string };

export interface ChatMessage {
  id: string;
  role: MessageRole;
  timestamp: number;
  content?: string;
  outputItems: OutputItem[];
  toolCalls?: MessageToolCall[];
  toolResults?: MessageToolResult[];
  toolCallId?: string;
  toolResult?: MessageToolResult;
  diff?: CurrentDiff;
}

export interface CurrentDiff {
  originalText: string;
  newText: string;
  hunks: DiffHunk[];
  summary: string;
}

export interface ActiveToolCall {
  id: string;
  name: string;
  arguments: Record<string, unknown>;
  status: 'pending' | 'executing' | 'success' | 'error';
  result?: string;
  error?: string;
  startTime: number;
  duration?: number;
  diffSummary?: DiffSummary;
}

export interface ChatSession {
  id: string;
  title: string;
  createdAt: number;
  mode: ChatMode;
  messages: ChatMessage[];
  isStreaming: boolean;
  currentDiff: CurrentDiff | null;
  activeToolCalls: ActiveToolCall[];
  pendingDiff: CurrentDiff | null;
}

interface AIPanelState {
  isOpen: boolean;
  activeTab: 'chat' | 'edit';

  sessions: ChatSession[];
  activeSessionId: string;

  setIsOpen: (open: boolean) => void;
  togglePanel: () => void;
  setActiveTab: (tab: 'chat' | 'edit') => void;

  createSession: () => string;
  deleteSession: (sessionId: string) => void;
  setActiveSession: (sessionId: string) => void;
  setSessionMode: (sessionId: string, mode: ChatMode) => void;

  addMessage: (sessionId: string, message: ChatMessage) => void;
  updateMessage: (sessionId: string, messageId: string, content: string) => void;
  appendMessageContent: (sessionId: string, messageId: string, content: string) => void;
  setIsStreaming: (sessionId: string, streaming: boolean) => void;
  clearMessages: (sessionId: string) => void;
  truncateMessagesAfter: (sessionId: string, messageId: string) => void;

  addToolCall: (sessionId: string, toolCall: ActiveToolCall) => void;
  updateToolCall: (sessionId: string, toolCallId: string, update: Partial<ActiveToolCall>) => void;
  removeToolCall: (sessionId: string, toolCallId: string) => void;
  clearToolCalls: (sessionId: string) => void;

  setCurrentDiff: (sessionId: string, diff: CurrentDiff | null) => void;
  setMessageDiff: (sessionId: string, messageId: string, diff: CurrentDiff | null) => void;
  setPendingDiff: (sessionId: string, diff: CurrentDiff | null) => void;
  acceptHunk: (sessionId: string, hunkId: string) => void;
  rejectHunk: (sessionId: string, hunkId: string) => void;
  acceptAllHunks: (sessionId: string) => void;
  rejectAllHunks: (sessionId: string) => void;

  getSession: (sessionId: string) => ChatSession | undefined;
  getMessage: (sessionId: string, messageId: string) => ChatMessage | undefined;
  updateSession: (sessionId: string, updater: (session: ChatSession) => ChatSession) => void;
  updateMessageOutput: (sessionId: string, messageId: string, outputItems: OutputItem[]) => void;
  addOutputToMessage: (sessionId: string, messageId: string, outputItem: OutputItem) => void;
  finishMessageStreaming: (sessionId: string, messageId: string, finalContent: string) => void;
  setErrorMessage: (sessionId: string, messageId: string, error: string) => void;
}

function createSessionTitle(index: number) {
  return `对话 ${index}`;
}

function createNewSession(index: number): ChatSession {
  const now = Date.now();
  return {
    id: crypto.randomUUID(),
    title: createSessionTitle(index),
    createdAt: now,
    mode: 'ask',
    messages: [],
    isStreaming: false,
    currentDiff: null,
    activeToolCalls: [],
    pendingDiff: null,
  };
}

export const useAIPanelStore = create<AIPanelState>()(
  persist(
    (set, get) => {
      const initialSession = createNewSession(1);

      return {
        isOpen: true,
        activeTab: 'chat',

        sessions: [initialSession],
        activeSessionId: initialSession.id,

        setIsOpen: (open) => set({ isOpen: open }),
        togglePanel: () => set((state) => ({ isOpen: !state.isOpen })),
        setActiveTab: (tab) => set({ activeTab: tab }),

        createSession: () => {
          const index = get().sessions.length + 1;
          const session = createNewSession(index);
          set((state) => ({
            sessions: [session, ...state.sessions],
            activeSessionId: session.id,
          }));
          return session.id;
        },

        deleteSession: (sessionId) => {
          set((state) => {
            const remaining = state.sessions.filter((s) => s.id !== sessionId);
            const safeRemaining = remaining.length > 0 ? remaining : [createNewSession(1)];
            const nextActiveId =
              state.activeSessionId === sessionId ? safeRemaining[0].id : state.activeSessionId;

            return {
              sessions: safeRemaining,
              activeSessionId: nextActiveId,
            };
          });
        },

        setActiveSession: (sessionId) => set({ activeSessionId: sessionId }),

        setSessionMode: (sessionId, mode) =>
          set((state) => ({
            sessions: state.sessions.map((s) => (s.id === sessionId ? { ...s, mode } : s)),
          })),

        addMessage: (sessionId, message) =>
          set((state) => ({
            sessions: state.sessions.map((s) =>
              s.id === sessionId ? { ...s, messages: [...s.messages, message] } : s
            ),
          })),

        updateMessage: (sessionId, messageId, content) =>
          set((state) => ({
            sessions: state.sessions.map((s) =>
              s.id === sessionId
                ? {
                    ...s,
                    messages: s.messages.map((m) => (m.id === messageId ? { ...m, content } : m)),
                  }
                : s
            ),
          })),

        appendMessageContent: (sessionId, messageId, content) =>
          set((state) => ({
            sessions: state.sessions.map((s) =>
              s.id === sessionId
                ? {
                    ...s,
                    messages: s.messages.map((m) =>
                      m.id === messageId
                        ? { ...m, content: m.content + content }
                        : m
                    ),
                  }
                : s
            ),
          })),

        setIsStreaming: (sessionId, streaming) =>
          set((state) => ({
            sessions: state.sessions.map((s) => (s.id === sessionId ? { ...s, isStreaming: streaming } : s)),
          })),

        clearMessages: (sessionId) =>
          set((state) => ({
            sessions: state.sessions.map((s) =>
              s.id === sessionId ? { ...s, messages: [], isStreaming: false, currentDiff: null, activeToolCalls: [], pendingDiff: null } : s
            ),
          })),

        truncateMessagesAfter: (sessionId, messageId) =>
          set((state) => ({
            sessions: state.sessions.map((s) =>
              s.id === sessionId
                ? {
                    ...s,
                    messages: s.messages.slice(0, s.messages.findIndex((m) => m.id === messageId) + 1),
                    isStreaming: false,
                    currentDiff: null,
                    activeToolCalls: [],
                    pendingDiff: null,
                  }
                : s
            ),
          })),

        addToolCall: (sessionId, toolCall) =>
          set((state) => ({
            sessions: state.sessions.map((s) =>
              s.id === sessionId
                ? { ...s, activeToolCalls: [...s.activeToolCalls, toolCall] }
                : s
            ),
          })),

        updateToolCall: (sessionId, toolCallId, update) =>
          set((state) => ({
            sessions: state.sessions.map((s) =>
              s.id === sessionId
                ? {
                    ...s,
                    activeToolCalls: s.activeToolCalls.map((tc) =>
                      tc.id === toolCallId ? { ...tc, ...update } : tc
                    ),
                  }
                : s
            ),
          })),

        removeToolCall: (sessionId, toolCallId) =>
          set((state) => ({
            sessions: state.sessions.map((s) =>
              s.id === sessionId
                ? { ...s, activeToolCalls: s.activeToolCalls.filter((tc) => tc.id !== toolCallId) }
                : s
            ),
          })),

        clearToolCalls: (sessionId) =>
          set((state) => ({
            sessions: state.sessions.map((s) =>
              s.id === sessionId
                ? { ...s, activeToolCalls: [] }
                : s
            ),
          })),

        setCurrentDiff: (sessionId, diff) =>
          set((state) => ({
            sessions: state.sessions.map((s) => (s.id === sessionId ? { ...s, currentDiff: diff } : s)),
          })),

        setMessageDiff: (sessionId, messageId, diff) =>
          set((state) => ({
            sessions: state.sessions.map((s) =>
              s.id === sessionId
                ? {
                    ...s,
                    messages: s.messages.map((m) =>
                      m.id === messageId ? { ...m, diff: diff ?? undefined } as ChatMessage : m
                    ),
                  }
                : s
            ),
          })),

        setPendingDiff: (sessionId, diff) =>
          set((state) => ({
            sessions: state.sessions.map((s) =>
              s.id === sessionId ? { ...s, pendingDiff: diff } : s
            ),
          })),

        acceptHunk: (sessionId, hunkId) =>
          set((state) => ({
            sessions: state.sessions.map((s) => {
              if (s.id !== sessionId) return s;

              if (s.currentDiff) {
                const newHunks = s.currentDiff.hunks.filter((h) => h.id !== hunkId);
                return {
                  ...s,
                  currentDiff: newHunks.length > 0 ? { ...s.currentDiff, hunks: newHunks } : null,
                };
              }

              const updatedMessages = s.messages.map((m) => {
                if (!m.diff) return m;
                const newHunks = m.diff.hunks.filter((h) => h.id !== hunkId);
                const { diff: _, ...rest } = m;
                return { ...rest, diff: newHunks.length > 0 ? { ...m.diff, hunks: newHunks } : undefined } as ChatMessage;
              });

              return { ...s, messages: updatedMessages };
            }),
          })),

        rejectHunk: (sessionId, hunkId) =>
          set((state) => ({
            sessions: state.sessions.map((s) => {
              if (s.id !== sessionId) return s;

              if (s.currentDiff) {
                const newHunks = s.currentDiff.hunks.filter((h) => h.id !== hunkId);
                return {
                  ...s,
                  currentDiff: newHunks.length > 0 ? { ...s.currentDiff, hunks: newHunks } : null,
                };
              }

              const updatedMessages = s.messages.map((m) => {
                if (!m.diff) return m;
                const newHunks = m.diff.hunks.filter((h) => h.id !== hunkId);
                const { diff: _, ...rest } = m;
                return { ...rest, diff: newHunks.length > 0 ? { ...m.diff, hunks: newHunks } : undefined } as ChatMessage;
              });

              return { ...s, messages: updatedMessages };
            }),
          })),

        acceptAllHunks: (sessionId) =>
          set((state) => ({
            sessions: state.sessions.map((s) => {
              if (s.id !== sessionId) return s;

              if (s.currentDiff || s.pendingDiff) {
                return { ...s, currentDiff: null, pendingDiff: null };
              }

              const updatedMessages = s.messages.map((m) => {
                const { diff: _, ...rest } = m;
                return { ...rest, diff: undefined } as ChatMessage;
              });

              return { ...s, messages: updatedMessages };
            }),
          })),

        rejectAllHunks: (sessionId) =>
          set((state) => ({
            sessions: state.sessions.map((s) => {
              if (s.id !== sessionId) return s;

              if (s.currentDiff || s.pendingDiff) {
                return { ...s, currentDiff: null, pendingDiff: null };
              }

              const updatedMessages = s.messages.map((m) => {
                const { diff: _, ...rest } = m;
                return { ...rest, diff: undefined } as ChatMessage;
              });

              return { ...s, messages: updatedMessages };
            }),
          })),

        getSession: (sessionId) => get().sessions.find((s) => s.id === sessionId),

        getMessage: (sessionId, messageId) => {
          const session = get().sessions.find((s) => s.id === sessionId);
          return session?.messages.find((m) => m.id === messageId);
        },

        updateSession: (sessionId, updater) =>
          set((state) => ({
            sessions: state.sessions.map((s) =>
              s.id === sessionId ? updater(s) : s
            ),
          })),

        updateMessageOutput: (sessionId, messageId, outputItems) =>
          set((state) => ({
            sessions: state.sessions.map((s) =>
              s.id === sessionId
                ? {
                    ...s,
                    messages: s.messages.map((m) =>
                      m.id === messageId ? { ...m, outputItems } : m
                    ),
                  }
                : s
            ),
          })),

        addOutputToMessage: (sessionId, messageId, outputItem) =>
          set((state) => ({
            sessions: state.sessions.map((s) =>
              s.id === sessionId
                ? {
                    ...s,
                    messages: s.messages.map((m) =>
                      m.id === messageId
                        ? { ...m, outputItems: [...m.outputItems, outputItem] }
                        : m
                    ),
                  }
                : s
            ),
          })),

        finishMessageStreaming: (sessionId, messageId, finalContent) =>
          set((state) => ({
            sessions: state.sessions.map((s) =>
              s.id === sessionId
                ? {
                    ...s,
                    isStreaming: false,
                    messages: s.messages.map((m) =>
                      m.id === messageId
                        ? {
                            ...m,
                            content: m.content || finalContent,
                            outputItems: m.outputItems.length > 0
                              ? m.outputItems.map((item) =>
                                  item.type === 'text'
                                    ? { ...item, isPendingMarkdown: false }
                                    : item
                                )
                              : [{ type: 'text' as const, content: finalContent, isPendingMarkdown: false }],
                          }
                        : m
                    ),
                  }
                : s
            ),
          })),

        setErrorMessage: (sessionId, messageId, error) =>
          set((state) => ({
            sessions: state.sessions.map((s) =>
              s.id === sessionId
                ? {
                    ...s,
                    isStreaming: false,
                    messages: s.messages.map((m) =>
                      m.id === messageId ? { ...m, content: error } : m
                    ),
                  }
                : s
            ),
          })),
      };
    },
    {
      name: 'inkuo-aipanel',
      // 只持久化必要的元数据，不持久化 messages 和 outputItems（它们可能很大）
      // 使用 shallow 比较避免深度对象比较
      partialize: (state) => ({
        isOpen: state.isOpen,
        sessions: state.sessions.map((s) => ({
          id: s.id,
          title: s.title,
          createdAt: s.createdAt,
          mode: s.mode,
          // 不持久化 messages、isStreaming、currentDiff、activeToolCalls、pendingDiff
        })),
        activeSessionId: state.activeSessionId,
      }),
      // 延迟写入 localStorage，减少频繁写入
      storage: createJSONStorage(() => localStorage, {
        reviver: (key, value) => {
          // 处理 Set 类型（expandedDirs）
          if (key === 'sessions') {
            return value;
          }
          return value;
        },
      }),
    }
  )
);
