import { describe, expect, it } from 'vitest';
import type { CloudAccount } from '../../types';
import { accountBalancePoints, formatPointsAsYuan } from './cloudMoney';

const account = (overrides: Partial<CloudAccount>): CloudAccount => ({
  base_url: 'https://cloud.example.test',
  email: 'reader@example.test',
  user_id: 'user-1',
  access_token: 'test-access-token',
  refresh_token: 'test-refresh-token',
  access_expires_at: '2030-01-01T00:00:00Z',
  plan_name: null,
  balance_points: 0,
  ...overrides,
});

describe('cloud point display', () => {
  it('preserves ¥0.001 instead of rounding it to a cent', () => {
    expect(formatPointsAsYuan(1)).toBe('¥0.001');
    expect(formatPointsAsYuan(12_345)).toBe('¥12.345');
  });

  it('prefers canonical points over a stale cents mirror', () => {
    expect(accountBalancePoints(account({ balance_points: 123, balance_cents: 0 }))).toBe(123);
  });

  it('migrates a legacy cents-only renderer snapshot', () => {
    const legacy = account({ balance_cents: 12.3 }) as unknown as
      Omit<CloudAccount, 'balance_points'> & { balance_points?: number };
    delete legacy.balance_points;
    expect(accountBalancePoints(legacy as CloudAccount)).toBe(123);
  });
});
