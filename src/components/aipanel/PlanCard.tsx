import React, { useEffect, useMemo, useState } from 'react';
import {
  CheckCircle2,
  AlertTriangle,
  AlertOctagon,
  FilePlus2,
  FileEdit,
  FileMinus2,
  FileSearch,
  FileSymlink,
  Loader2,
  ChevronDown,
  ChevronRight,
  Sparkles,
  Save,
  X,
} from 'lucide-react';
import type { PlanFileIntent, PlanFileTouch, PlanOutput, PlanRisk } from '../../types';
import { MarkdownRenderer } from './MarkdownRenderer';
import { StreamingMarkdownRenderer } from './StreamingMarkdownRenderer';
import styles from './PlanCard.module.css';

interface PlanCardProps {
  /** The full raw text emitted by the model (Markdown + ```plan block). */
  rawText: string;
  /** Parsed plan, or null when the ```plan block is open or JSON is invalid. */
  plan: PlanOutput | null;
  /** Parse error string, if the ```plan block closed but JSON was invalid. */
  parseError?: string;
  /** True while the model is still streaming the plan. */
  isStreaming?: boolean;
  /** Owning message id — surfaced to onApply / onAdjust so the parent
   *  action handler can locate the corresponding plan item to destroy. */
  messageId: string;
  /**
   * Click handler for "Apply" — flips session to agent mode and triggers
   * run. Receives the messageId so the parent action can resolve the
   * matching plan item and tear down its `.md` artifact.
   */
  onApply?: (messageId: string, plan: PlanOutput) => void;
  /**
   * Click handler for "Adjust" — fills the input with a hint. Same
   * messageId forwarding applies (currently unused by the parent action
   * but kept symmetric to onApply).
   */
  onAdjust?: (messageId: string, plan: PlanOutput) => void;
  /**
   * Click handler for the "保存到 .inkuo/plans/" button. Persists the plan
   * to disk and stamps `planFileId` / `planFilePath` back onto the item.
   * Tapping it again is a no-op — `savedFilePath` renders a confirmation
   * pill once the write succeeds.
   */
  onSave?: () => void;
  /** Absolute path to the persisted plan md on disk (set after onSave). */
  savedFilePath?: string;
  /** Click handler for file paths inside the details section. */
  onFileClick?: (filePath: string) => void;
  /** Workspace root for resolving relative file paths. */
  workspacePath?: string;
}

const RISK_LABELS: Record<PlanRisk, string> = {
  low: '低风险',
  medium: '中等风险',
  high: '高风险',
};

const INTENT_META: Record<
  PlanFileIntent,
  { label: string; icon: React.ComponentType<{ size?: number }>; tone: string }
> = {
  read: { label: '读取', icon: FileSearch, tone: 'intentRead' },
  create: { label: '创建', icon: FilePlus2, tone: 'intentCreate' },
  modify: { label: '修改', icon: FileEdit, tone: 'intentModify' },
  delete: { label: '删除', icon: FileMinus2, tone: 'intentDelete' },
  rename: { label: '重命名', icon: FileSymlink, tone: 'intentRename' },
};

function isDestructiveIntent(intent: PlanFileIntent): boolean {
  return intent === 'delete' || intent === 'rename';
}

function summarizeFiles(files: PlanFileTouch[]): string {
  const destructive = files.filter((f) => isDestructiveIntent(f.intent)).length;
  const create = files.filter((f) => f.intent === 'create').length;
  const parts: string[] = [`影响 ${files.length} 个文件`];
  if (destructive > 0) parts.push(`其中 ${destructive} 个删除/重命名`);
  else if (create > 0) parts.push(`其中 ${create} 个新建`);
  return parts.join(' · ');
}

/**
 * Cursor-style plan card. Renders the structured plan as a compact
 * overview with a collapsible details section showing the model's free-form
 * Markdown prose. Supports three states:
 *   - "Streaming" — block open or no plan yet: render the prose as a
 *     streaming markdown view + a "正在解析计划..." pill.
 *   - "Parsed" — plan object present: render the full structured card.
 *   - "Failed" — block closed but JSON invalid: render the error + raw
 *     text fallback, no apply button.
 */
export const PlanCard: React.FC<PlanCardProps> = ({
  rawText,
  plan,
  parseError,
  isStreaming,
  messageId,
  onApply,
  onAdjust,
  onSave,
  savedFilePath,
  onFileClick,
  workspacePath,
}) => {
  const [detailsOpen, setDetailsOpen] = useState(false);
  const [savePending, setSavePending] = useState(false);
  const [saveError, setSaveError] = useState<string | null>(null);

  // When the underlying plan item gets re-streamed (re-edit), the
  // `savedFilePath` prop clears. Reset the local "saving..." / error state
  // so the save button becomes useful again.
  useEffect(() => {
    if (!savedFilePath) {
      setSavePending(false);
      setSaveError(null);
    }
  }, [savedFilePath]);

  const handleSaveClick = async () => {
    if (!onSave || savePending) return;
    setSavePending(true);
    setSaveError(null);
    try {
      await onSave();
    } catch (err) {
      setSaveError(err instanceof Error ? err.message : String(err));
    } finally {
      setSavePending(false);
    }
  };

  // Split the raw text into a "details" Markdown section and skip the JSON
  // block — reuses the same parser the streaming reducer uses so the
  // visible Markdown and the parsed plan stay in lockstep.
  const detailsMarkdown = useMemo(() => {
    if (!plan) return null;
    const fenceIdx = rawText.lastIndexOf('```plan');
    if (fenceIdx === -1) return null;
    return rawText.slice(0, fenceIdx).trim();
  }, [plan, rawText]);

  // Failed state: no plan and a parse error → show error + raw text.
  if (parseError && !plan) {
    return (
      <div className={`${styles.card} ${styles.failed}`}>
        <div className={styles.header}>
          <div className={styles.headerLeft}>
            <div className={`${styles.icon} ${styles.iconError}`}>
              <AlertOctagon size={14} />
            </div>
            <span className={styles.title}>解析计划失败</span>
          </div>
        </div>
        <div className={styles.body}>
          <div className={styles.errorText}>
            <X size={12} />
            <span>{parseError}</span>
          </div>
          {rawText && (
            <pre className={styles.rawFallback}>{rawText}</pre>
          )}
        </div>
      </div>
    );
  }

  // Pending state: plan has been finalised by the `create_plan` tool but
  // the AI's turn isn't done yet. Don't reveal the full card yet — wait
  // for the `done` event (`finishPlanItem` flips isStreaming=false).
  if (plan && isStreaming) {
    return (
      <div className={`${styles.card} ${styles.streaming}`}>
        <div className={styles.header}>
          <div className={styles.headerLeft}>
            <div className={`${styles.icon} ${styles.iconPlanning}`}>
              <Sparkles size={14} />
            </div>
            <span className={styles.title}>正在整理计划</span>
            <Loader2 size={12} className={styles.spinning} />
          </div>
        </div>
      </div>
    );
  }

  // Streaming state: no plan yet, but the model is still emitting.
  if (!plan) {
    return (
      <div className={`${styles.card} ${styles.streaming}`}>
        <div className={styles.header}>
          <div className={styles.headerLeft}>
            <div className={`${styles.icon} ${styles.iconPlanning}`}>
              <Sparkles size={14} />
            </div>
            <span className={styles.title}>正在规划</span>
            {isStreaming && <Loader2 size={12} className={styles.spinning} />}
          </div>
        </div>
        {rawText && (
          <div className={styles.body}>
            <StreamingMarkdownRenderer
              content={rawText}
              isStreaming={!!isStreaming}
              onFileClick={onFileClick}
              workspacePath={workspacePath}
            />
          </div>
        )}
      </div>
    );
  }

  // Parsed state.
  const destructiveCount = plan.files_to_touch.filter((f) => isDestructiveIntent(f.intent)).length;
  const applyLabel =
    destructiveCount > 0
      ? `确认并应用到这些文件（含 ${destructiveCount} 个删除/重命名）`
      : '确认并应用到这些文件';

  return (
    <div className={`${styles.card} ${styles.parsed}`}>
      <div className={styles.header}>
        <div className={styles.headerLeft}>
          <div className={`${styles.icon} ${styles.iconPlanning}`}>
            <Sparkles size={14} />
          </div>
          <div className={styles.titleBlock}>
            <span className={styles.title}>计划</span>
            <span className={styles.summary}>{plan.plan_summary}</span>
          </div>
        </div>
        <div className={styles.headerRight}>
          <RiskBadge risk={plan.risk} />
        </div>
      </div>

      <div className={styles.body}>
        <div className={styles.fileSummary}>{summarizeFiles(plan.files_to_touch)}</div>

        <ul className={styles.fileList}>
          {plan.files_to_touch.map((f, idx) => {
            const meta = INTENT_META[f.intent] ?? INTENT_META.modify;
            const Icon = meta.icon;
            const destructive = isDestructiveIntent(f.intent);
            return (
              <li
                key={`${f.path}-${idx}`}
                className={`${styles.fileRow} ${styles[meta.tone]} ${
                  destructive ? styles.fileRowDestructive : ''
                }`}
              >
                <span className={styles.fileIcon}>
                  <Icon size={12} />
                </span>
                <button
                  type="button"
                  className={styles.filePath}
                  onClick={() => onFileClick?.(f.path)}
                  title={f.path}
                >
                  {f.path}
                </button>
                <span className={styles.intentTag}>{meta.label}</span>
                <span className={styles.fileReason}>{f.reason}</span>
                {destructive && (
                  <AlertTriangle size={12} className={styles.destructiveIcon} />
                )}
              </li>
            );
          })}
        </ul>

        {plan.risk_reason && (
          <div className={`${styles.riskNote} ${styles[`riskNote_${plan.risk}`]}`}>
            <AlertTriangle size={12} />
            <span>风险: {RISK_LABELS[plan.risk]} — {plan.risk_reason}</span>
          </div>
        )}

        {detailsMarkdown && detailsMarkdown.length > 0 && (
          <div className={styles.details}>
            <button
              type="button"
              className={styles.detailsToggle}
              onClick={() => setDetailsOpen((v) => !v)}
            >
              {detailsOpen ? <ChevronDown size={12} /> : <ChevronRight size={12} />}
              <span>{detailsOpen ? '收起详情' : '查看详情'}</span>
            </button>
            {detailsOpen && (
              <div className={styles.detailsContent}>
                {isStreaming ? (
                  <StreamingMarkdownRenderer
                    content={detailsMarkdown}
                    isStreaming={true}
                    onFileClick={onFileClick}
                    workspacePath={workspacePath}
                  />
                ) : (
                  <MarkdownRenderer
                    content={detailsMarkdown}
                    onFileClick={onFileClick}
                    workspacePath={workspacePath}
                  />
                )}
              </div>
            )}
          </div>
        )}
      </div>

      <div className={styles.actions}>
        <button
          type="button"
          className={styles.applyButton}
          onClick={() => onApply?.(messageId, plan)}
          disabled={!onApply}
        >
          <CheckCircle2 size={14} />
          <span>{applyLabel}</span>
        </button>
        <button
          type="button"
          className={styles.adjustButton}
          onClick={() => onAdjust?.(messageId, plan)}
          disabled={!onAdjust}
        >
          <span>调整计划</span>
        </button>
        {savedFilePath ? (
          <span
            className={styles.savedPill}
            title={savedFilePath}
          >
            <CheckCircle2 size={12} />
            <span>已保存到 .inkuo/plans/</span>
          </span>
        ) : (
          <button
            type="button"
            className={styles.saveButton}
            onClick={handleSaveClick}
            disabled={!onSave || savePending}
          >
            {savePending ? (
              <Loader2 size={12} className={styles.spinning} />
            ) : (
              <Save size={12} />
            )}
            <span>{savePending ? '保存中...' : '保存到 .inkuo/plans/'}</span>
          </button>
        )}
        {saveError && (
          <span className={styles.saveError} title={saveError}>
            <X size={12} />
            <span>保存失败：{saveError}</span>
          </span>
        )}
      </div>
    </div>
  );
};

const RiskBadge: React.FC<{ risk: PlanRisk }> = ({ risk }) => {
  const icon =
    risk === 'high' ? <AlertOctagon size={12} /> : risk === 'medium' ? <AlertTriangle size={12} /> : <CheckCircle2 size={12} />;
  return (
    <span className={`${styles.riskBadge} ${styles[`riskBadge_${risk}`]}`}>
      {icon}
      <span>{RISK_LABELS[risk]}</span>
    </span>
  );
};
