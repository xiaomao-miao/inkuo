import { invoke } from '@tauri-apps/api/core';
import type { CloudAccount, CloudModelEntry } from '../../types';

export interface CloudAccountInfo {
  id: string;
  email: string;
  balance_cents: number;
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
    return invoke<CloudAccount>('cloud_register', {
      baseUrl,
      inviteCode,
      email,
      password,
    });
  },

  async login(baseUrl: string, email: string, password: string): Promise<CloudAccount> {
    return invoke<CloudAccount>('cloud_login', { baseUrl, email, password });
  },

  async logout(): Promise<void> {
    await invoke('cloud_logout');
  },

  async fetchModels(): Promise<CloudModelEntry[]> {
    return invoke<CloudModelEntry[]>('cloud_fetch_models');
  },

  async fetchAccount(): Promise<CloudAccountInfo> {
    return invoke<CloudAccountInfo>('cloud_fetch_account');
  },

  /** Push the current in-process `CloudAccount` (post-login) back into
   * the persisted settings JSON so the Rust side reads fresh
   * credentials from disk on the next request. */
  async persistAccount(settings: unknown): Promise<void> {
    await invoke('cloud_persist_account', { settings });
  },
};