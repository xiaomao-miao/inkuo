import { api } from './client';

export interface UserListItem {
  id: string;
  email: string;
  balanceCents: number;
  createdAt: string;
  inviteCodeUsed: string;
  planName: string | null;
  subExpiresAt: string | null;
  totalTokens: number;
  totalCostCents: number;
  subscriptionCount: number;
  disabled: boolean;
}

export interface UserListResponse {
  total: number;
  page: number;
  pageSize: number;
  items: UserListItem[];
}

export const usersApi = {
  list: (params: { page?: number; pageSize?: number; search?: string; sortBy?: string; sortDir?: string }) =>
    api.get<UserListResponse>('/api/users/', { params }).then(r => r.data),
  detail: (id: string) => api.get(`/api/users/${id}`).then(r => r.data),
  adjustBalance: (id: string, deltaCents: number, reason: string) =>
    api.post(`/api/users/${id}/adjust-balance`, { deltaCents, reason }).then(r => r.data),
  revokeSessions: (id: string) => api.post(`/api/users/${id}/revoke-sessions`, {}).then(r => r.data),
  delete: (id: string) => api.delete(`/api/users/${id}`).then(r => r.data),
};