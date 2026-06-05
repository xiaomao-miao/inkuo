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

      const alreadyExists = session.activeToolCalls.some((toolCall) => toolCall.id === toolCallId);
      const updatedActiveToolCalls = alreadyExists
        ? session.activeToolCalls.map((toolCall) =>
            toolCall.id === toolCallId
              ? { ...toolCall, name: toolName, arguments: parsedArgs, status: 'executing' as const }
              : toolCall
          )
        : [...session.activeToolCalls, buildToolCallEntry(toolCallId, toolName, parsedArgs, startTime)];

      return {
        ...session,
        activeToolCalls: updatedActiveToolCalls,
        messages: session.messages.map((message) => {
          if (message.id !== messageId) return message;

          const existingIdx = message.outputItems.findIndex(
            (item) => item.type === 'tool_call_start' && item.toolCallId === toolCallId
          );

          if (existingIdx >= 0) {
            const updated = [...message.outputItems];
            const previous = updated[existingIdx] as Extract<OutputItem, { type: 'tool_call_start' }>;
            updated[existingIdx] = {
              ...previous,
              toolName,
              arguments: parsedArgs,
              rawArguments: rawArgs,
              streamingContent,
              isExecuting: true,
            };
            return { ...message, outputItems: updated };
          }

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

      return {
        ...session,
        messages: session.messages.map((message) => {
          let mutated = false;
          const updatedItems = message.outputItems.map((item) => {
            if (item.type !== 'tool_call_start') return item;

            const entry = pendingEntries.find(
              (candidate) => candidate.messageId === message.id && item.toolCallId in { [item.toolCallId]: true } && candidate.sessionId === session.id
            );
            if (!entry || item.toolCallId === undefined) return item;

            mutated = true;
            return {
              ...item,
              arguments: entry.parsedArgs,
              rawArguments: entry.rawArgs,
              streamingContent: entry.streamingContent,
              isExecuting: true,
            };
          });

          return mutated ? { ...message, outputItems: updatedItems } : message;
        }),
      };
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

      return {
        ...session,
        activeToolCalls: session.activeToolCalls.map((entry) =>
          entry.id === toolCallId
            ? {
                ...entry,
                status: isError ? 'error' : 'success',
                result: content,
                error: isError ? error : undefined,
                duration,
              }
            : entry
        ),
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
