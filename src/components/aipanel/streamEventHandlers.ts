import type { MutableRefObject } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { useAIPanelStore, useEditorStore, useSidebarStore } from '../../store';
import type { ChatMode, CurrentDiff } from '../../store';
import { normalizeSearchResults } from './messageTransform';
import { TOOL_CALL_CLEAR_DELAY_MS, type StreamPayload } from './streamTypes';
import {
  applyMessageSearchResults,
  applyStreamingError,
  finalizeStreamingMessage,
} from './messageStreamActions';
import { applyToolResultToState } from './toolCallStreamActions';

interface HandleStreamDoneArgs {
  payload: StreamPayload;
  currentMode: ChatMode;
  clearToolCalls: (sessionId: string) => void;
  flushAllPending: () => void;
  streamingContentRef: MutableRefObject<Record<string, string>>;
  setMessageDiff: (sessionId: string, messageId: string, diff: CurrentDiff | null) => void;
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
  flushAllPending,
  streamingContentRef,
  setMessageDiff,
}: HandleStreamDoneArgs) {
  const { session_id, message_id, final_content, summary, search_results } = payload;
  const normalizedSearchResults = normalizeSearchResults(search_results);

  flushAllPending();
  const effectiveContent = final_content || streamingContentRef.current[message_id] || '';

  delete streamingContentRef.current[message_id];

  setTimeout(() => clearToolCalls(session_id), TOOL_CALL_CLEAR_DELAY_MS);

  if (normalizedSearchResults) {
    useAIPanelStore.setState((state) =>
      applyMessageSearchResults(state, session_id, message_id, normalizedSearchResults)
    );
  }

  if (effectiveContent) {
    useAIPanelStore.setState((state) =>
      finalizeStreamingMessage(state, session_id, message_id, effectiveContent)
    );
  } else {
    useAIPanelStore.getState().updateSession(session_id, (session) => ({ ...session, isStreaming: false }));
  }

  if (effectiveContent && currentMode === 'agent') {
    try {
      const diff = await invoke<{ hunks?: CurrentDiff['hunks'] }>('compute_diff', {
        oldText: effectiveContent,
        newText: effectiveContent,
      });
      setMessageDiff(session_id, message_id, {
        originalText: effectiveContent,
        newText: effectiveContent,
        hunks: diff?.hunks ?? [],
        summary: summary ?? 'AI 已修改内容',
      });
    } catch {
      // ignore diff failure
    }
  }
}

export function handleToolResult(payload: StreamPayload) {
  const { session_id, message_id, tool_call_id, content, error, diff_summary, office_file_modified } = payload;
  if (!tool_call_id) return;

  const toolCall = useAIPanelStore.getState().sessions
    .find((session) => session.id === session_id)?.activeToolCalls.find((entry) => entry.id === tool_call_id);
  const duration = toolCall?.startTime ? Date.now() - toolCall.startTime : undefined;

  useAIPanelStore.setState((state) =>
    applyToolResultToState({
      state,
      sessionId: session_id,
      messageId: message_id,
      toolCallId: tool_call_id,
      content: content || '',
      error: error ?? undefined,
      diffSummary: diff_summary ?? undefined,
      duration,
    })
  );

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
  useAIPanelStore.setState((state) =>
    applyStreamingError(state, session_id, message_id, error ?? '发生错误')
  );
  if (currentMode === 'knowledge') {
    useAIPanelStore.setState((state) =>
      applyMessageSearchResults(state, session_id, message_id, [])
    );
  }
}
