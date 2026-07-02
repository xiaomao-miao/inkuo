import React, { useState } from 'react';
import { ChatHeader } from './ChatHeader';
import { ChatInput } from './ChatInput';
import { ChatView } from './ChatView';
import { HistorySidebar } from './HistorySidebar';
import { KnowledgeBuildToolCard } from './KnowledgeBuildToolCard';
import { KnowledgeToolbar } from './KnowledgeToolbar';
import { useAgentStream } from './useAgentStream';
import { useChatComposer } from './useChatComposer';
import { useAIPanelController } from './useAIPanelController';
import layoutStyles from './AIPanelLayout.module.css';

export const AIPanel: React.FC = () => {
  const [historyOpen, setHistoryOpen] = useState(false);

  const {
    sessions,
    visibleSessions,
    activeSessionId,
    activeSession,
    messages,
    isStreaming,
    pendingDiff,
    mode,
    activeToolCalls,
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
    closePanel,
  } = useAIPanelController();

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

  const handleHistorySelect = (sessionId: string) => {
    setActiveSession(sessionId);
    setHistoryOpen(false);
  };

  const handleHistoryNewChat = () => {
    createSession();
    setHistoryOpen(false);
  };

  return (
    <aside className={layoutStyles.panel}>
      <ChatHeader
        sessions={visibleSessions}
        activeSessionId={activeSessionId}
        onCreateSession={createSession}
        onSelectSession={setActiveSession}
        onCloseSession={closeSession}
        onClose={closePanel}
        onToggleHistory={() => setHistoryOpen((v) => !v)}
        historyOpen={historyOpen}
      />

      <div className={layoutStyles.panelBody}>
        {mode === 'knowledge' && (
          <KnowledgeToolbar
            statusLabel={knowledgeStatusLabel}
            primaryAction={knowledgeToolbar.primaryAction}
            secondaryAction={knowledgeToolbar.secondaryAction}
          />
        )}

        <div className={layoutStyles.chatArea}>
          {historyOpen && (
            <HistorySidebar
              sessions={sessions}
              activeSessionId={activeSessionId}
              onSelect={handleHistorySelect}
              onReopen={reopenSession}
              onNewChat={handleHistoryNewChat}
              onDelete={deleteSession}
              onClose={() => setHistoryOpen(false)}
            />
          )}

          <div className={layoutStyles.chatMain}>
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
              footer={mode === 'knowledge' && knowledgeToolCall ? (
                <KnowledgeBuildToolCard
                  toolCall={knowledgeToolCall}
                  buildProgress={buildProgress}
                />
              ) : undefined}
            />
          </div>
        </div>
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
