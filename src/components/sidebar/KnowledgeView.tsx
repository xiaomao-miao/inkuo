import {
  BookMarked,
  FileText,
  Layers,
  Clock3,
  Brain,
  Trash2,
  RefreshCw,
  FolderOpen,
  Files,
  Sparkles,
  ChevronRight,
  FileCode2,
  FileSpreadsheet,
  FileType2,
} from 'lucide-react';
import { useSidebarStore } from '../../store';
import { useNotificationStore } from '../../store';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { useCallback, useMemo } from 'react';
import type { BuildProgress } from '../../types';
import styles from './Sidebar.module.css';

function formatTime(ts: number) {
  const d = new Date(ts);
  return d.toLocaleString('zh-CN', {
    month: 'short',
    day: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
  });
}

function formatRelativeTime(ts: number) {
  const diff = Date.now() - ts;
  const minutes = Math.floor(diff / 60000);
  if (minutes < 1) return '刚刚更新';
  if (minutes < 60) return `${minutes} 分钟前`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours} 小时前`;
  const days = Math.floor(hours / 24);
  return `${days} 天前`;
}

function formatAverageChunks(documentCount: number, chunkCount: number) {
  if (documentCount === 0) return '0';
  return (chunkCount / documentCount).toFixed(chunkCount / documentCount >= 10 ? 0 : 1);
}

function getFileKindLabel(path: string) {
  const extension = path.split('.').pop()?.toLowerCase() ?? '';
  if (['md', 'mdx'].includes(extension)) return 'Markdown';
  if (['ts', 'tsx', 'js', 'jsx', 'json', 'rs', 'py', 'go', 'java', 'c', 'cpp'].includes(extension)) return '代码';
  if (['xlsx', 'xls', 'csv'].includes(extension)) return '表格';
  if (['doc', 'docx', 'txt'].includes(extension)) return '文档';
  return extension ? extension.toUpperCase() : '文件';
}

function getFileIcon(path: string) {
  const extension = path.split('.').pop()?.toLowerCase() ?? '';
  if (['md', 'mdx'].includes(extension)) return <FileText size={14} />;
  if (['ts', 'tsx', 'js', 'jsx', 'json', 'rs', 'py', 'go', 'java', 'c', 'cpp'].includes(extension)) {
    return <FileCode2 size={14} />;
  }
  if (['xlsx', 'xls', 'csv'].includes(extension)) return <FileSpreadsheet size={14} />;
  return <FileType2 size={14} />;
}

export const KnowledgeView = () => {
  const workspacePath = useSidebarStore((s) => s.workspacePath);
  const knowledgeBase = useSidebarStore((s) => s.knowledgeBase);
  const buildProgress = useSidebarStore((s) => s.buildProgress);
  const setKnowledgeBase = useSidebarStore((s) => s.setKnowledgeBase);
  const setBuildProgress = useSidebarStore((s) => s.setBuildProgress);
  const setKnowledgeToolCall = useSidebarStore((s) => s.setKnowledgeToolCall);
  const pushNotification = useNotificationStore((s) => s.pushNotification);

  const fileInsights = useMemo(() => {
    const members = knowledgeBase?.members ?? [];
    const folders = new Map<string, number>();

    for (const path of members) {
      const folder = path.includes('/') ? path.split('/').slice(0, -1).join('/') : '工作区根目录';
      folders.set(folder || '工作区根目录', (folders.get(folder || '工作区根目录') ?? 0) + 1);
    }

    const folderList = Array.from(folders.entries())
      .sort((a, b) => b[1] - a[1])
      .slice(0, 5)
      .map(([path, count]) => ({ path, count }));

    return {
      folderCount: folders.size,
      topFolders: folderList,
      longestPath: members.reduce((current, path) => (path.length > current.length ? path : current), ''),
    };
  }, [knowledgeBase]);

  const handleBuild = useCallback(async () => {
    if (!workspacePath) return;
    const toolCallId = `kb-build-${Date.now()}`;
    const startedAt = Date.now();

    setKnowledgeToolCall({
      id: toolCallId,
      name: 'knowledge_build',
      arguments: { workspacePath },
      status: 'executing',
      startTime: startedAt,
    });

    let unlisten: (() => void) | undefined;
    try {
      unlisten = await listen<{
        session_id: string;
        phase: string;
        current: number;
        total: number;
        message: string;
      }>('kb://build-progress', (event) => {
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
      console.error('Failed to listen for build progress:', err);
    }

    try {
      const result = await invoke<{
        total_documents: number;
        total_chunks: number;
        workspace_id: string;
      }>('knowledge_build', {
        workspacePath,
        sessionId: toolCallId,
      });

      const currentMembers = knowledgeBase?.members ?? [];
      setKnowledgeBase({
        workspaceId: result.workspace_id,
        documentCount: result.total_documents,
        chunkCount: result.total_chunks,
        lastUpdated: Date.now(),
        members: currentMembers,
      });
      pushNotification({
        kind: 'success',
        title: '知识库构建完成',
        message: `已构建 ${result.total_documents} 个文档，生成 ${result.total_chunks} 个分块。`,
      });
    } catch (err) {
      pushNotification({
        kind: 'error',
        title: '知识库构建失败',
        message: String(err),
      });
    } finally {
      unlisten?.();
    }
  }, [workspacePath, knowledgeBase, setKnowledgeBase, setBuildProgress, setKnowledgeToolCall, pushNotification]);

  const handleClear = useCallback(async () => {
    if (!workspacePath) return;
    try {
      await invoke('knowledge_clear', { workspacePath });
      setKnowledgeBase(undefined);
      setBuildProgress(undefined);
      setKnowledgeToolCall(undefined);
      pushNotification({
        kind: 'info',
        title: '知识库已清空',
        message: '所有知识库文件已移除。',
      });
    } catch (err) {
      pushNotification({
        kind: 'error',
        title: '清空知识库失败',
        message: String(err),
      });
    }
  }, [workspacePath, setKnowledgeBase, setBuildProgress, setKnowledgeToolCall, pushNotification]);

  if (!workspacePath) {
    return (
      <div className={styles.knowledgeViewEmpty}>
        <Brain size={40} className={styles.knowledgeViewIcon} />
        <p className={styles.knowledgeViewTitle}>未打开工作区</p>
        <p className={styles.knowledgeViewHint}>请先打开一个文件夹作为工作区</p>
      </div>
    );
  }

  if (buildProgress) {
    const progressPercent = buildProgress.total > 0
      ? Math.min(100, Math.round((buildProgress.current / buildProgress.total) * 100))
      : 0;

    return (
      <div className={styles.knowledgeViewEmpty}>
        <RefreshCw size={40} className={`${styles.knowledgeViewIcon} ${styles.spinning}`} />
        <p className={styles.knowledgeViewTitle}>正在构建知识库…</p>
        <p className={styles.knowledgeViewHint}>
          {buildProgress.current} / {buildProgress.total} · {buildProgress.currentFile || '处理中'}
        </p>
        <div className={styles.knowledgeBuildMeter}>
          <div className={styles.knowledgeBuildMeterBar} style={{ width: `${progressPercent}%` }} />
        </div>
        <span className={styles.knowledgeBuildMeterLabel}>{progressPercent}%</span>
      </div>
    );
  }

  if (!knowledgeBase) {
    return (
      <div className={styles.knowledgeViewEmpty}>
        <Brain size={40} className={styles.knowledgeViewIcon} />
        <p className={styles.knowledgeViewTitle}>知识库未初始化</p>
        <p className={styles.knowledgeViewHint}>构建后即可查看文件覆盖范围、分块数量和目录分布</p>
        <button className={styles.knowledgeViewActionPrimary} onClick={handleBuild}>
          <RefreshCw size={14} />
          <span>构建知识库</span>
        </button>
      </div>
    );
  }

  return (
    <div className={styles.knowledgeView}>
      <div className={styles.knowledgeHero}>
        <div className={styles.knowledgeHeroMain}>
          <div className={styles.knowledgeHeroBadge}>
            <Brain size={14} />
            <span>Workspace Knowledge</span>
          </div>
          <h2 className={styles.knowledgeHeroTitle}>知识库总览</h2>
          <p className={styles.knowledgeHeroSubtitle}>
            已收录 {knowledgeBase.members.length} 个文件，覆盖 {fileInsights.folderCount} 个目录，
            最近一次更新于 {formatRelativeTime(knowledgeBase.lastUpdated)}。
          </p>
        </div>
        <div className={styles.knowledgeHeroMeta}>
          <span className={styles.knowledgeHeroMetaLabel}>最后构建</span>
          <span className={styles.knowledgeHeroMetaValue}>{formatTime(knowledgeBase.lastUpdated)}</span>
        </div>
      </div>

      <div className={styles.knowledgeViewStatsGrid}>
        <div className={styles.knowledgeViewCard}>
          <div className={styles.knowledgeViewCardIcon}><BookMarked size={16} /></div>
          <div>
            <div className={styles.knowledgeViewCardValue}>{knowledgeBase.members.length}</div>
            <div className={styles.knowledgeViewCardLabel}>已纳入文件</div>
          </div>
        </div>
        <div className={styles.knowledgeViewCard}>
          <div className={styles.knowledgeViewCardIcon}><FileText size={16} /></div>
          <div>
            <div className={styles.knowledgeViewCardValue}>{knowledgeBase.documentCount}</div>
            <div className={styles.knowledgeViewCardLabel}>文档数</div>
          </div>
        </div>
        <div className={styles.knowledgeViewCard}>
          <div className={styles.knowledgeViewCardIcon}><Layers size={16} /></div>
          <div>
            <div className={styles.knowledgeViewCardValue}>{knowledgeBase.chunkCount}</div>
            <div className={styles.knowledgeViewCardLabel}>语义分块</div>
          </div>
        </div>
        <div className={styles.knowledgeViewCard}>
          <div className={styles.knowledgeViewCardIcon}><Sparkles size={16} /></div>
          <div>
            <div className={styles.knowledgeViewCardValue}>
              {formatAverageChunks(knowledgeBase.documentCount, knowledgeBase.chunkCount)}
            </div>
            <div className={styles.knowledgeViewCardLabel}>平均每文档分块</div>
          </div>
        </div>
      </div>

      <div className={styles.knowledgeViewOverviewGrid}>
        <div className={styles.knowledgeViewPanel}>
          <div className={styles.knowledgeViewPanelHeader}>
            <div>
              <span className={styles.knowledgeViewPanelEyebrow}>Coverage</span>
              <h3 className={styles.knowledgeViewPanelTitle}>知识库健康度</h3>
            </div>
            <Clock3 size={16} />
          </div>
          <div className={styles.knowledgeViewChecklist}>
            <div className={styles.knowledgeViewChecklistItem}>
              <span className={styles.knowledgeViewChecklistDot} />
              <span>已覆盖 {fileInsights.folderCount} 个目录</span>
            </div>
            <div className={styles.knowledgeViewChecklistItem}>
              <span className={styles.knowledgeViewChecklistDot} />
              <span>最长路径：{fileInsights.longestPath || '—'}</span>
            </div>
            <div className={styles.knowledgeViewChecklistItem}>
              <span className={styles.knowledgeViewChecklistDot} />
              <span>工作区 ID：{knowledgeBase.workspaceId}</span>
            </div>
          </div>
        </div>

        <div className={styles.knowledgeViewPanel}>
          <div className={styles.knowledgeViewPanelHeader}>
            <div>
              <span className={styles.knowledgeViewPanelEyebrow}>Actions</span>
              <h3 className={styles.knowledgeViewPanelTitle}>知识库操作</h3>
            </div>
            <RefreshCw size={16} />
          </div>
          <div className={styles.knowledgeViewActionsColumn}>
            <button className={styles.knowledgeViewActionPrimary} onClick={handleBuild} title="完整重建知识库">
              <RefreshCw size={14} />
              <span>重新构建索引</span>
            </button>
            <button className={styles.knowledgeViewAction} onClick={handleClear} title="清空知识库">
              <Trash2 size={14} />
              <span>清空当前知识库</span>
            </button>
          </div>
        </div>
      </div>

      <div className={styles.knowledgeViewPanel}>
        <div className={styles.knowledgeViewPanelHeader}>
          <div>
            <span className={styles.knowledgeViewPanelEyebrow}>Folders</span>
            <h3 className={styles.knowledgeViewPanelTitle}>目录分布</h3>
          </div>
          <Files size={16} />
        </div>
        <div className={styles.knowledgeViewFolderList}>
          {fileInsights.topFolders.length === 0 ? (
            <div className={styles.knowledgeViewEmptyHint}>暂无目录数据</div>
          ) : (
            fileInsights.topFolders.map((folder) => (
              <div key={folder.path} className={styles.knowledgeViewFolderItem}>
                <div className={styles.knowledgeViewFolderPathWrap}>
                  <FolderOpen size={14} />
                  <span className={styles.knowledgeViewFolderPath}>{folder.path}</span>
                </div>
                <span className={styles.knowledgeViewFolderCount}>{folder.count} 个文件</span>
              </div>
            ))
          )}
        </div>
      </div>

      <div className={styles.knowledgeViewPanel}>
        <div className={styles.knowledgeViewPanelHeader}>
          <div>
            <span className={styles.knowledgeViewPanelEyebrow}>Files</span>
            <h3 className={styles.knowledgeViewPanelTitle}>知识库文件列表</h3>
          </div>
          <span className={styles.knowledgeViewSectionCount}>{knowledgeBase.members.length}</span>
        </div>
        <div className={styles.knowledgeViewListEnhanced}>
          {knowledgeBase.members.length === 0 ? (
            <div className={styles.knowledgeViewEmptyHint}>暂无文件</div>
          ) : (
            knowledgeBase.members.map((path, index) => {
              const name = path.split('/').pop() ?? path;
              const directory = path.includes('/') ? path.split('/').slice(0, -1).join('/') : '工作区根目录';
              const kind = getFileKindLabel(path);
              return (
                <div key={path} className={styles.knowledgeViewListRow}>
                  <div className={styles.knowledgeViewListIndex}>{String(index + 1).padStart(2, '0')}</div>
                  <div className={styles.knowledgeViewListIcon}>{getFileIcon(path)}</div>
                  <div className={styles.knowledgeViewListMain}>
                    <div className={styles.knowledgeViewListTop}>
                      <span className={styles.knowledgeViewMemberName}>{name}</span>
                      <span className={styles.knowledgeViewKindBadge}>{kind}</span>
                    </div>
                    <div className={styles.knowledgeViewListBottom}>
                      <span className={styles.knowledgeViewMemberPath}>{directory}</span>
                    </div>
                  </div>
                  <ChevronRight size={14} className={styles.knowledgeViewListArrow} />
                </div>
              );
            })
          )}
        </div>
      </div>
    </div>
  );
};
