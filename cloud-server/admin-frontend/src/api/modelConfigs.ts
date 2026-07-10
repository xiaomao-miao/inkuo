import { api } from './client';

export interface ModelConfig {
  id: string;
  upstreamProvider: string;
  upstreamBaseUrl: string;
  upstreamApiKeyMasked: string;
  modelName: string;
  displayName: string;
  description: string | null;
  inputPricePerMTokens: number;
  outputPricePerMTokens: number;
  cachedInputPricePerMTokens: number;
  enabled: boolean;
  sortOrder: number;
  createdAt: string;
}

export interface ModelConfigUpsert {
  upstreamProvider: string;
  upstreamBaseUrl: string;
  upstreamApiKey: string;
  modelName: string;
  displayName: string;
  description?: string;
  inputPricePerMTokens: number;
  outputPricePerMTokens: number;
  cachedInputPricePerMTokens: number;
  enabled: boolean;
  sortOrder: number;
}

export const modelConfigsApi = {
  list: (includeKey = false) =>
    api.get<ModelConfig[]>('/api/model-configs/', { params: { includeKey } }).then(r => r.data),
  create: (data: ModelConfigUpsert) => api.post('/api/model-configs/', data).then(r => r.data),
  update: (id: string, data: Omit<ModelConfigUpsert, 'upstreamApiKey'> & { upstreamApiKey?: string }) =>
    api.put(`/api/model-configs/${id}`, data).then(r => r.data),
  delete: (id: string) => api.delete(`/api/model-configs/${id}`).then(r => r.data),
};