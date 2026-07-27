import { useCallback, useEffect, useState } from 'react';
import {
  ArrowRight,
  Cloud,
  FolderOpen,
  Palette,
  Plus,
  Sparkles,
  Loader2,
  AlertCircle,
  User,
  Wind,
  ZapOff,
  SunMedium,
  Moon,
  Sun,
  LogOut,
  Check,
} from 'lucide-react';
import { open } from '@tauri-apps/plugin-dialog';
import { invoke } from '@tauri-apps/api/core';
import { useNotificationStore, useSettingsStore } from '../../store';
import {
  applyWorkspaceDirectoryLoad,
  switchWorkspace,
} from '../../services/workspace';
import { reportError } from '../../utils/errors';
import { cloudApi } from '../cloud/cloudApi';
import { MOTION_LEVELS, type MotionLevel } from '../../hooks/useMotionLevel';
import { Wordmark } from './Wordmark';
import { getCloudBaseUrl } from '../../utils/cloudBaseUrl';
import styles from './WelcomePage.module.css';

interface WelcomePageProps {
  onWorkspaceSelected?: () => void;
}

type AuthMode = 'login' | 'register';

/** 内嵌 4 个主题预览,与 settings/AppearanceSettings 中保持一致。
 *  注:id 与 settings.theme 一一对应,但欢迎页不需出现 `inkuo-dark` 别名
 *  (settings store 默认 = 'inkuo-dark',欢迎页用 graphite 等价它)。 */
const THEME_PREVIEWS = [
  {
    id: 'graphite',
    label: '石墨',
    blurb: '深色低饱和 · 长时间写作',
    icon: <Moon size={12} />,
    swatches: {
      bg: '#161617',
      surface: '#1d1d1f',
      elevated: '#26262a',
      accent: '#8a93a0',
      fg: '#e8e8ea',
    },
  },
  {
    id: 'verdant',
    label: '墨绿',
    blurb: '深色墨绿 · Linear 气质',
    icon: <Sparkles size={12} />,
    swatches: {
      bg: '#14181a',
      surface: '#1b2023',
      elevated: '#23292c',
      accent: '#69b88c',
      fg: '#e6ecea',
    },
  },
  {
    id: 'iris',
    label: '紫调',
    blurb: '深色紫调 · 演示 / 强调 AI',
    icon: <Sun size={12} />,
    swatches: {
      bg: '#16151d',
      surface: '#1d1c26',
      elevated: '#272634',
      accent: '#9b8afd',
      fg: '#ecebf2',
    },
  },
  {
    id: 'inkuo-light',
    label: '纸质',
    blurb: '浅色 · 让内容当主角',
    icon: <SunMedium size={12} />,
    swatches: {
      bg: '#fbfbfa',
      surface: '#f4f4f1',
      elevated: '#ebebe7',
      accent: '#7c5cff',
      fg: '#2a2a2c',
    },
  },
] as const;

const MOTION_PREVIEWS: Array<{
  id: MotionLevel;
  label: string;
  blurb: string;
  icon: React.ReactNode;
}> = [
  { id: 'standard', label: '标准', blurb: '带 spring 弹跳', icon: <Sparkles size={12} /> },
  { id: 'gentle', label: '温和', blurb: '慢且无弹跳', icon: <Wind size={12} /> },
  { id: 'off', label: '关闭', blurb: '瞬间完成', icon: <ZapOff size={12} /> },
];

export const WelcomePage: React.FC<WelcomePageProps> = ({ onWorkspaceSelected }) => {
  const [isLoading, setIsLoading] = useState(false);
  const pushNotification = useNotificationStore((state) => state.pushNotification);
  const settings = useSettingsStore((s) => s.settings);
  const updateSetting = useSettingsStore((s) => s.updateSetting);
  const setCloudAccount = useSettingsStore((s) => s.setCloudAccount);

  // 当前主题兼容历史别名:inkuo-dark 等价 graphite
  const currentTheme = (settings.theme === 'inkuo-dark' ? 'graphite' : settings.theme) as
    | 'graphite'
    | 'verdant'
    | 'iris'
    | 'inkuo-light';

  // 动效档位独立于 settings store,与 AppearanceSettings 逻辑一致
  const [motionLevel, setMotionLevel] = useState<MotionLevel>(() => {
    if (typeof window === 'undefined') return 'standard';
    const stored = window.localStorage.getItem('inkuo-motion-level');
    return (MOTION_LEVELS as readonly string[]).includes(stored ?? '')
      ? (stored as MotionLevel)
      : 'standard';
  });
  const [reduceMotion, setReduceMotion] = useState<boolean>(() => {
    if (typeof window === 'undefined') return false;
    return window.matchMedia?.('(prefers-reduced-motion: reduce)').matches ?? false;
  });
  useEffect(() => {
    if (typeof window === 'undefined') return;
    const mql = window.matchMedia('(prefers-reduced-motion: reduce)');
    const onChange = () => setReduceMotion(mql.matches);
    mql.addEventListener('change', onChange);
    return () => mql.removeEventListener('change', onChange);
  }, []);
  useEffect(() => {
    if (typeof window === 'undefined') return;
    window.localStorage.setItem('inkuo-motion-level', motionLevel);
    const effective = reduceMotion ? 'off' : motionLevel;
    document.documentElement.setAttribute('data-motion', effective);
  }, [motionLevel, reduceMotion]);

  // ── Cloud auth 内嵌态 ─────────────────────────────────────────────
  const hasCloudAccount = !!settings.cloud.account;
  const cloudBaseUrl = getCloudBaseUrl();
  const initialEmail = settings.cloud.account?.email ?? '';
  // Show plan + remaining balance in the welcome card after login instead of
  // the server host (the host was a leak-ish implementation detail; plan and
  // balance are what a logged-in user actually wants to glance at).
  const planName = settings.cloud.account?.plan_name?.trim();
  const balanceCents = settings.cloud.account?.balance_cents ?? 0;
  const balanceYuan = (balanceCents / 100).toFixed(2);
  const planLabel = planName && planName.length > 0 ? planName : '免费套餐';
  const [authMode, setAuthMode] = useState<AuthMode>('login');
  const [email, setEmail] = useState(initialEmail);
  const [password, setPassword] = useState('');
  const [inviteCode, setInviteCode] = useState('');
  const [submitting, setSubmitting] = useState(false);
  const [authError, setAuthError] = useState<string | null>(null);
  const [justLoggedIn, setJustLoggedIn] = useState(false);

  // ── 打开工作区 ──────────────────────────────────────────────────
  const handleSelectWorkspace = useCallback(async () => {
    setIsLoading(true);
    try {
      const selected = await open({
        directory: true,
        multiple: false,
        title: '选择工作区文件夹',
      });
      if (selected) {
        switchWorkspace(selected);
        await applyWorkspaceDirectoryLoad(selected, { mergeWithExisting: false });
        pushNotification({
          kind: 'info',
          title: '工作区已打开',
          message: `已加载: ${selected}`,
        });
        onWorkspaceSelected?.();
      }
    } catch (err) {
      reportError('welcome-select-workspace', err);
      pushNotification({
        kind: 'error',
        title: '打开工作区失败',
        message: String(err),
      });
    } finally {
      setIsLoading(false);
    }
  }, [pushNotification, onWorkspaceSelected]);

  const handleNewWindow = useCallback(async () => {
    try {
      await invoke('create_new_window');
      pushNotification({
        kind: 'info',
        title: '新窗口已创建',
        message: '正在打开新窗口...',
      });
    } catch (err) {
      reportError('welcome-new-window', err);
      pushNotification({
        kind: 'error',
        title: '创建新窗口失败',
        message: String(err),
      });
    }
  }, [pushNotification]);

  // ── 主题切换 ────────────────────────────────────────────────────
  const handleThemeChange = (id: 'graphite' | 'verdant' | 'iris' | 'inkuo-light') => {
    void updateSetting('theme', id);
  };

  // ── Cloud submit ────────────────────────────────────────────────
  const handleCloudSubmit = async () => {
    if (submitting) return;
    setAuthError(null);
    if (!email.trim() || !password) {
      setAuthError('请填写邮箱和密码');
      return;
    }
    if (authMode === 'register' && !inviteCode.trim()) {
      setAuthError('注册需要邀请码');
      return;
    }
    setSubmitting(true);
    try {
      const account =
        authMode === 'register'
          ? await cloudApi.register(cloudBaseUrl, inviteCode.trim(), email.trim(), password)
          : await cloudApi.login(cloudBaseUrl, email.trim(), password);
      await setCloudAccount(account);
      await cloudApi.persistAccount({
        ...settings,
        cloud: { ...settings.cloud, account },
      });
      setPassword('');
      setInviteCode('');
      setJustLoggedIn(true);
      setTimeout(() => setJustLoggedIn(false), 1800);
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err);
      if (message.toLowerCase().includes('invite')) {
        setAuthError('邀请码无效或已用完');
      } else if (message.toLowerCase().includes('unauthorized') || message.includes('401')) {
        setAuthError('邮箱或密码错误');
      } else if (message.toLowerCase().includes('network')) {
        setAuthError('无法连接到云端服务器,请检查地址或网络');
      } else {
        setAuthError(message);
      }
    } finally {
      setSubmitting(false);
    }
  };

  const handleCloudLogout = async () => {
    try {
      await cloudApi.logout();
      await setCloudAccount(null);
    } catch (err) {
      reportError('welcome-cloud-logout', err);
    }
  };

  return (
    <div className={styles.welcomePage}>
      <div className={styles.welcomePage__ambient} aria-hidden />

      <main className={styles.shell}>
        {/* ─── 顶部:Hero 区(logo + 文案 + 主操作) ─── */}
        <section className={styles.hero}>
          <div className={styles.heroWordmark}>
            <Wordmark />
          </div>

          <p className={styles.tagline}>
            一款为文档而生的编辑器
            <span className={styles.taglineSep}> · </span>
            更加懂你的文档助手
          </p>

          <div className={styles.actions}>
            <button
              className={styles.primaryButton}
              onClick={handleSelectWorkspace}
              disabled={isLoading}
              type="button"
            >
              <FolderOpen size={16} />
              <span>打开文档文件夹</span>
              <ArrowRight size={14} className={styles.arrowIcon} />
              <span className={styles.kbdHint}>⌘O</span>
            </button>
            <button
              className={styles.secondaryButton}
              onClick={handleNewWindow}
              disabled={isLoading}
              type="button"
            >
              <Plus size={16} />
              <span>新窗口</span>
            </button>
          </div>
        </section>

        {/* ─── 主体下:两栏细调(主题 + Cloud) ─── */}
        <div className={styles.belowGrid}>
          {/* 主题 + 动效 */}
          <section className={styles.panel}>
            <header className={styles.panelHeader}>
              <Palette size={14} />
              <h2 className={styles.panelTitle}>主题与外观</h2>
              <span className={styles.panelHint}>点选立即生效</span>
            </header>

            <div className={styles.themeStrip}>
              {THEME_PREVIEWS.map((theme) => {
                const active = theme.id === currentTheme;
                return (
                  <button
                    key={theme.id}
                    type="button"
                    className={`${styles.themeChip} ${active ? styles.themeChipActive : ''}`}
                    onClick={() => handleThemeChange(theme.id)}
                    aria-pressed={active}
                    title={`${theme.label} · ${theme.blurb}`}
                  >
                    <div
                      className={styles.themePreview}
                      style={{ background: theme.swatches.bg }}
                    >
                      <div className={styles.previewRow}>
                        <span
                          className={styles.previewDot}
                          style={{ background: theme.swatches.accent }}
                        />
                        <span
                          className={styles.previewLineBar}
                          style={{
                            background: theme.swatches.fg,
                            width: '60%',
                          }}
                        />
                      </div>
                      <div
                        className={styles.previewCard}
                        style={{ background: theme.swatches.surface }}
                      >
                        <div
                          className={styles.previewBar}
                          style={{ background: theme.swatches.elevated, width: '78%' }}
                        />
                        <div
                          className={styles.previewBar}
                          style={{ background: theme.swatches.elevated, width: '48%' }}
                        />
                        <div
                          className={styles.previewChip}
                          style={{ background: theme.swatches.accent }}
                        />
                      </div>
                    </div>
                    <span className={styles.themeLabel}>
                      {theme.icon}
                      {theme.label}
                    </span>
                    {active && (
                      <span className={styles.themeCheck} aria-hidden>
                        <Check size={10} />
                      </span>
                    )}
                  </button>
                );
              })}
            </div>

            <div className={styles.divider} />

            <div className={styles.motionRow}>
              <span className={styles.motionLabel}>动效强度</span>
              <div className={styles.motionTabs}>
                {MOTION_PREVIEWS.map((m) => {
                  const active = motionLevel === m.id;
                  const blocked = reduceMotion && m.id !== 'off';
                  return (
                    <button
                      key={m.id}
                      type="button"
                      className={`${styles.motionTab} ${active ? styles.motionTabActive : ''}`}
                      onClick={() => setMotionLevel(m.id)}
                      aria-pressed={active}
                      disabled={blocked}
                      title={
                        blocked
                          ? '系统开启了「减少动效」,已强制为关闭'
                          : `${m.label} · ${m.blurb}`
                      }
                    >
                      {m.icon}
                      <span>{m.label}</span>
                    </button>
                  );
                })}
              </div>
            </div>
          </section>

          {/* Cloud */}
          <section className={styles.panel}>
            <header className={styles.panelHeader}>
              <Cloud size={14} />
              <h2 className={styles.panelTitle}>inkuo Cloud</h2>
              {hasCloudAccount ? (
                <span className={styles.panelBadge}>
                  <Check size={10} /> 已登录
                </span>
              ) : (
                <span className={styles.panelHint}>按 token 用量计费</span>
              )}
            </header>

            {hasCloudAccount ? (
              <div className={styles.cloudAccount}>
                <div className={styles.accountAvatar} aria-hidden>
                  <User size={18} />
                </div>
                <div className={styles.accountMeta}>
                  <span className={styles.accountEmail}>
                    {settings.cloud.account?.email}
                  </span>
                  <span className={styles.accountServer}>
                    {planLabel} · 余额 ¥{balanceYuan}
                  </span>
                </div>
                <button
                  type="button"
                  className={styles.logoutBtn}
                  onClick={handleCloudLogout}
                  title="退出登录"
                >
                  <LogOut size={12} />
                  退出
                </button>
              </div>
            ) : (
              <>
                <div className={styles.modeSwitch}>
                  <button
                    type="button"
                    className={`${styles.modeBtn} ${authMode === 'login' ? styles.modeBtnActive : ''}`}
                    onClick={() => setAuthMode('login')}
                    aria-pressed={authMode === 'login'}
                  >
                    登录
                  </button>
                  <button
                    type="button"
                    className={`${styles.modeBtn} ${authMode === 'register' ? styles.modeBtnActive : ''}`}
                    onClick={() => setAuthMode('register')}
                    aria-pressed={authMode === 'register'}
                  >
                    注册
                  </button>
                </div>

                <div className={styles.emailField}>
                  <label className={styles.fieldLabel} htmlFor="welcome-email">
                    邮箱
                  </label>
                  <input
                    id="welcome-email"
                    className={styles.fieldInput}
                    type="email"
                    value={email}
                    onChange={(e) => setEmail(e.target.value)}
                    placeholder="you@example.com"
                    autoComplete="email"
                  />
                </div>

                <div className={styles.field}>
                  <label className={styles.fieldLabel} htmlFor="welcome-pwd">
                    密码
                  </label>
                  <input
                    id="welcome-pwd"
                    className={styles.fieldInput}
                    type="password"
                    value={password}
                    onChange={(e) => setPassword(e.target.value)}
                    placeholder="••••••••"
                    autoComplete={authMode === 'login' ? 'current-password' : 'new-password'}
                  />
                </div>

                {authMode === 'register' && (
                  <div className={styles.field}>
                    <label className={styles.fieldLabel} htmlFor="welcome-invite">
                      邀请码
                    </label>
                    <input
                      id="welcome-invite"
                      className={styles.fieldInput}
                      type="text"
                      value={inviteCode}
                      onChange={(e) => setInviteCode(e.target.value)}
                      placeholder="如 INKUO2026"
                    />
                  </div>
                )}

                {authError && (
                  <div className={styles.error}>
                    <AlertCircle size={12} />
                    <span>{authError}</span>
                  </div>
                )}

                <button
                  type="button"
                  className={styles.submitBtn}
                  onClick={handleCloudSubmit}
                  disabled={submitting}
                >
                  {submitting ? (
                    <>
                      <Loader2 size={12} className={styles.spinner} />
                      处理中...
                    </>
                  ) : justLoggedIn ? (
                    <>
                      <Check size={12} /> 已登录
                    </>
                  ) : authMode === 'login' ? (
                    '登录'
                  ) : (
                    '注册并登录'
                  )}
                </button>

                <p className={styles.cloudHint}>
                  由我们托管,邀请制注册 + 兑换码充值。
                </p>
              </>
            )}
          </section>
        </div>
      </main>

      <footer className={styles.footer}>
        <span>v0.x</span>
        <span className={styles.footerDot}>·</span>
        <span>
          <kbd>⌘</kbd>
          <kbd>O</kbd>
        </span>
        <span>快速打开文档文件夹</span>
      </footer>
    </div>
  );
};
