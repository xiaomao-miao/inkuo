import { afterEach, describe, expect, it, vi } from 'vitest';
import type { MutableRefObject } from 'react';
import { dispatchStreamEvent, resetStreamDispatcherState } from './streamEventDispatcher';
import type { StreamPayload } from './streamTypes';

afterEach(() => {
  resetStreamDispatcherState();
});

describe('sub-agent stream ordering', () => {
  it('flushes reasoning before later text instead of reordering separate buffers', async () => {
    const appendOutputDeltaToSubagentActivity = vi.fn();
    const completeSubagentActivity = vi.fn();
    const addSubagentActivity = vi.fn();
    const common = {
      currentMode: 'agent' as const,
      clearToolCalls: vi.fn(),
      flushAllPending: vi.fn(),
      streamingContentRef: { current: {} } as MutableRefObject<Record<string, string>>,
      appendTextDelta: vi.fn(),
      appendReasoningDelta: vi.fn(),
      handleToolCallStart: vi.fn(),
      handleToolCallArgsDelta: vi.fn(),
      setPendingDiff: vi.fn(),
      addSubagentActivity,
      addOutputToSubagentActivity: vi.fn(),
      appendOutputDeltaToSubagentActivity,
      completeSubagentActivity,
    };
    const send = (payload: StreamPayload) => dispatchStreamEvent({ ...common, payload });

    await send({
      session_id: 'session',
      message_id: 'parent',
      event_type: 'subagent_start',
      tool_call_id: 'delegate-call-1',
      content: 'task',
      summary: 'researcher',
      tool_args: 'Researcher',
      final_content: 'sub:researcher:1',
      done: false,
    });
    expect(addSubagentActivity).toHaveBeenCalledWith(
      'session',
      'parent',
      expect.objectContaining({
        id: 'sub:researcher:1',
        parentToolCallId: 'delegate-call-1',
      }),
    );
    await send({
      session_id: 'session',
      message_id: 'sub:researcher:1',
      event_type: 'reasoning',
      content: 'reason first',
      done: false,
    });
    await send({
      session_id: 'session',
      message_id: 'sub:researcher:1',
      event_type: 'text',
      content: 'answer second',
      done: false,
    });
    await send({
      session_id: 'session',
      message_id: 'sub:researcher:1',
      event_type: 'done',
      final_content: 'complete',
      done: true,
    });

    expect(appendOutputDeltaToSubagentActivity.mock.calls.map((call) => call[3])).toEqual([
      { type: 'reasoning', content: 'reason first' },
      { type: 'text', content: 'answer second' },
    ]);
    expect(completeSubagentActivity).toHaveBeenCalledWith(
      'session',
      'parent',
      'sub:researcher:1',
      'completed',
      'complete',
    );
  });
});
