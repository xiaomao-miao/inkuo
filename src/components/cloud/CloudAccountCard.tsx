import { useEffect, useState } from 'react';
import {
  Cloud,
  LogOut,
  RefreshCw,
  Check,
  AlertCircle,
  Loader2,
  Coins,
  Calendar,
  Activity,
} from 'lucide-react';
import { useSettingsStore } from '../../store';
import type { CloudModelEntry } from '../../types';
import { cloudApi, type CloudAccountInfo } from './cloudApi';
import styles from './CloudPanel.module.css';

interface CloudAccountCardProps {
  onModelsLoaded?: (models: CloudModelEntry[]) => void;
}

/**
 * The signed-in state of the cloud panel. Shows account summary,
 * currently-selected cloud model, and a model picker (which the parent
 * supplies models for via the store). Includes actions for refreshing
 * the model list / account info and logging out.
 */
export const CloudAccountCard = ({ onModelsLoaded }: CloudAccountCardProps) => {
  const account = useSettingsStore((s) => s.settings.cloud.account);
  const setCloudAccountAndPersist = useSettingsStore((s) => s.setCloudAccountAndPersist);
  const setCloudModelsAndPersist = useSettingsStore((s) => s.setCloudModelsAndPersist);
  const setActiveCloudModelIdAndPersist = useSettingsStore(
    (s) => s.setActiveCloudModelIdAndPersist
  );
  const cachedModels = useSettingsStore((s) => s.settings.cloud.cached_models);
  const activeCloudModelId = useSettingsStore((s) => s.settings.cloud.active_cloud_model_id);

  const [info, setInfo] = useState<CloudAccountInfo | null>(null);
  const [refreshing, setRefreshing] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const refresh = async () => {
    if (!account) return;
    setRefreshing(true);
    setError(null);
    try {
      const [accountInfo, models] = await Promise.all([
        cloudApi.fetchAccount(),
        cloudApi.fetchModels(),
      ]);
      setInfo(accountInfo);
      await setCloudModelsAndPersist(models);
      // Refresh account record (server may have updated plan_name etc.)
      const updatedAccount: typeof account = {
        ...account,
        balance_cents: accountInfo.balance_cents,
        plan_name: accountInfo.plan_name,
      };
      await setCloudAccountAndPersist(updatedAccount);
      onModelsLoaded?.(models);

      // Auto-select first model if nothing picked yet
      if (!activeCloudModelId && models.length > 0) {
        await setActiveCloudModelIdAndPersist(models[0].id);
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setRefreshing(false);
    }
  };

  // Initial load + auto-refresh on mount
  useEffect(() => {
    if (account && cachedModels.length === 0) {
      refresh();
    } else if (account) {
      cloudApi.fetchAccount().then(setInfo).catch(() => {});
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [account?.user_id]);

  if (!account) return null;

  const formatYuan = (cents: number) => `¥${(cents / 100).toFixed(2)}`;
  const formatExpires = (iso: string | null) => {
    if (!iso) return '无到期';
    try {
      const d = new Date(iso);
      return d.toLocaleString('zh-CN', { year: 'numeric', month: '2-digit', day: '2-digit' });
    } catch {
      return iso;
    }
  };

  const activeModel = cachedModels.find((m) => m.id === activeCloudModelId);

  return (
    <div className={styles.accountCard}>
      <div className={styles.accountHeader}>
        <div className={styles.accountTitle}>
          <Cloud size={16} />
          <span>inkuo Cloud</span>
        </div>
        <button
          className={styles.refreshBtn}
          onClick={refresh}
          disabled={refreshing}
          title="刷新账号与模型列表"
        >
          {refreshing ? <Loader2 size={12} className={styles.spinner} /> : <RefreshCw size={12} />}
        </button>
      </div>

      <div className={styles.accountEmail}>{account.email}</div>

      <div className={styles.accountGrid}>
        <div className={styles.stat}>
          <div className={styles.statLabel}>
            <Coins size={12} /> 套餐
          </div>
          <div className={styles.statValue}>{info?.plan_name ?? account.plan_name ?? 'Free'}</div>
        </div>
        <div className={styles.stat}>
          <div className={styles.statLabel}>
            <Coins size={12} /> 余额
          </div>
          <div className={styles.statValue}>
            {formatYuan(info?.balance_cents ?? account.balance_cents)}
          </div>
        </div>
        <div className={styles.stat}>
          <div className={styles.statLabel}>
            <Calendar size={12} /> 到期
          </div>
          <div className={styles.statValue}>
            {formatExpires(info?.subscription_expires_at ?? null)}
          </div>
        </div>
        <div className={styles.stat}>
          <div className={styles.statLabel}>
            <Activity size={12} /> 本月用量
          </div>
          <div className={styles.statValue}>
            {info
              ? `${info.tokens_used_this_month.toLocaleString()} / ${info.monthly_token_limit.toLocaleString()} tokens`
              : '加载中...'}
          </div>
        </div>
      </div>

      <div className={styles.modelPicker}>
        <label className={styles.label}>当前云端模型</label>
        {cachedModels.length === 0 ? (
          <button
            className={styles.loadModelsBtn}
            onClick={refresh}
            disabled={refreshing}
          >
            {refreshing ? (
              <>
                <Loader2 size={12} className={styles.spinner} /> 加载中...
              </>
            ) : (
              <>
                <RefreshCw size={12} /> 加载模型列表
              </>
            )}
          </button>
        ) : (
          <select
            className={styles.select}
            value={activeCloudModelId ?? ''}
            onChange={(e) => setActiveCloudModelIdAndPersist(e.target.value || null)}
          >
            {cachedModels.map((m) => (
              <option key={m.id} value={m.id}>
                {m.display_name} (¥{m.input_price_per_m_tokens.toFixed(2)}/1M in)
              </option>
            ))}
          </select>
        )}
        {activeModel && (
          <div className={styles.activeModelNote}>
            <Check size={12} /> 已选择: {activeModel.display_name} · 通过云端路由
          </div>
        )}
      </div>

      {error && (
        <div className={styles.error}>
          <AlertCircle size={12} /> {error}
        </div>
      )}

      <button
        className={styles.logoutBtn}
        onClick={async () => {
          await cloudApi.logout();
          await setCloudAccountAndPersist(null);
        }}
      >
        <LogOut size={12} /> 退出登录
      </button>
    </div>
  );
};