//! Sub-agent activity slice of the AI panel store.
//!
//! Records and clears `SubagentActivity` entries attached to specific
//! messages inside a session. The slice is intentionally narrow —
//! sub-agent activity is read by the message renderer, not directly by
//! the user — so it never crosses the persistence boundary.

import type { AIPanelStateCreator, SubagentActivitySlice } from '../../aiPanelStore.types';
import { updateSessions } from '../../aiPanelReducers';

export const createSubagentSlice: AIPanelStateCreator<SubagentActivitySlice> = (set) => ({
  addSubagentActivity: (sessionId, messageId, activity) =>
    set((state) => ({
      sessions: updateSessions(state.sessions, sessionId, (session) => ({
        ...session,
        messages: session.messages.map((msg) =>
          msg.id === messageId
            ? {
                ...msg,
                subagentActivities: [
                  ...(msg.subagentActivities ?? []),
                  activity,
                ],
              }
            : msg,
        ),
      })),
    })),

  addOutputToSubagentActivity: (sessionId, parentMessageId, subagentId, outputItem) =>
    set((state) => ({
      sessions: updateSessions(state.sessions, sessionId, (session) => ({
        ...session,
        messages: session.messages.map((msg) =>
          msg.id === parentMessageId
            ? {
                ...msg,
                subagentActivities: msg.subagentActivities?.map((activity) =>
                  activity.id === subagentId
                    ? {
                        ...activity,
                        outputItems: [...activity.outputItems, outputItem],
                      }
                    : activity,
                ),
              }
            : msg,
        ),
      })),
    })),

  appendOutputDeltaToSubagentActivity: (
    sessionId: string,
    parentMessageId: string,
    subagentId: string,
    delta: { content: string; type: 'text' | 'reasoning' },
  ) =>
    set((state) => ({
      sessions: updateSessions(state.sessions, sessionId, (session) => ({
        ...session,
        messages: session.messages.map((msg) => {
          if (msg.id !== parentMessageId) return msg;
          return {
            ...msg,
            subagentActivities: msg.subagentActivities?.map((activity) => {
              if (activity.id !== subagentId) return activity;
              const items = activity.outputItems;
              const last = items[items.length - 1];
              if (last && last.type === delta.type && (last.type === 'text' || last.type === 'reasoning')) {
                const merged = {
                  ...last,
                  content: last.content + delta.content,
                };
                return {
                  ...activity,
                  outputItems: [...items.slice(0, -1), merged],
                };
              }
              const fresh =
                delta.type === 'text'
                  ? { type: 'text' as const, content: delta.content, isPendingMarkdown: false }
                  : { type: 'reasoning' as const, content: delta.content, isPendingMarkdown: false };
              return {
                ...activity,
                outputItems: [...items, fresh],
              };
            }),
          };
        }),
      })),
    })),

  completeSubagentActivity: (sessionId, parentMessageId, subagentId, status, summary, error) =>
    set((state) => ({
      sessions: updateSessions(state.sessions, sessionId, (session) => ({
        ...session,
        messages: session.messages.map((msg) =>
          msg.id === parentMessageId
            ? {
                ...msg,
                subagentActivities: msg.subagentActivities?.map((activity) =>
                  activity.id === subagentId
                    ? {
                        ...activity,
                        status,
                        summary,
                        error,
                        // Auto-collapse on completion
                        expanded: false,
                      }
                    : activity,
                ),
              }
            : msg,
        ),
      })),
    })),

  toggleSubagentActivityExpanded: (sessionId, parentMessageId, subagentId) =>
    set((state) => ({
      sessions: updateSessions(state.sessions, sessionId, (session) => ({
        ...session,
        messages: session.messages.map((msg) =>
          msg.id === parentMessageId
            ? {
                ...msg,
                subagentActivities: msg.subagentActivities?.map((activity) =>
                  activity.id === subagentId
                    ? { ...activity, expanded: !activity.expanded }
                    : activity,
                ),
              }
            : msg,
        ),
      })),
    })),
});
