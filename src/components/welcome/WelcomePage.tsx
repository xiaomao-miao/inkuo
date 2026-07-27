import { useCallback, useEffect, useRef, useState } from 'react';
import {
  AlertCircle,
  ArrowRight,
  Check,
  Cloud,
  FileText,
  FolderOpen,
  LayoutGrid,
  Loader2,
  LogOut,
  MessageCircle,
  Moon,
  Palette,
  Plus,
  Search,
  Sparkles,
  Sun,
  SunMedium,
  User,
  WandSparkles,
  Wind,
  X,
  ZapOff,
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
import { getModifierKeyLabel } from '../../utils/platform';
import styles from './WelcomePage.module.css';

interface WelcomePageProps {
  onWorkspaceSelected?: () => void;
}

type AuthMode = 'login' | 'register';
type PreferencePanel = 'appearance' | 'motion' | 'cloud';

const TYPING_PHRASES = [
  '帮你整理一份会议纪要',
  '把这份报告改得更清楚',
  '从表格里找出关键结论',
  '把想法写成一份完整方案',
] as const;

const FILE_TYPES = [
  { label: 'Word', className: 'word' },
  { label: 'Excel', className: 'excel' },
  { label: 'PPT', className: 'ppt' },
  { label: 'Markdown', className: 'markdown' },
] as const;

const AI_STEPS = [
  { label: '读懂你的文件', icon: Search },
  { label: '找到需要的内容', icon: LayoutGrid },
  { label: '帮你完成修改', icon: WandSparkles },
] as const;

const PREFERENCE_PANELS = {
  appearance: {
    title: '界面风格',
    description: '选一个看着舒服的主题，选择会立即生效。',
    icon: Palette,
  },
  motion: {
    title: '动效节奏',
    description: '让界面的反馈更活泼、更温和，或完全静止。',
    icon: Sparkles,
  },
  cloud: {
    title: '云端账户',
    description: '需要时再登录，云端模型和账户额度会更方便管理。',
    icon: Cloud,
  },
} as const;

/** 内嵌主题预览,与 settings/AppearanceSettings 中保持一致。 */
const THEME_PREVIEWS = [
  {
    id: 'paper-white',
    label: '纸白',
    blurb: '干净明亮 · 内容优先',
    icon: <FileText size={12} />,
    swatches: {
      bg: '#fafaf9',
      surface: '#f5f5f4',
      elevated: '#ffffff',
      accent: '#3957c5',
      fg: '#1c1c1f',
    },
  },
  {
    id: 'paper-cream',
    label: '米黄',
    blurb: '温润纸感 · 长时间阅读',
    icon: <SunMedium size={12} />,
    swatches: {
      bg: '#f6f1e7',
      surface: '#efe7d6',
      elevated: '#fbf6e9',
      accent: '#2f55b8',
      fg: '#2b2620',
    },
  },
  {
    id: 'graphite',
    label: '石墨',
    blurb: '深色低饱和 · 专注写作',
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
    blurb: '沉静自然 · 清爽耐看',
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
    blurb: '灵感氛围 · 突出 AI',
    icon: <Sun size={12} />,
    swatches: {
      bg: '#16151d',
      surface: '#1d1c26',
      elevated: '#272634',
      accent: '#9b8afd',
      fg: '#ecebf2',
    },
  },
] as const;

const MOTION_PREVIEWS: Array<{
  id: MotionLevel;
  label: string;
  blurb: string;
  icon: React.ReactNode;
}> = [
  { id: 'standard', label: '标准', blurb: '自然流畅', icon: <Sparkles size={12} /> },
  { id: 'gentle', label: '温和', blurb: '慢且柔和', icon: <Wind size={12} /> },
  { id: 'off', label: '关闭', blurb: '瞬间完成', icon: <ZapOff size={12} /> },
];

function useTypewriter(enabled: boolean): string {
  const [phraseIndex, setPhraseIndex] = useState(0);
  const [visibleLength, setVisibleLength] = useState(
    enabled ? 0 : TYPING_PHRASES[0].length,
  );
  const [isDeleting, setIsDeleting] = useState(false);

  useEffect(() => {
    if (!enabled) {
      setPhraseIndex(0);
      setVisibleLength(TYPING_PHRASES[0].length);
      setIsDeleting(false);
      return;
    }

    const phrase = TYPING_PHRASES[phraseIndex];
    const isComplete = visibleLength === phrase.length;
    const isEmpty = visibleLength === 0;
    const delay = isComplete && !isDeleting ? 2200 : isEmpty ? 420 : isDeleting ? 42 : 72;

    const timer = window.setTimeout(() => {
      if (isComplete && !isDeleting) {
        setIsDeleting(true);
      } else if (isEmpty && isDeleting) {
        setPhraseIndex((current) => (current + 1) % TYPING_PHRASES.length);
        setIsDeleting(false);
      } else {
        setVisibleLength((current) => current + (isDeleting ? -1 : 1));
      }
    }, delay);

    return () => window.clearTimeout(timer);
  }, [enabled, isDeleting, phraseIndex, visibleLength]);

  return TYPING_PHRASES[phraseIndex].slice(0, visibleLength);
}

export const WelcomePage: React.FC<WelcomePageProps> = ({ onWorkspaceSelected }) => {
  const [isLoading, setIsLoading] = useState(false);
  const pushNotification = useNotificationStore((state) => state.pushNotification);
  const settings = useSettingsStore((s) => s.settings);
  const updateSetting = useSettingsStore((s) => s.updateSetting);
  const setCloudAccount = useSettingsStore((s) => s.setCloudAccount);
  const modifierKey = getModifierKeyLabel();

  const currentTheme = (settings.theme === 'inkuo-dark'
    ? 'graphite'
    : settings.theme === 'inkuo-light' || settings.theme === 'high-contrast-light'
      ? 'paper-white'
      : settings.theme === 'high-contrast-dark'
        ? 'graphite'
        : settings.theme) as (typeof THEME_PREVIEWS)[number]['id'];

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

  const animatedPhrase = useTypewriter(!reduceMotion && motionLevel !== 'off');

  const currentThemeLabel = THEME_PREVIEWS.find((theme) => theme.id === currentTheme)?.label ?? '纸白';
  const motionSummary = reduceMotion
    ? '系统已关闭'
    : MOTION_PREVIEWS.find((motion) => motion.id === motionLevel)?.label ?? '标准';

  // ── Cloud auth 内嵌态 ─────────────────────────────────────────────
  const hasCloudAccount = !!settings.cloud.account;
  const cloudBaseUrl = getCloudBaseUrl();
  const initialEmail = settings.cloud.account?.email ?? '';
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
  const [activePanel, setActivePanel] = useState<PreferencePanel | null>(null);
  const emailInputRef = useRef<HTMLInputElement | null>(null);
  const dialogRef = useRef<HTMLDivElement | null>(null);
  const lastTriggerRef = useRef<HTMLElement | null>(null);

  const handleSelectWorkspace = useCallback(async () => {
    setIsLoading(true);
    try {
      const selected = await open({
        directory: true,
        multiple: false,
        title: '选择工作区文件夹',
      });
      if (selected) {
        await switchWorkspace(selected);
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

  const handleThemeChange = (id: (typeof THEME_PREVIEWS)[number]['id']) => {
    void updateSetting('theme', id);
  };

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

  const openPanel = (panel: PreferencePanel, trigger?: HTMLElement | null) => {
    if (trigger) lastTriggerRef.current = trigger;
    setActivePanel(panel);
  };

  const closePanel = useCallback(() => {
    setActivePanel(null);
  }, []);

  useEffect(() => {
    if (!activePanel) return;
    const previousOverflow = document.body.style.overflow;
    document.body.style.overflow = 'hidden';
    const focusTimer = window.setTimeout(() => {
      if (activePanel === 'cloud' && !hasCloudAccount) {
        emailInputRef.current?.focus();
      } else {
        dialogRef.current?.focus();
      }
    }, 60);
    return () => {
      document.body.style.overflow = previousOverflow;
      window.clearTimeout(focusTimer);
    };
  }, [activePanel, hasCloudAccount]);

  useEffect(() => {
    if (!activePanel) return;
    const handleKey = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        event.preventDefault();
        closePanel();
      }
    };
    window.addEventListener('keydown', handleKey);
    return () => window.removeEventListener('keydown', handleKey);
  }, [activePanel, closePanel]);

  useEffect(() => {
    if (activePanel) return;
    lastTriggerRef.current?.focus();
  }, [activePanel]);

  return (
    <div className={styles.welcomePage}>
      <div className={styles.welcomePage__ambient} aria-hidden />
      <div className={styles.welcomePage__grain} aria-hidden />

      <main className={styles.shell}>
        <header className={styles.topbar}>
          <div className={styles.topbarBrand}>
            <Wordmark size={28} />
            <span className={styles.brandDivider} aria-hidden />
            <span className={styles.brandCaption}>AI 文档工作台</span>
          </div>
          <button
            type="button"
            className={styles.newWindowButton}
            onClick={handleNewWindow}
            title="在新窗口打开"
          >
            <Plus size={14} />
            <span>新窗口</span>
          </button>
        </header>

        <section className={styles.hero}>
          <div className={styles.heroCopy}>
            <div className={styles.eyebrow}>
              <span className={styles.eyebrowPulse} aria-hidden />
              <span>把时间留给真正重要的事</span>
            </div>
            <h1 className={styles.heroTitle}>
              让 AI 直接
              <span className={styles.heroTitleAccent}>帮你处理文档</span>
            </h1>
            <p className={styles.heroSubtitle}>
              打开一个文件夹，剩下的交给 InkUO。
              <br />
              <span className={styles.typeLine} aria-hidden="true">
                {animatedPhrase}
                <span className={styles.typingCaret} aria-hidden />
              </span>
              <span className={styles.srOnly}>
                InkUO 可以帮你整理会议纪要、修改报告、分析表格或写方案。
              </span>
            </p>

            <div className={styles.actions}>
              <button
                className={styles.primaryButton}
                onClick={handleSelectWorkspace}
                disabled={isLoading}
                type="button"
              >
                {isLoading ? <Loader2 size={17} className={styles.spinner} /> : <FolderOpen size={17} />}
                <span>{isLoading ? '正在打开...' : '打开文档文件夹'}</span>
                {!isLoading && <ArrowRight size={15} className={styles.arrowIcon} />}
                <span className={styles.kbdHint}>{modifierKey} O</span>
              </button>
              <p className={styles.actionHint}>无需学习 · 打开就会用</p>
            </div>
          </div>

          <div
            className={styles.heroVisual}
            role="img"
            aria-label="InkUO 正在帮助你处理文档的示意图"
          >
            <div className={styles.visualGlow} aria-hidden />
            <div className={styles.documentOrbit} aria-hidden>
              <span className={`${styles.orbitFile} ${styles.orbitFileWord}`}>W</span>
              <span className={`${styles.orbitFile} ${styles.orbitFileExcel}`}>×</span>
              <span className={`${styles.orbitFile} ${styles.orbitFilePpt}`}>P</span>
              <span className={`${styles.orbitFile} ${styles.orbitFileMd}`}>#</span>
            </div>
            <div className={styles.aiCard}>
              <div className={styles.aiCardHeader}>
                <div className={styles.aiAvatar}>
                  <WandSparkles size={15} />
                </div>
                <div>
                  <strong>InkUO AI</strong>
                  <span>随时准备帮忙</span>
                </div>
                <span className={styles.aiStatus} aria-label="在线" />
              </div>
              <div className={styles.aiPrompt}>
                <span className={styles.aiPromptQuote}>“</span>
                <span>帮我把这份报告整理得更清楚</span>
              </div>
              <div className={styles.aiResult}>
                <div className={styles.aiResultIcon}>
                  <Check size={14} />
                </div>
                <div className={styles.aiResultBody}>
                  <strong>好的，我来处理</strong>
                  <span>阅读 · 理解 · 修改，一步完成</span>
                </div>
                <ArrowRight size={14} className={styles.aiResultArrow} />
              </div>
              <div className={styles.aiCardFooter}>
                {AI_STEPS.map((step, index) => {
                  const Icon = step.icon;
                  return (
                    <span key={step.label} className={styles.aiStep}>
                      <Icon size={11} />
                      {index > 0 && <i aria-hidden>·</i>}
                      <span>{step.label}</span>
                    </span>
                  );
                })}
              </div>
            </div>
          </div>
        </section>

        <section className={styles.capabilities} aria-label="InkUO 可以帮你做什么">
          <div className={styles.capabilityIntro}>
            <span className={styles.sectionKicker}>一个文件夹，整个工作流</span>
            <h2>你说想做什么，InkUO 就去完成。</h2>
          </div>
          <div className={styles.capabilityList}>
            <div className={styles.capabilityItem}>
              <div className={`${styles.capabilityIcon} ${styles.capabilityIconBlue}`}><Search size={17} /></div>
              <div><strong>帮你找</strong><span>不必翻遍文件夹，直接问它。</span></div>
            </div>
            <div className={styles.capabilityItem}>
              <div className={`${styles.capabilityIcon} ${styles.capabilityIconPurple}`}><MessageCircle size={17} /></div>
              <div><strong>帮你写</strong><span>从一个想法，变成一份完整文档。</span></div>
            </div>
            <div className={styles.capabilityItem}>
              <div className={`${styles.capabilityIcon} ${styles.capabilityIconGreen}`}><WandSparkles size={17} /></div>
              <div><strong>帮你改</strong><span>润色、重排、补全，交给 AI。</span></div>
            </div>
          </div>
        </section>

        <section className={styles.fileTypes} aria-label="支持的文件类型">
          <span className={styles.fileTypesLabel}>支持你每天都在用的文件</span>
          <div className={styles.fileTypeList}>
            {FILE_TYPES.map((fileType) => (
              <span key={fileType.label} className={`${styles.fileType} ${styles[`fileType${fileType.className}`]}`}>
                <span className={styles.fileTypeMark} aria-hidden />
                {fileType.label}
              </span>
            ))}
          </div>
        </section>

        <section className={styles.preferencesRow} aria-label="偏好">
          <div className={styles.preferencesIntro}>
            <span className={styles.preferencesEyebrow}>偏好</span>
            <span className={styles.preferencesIntroHint}>需要时点开，不必现在设置</span>
          </div>
          <div className={styles.preferencesButtons}>
            {(Object.keys(PREFERENCE_PANELS) as PreferencePanel[]).map((panel) => {
              const meta = PREFERENCE_PANELS[panel];
              const Icon = meta.icon;
              const summary =
                panel === 'appearance'
                  ? currentThemeLabel
                  : panel === 'motion'
                    ? motionSummary
                    : hasCloudAccount
                      ? settings.cloud.account?.email ?? '已登录'
                      : '未登录';
              return (
                <button
                  key={panel}
                  ref={(node) => {
                    if (panel === 'appearance') lastTriggerRef.current = node;
                  }}
                  type="button"
                  className={styles.preferenceBtn}
                  onClick={(event) => openPanel(panel, event.currentTarget)}
                  aria-haspopup="dialog"
                  aria-expanded={activePanel === panel}
                >
                  <span className={styles.preferenceBtnIcon} aria-hidden>
                    <Icon size={15} />
                  </span>
                  <span className={styles.preferenceBtnBody}>
                    <span className={styles.preferenceBtnLabel}>{meta.title}</span>
                    <span className={styles.preferenceBtnSummary}>{summary}</span>
                  </span>
                </button>
              );
            })}
          </div>
          {!hasCloudAccount && (
            <p className={styles.cloudRegisterHint} role="note">
              <Sparkles size={11} aria-hidden />
              <span>登录后可同步 AI 额度与设置 · 注册与登录需要邀请码</span>
            </p>
          )}
        </section>
      </main>

      {activePanel && (
        <div
          className={styles.modalBackdrop}
          onClick={(event) => {
            if (event.target === event.currentTarget) closePanel();
          }}
          role="presentation"
        >
          <div
            ref={dialogRef}
            className={styles.modalDialog}
            role="dialog"
            aria-modal="true"
            aria-labelledby={`preference-${activePanel}-title`}
            tabIndex={-1}
          >
            <header className={styles.modalHeader}>
              <div className={styles.modalHeaderCopy}>
                <span className={styles.modalHeaderKicker}>
                  {(() => {
                    const Icon = PREFERENCE_PANELS[activePanel].icon;
                    return <Icon size={12} aria-hidden />;
                  })()}
                  <span>偏好设置</span>
                </span>
                <h2 id={`preference-${activePanel}-title`} className={styles.modalTitle}>
                  {PREFERENCE_PANELS[activePanel].title}
                </h2>
                <p className={styles.modalDescription}>
                  {PREFERENCE_PANELS[activePanel].description}
                </p>
              </div>
              <button
                type="button"
                className={styles.modalClose}
                onClick={closePanel}
                aria-label="关闭偏好设置"
              >
                <X size={15} />
              </button>
            </header>

            <div className={styles.modalBody}>
              {activePanel === 'appearance' && (
                <div className={styles.themeGrid}>
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
                        <div className={styles.themePreview} style={{ background: theme.swatches.bg }}>
                          <div className={styles.previewRow}>
                            <span className={styles.previewDot} style={{ background: theme.swatches.accent }} />
                            <span className={styles.previewLineBar} style={{ background: theme.swatches.fg, width: '60%' }} />
                          </div>
                          <div className={styles.previewCard} style={{ background: theme.swatches.surface }}>
                            <div className={styles.previewBar} style={{ background: theme.swatches.elevated, width: '78%' }} />
                            <div className={styles.previewBar} style={{ background: theme.swatches.elevated, width: '48%' }} />
                            <div className={styles.previewChip} style={{ background: theme.swatches.accent }} />
                          </div>
                        </div>
                        <span className={styles.themeLabel}>{theme.icon}{theme.label}</span>
                        {active && <span className={styles.themeCheck} aria-hidden><Check size={10} /></span>}
                      </button>
                    );
                  })}
                </div>
              )}

              {activePanel === 'motion' && (
                <div className={styles.motionCard}>
                  <div className={styles.motionRow}>
                    <div className={styles.motionTabs}>
                      {MOTION_PREVIEWS.map((motion) => {
                        const active = motionLevel === motion.id;
                        const blocked = reduceMotion && motion.id !== 'off';
                        return (
                          <button
                            key={motion.id}
                            type="button"
                            className={`${styles.motionTab} ${active ? styles.motionTabActive : ''}`}
                            onClick={() => setMotionLevel(motion.id)}
                            aria-pressed={active}
                            disabled={blocked}
                            title={blocked ? '系统开启了「减少动效」,已强制为关闭' : `${motion.label} · ${motion.blurb}`}
                          >
                            {motion.icon}<span>{motion.label}</span>
                          </button>
                        );
                      })}
                    </div>
                  </div>
                  <p className={styles.motionHint}>
                    当前节奏：
                    <strong>
                      {reduceMotion
                        ? '系统已开启「减少动效」，所有动画停止'
                        : MOTION_PREVIEWS.find((motion) => motion.id === motionLevel)?.label ?? '标准'}
                    </strong>
                    。关闭动效可让界面切换更加迅速。
                  </p>
                </div>
              )}

              {activePanel === 'cloud' && (
                <div className={styles.cloudCard}>
                  {hasCloudAccount ? (
                    <div className={styles.cloudAccount}>
                      <div className={styles.accountAvatar} aria-hidden><User size={18} /></div>
                      <div className={styles.accountMeta}>
                        <span className={styles.accountEmail}>{settings.cloud.account?.email}</span>
                        <span className={styles.accountServer}>{planLabel} · 余额 ¥{balanceYuan}</span>
                      </div>
                      <button type="button" className={styles.logoutBtn} onClick={handleCloudLogout} title="退出登录">
                        <LogOut size={12} />退出
                      </button>
                    </div>
                  ) : (
                    <>
                      <p className={styles.cloudDescription}>登录后可同步 AI 额度与设置，注册需要邀请码。</p>
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
                      <div className={styles.field}>
                        <label className={styles.fieldLabel} htmlFor="preference-cloud-email">邮箱</label>
                        <input
                          id="preference-cloud-email"
                          ref={emailInputRef}
                          className={styles.fieldInput}
                          type="email"
                          value={email}
                          onChange={(e) => setEmail(e.target.value)}
                          placeholder="you@example.com"
                          autoComplete="email"
                        />
                      </div>
                      <div className={styles.field}>
                        <label className={styles.fieldLabel} htmlFor="preference-cloud-password">密码</label>
                        <input
                          id="preference-cloud-password"
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
                          <label className={styles.fieldLabel} htmlFor="preference-cloud-invite">邀请码</label>
                          <input
                            id="preference-cloud-invite"
                            className={styles.fieldInput}
                            type="text"
                            value={inviteCode}
                            onChange={(e) => setInviteCode(e.target.value)}
                            placeholder="请输入邀请码"
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
                            <Loader2 size={12} className={styles.spinner} />处理中...
                          </>
                        ) : justLoggedIn ? (
                          <>
                            <Check size={12} />已登录
                          </>
                        ) : authMode === 'login' ? (
                          '登录'
                        ) : (
                          '注册并登录'
                        )}
                      </button>
                      <p className={styles.cloudHint}>云端服务为可选功能，不影响本地使用。</p>
                    </>
                  )}
                </div>
              )}
            </div>
          </div>
        </div>
      )}

      <footer className={styles.footer}>
        <span>InkUO</span>
        <span className={styles.footerDot}>·</span>
        <span><kbd>{modifierKey}</kbd><kbd>O</kbd> 打开文件夹</span>
        <span className={styles.footerDot}>·</span>
        <span>你的文件，始终在你的电脑里</span>
      </footer>
    </div>
  );
};
