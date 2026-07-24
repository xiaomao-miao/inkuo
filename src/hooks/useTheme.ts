import { useEffect } from 'react';
import { useSettingsStore } from '../store';
import type { ThemeType } from '../types/settings';

/** 主题 ID 列表 —— 与 design-tokens.css 中的 `[data-theme="..."]` 选择器一一对应。 */
export const THEME_IDS = [
  'paper-white',
  'paper-cream',
  'graphite',
  'verdant',
  'iris',
] as const;
export type ThemeId = (typeof THEME_IDS)[number];

/** 旧主题 ID 的别名映射(向后兼容旧的 settings 文件)。 */
const LEGACY_THEME_MAP: Record<string, ThemeType> = {
  'inkuo-dark': 'graphite',
  'inkuo-light': 'paper-white',
  'high-contrast-light': 'paper-white',
};

function resolveTheme(raw: string | undefined | null): ThemeType {
  if (!raw) return 'paper-white';
  const lower = raw.toLowerCase();
  return (LEGACY_THEME_MAP[lower] ?? lower) as ThemeType;
}

/**
 * 把 settings.theme 同步到 `<html data-theme="...">` 上。
 * 必须挂在 App 顶层。订阅设置变化,即时切换。
 */
export function useTheme() {
  const theme = useSettingsStore((s) => s.settings.theme);

  useEffect(() => {
    const resolved = resolveTheme(theme);
    document.documentElement.setAttribute('data-theme', resolved);
  }, [theme]);

  return { themeId: resolveTheme(theme) };
}