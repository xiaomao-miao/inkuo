import React from 'react';
import { Loader2, Pencil, X, RotateCcw, FileText } from 'lucide-react';
import { MarkdownRenderer } from './MarkdownRenderer';
import { StreamingMarkdownRenderer } from './StreamingMarkdownRenderer';
import { ToolCallCard, CompactToolCard, COMPACT_TOOLS } from './ToolCallCard';
import { InlineDiffPreview } from './InlineDiffPreview';
import { parsePlanBlocks, type PlanBlock } from './planRender';
import {
  useEditorStore,
  useSidebarStore,
  type ChatMessage,
  type ChatSession,
  type ChatMode,
  type OutputItem,
  type ActiveToolCall,
  type SearchResult,
} from '../../store';
import styles from './AIPanel.module.css';

interface MessageItemProps {
  message: ChatMessage;
  isStreaming: boolean;
  mode: ChatMode;
  activeToolCalls: ActiveToolCall[];
  activeSession: ChatSession | undefined;
  editingMessageId: string | null;
  editingContent: string;
  onStartEdit: (id: string, content: string) => void;
  onCancelEdit: () => void;
  onSaveEdit: () => void;
  onSetEditingContent: (v: string) => void;
  onSetInput: (v: string) => void;
}

export const MessageItem: React.FC<MessageItemProps> = ({
  message,
  isStreaming,
  mode,
  activeToolCalls,
  activeSession,
  editingMessageId,
  editingContent,
  onStartEdit,
  onCancelEdit,
  onSaveEdit,
  onSetEditingContent,
  onSetInput,
}) => {
  if (message.role === 'user') {
    const isEditing = editingMessageId === message.id;
    return (
      <div className={`${styles.message} ${styles.user}`}>
        <div className={styles.messageBubble}>
          {isEditing ? (
            <div className={styles.editMode}>
              <textarea
                className={styles.editTextarea}
                value={editingContent}
                onChange={(e) => {
                  onSetEditingContent(e.target.value);
                  onSetInput(e.target.value);
                }}
                autoFocus
              />
              <div className={styles.editActions}>
                <button
                  className={styles.editCancelBtn}
                  onClick={onCancelEdit}
                  title="取消"
                  type="button"
                >
                  <X size={12} />
                  取消
                </button>
                <button
                  className={styles.editSaveBtn}
                  onClick={onSaveEdit}
                  disabled={!editingContent.trim()}
                  title="重新发送"
                  type="button"
                >
                  <RotateCcw size={12} />
                  重新发送
                </button>
              </div>
            </div>
          ) : (
            <>
              <div className={styles.messageText}>{message.content}</div>
              {!isStreaming && (
                <button
                  className={styles.editBtn}
                  onClick={() => onStartEdit(message.id, message.content || '')}
                  title="编辑并重新发送"
                  type="button"
                >
                  <Pencil size={12} />
                </button>
              )}
            </>
          )}
        </div>
      </div>
    );
  }

  if (message.role === 'tool') {
    return null; // Tool messages rendered as part of assistant outputItems
  }

  if (message.role === 'assistant') {
    const streamingMessageId = activeSession?.messages
      .slice().reverse().find(m => m.role === 'assistant')?.id;
    const isThisStreaming = isStreaming && message.id === streamingMessageId;
    const hasOutputItems = message.outputItems && message.outputItems.length > 0;

    return (
      <div className={`${styles.message} ${styles.assistant}`}>
        <div className={styles.messageContent}>
          {hasOutputItems ? (
            message.outputItems.map((item, idx) => (
              <OutputItemView
                key={idx}
                item={item}
                message={message}
                isThisStreaming={isThisStreaming}
                isLastItem={idx === message.outputItems.length - 1}
              />
            ))
          ) : message.content ? (
            <LegacyMessageContent
              message={message}
              isThisStreaming={isThisStreaming}
              mode={mode}
              activeToolCalls={activeToolCalls}
            />
          ) : null}

          {message.diff && !isThisStreaming && activeSession && (
            <InlineDiffPreview
              originalText={message.diff.originalText}
              newText={message.diff.newText}
              sessionId={activeSession.id}
            />
          )}

          {!isThisStreaming && mode === 'knowledge' && message.searchResults?.length ? (
            <KnowledgeReferences results={message.searchResults} />
          ) : null}

          {isThisStreaming && !hasOutputItems && (
            <span className={styles.streamingCursor} />
          )}
        </div>
      </div>
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
  item, message, isThisStreaming, isLastItem,
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
    const isCompact = COMPACT_TOOLS.has(item.toolName);

    if (isCompact) {
      return (
        <div className={styles.toolResultItem}>
          {isThisStreaming && isLastItem && (
            <div className={styles.continueGenerating}>
              <span className={styles.continueDots}>
                <span className={styles.dot} />
                <span className={styles.dot} />
                <span className={styles.dot} />
              </span>
            </div>
          )}
          <CompactToolCard
            id={item.toolCallId}
            name={item.toolName}
            arguments={item.arguments}
            status={item.status || (item.isExecuting ? 'executing' : 'pending')}
            duration={item.duration}
          />
        </div>
      );
    }

    return (
      <div className={styles.toolResultItem}>
        {isThisStreaming && isLastItem && (
          <div className={styles.continueGenerating}>
            <span className={styles.continueDots}>
              <span className={styles.dot} />
              <span className={styles.dot} />
              <span className={styles.dot} />
            </span>
          </div>
        )}
        <ToolCallCard
          id={item.toolCallId}
          name={item.toolName}
          arguments={item.arguments}
          rawArguments={item.rawArguments}
          streamingContent={item.streamingContent}
          status={item.status || (item.isExecuting ? 'executing' : 'pending')}
          isStreamingArguments={item.isExecuting}
          result={item.result}
          duration={item.duration}
          diffSummary={item.diffSummary}
        />
      </div>
    );
  }

  if (item.type === 'tool_result') {
    // This item is now merged into tool_call_start, so we render it as hidden
    // to maintain backward compatibility. The actual rendering happens in tool_call_start.
    const toolCall = message.toolCalls?.find(tc => tc.id === item.toolCallId);
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

interface LegacyMessageContentProps {
  message: ChatMessage;
  isThisStreaming: boolean;
  mode: ChatMode;
  activeToolCalls: ActiveToolCall[];
}

const LegacyMessageContent: React.FC<LegacyMessageContentProps> = ({
  message, isThisStreaming, mode, activeToolCalls,
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
          <StreamingMarkdownRenderer
            content={message.content}
            isStreaming={true}
          />
        ) : (
          <MarkdownRenderer content={message.content} />
        )
      ) : !message.toolResults?.length && !isThisStreaming ? (
        <div className={styles.toolOnlyPlaceholder}>工具执行完成</div>
      ) : null}
      {message.toolResults?.map((result) => {
        const toolCall = message.toolCalls?.find(tc => tc.id === result.toolCallId);
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

interface KnowledgeReferencesProps {
  results: SearchResult[];
}

const KnowledgeReferences: React.FC<KnowledgeReferencesProps> = ({ results }) => {
  const openTab = useSidebarStore((state) => state.openTab);
  const setSelection = useEditorStore((state) => state.setSelection);

  const references = Array.from(
    new Map(results.map((result) => [result.filePath, result])).values(),
  );

  const lineStartOffset = (content: string, lineNumber: number) => {
    if (lineNumber <= 1) return 0;

    let currentLine = 1;
    for (let i = 0; i < content.length; i += 1) {
      if (currentLine === lineNumber) {
        return i;
      }
      if (content[i] === '\n') {
        currentLine += 1;
      }
    }

    return content.length;
  };

  const handleOpenReference = (result: SearchResult) => {
    openTab({
      id: crypto.randomUUID(),
      path: result.filePath,
      name: result.documentTitle || result.filePath.split('/').pop() || '未命名文档',
      isDirty: false,
    });

    const startLine = result.startLine;
    if (!startLine) return;

    window.setTimeout(() => {
      const docState = useEditorStore.getState().documentContents[result.filePath];
      const content = docState?.content;
      if (!content) return;

      const from = lineStartOffset(content, startLine);
      const to = lineStartOffset(content, (result.endLine ?? startLine) + 1);
      setSelection(result.filePath, { from, to });

      const editorView = document.querySelector('.cm-editor') as HTMLElement | null;
      const lineElement = editorView?.querySelector(`.cm-line:nth-child(${startLine})`) as HTMLElement | null;
      lineElement?.scrollIntoView({ block: 'center', behavior: 'smooth' });
    }, 80);
  };

  return (
    <div className={styles.referencesSection}>
      <div className={styles.referencesTitle}>参考来源</div>
      <div className={styles.referencesList}>
        {references.map((result) => (
          <button
            key={result.filePath}
            type="button"
            className={styles.referenceItem}
            onClick={() => handleOpenReference(result)}
            title={`打开 ${result.filePath}`}
          >
            <div className={styles.referenceHeader}>
              <span className={styles.referenceIcon}>
                <FileText size={14} />
              </span>
              <span className={styles.referenceName}>{result.documentTitle}</span>
              <span className={styles.referenceScore}>{(result.score * 100).toFixed(1)}%</span>
            </div>
            <div className={styles.referencePath}>{result.filePath}</div>
            {(result.startLine || result.endLine) && (
              <div className={styles.referenceRange}>
                第 {result.startLine ?? '?'} 行{result.endLine && result.endLine !== result.startLine ? ` - ${result.endLine} 行` : ''}
              </div>
            )}
          </button>
        ))}
      </div>
    </div>
  );
};
