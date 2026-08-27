import { describe, expect, it } from 'vitest';
import type { SubagentActivity } from '../../types';
import { groupSubagentActivitiesByDelegate } from './subagentActivityGrouping';

const activity = (id: string, task: string, parentToolCallId?: string): SubagentActivity => ({
  id,
  parentToolCallId,
  expert: 'office_word_expert',
  label: 'Word 文档专家',
  task,
  status: 'completed',
  outputItems: [],
});

describe('groupSubagentActivitiesByDelegate', () => {
  it('keeps repeated expert runs on their exact delegate cards', () => {
    const grouped = groupSubagentActivitiesByDelegate([
      { id: 'call-1', expert: 'office_word_expert', task: '第一份文档' },
      { id: 'call-2', expert: 'office_word_expert', task: '第二份文档' },
    ], [
      activity('sub-1', '第一份文档', 'call-1'),
      activity('sub-2', '第二份文档', 'call-2'),
    ]);

    expect(grouped.get('call-1')?.map((item) => item.id)).toEqual(['sub-1']);
    expect(grouped.get('call-2')?.map((item) => item.id)).toEqual(['sub-2']);
  });

  it('reconciles legacy unscoped runs by task/order without accumulating them', () => {
    const grouped = groupSubagentActivitiesByDelegate([
      { id: 'call-1', expert: 'office_word_expert', task: '第一份文档' },
      { id: 'call-2', expert: 'office_word_expert', task: '第二份文档' },
    ], [activity('sub-2', '第二份文档'), activity('sub-1', '第一份文档')]);

    expect(grouped.get('call-1')?.map((item) => item.id)).toEqual(['sub-1']);
    expect(grouped.get('call-2')?.map((item) => item.id)).toEqual(['sub-2']);
  });
});
