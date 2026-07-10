import React, { useEffect, useLayoutEffect, useRef } from 'react';
import {
  Send, StopCircle, Terminal, Loader2,
  Plus, Database, Globe,
} from 'lucide-react';
import { useAIPanelStore, useSettingsStore } from '../../store';
import type { ChatMode, FeatureToggleId, FeatureToggleMap } from '../../types';
import { Tooltip } from '../common/Tooltip';
import styles from './AIPanelInput.module.css';

const MODE_LABELS: Record<ChatMode, string> = {
  ask: 'Ask',
  plan: 'Plan',
  agent: 'Agent',
};

const MODE_HINTS: Record<ChatMode, string> = {
  ask: '只回答（不修改文件）',
  plan: '只输出计划（不修改文件）',
  agent: 'Full Agent（可调用工具读写文件）',
};

interface ToggleSpec {
  id: FeatureToggleId;
  label: string;
  hint: string;
  icon: React.ReactNode;
  /** Modes in which this toggle is unusable. */
  disabledIn?: ChatMode[];
  disabledReason?: string;
}

/**
 * Single source of truth for the toggles rendered inside the composer
 * when it's expanded. Add a new entry here to introduce a new toggle —
 * the UI, the labels, and the disabled-state rules all flow from here.
 */
const TOGGLES: ToggleSpec[] = [
  {
    id: 'kb_strict',
    label: '严格 KB 引用',
    hint: '回答必须基于知识库检索结果，末尾列出参考来源。',
    icon: <Database size={13} />,
    disabledIn: ['plan'],
    disabledReason: 'Plan 模式不返回引用型回答。',
  },
  {
    id: 'web_search',
    label: '联网搜索',
    hint: '允许 Agent 检索最新网页内容（后续需配置 API）。',
    icon: <Globe size={13} />,
  },
];

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

/** Render the data-driven toolbar that lives inside the composer card.
 * Exported so callers (e.g. chat headers, snapshots) can preview the
 * "what's on" state without needing the full Composer. */
export const ComposerToggleRows: React.FC<{
  mode: ChatMode;
  sessionId: string | null;
  featureToggles: FeatureToggleMap | undefined;
  disabled?: boolean;
  onToggle: (id: FeatureToggleId, enable: boolean) => void;
}> = ({ mode, sessionId, featureToggles, disabled, onToggle }) => {
  return (
    <>
      {TOGGLES.map((spec) => {
        const isDisabled =
          disabled ||
          sessionId === null ||
          sessionId === '' ||
          (spec.disabledIn?.includes(mode) ?? false);
        const enabled = !!featureToggles?.[spec.id];
        return (
          <button
            key={spec.id}
            type="button"
            className={styles.toggleRow}
            data-enabled={enabled}
            data-disabled={isDisabled}
            aria-pressed={enabled}
            aria-disabled={isDisabled}
            disabled={isDisabled}
            title={
              isDisabled ? spec.disabledReason ?? '当前模式不可用' : spec.hint
            }
            onClick={() => {
              if (isDisabled) return;
              onToggle(spec.id, !enabled);
            }}
          >
            <span className={styles.toggleRowIcon}>{spec.icon}</span>
            <span className={styles.toggleRowLabel}>{spec.label}</span>
            <span className={styles.toggleRowSwitch} data-on={enabled} aria-hidden />
          </button>
        );
      })}
    </>
  );
};

/** Unified model switcher rendered next to the mode button.
 *
 * Lists every model the user can route through in a single dropdown,
 * grouped into two <optgroup>s:
 *   - 云端模型 — only shown when a cloud account is signed in
 *   - 本地 API  — always shown
 *
 * Picking any option flips `cloud_mode_enabled` accordingly and writes
 * the right id back into settings. The current selection is
 * highlighted by matching the `<select>` value against the active
 * id in the currently-active mode (cloud mode → cloud id, else →
 * local config id).
 *
 * If neither group has any entries (e.g. no API configs and no
 * cloud account) we hide the switcher entirely — there's nothing
 * meaningful the user could pick. */
const ModelSwitcher: React.FC<{ mode: ChatMode }> = ({ mode }) => {
  const cloudMode = useSettingsStore((s) => s.settings.cloud.cloud_mode_enabled);
  const cloudAccount = useSettingsStore((s) => s.settings.cloud.account);
  const cloudModels = useSettingsStore((s) => s.settings.cloud.cached_models);
  const activeCloudModelId = useSettingsStore((s) => s.settings.cloud.active_cloud_model_id);
  const setCloudModeEnabledAndPersist = useSettingsStore(
    (s) => s.setCloudModeEnabledAndPersist,
  );
  const setActiveCloudModelIdAndPersist = useSettingsStore(
    (s) => s.setActiveCloudModelIdAndPersist,
  );
  const apiConfigs = useSettingsStore((s) => s.settings.apiConfigs);
  const activeApiConfigId = useSettingsStore((s) => s.settings.activeApiConfigId);
  const setActiveApiConfigAndPersist = useSettingsStore(
    (s) => s.setActiveApiConfigAndPersist,
  );

  const hasCloudOptions = !!cloudAccount && cloudModels.length > 0;
  const hasLocalOptions = apiConfigs.length > 0;

  // Hide entirely when nothing is available — both groups empty.
  if (!hasCloudOptions && !hasLocalOptions) return null;

  // Build the active value the dropdown should highlight. Match the
  // current mode's active id; prefix so a cloud-id can never collide
  // with a local-config-id (both happen to be strings, but their
  // namespaces are independent — the store doesn't enforce uniqueness).
  const currentValue = cloudMode
    ? activeCloudModelId
      ? `cloud:${activeCloudModelId}`
      : ''
    : activeApiConfigId
      ? `local:${activeApiConfigId}`
      : apiConfigs[0]
        ? `local:${apiConfigs[0].id}`
        : '';

  const onChange = async (e: React.ChangeEvent<HTMLSelectElement>) => {
    const raw = e.target.value;
    if (!raw) return;
    const [kind, id] = raw.split(':', 2);
    if (kind === 'cloud' && id) {
      if (!cloudMode) await setCloudModeEnabledAndPersist(true);
      await setActiveCloudModelIdAndPersist(id);
    } else if (kind === 'local' && id) {
      if (cloudMode) await setCloudModeEnabledAndPersist(false);
      await setActiveApiConfigAndPersist(id);
    }
  };

  // Title summarizes what's currently active. We only show "cloud" or
  // "local" labels — the model name itself is rendered inside the
  // closed dropdown.
  const activeLabel = cloudMode
    ? `云端 · ${cloudModels.find((m) => m.id === activeCloudModelId)?.display_name ?? '未选择'}`
    : `本地 · ${apiConfigs.find((c) => c.id === activeApiConfigId)?.name ?? apiConfigs[0]?.name ?? '未选择'}`;

  return (
    <select
      className={styles.modelSwitcher}
      data-mode={mode}
      data-cloud={cloudMode ? 'true' : undefined}
      value={currentValue}
      onChange={onChange}
      title={`当前: ${activeLabel}`}
    >
      {hasCloudOptions && (
        <optgroup label="☁ 云端模型">
          {cloudModels.map((m) => (
            <option key={`cloud:${m.id}`} value={`cloud:${m.id}`}>
              {m.display_name}
            </option>
          ))}
        </optgroup>
      )}
      {hasLocalOptions && (
        <optgroup label="💻 本地 API">
          {apiConfigs.map((c) => (
            <option key={`local:${c.id}`} value={`local:${c.id}`}>
              {c.name}
            </option>
          ))}
        </optgroup>
      )}
    </select>
  );
};

/** Quiet inline status line rendered above the textarea when the
 * composer is collapsed. Shows which toggles are on as plain text
 * with bullet separators — no colored chips, no extra noise. Renders
 * nothing at all (zero DOM, zero height) when no toggles are on, so
 * the composer shrinks its collapsed footprint as much as possible. */
const ActiveToggleStrip: React.FC<{ featureToggles: FeatureToggleMap | undefined }> = ({
  featureToggles,
}) => {
  const active = TOGGLES.filter((spec) => featureToggles?.[spec.id]);
  if (active.length === 0) return null;
  return (
    <div className={styles.composerHeader}>
      <span className={styles.activeBadges} aria-label={`${active.length} 个功能已启用`}>
        {active.map((spec, idx) => (
          <React.Fragment key={spec.id}>
            {idx > 0 && <span className={styles.activeBadgeDot} aria-hidden>·</span>}
            <span className={styles.activeBadge}>
              {spec.icon}
              <span>{spec.label}</span>
            </span>
          </React.Fragment>
        ))}
      </span>
    </div>
  );
};

export const ChatInput: React.FC<ChatInputProps> = ({
  input, setInput, mode, isStreaming,
  sessionId, featureToggles,
  onSend, onStop, onCycleMode,
}) => {
  const expanded = useAIPanelStore((state) => state.featureToolbarExpanded);
  const toggleToolbar = useAIPanelStore((state) => state.toggleFeatureToolbar);
  const setSessionFeatureToggle = useAIPanelStore(
    (state) => state.setSessionFeatureToggle,
  );
  const panelRef = useRef<HTMLDivElement | null>(null);

  /**
   * Drive the panel height via JavaScript instead of `max-height` /
   * `grid-template-rows` transitions — those properties force a layout
   * recalc on every animation frame, which on lower-end hardware reads
   * as a stuttery open. The trade-off:
   *
   *  - When expanding, we measure `scrollHeight` once in `useLayoutEffect`
   *    (so the user never sees the old height) and pin `height` to it.
   *    The CSS transition then runs on a fixed px value (compositor can
   *    handle it cheaply because the surrounding flexbox height is
   *    recomputed once, not per-frame).
   *
   *  - When collapsing, we pin the current pixel height first (one frame)
   *    so the transition has a start value, then on the next frame we
   *    set it to 0. After the transition ends we clear the inline height
   *    so the panel returns to its natural flow.
   *
   * Net result: layout runs ~3 times total (expand) / ~3 times total
   * (collapse), instead of ~60 times per second while the animation runs.
   */
  useLayoutEffect(() => {
    const el = panelRef.current;
    if (!el) return;

    if (expanded) {
      // Measure the natural height of the children.
      const target = el.scrollHeight;
      // If the panel was previously collapsed, jump straight to the
      // target (the CSS opacity/transform handles the visual entry).
      el.style.transition = 'none';
      el.style.height = `${target}px`;
      // Force a frame so the browser commits the height before we
      // re-enable the transition for the (now no-op) settle.
      requestAnimationFrame(() => {
        el.style.transition = '';
        // After the (very short) transition clears, hand the panel back
        // to its natural height so it can adapt to content changes
        // (e.g. row hover states, future copy changes).
        const onEnd = (e: TransitionEvent) => {
          if (e.propertyName !== 'height') return;
          el.style.height = '';
          el.removeEventListener('transitionend', onEnd);
        };
        el.addEventListener('transitionend', onEnd);
      });
    } else {
      // Pin current height first so the transition has a meaningful
      // start value (transitioning from '' to '0px' would otherwise
      // skip straight to zero on most browsers).
      const current = el.getBoundingClientRect().height;
      el.style.transition = 'none';
      el.style.height = `${current}px`;
      requestAnimationFrame(() => {
        el.style.transition = '';
        el.style.height = '0px';
        const onEnd = (e: TransitionEvent) => {
          if (e.propertyName !== 'height') return;
          el.style.height = '';
          el.removeEventListener('transitionend', onEnd);
        };
        el.addEventListener('transitionend', onEnd);
      });
    }
  }, [expanded]);

  // Collapse the toolbar when focus leaves the composer, or when the
  // user dismisses it explicitly with Escape. We deliberately do NOT
  // auto-close on inner click — users expect to toggle multiple rows
  // in one visit.
  useEffect(() => {
    if (!expanded) return;

    // (1) Mouse click outside the composer card.
    const onMouseDown = (e: MouseEvent) => {
      const target = e.target as HTMLElement | null;
      if (!target) return;
      // Clicks inside the toggle panel → user is interacting with the
      // toolbar, leave it open.
      if (panelRef.current?.contains(target)) return;
      // Clicks anywhere inside the composer (textarea, header strip,
      // expand button) but outside the toggle rows should NOT close —
      // the user is still actively editing.
      if (target.closest('[data-composer-root]')) return;
      useAIPanelStore.getState().setFeatureToolbarExpanded(false);
    };

    // (2) Keyboard focus leaves the composer entirely. We listen on
    // `focusout` (bubbling) rather than `blur` because focusout fires
    // even when focus moves between elements inside the document, while
    // blur only fires when the element itself loses focus. We ignore
    // moves that land elsewhere inside the composer (textarea → expand
    // button, etc.) — only true exits collapse the toolbar.
    const onFocusOut = (e: FocusEvent) => {
      const next = e.relatedTarget as Node | null;
      // Focus is moving to something inside the composer → stay open.
      if (next && next instanceof Element && next.closest('[data-composer-root]')) return;
      // Focus is moving to an element inside the toggle panel → stay open.
      if (next && panelRef.current?.contains(next)) return;
      // Focus moved to nothing (window/tab switch) or to something
      // outside the composer → collapse. We use a microtask so that
      // focus-then-click sequences (focus moves to the expand button,
      // then mouse click toggles the panel state) settle cleanly.
      queueMicrotask(() => {
        if (!useAIPanelStore.getState().featureToolbarExpanded) return;
        useAIPanelStore.getState().setFeatureToolbarExpanded(false);
      });
    };

    // (3) Escape closes the panel and returns focus to the textarea —
    // a common pattern for "dismiss an overlay without leaving the
    // composer".
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key !== 'Escape') return;
      // Don't intercept Escape if the user is mid-composition (IME).
      // `isComposing` is true during IME pre-edit on Chromium/WebKit.
      if (e.isComposing || e.keyCode === 229) return;
      e.preventDefault();
      useAIPanelStore.getState().setFeatureToolbarExpanded(false);
      // Best-effort focus restoration; if the textarea isn't in the
      // document yet, we silently skip.
      const ta = document.querySelector<HTMLTextAreaElement>(
        '[data-composer-root] textarea',
      );
      ta?.focus();
    };

    // Defer the mouse listener by a tick so the click that opened the
    // panel doesn't immediately re-close it via the same mousedown.
    const mouseId = window.setTimeout(() => {
      document.addEventListener('mousedown', onMouseDown);
    }, 0);
    document.addEventListener('focusout', onFocusOut);
    document.addEventListener('keydown', onKeyDown);

    return () => {
      window.clearTimeout(mouseId);
      document.removeEventListener('mousedown', onMouseDown);
      document.removeEventListener('focusout', onFocusOut);
      document.removeEventListener('keydown', onKeyDown);
    };
  }, [expanded]);

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
        onChange={e => setInput(e.target.value)}
        onKeyDown={e => {
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
            title={MODE_HINTS[mode]}
          >
            {mode === 'agent' && <Terminal size={12} />}
            {MODE_LABELS[mode]}
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
              <button className={styles.iconBtn} onClick={onStop} title="停止生成" type="button">
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