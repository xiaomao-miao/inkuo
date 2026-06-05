import { create } from 'zustand';
import { persist } from 'zustand/middleware';
import type { DiffHunk } from './editorStore';

export type ChatMode = 'ask' | 'plan' | 'agent' | 'knowledge';

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
  | {
      type: 'tool_call_start';
      toolCallId: string;
      toolName: string;
      // The latest accumulated arguments JSON string. Updated incrementally as
      // the AI streams the JSON argument. May be an incomplete JSON while the
      // tool call is still being received.
      arguments: Record<string, unknown>;
      rawArguments?: string;
      // Extracted content field from partial JSON parsing. This is updated
      // incrementally as the AI streams the content, allowing real-time preview.
      streamingContent?: string;
      // When true the tool has been registered as "executing" and the UI
      // should show the running indicator. After `tool_result` arrives this
      // item is updated in-place (for visual continuity) with result info.
      isExecuting?: boolean;
      // Result info populated when tool execution completes
      result?: string;
      status?: 'success' | 'error';
      duration?: number;
      diffSummary?: DiffSummary;
    }
  | { type: 'tool_result'; toolCallId: string; status: 'success' | 'error'; result: string; duration?: number; diffSummary?: DiffSummary }
  | { type: 'tool_error'; toolCallId: string; error: string };

export interface SearchResult {
  chunkId: string;
  documentId: string;
  content: string;
  score: number;
  documentTitle: string;
  filePath: string;
  startLine?: number;
  endLine?: number;
}

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
  searchResults?: SearchResult[];
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

export interface KnowledgeBase {
  workspaceId: string;
  documentCount: number;
  chunkCount: number;
  lastUpdated: number;
}

export interface BuildProgress {
  phase: 'scanning' | 'chunking' | 'embedding' | 'storing' | 'done';
  current: number;
  total: number;
  currentFile?: string;
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
  // Note: knowledgeBase, buildProgress, knowledgeToolCall moved to workspace-level (sidebarStore)
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

  // Note: knowledgeBase, buildProgress, knowledgeToolCall moved to workspace-level (sidebarStore)
  // Only per-message search results remain here:
  setMessageSearchResults: (sessionId: string, messageId: string, results: SearchResult[]) => void;

  getSession: (sessionId: string) => ChatSession | undefined;
  getMessage: (sessionId: string, messageId: string) => ChatMessage | undefined;
  updateSession: (sessionId: string, updater: (session: ChatSession) => ChatSession) => void;
  updateMessageOutput: (sessionId: string, messageId: string, outputItems: OutputItem[]) => void;
  addOutputToMessage: (sessionId: string, messageId: string, outputItem: OutputItem) => void;
  /**
   * Locate an output item by predicate (typically by toolCallId or content) and
   * merge a partial patch into it. Used to stream incremental updates — for
   * example extending a `tool_call_start` item's arguments as the AI emits
   * the JSON argument string chunk-by-chunk.
   */
  patchOutputItem: (
    sessionId: string,
    messageId: string,
    matchKey: { toolCallId: string } | { contentContains: string },
    patch: Partial<OutputItem>,
  ) => void;
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

        patchOutputItem: (sessionId, messageId, matchKey, patch) =>
          set((state) => ({
            sessions: state.sessions.map((s) => {
              if (s.id !== sessionId) return s;
              return {
                ...s,
                messages: s.messages.map((m) => {
                  if (m.id !== messageId) return m;
                  let matched = false;
                  const updatedItems = m.outputItems.map((item) => {
                    if (matched) return item;
                    if ('toolCallId' in matchKey) {
                      const tcId = (item as { toolCallId?: string }).toolCallId;
                      if (tcId !== matchKey.toolCallId) return item;
                    } else {
                      const text = (item as { content?: string }).content ?? '';
                      if (!text.includes(matchKey.contentContains)) return item;
                    }
                    matched = true;
                    return { ...item, ...patch } as OutputItem;
                  });
                  if (!matched) return m;
                  return { ...m, outputItems: updatedItems };
                }),
              };
            }),
          })),

        finishMessageStreaming: (sessionId, messageId, finalContent) =>
          set((state) => ({
            sessions: state.sessions.map((s) =>
              s.id === sessionId
                ? {
                    ...s,
                    isStreaming: false,
                    messages: s.messages.map((m) => {
                      if (m.id !== messageId) return m;

                      const textItemIndexes = m.outputItems
                        .map((item, index) => (item.type === 'text' ? index : -1))
                        .filter((index) => index >= 0);

                      const updatedOutputItems = textItemIndexes.length > 0
                        ? m.outputItems.map((item, index) => {
                            if (item.type !== 'text') return item;
                            const isLastTextItem = index === textItemIndexes[textItemIndexes.length - 1];
                            return isLastTextItem
                              ? { ...item, content: finalContent, isPendingMarkdown: false }
                              : item;
                          })
                        : [{ type: 'text' as const, content: finalContent, isPendingMarkdown: false }];

                      return {
                        ...m,
                        content: finalContent,
                        outputItems: updatedOutputItems,
                      };
                    }),
                  }
                : s
            ),
          })),

        setMessageSearchResults: (sessionId, messageId, results) =>
          set((state) => ({
            sessions: state.sessions.map((s) =>
              s.id === sessionId
                ? {
                    ...s,
                    messages: s.messages.map((m) =>
                      m.id === messageId ? { ...m, searchResults: results } : m
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
                    messages: s.messages.map((m) => {
                      if (m.id !== messageId) return m;

                      const hasVisibleOutput = m.outputItems.some((item) =>
                        item.type === 'text' || item.type === 'tool_call_start' || item.type === 'tool_error'
                      );

                      return {
                        ...m,
                        content: error,
                        outputItems: hasVisibleOutput
                          ? m.outputItems
                          : [{ type: 'text' as const, content: error, isPendingMarkdown: false }],
                      };
                    }),
                  }
                : s
            ),
          })),

        // Knowledge base state moved to sidebarStore (workspace-level).
        // Only per-message search results remain in session.
      };
    },
    {
      name: 'inkuo-aipanel',
      partialize: (state) => ({
        isOpen: state.isOpen,
        sessions: state.sessions.map((s) => ({
          id: s.id,
          title: s.title,
          createdAt: s.createdAt,
          mode: s.mode,
          messages: s.messages,
          isStreaming: false,
          currentDiff: null,
          activeToolCalls: [],
          pendingDiff: null,
          // KB state (knowledgeBase, buildProgress, knowledgeToolCall) moved to sidebarStore
        })),
        activeSessionId: state.activeSessionId,
      }),
      merge: (persisted, current) => {
        const persistedState = persisted as { isOpen?: boolean; sessions?: Partial<ChatSession>[]; activeSessionId?: string } | undefined;
        return {
          ...current,
          ...persistedState,
          sessions: (persistedState?.sessions ?? []).map((s) => ({
            id: s.id ?? '',
            title: s.title ?? '',
            createdAt: s.createdAt ?? 0,
            mode: s.mode ?? 'ask',
            messages: s.messages ?? [],
            isStreaming: false,
            currentDiff: null,
            activeToolCalls: s.activeToolCalls ?? [],
            pendingDiff: null,
            // KB state (knowledgeBase, buildProgress, knowledgeToolCall) moved to sidebarStore
          })),
        };
      },
    }
  )
);
