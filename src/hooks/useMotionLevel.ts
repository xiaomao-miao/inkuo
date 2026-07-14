import { useEffect, useState } from 'react';

/** 动效强度档位。 */
export const MOTION_LEVELS = ['standard', 'gentle', 'off'] as const;
export type MotionLevel = (typeof MOTION_LEVELS)[number];

const STORAGE_KEY = 'inkuo-motion-level';

/**
 * 读取用户偏好 + 系统级 `prefers-reduced-motion`,输出实际生效的
 * `data-motion` 值。Hook 不需要持久化业务 store,只需:
 *   - 把用户的选择写回 localStorage
 *   - 把选择 + 系统偏好的并集写到 `<html data-motion>`
 */
export function useMotionLevel(): {
  level: MotionLevel;
  setLevel: (l: MotionLevel) => void;
} {
  const [level, setLevelState] = useState<MotionLevel>(() => {
    if (typeof window === 'undefined') return 'standard';
    const stored = window.localStorage.getItem(STORAGE_KEY);
    return (MOTION_LEVELS as readonly string[]).includes(stored ?? '')
      ? (stored as MotionLevel)
      : 'standard';
  });

  // 系统级 prefers-reduced-motion 是只读信号,与用户档位取并集:
  // 系统说减少动效,无论用户怎么选都强制 off。
  const [systemReduce, setSystemReduce] = useState<boolean>(() => {
    if (typeof window === 'undefined') return false;
    return window.matchMedia?.('(prefers-reduced-motion: reduce)').matches ?? false;
  });

  useEffect(() => {
    if (typeof window === 'undefined') return;
    const mql = window.matchMedia('(prefers-reduced-motion: reduce)');
    const onChange = () => setSystemReduce(mql.matches);
    mql.addEventListener('change', onChange);
    return () => mql.removeEventListener('change', onChange);
  }, []);

  useEffect(() => {
    const effective: MotionLevel = systemReduce ? 'off' : level;
    document.documentElement.setAttribute('data-motion', effective);
  }, [level, systemReduce]);

  const setLevel = (next: MotionLevel) => {
    setLevelState(next);
    if (typeof window !== 'undefined') {
      window.localStorage.setItem(STORAGE_KEY, next);
    }
  };

  return { level, setLevel };
}

/**
 * Synchronous read of the current effective motion level.
 *
 * Returns `'off'` if either:
 *   - The user picked `off` / `gentle` (gentle still keeps transitions
 *     but short; we treat it as "still animated" so it stays `'gentle'`).
 *   - The OS reports `prefers-reduced-motion: reduce` — we deliberately
 *     honour that system signal even if the user picked a higher level.
 *
 * Used by `ChatInput`'s `useLayoutEffect` so the height-toggle animation
 * can branch *before* setting inline styles: when motion is off we skip
 * the pin-then-RAF dance entirely, otherwise a `transition-duration: 0`
 * forced by the global reduce-motion CSS would leave `el.style.height`
 * pinned forever (the `transitionend` listener never fires).
 *
 * Falls back to `'standard'` during SSR / pre-mount — the effect that
 * reads this only runs after the component is on the DOM, so the real
 * value is already there by the time it matters.
 */
export function getEffectiveMotionLevel(): MotionLevel {
  if (typeof window === 'undefined') return 'standard';
  const systemReduce =
    window.matchMedia?.('(prefers-reduced-motion: reduce)').matches ?? false;
  if (systemReduce) return 'off';
  const stored = window.localStorage.getItem(STORAGE_KEY);
  if ((MOTION_LEVELS as readonly string[]).includes(stored ?? '')) {
    return stored as MotionLevel;
  }
  return 'standard';
}