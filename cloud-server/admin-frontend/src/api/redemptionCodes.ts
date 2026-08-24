import { api } from './client';

export interface RedemptionCode {
  id: number;
  code: string;
  creditPoints: number;
  planId: string | null;
  planName: string | null;
  maxUses: number;
  usedCount: number;
  expiresAt: string | null;
  createdAt: string;
  enabled: boolean;
}

export const redemptionCodesApi = {
  list: (page = 1, pageSize = 20) =>
    api.get<{ total: number; page: number; pageSize: number; items: RedemptionCode[] }>('/api/redemption-codes/', { params: { page, pageSize } }).then(r => r.data),
  create: (data: Omit<RedemptionCode, 'id' | 'usedCount' | 'createdAt' | 'planName'>) =>
    api.post('/api/redemption-codes/', data).then(r => r.data),
  update: (id: number, data: Omit<RedemptionCode, 'id' | 'usedCount' | 'createdAt' | 'planName'>) =>
    api.put(`/api/redemption-codes/${id}`, data).then(r => r.data),
  toggle: (id: number) => api.post(`/api/redemption-codes/${id}/toggle`, {}).then(r => r.data),
  delete: (id: number) => api.delete(`/api/redemption-codes/${id}`).then(r => r.data),
};
