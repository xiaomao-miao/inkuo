import { useState } from 'react';
import { Cloud, LogIn, UserPlus, Loader2, AlertCircle } from 'lucide-react';
import { useSettingsStore } from '../../store';
import { cloudApi } from './cloudApi';
import { getCloudBaseUrl } from '../../utils/cloudBaseUrl';
import styles from './CloudPanel.module.css';

interface CloudAuthPanelProps {
  onAuthSuccess?: () => void;
}

type AuthMode = 'login' | 'register';

/**
 * Email + password form, with an extra "invite code" field in register
 * mode. The cloud server URL is hard-coded in `utils/cloudBaseUrl.ts`
 * and is no longer user-editable — the previous "self-hosted" override
 * is removed for the GA build.
 */
export const CloudAuthPanel = ({ onAuthSuccess }: CloudAuthPanelProps) => {
  const setCloudAccount = useSettingsStore((s) => s.setCloudAccount);
  const settings = useSettingsStore((s) => s.settings);
  const cloudBaseUrl = getCloudBaseUrl();

  const initialEmail = settings.cloud.account?.email ?? '';

  const [mode, setMode] = useState<AuthMode>('login');
  const [email, setEmail] = useState(initialEmail);
  const [password, setPassword] = useState('');
  const [inviteCode, setInviteCode] = useState('');
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const handleSubmit = async () => {
    if (submitting) return;
    setError(null);

    if (!email.trim() || !password) {
      setError('请填写邮箱和密码');
      return;
    }
    if (mode === 'register' && !inviteCode.trim()) {
      setError('注册需要邀请码');
      return;
    }
    if (mode === 'register' && password.length < 6) {
      setError('密码至少需要 6 个字符');
      return;
    }

    setSubmitting(true);
    try {
      const account =
        mode === 'register'
          ? await cloudApi.register(cloudBaseUrl, inviteCode.trim(), email.trim(), password)
          : await cloudApi.login(cloudBaseUrl, email.trim(), password);

      await setCloudAccount(account);
      await cloudApi.persistAccount({
        ...settings,
        cloud: { ...settings.cloud, account },
      });

      setPassword('');
      onAuthSuccess?.();
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err);
      if (message.toLowerCase().includes('invite')) {
        setError('邀请码无效或已用完');
      } else if (message.toLowerCase().includes('unauthorized') || message.includes('401')) {
        setError('邮箱或密码错误');
      } else if (message.toLowerCase().includes('network')) {
        setError('无法连接到云端服务器，请检查网络后重试');
      } else {
        setError(message);
      }
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <div className={styles.authPanel}>
      <div className={styles.authHeader}>
        <Cloud size={18} />
        <span>inkuo Cloud</span>
      </div>

      <div className={styles.modeSwitch}>
        <button
          type="button"
          className={`${styles.modeBtn} ${mode === 'login' ? styles.modeBtnActive : ''}`}
          onClick={() => setMode('login')}
        >
          <LogIn size={12} /> 登录
        </button>
        <button
          type="button"
          className={`${styles.modeBtn} ${mode === 'register' ? styles.modeBtnActive : ''}`}
          onClick={() => setMode('register')}
        >
          <UserPlus size={12} /> 注册
        </button>
      </div>

      <div className={styles.field}>
        <label className={styles.label}>邮箱</label>
        <input
          className={styles.input}
          type="email"
          value={email}
          onChange={(e) => setEmail(e.target.value)}
          placeholder="you@example.com"
          autoComplete="email"
        />
      </div>

      <div className={styles.field}>
        <label className={styles.label}>密码</label>
        <input
          className={styles.input}
          type="password"
          value={password}
          onChange={(e) => setPassword(e.target.value)}
          placeholder="••••••••"
          autoComplete={mode === 'login' ? 'current-password' : 'new-password'}
        />
      </div>

      {mode === 'register' && (
        <div className={styles.field}>
          <label className={styles.label}>邀请码</label>
          <input
            className={styles.input}
            type="text"
            value={inviteCode}
            onChange={(e) => setInviteCode(e.target.value)}
            placeholder="请输入管理员提供的邀请码"
            autoComplete="off"
          />
        </div>
      )}

      {error && (
        <div className={styles.error}>
          <AlertCircle size={12} /> {error}
        </div>
      )}

      <button
        type="button"
        className={styles.submitBtn}
        onClick={handleSubmit}
        disabled={submitting}
      >
        {submitting ? (
          <>
            <Loader2 size={12} className={styles.spinner} /> 处理中...
          </>
        ) : mode === 'login' ? (
          <>
            <LogIn size={12} /> 登录
          </>
        ) : (
          <>
            <UserPlus size={12} /> 注册并登录
          </>
        )}
      </button>

      <p className={styles.hint}>
        inkuo Cloud 由我们托管，按 token 用量计费。当前支持邀请制注册 + 兑换码充值。
      </p>
    </div>
  );
};
