import React from 'react';
import { Loader2, X } from 'lucide-react';
import { LazyTextContent } from './LazyTextContent';
import { ReasoningBlock } from './ReasoningBlock';
import { ToolCallCard } from './ToolCallCard';
import { CompactToolCard } from './CompactToolCard';
import { DelegateToCard, GetToolHelpCard } from './DelegateToCard';
import { COMPACT_TOOLS } from './toolUtils';
import { PlanCard } from './PlanCard';
import { AskUserCard } from './AskUserCard';
import { useAIPanelStore, useSidebarStore } from '../../store';
import type {
  ActiveToolCall,
  ChatMessage,
  OutputItem,
  PlanOutput,
} from '../../store';
import styles from './AIPanelMessage.module.css';

interface AssistantMessageBodyProps {
  message: ChatMessage;
  isThisStreaming: boolean;
  activeToolCalls: ActiveToolCall[];
  sessionId: string;
  /**
   * Apply a structured plan: flips session to agent mode and tears down
   * the plan's `.inkuo/plans/<id>.md` artifact before dispatching the
   * follow-up turn. Receives the messageId so the action handler can
   * locate the right plan item to destroy.
   */
  onApplyPlan?: (messageId: string, plan: PlanOutput) => void;
  /**
   * Adjust a structured plan: refill the input with a hint pointing the
   * user at the plan for refinement. Currently unused (kept in reserve)
   * but kept symmetric to `onApplyPlan`.
   */
  onAdjustPlan?: (messageId: string, plan: PlanOutput) => void;
  /**
   * Persist the trailing plan OutputItem's content to
   * `<workspace>/.inkuo/plans/<id>.md`. Returns once Rust has written the
   * file (and stamped `planFileId` / `planFilePath` back onto the item).
   */
  onSavePlan?: (messageId: string) => Promise<void>;
}

export const AssistantMessageBody: React.FC<AssistantMessageBodyProps> = ({
  message,
  isThisStreaming,
  activeToolCalls,
  sessionId,
  onApplyPlan,
  onAdjustPlan,
  onSavePlan,
}) => {
  const hasOutputItems = message.outputItems && message.outputItems.length > 0;
  const workspacePath = useSidebarStore((s) => s.workspacePath);
  const openWorkspaceFile = useSidebarStore((s) => s.openWorkspaceFile);
  const toggleSubagentActivityExpanded = useAIPanelStore((s) => s.toggleSubagentActivityExpanded);

  /**
   * Precompute a toolCallId → toolCall map for O(1) lookups in
   * `OutputItemView`. Without this, every tool-result render triggers
   * an O(n) `find()` over `message.toolCalls`, and `AssistantMessageBody`
   * re-renders on every streaming token — so the total work was O(n×m)
   * per token for n output items × m tool calls. With the map the
   * per-token cost collapses to O(1) per lookup.
   *
   * The map is recomputed only when the underlying `toolCalls` array
   * identity changes, which is exactly when it could differ.
   */
  const toolCallMap = React.useMemo(() => {
    if (!message.toolCalls || message.toolCalls.length === 0) {
      return null;
    }
    return new Map(message.toolCalls.map((tc) => [tc.id, tc]));
  }, [message.toolCalls]);

  const handleFileClick = React.useCallback((filePath: string) => {
    openWorkspaceFile(filePath, { forceNew: true });
  }, [openWorkspaceFile]);

  const handleSubagentToggle = React.useCallback((subagentId: string) => {
    toggleSubagentActivityExpanded(sessionId, message.id, subagentId);
  }, [sessionId, message.id, toggleSubagentActivityExpanded]);

  return (
    <>
      {hasOutputItems && message.outputItems.map((item, idx) => (
        <OutputItemView
          key={idx}
          item={item}
          message={message}
          sessionId={sessionId}
          isThisStreaming={isThisStreaming}
          isLastItem={idx === message.outputItems.length - 1}
          onFileClick={handleFileClick}
          workspacePath={workspacePath ?? undefined}
          onSubagentToggle={handleSubagentToggle}
          onApplyPlan={onApplyPlan}
          onAdjustPlan={onAdjustPlan}
          onSavePlan={onSavePlan}
          toolCallMap={toolCallMap}
        />
      ))}
      {/* Legacy content path */}
      {!hasOutputItems && message.content && (
        <LegacyMessageContent
          message={message}
          sessionId={sessionId}
          isThisStreaming={isThisStreaming}
          activeToolCalls={activeToolCalls}
          onFileClick={handleFileClick}
          workspacePath={workspacePath ?? undefined}
          onSubagentToggle={handleSubagentToggle}
        />
      )}
    </>
  );
};

interface OutputItemViewProps {
  item: OutputItem;
  message: ChatMessage;
  sessionId: string;
  isThisStreaming: boolean;
  isLastItem: boolean;
  onFileClick?: (filePath: string) => void;
  workspacePath?: string;
  onSubagentToggle?: (subagentId: string) => void;
  onApplyPlan?: (messageId: string, plan: PlanOutput) => void;
  onAdjustPlan?: (messageId: string, plan: PlanOutput) => void;
  onSavePlan?: (messageId: string) => Promise<void>;
  /**
   * Precomputed toolCallId → toolCall map. Passing it down (instead of
   * doing a per-item `find`) keeps tool-result rendering O(1) instead
   * of O(n) per item, which matters because `OutputItemView` re-renders
   * on every streaming token.
   */
  toolCallMap?: Map<string, NonNullable<ChatMessage['toolCalls']>[number]> | null;
}

const OutputItemView: React.FC<OutputItemViewProps> = ({
  item,
  message,
  sessionId,
  isThisStreaming,
  isLastItem,
  onFileClick,
  workspacePath,
  onSubagentToggle,
  onApplyPlan,
  onAdjustPlan,
  onSavePlan,
  toolCallMap,
}) => {
  if (item.type === 'text') {
    return (
      <div className={styles.outputTextItem}>
        <LazyTextContent
          messageId={message.id}
          sessionId={sessionId}
          visibleContent={item.content}
          truncatedPrefixLength={item.truncatedPrefix?.length ?? 0}
          isStreaming={isThisStreaming}
          onFileClick={onFileClick}
          workspacePath={workspacePath}
        />
      </div>
    );
  }

  if (item.type === 'reasoning') {
    return <ReasoningItemView item={item} messageId={message.id} sessionId={sessionId} />;
  }

  if (item.type === 'ask_user') {
    return (
      <AskUserCard
        sessionId={sessionId}
        messageId={message.id}
        toolCallId={item.toolCallId}
        question={item.question}
        options={item.options}
        allowCustom={item.allowCustom ?? true}
        answer={item.answer}
      />
    );
  }

  if (item.type === 'tool_call_start') {
    const status = item.status || (item.isExecuting ? 'executing' : 'pending');

    // Specialized renderers for sub-agent meta tools.
    if (item.toolName === 'delegate_to') {
      const args = item.arguments || {};
      // Find subagent activities for this delegate_to call
      const subagentActivities = message.subagentActivities?.filter(
        a => a.expert === (args.expert as string)
      );
      return (
        <ToolOutputItem
          isCompact={false}
          isThisStreaming={isThisStreaming}
          isLastItem={isLastItem}
          content={
            <DelegateToCard
              id={item.toolCallId}
              expert={(args.expert as string) || ''}
              task={(args.task as string) || ''}
              status={status}
              result={item.result}
              duration={item.duration}
              subagentActivities={subagentActivities}
              onToggleSubagentActivity={subagentActivities?.length ? onSubagentToggle : undefined}
            />
          }
        />
      );
    }

    if (item.toolName === 'get_tool_help') {
      const args = item.arguments || {};
      const category = (args.category as string) || (args.spec as string) || '';
      return (
        <ToolOutputItem
          isCompact={false}
          isThisStreaming={isThisStreaming}
          isLastItem={isLastItem}
          content={
            <GetToolHelpCard
              id={item.toolCallId}
              spec={category}
              status={status}
              result={item.result}
              duration={item.duration}
            />
          }
        />
      );
    }

    return (
      <ToolOutputItem
        isCompact={COMPACT_TOOLS.has(item.toolName)}
        isThisStreaming={isThisStreaming}
        isLastItem={isLastItem}
        content={COMPACT_TOOLS.has(item.toolName) ? (
          <CompactToolCard
            id={item.toolCallId}
            name={item.toolName}
            arguments={item.arguments}
            status={status}
            duration={item.duration}
          />
        ) : (
          <ToolCallCard
            id={item.toolCallId}
            name={item.toolName}
            arguments={item.arguments}
            rawArguments={item.rawArguments}
            streamingContent={item.streamingContent}
            status={status}
            isStreamingArguments={item.isExecuting}
            result={item.result}
            duration={item.duration}
            diffSummary={item.diffSummary}
            onFileClick={onFileClick}
            workspacePath={workspacePath}
          />
        )}
      />
    );
  }

  if (item.type === 'tool_result') {
    // O(1) lookup via the map computed once per message in
    // AssistantMessageBody. Falls back to undefined (same semantics as
    // the old `find()` returning undefined) when no map was passed.
    const toolCall = toolCallMap?.get(item.toolCallId);
    const toolName = toolCall?.name || 'unknown';

    if (toolName === 'delegate_to') {
      const args = toolCall?.arguments || {};
      return (
        <div className={styles.toolResultItem} style={{ display: 'none' }}>
          <DelegateToCard
            id={item.toolCallId}
            expert={(args.expert as string) || ''}
            task={(args.task as string) || ''}
            status={item.status}
            result={item.result}
            duration={item.duration}
          />
        </div>
      );
    }

    if (toolName === 'get_tool_help') {
      const args = toolCall?.arguments || {};
      const category = (args.category as string) || (args.spec as string) || '';
      return (
        <div className={styles.toolResultItem} style={{ display: 'none' }}>
          <GetToolHelpCard
            id={item.toolCallId}
            spec={category}
            status={item.status}
            result={item.result}
            duration={item.duration}
          />
        </div>
      );
    }

    return (
      <div className={styles.toolResultItem} style={{ display: 'none' }}>
        <ToolCallCard
          id={item.toolCallId}
          name={toolName}
          arguments={toolCall?.arguments || {}}
          status={item.status}
          result={item.result}
          error={item.status === 'error' ? item.result : undefined}
          duration={item.duration}
          diffSummary={item.diffSummary}
          onFileClick={onFileClick}
          workspacePath={workspacePath}
        />
      </div>
    );
  }

  if (item.type === 'plan') {
    const handleSaveClick = async () => {
      if (!onSavePlan) return;
      try {
        await onSavePlan(message.id);
      } catch (err) {
        // Surface save failures as a chat notification so the user can
        // retry — silently dropping them leaves the card in an ambiguous
        // "should I have a file?" state. The error boundary is in
        // useChatSessionActions.handleSavePlan.
        console.warn('[plan-save] failed:', err);
      }
    };
    return (
      <div className={styles.outputTextItem}>
        <PlanCard
          rawText={item.rawText}
          plan={item.plan}
          parseError={item.parseError}
          isStreaming={item.isStreaming ?? isThisStreaming}
          messageId={message.id}
          onApply={onApplyPlan}
          onAdjust={onAdjustPlan}
          onSave={onSavePlan ? handleSaveClick : undefined}
          onFileClick={onFileClick}
          workspacePath={workspacePath}
          savedFilePath={item.planFilePath}
        />
      </div>
    );
  }

  if (item.type === 'tool_error') {
    return (
      <div className={styles.toolErrorItem}>
        <div className={styles.toolErrorBadge}>
          <X size={12} />
          <span>工具执行失败</span>
        </div>
        <pre className={styles.toolErrorText}>{item.error}</pre>
      </div>
    );
  }

  return null;
};

interface ToolOutputItemProps {
  isCompact: boolean;
  isThisStreaming: boolean;
  isLastItem: boolean;
  content: React.ReactNode;
}

const ToolOutputItem: React.FC<ToolOutputItemProps> = ({
  isThisStreaming,
  isLastItem,
  content,
}) => {
  return (
    <div className={styles.toolResultItem}>
      {isThisStreaming && isLastItem && <ContinueGeneratingIndicator />}
      {content}
    </div>
  );
};

interface ReasoningItemViewProps {
  item: Extract<OutputItem, { type: 'reasoning' }>;
  messageId: string;
  sessionId: string;
}

/**
 * Render a single reasoning OutputItem.
 *
 * Collapse state is per-block — each reasoning item has a `reasoningId`
 * that is added to (or removed from) the parent message's
 * `expandedReasoningIds` set as the user clicks the header. This way one
 * block can be expanded while another in the same message stays
 * collapsed.
 */
const ReasoningItemView: React.FC<ReasoningItemViewProps> = ({
  item,
  messageId,
  sessionId,
}) => {
  // The streaming reducer assigns a fresh stable id at creation time.
  // For items loaded from a persisted snapshot (no id) we fall back to
  // a stringified combination of messageId + the item's relative
  // position; that keeps the per-block state stable for the lifetime
  // of the item even without a real id.
  const reasoningId = item.reasoningId ?? `${messageId}:reasoning-legacy`;

  const userExpanded = useAIPanelStore((state) => {
    const session = state.sessions.find((s) => s.id === sessionId);
    const message = session?.messages.find((m) => m.id === messageId);
    return message?.expandedReasoningIds?.includes(reasoningId) ?? false;
  });
  const toggleReasoningExpansion = useAIPanelStore(
    (state) => state.toggleReasoningExpansion,
  );

  const handleToggleExpansion = () => {
    toggleReasoningExpansion(sessionId, messageId, reasoningId);
  };

  return (
    <ReasoningBlock
      content={item.content}
      completed={!!item.completed}
      userExpanded={userExpanded}
      reasoningId={reasoningId}
      onToggleExpansion={handleToggleExpansion}
    />
  );
};

const ContinueGeneratingIndicator: React.FC = () => (
  <div className={styles.continueGenerating}>
    <span className={styles.continueDots}>
      <span className={styles.dot} />
      <span className={styles.dot} />
      <span className={styles.dot} />
    </span>
  </div>
);

interface LegacyMessageContentProps {
  message: ChatMessage;
  sessionId: string;
  isThisStreaming: boolean;
  activeToolCalls: ActiveToolCall[];
  onFileClick?: (filePath: string) => void;
  workspacePath?: string;
  onSubagentToggle?: (subagentId: string) => void;
}

const LegacyMessageContent: React.FC<LegacyMessageContentProps> = ({
  message,
  sessionId,
  isThisStreaming,
  activeToolCalls,
  onFileClick,
  workspacePath,
  onSubagentToggle,
}) => {
  /**
   * Map for O(1) tool-call lookups. Built once per render so the
   * `.map(result => ...)` body below stays O(n) instead of O(n×m).
   */
  const toolCallMap = React.useMemo(() => {
    if (!message.toolCalls || message.toolCalls.length === 0) {
      return null;
    }
    return new Map(message.toolCalls.map((tc) => [tc.id, tc]));
  }, [message.toolCalls]);

  return (
    <>
      {isThisStreaming && activeToolCalls.map((tc) => (
        <div key={tc.id} className={styles.streamingToolCall}>
          <Loader2 size={12} className={styles.spinning} />
          <span className={styles.streamingToolName}>{tc.name}</span>
        </div>
      ))}
      {message.toolCalls && message.toolCalls.length > 0 && !message.toolResults?.length && (
        <div className={styles.toolExecutingIndicator}>
          <Loader2 size={12} className={styles.spinning} />
          <span>正在执行工具...</span>
        </div>
      )}
      {message.content ? (
        <LazyTextContent
          messageId={message.id}
          sessionId={sessionId}
          visibleContent={message.content}
          truncatedPrefixLength={message.truncatedPrefix?.length ?? 0}
          isStreaming={isThisStreaming}
          onFileClick={onFileClick}
          workspacePath={workspacePath}
        />
      ) : !message.toolResults?.length && !isThisStreaming ? (
        <div className={styles.toolOnlyPlaceholder}>工具执行完成</div>
      ) : null}
      {message.toolResults?.map((result) => {
        const toolCall = toolCallMap?.get(result.toolCallId);
        const toolName = toolCall?.name || 'unknown';
        if (toolName === 'delegate_to') {
          const args = toolCall?.arguments || {};
          const subagentActivities = message.subagentActivities?.filter(
            a => a.expert === (args.expert as string)
          );
          return (
            <div key={`tool-${result.toolCallId}`} className={styles.toolResultItem}>
              <DelegateToCard
                id={result.toolCallId}
                expert={(args.expert as string) || ''}
                task={(args.task as string) || ''}
                status={result.isError ? 'error' : 'success'}
                result={result.result}
                error={result.isError ? result.result : undefined}
                duration={result.duration}
                subagentActivities={subagentActivities}
                onToggleSubagentActivity={subagentActivities?.length ? onSubagentToggle : undefined}
              />
            </div>
          );
        }
        if (toolName === 'get_tool_help') {
          const args = toolCall?.arguments || {};
          const category = (args.category as string) || (args.spec as string) || '';
          return (
            <div key={`tool-${result.toolCallId}`} className={styles.toolResultItem}>
              <GetToolHelpCard
                id={result.toolCallId}
                spec={category}
                status={result.isError ? 'error' : 'success'}
                result={result.result}
                duration={result.duration}
              />
            </div>
          );
        }
        return (
          <div key={`tool-${result.toolCallId}`} className={styles.toolResultItem}>
            <ToolCallCard
              id={result.toolCallId}
              name={toolName}
              arguments={toolCall?.arguments || {}}
              status={result.isError ? 'error' : 'success'}
              result={result.result}
              error={result.isError ? result.result : undefined}
              duration={result.duration}
              diffSummary={result.diffSummary}
              onFileClick={onFileClick}
              workspacePath={workspacePath}
            />
          </div>
        );
      })}
    </>
  );
};
