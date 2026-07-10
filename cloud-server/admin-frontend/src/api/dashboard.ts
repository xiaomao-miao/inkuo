import { api } from './client';

export interface DashboardSummary {
  totalUsers: number;
  newUsersThisMonth: number;
  newUsersToday: number;
  activeSubscriptions: number;
  totalInviteCodes: number;
  usedInviteCodes: number;
  totalRedemptionCodes: number;
  usedRedemptionCodes: number;
  monthRevenueCents: number;
  totalRevenueCents: number;
  monthTokens: number;
}

export interface DailyUsagePoint {
  date: string;
  costCents: number;
  tokens: number;
  newUsers: number;
}

export interface PlanDistribution {
  planName: string;
  subscriptions: number;
}

export interface ModelUsageShare {
  modelName: string;
  tokens: number;
  costCents: number;
}

export const dashboardApi = {
  summary: () => api.get<DashboardSummary>('/api/dashboard/summary').then(r => r.data),
  usageTrend: () => api.get<DailyUsagePoint[]>('/api/dashboard/usage-trend').then(r => r.data),
  planDistribution: () => api.get<PlanDistribution[]>('/api/dashboard/plan-distribution').then(r => r.data),
  modelUsage: () => api.get<ModelUsageShare[]>('/api/dashboard/model-usage').then(r => r.data),
};