import { describe, expect, it } from 'vitest';

import type { ActiveToolCall, ChatSession } from '../../types';
import { DEFAULT_CHAT_MODE } from '../../constants/chatModes';
import {
  appendSessionToolCall,
  clearSessionToolCalls,
  removeSessionToolCall,
  updateToolCalls,
} from './toolCallReducer';

const makeSession = (overrides: Partial<ChatSession> = {}): ChatSession => ({
  id: 'sess-1',
  title: 'test',
  createdAt: 0,
  lastActivityAt: 0,
  mode: DEFAULT_CHAT_MODE,
  featureToggles: {},
  messages: [],
  isStreaming: false,
  currentDiff: null,
  activeToolCalls: [],
  pendingDiff: null,
  ...overrides,
});

const makeToolCall = (overrides: Partial<ActiveToolCall> = {}): ActiveToolCall => ({
  id: 'tc-1',
  ...overrides,
} as ActiveToolCall);

describe('aiPanelReducers/toolCallReducer', () => {
  describe('appendSessionToolCall', () => {
    it('adds the tool call to the targeted session', () => {
      const a = makeSession({ id: 'a' });
      const b = makeSession({ id: 'b' });
      const next = appendSessionToolCall([a, b], 'a', makeToolCall({ id: 'tc-1' }));
      expect(next[0].activeToolCalls.map((tc) => tc.id)).toEqual(['tc-1']);
      expect(next[1].activeToolCalls).toEqual([]);
    });
  });

  describe('removeSessionToolCall', () => {
    it('removes only the matching tool call from the targeted session', () => {
      const session = makeSession({
        id: 's',
        activeToolCalls: [
          makeToolCall({ id: 'tc-1' }),
          makeToolCall({ id: 'tc-2' }),
        ],
      });
      const next = removeSessionToolCall([session], 's', 'tc-1');
      expect(next[0].activeToolCalls.map((tc) => tc.id)).toEqual(['tc-2']);
    });

    it('returns a session with the original tool calls when no match is found', () => {
      // removeSessionToolCall uses updateSessions internally, which always
      // rebuilds the array via `.map()`. When the target id doesn't match,
      // the updater is never invoked, so the rebuilt session's tool calls
      // match the input by structure even though the array is a new one.
      const session = makeSession({
        id: 's',
        activeToolCalls: [makeToolCall({ id: 'tc-1' })],
      });
      const next = removeSessionToolCall([session], 's', 'nope');
      expect(next[0].activeToolCalls.map((tc) => tc.id)).toEqual(['tc-1']);
    });
  });

  describe('clearSessionToolCalls', () => {
    it('empties the targeted session only', () => {
      const a = makeSession({
        id: 'a',
        activeToolCalls: [makeToolCall({ id: 'tc-1' })],
      });
      const b = makeSession({
        id: 'b',
        activeToolCalls: [makeToolCall({ id: 'tc-2' })],
      });
      const next = clearSessionToolCalls([a, b], 'a');
      expect(next[0].activeToolCalls).toEqual([]);
      expect(next[1].activeToolCalls.map((tc) => tc.id)).toEqual(['tc-2']);
    });
  });

  describe('updateToolCalls', () => {
    it('applies the updater only to the matching tool call', () => {
      const session = makeSession({
        activeToolCalls: [makeToolCall({ id: 'a' }), makeToolCall({ id: 'b' })],
      });
      const next = updateToolCalls(session, 'a', (tc) => ({
        ...tc,
        status: 'success',
      } as unknown as ActiveToolCall));
      expect(next.activeToolCalls[0].status).toBe('success');
      expect(next.activeToolCalls[1].status).toBeUndefined();
    });

    it('returns the session unchanged when the target is missing', () => {
      const session = makeSession({
        activeToolCalls: [makeToolCall({ id: 'a' })],
      });
      const next = updateToolCalls(session, 'nope', (tc) => ({
        ...tc,
        status: 'success',
      } as unknown as ActiveToolCall));
      // map() creates a new array but no element should be mutated.
      expect(next.activeToolCalls[0].status).toBeUndefined();
    });
  });
});
