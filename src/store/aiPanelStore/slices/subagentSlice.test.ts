import { describe, expect, it } from 'vitest';
import type { OutputItem } from '../../../types';
import { applySubagentOutputItem, finalizeSubagentOutputItems } from './subagentSlice';

describe('subagent output terminal reconciliation', () => {
  it('patches the matching tool start instead of leaving its spinner beside a result', () => {
    const start: OutputItem = {
      type: 'tool_call_start',
      toolCallId: 'call-1',
      toolName: 'read_file',
      arguments: {},
      isExecuting: true,
      startedAt: 100,
    };
    const next = applySubagentOutputItem([start], {
      type: 'tool_result',
      toolCallId: 'call-1',
      status: 'success',
      result: 'ok',
    }, 600);
    expect(next).toHaveLength(1);
    expect(next[0]).toMatchObject({
      type: 'tool_call_start',
      isExecuting: false,
      status: 'success',
      result: 'ok',
      duration: 500,
    });
  });

  it('finalizes pending nested reasoning and tools when the subagent ends', () => {
    const next = finalizeSubagentOutputItems([
      { type: 'reasoning', content: 'work', startedAt: 100 },
      {
        type: 'tool_call_start',
        toolCallId: 'call-1',
        toolName: 'write_file',
        arguments: {},
        isExecuting: true,
        startedAt: 200,
      },
    ], 'completed', 1_100);
    expect(next[0]).toMatchObject({ completed: true, durationMs: 1_000 });
    expect(next[1]).toMatchObject({ isExecuting: false, status: 'success', duration: 900 });
  });
});
