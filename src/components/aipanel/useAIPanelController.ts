import { useMemo } from 'react';
import { useAIPanelStore, type ChatMode } from '../../store';
import { buildKnowledgeToolbarModel } from './KnowledgeToolbar';
import { useKnowledgeBase } from './useKnowledgeBase';

export function useAIPanelController() {
  const {
    sessions,
    activeSessionId,
    createSession,
    deleteSession,
    setActiveSession,
    clearMessages,
    setIsOpen,
  } = useAIPanelStore();

  const activeSession = useMemo(
    () => sessions.find((session) => session.id === activeSessionId) ?? sessions[0],
    [sessions, activeSessionId]
  );

  const messages = activeSession?.messages ?? [];
  const isStreaming = activeSession?.isStreaming ?? false;
  const pendingDiff = activeSession?.pendingDiff ?? null;
  const mode: ChatMode = activeSession?.mode ?? 'ask';
  const activeToolCalls = activeSession?.activeToolCalls ?? [];

  const {
    workspacePath,
    knowledgeBase,
    buildProgress,
    knowledgeToolCall,
    handleKnowledgeBuild,
    handleKnowledgeClear,
  } = useKnowledgeBase({ activeSessionId: activeSession?.id });

  const knowledgeStatusLabel = knowledgeBase
    ? `已建立知识库：${knowledgeBase.documentCount} 文档 / ${knowledgeBase.chunkCount} 分块`
    : buildProgress
      ? '正在构建知识库…'
      : '知识库未初始化';

  const knowledgeToolbar = useMemo(
    () => buildKnowledgeToolbarModel({
      enabled: mode === 'knowledge' && !!activeSession,
      hasKnowledgeBase: !!knowledgeBase,
      isBuilding: !!buildProgress,
      onBuild: handleKnowledgeBuild,
      onClear: handleKnowledgeClear,
    }),
    [mode, activeSession, knowledgeBase, buildProgress, handleKnowledgeBuild, handleKnowledgeClear]
  );

  return {
    sessions,
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
    setActiveSession,
    clearMessages,
    closePanel: () => setIsOpen(false),
  };
}
