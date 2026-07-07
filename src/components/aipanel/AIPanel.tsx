import React, { useState } from 'react';
import { ChatHeader } from './ChatHeader';
import { ChatInput } from './ChatInput';
import { ChatView } from './ChatView';
import { HistorySidebar } from './HistorySidebar';
import { TodoPanel } from './TodoPanel';
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
    createSession,
    deleteSession,
    closeSession,
    reopenSession,
    setActiveSession,
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
    handleApplyPlan,
    handleAdjustPlan,
    handleSavePlan,
    destroySessionPlanFiles,
  } = useChatComposer({
    activeSession,
    mode,
    messages,
    isStreaming,
  });

  useAgentStream({ mode });

  const handleHistoryActivate = (sessionId: string) => {
    // Clicking a session in the history panel should both restore it
    // to the chip bar AND make it the active conversation — never
    // just silently activate an archived session (it would still be
    // hidden in the header, which is confusing).
    reopenSession(sessionId);
    setActiveSession(sessionId);
    setHistoryOpen(false);
  };

  const handleHistoryNewChat = () => {
    createSession();
    setHistoryOpen(false);
  };

  /**
   * Wrap the store's `deleteSession` with a plan-file sweep so the
   * `.inkuo/plans/<id>.md` artifacts don't outlive the conversation
   * they came from. The Rust `plan_delete` calls run in parallel and
   * are best-effort — they're allowed to fail (e.g. workspace already
   * closed) without blocking the UI removal.
   */
  const handleDeleteSession = (sessionId: string) => {
    void destroySessionPlanFiles(sessionId);
    deleteSession(sessionId);
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
        <div className={layoutStyles.chatArea}>
          {historyOpen && (
            <HistorySidebar
              sessions={sessions}
              activeSessionId={activeSessionId}
              onActivate={handleHistoryActivate}
              onNewChat={handleHistoryNewChat}
              onDelete={handleDeleteSession}
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
              onApplyPlan={handleApplyPlan}
              onAdjustPlan={handleAdjustPlan}
              onSavePlan={handleSavePlan}
            />
          </div>
        </div>
      </div>

      {/* Task chip lives directly above the chat input. Renders nothing
          for sessions that haven't published an `update_todo` snapshot. */}
      <TodoPanel sessionId={activeSessionId} />

      <ChatInput
        input={input}
        setInput={setInput}
        mode={mode}
        isStreaming={isStreaming}
        sessionId={activeSessionId}
        featureToggles={activeSession?.featureToggles}
        onSend={handleSend}
        onStop={handleStop}
        onCycleMode={cycleMode}
      />
    </aside>
  );
};