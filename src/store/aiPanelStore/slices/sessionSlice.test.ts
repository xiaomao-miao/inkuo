import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { create } from 'zustand';

import type { AIPanelState, AIPanelStateCreator } from '../../aiPanelStore.types';
import type { ChatSession } from '../../../types';
import { DEFAULT_CHAT_MODE } from '../../../constants/chatModes';
import { createSessionSlice } from './sessionSlice';
import { useBaselineStore } from '../../baselineStore';

const STORAGE_KEY = 'inkuo-baselines';

// `createSessionSlice` is typed via `AIPanelStateCreator`, which still
// references the full `AIPanelState`. We satisfy the constraint by
// casting — the slice only ever reads/writes its own keys on this
// store, and the test never exercises the missing fields.
type SessionOnlyStore = Pick<
  AIPanelState,
  | 'sessions'
  | 'activeSessionId'
  | 'todoSnapshotBySession'
  | 'createSession'
  | 'deleteSession'
  | 'closeSession'
  | 'reopenSession'
  | 'setActiveSession'
  | 'setSessionMode'
  | 'setSessionFeatureToggle'
  | 'getSession'
  | 'updateSession'
  | 'setSessionTodoSnapshot'
  | 'clearSessionTodoSnapshot'
  | 'resetSessionDerivedState'
>;

const buildStore = () =>
  create<SessionOnlyStore>()((...a) => ({
    ...createSessionSlice(...a as Parameters<AIPanelStateCreator<SessionOnlyStore>>),
  }));

const makeSession = (overrides: Partial<ChatSession> = {}): ChatSession => ({
  id: 'sess-1',
  title: '对话 1',
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

describe('sessionSlice', () => {
  beforeEach(() => {
    useBaselineStore.getState().reset();
    if (typeof localStorage !== 'undefined') {
      localStorage.removeItem(STORAGE_KEY);
    }
  });

  afterEach(() => {
    useBaselineStore.getState().reset();
    if (typeof localStorage !== 'undefined') {
      localStorage.removeItem(STORAGE_KEY);
    }
  });

  describe('resetSessionDerivedState', () => {
    it('clears activeToolCalls, diffs, isStreaming, and the todo snapshot without touching messages', () => {
      const store = buildStore();
      store.setState({
        sessions: [
          makeSession({
            id: 'sess-1',
            messages: [
              { id: 'm1', role: 'user', content: 'hi', timestamp: 0, outputItems: [] } as ChatSession['messages'][number],
            ],
            activeToolCalls: [
              { id: 'tc-1', name: 'read_file', arguments: {}, status: 'success', startTime: 1 } as ChatSession['activeToolCalls'][number],
            ],
            currentDiff: { hunks: [] } as unknown as ChatSession['currentDiff'],
            pendingDiff: { hunks: [] } as unknown as ChatSession['pendingDiff'],
            isStreaming: true,
          }),
        ],
        todoSnapshotBySession: {
          'sess-1': {
            items: [],
            toolCallId: 'tc-1',
            updatedAt: 1,
          },
        },
      });

      store.getState().resetSessionDerivedState('sess-1');

      const session = store.getState().sessions[0];
      expect(session.activeToolCalls).toEqual([]);
      expect(session.currentDiff).toBeNull();
      expect(session.pendingDiff).toBeNull();
      expect(session.isStreaming).toBe(false);
      // Messages untouched.
      expect(session.messages.map((m) => m.id)).toEqual(['m1']);
      // Todo snapshot cleared.
      expect(store.getState().todoSnapshotBySession).toEqual({});
    });

    it('leaves the session list intact when the target id does not exist', () => {
      const store = buildStore();
      const before = store.getState();
      store.getState().resetSessionDerivedState('nope');
      // The slice still creates a new array via `map`, but no actual
      // session field is touched.
      expect(store.getState().sessions).toEqual(before.sessions);
      expect(store.getState().todoSnapshotBySession).toEqual(before.todoSnapshotBySession);
    });
  });

  describe('deleteSession', () => {
    it('clears the baselines owned by the deleted session', () => {
      const store = buildStore();
      store.setState({
        sessions: [
          makeSession({
            id: 'sess-1',
            messages: [
              { id: 'm1', role: 'user', content: 'a', timestamp: 0, outputItems: [] } as ChatSession['messages'][number],
              { id: 'm2', role: 'user', content: 'b', timestamp: 0, outputItems: [] } as ChatSession['messages'][number],
            ],
          }),
          makeSession({ id: 'sess-2' }),
        ],
      });
      useBaselineStore.getState().recordBaseline('m1', 'snap-1');
      useBaselineStore.getState().recordBaseline('m2', 'snap-2');
      useBaselineStore.getState().recordBaseline('m-kept', 'snap-3');

      store.getState().deleteSession('sess-1');

      expect(useBaselineStore.getState().baselines).toEqual({
        'm-kept': 'snap-3',
      });
    });

    it('keeps the baselines intact when the session id does not match', () => {
      const store = buildStore();
      store.setState({
        sessions: [makeSession({ id: 'sess-1' })],
      });
      useBaselineStore.getState().recordBaseline('m1', 'snap-1');

      store.getState().deleteSession('sess-999');

      expect(useBaselineStore.getState().baselines).toEqual({
        'm1': 'snap-1',
      });
    });
  });
});
