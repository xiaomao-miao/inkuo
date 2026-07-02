import React, { useState } from 'react';
import { Check, Loader2, X, ChevronDown, ChevronRight, Users } from 'lucide-react';
import { getExpertDisplayName, getToolDisplayName } from './toolUtils';
import styles from './ToolCallCard.module.css';

interface DelegateToCardProps {
  id: string;
  expert: string;
  task: string;
  status: 'pending' | 'executing' | 'success' | 'error';
  result?: string;
  error?: string;
  duration?: number;
}

/**
 * Renders a `delegate_to` tool call as a specialized card. The user-facing
 * semantics are different from a generic tool call — instead of "what file
 * did it touch", we show "which expert was consulted, with what task".
 *
 * Sub-agent intermediate events arrive under a different `message_id`
 * (`sub:<expert>:<uuid>`) so we don't try to inline them here. They are
 * rendered by the generic event dispatcher as their own collapsible block.
 */
export const DelegateToCard: React.FC<DelegateToCardProps> = React.memo(function DelegateToCard({
  id,
  expert,
  task,
  status,
  result,
  error,
  duration,
}) {
  const [expanded, setExpanded] = useState(false);
  const isExecuting = status === 'executing';

  return (
    <div className={`${styles.card} ${styles[status]}`} data-tool-call-id={id}>
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
          {isExecuting && (
            <>
              <Loader2 size={12} className={styles.spinning} />
              <span>子代理执行中</span>
            </>
          )}
          {!isExecuting && status === 'success' && (
            <>
              <Check size={12} />
              <span>完成</span>
            </>
          )}
          {!isExecuting && status === 'error' && (
            <>
              <X size={12} />
              <span>失败</span>
            </>
          )}
          {duration !== undefined && (
            <span className={styles.duration}>{duration}ms</span>
          )}
        </div>
      </div>

      <div className={styles.previewSection}>
        <button
          type="button"
          className={styles.previewToggle}
          onClick={() => setExpanded((v) => !v)}
          aria-label={expanded ? '收起详情' : '展开详情'}
        >
          {expanded ? <ChevronDown size={12} /> : <ChevronRight size={12} />}
          <span>任务：{task.slice(0, 80)}{task.length > 80 ? '…' : ''}</span>
        </button>
        {expanded && (
          <div className={styles.previewContainer} style={{ maxHeight: '160px' }}>
            <div className={styles.previewContent}>
              <div style={{ marginBottom: 8 }}>
                <strong>专家：</strong>
                <code>{getExpertDisplayName(expert)}</code>
                <span style={{ color: 'var(--text-muted, #888)', marginLeft: 8 }}>
                  ({expert})
                </span>
              </div>
              <div style={{ marginBottom: 8 }}>
                <strong>任务：</strong>
                <pre style={{ whiteSpace: 'pre-wrap', marginTop: 4 }}>{task}</pre>
              </div>
              {result && !error && (
                <div>
                  <strong>结果：</strong>
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
        )}
      </div>
    </div>
  );
});

/**
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
