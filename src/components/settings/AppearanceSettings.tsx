import { useEffect, useState } from 'react';
import { Palette, Sparkles, Moon, Sun, ZapOff, Wind, SunMedium } from 'lucide-react';
import { useSettingsStore } from '../../store';
import { MOTION_LEVELS, type MotionLevel } from '../../hooks/useMotionLevel';
import styles from './SettingsPanel.module.css';
import appearanceStyles from './AppearanceSettings.module.css';

interface ThemeSpec {
  id: 'graphite' | 'verdant' | 'iris' | 'inkuo-light';
  label: string;
  blurb: string;
  icon: React.ReactNode;
  preview: {
    bg: string;
    surface: string;
    elevated: string;
    accent: string;
    fg: string;
  };
}

const THEMES: ThemeSpec[] = [
  {
    id: 'graphite',
    label: '石墨',
    blurb: '深色低饱和 · 长时间写作',
    icon: <Moon size={14} />,
    preview: {
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
    icon: <Sparkles size={14} />,
    preview: {
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
    icon: <Sun size={14} />,
    preview: {
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
    icon: <SunMedium size={14} />,
    preview: {
      bg: '#fbfbfa',
      surface: '#f4f4f1',
      elevated: '#ebebe7',
      accent: '#7c5cff',
      fg: '#2a2a2c',
    },
  },
];

interface MotionSpec {
  id: MotionLevel;
  label: string;
  blurb: string;
  icon: React.ReactNode;
}

const MOTION_SPECS: MotionSpec[] = [
  { id: 'standard', label: '标准',  blurb: '120–320ms,带 spring 弹跳', icon: <Sparkles size={14} /> },
  { id: 'gentle',   label: '温和',  blurb: '慢 50%,无弹跳',             icon: <Wind size={14} /> },
  { id: 'off',      label: '关闭',  blurb: '所有过渡瞬间完成',           icon: <ZapOff size={14} /> },
];

export const AppearanceSettings: React.FC = () => {
  const settings = useSettingsStore((s) => s.settings);
  const updateSettingAndPersist = useSettingsStore((s) => s.updateSettingAndPersist);

  // 动效档位存在 localStorage,不进 settings store
  const [motion, setMotion] = useState<MotionLevel>(() => {
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
    window.localStorage.setItem('inkuo-motion-level', motion);
    const effective = reduceMotion ? 'off' : motion;
    document.documentElement.setAttribute('data-motion', effective);
  }, [motion, reduceMotion]);

  const handleThemeChange = (id: ThemeSpec['id']) => {
    void updateSettingAndPersist('theme', id);
  };

  return (
    <div className={styles.tabContent}>
      {/* ── 主题 ───────────────────────────────────────────────── */}
      <div className={styles.section}>
        <h4 className={styles.sectionTitle}>
          <Palette size={14} />
          主题
        </h4>
        <p className={styles.sectionDescription}>
          选择整套配色。选择会立即生效并持久化。
        </p>

        <div className={appearanceStyles.themeGrid}>
          {THEMES.map((theme) => {
            const active = settings.theme === theme.id ||
              (theme.id === 'graphite' && settings.theme === 'inkuo-dark');
            return (
              <button
                key={theme.id}
                type="button"
                className={`${appearanceStyles.themeCard} ${active ? appearanceStyles.themeCardActive : ''}`}
                onClick={() => handleThemeChange(theme.id)}
                aria-pressed={active}
              >
                <div
                  className={appearanceStyles.themePreview}
                  style={{ background: theme.preview.bg }}
                >
                  {/* 一个微型 UI 截图,让对比更直观 */}
                  <div
                    className={appearanceStyles.previewSurface}
                    style={{ background: theme.preview.surface }}
                  >
                    <div
                      className={appearanceStyles.previewLine}
                      style={{ background: theme.preview.elevated, width: '70%' }}
                    />
                    <div
                      className={appearanceStyles.previewLine}
                      style={{ background: theme.preview.elevated, width: '55%' }}
                    />
                    <div
                      className={appearanceStyles.previewAccent}
                      style={{ background: theme.preview.accent }}
                    />
                  </div>
                  <div
                    className={appearanceStyles.previewFg}
                    style={{ background: theme.preview.fg }}
                  />
                </div>
                <div className={appearanceStyles.themeMeta}>
                  <span className={appearanceStyles.themeName}>
                    {theme.icon}
                    {theme.label}
                  </span>
                  <span className={appearanceStyles.themeBlurb}>{theme.blurb}</span>
                </div>
              </button>
            );
          })}
        </div>
      </div>

      {/* ── 动效 ───────────────────────────────────────────────── */}
      <div className={styles.section}>
        <h4 className={styles.sectionTitle}>
          <Sparkles size={14} />
          动效强度
        </h4>
        <p className={styles.sectionDescription}>
          控制面板切换、菜单弹出、按钮 hover 等所有过渡的强度。
          {reduceMotion && (
            <>
              {' '}
              <span className={appearanceStyles.systemFlag}>
                系统开启了「减少动效」,已强制为关闭。
              </span>
            </>
          )}
        </p>

        <div className={appearanceStyles.motionGrid}>
          {MOTION_SPECS.map((m) => {
            const active = motion === m.id;
            const blocked = reduceMotion && m.id !== 'off';
            return (
              <button
                key={m.id}
                type="button"
                className={`${appearanceStyles.motionCard} ${active ? appearanceStyles.motionCardActive : ''}`}
                onClick={() => setMotion(m.id)}
                aria-pressed={active}
                disabled={blocked}
                title={blocked ? '系统已强制覆盖' : undefined}
              >
                <span className={appearanceStyles.motionName}>
                  {m.icon}
                  {m.label}
                </span>
                <span className={appearanceStyles.motionBlurb}>{m.blurb}</span>
                {/* 一个迷你动画示意,3 条横线按档位错开 */}
                <div className={appearanceStyles.motionDemo} data-active={active}>
                  <span style={{ background: 'var(--accent)', animationDelay: '0ms' }} />
                  <span style={{ background: 'var(--accent)', animationDelay: '120ms' }} />
                  <span style={{ background: 'var(--accent)', animationDelay: '240ms' }} />
                </div>
              </button>
            );
          })}
        </div>
      </div>
    </div>
  );
};