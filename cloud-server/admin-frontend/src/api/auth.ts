import { api } from './client';

export interface AdminUser {
  id: string;
  username: string;
  role: 'admin' | 'superadmin';
}

export interface LoginResponse {
  accessToken: string;
  expiresAt: string;
  admin: AdminUser;
}

export const authApi = {
  login: (username: string, password: string) =>
    api.post<LoginResponse>('/api/auth/login', { username, password }).then(r => r.data),
  me: () => api.get<AdminUser>('/api/auth/me').then(r => r.data),
  changePassword: (currentPassword: string, newPassword: string) =>
    api.post<{ message: string }>('/api/auth/change-password', { currentPassword, newPassword }).then(r => r.data),
  listAdmins: () => api.get('/api/auth/').then(r => r.data),
  createAdmin: (username: string, password: string, role: string) =>
    api.post('/api/auth/create', { username, password, role }).then(r => r.data),
  deleteAdmin: (id: string) => api.delete(`/api/auth/${id}`).then(r => r.data),
};