import { useEffect, useState } from 'react';
import { Cloud, LogIn, CheckCircle2, Sparkles, ShieldCheck } from 'lucide-react';
import { useSettingsStore } from '../../store';
import { cloudApi, type CloudAccountInfo } from './cloudApi';
import { CloudPanel } from './CloudPanel';
import styles from './CloudPage.module.css';

/**
 * Full workspace page for the cloud account. Mirrors the role that
 * `SettingsPanel` plays for the settings tab: lives inside the editor
 * tree and gets rendered whenever the user activates the cloud tab
 * (id `CLOUD_TAB_ID`). The same `CloudPanel` it hosts already branches
 * on `settings.cloud.account` to show either the login/register form
 * (no account) or the account card (signed in), so this page is a
 * thin shell that adds branding + a live status pill at the top.
 */
export const CloudPage: React.FC = () => {
  const account = useSettingsStore((s) => s.settings.cloud.account);
  const cloudMode = useSettingsStore((s) => s.settings.cloud.cloud_mode_enabled);

  const [accountInfo, setAccountInfo] = useState<CloudAccountInfo | null>(null);

  // Refresh account info when the page mounts so the status pill
  // shows the freshest balance / plan name. Errors are silently
  // swallowed — the page itself should always render even if the
  // server is unreachable.
  useEffect(() => {
    let cancelled = false;
    if (!account) {
      setAccountInfo(null);
      return;
    }
    cloudApi
      .fetchAccount()
      .then((info) => {
        if (!cancelled) setAccountInfo(info);
      })
      .catch(() => {
        /* ignored */
      });
    return () => {
      cancelled = true;
    };
    // We only want to re-fetch when the user identity changes; we
    // intentionally ignore other fields on `account` because token
    // refresh ticks (which mutate `access_expires_at`) would otherwise
    // re-fire a redundant server roundtrip on every refresh cycle.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [account?.user_id]);

  return (
    <div className={styles.page}>
      <div className={styles.hero}>
        <div className={styles.heroLeft}>
          <div className={styles.heroIcon}>
            <Cloud size={22} />
          </div>
          <div className={styles.heroText}>
            <h1 className={styles.heroTitle}>inkuo Cloud</h1>
            <p className={styles.heroSubtitle}>
              云端模型路由，按 token 用量计费，邀请制注册 + 兑换码充值。
            </p>
          </div>
        </div>
        <div className={styles.heroRight}>
          <StatusPill
            account={account}
            accountInfo={accountInfo}
            cloudMode={cloudMode}
          />
        </div>
      </div>

      <div className={styles.body}>
        <CloudPanel />
      </div>

      <div className={styles.featureRow}>
        <FeatureBadge icon={<Sparkles size={13} />} label="多模型路由" />
        <FeatureBadge icon={<ShieldCheck size={13} />} label="用量审计" />
        <FeatureBadge icon={<CheckCircle2 size={13} />} label="余额自动扣费" />
      </div>
    </div>
  );
};

/** Pill in the top-right of the hero. Renders one of:
 *  - "未登录" (neutral grey) when no account
 *  - "{email} · {plan}" (success-tinted) when signed in
 */
const StatusPill: React.FC<{
  account: ReturnType<typeof useSettingsStore.getState>['settings']['cloud']['account'];
  accountInfo: CloudAccountInfo | null;
  cloudMode: boolean;
}> = ({ account, accountInfo, cloudMode }) => {
  if (!account) {
    return (
      <div className={styles.pill} data-state="anonymous">
        <LogIn size={12} />
        <span>未登录</span>
      </div>
    );
  }
  const planLabel = accountInfo?.plan_name ?? account.plan_name ?? 'Free';
  return (
    <div className={styles.pill} data-state={cloudMode ? 'active' : 'idle'}>
      <CheckCircle2 size={12} />
      <span className={styles.pillEmail}>{account.email}</span>
      <span className={styles.pillDivider}>·</span>
      <span>{planLabel}</span>
      {cloudMode && <span className={styles.pillTag}>当前模式</span>}
    </div>
  );
};

/** Compact feature chip at the bottom of the page — small marketing
 * flourish so the page doesn't read as "a form over a flat header". */
const FeatureBadge: React.FC<{ icon: React.ReactNode; label: string }> = ({
  icon,
  label,
}) => (
  <div className={styles.featureBadge}>
    <span className={styles.featureIcon}>{icon}</span>
    <span>{label}</span>
  </div>
);