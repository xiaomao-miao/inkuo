import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { useCallback, useEffect } from 'react';
import {
  useSidebarStore,
  type ActiveToolCall,
  type BuildProgress,
} from '../../store';

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
}

interface KnowledgeStatusPayload {
  workspace_id: string;
  workspace_path: string;
  document_count: number;
  chunk_count: number;
  created_at: string;
  last_updated: string;
}

export function useKnowledgeBase({ activeSessionId }: UseKnowledgeBaseArgs): UseKnowledgeBaseResult {
  const workspacePath = useSidebarStore((state) => state.workspacePath);
  const knowledgeBase = useSidebarStore((state) => state.knowledgeBase);
  const buildProgress = useSidebarStore((state) => state.buildProgress);
  const knowledgeToolCall = useSidebarStore((state) => state.knowledgeToolCall);
  const setKnowledgeBase = useSidebarStore((state) => state.setKnowledgeBase);
  const setBuildProgress = useSidebarStore((state) => state.setBuildProgress);
  const setKnowledgeToolCall = useSidebarStore((state) => state.setKnowledgeToolCall);

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
        });
      } catch (err) {
        if (!cancelled) {
          console.error('Failed to load knowledge base status:', err);
        }
      }
    };

    loadKnowledgeStatus();

    return () => {
      cancelled = true;
    };
  }, [workspacePath, setKnowledgeBase, setBuildProgress, setKnowledgeToolCall]);

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
      console.error('Failed to listen to build progress:', err);
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
      });
      setKnowledgeToolCall({
        id: toolCallId,
        name: 'knowledge_build',
        arguments: {
          workspacePath,
          total_documents: result.total_documents,
          total_chunks: result.total_chunks,
        },
        status: 'success',
        result: `已构建 ${result.total_documents} 个文档，生成 ${result.total_chunks} 个分块。`,
        startTime: startedAt,
        duration: Date.now() - startedAt,
      });
    } catch (err) {
      console.error('Failed to build knowledge base:', err);
      setKnowledgeToolCall({
        id: toolCallId,
        name: 'knowledge_build',
        arguments: {
          workspacePath,
        },
        status: 'error',
        error: String(err),
        result: String(err),
        startTime: startedAt,
        duration: Date.now() - startedAt,
      });
    } finally {
      unlistenProgress?.();
    }
  }, [activeSessionId, workspacePath, setKnowledgeBase, setBuildProgress, setKnowledgeToolCall]);

  const handleKnowledgeClear = useCallback(async () => {
    if (!activeSessionId || !workspacePath) return;

    try {
      await invoke('knowledge_clear', { workspacePath });
      setKnowledgeBase(undefined);
      setBuildProgress(undefined);
      setKnowledgeToolCall(undefined);
    } catch (err) {
      console.error('Failed to clear knowledge base:', err);
    }
  }, [activeSessionId, workspacePath, setKnowledgeBase, setBuildProgress, setKnowledgeToolCall]);

  return {
    workspacePath: workspacePath ?? undefined,
    knowledgeBase,
    buildProgress,
    knowledgeToolCall,
    handleKnowledgeBuild,
    handleKnowledgeClear,
  };
}
