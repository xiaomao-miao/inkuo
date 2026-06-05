import type { MutableRefObject } from 'react';
import { invoke } from '@tauri-apps/api/core';
import {
  useAIPanelStore,
  useEditorStore,
  useSidebarStore,
  type ChatMode,
  type CurrentDiff,
} from '../../store';
import { normalizeSearchResults } from './messageTransform';
import { TOOL_CALL_CLEAR_DELAY_MS, type StreamPayload } from './streamTypes';

interface HandleStreamDoneArgs {
  payload: StreamPayload;
  currentMode: ChatMode;
  clearToolCalls: (sessionId: string) => void;
  setMessageDiff: (sessionId: string, messageId: string, diff: CurrentDiff | null) => void;
  flushAllPending: () => void;
  streamingContentRef: MutableRefObject<Record<string, string>>;
}

interface HandleStreamErrorArgs {
  payload: StreamPayload;
  currentMode: ChatMode;
  flushAllPending: () => void;
  streamingContentRef: MutableRefObject<Record<string, string>>;
}

export async function handleStreamDone({
  payload,
  currentMode,
  clearToolCalls,
  setMessageDiff,
  flushAllPending,
  streamingContentRef,
}: HandleStreamDoneArgs) {
  const { session_id, message_id, final_content, summary, search_results } = payload;
  const normalizedSearchResults = normalizeSearchResults(search_results);

  flushAllPending();
  const effectiveContent = final_content || streamingContentRef.current[message_id] || '';

  delete streamingContentRef.current[message_id];

  setTimeout(() => clearToolCalls(session_id), TOOL_CALL_CLEAR_DELAY_MS);

  if (normalizedSearchResults) {
    useAIPanelStore.getState().setMessageSearchResults(session_id, message_id, normalizedSearchResults);
  }

  if (effectiveContent) {
    useAIPanelStore.getState().finishMessageStreaming(session_id, message_id, effectiveContent);
  } else {
    useAIPanelStore.getState().updateSession(session_id, (session) => ({ ...session, isStreaming: false }));
  }

  if (effectiveContent && currentMode === 'agent') {
    try {
      const selection = useEditorStore.getState().getSelection?.();
      if (selection && effectiveContent !== selection) {
        const originalText = selection;
        const diff = await invoke<{ hunks?: CurrentDiff['hunks'] }>('compute_diff', {
          oldText: originalText,
          newText: effectiveContent,
        });
        setMessageDiff(session_id, message_id, {
          originalText,
          newText: effectiveContent,
          hunks: diff?.hunks ?? [],
          summary: summary ?? 'AI 已修改内容',
        });
      }
    } catch {
      // ignore diff failure
    }
  }
}

export function handleToolResult(payload: StreamPayload) {
  const { session_id, message_id, tool_call_id, content, error, diff_summary, office_file_modified } = payload;
  if (!tool_call_id) return;

  const isError = !!error;
  const toolCall = useAIPanelStore.getState().sessions
    .find((session) => session.id === session_id)?.activeToolCalls.find((entry) => entry.id === tool_call_id);
  const duration = toolCall?.startTime ? Date.now() - toolCall.startTime : undefined;

  const toolResult = {
    toolCallId: tool_call_id,
    result: content || '',
    isError,
    duration,
    diffSummary: diff_summary ?? undefined,
  };

  useAIPanelStore.getState().updateSession(session_id, (session) => ({
    ...session,
    activeToolCalls: session.activeToolCalls.map((entry) =>
      entry.id === tool_call_id
        ? { ...entry, status: isError ? 'error' : 'success', result: content, error: isError ? error : undefined, duration }
        : entry
    ),
    messages: session.messages.map((message) => {
      if (message.id !== message_id) return message;
      const updatedItems = message.outputItems
        .filter((item) => !(item.type === 'tool_result' && (item as { toolCallId?: string }).toolCallId === tool_call_id))
        .map((item) => {
          if (item.type !== 'tool_call_start' || (item as { toolCallId?: string }).toolCallId !== tool_call_id) return item;
          return {
            ...item,
            isExecuting: false,
            status: isError ? 'error' as const : 'success' as const,
            result: content || '',
            duration,
            diffSummary: diff_summary ?? undefined,
          };
        });
      return {
        ...message,
        toolResults: [...(message.toolResults || []), toolResult],
        outputItems: updatedItems,
      };
    }),
  }));

  useAIPanelStore.getState().addMessage(session_id, {
    id: `tool-${tool_call_id}-${crypto.randomUUID()}`,
    role: 'tool',
    content: content ?? error ?? '',
    toolCallId: tool_call_id,
    timestamp: Date.now(),
    outputItems: [],
  });

  if (office_file_modified) {
    const { path } = office_file_modified;
    const { invalidateOfficeBuffer } = useEditorStore.getState();
    const { setOpenTabDirty } = useSidebarStore.getState();
    invalidateOfficeBuffer(path);
    setOpenTabDirty(path, false);
  }
}

export function handleStreamError({
  payload,
  currentMode,
  flushAllPending,
  streamingContentRef,
}: HandleStreamErrorArgs) {
  const { session_id, message_id, error } = payload;

  flushAllPending();
  delete streamingContentRef.current[message_id];
  useAIPanelStore.getState().setErrorMessage(session_id, message_id, error ?? '发生错误');
  if (currentMode === 'knowledge') {
    useAIPanelStore.getState().setMessageSearchResults(session_id, message_id, []);
  }
}
