import { api } from './client';

/** Wire shape for a single web_search provider row, as exposed by the
 * cloud admin API. The API key is masked by default; pass
 * `includeKey=true` to `list()` to surface the real value (operators
 * only). */
export interface WebSearchProvider {
  id: string;
  providerId: string;
  displayName: string;
  upstreamBaseUrl: string | null;
  upstreamApiKeyMasked: string;
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
  list: (includeKey = false) =>
    api.get<WebSearchProvider[]>('/api/web-search-providers/', { params: { includeKey } }).then((r) => r.data),
  create: (data: WebSearchProviderUpsert) =>
    api.post('/api/web-search-providers/', data).then((r) => r.data),
  update: (id: string, data: Omit<WebSearchProviderUpsert, 'upstreamApiKey'> & { upstreamApiKey?: string | null }) =>
    api.put(`/api/web-search-providers/${id}`, data).then((r) => r.data),
  delete: (id: string) => api.delete(`/api/web-search-providers/${id}`).then((r) => r.data),
};
