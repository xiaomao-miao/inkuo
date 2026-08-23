import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { open, save } from '@tauri-apps/plugin-dialog';
import {
  ArchiveRestore,
  BookOpenText,
  Check,
  Download,
  LoaderCircle,
  PackageOpen,
  Plus,
  RefreshCw,
  Trash2,
  Upload,
  X,
} from 'lucide-react';

import {
  createPluginPackage,
  exportPlugin,
  importPlugin,
  listPlugins,
  removePlugin,
  setPluginEnabled,
} from '../../services/plugins';
import type { InstalledPlugin, PluginCreateInput } from '../../types/plugins';
import styles from './PluginManager.module.css';

export interface PluginManagerProps {
  className?: string;
  /** Called after install, enable/disable, or removal. Useful for a host
   * settings page that wants to refresh an extension badge/count. */
  onChanged?: (plugins: InstalledPlugin[]) => void;
  /** Open the package-creation form on first mount. */
  defaultCreateOpen?: boolean;
}

const EMPTY_DRAFT: Omit<PluginCreateInput, 'knowledgePaths' | 'outputPath'> = {
  id: '',
  name: '',
  version: '1.0.0',
  description: '',
  prompt: '',
};

function errorText(error: unknown): string {
  if (typeof error === 'string') return error;
  if (error && typeof error === 'object') {
    const value = error as { message?: unknown; content?: unknown };
    if (typeof value.message === 'string') return value.message;
    if (typeof value.content === 'string') return value.content;
  }
  return String(error);
}

export function PluginManager({
  className,
  onChanged,
  defaultCreateOpen = false,
}: PluginManagerProps) {
  const [plugins, setPlugins] = useState<InstalledPlugin[]>([]);
  const [loading, setLoading] = useState(true);
  const [busyId, setBusyId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [createOpen, setCreateOpen] = useState(defaultCreateOpen);
  const [draft, setDraft] = useState(EMPTY_DRAFT);
  const [knowledgePaths, setKnowledgePaths] = useState<string[]>([]);
  const [deleteArmed, setDeleteArmed] = useState<string | null>(null);
  const operationLock = useRef(false);

  const beginOperation = useCallback((id: string): boolean => {
    if (operationLock.current) return false;
    operationLock.current = true;
    setBusyId(id);
    return true;
  }, []);

  const endOperation = useCallback(() => {
    operationLock.current = false;
    setBusyId(null);
  }, []);

  const publish = useCallback((next: InstalledPlugin[]) => {
    setPlugins(next);
    onChanged?.(next);
  }, [onChanged]);

  const refresh = useCallback(async (allowDuringMutation = false) => {
    if (operationLock.current && !allowDuringMutation) return;
    setLoading(true);
    setError(null);
    try {
      publish(await listPlugins());
    } catch (reason) {
      setError(errorText(reason));
    } finally {
      setLoading(false);
    }
  }, [publish]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const enabledCount = useMemo(
    () => plugins.filter((plugin) => plugin.enabled).length,
    [plugins],
  );

  const pickKnowledge = useCallback(async () => {
    const selected = await open({
      multiple: true,
      directory: false,
      filters: [{
        name: '知识文件',
        extensions: ['md', 'mdx', 'txt', 'json', 'yaml', 'yml', 'csv', 'tsv', 'xml', 'html', 'htm'],
      }],
    });
    if (!selected) return;
    setKnowledgePaths(Array.isArray(selected) ? selected : [selected]);
  }, []);

  const handleCreate = useCallback(async () => {
    if (!beginOperation('__create__')) return;
    setError(null);
    setNotice(null);
    try {
      const outputPath = await save({
        defaultPath: `${draft.id || 'my-plugin'}-${draft.version || '1.0.0'}.inkuo-plugin`,
        filters: [{ name: 'inkuo 插件包', extensions: ['inkuo-plugin'] }],
      });
      if (!outputPath) return;
      const result = await createPluginPackage({
        ...draft,
        knowledgePaths,
        outputPath,
      });
      setNotice(`插件包已创建：${result.path}`);
      setCreateOpen(false);
      setDraft(EMPTY_DRAFT);
      setKnowledgePaths([]);
    } catch (reason) {
      setError(errorText(reason));
    } finally {
      endOperation();
    }
  }, [beginOperation, draft, endOperation, knowledgePaths]);

  const handleImport = useCallback(async () => {
    if (!beginOperation('__import__')) return;
    setError(null);
    setNotice(null);
    try {
      const selected = await open({
        multiple: false,
        directory: false,
        filters: [{ name: 'inkuo 插件包', extensions: ['inkuo-plugin'] }],
      });
      if (!selected || Array.isArray(selected)) return;
      const installed = await importPlugin(selected);
      setNotice(`${installed.manifest.name} ${installed.manifest.version} 已导入并保持停用。请核对包清单与 SHA-256 指纹后手动启用。`);
      await refresh(true);
    } catch (reason) {
      setError(errorText(reason));
    } finally {
      endOperation();
    }
  }, [beginOperation, endOperation, refresh]);

  const handleToggle = useCallback(async (plugin: InstalledPlugin) => {
    if (!beginOperation(plugin.manifest.id)) return;
    setError(null);
    try {
      const updated = await setPluginEnabled(plugin.manifest.id, !plugin.enabled);
      await refresh(true);
      setNotice(updated.enabled
        ? `${updated.manifest.name} 已启用，将注入下一次 AI 请求。`
        : `${updated.manifest.name} 已停用。`);
    } catch (reason) {
      setError(errorText(reason));
    } finally {
      endOperation();
    }
  }, [beginOperation, endOperation, refresh]);

  const handleExport = useCallback(async (plugin: InstalledPlugin) => {
    if (!beginOperation(plugin.manifest.id)) return;
    setError(null);
    try {
      const outputPath = await save({
        defaultPath: `${plugin.manifest.id}-${plugin.manifest.version}.inkuo-plugin`,
        filters: [{ name: 'inkuo 插件包', extensions: ['inkuo-plugin'] }],
      });
      if (!outputPath) return;
      const result = await exportPlugin(plugin.manifest.id, outputPath);
      setNotice(`已导出：${result.path}`);
    } catch (reason) {
      setError(errorText(reason));
    } finally {
      endOperation();
    }
  }, [beginOperation, endOperation]);

  const handleRemove = useCallback(async (plugin: InstalledPlugin) => {
    if (deleteArmed !== plugin.manifest.id) {
      setDeleteArmed(plugin.manifest.id);
      return;
    }
    if (!beginOperation(plugin.manifest.id)) return;
    setError(null);
    try {
      await removePlugin(plugin.manifest.id);
      await refresh(true);
      setNotice(`${plugin.manifest.name} 已移除。`);
      setDeleteArmed(null);
    } catch (reason) {
      setError(errorText(reason));
    } finally {
      endOperation();
    }
  }, [beginOperation, deleteArmed, endOperation, refresh]);

  const operationBusy = busyId !== null || loading;

  return (
    <section className={[styles.manager, className].filter(Boolean).join(' ')} aria-label="插件管理">
      <header className={styles.header}>
        <div>
          <span className={styles.eyebrow}><PackageOpen size={13} /> 本地插件包</span>
          <h2>插件</h2>
          <p>把提示词和知识文件封装为可分发的版本化插件包。导入后默认停用，启用前请检查来源、清单与指纹。</p>
        </div>
        <div className={styles.summary}>
          <strong>{enabledCount}</strong>
          <span>已启用 / {plugins.length} 已安装</span>
        </div>
      </header>

      <div className={styles.toolbar}>
        <button type="button" className={styles.primaryButton} onClick={() => setCreateOpen((open) => !open)} disabled={operationBusy}>
          {createOpen ? <X size={15} /> : <Plus size={15} />}
          {createOpen ? '收起创建器' : '创建插件包'}
        </button>
        <button type="button" className={styles.secondaryButton} onClick={() => void handleImport()} disabled={operationBusy}>
          {busyId === '__import__' ? <LoaderCircle className={styles.spin} size={15} /> : <Upload size={15} />}
          导入
        </button>
        <button type="button" className={styles.iconButton} onClick={() => void refresh()} aria-label="刷新插件列表" disabled={operationBusy}>
          <RefreshCw className={loading ? styles.spin : undefined} size={15} />
        </button>
      </div>

      {createOpen && (
        <div className={styles.creator}>
          <div className={styles.formGrid}>
            <label>
              <span>插件 ID</span>
              <input disabled={operationBusy} value={draft.id} placeholder="paper-helper" onChange={(event) => setDraft({ ...draft, id: event.target.value.toLowerCase().replace(/[^a-z0-9_-]/g, '') })} />
            </label>
            <label>
              <span>名称</span>
              <input disabled={operationBusy} value={draft.name} placeholder="论文写作助手" onChange={(event) => setDraft({ ...draft, name: event.target.value })} />
            </label>
            <label>
              <span>版本</span>
              <input disabled={operationBusy} value={draft.version} placeholder="1.0.0" onChange={(event) => setDraft({ ...draft, version: event.target.value })} />
            </label>
            <label>
              <span>说明</span>
              <input disabled={operationBusy} value={draft.description} placeholder="适用范围与用途" onChange={(event) => setDraft({ ...draft, description: event.target.value })} />
            </label>
          </div>
          <label className={styles.promptField}>
            <span>插件提示词</span>
            <textarea disabled={operationBusy} value={draft.prompt} placeholder="描述这个插件应该如何帮助用户、何时使用知识文件，以及期望的输出标准……" onChange={(event) => setDraft({ ...draft, prompt: event.target.value })} />
          </label>
          <div className={styles.knowledgePicker}>
            <button type="button" className={styles.secondaryButton} onClick={() => void pickKnowledge()} disabled={operationBusy}>
              <BookOpenText size={15} /> 选择知识文件
            </button>
            <span>{knowledgePaths.length ? `已选择 ${knowledgePaths.length} 个文件` : '可选，最多 32 个 UTF-8 文本知识文件'}</span>
          </div>
          {knowledgePaths.length > 0 && (
            <div className={styles.fileChips}>
              {knowledgePaths.map((path) => <span key={path} title={path}>{path.split(/[\\/]/).pop()}</span>)}
            </div>
          )}
          <button
            type="button"
            className={styles.primaryButton}
            disabled={!draft.id || !draft.name || !draft.prompt.trim() || operationBusy}
            onClick={() => void handleCreate()}
          >
            {busyId === '__create__' ? <LoaderCircle className={styles.spin} size={15} /> : <ArchiveRestore size={15} />}
            生成 .inkuo-plugin
          </button>
        </div>
      )}

      {error && <div className={styles.error} role="alert">{error}</div>}
      {notice && <div className={styles.notice}><Check size={14} /> {notice}</div>}

      <div className={styles.list}>
        {loading && plugins.length === 0 ? (
          <div className={styles.empty}><LoaderCircle className={styles.spin} size={24} /><span>正在读取插件…</span></div>
        ) : plugins.length === 0 ? (
          <div className={styles.empty}><PackageOpen size={28} /><strong>还没有安装插件</strong><span>创建自己的插件包，或导入别人分享的 .inkuo-plugin。</span></div>
        ) : plugins.map((plugin) => {
          const isArmed = deleteArmed === plugin.manifest.id;
          return (
            <article key={plugin.manifest.id} className={`${styles.card} ${plugin.enabled ? styles.cardEnabled : ''}`}>
              <div className={styles.cardMain}>
                <div className={styles.packageIcon}><PackageOpen size={18} /></div>
                <div className={styles.cardCopy}>
                  <div className={styles.cardTitle}>
                    <strong>{plugin.manifest.name}</strong>
                    <span>v{plugin.manifest.version}</span>
                    {plugin.enabled && <em>已启用</em>}
                  </div>
                  <p>{plugin.manifest.description || '无说明'}</p>
                  <small>{plugin.manifest.id} · {plugin.knowledgeFileCount} 个知识文件</small>
                  <details className={styles.packageDetails}>
                    <summary>查看包清单与指纹</summary>
                    <dl>
                      <div><dt>提示词文件</dt><dd>{plugin.manifest.prompt_path}</dd></div>
                      <div><dt>知识文件</dt><dd>{plugin.manifest.knowledge_files.length ? plugin.manifest.knowledge_files.join('、') : '无'}</dd></div>
                      <div><dt>SHA-256</dt><dd title={plugin.packageSha256}>{plugin.packageSha256}</dd></div>
                    </dl>
                  </details>
                </div>
              </div>
              <div className={styles.cardActions}>
                <button type="button" className={`${styles.toggle} ${plugin.enabled ? styles.toggleOn : ''}`} onClick={() => void handleToggle(plugin)} disabled={operationBusy} aria-pressed={plugin.enabled}>
                  <i /><span>{plugin.enabled ? '启用中' : '已停用'}</span>
                </button>
                <button type="button" className={styles.iconButton} onClick={() => void handleExport(plugin)} disabled={operationBusy} title="导出插件包"><Download size={15} /></button>
                <button type="button" className={`${styles.iconButton} ${isArmed ? styles.dangerArmed : ''}`} onClick={() => void handleRemove(plugin)} disabled={operationBusy} title={isArmed ? '再次点击确认移除' : '移除'}><Trash2 size={15} /></button>
              </div>
            </article>
          );
        })}
      </div>
    </section>
  );
}

export default PluginManager;
