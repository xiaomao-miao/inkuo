import { parse as parsePartialJson } from 'jsonchunk';
import { useCallback, useRef } from 'react';
import {
  useAIPanelStore,
  type ChatMessage,
  type OutputItem,
} from '../../store';
import type { StreamPayload } from './streamTypes';

interface PendingToolArgEntry {
  sessionId: string;
  messageId: string;
  rawArgs: string;
  parsedArgs: Record<string, unknown>;
  streamingContent?: string;
}

function parseToolArgs(rawArgs: string) {
  const partial = parsePartialJson(rawArgs) as Record<string, unknown> | undefined;
  const streamingContent = (partial?.content || partial?.new_text || partial?.json_content) as string | undefined;

  let parsedArgs: Record<string, unknown> = {};
  try {
    if (rawArgs) parsedArgs = JSON.parse(rawArgs);
  } catch {
    parsedArgs = partial || {};
  }

  return {
    parsedArgs,
    streamingContent: streamingContent ?? undefined,
  };
}

export function useToolCallStreaming() {
  const pendingToolArgsRef = useRef<Record<string, PendingToolArgEntry>>({});
  const pendingToolArgsOrderRef = useRef<string[]>([]);
  const flushToolArgsTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  const flushToolArgs = useCallback(() => {
    const pending = pendingToolArgsRef.current;
    const order = pendingToolArgsOrderRef.current;
    if (order.length === 0) return;

    pendingToolArgsRef.current = {};
    pendingToolArgsOrderRef.current = [];
    flushToolArgsTimeoutRef.current = null;

    useAIPanelStore.setState((state) => {
      const sessionIds = new Set<string>();
      for (const id of order) {
        const entry = pending[id];
        if (entry) sessionIds.add(entry.sessionId);
      }

      return {
        sessions: state.sessions.map((session) => {
          if (!sessionIds.has(session.id)) return session;

          return {
            ...session,
            messages: session.messages.map((message) => {
              let mutated = false;
              const updatedItems = message.outputItems.map((item) => {
                if (item.type !== 'tool_call_start') return item;
                const entry = pending[item.toolCallId];
                if (!entry || entry.messageId !== message.id) return item;
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
    });
  }, []);

  const scheduleToolArgsFlush = useCallback(() => {
    if (flushToolArgsTimeoutRef.current !== null) return;
    flushToolArgsTimeoutRef.current = setTimeout(flushToolArgs, 16);
  }, [flushToolArgs]);

  const handleToolCallStart = useCallback((payload: StreamPayload) => {
    const { session_id, message_id, tool_call_id, tool_name, tool_args } = payload;
    if (!tool_call_id || !tool_name) return;

    const rawArgs = tool_args ?? '';
    const { parsedArgs, streamingContent } = parseToolArgs(rawArgs);

    useAIPanelStore.getState().updateSession(session_id, (session) => {
      const alreadyExists = session.activeToolCalls.some((toolCall) => toolCall.id === tool_call_id);
      const updatedActiveToolCalls = alreadyExists
        ? session.activeToolCalls.map((toolCall) =>
            toolCall.id === tool_call_id
              ? { ...toolCall, name: tool_name, arguments: parsedArgs, status: 'executing' as const }
              : toolCall
          )
        : [
            ...session.activeToolCalls,
            {
              id: tool_call_id,
              name: tool_name,
              arguments: parsedArgs,
              status: 'executing' as const,
              startTime: Date.now(),
            },
          ];

      return {
        ...session,
        activeToolCalls: updatedActiveToolCalls,
        messages: session.messages.map((message) => {
          if (message.id !== message_id) return message;
          const existingIdx = message.outputItems.findIndex(
            (item) => item.type === 'tool_call_start' && item.toolCallId === tool_call_id
          );
          if (existingIdx >= 0) {
            const updated = [...message.outputItems];
            const previous = updated[existingIdx] as Extract<OutputItem, { type: 'tool_call_start' }>;
            updated[existingIdx] = {
              ...previous,
              toolName: tool_name,
              arguments: parsedArgs,
              rawArguments: rawArgs,
              streamingContent,
              isExecuting: true,
            };
            return { ...message, outputItems: updated };
          }

          return {
            ...message,
            toolCalls: [...(message.toolCalls || []), { id: tool_call_id, name: tool_name, arguments: parsedArgs }],
            outputItems: [
              ...message.outputItems,
              {
                type: 'tool_call_start' as const,
                toolCallId: tool_call_id,
                toolName: tool_name,
                arguments: parsedArgs,
                rawArguments: rawArgs,
                streamingContent,
                isExecuting: true,
              },
            ],
          } as ChatMessage;
        }),
      };
    });
  }, []);

  const handleToolCallArgsDelta = useCallback((payload: StreamPayload) => {
    const { session_id, message_id, tool_call_id, tool_args } = payload;
    if (!tool_call_id) return;

    const rawArgs = tool_args ?? '';
    const { parsedArgs, streamingContent } = parseToolArgs(rawArgs);

    const previous = pendingToolArgsRef.current[tool_call_id];
    if (!previous || previous.rawArgs !== rawArgs) {
      pendingToolArgsRef.current[tool_call_id] = {
        sessionId: session_id,
        messageId: message_id,
        rawArgs,
        parsedArgs,
        streamingContent,
      };
      if (!previous) pendingToolArgsOrderRef.current.push(tool_call_id);
    }
    scheduleToolArgsFlush();
  }, [scheduleToolArgsFlush]);

  const resetToolCallStreaming = useCallback(() => {
    if (flushToolArgsTimeoutRef.current !== null) {
      clearTimeout(flushToolArgsTimeoutRef.current);
      flushToolArgsTimeoutRef.current = null;
    }
    pendingToolArgsRef.current = {};
    pendingToolArgsOrderRef.current = [];
  }, []);

  return {
    flushToolArgs,
    handleToolCallStart,
    handleToolCallArgsDelta,
    resetToolCallStreaming,
  };
}
