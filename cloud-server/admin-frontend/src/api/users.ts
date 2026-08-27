import { api } from './client';

export interface UserListItem {
  id: string;
  email: string;
  balancePoints: number;
  reservedPoints: number;
  debtPoints: number;
  isSuspended: boolean;
  createdAt: string;
  inviteCodeUsed: string;
  planName: string | null;
  subExpiresAt: string | null;
  totalTokens: number;
  totalCostPoints: number;
  subscriptionCount: number;
}

export interface UserListResponse {
  total: number;
  page: number;
  pageSize: number;
  items: UserListItem[];
}

export interface UserDetailResponse {
  user: {
    id: string;
    email: string;
    createdAt: string;
    inviteCodeUsed: string;
    balancePoints: number;
    reservedPoints: number;
    debtPoints: number;
    isSuspended: boolean;
  };
  subscriptions: Array<{
    id: string;
    planName: string;
    startedAt: string;
    expiresAt: string;
    status: string;
  }>;
  totalUsage: { tokens: number; costPoints: number; recordCount: number };
  recentUsage: Array<{
    id: string;
    modelName: string;
    promptTokens: number;
    completionTokens: number;
    costPoints: number;
    billingStatus: string;
    recordedAt: string;
  }>;
  refreshTokens: Array<{ jti: string; expiresAt: string; revoked: boolean }>;
}

export const usersApi = {
  list: (params: { page?: number; pageSize?: number; search?: string; sortBy?: string; sortDir?: string }) =>
    api.get<UserListResponse>('/api/users/', { params }).then(r => r.data),
  detail: (id: string) => api.get<UserDetailResponse>(`/api/users/${id}`).then(r => r.data),
  adjustBalance: (id: string, deltaPoints: number, reason: string) =>
    api.post(`/api/users/${id}/adjust-balance`, { deltaPoints, reason }).then(r => r.data),
  revokeSessions: (id: string) => api.post(`/api/users/${id}/revoke-sessions`, {}).then(r => r.data),
  delete: (id: string) => api.delete(`/api/users/${id}`).then(r => r.data),
};
