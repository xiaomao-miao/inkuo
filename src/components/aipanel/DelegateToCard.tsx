import React, { useState } from 'react';
import { ChevronDown, ChevronRight, Check, Loader2, X, Users, Bot, FileText, BrainCircuit, Wrench } from 'lucide-react';
import { TIMING } from '../../constants/timing';
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
 * Renders a `delegate_to` tool call as a specialized card.
 * Memoized to prevent re-renders when subagentActivities content hasn't changed.
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
  // Three-state `userToggled`:
  //   - `null`     : no manual override (auto-follows the running rule)
  //   - `true`/`false`: user explicitly toggled the card
  //
  // While a sub-agent is running, the card defaults to OPEN. Once every
  // sub-agent has finished, the card defaults to COLLAPSED. The first
  // running-state edge clears any stale preference so a finished card from
  // the previous run doesn't carry over.
  const [userToggled, setUserToggled] = useState<boolean | null>(null);
  const isRunning = subagentActivities?.some(a => a.status === 'running') ?? false;
  const effectiveStatus = isRunning ? 'executing' : status;
  const isWorking = effectiveStatus === 'pending' || effectiveStatus === 'executing';
  const previousRunningRef = React.useRef(false);
  React.useEffect(() => {
    if (isRunning && !previousRunningRef.current) {
      setUserToggled(null);
    }
    previousRunningRef.current = isRunning;
  }, [isRunning]);
  const cardExpanded = isRunning ? (userToggled ?? true) : (userToggled ?? false);

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
          {isWorking && (
            <>
              <Loader2 size={12} className={styles.spinning} />
              <span>{effectiveStatus === 'pending' ? '等待执行' : '执行中'}</span>
            </>
          )}
          {!isWorking && status === 'success' && (
            <>
              <Check size={12} />
              <span>完成</span>
            </>
          )}
          {!isWorking && status === 'error' && (
            <>
              <X size={12} />
              <span>失败</span>
            </>
          )}
          {duration !== undefined && (
            <span className={styles.duration}>{duration}ms</span>
          )}
          <button
            type="button"
            className={styles.expandBtn}
            onClick={() => setUserToggled(v => !(v ?? cardExpanded))}
            aria-label={cardExpanded ? '收起详情' : '展开详情'}
          >
            {cardExpanded ? <ChevronDown size={12} /> : <ChevronRight size={12} />}
          </button>
        </div>
      </div>

      {/* 展开后的详细内容 */}
      {cardExpanded && (
        <>
          {/* 任务（提示词）- 默认折叠 */}
          <PromptSection task={task} />

          {/* 子代理活动 */}
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

          {/* 结果 */}
          {result && !error && (
            <div className={styles.delegateResult}>
              <strong>结果：</strong>
              <pre style={{ whiteSpace: 'pre-wrap', marginTop: 4, fontSize: '12px' }}>{result}</pre>
            </div>
          )}

          {error && (
            <div className={styles.error}>
              <span className={styles.errorLabel}>错误:</span>
              <pre className={styles.errorContent}>{error}</pre>
            </div>
          )}
        </>
      )}
    </div>
  );
}, (prevProps, nextProps) => {
  // Only re-render if the status, result, or subagentActivities actually changed.
  // This prevents cascading re-renders when parent passes a new array reference
  // for subagentActivities but the content hasn't changed.
  if (prevProps.id !== nextProps.id) return false;
  if (prevProps.expert !== nextProps.expert) return false;
  if (prevProps.task !== nextProps.task) return false;
  if (prevProps.status !== nextProps.status) return false;
  if (prevProps.result !== nextProps.result) return false;
  if (prevProps.error !== nextProps.error) return false;
  if (prevProps.duration !== nextProps.duration) return false;

  // For arrays, check length and key elements instead of reference equality
  const prevActivities = prevProps.subagentActivities;
  const nextActivities = nextProps.subagentActivities;
  if (prevActivities === nextActivities) return true;
  if (!prevActivities || !nextActivities) return false;
  if (prevActivities.length !== nextActivities.length) return false;

  // Check if any activity content changed
  for (let i = 0; i < prevActivities.length; i++) {
    const prev = prevActivities[i];
    const next = nextActivities[i];
    if (prev.id !== next.id) return false;
    if (prev.status !== next.status) return false;
    if (prev.expanded !== next.expanded) return false;
    if (prev.summary !== next.summary) return false;
  }

  return true;
});

// 任务（提示词）区域 - 默认折叠
interface PromptSectionProps {
  task: string;
}

const PromptSection: React.FC<PromptSectionProps> = React.memo(function PromptSection({ task }) {
  const [expanded, setExpanded] = useState(false);

  return (
    <div className={styles.promptSection}>
      <button
        type="button"
        className={styles.promptToggle}
        onClick={() => setExpanded(v => !v)}
      >
        {expanded ? <ChevronDown size={12} /> : <ChevronRight size={12} />}
        <FileText size={12} />
        <span>任务详情</span>
      </button>
      {expanded && (
        <div className={styles.promptContent}>
          <pre style={{ whiteSpace: 'pre-wrap', margin: 0 }}>{task}</pre>
        </div>
      )}
    </div>
  );
});

// 子代理活动条目
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
      {/* 子代理头部 - 点击可折叠展开 */}
      <div
        className={styles.subagentHeader}
        onClick={onToggle}
        style={{ cursor: onToggle ? 'pointer' : 'default' }}
      >
        <Bot size={12} />
        <span className={styles.subagentLabel}>{activity.label}</span>
        {isRunning && <Loader2 size={10} className={styles.spinning} />}
        {isCompleted && <Check size={10} />}
        {isError && <X size={10} />}
        {onToggle && (
          activity.expanded ? <ChevronDown size={10} /> : <ChevronRight size={10} />
        )}
      </div>

      {/* 子代理展开内容 */}
      {activity.expanded && (
        <>
          {/* 子代理任务 */}
          <SubagentTask task={activity.task} />

          {/* 输出项 */}
          {activity.outputItems.map((item, idx) => (
            <SubagentOutputItem key={`sub-out-${idx}`} item={item} />
          ))}

          {/* 总结 */}
          {isCompleted && activity.summary && (
            <div className={styles.subagentSummary}>
              <pre style={{ whiteSpace: 'pre-wrap', fontSize: '11px', margin: 0 }}>
                {activity.summary.length > TIMING.SUBAGENT_SUMMARY_PREVIEW_CHARS ? activity.summary.slice(0, TIMING.SUBAGENT_SUMMARY_PREVIEW_CHARS) + '...' : activity.summary}
              </pre>
            </div>
          )}

          {/* 错误 */}
          {isError && activity.error && (
            <div className={styles.error}>
              <span className={styles.errorLabel}>错误:</span>
              <pre className={styles.errorContent}>{activity.error}</pre>
            </div>
          )}
        </>
      )}
    </div>
  );
});

// 子代理任务区域 - 默认折叠
interface SubagentTaskProps {
  task: string;
}

const SubagentTask: React.FC<SubagentTaskProps> = React.memo(function SubagentTask({ task }) {
  const [expanded, setExpanded] = useState(false);

  return (
    <div className={styles.subagentTask}>
      <button
        type="button"
        className={styles.taskToggle}
        onClick={() => setExpanded(v => !v)}
      >
        {expanded ? <ChevronDown size={10} /> : <ChevronRight size={10} />}
        <FileText size={10} />
        <span>任务</span>
      </button>
      {expanded && (
        <div className={styles.taskContent}>
          <pre style={{ whiteSpace: 'pre-wrap', margin: 0 }}>{task}</pre>
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
        <span>思考: {item.content.length > 100 ? item.content.slice(0, 100) + '...' : item.content}</span>
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
 * Tiny inline indicator for `get_tool_help`.
 */
export const GetToolHelpCard: React.FC<{
  id: string;
  spec: string;
  status: 'pending' | 'executing' | 'success' | 'error';
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
