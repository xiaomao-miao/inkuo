// Orchestrator for the AI-panel composer bubble.
//
// Composes the three sub-components that used to live inline in
// `ChatInput.tsx`:
//
//   - `ActiveToggleStrip`   — quiet header line when collapsed
//   - `ComposerToggleRows`  — the expand-mode toggle rows
//   - `ModelSwitcher`       — cloud / local model picker
//
// Also wires three hooks that own the side-effects:
//
//   - `useComposerPanelAnimation` — height pin + transition race
//   - `useComposerDismiss`        — outside click / focus-out / ESC
//
// The remaining JSX is just the textarea, send button, and bottom row.

import React, { useRef } from 'react';
import { ImagePlus, Loader2, Plus, Send, StopCircle, X } from 'lucide-react';
import { open } from '@tauri-apps/plugin-dialog';

import { useAIPanelStore } from '../../../store';
import type { FeatureToggleMap, ImageAttachmentInput } from '../../../types';
import { Tooltip } from '../../common/Tooltip';

import { ActiveToggleStrip } from './ActiveToggleStrip';
import { ComposerToggleRows } from './ComposerToggleRows';
import { ModelSwitcher } from './ModelSwitcher';
import { useComposerDismiss } from './useComposerDismiss';
import { useComposerPanelAnimation } from './useComposerPanelAnimation';
import {
  appendImagePaths,
  MAX_COMPOSER_IMAGE_ATTACHMENTS,
} from './imageAttachments';

import styles from '../AIPanelInput.module.css';

interface ChatInputProps {
  input: string;
  setInput: (v: string) => void;
  isStreaming: boolean;
  sessionId: string | null;
  featureToggles: FeatureToggleMap | undefined;
  imageAttachments: ImageAttachmentInput[];
  onImageAttachmentsChange: (attachments: ImageAttachmentInput[]) => void;
  onSend: () => void;
  onStop: () => void;
}

export const ChatInput: React.FC<ChatInputProps> = ({
  input,
  setInput,
  isStreaming,
  sessionId,
  featureToggles,
  imageAttachments,
  onImageAttachmentsChange,
  onSend,
  onStop,
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

  const pickImages = async () => {
    const selection = await open({
      multiple: true,
      directory: false,
      filters: [{
        name: '图片',
        extensions: ['png', 'jpg', 'jpeg', 'webp', 'gif'],
      }],
    });
    if (!selection) return;
    const paths = Array.isArray(selection) ? selection : [selection];
    onImageAttachmentsChange(appendImagePaths(imageAttachments, paths));
  };

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
            sessionId={sessionId}
            featureToggles={featureToggles}
            onToggle={(id, enable) => {
              if (!sessionId) return;
              setSessionFeatureToggle(sessionId, id, enable);
            }}
          />
        </div>
      </div>

      {imageAttachments.length > 0 && (
        <div className={styles.imageAttachments} aria-label="待发送图片">
          {imageAttachments.map((attachment, index) => (
            <span
              className={styles.imageAttachment}
              key={attachment.path ?? attachment.name ?? index}
              title={attachment.path ?? attachment.name}
            >
              <ImagePlus size={12} />
              <span>{attachment.name ?? `图片 ${index + 1}`}</span>
              <button
                type="button"
                aria-label={`移除 ${attachment.name ?? `图片 ${index + 1}`}`}
                onClick={() => onImageAttachmentsChange(
                  imageAttachments.filter((_, attachmentIndex) => attachmentIndex !== index),
                )}
              >
                <X size={11} />
              </button>
            </span>
          ))}
        </div>
      )}

      <textarea
        className={styles.input}
        placeholder="输入指令... (例如：帮我创建一个文档)"
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
          <ModelSwitcher />
        </div>

        <div className={styles.inputActions}>
          <Tooltip
            content={`添加图片（最多 ${MAX_COMPOSER_IMAGE_ATTACHMENTS} 张）`}
            side="top"
          >
            <button
              type="button"
              className={styles.attachmentBtn}
              aria-label="添加图片"
              onClick={() => void pickImages()}
              disabled={isStreaming || imageAttachments.length >= MAX_COMPOSER_IMAGE_ATTACHMENTS}
            >
              <ImagePlus size={14} />
            </button>
          </Tooltip>
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
              disabled={(!input.trim() && imageAttachments.length === 0) || isStreaming}
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
