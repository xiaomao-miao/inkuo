import React, { useState } from 'react';
import { ChevronDown, ChevronRight, Loader2, Check, X, Bot, FileText, BrainCircuit, Wrench } from 'lucide-react';
import type { SubagentActivity as SubagentActivityType, OutputItem } from '../../types';
import { getExpertDisplayName, getToolDisplayName } from './toolUtils';
import styles from './ToolCallCard.module.css';

interface SubagentActivityProps {
  activity: SubagentActivityType;
  onToggleExpand: () => void;
}

export const SubagentActivity: React.FC<SubagentActivityProps> = React.memo(function SubagentActivity({
  activity,
  onToggleExpand,
}) {
  const isRunning = activity.status === 'running';
  const isCompleted = activity.status === 'completed';
  const isError = activity.status === 'error';

  return (
    <div className={`${styles.card} ${styles.subagent} ${styles[activity.status]}`}>
      <div className={styles.header} onClick={onToggleExpand} style={{ cursor: 'pointer' }}>
        <div className={styles.headerLeft}>
          <div className={styles.icon}>
            <Bot size={14} />
          </div>
          <span className={styles.toolName}>
            {getExpertDisplayName(activity.expert)}
          </span>
          <span className={styles.fileName}>{activity.label}</span>
        </div>
        <div className={styles.headerRight}>
          {isRunning && (
            <>
              <Loader2 size={12} className={styles.spinning} />
              <span>运行中</span>
            </>
          )}
          {isCompleted && (
            <>
              <Check size={12} />
              <span>完成</span>
            </>
          )}
          {isError && (
            <>
              <X size={12} />
              <span>失败</span>
            </>
          )}
          {activity.expanded ? <ChevronDown size={12} /> : <ChevronRight size={12} />}
        </div>
      </div>

      {activity.expanded && (
        <>
          <div className={styles.previewSection}>
            <div className={styles.previewContainer} style={{ maxHeight: '120px' }}>
              <div className={styles.previewContent}>
                <div style={{ marginBottom: 8 }}>
                  <strong>任务：</strong>
                  <pre style={{ whiteSpace: 'pre-wrap', marginTop: 4, fontSize: '12px' }}>
                    {activity.task}
                  </pre>
                </div>
              </div>
            </div>
          </div>

          {/* Nested output items */}
          <div className={styles.subagentActivity}>
            {activity.outputItems.map((item, index) => (
              <SubagentOutputItem key={index} item={item} />
            ))}
          </div>

          {/* Summary/error on completion */}
          {isCompleted && activity.summary && (
            <div className={styles.subagentSummary}>
              <strong>总结：</strong>
              <pre style={{ whiteSpace: 'pre-wrap', fontSize: '12px', marginTop: 4 }}>
                {activity.summary}
              </pre>
            </div>
          )}

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

interface SubagentOutputItemProps {
  item: OutputItem;
}

const SubagentOutputItem: React.FC<SubagentOutputItemProps> = React.memo(function SubagentOutputItem({ item }) {
  const [expanded, setExpanded] = useState(false);

  if (item.type === 'text') {
    return (
      <div className={styles.subagentText}>
        <FileText size={12} />
        <span>{item.content}</span>
      </div>
    );
  }

  if (item.type === 'reasoning') {
    return (
      <div className={styles.subagentReasoning}>
        <button
          type="button"
          className={styles.reasoningToggle}
          onClick={() => setExpanded(!expanded)}
        >
          <BrainCircuit size={12} />
          {expanded ? <ChevronDown size={12} /> : <ChevronRight size={12} />}
          <span>思考过程</span>
        </button>
        {expanded && (
          <pre className={styles.reasoningContent}>
            {item.content}
          </pre>
        )}
      </div>
    );
  }

  if (item.type === 'tool_call_start') {
    return (
      <div className={styles.subagentToolCall}>
        <Wrench size={12} />
        <span className={styles.toolName}>{getToolDisplayName(item.toolName)}</span>
        {item.isExecuting && <Loader2 size={10} className={styles.spinning} />}
      </div>
    );
  }

  if (item.type === 'tool_result') {
    return (
      <div className={`${styles.subagentToolResult} ${item.status === 'error' ? styles.error : ''}`}>
        <div className={styles.toolResultHeader}>
          {item.status === 'error' ? <X size={10} /> : <Check size={10} />}
          <span>工具结果</span>
        </div>
        <pre className={styles.toolResultContent}>
          {item.result.length > 200 ? item.result.slice(0, 200) + '...' : item.result}
        </pre>
      </div>
    );
  }

  return null;
});
