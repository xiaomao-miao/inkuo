import { invoke } from '@tauri-apps/api/core';
import { listen, UnlistenFn } from '@tauri-apps/api/event';
import { useCallback, useRef } from 'react';
import {
  useAIPanelStore,
  useSidebarStore,
  useSettingsStore,
  useBaselineStore,
  type ChatMessage,
  type ChatMode,
  type ChatSession,
  type PlanOutput,
} from '../../store';
import { nextChatMode } from '../../constants/chatModes';
import {
  buildConversationHistory,
  buildConversationHistoryBefore,
} from './messageTransform';
import { extractErrorMessage } from '../../utils/errors';
import {
  collectWorkspaceFiles,
  createSnapshot,
  listSnapshots,
  restoreSnapshot,
} from '../../services/snapshots';
import {
  deletePlanFile,
  generatePlanId,
  savePlanToFile,
} from '../../services/planFiles';
import { useNotificationStore } from '../../store';
import type { AIProviderType } from '../../types';

interface UseChatSessionActionsArgs {
  activeSession: ChatSession | undefined;
  mode: ChatMode;
  messages: ChatMessage[];
  isStreaming: boolean;
  input: string;
  setInput: (value: string) => void;
  editingMessageId: string | null;
  editingContent: string;
  clearEditingState: () => void;
}

interface AgentStreamEvent {
  session_id: string;
  message_id: string;
  event_type: string;
  done?: boolean;
  error?: string;
  final_content?: string;
}

export function useChatSessionActions({
  activeSession,
  mode,
  // `messages` is intentionally destructured-but-unused: the old
  // closure-bound use of this array was the source of the "re-send a
  // previous question" bug (it carried the previous assistant reply
  // into the new history). Both `sendMessage` and `resendUserMessage`
  // now read the freshest state via `useAIPanelStore.getState()`. The
  // parameter is kept in the signature so callers don't have to change.
  messages: _messages,
  isStreaming,
  input,
  setInput,
  editingMessageId,
  editingContent,
  clearEditingState,
}: UseChatSessionActionsArgs) {
  const addMessage = useAIPanelStore((state) => state.addMessage);
  const updateMessage = useAIPanelStore((state) => state.updateMessage);
  const setIsStreaming = useAIPanelStore((state) => state.setIsStreaming);
  const truncateMessagesAfter = useAIPanelStore((state) => state.truncateMessagesAfter);
  const clearToolCalls = useAIPanelStore((state) => state.clearToolCalls);
  const hardCollapseHistory = useAIPanelStore((state) => state.hardCollapseHistory);
  const collapseOldMessages = useAIPanelStore((state) => state.collapseOldMessages);
  const setPlanItemFile = useAIPanelStore((state) => state.setPlanItemFile);
  const clearPlanItemFile = useAIPanelStore((state) => state.clearPlanItemFile);
  const resetSessionDerivedState = useAIPanelStore((state) => state.resetSessionDerivedState);
  const pushNotification = useNotificationStore((s) => s.pushNotification);

  // Keep references so event listeners can read the latest values.
  const recordBaseline = useRef(useBaselineStore.getState().recordBaseline);

  /**
   * Locate the trailing plan OutputItem (if any) on `messageId`. Used by
   * the save / destroy flows to read and patch the same item.
   */
  const findTrailingPlanItem = useCallback(
    (sessionId: string, messageId: string) => {
      const session = useAIPanelStore
        .getState()
        .sessions.find((s) => s.id === sessionId);
      const message = session?.messages.find((m) => m.id === messageId);
      if (!message) return undefined;
      const items = message.outputItems;
      for (let i = items.length - 1; i >= 0; i -= 1) {
        const it = items[i];
        if (it.type === 'plan') return it;
      }
      return undefined;
    },
    [],
  );

  /**
   * Helper: best-effort destroy a single plan file. Swallows errors so a
   * missing or already-deleted file never blocks user actions (apply,
   * cancel, close).
   */
  const destroyPlanFileSilently = useCallback(
    async (workspacePath: string, planFileId: string) => {
      try {
        await deletePlanFile(workspacePath, planFileId);
      } catch (err) {
        // Don't surface — the file may have been removed manually, or
        // the workspace was closed. Either way, "best effort".
        console.warn('[plan-destroy] failed:', err);
      }
    },
    [],
  );

  /**
   * Persist a plan OutputItem's raw text to `<workspace>/.inkuo/plans/<id>.md`
   * and stamp the resulting `planFileId` / `planFilePath` back onto the
   * item. Throws on failure so the PlanCard surface can show the error
   * inline.
   */
  const handleSavePlan = useCallback(
    async (messageId: string) => {
      if (!activeSession) throw new Error('No active session');
      const workspacePath = useSidebarStore.getState().workspacePath;
      if (!workspacePath) {
        throw new Error('No workspace path — open a workspace first.');
      }
      const item = findTrailingPlanItem(activeSession.id, messageId);
      if (!item || item.type !== 'plan') {
        throw new Error('No plan in this message.');
      }
      // Re-use an existing planFileId if the user clicks Save again after
      // an edit. Otherwise generate a fresh one. The Rust side sanitizes
      // and atomically writes the file.
      const planFileId = item.planFileId ?? generatePlanId();
      const { path } = await savePlanToFile(
        workspacePath,
        planFileId,
        item.rawText,
      );
      setPlanItemFile(activeSession.id, messageId, planFileId, path);
    },
    [activeSession, findTrailingPlanItem, setPlanItemFile],
  );

  /**
   * Sweep every plan `planFileId` recorded on this session's plan items
   * and dispatch `plan_delete` for each. Used by the delete-session flow
   * in AIPanel (when the user permanently removes a conversation).
   *
   * Best-effort: errors are logged and swallowed. We also clear the
   * in-memory `planFileId` / `planFilePath` on the store so re-opening
   * the session (in case the store is hydrated from localStorage) won't
   * double-delete the same id.
   */
  const destroySessionPlanFiles = useCallback(
    async (sessionId: string) => {
      const workspacePath = useSidebarStore.getState().workspacePath;
      if (!workspacePath) return;
      const session = useAIPanelStore
        .getState()
        .sessions.find((s) => s.id === sessionId);
      if (!session) return;
      const ids = new Set<string>();
      for (const message of session.messages) {
        for (const item of message.outputItems) {
          if (item.type === 'plan' && item.planFileId) {
            ids.add(item.planFileId);
            // Clear local state so re-opening (if it's archived, not
            // deleted) doesn't show a "saved" pill for an absent file.
            clearPlanItemFile(sessionId, message.id);
          }
        }
      }
      await Promise.all(
        Array.from(ids).map((id) => destroyPlanFileSilently(workspacePath, id)),
      );
    },
    [destroyPlanFileSilently, clearPlanItemFile],
  );

  /**
   * Build the `AIConfigInput` payload the Rust command expects, reading
   * the current settings from the store on demand. Callers must use
   * this at the moment of dispatch so the most recent cloud / API
   * config is used (the previous code captured values through stale
   * React closure captures whenever the resend branch passed through
   * `sendMessage`).
   */
  const resolveConfigInput = useCallback((): {
    provider: AIProviderType;
    apiKey: string | null;
    baseUrl: string;
    model: string;
    temperature: number;
    maxTokens: number | null;
  } => {
    const { apiConfigs, activeApiConfigId, cloud } = useSettingsStore.getState().settings;
    if (cloud.cloud_mode_enabled && cloud.account && cloud.active_cloud_model_id) {
      const entry = cloud.cached_models.find((m) => m.id === cloud.active_cloud_model_id);
      if (!entry) {
        throw new Error('所选云端模型已失效，请在设置中重新选择');
      }
      return {
        provider: 'cloud',
        apiKey: cloud.account.access_token,
        baseUrl: `${cloud.account.base_url.replace(/\/+$/, '')}/v1`,
        model: entry.id,
        temperature: 0.7,
        maxTokens: null,
      };
    }
    const activeConfig =
      apiConfigs.find((config) => config.id === activeApiConfigId) ?? apiConfigs[0];
    if (!activeConfig) {
      throw new Error('没有可用的本地 API 配置');
    }
    return {
      provider: activeConfig.provider,
      apiKey: activeConfig.apiKey,
      baseUrl: activeConfig.baseUrl,
      model: activeConfig.model,
      temperature: activeConfig.temperature,
      maxTokens: activeConfig.maxTokens,
    };
  }, []);

  const sendMessage = useCallback(async (instructionOverride?: string) => {
    const instruction = (instructionOverride ?? input).trim();
    if (!activeSession || !instruction || isStreaming) return;

    const sessionId = activeSession.id;
    const isEditing = editingMessageId !== null;
    const userMessageId = isEditing ? editingMessageId : crypto.randomUUID();
    const assistantMessageId = crypto.randomUUID();

    const userMessage: ChatMessage = {
      id: userMessageId,
      role: 'user',
      content: instruction,
      timestamp: Date.now(),
      outputItems: [],
    };

    const assistantPlaceholder: ChatMessage = {
      id: assistantMessageId,
      role: 'assistant',
      timestamp: Date.now(),
      outputItems: [],
    };

    if (isEditing) {
      updateMessage(sessionId, userMessageId, instruction);
    } else {
      addMessage(sessionId, userMessage);
    }
    // Re-collapse any previously-expanded history placeholders so the
    // DOM stays bounded for the upcoming stream. This is the
    // "新问题触发时立即卸载旧消息" hook — by the time React renders
    // the new turn, every older placeholder is already collapsed and
    // the renderer's live window shrinks back to the tail.
    hardCollapseHistory(sessionId);
    collapseOldMessages(sessionId);
    addMessage(sessionId, assistantPlaceholder);

    if (isEditing) {
      clearEditingState();
    }
    setInput('');
    setIsStreaming(sessionId, true);
    clearToolCalls(sessionId);

    const workspacePath = useSidebarStore.getState().workspacePath || undefined;
    const {
      snapshot,
      agent_max_iterations,
      expert_max_iterations,
    } = useSettingsStore.getState().settings;

    // Re-read the message list from the store at dispatch time. The
    // render-time `messages` array captured by `useCallback` is stale
    // for the editing branch (it still carries the previous assistant
    // response that we're about to regenerate), so we always pull the
    // freshest state. For a brand-new user message the new entries we
    // just added above are visible here.
    const liveMessages = useAIPanelStore
      .getState()
      .sessions.find((s) => s.id === sessionId)?.messages ?? [];

    let configInput: {
      provider: AIProviderType;
      api_key: string | null;
      base_url: string;
      model: string;
      temperature: number;
      max_tokens: number | null;
    };
    try {
      const resolved = resolveConfigInput();
      configInput = {
        provider: resolved.provider,
        api_key: resolved.apiKey,
        base_url: resolved.baseUrl,
        model: resolved.model,
        temperature: resolved.temperature,
        max_tokens: resolved.maxTokens,
      };
    } catch (err) {
      updateMessage(sessionId, assistantMessageId, `抱歉，发生了错误：${extractErrorMessage(err)}`);
      setIsStreaming(sessionId, false);
      return;
    }

    // For the editing branch we deliberately EXCLUDE the edited user
    // message from the history payload: the same text is sent as the
    // `instruction` field on this turn, so duplicating it would teach
    // the model to treat the question as a follow-up to itself.
    const conversationHistory = isEditing
      ? buildConversationHistoryBefore(liveMessages, userMessageId) ?? []
      : buildConversationHistory(liveMessages);

    // Auto-baseline: when sending a brand-new (not re-edited) agent-mode
    // instruction, capture a snapshot so re-editing the user message can
    // roll the workspace back.  Failure here is non-fatal — we just skip
    // the baseline and the user can still create one manually.
    if (
      !isEditing &&
      mode === 'agent' &&
      snapshot.autoBaseline &&
      workspacePath
    ) {
      try {
        const files = await collectWorkspaceFiles(workspacePath);
        if (files.length > 0) {
          const label = `AI 基线: ${instruction.slice(0, 30)}`;
          const manifest = await createSnapshot(
            workspacePath,
            label,
            'ai_baseline',
            files
          );
          recordBaseline.current(userMessageId, manifest.snapshotId);
        }
      } catch (err) {
        // Best-effort: log and continue. console.warn is the right tool here
        // because a snapshot failure is a real diagnostic signal — the user
        // has `auto-baseline` on and the call failed, which they should see
        // in the devtools console even though it doesn't break the turn.
        console.warn('[snapshot] baseline creation failed', err);
      }
    }

    // Subscribe to the agent stream's terminal events. The baseline is
    // intentionally NOT consumed on success — leaving it in place lets
    // the user re-edit the same question later and see the model
    // re-approach it from the original pre-instruction state. It is
    // dropped only when the message/session is deleted or the snapshot
    // is evicted by the LRU pass.
    let unlistenAgent: UnlistenFn | null = null;
    if (mode === 'agent' || mode === 'plan' || mode === 'ask') {
      listen<AgentStreamEvent>('ai://stream', (event) => {
        const payload = event.payload;
        if (!payload) return;
        if (payload.session_id !== sessionId) return;
        if (payload.message_id !== assistantMessageId) return;
        if (payload.event_type === 'done') {
          if (unlistenAgent) {
            unlistenAgent();
            unlistenAgent = null;
          }
        } else if (payload.event_type === 'error') {
          if (unlistenAgent) {
            unlistenAgent();
            unlistenAgent = null;
          }
        }
      }).then((fn) => {
        unlistenAgent = fn;
      });
    }

    try {
      const featureToggles = activeSession.featureToggles ?? {};
      // Strict KB toggles are NOT silently consumed — every prompt layer
      // and tool gate we apply is keyed off the explicit list below, so
      // future toggles can be added without touching the send path.
      const enabledToggles = Object.entries(featureToggles)
        .filter(([, on]) => Boolean(on))
        .map(([id]) => id);

      invoke('ai_agent_stream', {
        sessionId,
        messageId: assistantMessageId,
        instruction,
        workspacePath,
        // `mode` replaces the old `readOnly` flag. Rust dispatches:
        //   "plan" → plan prompt + no-tools constraint
        //   "ask"  → ask prompt + read-only registry
        //   "agent"→ agent prompt + full registry
        mode,
        // Forward the user-configured agent loop cap. The Rust side clamps
        // / defaults internally; we just send the raw value (1–200).
        maxIterations: agent_max_iterations,
        // Per-sub-agent iteration cap overrides, keyed by profile name
        // (e.g. `"office_excel_expert"`). The Rust handler drops unknown
        // keys and clamps values to [1, 200]. Missing keys fall back to
        // the compile-time default in `prompts.rs`.
        expertMaxIterations: expert_max_iterations,
        history: conversationHistory,
        // Feature toggles that constrain the prompt and tool set on the
        // Rust side. The Rust handler is responsible for translating each
        // id into the appropriate fragment + tool gate; see
        // `src-tauri/src/feature_toggles.rs` for the registry.
        enabledToggles,
        configInput,
      }).catch((err) => {
        updateMessage(sessionId, assistantMessageId, `抱歉，发生了错误：${extractErrorMessage(err)}`);
        setIsStreaming(sessionId, false);
      });
    } catch (err) {
      updateMessage(sessionId, assistantMessageId, `抱歉，发生了错误：${extractErrorMessage(err)}`);
      setIsStreaming(sessionId, false);
    }
  }, [activeSession, input, isStreaming, editingMessageId, updateMessage, addMessage, clearEditingState, setInput, setIsStreaming, clearToolCalls, hardCollapseHistory, collapseOldMessages, mode, resolveConfigInput]);

  const handleSend = useCallback(async () => {
    await sendMessage();
  }, [sendMessage]);

  /**
   * Send a fully-formed prompt without going through the composer
   * input. Used by the floating selection toolbar: we don't want to
   * yank the user's in-progress input away just because they selected
   * a sentence to ask about. Same guards as `sendMessage` (no
   * streaming). Unlike `sendMessage`, this route also refuses to
   * reinterpret the request as an "edit + resend" — a toolbar click
   * while the user is editing an earlier message is a fresh ask, not
   * a regenerate of the previous turn.
   */
  const sendWithPrompt = useCallback(async (prompt: string) => {
    if (!activeSession) return;
    const instruction = prompt.trim();
    if (!instruction || isStreaming) return;
    if (editingMessageId !== null) {
      // Drop the in-progress edit first so the existing sendMessage
      // path doesn't rewrite the message-id-in-progress.
      clearEditingState();
    }
    await sendMessage(instruction);
  }, [activeSession, isStreaming, editingMessageId, clearEditingState, sendMessage]);

  const handleStop = useCallback(async () => {
    if (!activeSession) return;
    try {
      if (mode === 'agent') {
        await invoke('ai_agent_cancel', { sessionId: activeSession.id });
      } else {
        await invoke('ai_stream_cancel', { sessionId: activeSession.id });
      }
    } catch {
      // ignore
    }
  }, [activeSession, mode]);

  const cycleMode = useCallback(() => {
    if (!activeSession) return;
    useAIPanelStore.getState().setSessionMode(activeSession.id, nextChatMode(mode));
  }, [activeSession, mode]);

  /**
   * Build a follow-up instruction from a structured plan and dispatch it
   * in agent mode. The session's `mode` is flipped to `agent` first so
   * the `sendMessage` snapshot-baseline branch fires (auto-baseline only
   * triggers in agent mode).
   *
   * Before dispatching, the plan's persisted file (`.inkuo/plans/<id>.md`)
   * is destroyed via `plan_delete` — ephemeral plans: once the user
   * commits them, the on-disk artifact is consumed. The `messageId` is
   * provided by PlanCard so we know which plan item to read the
   * `planFileId` from.
   */
  const handleApplyPlan = useCallback(
    async (messageId: string, plan: PlanOutput) => {
      if (!activeSession || isStreaming) return;
      const workspacePath = useSidebarStore.getState().workspacePath;
      // Tear down the .md on disk if it was saved. We capture the
      // planFileId BEFORE flipping modes so the lookup sees the same
      // store snapshot.
      const item = findTrailingPlanItem(activeSession.id, messageId);
      const planFileId = item && item.type === 'plan' ? item.planFileId : undefined;
      if (planFileId && workspacePath) {
        void destroyPlanFileSilently(workspacePath, planFileId);
        clearPlanItemFile(activeSession.id, messageId);
      }

      const fileList = plan.files_to_touch
        .map((f: PlanOutput['files_to_touch'][number]) => `- ${f.path} (${f.intent}): ${f.reason}`)
        .join('\n');
      const instruction = [
        `请按照以下计划执行：${plan.plan_summary}`,
        '',
        '涉及文件：',
        fileList,
        '',
        plan.risk_reason ? `风险说明：${plan.risk_reason}` : '',
        '请按顺序处理每个文件，对每个 delete/rename 操作先和我确认。',
      ]
        .filter(Boolean)
        .join('\n');
      // Flip the session to agent mode BEFORE calling sendMessage so the
      // auto-baseline path inside sendMessage activates.
      useAIPanelStore.getState().setSessionMode(activeSession.id, 'agent');
      setInput(instruction);
      await sendMessage(instruction);
    },
    [activeSession, isStreaming, sendMessage, setInput, findTrailingPlanItem, destroyPlanFileSilently, clearPlanItemFile],
  );

  /**
   * Refill the chat input with a hint pointing the user back at the
   * plan for refinement, without firing the run.
   */
  const handleAdjustPlan = useCallback((_messageId: string, plan: PlanOutput) => {
    if (!activeSession) return;
    const fileList = plan.files_to_touch
      .map((f: PlanOutput['files_to_touch'][number]) => `- ${f.path} (${f.intent}): ${f.reason}`)
      .join('\n');
    const prompt = [
      `请调整计划："${plan.plan_summary}"`,
      '',
      '当前涉及文件：',
      fileList,
      '',
      '请告诉我需要怎么调整。',
    ].join('\n');
    setInput(prompt);
  }, [activeSession, setInput]);

  /**
   * Worker that performs the "re-send an earlier user message" transition
   * without depending on render-time `messages`, `editingMessageId`, or
   * `editingContent`. Every read is done through `useAIPanelStore.getState()`
   * so a stale React closure cannot smuggle the old assistant reply back
   * into the model context.
   *
   * Order is significant:
   *   1. Verify the target message and (for agent mode) the baseline
   *      snapshot still exist on disk. Bail out cleanly otherwise.
   *   2. Restore the workspace to that baseline so file contents and
   *      model context agree.
   *   3. Truncate the conversation to the target message and clear
   *      session-level derived state (active tool calls, diffs, todo).
   *   4. Rewrite the target user message in place with the new content.
   *   5. Append a fresh assistant placeholder.
   *   6. Build history from the post-truncation store snapshot, but
   *      EXCLUDE the target itself — the new content is sent as
   *      `instruction` so the model never sees the question twice.
   *   7. Resolve the AI config and dispatch the stream.
   *
   * Returns the new assistant message id when everything succeeds, so
   * callers can correlate stream events back to the dispatched turn.
   */
  const resendUserMessage = useCallback(
    async (params: {
      targetSessionId: string;
      targetUserMessageId: string;
      newContent: string;
    }): Promise<string | null> => {
      const { targetSessionId, targetUserMessageId, newContent } = params;
      const instruction = newContent.trim();
      if (!instruction) return null;

      const sessionsNow = useAIPanelStore.getState().sessions;
      const session = sessionsNow.find((s) => s.id === targetSessionId);
      if (!session) return null;
      if (session.isStreaming) return null;

      const targetMessage = session.messages.find((m) => m.id === targetUserMessageId);
      if (!targetMessage || targetMessage.role !== 'user') return null;

      const sessionMode = session.mode;
      const workspacePath = useSidebarStore.getState().workspacePath || undefined;

      // Locate the baseline that was captured the first time the user
      // sent this question. We only require a baseline for agent mode;
      // ask/plan runs don't modify the workspace, so we can safely fall
      // back to the current file state.
      const baselineId = useBaselineStore.getState().peekBaseline(targetUserMessageId);
      if (sessionMode === 'agent' && baselineId && workspacePath) {
        try {
          // Verify the snapshot is still on disk before issuing the
          // restore. listSnapshots is cheap relative to the Tauri
          // round-trip and protects against an LRU-evicted baseline
          // that is still in the localStorage map.
          const existing = await listSnapshots(workspacePath);
          const snapshotExists = existing.some((entry) => entry.id === baselineId);
          if (snapshotExists) {
            await restoreSnapshot(workspacePath, baselineId);
          } else {
            useBaselineStore.getState().clearBaseline(targetUserMessageId);
            pushNotification({
              kind: 'error',
              title: '基线快照已失效',
              message: '快照已被回收，无法安全回滚工作区，已中止重发。',
            });
            return null;
          }
        } catch (err) {
          pushNotification({
            kind: 'error',
            title: '回滚基线失败',
            message: extractErrorMessage(err),
          });
          return null;
        }
      }

      // Now commit the chat-side rollback. Order matters: truncate first
      // so the target message is the new tail, then clear any session-
      // level panels that reflected the previous run's tool calls /
      // diffs, then rewrite the user message in place, and finally
      // append the assistant placeholder.
      truncateMessagesAfter(targetSessionId, targetUserMessageId);
      resetSessionDerivedState(targetSessionId);
      updateMessage(targetSessionId, targetUserMessageId, instruction);
      hardCollapseHistory(targetSessionId);
      collapseOldMessages(targetSessionId);

      const assistantMessageId = crypto.randomUUID();
      const assistantPlaceholder: ChatMessage = {
        id: assistantMessageId,
        role: 'assistant',
        timestamp: Date.now(),
        outputItems: [],
      };
      addMessage(targetSessionId, assistantPlaceholder);
      clearEditingState();
      setInput('');
      setIsStreaming(targetSessionId, true);
      clearToolCalls(targetSessionId);

      // Read the now-truncated chat from the store. We MUST NOT use the
      // render-time `messages` array captured by `useCallback` — for
      // resends it still contains the previous assistant reply that
      // we just truncated.
      const liveMessages =
        useAIPanelStore.getState().sessions.find((s) => s.id === targetSessionId)?.messages ?? [];

      const conversationHistory =
        buildConversationHistoryBefore(liveMessages, targetUserMessageId);
      if (conversationHistory === undefined) {
        // The target message vanished between the read above and the
        // store update — extremely unlikely, but fail loud instead of
        // producing a confusing "stuck streaming" UI.
        pushNotification({
          kind: 'error',
          title: '重发失败',
          message: '目标消息已被并发操作移除。',
        });
        setIsStreaming(targetSessionId, false);
        return null;
      }

      let configInput: {
        provider: AIProviderType;
        api_key: string | null;
        base_url: string;
        model: string;
        temperature: number;
        max_tokens: number | null;
      };
      try {
        const resolved = resolveConfigInput();
        configInput = {
          provider: resolved.provider,
          api_key: resolved.apiKey,
          base_url: resolved.baseUrl,
          model: resolved.model,
          temperature: resolved.temperature,
          max_tokens: resolved.maxTokens,
        };
      } catch (err) {
        updateMessage(targetSessionId, assistantMessageId, `抱歉，发生了错误：${extractErrorMessage(err)}`);
        setIsStreaming(targetSessionId, false);
        return null;
      }

      const { agent_max_iterations, expert_max_iterations } = useSettingsStore.getState().settings;
      const featureToggles = session.featureToggles ?? {};
      const enabledToggles = Object.entries(featureToggles)
        .filter(([, on]) => Boolean(on))
        .map(([id]) => id);

      // Same listener as `sendMessage` — we do NOT consume the baseline
      // on success so a future re-edit still rolls back to the original
      // pre-instruction state.
      let unlistenAgent: UnlistenFn | null = null;
      if (sessionMode === 'agent' || sessionMode === 'plan' || sessionMode === 'ask') {
        listen<AgentStreamEvent>('ai://stream', (event) => {
          const payload = event.payload;
          if (!payload) return;
          if (payload.session_id !== targetSessionId) return;
          if (payload.message_id !== assistantMessageId) return;
          if (payload.event_type === 'done' || payload.event_type === 'error') {
            if (unlistenAgent) {
              unlistenAgent();
              unlistenAgent = null;
            }
          }
        }).then((fn) => {
          unlistenAgent = fn;
        });
      }

      try {
        await invoke('ai_agent_stream', {
          sessionId: targetSessionId,
          messageId: assistantMessageId,
          instruction,
          workspacePath,
          mode: sessionMode,
          maxIterations: agent_max_iterations,
          expertMaxIterations: expert_max_iterations,
          history: conversationHistory,
          enabledToggles,
          configInput,
        });
      } catch (err) {
        updateMessage(targetSessionId, assistantMessageId, `抱歉，发生了错误：${extractErrorMessage(err)}`);
        setIsStreaming(targetSessionId, false);
        return null;
      }

      return assistantMessageId;
    },
    [
      truncateMessagesAfter,
      resetSessionDerivedState,
      updateMessage,
      hardCollapseHistory,
      collapseOldMessages,
      addMessage,
      clearEditingState,
      setInput,
      setIsStreaming,
      clearToolCalls,
      resolveConfigInput,
      pushNotification,
    ],
  );

  const handleSaveEdit = useCallback(async () => {
    if (!activeSession || !editingMessageId || !editingContent.trim() || isStreaming) return;

    const targetSessionId = activeSession.id;
    const targetUserMessageId = editingMessageId;
    const newContent = editingContent.trim();

    // Delegate the entire rollback + re-send transaction to the worker
    // so we never fall back to the closure-bound `sendMessage`, which
    // would otherwise read the stale `messages` array and smuggle the
    // previous assistant reply back into the model context.
    await resendUserMessage({
      targetSessionId,
      targetUserMessageId,
      newContent,
    });
  }, [activeSession, editingMessageId, editingContent, isStreaming, resendUserMessage]);

  return {
    handleSend,
    sendWithPrompt,
    handleStop,
    cycleMode,
    handleSaveEdit,
    handleApplyPlan,
    handleAdjustPlan,
    handleSavePlan,
    destroySessionPlanFiles,
  };
}
