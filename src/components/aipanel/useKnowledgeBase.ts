import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { useCallback, useEffect } from 'react';
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

    const toolCallId = `knowledge-build-${activeSessionId}`;
    const startedAt = Date.now();
    setKnowledgeToolCall({
      id: toolCallId,
      name: 'knowledge_build',
      arguments: {
        workspacePath,
      },
      status: 'executing',
      startTime: startedAt,
    });

    let unlistenProgress: (() => void) | undefined;
    try {
      unlistenProgress = await listen<{
        session_id: string;
        phase: string;
        current: number;
        total: number;
        message: string;
      }>('kb://build-progress', (event) => {
        if (event.payload.session_id !== activeSessionId) return;
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
    } catch (err) {
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
    } finally {
      unlistenProgress?.();
    }
  }, [activeSessionId, workspacePath, setKnowledgeBase, setBuildProgress, setKnowledgeToolCall, pushNotification]);

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

    let unlistenProgress: (() => void) | undefined;
    try {
      unlistenProgress = await listen<{
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
    } catch (err) {
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
      unlistenProgress?.();
    }
  }, [workspacePath, activeSessionId, setKnowledgeBase, setBuildProgress, pushNotification]);

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
