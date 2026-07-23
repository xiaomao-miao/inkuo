// Orchestrator for the AI-panel composer bubble.
//
// Composes the four sub-components that used to live inline in
// `ChatInput.tsx`:
//
//   - `ActiveToggleStrip`   — quiet header line when collapsed
//   - `ComposerToggleRows`  — the expand-mode toggle rows
//   - `ModelSwitcher`       — cloud / local model picker
//   - `ModeButton`          — the mode cycler (still inline)
//
// Also wires three hooks that own the side-effects:
//
//   - `useComposerPanelAnimation` — height pin + transition race
//   - `useComposerDismiss`        — outside click / focus-out / ESC
//
// The remaining JSX is just the textarea, send button, and bottom row.

import React, { useRef } from 'react';
import { Send, StopCircle, Terminal, Loader2, Plus } from 'lucide-react';

import { useAIPanelStore } from '../../../store';
import type { ChatMode, FeatureToggleMap } from '../../../types';
import { Tooltip } from '../../common/Tooltip';
import { CHAT_MODE_HINT, CHAT_MODE_LABEL } from '../../../constants/chatModes';

import { ActiveToggleStrip } from './ActiveToggleStrip';
import { ComposerToggleRows } from './ComposerToggleRows';
import { ModelSwitcher } from './ModelSwitcher';
import { useComposerDismiss } from './useComposerDismiss';
import { useComposerPanelAnimation } from './useComposerPanelAnimation';

import styles from '../AIPanelInput.module.css';

interface ChatInputProps {
  input: string;
  setInput: (v: string) => void;
  mode: ChatMode;
  isStreaming: boolean;
  sessionId: string | null;
  featureToggles: FeatureToggleMap | undefined;
  onSend: () => void;
  onStop: () => void;
  onCycleMode: () => void;
}

export const ChatInput: React.FC<ChatInputProps> = ({
  input,
  setInput,
  mode,
  isStreaming,
  sessionId,
  featureToggles,
  onSend,
  onStop,
  onCycleMode,
}) => {
  const expanded = useAIPanelStore((state) => state.featureToolbarExpanded);
  const toggleToolbar = useAIPanelStore((state) => state.toggleFeatureToolbar);
  const setSessionFeatureToggle = useAIPanelStore(
    (state) => state.setSessionFeatureToggle,
  );
  const panelRef = useRef<HTMLDivElement | null>(null);

  // Two focused hooks own the side-effects. See their docs.
  useComposerPanelAnimation(panelRef, expanded);
  useComposerDismiss(panelRef, expanded);

  return (
    <div
      className={styles.inputBubble}
      data-composer-root
      data-composer-open={expanded || undefined}
    >
      {/* Header strip — only rendered when at least one toggle is on.
       * Returning null (rather than an empty div) means the textarea
       * sits flush against the bubble's top padding when nothing is
       * active, so the composer uses zero vertical space for the hint. */}
      <ActiveToggleStrip featureToggles={featureToggles} />

      {/* Toggle panel — grows the bubble in place when expanded. No
       * popovers, no overlays. Rows are laid out as a 2-column grid so
       * even with a handful of toggles the panel stays compact. */}
      <div
        ref={panelRef}
        className={styles.togglePanel}
        data-open={expanded}
        aria-hidden={!expanded}
      >
        <div className={styles.toggleGrid}>
          <ComposerToggleRows
            mode={mode}
            sessionId={sessionId}
            featureToggles={featureToggles}
            onToggle={(id, enable) => {
              if (!sessionId) return;
              setSessionFeatureToggle(sessionId, id, enable);
            }}
          />
        </div>
      </div>

      <textarea
        className={styles.input}
        placeholder={
          mode === 'agent'
            ? '输入指令... (例如：帮我创建一个文档)'
            : `输入消息... (Enter 发送，Shift+Enter 换行)`
        }
        value={input}
        onChange={(e) => setInput(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === 'Enter' && !e.shiftKey) {
            e.preventDefault();
            onSend();
          }
        }}
        rows={1}
      />

      <div className={styles.inputBottomRow}>
        <div className={styles.modeGroup}>
          <button
            type="button"
            className={`${styles.modeButton} ${mode === 'agent' ? styles.agentModeActive : ''}`}
            onClick={onCycleMode}
            title={CHAT_MODE_HINT[mode]}
          >
            {mode === 'agent' && <Terminal size={12} />}
            {CHAT_MODE_LABEL[mode]}
          </button>

          <ModelSwitcher mode={mode} />
        </div>

        <div className={styles.inputActions}>
          <button
            type="button"
            className={styles.expandBtn}
            data-open={expanded}
            title={expanded ? '收起功能开关' : '展开功能开关'}
            aria-expanded={expanded}
            onClick={toggleToolbar}
          >
            <Plus size={14} />
          </button>
          {isStreaming ? (
            <Tooltip content="停止生成" side="top">
              <button
                className={styles.iconBtn}
                onClick={onStop}
                title="停止生成"
                type="button"
              >
                <StopCircle size={14} />
              </button>
            </Tooltip>
          ) : null}
          <Tooltip
            content={isStreaming ? 'AI 正在回复…' : '发送消息'}
            side="top"
            shortcut="↵"
          >
            <button
              type="button"
              className={styles.sendBtn}
              onClick={onSend}
              disabled={!input.trim() || isStreaming}
            >
              {isStreaming ? (
                <Loader2 size={16} className={styles.loadingSpinner} />
              ) : (
                <Send size={16} />
              )}
            </button>
          </Tooltip>
        </div>
      </div>
    </div>
  );
};