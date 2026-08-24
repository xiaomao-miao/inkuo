import { api } from './client';

export type UsageType = 'chat' | 'search' | 'all';

export interface UsageRecord {
  id: string;
  usageType: Exclude<UsageType, 'all'>;
  userId: string;
  userEmail: string;
  modelConfigId: string | null;
  modelName: string | null;
  providerId: string | null;
  query: string | null;
  promptTokens: number;
  completionTokens: number;
  costPoints: number;
  reservedPoints: number | null;
  billingStatus: string;
  recordedAt: string;
}

export interface UsageListResponse {
  total: number;
  page: number;
  pageSize: number;
  totalCostPoints: number;
  totalTokens: number;
  chatRecords: number;
  webSearchRecords: number;
  chatCostPoints: number;
  webSearchCostPoints: number;
  items: UsageRecord[];
}

export const usageApi = {
  list: (params: {
    page?: number;
    pageSize?: number;
    userId?: string;
    modelId?: string;
    from?: string;
    to?: string;
    usageType?: UsageType;
  }) =>
    api.get<UsageListResponse>(
      '/api/usage/', { params }
    ).then(r => r.data),
};
