import { useMemo } from 'react';
import { useAIPanelStore, type ChatMode } from '../../store';
import { buildKnowledgeToolbarModel } from './knowledgeToolbarModel';
import { useKnowledgeBase } from './useKnowledgeBase';
import { DEFAULT_CHAT_MODE } from '../../constants/chatModes';

export function useAIPanelController() {
  const sessions = useAIPanelStore((state) => state.sessions);
  const activeSessionId = useAIPanelStore((state) => state.activeSessionId);
  const createSession = useAIPanelStore((state) => state.createSession);
  const deleteSession = useAIPanelStore((state) => state.deleteSession);
  const closeSession = useAIPanelStore((state) => state.closeSession);
  const reopenSession = useAIPanelStore((state) => state.reopenSession);
  const setActiveSession = useAIPanelStore((state) => state.setActiveSession);
  const clearMessages = useAIPanelStore((state) => state.clearMessages);
  const setIsOpen = useAIPanelStore((state) => state.setIsOpen);

  // Only show non-archived sessions in the header chip bar. The closed
  // ones still live in the array (and on disk) — they're reachable from
  // the HistorySidebar.
  const visibleSessions = useMemo(
    () => sessions.filter((s) => !s.archived),
    [sessions]
  );

  const activeSession = useMemo(
    () => sessions.find((session) => session.id === activeSessionId) ?? sessions[0],
    [sessions, activeSessionId]
  );

  const messages = activeSession?.messages ?? [];
  const isStreaming = activeSession?.isStreaming ?? false;
  const pendingDiff = activeSession?.pendingDiff ?? null;
  const mode: ChatMode = activeSession?.mode ?? DEFAULT_CHAT_MODE;
  const activeToolCalls = activeSession?.activeToolCalls ?? [];

  const {
    workspacePath,
    knowledgeBase,
    buildProgress,
    knowledgeToolCall,
  } = useKnowledgeBase({ activeSessionId: activeSession?.id });

  const knowledgeStatusLabel = knowledgeBase
    ? `知识库：${knowledgeBase.members.length} 个文件 / ${knowledgeBase.documentCount} 文档 / ${knowledgeBase.chunkCount} 分块`
    : buildProgress
      ? '正在构建知识库…'
      : '知识库未初始化';

  const knowledgeToolbar = useMemo(
    () => buildKnowledgeToolbarModel(),
    []
  );

  return {
    sessions,
    visibleSessions,
    activeSessionId,
    activeSession,
    messages,
    isStreaming,
    pendingDiff,
    mode,
    activeToolCalls,
    workspacePath,
    knowledgeBase,
    buildProgress,
    knowledgeToolCall,
    knowledgeStatusLabel,
    knowledgeToolbar,
    createSession,
    deleteSession,
    closeSession,
    reopenSession,
    setActiveSession,
    clearMessages,
    closePanel: () => setIsOpen(false),
  };
}
