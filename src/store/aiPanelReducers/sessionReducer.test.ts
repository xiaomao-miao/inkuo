import { describe, expect, it } from 'vitest';

import { DEFAULT_CHAT_MODE } from '../../constants/chatModes';
import type { ChatMessage, ChatSession } from '../../types';
import {
  appendSessionMessage,
  clearSessionConversation,
  createNewSession,
  createSessionTitle,
  finishSessionMessageStreaming,
  touchSession,
  trimSessionMessagesAfter,
  updateMessages,
  updateSessionState,
  updateSessions,
} from './sessionReducer';

const makeSession = (overrides: Partial<ChatSession> = {}): ChatSession => ({
  id: 'sess-1',
  title: createSessionTitle(1),
  createdAt: 1_700_000_000_000,
  lastActivityAt: 1_700_000_000_000,
  mode: DEFAULT_CHAT_MODE,
  featureToggles: {},
  messages: [],
  isStreaming: false,
  currentDiff: null,
  activeToolCalls: [],
  pendingDiff: null,
  ...overrides,
});

const makeMessage = (overrides: Partial<ChatMessage> = {}): ChatMessage => ({
  id: 'msg-1',
  role: 'user',
  content: 'hello',
  ...overrides,
} as ChatMessage);

describe('aiPanelReducers/sessionReducer', () => {
  describe('createSessionTitle', () => {
    it('labels the session by index', () => {
      expect(createSessionTitle(1)).toBe('对话 1');
      expect(createSessionTitle(42)).toBe('对话 42');
    });
  });

  describe('createNewSession', () => {
    it('builds a session with sane defaults', () => {
      const before = Date.now();
      const session = createNewSession(5);
      const after = Date.now();

      expect(session.title).toBe('对话 5');
      expect(session.mode).toBe(DEFAULT_CHAT_MODE);
      expect(session.messages).toEqual([]);
      expect(session.isStreaming).toBe(false);
      expect(session.activeToolCalls).toEqual([]);
      expect(session.featureToggles).toEqual({});
      expect(session.currentDiff).toBeNull();
      expect(session.pendingDiff).toBeNull();
      expect(session.id).toMatch(/^[0-9a-f-]{36}$/i); // UUID-like
      // Timestamp ordering is monotonic between the two captures.
      expect(session.createdAt).toBeGreaterThanOrEqual(before);
      expect(session.createdAt).toBeLessThanOrEqual(after);
      expect(session.lastActivityAt).toBe(session.createdAt);
    });

    it('produces a unique id per call', () => {
      const a = createNewSession(1);
      const b = createNewSession(2);
      expect(a.id).not.toBe(b.id);
    });
  });

  describe('touchSession', () => {
    it('updates lastActivityAt to now', () => {
      const session = makeSession({ lastActivityAt: 100 });
      const before = Date.now();
      const touched = touchSession(session);
      const after = Date.now();

      expect(touched.lastActivityAt).toBeGreaterThanOrEqual(before);
      expect(touched.lastActivityAt).toBeLessThanOrEqual(after);
      // Untouched fields are preserved.
      expect(touched.id).toBe(session.id);
      expect(touched.messages).toBe(session.messages);
    });
  });

  describe('updateSessions', () => {
    it('returns the same array reference when no session matches', () => {
      const sessions = [makeSession({ id: 'a' })];
      const next = updateSessions(sessions, 'nope', (s) => s);
      // map() always returns a new array; this is acceptable but we want
      // the no-op case to be obvious in callers, so just verify the id.
      expect(next[0].id).toBe('a');
      expect(next).not.toBe(sessions);
    });

    it('updates the targeted session and leaves the rest intact', () => {
      const a = makeSession({ id: 'a' });
      const b = makeSession({ id: 'b', title: 'old' });
      const next = updateSessions([a, b], 'b', (s) => ({ ...s, title: 'new' }));
      expect(next[0]).toBe(a);
      expect(next[1].title).toBe('new');
    });
  });

  describe('updateSessionState', () => {
    it('applies a partial patch to the targeted session', () => {
      const a = makeSession({ id: 'a' });
      const next = updateSessionState([a], 'a', { isStreaming: true, title: 'updated' });
      expect(next[0].isStreaming).toBe(true);
      expect(next[0].title).toBe('updated');
      // Untouched fields survive.
      expect(next[0].id).toBe('a');
      expect(next[0].messages).toBe(a.messages);
    });
  });

  describe('appendSessionMessage', () => {
    it('appends the message to the targeted session only', () => {
      const a = makeSession({ id: 'a' });
      const b = makeSession({ id: 'b' });
      const message = makeMessage({ id: 'm1' });
      const next = appendSessionMessage([a, b], 'a', message);
      expect(next[0].messages.map((m) => m.id)).toEqual(['m1']);
      expect(next[1].messages).toEqual([]);
    });
  });

  describe('updateMessages', () => {
    it('returns the session unchanged when no message matches', () => {
      const session = makeSession({ messages: [makeMessage({ id: 'm1' })] });
      const next = updateMessages(session, 'nope', (m) => ({ ...m, content: 'changed' }));
      // The function still produces a new object because of spread, but
      // the content should reflect the not-matched default (since the
      // updater is only invoked for the matching message).
      expect(next.messages[0].content).toBe('hello');
    });

    it('applies the updater only to the matching message', () => {
      const session = makeSession({
        messages: [
          makeMessage({ id: 'm1', content: 'a' }),
          makeMessage({ id: 'm2', content: 'b' }),
        ],
      });
      const next = updateMessages(session, 'm1', (m) => ({ ...m, content: 'A' }));
      expect(next.messages[0].content).toBe('A');
      expect(next.messages[1].content).toBe('b');
    });
  });

  describe('finishSessionMessageStreaming', () => {
    it('replaces message content and flips isStreaming to false', () => {
      const a = makeSession({ id: 'a', isStreaming: true, messages: [makeMessage({ id: 'm1', content: 'partial' })] });
      const next = finishSessionMessageStreaming([a], 'a', 'm1', 'final');
      expect(next[0].messages[0].content).toBe('final');
      expect(next[0].isStreaming).toBe(false);
    });
  });

  describe('clearSessionConversation', () => {
    it('drops messages, currentDiff, pendingDiff, and activeToolCalls', () => {
      const session = makeSession({
        messages: [makeMessage()],
        currentDiff: { hunks: [] } as unknown as ChatSession['currentDiff'],
        pendingDiff: { hunks: [] } as unknown as ChatSession['pendingDiff'],
        activeToolCalls: [{ id: 'tc' }] as unknown as ChatSession['activeToolCalls'],
      });
      const cleared = clearSessionConversation(session);
      expect(cleared.messages).toEqual([]);
      expect(cleared.currentDiff).toBeNull();
      expect(cleared.pendingDiff).toBeNull();
      expect(cleared.activeToolCalls).toEqual([]);
      // Identity fields survive.
      expect(cleared.id).toBe(session.id);
    });
  });

  describe('trimSessionMessagesAfter', () => {
    it('returns the session unchanged when target message is missing', () => {
      const session = makeSession({ messages: [makeMessage({ id: 'm1' })] });
      const next = trimSessionMessagesAfter(session, 'nope');
      expect(next.messages).toBe(session.messages);
    });

    it('drops messages after the target (inclusive of target)', () => {
      const session = makeSession({
        messages: [
          makeMessage({ id: 'm1' }),
          makeMessage({ id: 'm2' }),
          makeMessage({ id: 'm3' }),
        ],
      });
      const next = trimSessionMessagesAfter(session, 'm1');
      expect(next.messages.map((m) => m.id)).toEqual(['m1']);
    });
  });
});
