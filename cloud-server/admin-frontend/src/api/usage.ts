import { api } from './client';

export interface UsageRecord {
  id: string;
  userId: string;
  userEmail: string;
  modelConfigId: string;
  modelName: string;
  promptTokens: number;
  completionTokens: number;
  costCents: number;
  recordedAt: string;
}

export const usageApi = {
  list: (params: { page?: number; pageSize?: number; userId?: string; modelId?: string; from?: string; to?: string }) =>
    api.get<{ total: number; page: number; pageSize: number; totalCostCents: number; totalTokens: number; items: UsageRecord[] }>(
      '/api/usage/', { params }
    ).then(r => r.data),
};