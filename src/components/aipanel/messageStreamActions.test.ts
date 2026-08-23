import { describe, expect, it } from 'vitest';
import type { OutputItem } from '../../types';
import { finalizeTerminalOutputItems } from './messageStreamActions';

describe('finalizeTerminalOutputItems', () => {
  it('freezes reasoning and stops a tool spinner on success', () => {
    const items: OutputItem[] = [
      { type: 'reasoning', content: 'thinking', startedAt: 1_000 },
      {
        type: 'tool_call_start',
        toolCallId: 'tool-1',
        toolName: 'read_file',
        arguments: {},
        isExecuting: true,
        startedAt: 1_500,
      },
    ];
    const next = finalizeTerminalOutputItems(items, 'success', 3_000);
    expect(next[0]).toMatchObject({ completed: true, durationMs: 2_000 });
    expect(next[1]).toMatchObject({
      isExecuting: false,
      status: 'success',
      duration: 1_500,
    });
  });

  it('marks a missing tool result as terminal on error', () => {
    const next = finalizeTerminalOutputItems([{
      type: 'tool_call_start',
      toolCallId: 'tool-1',
      toolName: 'write_file',
      arguments: {},
      isExecuting: true,
    }], 'error', 3_000);
    expect(next[0]).toMatchObject({
      isExecuting: false,
      status: 'error',
      result: '任务在工具返回前结束',
    });
  });

  it('keeps already-terminal output objects stable', () => {
    const item: OutputItem = {
      type: 'tool_call_start',
      toolCallId: 'tool-1',
      toolName: 'read_file',
      arguments: {},
      isExecuting: false,
      status: 'success',
    };
    const input = [item];
    expect(finalizeTerminalOutputItems(input, 'success', 3_000)).toBe(input);
  });
});
