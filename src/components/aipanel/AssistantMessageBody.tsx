import React from 'react';
import { Loader2, X } from 'lucide-react';
import { LazyTextContent } from './LazyTextContent';
import { ReasoningBlock } from './ReasoningBlock';
import { ToolCallCard } from './ToolCallCard';
import { CompactToolCard } from './CompactToolCard';
import { DelegateToCard, GetToolHelpCard } from './DelegateToCard';
import { COMPACT_TOOLS } from './toolUtils';
import { parsePlanBlocks, type PlanBlock } from './planRender';
import { useAIPanelStore, useSidebarStore } from '../../store';
import type { ActiveToolCall, ChatMessage, ChatMode, OutputItem } from '../../store';
import styles from './AIPanelMessage.module.css';

interface AssistantMessageBodyProps {
  message: ChatMessage;
  isThisStreaming: boolean;
  mode: ChatMode;
  activeToolCalls: ActiveToolCall[];
  sessionId: string;
}

export const AssistantMessageBody: React.FC<AssistantMessageBodyProps> = ({
  message,
  isThisStreaming,
  mode,
  activeToolCalls,
  sessionId,
}) => {
  const hasOutputItems = message.outputItems && message.outputItems.length > 0;
  const workspacePath = useSidebarStore((s) => s.workspacePath);
  const openWorkspaceFile = useSidebarStore((s) => s.openWorkspaceFile);

  const handleFileClick = React.useCallback((filePath: string) => {
    openWorkspaceFile(filePath, { forceNew: true });
  }, [openWorkspaceFile]);

  if (hasOutputItems) {
    return (
      <>
        {message.outputItems.map((item, idx) => (
          <OutputItemView
            key={idx}
            item={item}
            message={message}
            sessionId={sessionId}
            isThisStreaming={isThisStreaming}
            isLastItem={idx === message.outputItems.length - 1}
            onFileClick={handleFileClick}
            workspacePath={workspacePath ?? undefined}
          />
        ))}
      </>
    );
  }

  if (message.content) {
    return (
      <LegacyMessageContent
        message={message}
        sessionId={sessionId}
        isThisStreaming={isThisStreaming}
        mode={mode}
        activeToolCalls={activeToolCalls}
        onFileClick={handleFileClick}
        workspacePath={workspacePath ?? undefined}
      />
    );
  }

  return null;
};

interface OutputItemViewProps {
  item: OutputItem;
  message: ChatMessage;
  sessionId: string;
  isThisStreaming: boolean;
  isLastItem: boolean;
  onFileClick?: (filePath: string) => void;
  workspacePath?: string;
}

const OutputItemView: React.FC<OutputItemViewProps> = ({
  item,
  message,
  sessionId,
  isThisStreaming,
  isLastItem,
  onFileClick,
  workspacePath,
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

  if (item.type === 'tool_call_start') {
    const status = item.status || (item.isExecuting ? 'executing' : 'pending');

    // Specialized renderers for sub-agent meta tools.
    if (item.toolName === 'delegate_to') {
      const args = item.arguments || {};
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
    const toolCall = message.toolCalls?.find((tc) => tc.id === item.toolCallId);
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
  mode: ChatMode;
  activeToolCalls: ActiveToolCall[];
  onFileClick?: (filePath: string) => void;
  workspacePath?: string;
}

const LegacyMessageContent: React.FC<LegacyMessageContentProps> = ({
  message,
  sessionId,
  isThisStreaming,
  mode,
  activeToolCalls,
  onFileClick,
  workspacePath,
}) => {
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
      {mode === 'plan' && message.content ? (
        <div className={styles.planBlocks}>
          {parsePlanBlocks(message.content).map((b: PlanBlock, idx: number) => (
            <div key={idx} className={styles.planBlock}>
              <div className={styles.planTitle}>{b.title}</div>
              <pre className={styles.planBody}>{b.lines.join('\n')}</pre>
            </div>
          ))}
        </div>
      ) : message.content ? (
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
        const toolCall = message.toolCalls?.find((tc) => tc.id === result.toolCallId);
        const toolName = toolCall?.name || 'unknown';
        if (toolName === 'delegate_to') {
          const args = toolCall?.arguments || {};
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
