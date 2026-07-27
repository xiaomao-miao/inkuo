// Drives a single floating AI popover's stream.
//
// The hook is intentionally narrow: it owns one Tauri `ai://stream`
// subscription keyed by the popover's id, and routes deltas into the
// `floatingAiStore`. The hook also exposes the imperative `cancel`
// action so the popover component can wire it to a Stop button.
//
// We deliberately skip the full `useAgentStream` /
// `useTextStreaming` / `useReasoningStreaming` machinery used by the
// AI panel: the panel's streaming hooks were built around persistent
// chat sessions, plan items, tool calls, and baseline snapshots that
// don't apply to a single-shot "explain this passage" popover.
//
// More importantly, we deliberately skip `ai_agent_stream` on the
// Rust side. `ai_agent_stream` runs the agent loop — every LLM
// response counts as one iteration, and even a successful
// tool-less ask-mode response trips the "Max iterations (1)
// reached" guard. For popovers we want a true one-shot chat
// completion, so we route through the new `ai_ask_stream` Rust
// command (which calls `AIProviderAdapter::chat_stream` directly,
// no agent loop, no iteration counting).

import { invoke } from '@tauri-apps/api/core';
import { listen, UnlistenFn } from '@tauri-apps/api/event';
import { useCallback, useEffect, useRef } from 'react';

import { useFloatingAiStore } from '../../store';
import { isTauriRuntime } from '../../utils/tauri';
import type { StreamPayload } from '../aipanel/streamTypes';
import { extractErrorMessage } from '../../utils/errors';

/**
 * Configuration for a floating AI popover stream. Each popover mints
 * a unique `id` (the Tauri session id) and the matching store entry is
 * updated as deltas arrive.
 */
export interface FloatingAiRequest {
  /** Tauri session id, equal to the popover store id. */
  id: string;
  /** Human prompt sent to the model. */
  instruction: string;
}

interface UseFloatingAiStreamArgs {
  /** When non-null, fire the request on mount. */
  request: FloatingAiRequest | null;
}

export function useFloatingAiStream({ request }: UseFloatingAiStreamArgs) {
  const setStatus = useFloatingAiStore((s) => s.setStatus);
  const appendDelta = useFloatingAiStore((s) => s.appendDelta);
  const finish = useFloatingAiStore((s) => s.finish);

  // Hold the latest `request` in a ref so the cancel handler — bound
  // once — can still see the current id without re-binding on every
  // render.
  const requestRef = useRef<FloatingAiRequest | null>(request);
  useEffect(() => {
    requestRef.current = request;
  }, [request]);

  // The streamer should depend on the *stable* primitives of `request`
  // (id + instruction text), not the object identity. Without this,
  // every store update creates a fresh `request` object → the
  // subscribe-effect below re-runs → it calls `setStatus('streaming')`
  // → the store changes → the parent re-renders → yet another fresh
  // `request` → infinite loop ("Maximum update depth exceeded").
  const requestId = request?.id ?? null;
  const requestInstruction = request?.instruction ?? '';

  /**
   * Fire the single-shot ask stream. Called once per `requestId` /
   * `requestInstruction` change. The Rust side resolves the active
   * AI config (cloud or local) internally; we only need to ship the
   * prompt and the session id.
   */
  useEffect(() => {
    if (!requestId) return undefined;
    if (!isTauriRuntime()) return undefined;

    const id = requestId;
    const instruction = requestInstruction.trim();
    if (!instruction) {
      setStatus(id, 'error', '空的提示');
      return undefined;
    }

    let disposed = false;
    let unlisten: UnlistenFn | null = null;
    setStatus(id, 'streaming');

    (async () => {
      // Subscribe BEFORE invoking so we don't miss the first delta.
      unlisten = await listen<StreamPayload>('ai://stream', (event) => {
        const payload = event.payload;
        if (!payload || payload.session_id !== id) return;
        switch (payload.event_type) {
          case 'text':
            if (payload.content) {
              appendDelta(id, payload.content);
            }
            break;
          case 'error':
            setStatus(id, 'error', payload.error ?? '未知错误');
            break;
          case 'done':
            // The ask stream's `done` event carries the final
            // accumulated content. Replace the streamed concatenation
            // with it so any server-side trimming (e.g. trailing
            // whitespace) is reflected in the final render.
            finish(id, payload.final_content ?? '');
            break;
          case 'reasoning':
          case 'tool_call_start':
          case 'tool_call_args_delta':
          case 'tool_result':
          case 'subagent_start':
          case 'subagent_end':
            // Popovers don't surface reasoning, tool calls, or
            // sub-agent blocks — they're single-shot explanations.
            // Ignore silently.
            break;
          default: {
            const unknown = payload.event_type as string;
            // Don't trip type-level exhaustive checks if Rust adds
            // a new event type in the future — just log it.
            if (typeof unknown === 'string') {
              console.debug('[floating-ai] unhandled event:', unknown);
            }
          }
        }
      });

      if (disposed) {
        unlisten?.();
        unlisten = null;
        return;
      }

      try {
        await invoke('ai_ask_stream', {
          sessionId: id,
          // Popovers don't use message-id routing the way the panel
          // does (no chat history to thread under), but the Rust
          // command requires a non-empty string.
          messageId: id,
          instruction,
        });
        // Errors are surfaced via the `ai://stream` `error` event;
        // we still catch unhandled rejections defensively so the UI
        // doesn't get stuck on "streaming" if the IPC itself fails.
      } catch (err) {
        if (!disposed) {
          setStatus(id, 'error', extractErrorMessage(err));
        }
      }
    })();

    return () => {
      disposed = true;
      unlisten?.();
      unlisten = null;
    };
    // Depend on the stable primitives only. Picking the wrong deps
    // (e.g. `request`, `setStatus`) makes this effect re-run on
    // every store update and triggers an infinite render loop —
    // see `requestId` / `requestInstruction` defined above.
  }, [requestId, requestInstruction, setStatus, appendDelta, finish]);

  const cancel = useCallback(async () => {
    const id = requestRef.current?.id;
    if (!id) return;
    try {
      await invoke('ai_ask_cancel', { sessionId: id });
    } catch {
      // Already finished or unknown id — the store stays in
      // `streaming` if the cancel IPC fails, which is the right
      // user-visible state (the dot still animates).
    }
    setStatus(id, 'cancelled');
  }, [setStatus]);

  return { cancel };
}
