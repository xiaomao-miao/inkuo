import React from 'react';
import { Loader2, X } from 'lucide-react';
import { MarkdownRenderer } from './MarkdownRenderer';
import { StreamingMarkdownRenderer } from './StreamingMarkdownRenderer';
import { ToolCallCard } from './ToolCallCard';
import { CompactToolCard } from './CompactToolCard';
import { COMPACT_TOOLS } from './toolUtils';
import { parsePlanBlocks, type PlanBlock } from './planRender';
import type { ActiveToolCall, ChatMessage, ChatMode, OutputItem } from '../../store';
import styles from './AIPanelMessage.module.css';

interface AssistantMessageBodyProps {
  message: ChatMessage;
  isThisStreaming: boolean;
  mode: ChatMode;
  activeToolCalls: ActiveToolCall[];
}

export const AssistantMessageBody: React.FC<AssistantMessageBodyProps> = ({
  message,
  isThisStreaming,
  mode,
  activeToolCalls,
}) => {
  const hasOutputItems = message.outputItems && message.outputItems.length > 0;

  if (hasOutputItems) {
    return (
      <>
        {message.outputItems.map((item, idx) => (
          <OutputItemView
            key={idx}
            item={item}
            message={message}
            isThisStreaming={isThisStreaming}
            isLastItem={idx === message.outputItems.length - 1}
          />
        ))}
      </>
    );
  }

  if (message.content) {
    return (
      <LegacyMessageContent
        message={message}
        isThisStreaming={isThisStreaming}
        mode={mode}
        activeToolCalls={activeToolCalls}
      />
    );
  }

  return null;
};

interface OutputItemViewProps {
  item: OutputItem;
  message: ChatMessage;
  isThisStreaming: boolean;
  isLastItem: boolean;
}

const OutputItemView: React.FC<OutputItemViewProps> = ({
  item,
  message,
  isThisStreaming,
  isLastItem,
}) => {
  if (item.type === 'text') {
    return (
      <div className={styles.outputTextItem}>
        {item.isPendingMarkdown ? (
          <StreamingMarkdownRenderer
            content={item.content}
            isStreaming={isThisStreaming}
          />
        ) : (
          <MarkdownRenderer content={item.content} />
        )}
      </div>
    );
  }

  if (item.type === 'tool_call_start') {
    const status = item.status || (item.isExecuting ? 'executing' : 'pending');

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
          />
        )}
      />
    );
  }

  if (item.type === 'tool_result') {
    const toolCall = message.toolCalls?.find((tc) => tc.id === item.toolCallId);
    return (
      <div className={styles.toolResultItem} style={{ display: 'none' }}>
        <ToolCallCard
          id={item.toolCallId}
          name={toolCall?.name || 'unknown'}
          arguments={toolCall?.arguments || {}}
          status={item.status}
          result={item.result}
          error={item.status === 'error' ? item.result : undefined}
          duration={item.duration}
          diffSummary={item.diffSummary}
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
  isThisStreaming: boolean;
  mode: ChatMode;
  activeToolCalls: ActiveToolCall[];
}

const LegacyMessageContent: React.FC<LegacyMessageContentProps> = ({
  message,
  isThisStreaming,
  mode,
  activeToolCalls,
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
        isThisStreaming ? (
          <StreamingMarkdownRenderer content={message.content} isStreaming={true} />
        ) : (
          <MarkdownRenderer content={message.content} />
        )
      ) : !message.toolResults?.length && !isThisStreaming ? (
        <div className={styles.toolOnlyPlaceholder}>工具执行完成</div>
      ) : null}
      {message.toolResults?.map((result) => {
        const toolCall = message.toolCalls?.find((tc) => tc.id === result.toolCallId);
        return (
          <div key={`tool-${result.toolCallId}`} className={styles.toolResultItem}>
            <ToolCallCard
              id={result.toolCallId}
              name={toolCall?.name || 'unknown'}
              arguments={toolCall?.arguments || {}}
              status={result.isError ? 'error' : 'success'}
              result={result.result}
              error={result.isError ? result.result : undefined}
              duration={result.duration}
              diffSummary={result.diffSummary}
            />
          </div>
        );
      })}
    </>
  );
};
