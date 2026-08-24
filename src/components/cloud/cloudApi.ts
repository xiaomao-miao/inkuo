import { invoke } from '@tauri-apps/api/core';
import type { CloudAccount, CloudModelEntry } from '../../types';

export interface CloudAccountInfo {
  id: string;
  email: string;
  /** Billing values remain integer points so sub-cent charges and debt stay exact. */
  balance_points: number;
  reserved_points: number;
  debt_points: number;
  is_suspended: boolean;
  plan_name: string | null;
  monthly_token_limit: number;
  subscription_expires_at: string | null;
  tokens_used_this_month: number;
  monthly_tokens_remaining: number;
}

/** Thin wrapper around the Rust-side `cloud_*` commands. The store calls
 * into here to talk to the in-process `CloudClient` rather than doing
 * HTTP directly from the renderer. */
export const cloudApi = {
  async register(
    baseUrl: string,
    inviteCode: string,
    email: string,
    password: string
  ): Promise<CloudAccount> {
    const account = await invoke<CloudAccount>('cloud_register', {
      baseUrl,
      inviteCode,
      email,
      password,
    });
    return normalizeCloudAccount(account);
  },

  async login(baseUrl: string, email: string, password: string): Promise<CloudAccount> {
    const account = await invoke<CloudAccount>('cloud_login', { baseUrl, email, password });
    return normalizeCloudAccount(account);
  },

  async logout(): Promise<void> {
    await invoke('cloud_logout');
  },

  async fetchModels(): Promise<CloudModelEntry[]> {
    return invoke<CloudModelEntry[]>('cloud_fetch_models');
  },

  async fetchAccount(): Promise<CloudAccountInfo> {
    const info = await invoke<CloudAccountInfo>('cloud_fetch_account');
    return {
      ...info,
      balance_points: requireSafePoints(info.balance_points, 'balance_points'),
      reserved_points: requireSafePoints(info.reserved_points, 'reserved_points'),
      debt_points: requireSafePoints(info.debt_points, 'debt_points'),
    };
  },

  /** Push the current in-process `CloudAccount` (post-login) back into
   * the persisted settings JSON so the Rust side reads fresh
   * credentials from disk on the next request. */
  async persistAccount(settings: unknown): Promise<void> {
    await invoke('cloud_persist_account', { settings });
  },
};

/** Normalize the one-release cents compatibility field returned by Rust.
 * Point values are deliberately rejected outside JavaScript's safe integer
 * range; silently rounding a user's money would be worse than surfacing an
 * actionable malformed-response error. */
export function normalizeCloudAccount(account: CloudAccount): CloudAccount {
  const raw = account as Omit<CloudAccount, 'balance_points' | 'balance_cents'> & {
    balance_points?: unknown;
    balance_cents?: unknown;
  };
  const balancePoints = raw.balance_points !== undefined
    ? requireSafePoints(raw.balance_points, 'balance_points')
    : typeof raw.balance_cents === 'number' && Number.isFinite(raw.balance_cents)
      ? Math.max(0, Math.round(raw.balance_cents * 10))
      : 0;

  return {
    ...account,
    base_url: account.base_url.replace(/\/+$/, ''),
    balance_points: balancePoints,
    reserved_points: optionalSafePoints(account.reserved_points),
    debt_points: optionalSafePoints(account.debt_points),
    is_suspended: account.is_suspended === true,
    // Keep older renderer surfaces working during the schema transition.
    balance_cents: balancePoints / 10,
  };
}

function optionalSafePoints(value: unknown): number {
  return Number.isSafeInteger(value) && Number(value) >= 0 ? Number(value) : 0;
}

function requireSafePoints(value: unknown, field: string): number {
  if (!Number.isSafeInteger(value) || Number(value) < 0) {
    throw new Error(`云端返回了无效的 ${field}，请刷新后重试。`);
  }
  return Number(value);
}
