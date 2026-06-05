import React from 'react';
import { ChatHeader } from './ChatHeader';
import { ChatInput } from './ChatInput';
import { ChatView } from './ChatView';
import { KnowledgeBuildToolCard } from './KnowledgeBuildToolCard';
import { KnowledgeToolbar } from './KnowledgeToolbar';
import { useAgentStream } from './useAgentStream';
import { useChatComposer } from './useChatComposer';
import { useAIPanelController } from './useAIPanelController';
import layoutStyles from './AIPanelLayout.module.css';

export const AIPanel: React.FC = () => {
  const {
    sessions,
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

  return (
    <aside className={layoutStyles.panel}>
      <ChatHeader
        sessions={sessions}
        activeSessionId={activeSessionId}
        onCreateSession={createSession}
        onSelectSession={setActiveSession}
        onDeleteSession={deleteSession}
        onClose={closePanel}
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
          footer={mode === 'knowledge' && knowledgeToolCall ? (
            <KnowledgeBuildToolCard
              toolCall={knowledgeToolCall}
              buildProgress={buildProgress}
            />
          ) : undefined}
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
