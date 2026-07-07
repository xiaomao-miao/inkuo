import React, { useState, useCallback } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { MessageSquare, ChevronRight, RefreshCw, CheckCircle } from 'lucide-react';
import { useAIPanelStore } from '../../store';
import styles from './AskUserCard.module.css';

const PAGE_SIZE = 5;

interface AskUserCardProps {
  sessionId: string;
  messageId: string;
  toolCallId: string;
  question: string;
  options: string[];
  allowCustom: boolean;
  /** Already answered — show result only, no interaction */
  answer?: string;
}

export const AskUserCard: React.FC<AskUserCardProps> = ({
  sessionId,
  messageId,
  toolCallId,
  question,
  options,
  allowCustom,
  answer,
}) => {
  const patchOutputItem = useAIPanelStore((s) => s.patchOutputItem);
  const [page, setPage] = useState(0);
  const [customValue, setCustomValue] = useState('');
  const [submitting, setSubmitting] = useState(false);
  const [localAnswer, setLocalAnswer] = useState<string | undefined>(answer);

  const totalPages = Math.max(1, Math.ceil(options.length / PAGE_SIZE));
  const pageOptions = options.slice(page * PAGE_SIZE, (page + 1) * PAGE_SIZE);

  const submit = useCallback(
    async (ans: string) => {
      if (submitting || localAnswer !== undefined) return;
      setSubmitting(true);
      try {
        await invoke('answer_ask_user', { toolCallId, answer: ans });
        setLocalAnswer(ans);
        // Persist to store so the card stays in "answered" state after re-render
        patchOutputItem(sessionId, messageId, { toolCallId }, {
          type: 'ask_user',
          toolCallId,
          question,
          options,
          optionPage: page,
          totalPages,
          isPending: false,
          answer: ans,
        } as never);
      } catch (err) {
        console.error('[AskUserCard] answer_ask_user failed:', err);
        setSubmitting(false);
      }
    },
    [submitting, localAnswer, sessionId, messageId, toolCallId, question, options, page, totalPages, patchOutputItem],
  );

  const handleOptionClick = useCallback((opt: string) => submit(opt), [submit]);

  const handleCustomSubmit = useCallback(() => {
    const trimmed = customValue.trim();
    if (trimmed) submit(trimmed);
  }, [customValue, submit]);

  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent<HTMLInputElement>) => {
      if (e.key === 'Enter') handleCustomSubmit();
    },
    [handleCustomSubmit],
  );

  const handleNextPage = useCallback(() => {
    setPage((p) => (p + 1) % totalPages);
  }, [totalPages]);

  const isAnswered = localAnswer !== undefined;

  return (
    <div className={`${styles.card} ${isAnswered ? styles.answered : ''}`}>
      <div className={styles.header}>
        <div className={styles.headerLeft}>
          <span className={styles.icon}>
            {isAnswered ? <CheckCircle size={14} /> : <MessageSquare size={14} />}
          </span>
          <span className={styles.toolName}>向用户提问</span>
        </div>
        {isAnswered && (
          <span className={styles.answerBadge} title={localAnswer}>
            <CheckCircle size={11} />
            <span>{localAnswer}</span>
          </span>
        )}
      </div>

      <div className={styles.question}>{question}</div>

      {!isAnswered && (
        <div className={styles.optionsSection}>
          <div className={styles.optionsList}>
            {pageOptions.map((opt) => (
              <button
                key={opt}
                className={styles.optionBtn}
                disabled={submitting}
                onClick={() => handleOptionClick(opt)}
              >
                <ChevronRight size={13} className={styles.optionArrow} />
                {opt}
              </button>
            ))}
          </div>

          {totalPages > 1 && (
            <button
              className={styles.nextPageBtn}
              disabled={submitting}
              onClick={handleNextPage}
            >
              <RefreshCw size={12} />
              换一批
              <span className={styles.pageIndicator}>
                {page + 1}/{totalPages}
              </span>
            </button>
          )}

          {allowCustom && (
            <div className={styles.customInputRow}>
              <input
                className={styles.customInput}
                type="text"
                placeholder="或者输入自定义答案…"
                value={customValue}
                disabled={submitting}
                onChange={(e) => setCustomValue(e.target.value)}
                onKeyDown={handleKeyDown}
              />
              <button
                className={styles.customSubmitBtn}
                disabled={submitting || customValue.trim().length === 0}
                onClick={handleCustomSubmit}
              >
                提交
              </button>
            </div>
          )}
        </div>
      )}
    </div>
  );
};
