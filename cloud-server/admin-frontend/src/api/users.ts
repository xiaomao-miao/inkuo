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

export const usersApi = {
  list: (params: { page?: number; pageSize?: number; search?: string; sortBy?: string; sortDir?: string }) =>
    api.get<UserListResponse>('/api/users/', { params }).then(r => r.data),
  detail: (id: string) => api.get(`/api/users/${id}`).then(r => r.data),
  adjustBalance: (id: string, deltaPoints: number, reason: string) =>
    api.post(`/api/users/${id}/adjust-balance`, { deltaPoints, reason }).then(r => r.data),
  revokeSessions: (id: string) => api.post(`/api/users/${id}/revoke-sessions`, {}).then(r => r.data),
  delete: (id: string) => api.delete(`/api/users/${id}`).then(r => r.data),
};
