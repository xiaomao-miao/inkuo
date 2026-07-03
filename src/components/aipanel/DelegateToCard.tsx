import React, { useState } from 'react';
import { Check, Loader2, X, ChevronDown, ChevronRight, Users, Bot, FileText, BrainCircuit, Wrench } from 'lucide-react';
import { getExpertDisplayName, getToolDisplayName } from './toolUtils';
import type { SubagentActivity as SubagentActivityType, OutputItem } from '../../types';
import styles from './ToolCallCard.module.css';

interface DelegateToCardProps {
  id: string;
  expert: string;
  task: string;
  status: 'pending' | 'executing' | 'success' | 'error';
  result?: string;
  error?: string;
  duration?: number;
  /** Nested sub-agent activities to render inside this card */
  subagentActivities?: SubagentActivityType[];
  /** Callback to toggle subagent activity expansion */
  onToggleSubagentActivity?: (subagentId: string) => void;
}

/**
 * Renders a `delegate_to` tool call as a specialized card. The user-facing
 * semantics are different from a generic tool call — instead of "what file
 * did it touch", we show "which expert was consulted, with what task".
 *
 * Sub-agent intermediate events are rendered inside this card when provided
 * via the `subagentActivities` prop.
 */
export const DelegateToCard: React.FC<DelegateToCardProps> = React.memo(function DelegateToCard({
  id,
  expert,
  task,
  status,
  result,
  error,
  duration,
  subagentActivities,
  onToggleSubagentActivity,
}) {
  const [expanded, setExpanded] = useState(false);
  const isRunning = subagentActivities?.some(a => a.status === 'running') ?? false;
  const effectiveStatus = isRunning ? 'executing' : status;

  return (
    <div className={`${styles.card} ${styles.delegateTo} ${styles[effectiveStatus]}`} data-tool-call-id={id}>
      <div className={styles.header}>
        <div className={styles.headerLeft}>
          <div className={styles.icon}>
            <Users size={14} />
          </div>
          <span className={styles.toolName}>
            委派给 {getExpertDisplayName(expert)}
          </span>
          <span className={styles.fileName}>{expert}</span>
        </div>
        <div className={styles.headerRight}>
          {isRunning && (
            <>
              <Loader2 size={12} className={styles.spinning} />
              <span>子代理执行中</span>
            </>
          )}
          {!isRunning && status === 'success' && (
            <>
              <Check size={12} />
              <span>完成</span>
            </>
          )}
          {!isRunning && status === 'error' && (
            <>
              <X size={12} />
              <span>失败</span>
            </>
          )}
          {duration !== undefined && (
            <span className={styles.duration}>{duration}ms</span>
          )}
          {(subagentActivities?.length ?? 0) > 0 && (
            <button
              type="button"
              className={styles.expandBtn}
              onClick={() => setExpanded(v => !v)}
              aria-label={expanded ? '收起详情' : '展开详情'}
            >
              {expanded ? <ChevronDown size={12} /> : <ChevronRight size={12} />}
            </button>
          )}
        </div>
      </div>

      {/* Sub-agent activities rendered inside the card */}
      {subagentActivities && subagentActivities.length > 0 && (
        <div className={styles.subagentActivities}>
          {subagentActivities.map((activity) => (
            <SubagentActivityItem
              key={activity.id}
              activity={activity}
              onToggle={onToggleSubagentActivity ? () => onToggleSubagentActivity(activity.id) : undefined}
            />
          ))}
        </div>
      )}

      {/* Task preview / result */}
      {expanded && (
        <div className={styles.previewSection}>
          <div className={styles.previewContainer}>
            <div className={styles.previewContent}>
              <div style={{ marginBottom: 8 }}>
                <strong>任务：</strong>
                <pre style={{ whiteSpace: 'pre-wrap', marginTop: 4 }}>{task}</pre>
              </div>
              {result && !error && (
                <div>
                  <strong>最终结果：</strong>
                  <pre style={{ whiteSpace: 'pre-wrap', marginTop: 4 }}>{result}</pre>
                </div>
              )}
              {error && (
                <div className={styles.error}>
                  <span className={styles.errorLabel}>错误:</span>
                  <pre className={styles.errorContent}>{error}</pre>
                </div>
              )}
            </div>
          </div>
        </div>
      )}
    </div>
  );
});

// Internal component for rendering sub-agent activity items
interface SubagentActivityItemProps {
  activity: SubagentActivityType;
  onToggle?: () => void;
}

const SubagentActivityItem: React.FC<SubagentActivityItemProps> = React.memo(function SubagentActivityItem({
  activity,
  onToggle,
}) {
  const isRunning = activity.status === 'running';
  const isCompleted = activity.status === 'completed';
  const isError = activity.status === 'error';

  return (
    <div className={`${styles.subagentItem} ${styles[activity.status]}`}>
      <div className={styles.subagentHeader} onClick={onToggle} style={{ cursor: onToggle ? 'pointer' : 'default' }}>
        <Bot size={12} />
        <span className={styles.subagentLabel}>{activity.label}</span>
        {isRunning && <Loader2 size={10} className={styles.spinning} />}
        {isCompleted && <Check size={10} />}
        {isError && <X size={10} />}
        {onToggle && (activity.expanded ? <ChevronDown size={10} /> : <ChevronRight size={10} />)}
      </div>

      {/* Output items rendered inline */}
      {activity.outputItems.map((item, idx) => (
        <SubagentOutputItem key={idx} item={item} />
      ))}

      {/* Summary on completion */}
      {isCompleted && activity.summary && (
        <div className={styles.subagentSummary}>
          <pre style={{ whiteSpace: 'pre-wrap', fontSize: '11px', margin: 0 }}>
            {activity.summary.length > 300 ? activity.summary.slice(0, 300) + '...' : activity.summary}
          </pre>
        </div>
      )}
    </div>
  );
});

interface SubagentOutputItemProps {
  item: OutputItem;
}

const SubagentOutputItem: React.FC<SubagentOutputItemProps> = React.memo(function SubagentOutputItem({ item }) {
  if (item.type === 'text') {
    return (
      <div className={styles.subagentText}>
        <FileText size={10} />
        <span>{item.content.length > 150 ? item.content.slice(0, 150) + '...' : item.content}</span>
      </div>
    );
  }

  if (item.type === 'reasoning') {
    return (
      <div className={styles.subagentReasoning}>
        <BrainCircuit size={10} />
        <span>思考过程: {item.content.length > 100 ? item.content.slice(0, 100) + '...' : item.content}</span>
      </div>
    );
  }

  if (item.type === 'tool_call_start') {
    return (
      <div className={styles.subagentToolCall}>
        <Wrench size={10} />
        <span>{getToolDisplayName(item.toolName)}</span>
        {item.isExecuting && <Loader2 size={8} className={styles.spinning} />}
      </div>
    );
  }

  if (item.type === 'tool_result') {
    return (
      <div className={`${styles.subagentToolResult} ${item.status === 'error' ? styles.error : ''}`}>
        <span className={styles.toolResultLabel}>
          {item.status === 'error' ? <X size={8} /> : <Check size={8} />}
          工具结果
        </span>
        <pre style={{ fontSize: '10px', margin: 0, whiteSpace: 'pre-wrap' }}>
          {item.result.length > 100 ? item.result.slice(0, 100) + '...' : item.result}
        </pre>
      </div>
    );
  }

  return null;
});

/**
 * Tiny inline indicator for `get_tool_help`. Shows only that a category of
 * help was loaded into the agent's context — NOT the spec contents.
 * Spec text is internal infrastructure (injected into LLM context);
 * exposing it to users would leak prompt engineering details and feel
 * noisy in the chat stream.
 */
export const GetToolHelpCard: React.FC<{
  id: string;
  spec: string;
  status: 'pending' | 'executing' | 'success' | 'error';
  /** Internal LLM context — not rendered. Kept on the type so callers
   *  can keep passing it; we just don't read it here. */
  result?: string;
  duration?: number;
}> = React.memo(function GetToolHelpCard({ id, spec, status, duration }) {
  return (
    <div className={`${styles.card} ${styles[status]}`} data-tool-call-id={id}>
      <div className={styles.header}>
        <div className={styles.headerLeft}>
          <div className={styles.icon}>
            <ChevronDown size={14} />
          </div>
          <span className={styles.toolName}>
            {getToolDisplayName('get_tool_help')}
          </span>
          <span className={styles.fileName}>已加载「{spec}」帮助</span>
        </div>
        <div className={styles.headerRight}>
          {status === 'success' && <Check size={12} />}
          {status === 'executing' && <Loader2 size={12} className={styles.spinning} />}
          {status === 'error' && <X size={12} />}
          {duration !== undefined && <span className={styles.duration}>{duration}ms</span>}
        </div>
      </div>
    </div>
  );
});
