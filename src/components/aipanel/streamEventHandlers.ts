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
import { syncTodoSnapshotFromToolCall } from './todoSync';

interface HandleStreamDoneArgs {
  payload: StreamPayload;
  currentMode: ChatMode;
  clearToolCalls: (sessionId: string) => void;
  flushAllPending: () => void;
  streamingContentRef: MutableRefObject<Record<string, string>>;
  setPendingDiff: (sessionId: string, diff: CurrentDiff | null) => void;
}

interface HandleStreamErrorArgs {
  payload: StreamPayload;
  flushAllPending: () => void;
  streamingContentRef: MutableRefObject<Record<string, string>>;
}

export async function handleStreamDone({
  payload,
  currentMode,
  clearToolCalls,
  flushAllPending,
  streamingContentRef,
  setPendingDiff,
}: HandleStreamDoneArgs) {
  const { session_id, message_id, final_content, summary, search_results, original_content, new_content, file_path } = payload;
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
    // If the message has a plan OutputItem, the structured card is the
    // canonical rendering — don't overwrite its rawText with the model's
    // (now stale) final_content. Just flip isStreaming off on the plan
    // item and finalise the session state without touching message.content.
    const messageHasPlan = useAIPanelStore.getState().sessions
      .find((s) => s.id === session_id)
      ?.messages.find((m) => m.id === message_id)
      ?.outputItems.some((it) => it.type === 'plan');
    if (messageHasPlan) {
      useAIPanelStore.getState().finishPlanItem(session_id, message_id);
      useAIPanelStore.getState().updateSession(session_id, (session) => ({ ...session, isStreaming: false }));
    } else {
      useAIPanelStore.setState((state) =>
        finalizeStreamingMessage(state, session_id, message_id, effectiveContent)
      );
    }
  } else {
    useAIPanelStore.getState().updateSession(session_id, (session) => ({ ...session, isStreaming: false }));
  }

  if (currentMode === 'agent' && original_content && new_content) {
    try {
      const diff = await invoke<{ hunks?: CurrentDiff['hunks'] }>('compute_diff', {
        oldText: original_content,
        newText: new_content,
      });
      setPendingDiff(session_id, {
        originalText: original_content,
        newText: new_content,
        hunks: diff?.hunks ?? [],
        summary: summary ?? 'AI 已修改内容',
        filePath: file_path,
      });
    } catch {
      // ignore diff failure
    }
  }
}

export function handleToolResult(
  payload: StreamPayload,
  flushAllPending: () => void,
) {
  const { session_id, message_id, tool_call_id, content, error, diff_summary, office_file_modified } = payload;
  if (!tool_call_id) return;

  // Flush buffered args so the outputItem has complete arguments before
  // being marked as done. This also clears the pending state so a later
  // flush timer fire won't overwrite the completed item.
  flushAllPending();

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

  // `update_todo` is a special case: its tool_call_start OutputItem
  // holds the published todo list in `arguments.items`. After the
  // tool result lands we copy that list into the AIPanelStore's
  // todoSnapshotBySession map so the TodoPanel can render it without
  // having to scan the message history. Done here (rather than in the
  // tool_call_start handler) so we only commit a *complete* snapshot
  // — partial JSON parse failures during streaming would otherwise
  // surface a half-list to the user.
  syncTodoSnapshotFromToolCall(session_id, message_id, tool_call_id);

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
  flushAllPending,
  streamingContentRef,
}: HandleStreamErrorArgs) {
  const { session_id, message_id, error } = payload;

  flushAllPending();
  delete streamingContentRef.current[message_id];
  // If the message has a plan item, mark it as no longer streaming so the
  // user sees a static (not spinner) plan card. applyStreamingError will
  // then write the error string as the message's `content` — the plan
  // item itself stays intact in `outputItems` so the user can still see
  // what was parsed.
  const messageHasPlan = useAIPanelStore.getState().sessions
    .find((s) => s.id === session_id)
    ?.messages.find((m) => m.id === message_id)
    ?.outputItems.some((it) => it.type === 'plan');
  if (messageHasPlan) {
    useAIPanelStore.getState().finishPlanItem(session_id, message_id);
  }
  useAIPanelStore.setState((state) =>
    applyStreamingError(state, session_id, message_id, error ?? '发生错误')
  );
}
