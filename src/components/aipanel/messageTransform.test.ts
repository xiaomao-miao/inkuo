import { describe, expect, it } from 'vitest';

import type { ChatMessage } from '../../types';
import { buildConversationHistoryBefore } from './messageTransform';

const makeUser = (id: string, content: string): ChatMessage =>
  ({ id, role: 'user', content, timestamp: 0, outputItems: [] } as ChatMessage);

const makeAssistant = (id: string, content: string): ChatMessage =>
  ({ id, role: 'assistant', content, timestamp: 0, outputItems: [] } as ChatMessage);

const makeAssistantWithToolCalls = (id: string): ChatMessage =>
  ({
    id,
    role: 'assistant',
    content: '',
    timestamp: 0,
    outputItems: [],
    toolCalls: [
      { id: 'call-1', name: 'read_file', arguments: { path: 'a.txt' } },
    ],
  } as ChatMessage);

const makeToolResult = (id: string, toolCallId: string, content: string): ChatMessage =>
  ({ id, role: 'tool', toolCallId, content, timestamp: 0, outputItems: [] } as ChatMessage);

const makeOrphanTool = (id: string, toolCallId: string): ChatMessage =>
  ({ id, role: 'tool', toolCallId, content: 'orphan', timestamp: 0, outputItems: [] } as ChatMessage);

describe('buildConversationHistoryBefore', () => {
  it('returns undefined when the target message id is not present', () => {
    const messages = [makeUser('u1', 'hi'), makeAssistant('a1', 'hello')];
    expect(buildConversationHistoryBefore(messages, 'missing')).toBeUndefined();
  });

  it('returns an empty array when the target is the first message', () => {
    const messages = [makeUser('u1', 'hi'), makeAssistant('a1', 'hello')];
    expect(buildConversationHistoryBefore(messages, 'u1')).toEqual([]);
  });

  it('excludes the target message itself and everything after it', () => {
    const messages = [
      makeUser('u1', 'first'),
      makeAssistant('a1', 'first reply'),
      makeUser('u2', 'second'),
      makeAssistant('a2', 'second reply'),
    ];
    const history = buildConversationHistoryBefore(messages, 'u2') ?? [];
    expect(history.map((m) => m.id)).toEqual(['u1', 'a1']);
  });

  it('excludes the just-appended normal-send user turn and assistant placeholder', () => {
    const messages = [
      makeUser('u1', 'first'),
      makeAssistant('a1', 'first reply'),
      makeUser('current-user', 'new instruction'),
      { id: 'placeholder', role: 'assistant', timestamp: 0, outputItems: [] } as ChatMessage,
    ];
    const history = buildConversationHistoryBefore(messages, 'current-user') ?? [];
    expect(history.map((message) => message.id)).toEqual(['u1', 'a1']);
    expect(history.some((message) => message.content === 'new instruction')).toBe(false);
  });

  it('preserves image attachments on historical user messages', () => {
    const first = makeUser('u1', 'inspect this');
    first.imageAttachments = [{
      path: '/workspace/page.png',
      detail: 'high',
      name: 'page.png',
    }];
    const messages = [first, makeAssistant('a1', 'looks good'), makeUser('u2', 'continue')];
    const history = buildConversationHistoryBefore(messages, 'u2') ?? [];

    expect(history[0].imageAttachments).toEqual(first.imageAttachments);
  });

  it('preserves complete tool-call / tool-result pairs from earlier turns', () => {
    const messages = [
      makeUser('u1', 'read a.txt'),
      makeAssistantWithToolCalls('a1'),
      makeToolResult('t1', 'call-1', 'contents'),
      makeUser('u2', 'thanks'),
    ];
    const history = buildConversationHistoryBefore(messages, 'u2') ?? [];
    expect(history.map((m) => ({ id: m.id, role: m.role, tool_call_id: m.tool_call_id }))).toEqual([
      { id: 'u1', role: 'user', tool_call_id: undefined },
      { id: 'a1', role: 'assistant', tool_call_id: undefined },
      { id: 't1', role: 'tool', tool_call_id: 'call-1' },
    ]);
  });

  it('drops an earlier assistant message whose tool responses are missing', () => {
    // The first assistant has no tool result in the visible history;
    // the orphan tool message is ignored by sanitize, so the assistant
    // must be removed entirely to keep the model from seeing a broken
    // tool_calls pair.
    const messages = [
      makeUser('u1', 'incomplete'),
      makeAssistantWithToolCalls('a1'),
      makeOrphanTool('orphan', 'call-404'),
      makeUser('u2', 'next'),
    ];
    const history = buildConversationHistoryBefore(messages, 'u2') ?? [];
    expect(history.map((m) => m.id)).toEqual(['u1']);
  });

  it('matches the new text content when the user edits the question', () => {
    // Re-asking the same user message with a different content should
    // NOT include the old assistant reply under the same id, because the
    // helper slices strictly before the target index.
    const messages = [
      makeUser('u1', 'original question'),
      makeAssistant('a1', 'original answer'),
    ];
    const history = buildConversationHistoryBefore(messages, 'u1');
    expect(history).toEqual([]);
  });
});
