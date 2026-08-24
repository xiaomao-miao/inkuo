import type { CloudAccount } from '../../types';

/** Resolve old cents-only settings without sacrificing point precision for
 * current accounts. Rust also performs this migration, but this guard keeps
 * the renderer safe when Zustand rehydrates a legacy browser snapshot. */
export function accountBalancePoints(account: CloudAccount): number {
  if (Number.isSafeInteger(account.balance_points) && account.balance_points >= 0) {
    return account.balance_points;
  }
  if (typeof account.balance_cents === 'number' && Number.isFinite(account.balance_cents)) {
    return Math.max(0, Math.round(account.balance_cents * 10));
  }
  return 0;
}

export function formatPointsAsYuan(points: number): string {
  const safePoints = Number.isSafeInteger(points) ? points : 0;
  return `¥${(safePoints / 1000).toFixed(3)}`;
}
