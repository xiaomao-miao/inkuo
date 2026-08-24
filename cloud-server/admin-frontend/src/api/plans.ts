import { api } from './client';

export interface Plan {
  id: string;
  name: string;
  monthlyPricePoints: number;
  monthlyTokenLimit: number;
  overageInputPricePer1k: number;
  overageOutputPricePer1k: number;
  enabled: boolean;
  createdAt: string;
  subscriberCount: number;
}

export const plansApi = {
  list: () => api.get<Plan[]>('/api/plans/').then(r => r.data),
  create: (data: Omit<Plan, 'id' | 'createdAt' | 'subscriberCount'>) =>
    api.post('/api/plans/', data).then(r => r.data),
  update: (id: string, data: Omit<Plan, 'id' | 'createdAt' | 'subscriberCount'>) =>
    api.put(`/api/plans/${id}`, data).then(r => r.data),
  delete: (id: string) => api.delete(`/api/plans/${id}`).then(r => r.data),
};
