import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { useCallback, useEffect, useRef } from 'react';
import {
  useSidebarStore,
  useNotificationStore,
  type ActiveToolCall,
  type BuildProgress,
} from '../../store';
import { extractErrorMessage, reportError } from '../../utils/errors';

interface UseKnowledgeBaseArgs {
  activeSessionId?: string;
}

interface UseKnowledgeBaseResult {
  workspacePath?: string;
  knowledgeBase: ReturnType<typeof useSidebarStore.getState>['knowledgeBase'];
  buildProgress: BuildProgress | undefined;
  knowledgeToolCall: ActiveToolCall | undefined;
  handleKnowledgeBuild: () => Promise<void>;
  handleKnowledgeClear: () => Promise<void>;
  handleAddMembers: (memberPaths: string[]) => Promise<void>;
  handleRemoveMembers: (memberPaths: string[]) => Promise<void>;
}

interface KnowledgeStatusPayload {
  workspace_id: string;
  workspace_path: string;
  document_count: number;
  chunk_count: number;
  created_at: string;
  last_updated: string;
  members: string[];
}

export function useKnowledgeBase({ activeSessionId }: UseKnowledgeBaseArgs): UseKnowledgeBaseResult {
  const workspacePath = useSidebarStore((state) => state.workspacePath);
  const knowledgeBase = useSidebarStore((state) => state.knowledgeBase);
  const buildProgress = useSidebarStore((state) => state.buildProgress);
  const knowledgeToolCall = useSidebarStore((state) => state.knowledgeToolCall);
  const setKnowledgeBase = useSidebarStore((state) => state.setKnowledgeBase);
  const setBuildProgress = useSidebarStore((state) => state.setBuildProgress);
  const setKnowledgeToolCall = useSidebarStore((state) => state.setKnowledgeToolCall);
  const pushNotification = useNotificationStore((state) => state.pushNotification);

  // Tracks in-flight progress listeners keyed by `session_id`. Sharing this
  // map across `handleKnowledgeBuild` and `handleAddMembers` guarantees a
  // single listener per session, so two concurrent in-flight operations on
  // the same session cannot both `setBuildProgress` for the same event and
  // clobber each other's state.
  const knowledgeProgressUnlistenRef = useRef<Map<string, UnlistenFn>>(new Map());
  // Tracks the `toolCallId` of the currently-pending build so stale progress
  // events can be ignored.
  const activeBuildIdRef = useRef<string | null>(null);
  // Detach the progress listener for `session_id` and clear the active-build
  // marker if no listener remains. Safe to call multiple times.
  const detachBuildListener = useCallback((sessionId: string) => {
    const fn = knowledgeProgressUnlistenRef.current.get(sessionId);
    if (fn) {
      knowledgeProgressUnlistenRef.current.delete(sessionId);
      try {
        fn();
      } catch (err) {
        console.warn('Failed to detach build progress listener:', err);
      }
    }
    if (knowledgeProgressUnlistenRef.current.size === 0) {
      activeBuildIdRef.current = null;
    }
  }, []);

  useEffect(() => {
    if (!workspacePath) {
      setKnowledgeBase(undefined);
      setBuildProgress(undefined);
      setKnowledgeToolCall(undefined);
      return;
    }

    let cancelled = false;

    const loadKnowledgeStatus = async () => {
      try {
        const status = await invoke<KnowledgeStatusPayload | null>('knowledge_status', {
          workspacePath,
        });

        if (cancelled) {
          return;
        }

        if (!status) {
          setKnowledgeBase(undefined);
          return;
        }

        setKnowledgeBase({
          workspaceId: status.workspace_id,
          documentCount: status.document_count,
          chunkCount: status.chunk_count,
          lastUpdated: new Date(status.last_updated).getTime() || Date.now(),
          members: status.members ?? [],
        });
      } catch (err) {
        if (!cancelled) {
          const message = reportError('knowledge-status-load', err);
          pushNotification({
            kind: 'error',
            title: '读取知识库状态失败',
            message,
          });
        }
      }
    };

    loadKnowledgeStatus();

    return () => {
      cancelled = true;
    };
  }, [workspacePath, setKnowledgeBase, setBuildProgress, setKnowledgeToolCall, pushNotification]);

  const handleKnowledgeBuild = useCallback(async () => {
    if (!activeSessionId || !workspacePath) return;

    // Reject re-entry for the same session: a second build started before
    // the first finished would otherwise stack a second listener on the
    // kb://build-progress channel and clobber progress updates. The user
    // can re-trigger after this one finishes.
    if (knowledgeProgressUnlistenRef.current.has(activeSessionId)) {
      pushNotification({
        kind: 'info',
        title: '知识库正在构建中',
        message: '请等待当前构建完成后再试。',
      });
      return;
    }

    // Use a unique `toolCallId` per invocation (not per session) so two
    // builds for the same session can be distinguished. Also use it as
    // the `activeBuildIdRef` marker for stale-progress filtering.
    const toolCallId = `knowledge-build-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;
    const startedAt = Date.now();
    activeBuildIdRef.current = toolCallId;
    setKnowledgeToolCall({
      id: toolCallId,
      name: 'knowledge_build',
      arguments: {
        workspacePath,
      },
      status: 'executing',
      startTime: startedAt,
    });

    try {
      const unlisten = await listen<{
        session_id: string;
        phase: string;
        current: number;
        total: number;
        message: string;
      }>('kb://build-progress', (event) => {
        // Drop events that don't belong to the current build. We can't
        // easily route by `toolCallId` because the backend identifies the
        // build by `sessionId`; instead, we trust the backend to deliver
        // events only for the most recent build per session, and we drop
        // events that arrive after `activeBuildIdRef` was reset (e.g.
        // listener was just detached).
        if (event.payload.session_id !== activeSessionId) return;
        if (activeBuildIdRef.current !== toolCallId) return;
        if (event.payload.phase === 'done') {
          setBuildProgress(undefined);
        } else {
          setBuildProgress({
            phase: event.payload.phase as BuildProgress['phase'],
            current: event.payload.current,
            total: event.payload.total,
            currentFile: event.payload.message,
          });
        }
      });
      knowledgeProgressUnlistenRef.current.set(activeSessionId, unlisten);
    } catch (err) {
      const message = reportError('knowledge-build-listener', err);
      pushNotification({
        kind: 'error',
        title: '监听知识库构建进度失败',
        message,
      });
    }

    try {
      const result = await invoke<{ total_documents: number; total_chunks: number; workspace_id: string }>('knowledge_build', {
        workspacePath,
        sessionId: activeSessionId,
      });

      // Only apply the result if we are still the active build.
      if (activeBuildIdRef.current === toolCallId) {
        setKnowledgeBase({
          workspaceId: result.workspace_id,
          documentCount: result.total_documents,
          chunkCount: result.total_chunks,
          lastUpdated: Date.now(),
          members: [],
        });
        pushNotification({
          kind: 'success',
          title: '知识库构建完成',
          message: `已构建 ${result.total_documents} 个文档，生成 ${result.total_chunks} 个分块。`,
        });
      }
    } catch (err) {
      if (activeBuildIdRef.current === toolCallId) {
        const message = reportError('knowledge-build', err);
        pushNotification({
          kind: 'error',
          title: '知识库构建失败',
          message,
        });
        setKnowledgeToolCall({
          id: toolCallId,
          name: 'knowledge_build',
          arguments: {
            workspacePath,
          },
          status: 'error',
          error: extractErrorMessage(err),
          result: extractErrorMessage(err),
          startTime: startedAt,
          duration: Date.now() - startedAt,
        });
      }
    } finally {
      // Only the build that owns the listener should tear it down, and
      // only if it is still the active build (a newer build would have
      // re-assigned the entry).
      if (activeBuildIdRef.current === toolCallId) {
        detachBuildListener(activeSessionId);
      }
    }
  }, [activeSessionId, workspacePath, setKnowledgeBase, setBuildProgress, setKnowledgeToolCall, pushNotification, detachBuildListener]);

  const handleKnowledgeClear = useCallback(async () => {
    if (!activeSessionId || !workspacePath) return;

    try {
      await invoke('knowledge_clear', { workspacePath });
      setKnowledgeBase(undefined);
      setBuildProgress(undefined);
      setKnowledgeToolCall(undefined);
    } catch (err) {
      const message = reportError('knowledge-clear', err);
      pushNotification({
        kind: 'error',
        title: '清空知识库失败',
        message,
      });
    }
  }, [activeSessionId, workspacePath, setKnowledgeBase, setBuildProgress, setKnowledgeToolCall, pushNotification]);

  const handleAddMembers = useCallback(async (memberPaths: string[]) => {
    if (!workspacePath || memberPaths.length === 0) return;

    const toolCallId = `knowledge-add-members-${Date.now()}`;
    const sessionId = activeSessionId ?? toolCallId;

    // Reject re-entry on the same session so two concurrent addMembers
    // invocations (or a build still in flight for the same session) can't
    // stack two listeners on `kb://build-progress`.
    if (knowledgeProgressUnlistenRef.current.has(sessionId)) {
      pushNotification({
        kind: 'info',
        title: '知识库正在构建中',
        message: '请等待当前任务完成后再试。',
      });
      return;
    }

    try {
      const unlistenProgress = await listen<{
        session_id: string;
        phase: string;
        current: number;
        total: number;
        message: string;
      }>('kb://build-progress', (event) => {
        if (event.payload.session_id !== sessionId) return;
        if (event.payload.phase === 'done') {
          setBuildProgress(undefined);
        } else {
          setBuildProgress({
            phase: event.payload.phase as BuildProgress['phase'],
            current: event.payload.current,
            total: event.payload.total,
            currentFile: event.payload.message,
          });
        }
      });
      knowledgeProgressUnlistenRef.current.set(sessionId, unlistenProgress);
    } catch {
      // Silently ignore progress listener errors
    }

    try {
      const result = await invoke<{ added: number; removed: number; updated: number }>(
        'knowledge_add_members',
        { workspacePath, memberPaths, sessionId },
      );

      // Refresh knowledge status to get updated member list
      const status = await invoke<KnowledgeStatusPayload | null>('knowledge_status', {
        workspacePath,
      });
      if (status) {
        setKnowledgeBase({
          workspaceId: status.workspace_id,
          documentCount: status.document_count,
          chunkCount: status.chunk_count,
          lastUpdated: new Date(status.last_updated).getTime() || Date.now(),
          members: status.members ?? [],
        });
      }

      pushNotification({
        kind: 'success',
        title: '已加入知识库',
        message: `已添加 ${result.added} 个文件到知识库。`,
      });
    } catch (err) {
      const message = reportError('knowledge-add-members', err);
      pushNotification({
        kind: 'error',
        title: '加入知识库失败',
        message,
      });
    } finally {
      detachBuildListener(sessionId);
    }
  }, [workspacePath, activeSessionId, setKnowledgeBase, setBuildProgress, pushNotification, detachBuildListener]);

  const handleRemoveMembers = useCallback(async (memberPaths: string[]) => {
    if (!workspacePath || memberPaths.length === 0) return;

    try {
      const result = await invoke<{ added: number; removed: number; updated: number }>(
        'knowledge_remove_members',
        { workspacePath, memberPaths },
      );

      // Refresh knowledge status
      const status = await invoke<KnowledgeStatusPayload | null>('knowledge_status', {
        workspacePath,
      });
      if (status) {
        setKnowledgeBase({
          workspaceId: status.workspace_id,
          documentCount: status.document_count,
          chunkCount: status.chunk_count,
          lastUpdated: new Date(status.last_updated).getTime() || Date.now(),
          members: status.members ?? [],
        });
      } else {
        setKnowledgeBase(undefined);
      }

      pushNotification({
        kind: 'success',
        title: '已移出知识库',
        message: `已移除 ${result.removed} 个文件。`,
      });
    } catch (err) {
      const message = reportError('knowledge-remove-members', err);
      pushNotification({
        kind: 'error',
        title: '移出知识库失败',
        message,
      });
    }
  }, [workspacePath, setKnowledgeBase, pushNotification]);

  // Tear down every progress listener when the consumer unmounts. Without
  // this, a build in-flight when the panel closes would keep its listener
  // subscribed, leaking Tauri IPC handles across navigations.
  useEffect(() => {
    return () => {
      knowledgeProgressUnlistenRef.current.forEach((unlisten) => {
        try {
          unlisten();
        } catch (err) {
          console.warn('Failed to detach build progress listener:', err);
        }
      });
      knowledgeProgressUnlistenRef.current.clear();
      activeBuildIdRef.current = null;
    };
  }, []);

  return {
    workspacePath: workspacePath ?? undefined,
    knowledgeBase,
    buildProgress,
    knowledgeToolCall,
    handleKnowledgeBuild,
    handleKnowledgeClear,
    handleAddMembers,
    handleRemoveMembers,
  };
}
