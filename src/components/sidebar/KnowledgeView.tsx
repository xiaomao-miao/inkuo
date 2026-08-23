import {
  AlertCircle,
  BookOpenCheck,
  CheckCircle2,
  Database,
  FileCode2,
  FileSpreadsheet,
  FileText,
  FileType2,
  FolderInput,
  Plus,
  RefreshCw,
  RotateCw,
  Search,
  Trash2,
  Upload,
  X,
} from 'lucide-react';
import { open } from '@tauri-apps/plugin-dialog';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { useCallback, useEffect, useMemo, useState } from 'react';
import { useNotificationStore, useSidebarStore } from '../../store';
import type {
  BuildProgress,
  KnowledgeBase,
  KnowledgeDocumentStatus,
  KnowledgeUpdateResult,
} from '../../types';
import styles from './Sidebar.module.css';

interface KnowledgeStatusPayload {
  workspace_id: string;
  document_count: number;
  chunk_count: number;
  last_updated: string;
  members: string[];
  collections?: Record<string, string[]>;
  documents?: Array<{
    path: string;
    collection: string;
    status: 'indexed' | 'pending' | 'error';
    chunk_count: number;
    source_type: string;
    size_bytes: number;
    indexed_at?: string | null;
    error?: string | null;
  }>;
  supported_extensions?: string[];
}

const DEFAULT_EXTENSIONS = [
  'txt', 'md', 'mdx', 'pdf', 'docx', 'pptx', 'xlsx', 'csv', 'tsv', 'html',
  'htm', 'json', 'yaml', 'yml', 'toml', 'xml', 'js', 'jsx', 'ts', 'tsx', 'py',
  'rs', 'go', 'java', 'cpp', 'c', 'h', 'sql', 'css', 'scss', 'vue', 'svelte',
];

function fromStatus(status: KnowledgeStatusPayload): KnowledgeBase {
  return {
    workspaceId: status.workspace_id,
    documentCount: status.document_count,
    chunkCount: status.chunk_count,
    lastUpdated: new Date(status.last_updated).getTime() || Date.now(),
    members: status.members ?? [],
    collections: status.collections ?? { default: status.members ?? [] },
    documents: (status.documents ?? []).map((document) => ({
      path: document.path,
      collection: document.collection,
      status: document.status,
      chunkCount: document.chunk_count,
      sourceType: document.source_type,
      sizeBytes: document.size_bytes,
      indexedAt: document.indexed_at ? new Date(document.indexed_at).getTime() : undefined,
      error: document.error ?? undefined,
    })),
    supportedExtensions: status.supported_extensions ?? DEFAULT_EXTENSIONS,
  };
}

function formatTime(timestamp: number) {
  return new Date(timestamp).toLocaleString('zh-CN', {
    month: 'short',
    day: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
  });
}

function formatBytes(bytes: number) {
  if (!bytes) return '—';
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

function fileKind(path: string, sourceType: string) {
  if (sourceType) return sourceType;
  return path.split('.').pop()?.toLowerCase() || 'file';
}

function fileIcon(document: KnowledgeDocumentStatus) {
  const kind = fileKind(document.path, document.sourceType);
  if (['spreadsheet', 'xlsx', 'csv', 'tsv'].includes(kind)) return <FileSpreadsheet size={15} />;
  if (['code', 'rs', 'js', 'ts', 'tsx', 'jsx', 'py'].includes(kind)) return <FileCode2 size={15} />;
  if (['markdown', 'text', 'txt', 'md'].includes(kind)) return <FileText size={15} />;
  return <FileType2 size={15} />;
}

export const KnowledgeView = () => {
  const workspacePath = useSidebarStore((state) => state.workspacePath);
  const knowledgeBase = useSidebarStore((state) => state.knowledgeBase);
  const buildProgress = useSidebarStore((state) => state.buildProgress);
  const setKnowledgeBase = useSidebarStore((state) => state.setKnowledgeBase);
  const setBuildProgress = useSidebarStore((state) => state.setBuildProgress);
  const setKnowledgeToolCall = useSidebarStore((state) => state.setKnowledgeToolCall);
  const pushNotification = useNotificationStore((state) => state.pushNotification);

  const [selectedCollection, setSelectedCollection] = useState('default');
  const [newCollection, setNewCollection] = useState('');
  const [showNewCollection, setShowNewCollection] = useState(false);
  const [query, setQuery] = useState('');
  const [busy, setBusy] = useState<string | null>(null);

  const refreshStatus = useCallback(async () => {
    if (!workspacePath) return;
    const status = await invoke<KnowledgeStatusPayload | null>('knowledge_status', { workspacePath });
    setKnowledgeBase(status ? fromStatus(status) : undefined);
  }, [workspacePath, setKnowledgeBase]);

  useEffect(() => {
    if (!workspacePath) return;
    void refreshStatus().catch((error) => {
      pushNotification({ kind: 'error', title: '读取知识库失败', message: String(error) });
    });
  }, [workspacePath, refreshStatus, pushNotification]);

  const collections = useMemo(() => {
    const names = Object.keys(knowledgeBase?.collections ?? { default: [] });
    if (!names.includes('default')) names.unshift('default');
    if (!names.includes(selectedCollection)) names.push(selectedCollection);
    return names.sort((left, right) => (left === 'default' ? -1 : right === 'default' ? 1 : left.localeCompare(right)));
  }, [knowledgeBase, selectedCollection]);

  const documents = useMemo(() => {
    const normalizedQuery = query.trim().toLowerCase();
    return (knowledgeBase?.documents ?? [])
      .filter((document) => document.collection === selectedCollection)
      .filter((document) => !normalizedQuery || document.path.toLowerCase().includes(normalizedQuery))
      .sort((left, right) => {
        if (left.status !== right.status) return left.status === 'error' ? -1 : right.status === 'error' ? 1 : 0;
        return left.path.localeCompare(right.path);
      });
  }, [knowledgeBase, query, selectedCollection]);

  const runWithProgress = useCallback(async <T,>(
    label: string,
    operation: (sessionId: string) => Promise<T>,
  ): Promise<T | undefined> => {
    if (!workspacePath || busy) return undefined;
    const sessionId = `kb-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;
    const startedAt = Date.now();
    setBusy(label);
    setKnowledgeToolCall({
      id: sessionId,
      name: label,
      arguments: { workspacePath, collection: selectedCollection },
      status: 'executing',
      startTime: startedAt,
    });
    let unlisten: (() => void) | undefined;
    try {
      unlisten = await listen<{
        session_id: string;
        phase: BuildProgress['phase'];
        current: number;
        total: number;
        message: string;
      }>('kb://build-progress', (event) => {
        if (event.payload.session_id !== sessionId) return;
        if (event.payload.phase === 'done') {
          setBuildProgress(undefined);
          return;
        }
        setBuildProgress({
          phase: event.payload.phase,
          current: event.payload.current,
          total: event.payload.total,
          currentFile: event.payload.message,
        });
      });
      const result = await operation(sessionId);
      await refreshStatus();
      setKnowledgeToolCall({
        id: sessionId,
        name: label,
        arguments: { workspacePath, collection: selectedCollection },
        status: 'success',
        startTime: startedAt,
        duration: Date.now() - startedAt,
      });
      return result;
    } catch (error) {
      setKnowledgeToolCall({
        id: sessionId,
        name: label,
        arguments: { workspacePath, collection: selectedCollection },
        status: 'error',
        error: String(error),
        startTime: startedAt,
        duration: Date.now() - startedAt,
      });
      pushNotification({ kind: 'error', title: '知识库操作失败', message: String(error) });
      return undefined;
    } finally {
      unlisten?.();
      setBuildProgress(undefined);
      setBusy(null);
    }
  }, [busy, pushNotification, refreshStatus, selectedCollection, setBuildProgress, setKnowledgeToolCall, workspacePath]);

  const handleImport = useCallback(async () => {
    const selected = await open({
      multiple: true,
      directory: false,
      title: `导入到知识集合「${selectedCollection}」`,
      filters: [{
        name: '可索引文档',
        extensions: knowledgeBase?.supportedExtensions?.length
          ? knowledgeBase.supportedExtensions
          : DEFAULT_EXTENSIONS,
      }],
    });
    const memberPaths = Array.isArray(selected) ? selected : selected ? [selected] : [];
    if (!memberPaths.length || !workspacePath) return;

    const result = await runWithProgress<KnowledgeUpdateResult>('knowledge_add_members', (sessionId) =>
      invoke('knowledge_add_members', {
        workspacePath,
        memberPaths,
        sessionId,
        collection: selectedCollection,
      }),
    );
    if (!result) return;
    pushNotification({
      kind: result.failed ? 'info' : 'success',
      title: result.failed ? '批量导入已完成，但有文件跳过' : '批量导入完成',
      message: `新增 ${result.added}，更新 ${result.updated}，未变化 ${result.unchanged}，失败 ${result.failed}。`,
    });
  }, [knowledgeBase, pushNotification, runWithProgress, selectedCollection, workspacePath]);

  const handleSync = useCallback(async () => {
    if (!workspacePath) return;
    const result = await runWithProgress<KnowledgeUpdateResult>('knowledge_update', (sessionId) =>
      invoke('knowledge_update', { workspacePath, sessionId, collection: selectedCollection }),
    );
    if (result) {
      pushNotification({
        kind: result.failed ? 'info' : 'success',
        title: '知识集合已同步',
        message: `新增 ${result.added}，更新 ${result.updated}，移除过期索引 ${result.removed}，失败 ${result.failed}。`,
      });
    }
  }, [pushNotification, runWithProgress, selectedCollection, workspacePath]);

  const handleIndexWorkspace = useCallback(async () => {
    if (!workspacePath) return;
    const result = await runWithProgress<{ total_documents: number; total_chunks: number }>(
      'knowledge_build',
      (sessionId) => invoke('knowledge_build', {
        workspacePath,
        sessionId,
        collection: selectedCollection,
      }),
    );
    if (result) {
      pushNotification({
        kind: 'success',
        title: '工作区文件已追加',
        message: `已索引 ${result.total_documents} 个工作区文件，共 ${result.total_chunks} 个语义分块；当前集合原有文件已保留。`,
      });
    }
  }, [pushNotification, runWithProgress, selectedCollection, workspacePath]);

  const handleRemove = useCallback(async (path: string) => {
    if (!workspacePath || busy) return;
    setBusy(`remove:${path}`);
    try {
      await invoke<KnowledgeUpdateResult>('knowledge_remove_members', {
        workspacePath,
        memberPaths: [path],
        collection: selectedCollection,
      });
      await refreshStatus();
    } catch (error) {
      pushNotification({ kind: 'error', title: '移除失败', message: String(error) });
    } finally {
      setBusy(null);
    }
  }, [busy, pushNotification, refreshStatus, selectedCollection, workspacePath]);

  const handleRetry = useCallback(async (path: string) => {
    if (!workspacePath) return;
    await runWithProgress('knowledge_add_members', (sessionId) => invoke('knowledge_add_members', {
      workspacePath,
      memberPaths: [path],
      sessionId,
      collection: selectedCollection,
    }));
  }, [runWithProgress, selectedCollection, workspacePath]);

  const handleClear = useCallback(async () => {
    if (!workspacePath || busy) return;
    setBusy('knowledge_clear');
    try {
      await invoke('knowledge_clear', { workspacePath });
      setKnowledgeBase(undefined);
      setSelectedCollection('default');
      pushNotification({ kind: 'info', title: '知识库已清空', message: '索引和集合元数据已移除。' });
    } catch (error) {
      pushNotification({ kind: 'error', title: '清空失败', message: String(error) });
    } finally {
      setBusy(null);
    }
  }, [busy, pushNotification, setKnowledgeBase, workspacePath]);

  const createCollection = () => {
    const hasControlCharacter = Array.from(newCollection).some((character) => {
      const codePoint = character.codePointAt(0) ?? 0;
      return codePoint <= 0x1f || codePoint === 0x7f;
    });
    if (hasControlCharacter) {
      pushNotification({
        kind: 'error',
        title: '集合名称无效',
        message: '集合名称不能包含换行、制表符或其他控制字符。',
      });
      return;
    }
    const name = newCollection.trim().replace(/\s+/g, ' ');
    if (!name) return;
    if (Array.from(name).length > 80) {
      pushNotification({ kind: 'error', title: '集合名称过长', message: '集合名称最多 80 个字符。' });
      return;
    }
    setSelectedCollection(name);
    setNewCollection('');
    setShowNewCollection(false);
  };

  if (!workspacePath) {
    return (
      <div className={styles.knowledgeViewEmpty}>
        <Database size={38} className={styles.knowledgeViewIcon} />
        <p className={styles.knowledgeViewTitle}>先打开一个工作区</p>
        <p className={styles.knowledgeViewHint}>知识库会按工作区隔离保存。</p>
      </div>
    );
  }

  return (
    <div className={styles.knowledgeManager}>
      <header className={styles.knowledgeManagerHeader}>
        <div>
          <div className={styles.knowledgeManagerTitleRow}>
            <BookOpenCheck size={18} />
            <h2>知识库</h2>
          </div>
          <p>批量导入多种文档，按集合检索，并清楚看到每个文件的索引状态。</p>
        </div>
        <button className={styles.knowledgeIconAction} onClick={() => void refreshStatus()} title="刷新状态">
          <RefreshCw size={14} />
        </button>
      </header>

      <div className={styles.knowledgeCollectionBar}>
        <select
          className={styles.knowledgeCollectionSelect}
          value={selectedCollection}
          onChange={(event) => setSelectedCollection(event.target.value)}
          aria-label="知识集合"
        >
          {collections.map((collection) => (
            <option key={collection} value={collection}>
              {collection === 'default' ? '默认集合' : collection}
              {' · '}{knowledgeBase?.collections[collection]?.length ?? 0}
            </option>
          ))}
        </select>
        {showNewCollection ? (
          <div className={styles.knowledgeNewCollection}>
            <input
              autoFocus
              value={newCollection}
              onChange={(event) => setNewCollection(event.target.value)}
              onKeyDown={(event) => {
                if (event.key === 'Enter') createCollection();
                if (event.key === 'Escape') setShowNewCollection(false);
              }}
              placeholder="集合名称"
            />
            <button onClick={createCollection} disabled={!newCollection.trim()}>创建</button>
            <button onClick={() => setShowNewCollection(false)} aria-label="取消"><X size={13} /></button>
          </div>
        ) : (
          <button className={styles.knowledgeIconAction} onClick={() => setShowNewCollection(true)} title="新建集合">
            <Plus size={14} />
          </button>
        )}
      </div>

      <div className={styles.knowledgeCommandRow}>
        <button className={styles.knowledgePrimaryCommand} onClick={() => void handleImport()} disabled={!!busy}>
          <Upload size={14} />
          批量导入文件
        </button>
        <button onClick={() => void handleSync()} disabled={!!busy}>
          <RotateCw size={14} />
          同步变化
        </button>
        <button onClick={() => void handleIndexWorkspace()} disabled={!!busy} title="将工作区内所有支持的文件追加到当前集合；保留已导入的外部文件">
          <FolderInput size={14} />
          追加工作区
        </button>
      </div>

      {buildProgress && (
        <div className={styles.knowledgeProgressInline}>
          <RefreshCw size={13} className={styles.spinning} />
          <span>{buildProgress.currentFile || '处理中'}</span>
          <span>{buildProgress.current}/{buildProgress.total}</span>
        </div>
      )}

      <div className={styles.knowledgeSummaryLine}>
        <span>{documents.filter((document) => document.status === 'indexed').length} 个已索引</span>
        <span>{documents.reduce((sum, document) => sum + document.chunkCount, 0)} 个分块</span>
        <span>{documents.filter((document) => document.status === 'error').length} 个异常</span>
        {knowledgeBase && <span>更新于 {formatTime(knowledgeBase.lastUpdated)}</span>}
      </div>

      <div className={styles.knowledgeSearchRow}>
        <Search size={13} />
        <input value={query} onChange={(event) => setQuery(event.target.value)} placeholder="筛选当前集合的文件" />
        {query && <button onClick={() => setQuery('')} aria-label="清除筛选"><X size={12} /></button>}
      </div>

      <div className={styles.knowledgeDocumentList}>
        {documents.length === 0 ? (
          <div className={styles.knowledgeDocumentEmpty}>
            <Database size={28} />
            <strong>当前集合还没有文件</strong>
            <span>可以一次选择多个 DOCX、PDF、PPTX、XLSX、Markdown、代码等文件。</span>
          </div>
        ) : documents.map((document) => {
          const name = document.path.split(/[\\/]/).pop() || document.path;
          const isRemoving = busy === `remove:${document.path}`;
          return (
            <article key={`${document.collection}:${document.path}`} className={styles.knowledgeDocumentRow} data-status={document.status}>
              <div className={styles.knowledgeDocumentIcon}>{fileIcon(document)}</div>
              <div className={styles.knowledgeDocumentBody}>
                <div className={styles.knowledgeDocumentTopline}>
                  <strong title={document.path}>{name}</strong>
                  <span className={styles.knowledgeDocumentStatus}>
                    {document.status === 'indexed' ? <CheckCircle2 size={12} /> : <AlertCircle size={12} />}
                    {document.status === 'indexed' ? '已索引' : document.status === 'error' ? '异常' : '等待中'}
                  </span>
                </div>
                <div className={styles.knowledgeDocumentMeta} title={document.path}>
                  <span>{fileKind(document.path, document.sourceType)}</span>
                  <span>{formatBytes(document.sizeBytes)}</span>
                  <span>{document.chunkCount} 分块</span>
                  <span className={styles.knowledgeDocumentPath}>{document.path}</span>
                </div>
                {document.error && <p className={styles.knowledgeDocumentError}>{document.error}</p>}
              </div>
              <div className={styles.knowledgeDocumentActions}>
                {document.status === 'error' && (
                  <button onClick={() => void handleRetry(document.path)} disabled={!!busy} title="重新解析">
                    <RotateCw size={13} />
                  </button>
                )}
                <button onClick={() => void handleRemove(document.path)} disabled={!!busy} title="从当前集合移除">
                  {isRemoving ? <RefreshCw size={13} className={styles.spinning} /> : <Trash2 size={13} />}
                </button>
              </div>
            </article>
          );
        })}
      </div>

      <footer className={styles.knowledgeManagerFooter}>
        <span>支持 {knowledgeBase?.supportedExtensions.length || DEFAULT_EXTENSIONS.length} 种扩展名；失败文件不会进入索引。</span>
        <button onClick={() => void handleClear()} disabled={!!busy}>
          <Trash2 size={12} />
          清空全部
        </button>
      </footer>
    </div>
  );
};
