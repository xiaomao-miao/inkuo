import React, { useMemo } from 'react';
import { useAIPanelStore, type ChatMode } from '../../store';
import { ChatHeader } from './ChatHeader';
import { ChatInput } from './ChatInput';
import { ChatView } from './ChatView';
import { KnowledgeToolbar, buildKnowledgeToolbarModel } from './KnowledgeToolbar';
import { useAgentStream } from './useAgentStream';
import { useChatComposer } from './useChatComposer';
import { useKnowledgeBase } from './useKnowledgeBase';
import layoutStyles from './AIPanelLayout.module.css';

export const AIPanel: React.FC = () => {
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
    workspacePath: _workspacePath,
    knowledgeBase,
    buildProgress,
    knowledgeToolCall,
    handleKnowledgeBuild,
    handleKnowledgeClear,
  } = useKnowledgeBase({ activeSessionId: activeSession?.id });

  const {
    input,
    setInput,
    editingMessageId,
    editingContent,
    setEditingContent,
    handleSend,
    handleStop,
    cycleMode,
    handleStartEdit,
    handleCancelEdit,
    handleSaveEdit,
  } = useChatComposer({
    activeSession,
    mode,
    messages,
    isStreaming,
  });

  useAgentStream({ mode });

  const knowledgeStatusLabel = knowledgeBase
    ? `已索引 ${knowledgeBase.documentCount} 文档 / ${knowledgeBase.chunkCount} 分块`
    : buildProgress
      ? '正在构建知识库…'
      : '知识库未创建';

  const knowledgeToolbar = useMemo(() => buildKnowledgeToolbarModel({
    enabled: mode === 'knowledge' && !!activeSession,
    hasKnowledgeBase: !!knowledgeBase,
    isBuilding: !!buildProgress,
    onBuild: handleKnowledgeBuild,
    onClear: handleKnowledgeClear,
  }), [mode, activeSession, knowledgeBase, buildProgress, handleKnowledgeBuild, handleKnowledgeClear]);

  return (
    <aside className={layoutStyles.panel}>
      <ChatHeader
        sessions={sessions}
        activeSessionId={activeSessionId}
        onCreateSession={createSession}
        onSelectSession={setActiveSession}
        onDeleteSession={deleteSession}
        onClose={() => setIsOpen(false)}
      />

      <div className={layoutStyles.panelBody}>
        {mode === 'knowledge' && (
          <KnowledgeToolbar
            statusLabel={knowledgeStatusLabel}
            primaryAction={knowledgeToolbar.primaryAction}
            secondaryAction={knowledgeToolbar.secondaryAction}
          />
        )}

        <ChatView
          messages={messages}
          activeSession={activeSession}
          isStreaming={isStreaming}
          pendingDiff={pendingDiff}
          mode={mode}
          activeToolCalls={activeToolCalls}
          editingMessageId={editingMessageId}
          editingContent={editingContent}
          onStartEdit={handleStartEdit}
          onCancelEdit={handleCancelEdit}
          onSaveEdit={handleSaveEdit}
          onSetEditingContent={setEditingContent}
          onSetInput={setInput}
          knowledgeToolCall={mode === 'knowledge' ? knowledgeToolCall : undefined}
          knowledgeBuildProgress={mode === 'knowledge' ? buildProgress : undefined}
        />
      </div>

      <ChatInput
        input={input}
        setInput={setInput}
        mode={mode}
        isStreaming={isStreaming}
        hasMessages={messages.length > 0}
        onSend={handleSend}
        onStop={handleStop}
        onClear={() => activeSession && clearMessages(activeSession.id)}
        onCycleMode={cycleMode}
      />
    </aside>
  );
};
