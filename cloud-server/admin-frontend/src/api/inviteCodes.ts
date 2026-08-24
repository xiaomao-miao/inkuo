import { api } from './client';

export interface InviteCode {
  id: number;
  code: string;
  freePoints: number;
  maxUses: number;
  usedCount: number;
  expiresAt: string | null;
  createdAt: string;
  enabled: boolean;
}

export const inviteCodesApi = {
  list: (page = 1, pageSize = 20) =>
    api.get<{ total: number; page: number; pageSize: number; items: InviteCode[] }>('/api/invite-codes/', { params: { page, pageSize } }).then(r => r.data),
  create: (data: Omit<InviteCode, 'id' | 'usedCount' | 'createdAt'>) =>
    api.post('/api/invite-codes/', data).then(r => r.data),
  update: (id: number, data: Omit<InviteCode, 'id' | 'usedCount' | 'createdAt'>) =>
    api.put(`/api/invite-codes/${id}`, data).then(r => r.data),
  toggle: (id: number) => api.post(`/api/invite-codes/${id}/toggle`, {}).then(r => r.data),
  delete: (id: number) => api.delete(`/api/invite-codes/${id}`).then(r => r.data),
};
