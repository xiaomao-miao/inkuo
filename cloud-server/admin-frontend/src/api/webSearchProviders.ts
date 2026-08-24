import { api } from './client';

/** Wire shape for a single web_search provider row. Provider credentials
 * are write-only: the API only tells the UI whether a key is configured. */
export interface WebSearchProvider {
  id: string;
  providerId: string;
  displayName: string;
  upstreamBaseUrl: string | null;
  hasUpstreamApiKey: boolean;
  enabled: boolean;
  createdAt: string;
}

export interface WebSearchProviderUpsert {
  providerId: string;
  displayName: string;
  upstreamBaseUrl?: string | null;
  upstreamApiKey?: string | null;
  enabled: boolean;
}

export const webSearchProvidersApi = {
  list: () => api.get<WebSearchProvider[]>('/api/web-search-providers/').then((r) => r.data),
  create: (data: WebSearchProviderUpsert) =>
    api.post('/api/web-search-providers/', data).then((r) => r.data),
  update: (id: string, data: Omit<WebSearchProviderUpsert, 'upstreamApiKey'> & { upstreamApiKey?: string | null }) =>
    api.put(`/api/web-search-providers/${id}`, data).then((r) => r.data),
  delete: (id: string) => api.delete(`/api/web-search-providers/${id}`).then((r) => r.data),
};
