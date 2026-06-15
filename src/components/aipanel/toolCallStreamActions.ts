import type {
  ActiveToolCall,
  ChatMessage,
  OutputItem,
  StreamDiffSummary,
} from '../../types';
import type { AIPanelState } from '../../store/aiPanelStore.types';

export interface PendingToolArgEntry {
  sessionId: string;
  messageId: string;
  tool_call_id: string;
  rawArgs: string;
  parsedArgs: Record<string, unknown>;
  streamingContent?: string;
}

interface ApplyToolResultArgs {
  state: AIPanelState;
  sessionId: string;
  messageId: string;
  toolCallId: string;
  content: string;
  error?: string;
  diffSummary?: StreamDiffSummary;
  duration?: number;
}

interface ToolCallStartStateArgs {
  state: AIPanelState;
  sessionId: string;
  messageId: string;
  toolCallId: string;
  toolName: string;
  parsedArgs: Record<string, unknown>;
  rawArgs: string;
  streamingContent?: string;
  startTime: number;
}

function buildToolCallEntry(
  toolCallId: string,
  toolName: string,
  parsedArgs: Record<string, unknown>,
  startTime: number,
): ActiveToolCall {
  return {
    id: toolCallId,
    name: toolName,
    arguments: parsedArgs,
    status: 'executing',
    startTime,
  };
}

function buildToolCallOutputItem(
  toolCallId: string,
  toolName: string,
  parsedArgs: Record<string, unknown>,
  rawArgs: string,
  streamingContent?: string,
): Extract<OutputItem, { type: 'tool_call_start' }> {
  return {
    type: 'tool_call_start',
    toolCallId,
    toolName,
    arguments: parsedArgs,
    rawArguments: rawArgs,
    streamingContent,
    isExecuting: true,
  };
}

function buildToolResultMessage(
  toolCallId: string,
  content: string,
  isError: boolean,
  duration?: number,
  diffSummary?: StreamDiffSummary,
) {
  return {
    toolCallId,
    result: content,
    isError,
    duration,
    diffSummary,
  };
}

export function applyToolCallStartToState({
  state,
  sessionId,
  messageId,
  toolCallId,
  toolName,
  parsedArgs,
  rawArgs,
  streamingContent,
  startTime,
}: ToolCallStartStateArgs): AIPanelState {
  return {
    ...state,
    sessions: state.sessions.map((session) => {
      if (session.id !== sessionId) return session;

      // Add to activeToolCalls so the panel can show "executing" status.
      // The tool is removed from activeToolCalls in applyToolResultToState.
      const activeToolCalls = [
        ...session.activeToolCalls,
        buildToolCallEntry(toolCallId, toolName, parsedArgs, startTime),
      ];

      return {
        ...session,
        activeToolCalls,
        messages: session.messages.map((message) => {
          if (message.id !== messageId) return message;

          // Always append a new outputItem. Do NOT try to "update existing" —
          // that creates subtle bugs where an old entry's streamingContent
          // leaks into the new card via object spread.
          return {
            ...message,
            toolCalls: [...(message.toolCalls || []), { id: toolCallId, name: toolName, arguments: parsedArgs }],
            outputItems: [
              ...message.outputItems,
              buildToolCallOutputItem(toolCallId, toolName, parsedArgs, rawArgs, streamingContent),
            ],
          } as ChatMessage;
        }),
      };
    }),
  };
}

export function applyPendingToolArgs(
  state: AIPanelState,
  pendingEntries: PendingToolArgEntry[],
): AIPanelState {
  if (pendingEntries.length === 0) return state;

  const sessionIds = new Set(pendingEntries.map((entry) => entry.sessionId));

  return {
    ...state,
    sessions: state.sessions.map((session) => {
      if (!sessionIds.has(session.id)) return session;

      let sessionMutated = false;
      const updatedMessages = session.messages.map((message) => {
        let messageMutated = false;
        const updatedItems = message.outputItems.map((item) => {
          if (item.type !== 'tool_call_start') return item;

          const entry = pendingEntries.find(
            (candidate) =>
              candidate.messageId === message.id &&
              item.toolCallId === candidate.tool_call_id &&
              candidate.sessionId === session.id
          );
          if (!entry) return item;

          // Skip items that have already received their result — updating
          // those would create an unnecessary new reference and re-render
          // a card that is already in its final state.
          if ('result' in item) return item;

          messageMutated = true;
          return {
            ...item,
            arguments: entry.parsedArgs,
            rawArguments: entry.rawArgs,
          };
        });

        if (messageMutated) {
          sessionMutated = true;
          return { ...message, outputItems: updatedItems };
        }
        return message;
      });

      if (sessionMutated) {
        return { ...session, messages: updatedMessages };
      }
      return session;
    }),
  };
}

export function applyToolResultToState({
  state,
  sessionId,
  messageId,
  toolCallId,
  content,
  error,
  diffSummary,
  duration,
}: ApplyToolResultArgs): AIPanelState {
  const isError = !!error;

  return {
    ...state,
    sessions: state.sessions.map((session) => {
      if (session.id !== sessionId) return session;

      // Remove from activeToolCalls so that a subsequent tool_call_start
      // with the same tool_call_id (which can happen when the AI references
      // a previous tool) will correctly create a NEW outputItem instead of
      // patching the completed one.
      const activeToolCalls = session.activeToolCalls.filter((entry) => entry.id !== toolCallId);

      return {
        ...session,
        activeToolCalls,
        messages: session.messages.map((message) => {
          if (message.id !== messageId) return message;

          const updatedItems = message.outputItems
            .filter((item) => !(item.type === 'tool_result' && item.toolCallId === toolCallId))
            .map((item) => {
              if (item.type !== 'tool_call_start' || item.toolCallId !== toolCallId) return item;
              return {
                ...item,
                isExecuting: false,
                status: isError ? 'error' as const : 'success' as const,
                result: content,
                duration,
                diffSummary,
              };
            });

          return {
            ...message,
            toolResults: [
              ...(message.toolResults || []),
              buildToolResultMessage(toolCallId, content, isError, duration, diffSummary),
            ],
            outputItems: updatedItems,
          };
        }),
      };
    }),
  };
}
