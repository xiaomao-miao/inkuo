import { describe, expect, it } from 'vitest';
import type { OutputItem } from '../../types';
import {
  buildMinimalActivities,
  shouldRenderOutputItemInMinimal,
} from './minimalActivity';

describe('minimal AI output', () => {
  it('keeps user questions visible while folding verbose tool/reasoning details', () => {
    const text: OutputItem = { type: 'text', content: '完成了' };
    const ask: OutputItem = {
      type: 'tool_call_start',
      toolCallId: 'ask-1',
      toolName: 'ask_user',
      arguments: {},
      isExecuting: false,
      interactionState: 'pending',
    };
    const regularTool: OutputItem = {
      type: 'tool_call_start',
      toolCallId: 'read-1',
      toolName: 'read_file',
      arguments: { path: '/workspace/spec.md' },
      isExecuting: true,
    };
    const reasoning: OutputItem = { type: 'reasoning', content: '分析中' };

    expect(shouldRenderOutputItemInMinimal(text)).toBe(true);
    expect(shouldRenderOutputItemInMinimal(ask)).toBe(true);
    expect(shouldRenderOutputItemInMinimal(regularTool)).toBe(false);
    expect(shouldRenderOutputItemInMinimal(reasoning)).toBe(false);
  });

  it('summarises real current work instead of returning a generic placeholder', () => {
    const activities = buildMinimalActivities([
      {
        type: 'reasoning',
        content: '先检查现有结构\n正在确认文档和图片工具',
        reasoningId: 'r1',
        completed: true,
      },
      {
        type: 'tool_call_start',
        toolCallId: 'delegate-1',
        toolName: 'delegate_to',
        arguments: { expert: 'office_word_expert', task: '制作带配图的产品报告' },
        isExecuting: true,
      },
      {
        type: 'tool_call_start',
        toolCallId: 'write-1',
        toolName: 'write_file',
        arguments: { path: '/workspace/report.md' },
        isExecuting: false,
        status: 'success',
      },
    ]);

    expect(activities).toEqual([
      expect.objectContaining({
        label: '分析完成',
        detail: '正在确认文档和图片工具',
        status: 'success',
      }),
      expect.objectContaining({
        label: '委派给 Word 文档专家',
        detail: '制作带配图的产品报告',
        status: 'working',
      }),
      expect.objectContaining({ label: '写入文件', status: 'success' }),
    ]);
  });
});
