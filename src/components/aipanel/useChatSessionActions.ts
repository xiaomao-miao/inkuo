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
import { buildConversationHistory } from './messageTransform';
import { extractErrorMessage } from '../../utils/errors';
import {
  collectWorkspaceFiles,
  createSnapshot,
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
  messages,
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
  const pushNotification = useNotificationStore((s) => s.pushNotification);

  // Keep references so event listeners can read the latest values.
  const recordBaseline = useRef(useBaselineStore.getState().recordBaseline);
  const consumeBaseline = useRef(useBaselineStore.getState().consumeBaseline);

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

    clearEditingState();
    setInput('');
    setIsStreaming(sessionId, true);
    clearToolCalls(sessionId);

    const workspacePath = useSidebarStore.getState().workspacePath || undefined;
    const {
      apiConfigs,
      activeApiConfigId,
      snapshot,
      agent_max_iterations,
      expert_max_iterations,
      cloud,
    } = useSettingsStore.getState().settings;

    // Cloud-mode branch: pick the active cloud model from the cached
    // list and send the JWT-bearing `base_url` + cloud `model_id` so
    // the Rust side can route through the cloud server.
    let configInput: {
      provider: AIProviderType;
      api_key: string | null;
      base_url: string;
      model: string;
      temperature: number;
      max_tokens: number | null;
    };

    if (cloud.cloud_mode_enabled && cloud.account && cloud.active_cloud_model_id) {
      const entry = cloud.cached_models.find((m) => m.id === cloud.active_cloud_model_id);
      if (!entry) {
        throw new Error('所选云端模型已失效，请在设置中重新选择');
      }
      configInput = {
        provider: 'cloud',
        api_key: cloud.account.access_token,
        base_url: `${cloud.account.base_url.replace(/\/+$/, '')}/v1`,
        model: entry.id,
        temperature: 0.7,
        max_tokens: null,
      };
    } else {
      const activeConfig =
        apiConfigs.find((config) => config.id === activeApiConfigId) ?? apiConfigs[0];
      if (!activeConfig) {
        throw new Error('没有可用的本地 API 配置');
      }
      configInput = {
        provider: activeConfig.provider,
        api_key: activeConfig.apiKey,
        base_url: activeConfig.baseUrl,
        model: activeConfig.model,
        temperature: activeConfig.temperature,
        max_tokens: activeConfig.maxTokens,
      };
    }
    const conversationHistory = buildConversationHistory(messages);

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

    // Subscribe to the agent stream's terminal events so we can consume
    // the baseline when the run completes successfully.  We keep the
    // listener open until the matching message id is seen finished.
    let unlistenAgent: UnlistenFn | null = null;
    if (mode === 'agent' || mode === 'plan' || mode === 'ask') {
      listen<AgentStreamEvent>('ai://stream', (event) => {
        const payload = event.payload;
        if (!payload) return;
        if (payload.session_id !== sessionId) return;
        if (payload.message_id !== assistantMessageId) return;
        if (payload.event_type === 'done') {
          // Successful completion — drop the baseline.
          consumeBaseline.current(userMessageId);
          if (unlistenAgent) {
            unlistenAgent();
            unlistenAgent = null;
          }
        } else if (payload.event_type === 'error') {
          // Keep the baseline so the user can re-edit and retry.
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
  }, [activeSession, input, isStreaming, editingMessageId, updateMessage, addMessage, clearEditingState, setInput, setIsStreaming, clearToolCalls, hardCollapseHistory, collapseOldMessages, messages, mode]);

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

  const handleSaveEdit = useCallback(async () => {
    if (!activeSession || !editingMessageId || !editingContent.trim() || isStreaming) return;

    const newContent = editingContent.trim();
    const workspacePath = useSidebarStore.getState().workspacePath;

    // Roll the workspace back to the baseline that was captured at the
    // start of the original agent run, if any.  Failure is non-fatal —
    // the user will still get the truncated conversation and re-sent
    // instruction, but with files at their current state.
    if (workspacePath) {
      const baselineId = useBaselineStore.getState().peekBaseline(editingMessageId);
      if (baselineId) {
        try {
          await restoreSnapshot(workspacePath, baselineId);
        } catch (err) {
          pushNotification({
            kind: 'error',
            title: '回滚基线失败',
            message: extractErrorMessage(err),
          });
        }
      }
    }

    truncateMessagesAfter(activeSession.id, editingMessageId);
    clearEditingState();
    setInput(newContent);
    await sendMessage(newContent);
  }, [activeSession, editingMessageId, editingContent, isStreaming, truncateMessagesAfter, clearEditingState, setInput, sendMessage, pushNotification]);

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
