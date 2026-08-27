import React, { useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { Check, MessageCircleQuestion, X } from 'lucide-react';
import {
  useAIPanelStore,
  type AskUserAnswer,
  type AskUserQuestion,
} from '../../store';
import styles from './AskUserCard.module.css';

interface AskUserCardProps {
  sessionId: string;
  messageId: string;
  toolCallId: string;
  requestId: string;
  questions: AskUserQuestion[];
}

/**
 * Render an `ask_user` pause as a card of one or more questions. The
 * user picks one option (or types free text in the "Other" input) per
 * question, optionally skips an individual question, and finally hits
 * Submit / Cancel-all. On Submit we invoke the `ai_agent_resume`
 * backend command with the answers; the loop continues from the
 * parked session.
 *
 * Why this renders inline with the `tool_call_start` output item
 * instead of in a global spot: the questions are tied to a specific
 * turn in the conversation, so the user sees them right next to the
 * reason the agent paused. Submit/cancel persists an interaction state on
 * the owning output item, so the resolved summary survives panel folding and
 * history remounts.
 */
export const AskUserCard: React.FC<AskUserCardProps> = ({
  sessionId,
  messageId,
  toolCallId,
  requestId,
  questions,
}) => {
  const setPendingAsk = useAIPanelStore((s) => s.setPendingAsk);
  const clearPendingAsk = useAIPanelStore((s) => s.clearPendingAsk);
  const patchOutputItem = useAIPanelStore((s) => s.patchOutputItem);

  // Per-question selected-option indices + free-text "Other" input.
  // `Set` makes toggling multi-select options O(1) without an array
  // shuffle. We materialise to a sorted array on submit so the
  // backend gets a deterministic order.
  const [picks, setPicks] = useState<Record<number, Set<number>>>(() => ({}));
  const [customTexts, setCustomTexts] = useState<Record<number, string>>(() => ({}));
  const [submitting, setSubmitting] = useState(false);

  const buildAnswerSummary = () => {
    const picksAt = (idx: number) => picks[idx] ?? new Set<number>();
    return questions.map((q, idx) => {
      const chosen = picksAt(idx);
      const labels = q.options
        .map((opt, i) => (chosen.has(i) ? opt.label : null))
        .filter((x): x is string => x !== null);
      const custom = customTexts[idx]?.trim();
      return {
        question: q.question,
        labels,
        custom,
      };
    });
  };

  const setPick = (questionIdx: number, optionIdx: number) => {
    setPicks((prev) => {
      const current = new Set(prev[questionIdx] ?? []);
      const isMulti = questions[questionIdx]?.multiSelect ?? false;
      if (isMulti) {
        if (current.has(optionIdx)) current.delete(optionIdx);
        else current.add(optionIdx);
      } else {
        current.clear();
        current.add(optionIdx);
      }
      return { ...prev, [questionIdx]: current };
    });
  };

  const onSubmit = async () => {
    if (submitting) return;
    setSubmitting(true);
    try {
      const answers: AskUserAnswer[] = questions.map((q, idx) => {
        const chosen = Array.from(picks[idx] ?? []);
        const labels = chosen
          .map((i) => q.options[i]?.label)
          .filter((s): s is string => typeof s === 'string');
        const customText = customTexts[idx]?.trim();
        return {
          questionIndex: idx,
          selectedLabels: labels,
          ...(customText ? { customText } : {}),
        };
      });

      const interactionSummary = summariseAnswers(buildAnswerSummary());
      // Persist the resolution on the message itself before awaiting the
      // resumed agent run. That command may stream for minutes; the user
      // should immediately see that their answer was accepted.
      clearPendingAsk(sessionId, messageId);
      patchOutputItem(
        sessionId,
        messageId,
        { toolCallId },
        { interactionState: 'answered', interactionSummary },
      );

      await invoke('ai_agent_resume', {
        sessionId,
        requestId,
        answers,
        cancel: false,
      });
    } catch (err) {
      patchOutputItem(
        sessionId,
        messageId,
        { toolCallId },
        { interactionState: 'pending', interactionSummary: undefined },
      );
      setPendingAsk(sessionId, messageId, {
        sessionId,
        messageId,
        requestId,
        toolCallId,
        questions,
      });
      console.error('ai_agent_resume failed', err);
    }
  };

  const onCancel = async () => {
    if (submitting) return;
    setSubmitting(true);
    try {
      clearPendingAsk(sessionId, messageId);
      patchOutputItem(
        sessionId,
        messageId,
        { toolCallId },
        { interactionState: 'cancelled', interactionSummary: '用户已取消' },
      );
      await invoke('ai_agent_resume', {
        sessionId,
        requestId,
        answers: [],
        cancel: true,
      });
    } catch (err) {
      console.error('cancel ask_user failed', err);
      patchOutputItem(
        sessionId,
        messageId,
        { toolCallId },
        { interactionState: 'pending', interactionSummary: undefined },
      );
      setPendingAsk(sessionId, messageId, {
        sessionId,
        messageId,
        requestId,
        toolCallId,
        questions,
      });
    }
  };

  return (
    <div className={styles.card}>
      <div className={styles.header}>
        <div className={styles.headerLeft}>
          <div className={styles.icon}>
            <MessageCircleQuestion size={14} />
          </div>
          <span className={styles.toolName}>AI 询问</span>
        </div>
      </div>

      {questions.map((q, qIdx) => (
        <QuestionBlock
          key={qIdx}
          question={q}
          index={qIdx}
          picks={picks[qIdx] ?? new Set<number>()}
          customText={customTexts[qIdx] ?? ''}
          disabled={submitting}
          onPick={(optionIdx) => setPick(qIdx, optionIdx)}
          onCustomTextChange={(text) =>
            setCustomTexts((prev) => ({ ...prev, [qIdx]: text }))
          }
          onSkip={() => setPicks((prev) => ({ ...prev, [qIdx]: new Set() }))}
        />
      ))}

      <div className={styles.optionsSection}>
        <div className={styles.optionsList}>
          <button
            className={styles.customSubmitBtn}
            onClick={onSubmit}
            disabled={submitting}
            type="button"
          >
            {submitting ? '提交中…' : '提交答案'}
          </button>
          <button
            className={styles.nextPageBtn}
            onClick={onCancel}
            disabled={submitting}
            type="button"
          >
            <X size={12} />
            全部取消
          </button>
        </div>
      </div>
    </div>
  );
};

export const AskUserResolvedCard: React.FC<{
  state: 'answered' | 'cancelled' | 'error';
  summary?: string;
}> = ({ state, summary }) => {
  const stateClass =
    state === 'answered'
      ? styles.answered
      : state === 'cancelled'
        ? styles.cancelled
        : styles.failed;

  return (
    <div className={`${styles.card} ${stateClass}`}>
      <div className={styles.header}>
        <div className={styles.headerLeft}>
          <div className={styles.icon}>
            {state === 'answered' ? <Check size={14} /> : <X size={14} />}
          </div>
          <span className={styles.toolName}>
            {state === 'answered'
              ? '已回答 AI 提问'
              : state === 'cancelled'
                ? '已取消 AI 提问'
                : 'AI 提问不可用'}
          </span>
        </div>
        {summary && (
          <div className={styles.answerBadge} title={summary}>
            <span>{summary}</span>
          </div>
        )}
      </div>
    </div>
  );
};

interface QuestionBlockProps {
  question: AskUserQuestion;
  index: number;
  picks: Set<number>;
  customText: string;
  disabled: boolean;
  onPick: (optionIdx: number) => void;
  onCustomTextChange: (text: string) => void;
  onSkip: () => void;
}

const QuestionBlock: React.FC<QuestionBlockProps> = ({
  question,
  picks,
  customText,
  disabled,
  onPick,
  onCustomTextChange,
  onSkip,
}) => {
  const isMulti = question.multiSelect ?? false;
  return (
    <>
      <div className={styles.question}>
        {question.header && (
          <span className={styles.pageIndicator}>{question.header}</span>
        )}
        {question.header ? ' ' : ''}
        {question.question}
      </div>
      <div className={styles.optionsSection}>
        <div className={styles.optionsList}>
          {question.options.map((option, optIdx) => {
            const selected = picks.has(optIdx);
            return (
              <button
                key={optIdx}
                className={styles.optionBtn}
                onClick={() => onPick(optIdx)}
                disabled={disabled}
                type="button"
                style={
                  selected
                    ? {
                        borderColor: 'var(--accent-primary)',
                        backgroundColor: 'var(--accent-subtle)',
                        color: 'var(--accent-primary)',
                      }
                    : undefined
                }
              >
                {selected && <Check size={12} />}
                <span>{option.label}</span>
                {option.description && (
                  <span style={{ color: 'var(--fg-muted)', fontSize: 12 }}>
                    {' '}
                    — {option.description}
                  </span>
                )}
              </button>
            );
          })}
        </div>
        <div className={styles.customInputRow}>
          <input
            className={styles.customInput}
            placeholder={
              isMulti
                ? 'Other (可选, 多选用 free-text 加补充)'
                : 'Other (可选, 自由输入答案)'
            }
            value={customText}
            onChange={(e) => onCustomTextChange(e.target.value)}
            disabled={disabled}
          />
          <button
            className={styles.nextPageBtn}
            onClick={onSkip}
            disabled={disabled}
            type="button"
          >
            跳过此题
          </button>
        </div>
      </div>
    </>
  );
};

function summariseAnswers(
  summary: Array<{ question: string; labels: string[]; custom?: string }>,
): string {
  const parts: string[] = [];
  for (const entry of summary) {
    if (entry.labels.length > 0) parts.push(entry.labels.join(', '));
    if (entry.custom) parts.push(entry.custom);
  }
  if (parts.length === 0) return '已跳过';
  return parts.join(' · ');
}
